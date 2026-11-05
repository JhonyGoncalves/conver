//! Marca d'água em lote: texto ou logo, em imagens, vídeos e PDFs.
//! Imagens e vídeos passam por um filtro do ffmpeg (drawtext / overlay); nos PDFs, o texto ou o logo é desenhado em
//! cada página com o lopdf, com transparência de verdade (ExtGState) e sem mexer no conteúdo original.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, Stream, dictionary};

use crate::media::{encode_to, keep_or_mp4, path_s};
use crate::video::s;
use crate::{imaging, tools, video};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pos {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    /// Atravessada no meio, na diagonal.
    Diagonal,
}

impl Pos {
    pub const GRID: [[Pos; 3]; 3] = [
        [Pos::TopLeft, Pos::Top, Pos::TopRight],
        [Pos::Left, Pos::Center, Pos::Right],
        [Pos::BottomLeft, Pos::Bottom, Pos::BottomRight],
    ];

    /// Coluna e linha (0, 1 ou 2) na grade.
    fn cell(self) -> (u8, u8) {
        match self {
            Pos::TopLeft => (0, 0),
            Pos::Top => (1, 0),
            Pos::TopRight => (2, 0),
            Pos::Left => (0, 1),
            Pos::Center | Pos::Diagonal => (1, 1),
            Pos::Right => (2, 1),
            Pos::BottomLeft => (0, 2),
            Pos::Bottom => (1, 2),
            Pos::BottomRight => (2, 2),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Text { text: String, color: [u8; 3] },
    Logo { path: PathBuf },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Opts {
    pub mark: Mark,
    pub pos: Pos,
    /// Opacidade, 0 a 100.
    pub opacity: u8,
    /// Texto: altura da letra em % do lado menor. Logo: largura em % da largura da imagem.
    pub size: f32,
    /// Distância da borda, em % do lado menor.
    pub margin: f32,
}

impl Opts {
    fn alpha(&self) -> f32 {
        self.opacity.min(100) as f32 / 100.0
    }
}


pub fn accepts(p: &Path) -> bool {
    let e = tools::ext_lower(p);
    imaging::IMAGE_EXTS.contains(&e.as_str()) || video::VIDEO_EXTS.contains(&e.as_str()) || e == "pdf"
}

/// Caminho dentro de um filtro do ffmpeg: barras normais, ":" e "'" escapados.
fn filter_path(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\\', "/").replace(':', "\\:").replace('\'', "\\'"))
}

fn font_file() -> PathBuf {
    let fonts = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Fonts");
    ["segoeuib.ttf", "arialbd.ttf", "arial.ttf"].iter().map(|f| fonts.join(f)).find(|p| p.is_file()).unwrap_or_else(|| fonts.join("arial.ttf"))
}

/// O filtro (filter_complex) que desenha a marca num quadro de `w`×`h`. O texto vai num arquivo (`textfile`), para
/// não precisar escapar nada. A entrada do logo, quando há, é a [1].
// i18n: fora
fn filter(w: u32, h: u32, o: &Opts, textfile: &Path) -> String {
    let short = w.min(h) as f32;
    let m = (short * o.margin / 100.0).round();
    let (col, row) = o.pos.cell();
    let a = o.alpha();
    let xy = |ow: &str, oh: &str, big_w: &str, big_h: &str| {
        let x = match col {
            0 => format!("{m}"),
            1 => format!("({big_w}-{ow})/2"),
            _ => format!("{big_w}-{ow}-{m}"),
        };
        let y = match row {
            0 => format!("{m}"),
            1 => format!("({big_h}-{oh})/2"),
            _ => format!("{big_h}-{oh}-{m}"),
        };
        (x, y)
    };
    let angle = -((h as f32).atan2(w as f32));
    match &o.mark {
        Mark::Text { color: [r, g, b], .. } => {
            let size = (short * o.size / 100.0).max(8.0).round();
            let shadow = (size / 18.0).max(1.0).round();
            let text = format!(
                "drawtext=fontfile={}:textfile={}:fontsize={size}:fontcolor=0x{r:02X}{g:02X}{b:02X}@{a:.2}:shadowcolor=black@{:.2}:shadowx={shadow}:shadowy={shadow}",
                filter_path(&font_file()),
                filter_path(textfile),
                a * 0.55
            );
            if o.pos == Pos::Diagonal {
                format!(
                    "color=c=black@0.0:s={w}x{h},format=rgba,{text}:x=(w-tw)/2:y=(h-th)/2,rotate={angle:.4}:c=none[t];[0:v][t]overlay=0:0:shortest=1,format=yuv420p[out]"
                )
            } else {
                let (x, y) = xy("tw", "th", "w", "h");
                format!("[0:v]{text}:x={x}:y={y}[out]")
            }
        }
        Mark::Logo { .. } => {
            let lw = ((w as f32 * o.size / 100.0).round() as u32).max(8) & !1;
            let logo = format!("[1:v]format=rgba,scale={lw}:-2,colorchannelmixer=aa={a:.2}");
            if o.pos == Pos::Diagonal {
                format!("{logo},rotate={angle:.4}:c=none:ow=rotw({angle:.4}):oh=roth({angle:.4})[l];[0:v][l]overlay=(W-w)/2:(H-h)/2:shortest=1,format=yuv420p[out]")
            } else {
                let (x, y) = xy("w", "h", "W", "H");
                format!("{logo}[l];[0:v][l]overlay={x}:{y}:shortest=1,format=yuv420p[out]")
            }
        }
    }
}
// i18n: dentro

/// Grava o texto da marca num arquivo temporário (o drawtext lê dali).
fn text_file(o: &Opts) -> Result<PathBuf, String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("conver_marca_{}_{n}.txt", std::process::id()));
    let text = match &o.mark {
        Mark::Text { text, .. } => text.trim().to_string(),
        Mark::Logo { .. } => String::new(),
    };
    std::fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(p)
}

