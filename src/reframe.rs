//! Vertical automático: transforma vídeo horizontal em 9:16 (ou 4:5, 1:1) seguindo o rosto de quem aparece, em vez
//! de cortar sempre o centro. Duas passadas: a primeira olha 5 quadros por segundo em baixa resolução e acha os
//! rostos (YuNet, o mesmo de "Desfocar rostos"); o caminho da "câmera" é suavizado (só anda quando o rosto sai do
//! meio, e acompanha devagar, sem tremer; numa troca de cena, pula direto). A segunda passada é um ffmpeg só, com o
//! recorte movido quadro a quadro por `sendcmd`, e o áudio copiado.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use image::RgbImage;

use crate::media::path_s;
use crate::tools;
use crate::video::{self, s};

/// Quadros por segundo olhados na primeira passada.
const SAMPLE_FPS: f64 = 5.0;
/// Largura da imagem olhada (a detecção não precisa de mais).
const SAMPLE_W: u32 = 640;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Aspect {
    #[default]
    Vertical,
    Portrait,
    Square,
}

impl Aspect {
    /// Largura sobre altura.
    fn ratio(self) -> f64 {
        match self {
            Aspect::Vertical => 9.0 / 16.0,
            Aspect::Portrait => 4.0 / 5.0,
            Aspect::Square => 1.0,
        }
    }

