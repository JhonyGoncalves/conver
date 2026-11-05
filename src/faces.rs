//! Desfocar rostos em fotos e vídeos, sem internet: o detector YuNet (OpenCV, MIT, 228 KB) acha os rostos e o
//! Conver desfoca, pixeliza ou cobre cada um. Nos vídeos, os quadros passam por um "cano" do ffmpeg (decodifica →
//! Conver → codifica) e o áudio é copiado.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};

use image::{Rgb, RgbImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

use crate::ai::{self, ModelSpec};
use crate::{tools, video};

pub const MODEL: ModelSpec = ModelSpec {
    file: "face_detection_yunet_2023mar.onnx",
    url: "https://huggingface.co/opencv/face_detection_yunet/resolve/3cc26e7f1014a5ee5d74a42acee58bafc9d0a310/face_detection_yunet_2023mar.onnx",
    sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
    size: 232_589,
    label: "YuNet (228 KB)",
};

const SIDE: usize = 640;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Desfoque,
    Pixelado,
    Tarja,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub score: f32,
}

impl FaceBox {
    fn iou(&self, o: &FaceBox) -> f32 {
        let (x1, y1) = (self.x.max(o.x), self.y.max(o.y));
        let (x2, y2) = ((self.x + self.w).min(o.x + o.w), (self.y + self.h).min(o.y + o.h));
        let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
        inter / (self.w * self.h + o.w * o.h - inter).max(1e-6)
    }
}

fn model() -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(&MODEL, &|_| {}, &|| false)?;
    let s = ai::session(&path)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

/// Rostos da imagem (em pixels dela), do mais confiável para o menos. `min_score` de 0 a 1 (0,6 é o padrão do OpenCV).
pub fn detect(img: &RgbImage, min_score: f32) -> Result<Vec<FaceBox>, String> {
    let session = model()?;
    let (w, h) = img.dimensions();
    // cabe a imagem em 640×640 sem distorcer; o resto fica preto (embaixo e à direita)
    let scale = (SIDE as f32 / w as f32).min(SIDE as f32 / h as f32);
    let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    let small = image::imageops::resize(img, nw, nh, FilterType::Triangle);
    // o YuNet foi treinado em BGR, valores de 0 a 255
    let mut data = vec![0f32; 3 * SIDE * SIDE];
    for (x, y, p) in small.enumerate_pixels() {
        let i = y as usize * SIDE + x as usize;
        data[i] = p[2] as f32;
        data[SIDE * SIDE + i] = p[1] as f32;
        data[2 * SIDE * SIDE + i] = p[0] as f32;
    }
    let t = Tensor::from_array(([1usize, 3, SIDE, SIDE], data)).map_err(|e| e.to_string())?;
    let mut s = session.lock().unwrap();
    let out = s.run(ort::inputs!["input" => t]).map_err(|e| format!("falha na IA: {e}"))?;
    let mut faces = Vec::new();
    for stride in [8usize, 16, 32] {
        let get = |name: String| -> Result<Vec<f32>, String> {
            let v = out.get(name.as_str()).ok_or_else(|| format!("saída {name} ausente"))?;
            Ok(v.try_extract_tensor::<f32>().map_err(|e| e.to_string())?.1.to_vec())
        };
        let (cls, obj, bbox) = (get(format!("cls_{stride}"))?, get(format!("obj_{stride}"))?, get(format!("bbox_{stride}"))?);
        let cols = SIDE / stride;
        for i in 0..cls.len() {
            let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
            if score < min_score {
                continue;
            }
            let (r, c) = ((i / cols) as f32, (i % cols) as f32);
            let st = stride as f32;
            let b = &bbox[i * 4..i * 4 + 4];
            let (cx, cy) = ((c + b[0]) * st, (r + b[1]) * st);
            let (bw, bh) = (b[2].exp() * st, b[3].exp() * st);
            faces.push(FaceBox { x: (cx - bw / 2.0) / scale, y: (cy - bh / 2.0) / scale, w: bw / scale, h: bh / scale, score });
        }
    }
    // NMS: entre caixas muito sobrepostas, fica a de maior nota
    faces.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<FaceBox> = Vec::new();
    for f in faces {
        if keep.iter().all(|k| k.iou(&f) < 0.3) {
            keep.push(f);
        }
    }
    Ok(keep)
}