fn check(o: &Opts) -> Result<(), String> {
    match &o.mark {
        Mark::Text { text, .. } if text.trim().is_empty() => Err("escreva o texto da marca d'água.".into()),
        Mark::Logo { path } if !path.is_file() => Err("escolha a imagem do logo.".into()),
        _ => Ok(()),
    }
}

/// Aplica a marca num arquivo (imagem, vídeo ou PDF).
pub fn apply(src: &Path, out_dir: Option<&Path>, o: &Opts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    check(o)?;
    let ext = tools::ext_lower(src);
    if ext == "pdf" {
        return apply_pdf(src, out_dir, o);
    }
    let is_video = video::VIDEO_EXTS.contains(&ext.as_str()) && ext != "gif";
    let info = video::probe(src);
    let (w, h) = match info.size {
        Some(s) => s,
        None => image::image_dimensions(src).map_err(|e| format!("não foi possível ler {}: {e}", tools::file_name(src)))?,
    };
    let tf = text_file(o)?;
    let mut input = s(&["-i", &path_s(src)]);
    if let Mark::Logo { path } = &o.mark {
        input.extend(s(&["-loop", "1", "-i", &path_s(path)]));
    }
    let mut out = s(&["-filter_complex", &filter(w, h, o, &tf), "-map", "[out]"]);
    let (dst, label) = if is_video {
        let e = keep_or_mp4(src);
        out.extend(s(&["-map", "0:a?", "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-c:a", "copy"]));
        if e == "mp4" {
            out.extend(s(&["-movflags", "+faststart"]));
        }
        (tools::out_path(src, out_dir, "_marca", &e), "marca d'água no vídeo")
    } else {
        let e = match ext.as_str() {
            "jpg" | "jpeg" => "jpg",
            "webp" => "webp",
            "bmp" => "bmp",
            _ => "png",
        };
        out.extend(s(&["-frames:v", "1", "-update", "1"]));
        match e {
            "jpg" => out.extend(s(&["-q:v", "2"])),
            "webp" => out.extend(s(&["-quality", "92"])),
            _ => {}
        }
        // o yuv420p do filtro serve ao vídeo; nas imagens, as cores ficam em RGB
        if let Some(last) = out.iter_mut().find(|a| a.contains("format=yuv420p")) {
            *last = last.replace(",format=yuv420p", "");
        }
        (tools::out_path(src, out_dir, "_marca", e), "marca d'água")
    };
    let r = encode_to(input, out, &dst, if is_video { info.duration } else { None }, on_progress, cancelled);
    let _ = std::fs::remove_file(&tf);
    r?;
    Ok((format!("{label} • {}", tools::human_size(tools::file_size(&dst))), dst))
}

