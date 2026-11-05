//! Gerar imagens a partir de texto no próprio PC: stable-diffusion.cpp (MIT, Vulkan: roda na placa de vídeo, de
//! qualquer marca) com o modelo DreamShaper 8 LCM (CreativeML OpenRAIL-M, 2,1 GB, baixado no primeiro uso), que gera
//! uma boa imagem em 4 a 8 passos.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::ai::{self, ModelSpec};
use crate::tools;

pub const MODEL: ModelSpec = ModelSpec {
    file: "DreamShaper8_LCM.safetensors",
    url: "https://huggingface.co/Lykon/dreamshaper-8-lcm/resolve/main/DreamShaper8_LCM.safetensors",
    sha256: "a4f3e1526c5dc4fcbe342f5c410d83ae202c7a415fcefcbb92e0f93fcd0a87c3",
    size: 2_133_804_992,
    label: "DreamShaper 8 LCM (2,1 GB)",
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Quadrada,
    Retrato,
    Paisagem,
}

impl Shape {
    pub fn size(self) -> (u32, u32) {
        match self {
            Shape::Quadrada => (512, 512),
            Shape::Retrato => (512, 768),
            Shape::Paisagem => (768, 512),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    /// 4 passos, sem guia: uns 10 s por imagem numa placa de vídeo comum.
    Rapida,
    /// 8 passos com guia leve: mais detalhe, ~3× mais lento.
    Melhor,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub prompt: String,
    pub negative: String,
    pub shape: Shape,
    pub quality: Quality,
    pub count: u32,
    /// `None` = sorteado.
    pub seed: Option<i64>,
}

/// O que sempre vai no "não quero" (o usuário pode acrescentar).
const BASE_NEGATIVE: &str = "blurry, low quality, deformed, bad anatomy, watermark, text, signature";

fn sd_cli() -> Result<PathBuf, String> {
    tools::find_tool_file(r"sd\sd-cli.exe").ok_or_else(|| "sd-cli.exe não encontrado (pasta bin\\sd).".to_string())
}

/// Pasta padrão: Imagens\Conver.
pub fn default_dir() -> PathBuf {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Pictures").join("Conver")).unwrap_or_else(std::env::temp_dir)
}

/// Gera as imagens em `dir` e devolve os arquivos.
pub fn generate(req: &Request, dir: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<PathBuf>, String> {
    if req.prompt.trim().is_empty() {
        return Err("descreva a imagem.".into());
    }
    let exe = sd_cli()?;
    // 0–20%: baixar o modelo (só na primeira vez, 2,1 GB)
    let model = ai::ensure_model(&MODEL, &|p| on_progress(p * 0.2), cancelled)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("não foi possível criar a pasta: {e}"))?;
    let (w, h) = req.shape.size();
    let (steps, cfg) = match req.quality {
        Quality::Rapida => (4, "1.0"),
        Quality::Melhor => (8, "1.5"),
    };
    let count = req.count.clamp(1, 8);
    let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
    // o horário vai até o minuto: se já há imagens com esse nome (outra geração no mesmo minuto), acrescenta um número
    let taken = |b: &str| (0..count).any(|i| dir.join(format!("{b}_{i}.png")).exists());
    let mut base = format!("imagem_{stamp}");
    let mut n = 2;
    while taken(&base) {
        base = format!("imagem_{stamp}-{n}");
        n += 1;
    }
    let negative = if req.negative.trim().is_empty() { BASE_NEGATIVE.to_string() } else { format!("{}, {BASE_NEGATIVE}", req.negative.trim()) };
    let mut child = tools::command(&exe)
        .arg("-m")
        .arg(&model)
        .args(["-p", req.prompt.trim(), "-n", &negative, "--sampling-method", "lcm", "--steps", &steps.to_string(), "--cfg-scale", cfg])
        .args(["-W", &w.to_string(), "-H", &h.to_string(), "-b", &count.to_string(), "-s", &req.seed.unwrap_or(-1).to_string(), "-o"])
        .arg(dir.join(format!("{base}_%d.png")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o gerador: {e}"))?;
    // o progresso vem com "\r": lê os bytes e separa as linhas à mão
    let mut out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let err_text = std::thread::spawn(move || {
        let mut s = String::new();
        let mut e = err;
        let _ = e.read_to_string(&mut s);
        s
    });
    let mut buf = [0u8; 4096];
    let mut line = String::new();
    let mut tail: Vec<String> = Vec::new();
    let (mut image_k, mut loaded) = (0u32, false);
    loop {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cancelado.".into());
        }
        let n = match out.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for ch in String::from_utf8_lossy(&buf[..n]).chars() {
            if ch != '\r' && ch != '\n' {
                line.push(ch);
                continue;
            }
            let l = std::mem::take(&mut line);
            if let Some(rest) = l.split("generating image: ").nth(1) {
                image_k = rest.split('/').next().and_then(|v| v.trim().parse().ok()).unwrap_or(image_k + 1);
                loaded = true;
            } else if let Some((done, total)) = l.split('|').nth(2).and_then(|r| r.split('-').next()).and_then(|r| r.trim().split_once('/')) {
                if let (Ok(d), Ok(t)) = (done.trim().parse::<f32>(), total.trim().parse::<f32>()) {
                    let frac = (d / t.max(1.0)).clamp(0.0, 1.0);
                    let p = if loaded {
                        0.3 + 0.7 * ((image_k.max(1) - 1) as f32 + frac) / count as f32
                    } else {
                        0.2 + 0.1 * frac // carregando o modelo na placa de vídeo
                    };
                    on_progress(p.min(0.99));
                }
            }
            if !l.trim().is_empty() {
                tail.push(l);
                if tail.len() > 8 {
                    tail.remove(0);
                }
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let err_text = err_text.join().unwrap_or_default();
    let files: Vec<PathBuf> = (0..count).map(|i| dir.join(format!("{base}_{i}.png"))).filter(|p| p.is_file()).collect();
    if !status.success() || files.is_empty() {
        let why = err_text.lines().chain(tail.iter().map(String::as_str)).filter(|l| l.contains("[E]") || l.to_lowercase().contains("error")).last().unwrap_or("o gerador parou sem salvar a imagem.").to_string();
        let low = why.to_lowercase();
        return Err(if low.contains("memory") || low.contains("alloc") { format!("faltou memória na placa de vídeo; tente o formato quadrado. ({why})") } else { why });
    }
    on_progress(1.0);
    Ok(files)
}