/// Desfoque "caixa" separável, três vezes (fica parecido com o gaussiano), só dentro do retângulo.
fn box_blur(img: &mut RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, r: u32) {
    let (w, h) = (x1 - x0, y1 - y0);
    if w < 2 || h < 2 || r == 0 {
        return;
    }
    let mut buf: Vec<[f32; 3]> = (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| {
        let p = img.get_pixel(x, y);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    }).collect();
    let idx = |x: u32, y: u32| (y * w + x) as usize;
    for _ in 0..3 {
        for horizontal in [true, false] {
            let (len, lines) = if horizontal { (w, h) } else { (h, w) };
            let mut tmp = vec![[0f32; 3]; len as usize];
            for line in 0..lines {
                let at = |k: u32| if horizontal { idx(k, line) } else { idx(line, k) };
                // janela deslizante de 2r+1, com as bordas repetidas
                let mut acc = [0f32; 3];
                let n = (2 * r + 1) as f32;
                for k in 0..=(2 * r) {
                    let j = (k as i64 - r as i64).clamp(0, len as i64 - 1) as u32;
                    for c in 0..3 {
                        acc[c] += buf[at(j)][c];
                    }
                }
                for k in 0..len {
                    tmp[k as usize] = [acc[0] / n, acc[1] / n, acc[2] / n];
                    let out = (k as i64 - r as i64).clamp(0, len as i64 - 1) as u32;
                    let inn = (k as i64 + r as i64 + 1).clamp(0, len as i64 - 1) as u32;
                    for c in 0..3 {
                        acc[c] += buf[at(inn)][c] - buf[at(out)][c];
                    }
                }
                for k in 0..len {
                    buf[at(k)] = tmp[k as usize];
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let v = buf[idx(x, y)];
            img.put_pixel(x0 + x, y0 + y, Rgb([v[0] as u8, v[1] as u8, v[2] as u8]));
        }
    }
}

/// Esconde os rostos na imagem. `expand`: quanto a área cresce em volta da caixa (0,25 = 25%, pega cabelo e queixo).
pub fn hide(img: &mut RgbImage, faces: &[FaceBox], style: Style, expand: f32) {
    let (w, h) = img.dimensions();
    for f in faces {
        let (ex, ey) = (f.w * expand / 2.0, f.h * expand / 2.0);
        let x0 = (f.x - ex).max(0.0) as u32;
        let y0 = (f.y - ey).max(0.0) as u32;
        let x1 = ((f.x + f.w + ex).ceil() as u32).min(w);
        let y1 = ((f.y + f.h + ey).ceil() as u32).min(h);
        if x1 <= x0 + 1 || y1 <= y0 + 1 {
            continue;
        }
        let side = (x1 - x0).max(y1 - y0);
        match style {
            Style::Tarja => {
                for y in y0..y1 {
                    for x in x0..x1 {
                        img.put_pixel(x, y, Rgb([0, 0, 0]));
                    }
                }
            }
            Style::Pixelado => {
                let block = (side / 8).max(4);
                let mut by = y0;
                while by < y1 {
                    let mut bx = x0;
                    while bx < x1 {
                        let (ex, ey) = ((bx + block).min(x1), (by + block).min(y1));
                        let mut acc = [0u64; 3];
                        let n = ((ex - bx) * (ey - by)) as u64;
                        for y in by..ey {
                            for x in bx..ex {
                                let p = img.get_pixel(x, y);
                                for c in 0..3 {
                                    acc[c] += p[c] as u64;
                                }
                            }
                        }
                        let px = Rgb([(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8]);
                        for y in by..ey {
                            for x in bx..ex {
                                img.put_pixel(x, y, px);
                            }
                        }
                        bx = ex;
                    }
                    by += block;
                }
            }
            Style::Desfoque => box_blur(img, x0, y0, x1, y1, (side / 9).max(3)),
        }
    }
}

/// Foto: acha e esconde os rostos.
pub fn process_image(src: &Path, out_dir: Option<&Path>, style: Style, min_score: f32) -> Result<(String, PathBuf), String> {
    let mut img = image::open(src).map_err(|e| format!("não foi possível abrir a imagem: {e}"))?.to_rgb8();
    let faces = detect(&img, min_score)?;
    hide(&mut img, &faces, style, 0.25);
    let ext = match tools::ext_lower(src).as_str() {
        "jpg" | "jpeg" | "jfif" => "jpg",
        "webp" => "webp",
        "bmp" => "bmp",
        _ => "png",
    };
    let dst = tools::out_path(src, out_dir, "_rostos", ext);
    if ext == "jpg" {
        let f = std::fs::File::create(&dst).map_err(|e| e.to_string())?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 92).encode_image(&img).map_err(|e| e.to_string())?;
    } else {
        img.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    }
    Ok((format!("{} rosto(s)", faces.len()), dst))
}

/// Vídeo: cada quadro passa pelo detector. Um rosto que some por poucos quadros (de lado, desfocado pelo
/// movimento) continua escondido por mais ~0,3 s, para o desfoque não "piscar".
pub fn process_video(src: &Path, out_dir: Option<&Path>, style: Style, min_score: f32, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let info = video::probe(src);
    let (w, h) = info.size.ok_or("o arquivo não tem vídeo.")?;
    let (w, h) = (w & !1, h & !1);
    let fps = video_fps(src).unwrap_or(30.0);
    let total = info.duration.map(|d| (d * fps) as u64).unwrap_or(0);
    model()?;
    let mut dec = tools::command(&ff)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(src)
        .args(["-vf", &format!("crop={w}:{h}:0:0"), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    let ext = crate::media::keep_or_mp4(src);
    let dst = tools::out_path(src, out_dir, "_rostos", &ext);
    let mut enc = tools::command(&ff)
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", &format!("{w}x{h}"), "-r", &format!("{fps:.3}"), "-i", "-", "-i"])
        .arg(src)
        .args(["-map", "0:v", "-map", "1:a?", "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "copy", "-shortest"])
        .arg(&dst)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    let frame_len = (w * h * 3) as usize;
    let mut buf = vec![0u8; frame_len];
    let (mut input, mut output) = (dec.stdout.take().unwrap(), enc.stdin.take().unwrap());
    let hold = (fps * 0.3).round().max(1.0) as u32;
    let mut recent: Vec<(FaceBox, u32)> = Vec::new();
    let (mut n, mut most) = (0u64, 0usize);
    let result: Result<(), String> = (|| {
        loop {
            if cancelled() {
                return Err("Cancelado.".into());
            }
            if input.read_exact(&mut buf).is_err() {
                break;
            }
            let mut img = RgbImage::from_raw(w, h, std::mem::take(&mut buf)).ok_or("quadro inválido")?;
            let found = detect(&img, min_score)?;
            most = most.max(found.len());
            // os de agora + os que sumiram há pouco (e não se sobrepõem a um de agora)
            recent.retain_mut(|(b, age)| {
                *age += 1;
                *age <= hold && found.iter().all(|f| f.iou(b) < 0.3)
            });
            let mut boxes = found.clone();
            boxes.extend(recent.iter().map(|(b, _)| *b));
            recent.extend(found.into_iter().map(|b| (b, 0)));
            hide(&mut img, &boxes, style, 0.25);
            output.write_all(img.as_raw()).map_err(|_| "o ffmpeg parou de receber os quadros.".to_string())?;
            buf = img.into_raw();
            n += 1;
            if total > 0 && n % 5 == 0 {
                on_progress((n as f32 / total as f32).min(0.99));
            }
        }
        Ok(())
    })();
    drop(output);
    let _ = dec.kill();
    let _ = dec.wait();
    let mut err = String::new();
    if let Some(mut e) = enc.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let status = enc.wait().map_err(|e| e.to_string())?;
    if let Err(e) = result {
        let _ = std::fs::remove_file(&dst);
        return Err(e);
    }
    if !status.success() || n == 0 {
        let _ = std::fs::remove_file(&dst);
        return Err(format!("falha no ffmpeg: {}", err.lines().last().unwrap_or("nenhum quadro")));
    }
    on_progress(1.0);
    Ok((format!("{n} quadros • até {most} rosto(s) por quadro"), dst))
}

/// Quadros por segundo do vídeo (da linha "fps" do ffmpeg).
fn video_fps(src: &Path) -> Option<f64> {
    let ff = tools::find_tool("ffmpeg")?;
    let out = tools::command(&ff).args(["-hide_banner", "-i"]).arg(src).output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text.lines().find(|l| l.contains("Video:"))?;
    let part = line.split(',').find(|p| p.trim_end().ends_with(" fps"))?;
    part.trim().trim_end_matches(" fps").trim().parse().ok()
}

/// Foto ou vídeo, conforme a extensão.
pub fn process(src: &Path, out_dir: Option<&Path>, style: Style, min_score: f32, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let ext = tools::ext_lower(src);
    if video::VIDEO_EXTS.contains(&ext.as_str()) && ext != "gif" {
        process_video(src, out_dir, style, min_score, on_progress, cancelled)
    } else {
        process_image(src, out_dir, style, min_score)
    }
}