/// Prévia: a marca aplicada numa imagem (PNG em memória), reduzida para `max` de altura.
pub fn preview(base_png: &Path, o: &Opts, max: u32) -> Result<Vec<u8>, String> {
    check(o)?;
    let (w, h) = image::image_dimensions(base_png).map_err(|e| e.to_string())?;
    let tf = text_file(o)?;
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let mut cmd = tools::command(&ff);
    cmd.args(["-v", "error", "-nostdin", "-i"]).arg(base_png);
    if let Mark::Logo { path } = &o.mark {
        cmd.args(["-loop", "1", "-i"]).arg(path);
    }
    let f = filter(w, h, o, &tf).replace(",format=yuv420p", "").replace("[out]", &format!(",scale=-2:{}[out]", max.min(h)));
    let r = cmd.args(["-filter_complex", &f, "-map", "[out]", "-frames:v", "1", "-f", "image2pipe", "-c:v", "png", "-"]).output();
    let _ = std::fs::remove_file(&tf);
    let out = r.map_err(|e| e.to_string())?;
    if out.stdout.is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("ffmpeg falhou").to_string());
    }
    Ok(out.stdout)
}

// ------------------------------------------------------------------ PDF

/// Texto no WinAnsi da fonte padrão (português e espanhol cabem; o resto vira "?").
fn win_ansi(text: &str) -> Vec<u8> {
    text.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).collect()
}

fn pdf_string(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'('];
    for &b in bytes {
        if matches!(b, b'(' | b')' | b'\\') {
            out.push(b'\\');
        }
        out.push(b);
    }
    out.push(b')');
    out
}

fn num(o: &Object) -> Option<f32> {
    o.as_float().ok().or_else(|| o.as_i64().ok().map(|v| v as f32))
}

/// Um valor do dicionário como dicionário (seguindo a referência, se for uma).
fn resolve_dict(doc: &Document, v: Option<&Object>) -> Dictionary {
    match v {
        Some(Object::Dictionary(d)) => d.clone(),
        Some(Object::Reference(id)) => doc.get_dictionary(*id).cloned().unwrap_or_default(),
        _ => Dictionary::new(),
    }
}