    /// Tamanho final.
    fn out_size(self) -> (u32, u32) {
        match self {
            Aspect::Vertical => (1080, 1920),
            Aspect::Portrait => (1080, 1350),
            Aspect::Square => (1080, 1080),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Opts {
    pub aspect: Aspect,
    /// Rapidez para acompanhar o rosto: 0,1 (bem suave) a 0,5 (rápido).
    pub follow: f64,
}

impl Default for Opts {
    fn default() -> Self {
        Self { aspect: Aspect::Vertical, follow: 0.25 }
    }
}

/// Onde o centro do recorte deve ficar em cada amostra (em pixels do vídeo), a partir do centro do rosto principal de
/// cada amostra (`None` = nenhum rosto). `width` é a largura do vídeo e `crop` a do recorte.
pub fn camera_path(targets: &[Option<f64>], width: f64, crop: f64, follow: f64) -> Vec<f64> {
    let half = crop / 2.0;
    let clamp = |x: f64| x.clamp(half, (width - half).max(half));
    let dead = crop * 0.12;
    let mut out = Vec::with_capacity(targets.len());
    // começa onde aparece o primeiro rosto (ou no meio)
    let mut cam = clamp(targets.iter().flatten().next().copied().unwrap_or(width / 2.0));
    let (mut far, mut moving) = (0, false);
    for t in targets {
        if let Some(t) = *t {
            let d = t - cam;
            if d.abs() > width * 0.3 {
                // longe demais: uma troca de cena ou de pessoa; se continua assim, pula direto
                far += 1;
                if far >= 2 {
                    cam = t;
                    far = 0;
                    moving = false;
                }
            } else {
                far = 0;
                // parada enquanto o rosto está na faixa do meio; quando ele sai, anda até centralizar de novo
                if d.abs() > dead {
                    moving = true;
                }
                if moving {
                    cam += d * follow;
                    if (t - cam).abs() < crop * 0.03 {
                        moving = false;
                    }
                }
            }
        }
        cam = clamp(cam);
        out.push(cam);
    }
    out
}

/// Centro do rosto principal (o maior, pesado pela confiança) de cada amostra.
fn analyze(src: &Path, w: u32, h: u32, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<Option<f64>>, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let sh = ((SAMPLE_W as f64 * h as f64 / w as f64).round() as u32).max(2) & !1;
    let total = video::probe(src).duration.map(|d| (d * SAMPLE_FPS) as usize).unwrap_or(0);
    let mut dec = tools::command(&ff)
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(src)
        .args(["-vf", &format!("fps={SAMPLE_FPS},scale={SAMPLE_W}:{sh}"), "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    let mut input = dec.stdout.take().ok_or("ffmpeg")?;
    let mut buf = vec![0u8; (SAMPLE_W * sh * 3) as usize];
    let scale = w as f64 / SAMPLE_W as f64;
    let mut out = Vec::new();
    let r = (|| {
        while input.read_exact(&mut buf).is_ok() {
            if cancelled() {
                return Err("Cancelado.".to_string());
            }
            let img = RgbImage::from_raw(SAMPLE_W, sh, buf.clone()).ok_or("quadro inválido")?;
            let faces = crate::faces::detect(&img, 0.6)?;
            let main = faces.iter().max_by(|a, b| (a.w * a.h * a.score).total_cmp(&(b.w * b.h * b.score)));
            out.push(main.map(|f| (f.x + f.w / 2.0) as f64 * scale));
            if total > 0 {
                on_progress((out.len() as f32 / total as f32).min(1.0) * 0.4);
            }
        }
        Ok(())
    })();
    let _ = dec.kill();
    let _ = dec.wait();
    r.map(|_| out)
}

/// Faz a versão vertical de um vídeo.
pub fn reframe(src: &Path, out_dir: Option<&Path>, o: Opts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let info = video::probe(src);
    let (w, h) = info.size.ok_or("o arquivo não tem vídeo.")?;
    let crop_w = ((h as f64 * o.aspect.ratio()).round() as u32 & !1).min(w & !1);
    if crop_w as f64 >= w as f64 * 0.98 {
        return Err("este vídeo já é vertical (ou mais estreito que o formato pedido).".into());
    }
    let targets = analyze(src, w, h, on_progress, cancelled)?;
    let found = targets.iter().filter(|t| t.is_some()).count();
    let path = camera_path(&targets, w as f64, crop_w as f64, o.follow);
    // um comando por quadro de saída (a 30 por segundo), interpolando entre as amostras
    let dir = std::env::temp_dir().join(format!("conver_vertical_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut cmds = String::new();
    let dur = info.duration.unwrap_or(path.len() as f64 / SAMPLE_FPS);
    let steps = (dur * 30.0).ceil() as usize;
    for i in 0..steps {
        let t = i as f64 / 30.0;
        let pos = t * SAMPLE_FPS;
        let (a, frac) = (pos.floor() as usize, pos.fract());
        let x = match (path.get(a), path.get(a + 1)) {
            (Some(x0), Some(x1)) => x0 + (x1 - x0) * frac,
            (Some(x0), None) => *x0,
            _ => path.last().copied().unwrap_or(w as f64 / 2.0),
        };
        let left = ((x - crop_w as f64 / 2.0).round().max(0.0) as u32).min(w - crop_w);
        // i18n: fora
        cmds.push_str(&format!("{t:.3} crop@c x {left};\n"));
        // i18n: dentro
    }
    std::fs::write(dir.join("caminho.txt"), cmds).map_err(|e| e.to_string())?;
    let first = path.first().map(|x| ((x - crop_w as f64 / 2.0).max(0.0) as u32).min(w - crop_w)).unwrap_or((w - crop_w) / 2);
    let (ow, oh) = o.aspect.out_size();
    let filter = format!("sendcmd=f=caminho.txt,crop@c=w={crop_w}:h={h}:x={first}:y=0,scale={ow}:{oh}:flags=lanczos,setsar=1");
    let dst = tools::out_path(src, out_dir, "_vertical", "mp4");
    // o filtro lê o arquivo de comandos pela pasta temporária (caminhos do Windows não cabem bem num filtro)
    let input = s(&["-i", &path_s(src)]);
    let output = s(&["-map", "0:v:0", "-map", "0:a?", "-vf", &filter, "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]);
    let r = run_in(&dir, input, output, &dst, info.duration, &|p| on_progress(0.4 + p * 0.6), cancelled);
    let _ = std::fs::remove_dir_all(&dir);
    r?;
    let note = if found == 0 { "nenhum rosto encontrado: ficou o centro" } else { "seguindo o rosto" };
    Ok((format!("{}×{} • {} • {}", ow, oh, crate::i18n::tr(note), tools::human_size(tools::file_size(&dst))), dst))
}

/// Roda o ffmpeg com a pasta de trabalho `dir` (para o `sendcmd` achar o arquivo).
fn run_in(dir: &Path, input: Vec<String>, output: Vec<String>, dst: &Path, dur: Option<f64>, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let mut args = input;
    args.extend(output);
    args.push(path_s(dst));
    crate::video::run(args, dur, Some(dir), on_progress, cancelled).inspect_err(|_| {
        let _ = std::fs::remove_file(dst);
    })
}