// i18n: fora
fn apply_pdf(src: &Path, out_dir: Option<&Path>, o: &Opts) -> Result<(String, PathBuf), String> {
    let mut doc = crate::pdf::open(src)?;
    let alpha = o.alpha();
    let gs = doc.add_object(dictionary! { "Type" => "ExtGState", "ca" => alpha, "CA" => alpha });
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica-Bold", "Encoding" => "WinAnsiEncoding" });
    // o logo vira uma imagem do PDF (RGB) com a transparência numa máscara (SMask)
    let logo = match &o.mark {
        Mark::Logo { path } => {
            let img = image::open(path).map_err(|e| format!("não foi possível abrir o logo: {e}"))?.to_rgba8();
            let (lw, lh) = img.dimensions();
            let rgb: Vec<u8> = img.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
            let a: Vec<u8> = img.pixels().map(|p| p[3]).collect();
            let mut mask = Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => lw as i64, "Height" => lh as i64, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 }, a);
            let _ = mask.compress();
            let mask_id = doc.add_object(mask);
            let mut st = Stream::new(
                dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => lw as i64, "Height" => lh as i64, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "SMask" => mask_id },
                rgb,
            );
            let _ = st.compress();
            Some((doc.add_object(st), lw as f32, lh as f32))
        }
        Mark::Text { .. } => None,
    };
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    for pid in &pages {
        let full = crate::pdf::with_inherited(&doc, *pid).ok_or("não foi possível ler uma página do PDF.")?;
        let mb: Vec<f32> = full.get(b"MediaBox").ok().and_then(|v| v.as_array().ok()).map(|a| a.iter().filter_map(num).collect()).unwrap_or_default();
        let (x0, y0, x1, y1) = if mb.len() == 4 { (mb[0], mb[1], mb[2], mb[3]) } else { (0.0, 0.0, 595.0, 842.0) };
        let (pw, ph) = (x1 - x0, y1 - y0);
        let rot = full.get(b"Rotate").ok().and_then(|v| v.as_i64().ok()).unwrap_or(0).rem_euclid(360);
        // desenhamos no espaço "como a página aparece" e convertemos para o da página
        let (dw, dh) = if rot == 90 || rot == 270 { (ph, pw) } else { (pw, ph) };
        let cm = match rot {
            90 => format!("0 1 -1 0 {} {}", x0 + pw, y0),
            180 => format!("-1 0 0 -1 {} {}", x0 + pw, y0 + ph),
            270 => format!("0 -1 1 0 {} {}", x0, y0 + ph),
            _ => format!("1 0 0 1 {x0} {y0}"),
        };
        let short = dw.min(dh);
        let m = short * o.margin / 100.0;
        let (col, row) = o.pos.cell();
        let place = |bw: f32, bh: f32| -> (f32, f32) {
            let x = match col {
                0 => m,
                1 => (dw - bw) / 2.0,
                _ => dw - bw - m,
            };
            // no PDF o y cresce para cima
            let y = match row {
                0 => dh - bh - m,
                1 => (dh - bh) / 2.0,
                _ => m,
            };
            (x, y)
        };
        let angle = dh.atan2(dw);
        let mut ops: Vec<u8> = format!("q /ConverWmG gs {cm} cm\n").into_bytes();
        match (&o.mark, logo) {
            (Mark::Text { text, color: [r, g, b] }, _) => {
                let size = (short * o.size / 100.0).max(6.0);
                let bytes = win_ansi(text.trim());
                let tw = bytes.len() as f32 * size * 0.58;
                let color = format!("{:.3} {:.3} {:.3} rg", *r as f32 / 255.0, *g as f32 / 255.0, *b as f32 / 255.0);
                let tm = if o.pos == Pos::Diagonal {
                    let (c, s) = (angle.cos(), angle.sin());
                    // centro da página, com o texto centrado na linha inclinada
                    let (cx, cy) = (dw / 2.0 - c * tw / 2.0 + s * size * 0.35, dh / 2.0 - s * tw / 2.0 - c * size * 0.35);
                    format!("{c:.4} {s:.4} {:.4} {c:.4} {cx:.2} {cy:.2} Tm", -s)
                } else {
                    let (x, y) = place(tw, size * 0.72);
                    format!("1 0 0 1 {x:.2} {y:.2} Tm")
                };
                ops.extend(format!("BT /ConverWmF {size:.2} Tf {color} {tm} ").into_bytes());
                ops.extend(pdf_string(&bytes));
                ops.extend(b" Tj ET\n");
            }
            (Mark::Logo { .. }, Some((_, lw, lh))) => {
                let w = dw * o.size / 100.0;
                let h = w * lh / lw;
                if o.pos == Pos::Diagonal {
                    let (c, s) = (angle.cos(), angle.sin());
                    let (cx, cy) = (dw / 2.0 - (c * w - s * h) / 2.0, dh / 2.0 - (s * w + c * h) / 2.0);
                    ops.extend(format!("{:.4} {:.4} {:.4} {:.4} {cx:.2} {cy:.2} cm /ConverWmI Do\n", c * w, s * w, -s * h, c * h).into_bytes());
                } else {
                    let (x, y) = place(w, h);
                    ops.extend(format!("{w:.2} 0 0 {h:.2} {x:.2} {y:.2} cm /ConverWmI Do\n").into_bytes());
                }
            }
            _ => {}
        }
        ops.extend(b"Q\n");
        // o conteúdo original fica entre q … Q, para nada que ele deixe mudado afetar a marca
        let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
        let close = doc.add_object(Stream::new(Dictionary::new(), b"Q\n".to_vec()));
        let mark = doc.add_object(Stream::new(Dictionary::new(), ops));
        let mut contents = vec![Object::Reference(open)];
        match full.get(b"Contents") {
            Ok(Object::Array(a)) => contents.extend(a.iter().cloned()),
            Ok(other) => contents.push(other.clone()),
            Err(_) => {}
        }
        contents.push(Object::Reference(close));
        contents.push(Object::Reference(mark));
        // recursos da página (herdados ou não) + os da marca
        let mut res = resolve_dict(&doc, full.get(b"Resources").ok());
        let mut ext = resolve_dict(&doc, res.get(b"ExtGState").ok());
        ext.set("ConverWmG", gs);
        res.set("ExtGState", ext);
        let mut fonts = resolve_dict(&doc, res.get(b"Font").ok());
        fonts.set("ConverWmF", font);
        res.set("Font", fonts);
        if let Some((img, _, _)) = logo {
            let mut xo = resolve_dict(&doc, res.get(b"XObject").ok());
            xo.set("ConverWmI", img);
            res.set("XObject", xo);
        }
        let page = doc.get_dictionary_mut(*pid).map_err(|e| e.to_string())?;
        page.set("Contents", Object::Array(contents));
        page.set("Resources", res);
    }
    let dst = tools::out_path(src, out_dir, "_marca", "pdf");
    doc.compress();
    doc.save(&dst).map_err(|e| format!("não foi possível salvar o PDF: {e}"))?;
    Ok((format!("{} página(s) com marca d'água", pages.len()), dst))
}
// i18n: dentro
