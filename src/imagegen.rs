//! Gerar imagens a partir de texto no próprio PC: stable-diffusion.cpp (MIT, Vulkan: roda na placa de vídeo, de
//! qualquer marca) com um de três modelos, baixados no primeiro uso:
//! - Rápido: DreamShaper 8 LCM (SD 1.5, CreativeML OpenRAIL-M, 2,1 GB), 512 px em 4 a 8 passos;
//! - SDXL: DreamShaper XL Lightning (OpenRAIL++, 6,9 GB), 1024 px em 6 a 8 passos;
//! - Z-Image Turbo (Alibaba, Apache-2.0; GGUF Q4_K + Qwen3 4B como leitor do texto + VAE do FLUX, 6,7 GB),
//!   1024 px em 8 passos, o que melhor entende pedidos longos e escreve textos na imagem.

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

// i18n: fora
pub const XL: ModelSpec = ModelSpec {
    file: "DreamShaperXL_Lightning.safetensors",
    url: "https://huggingface.co/Lykon/dreamshaper-xl-lightning/resolve/561086c22aba3925b1e8803a73857f061635c7c3/DreamShaperXL_Lightning.safetensors",
    sha256: "fdbe56354b8f876b736f24d3ad867ecd4140c019f57642ec8ddd878088d44f64",
    size: 6_939_220_250,
    label: "DreamShaper XL Lightning (6,9 GB)",
};

pub const Z_IMAGE: ModelSpec = ModelSpec {
    file: "z_image_turbo-Q4_K.gguf",
    url: "https://huggingface.co/leejet/Z-Image-Turbo-GGUF/resolve/c61c0e422dc8b541b7548cf33a4ef8302b0f8085/z_image_turbo-Q4_K.gguf",
    sha256: "14b375ab4f226bc5378f68f37e899ef3c2242b8541e61e2bc1aff40976086fbd",
    size: 3_864_250_304,
    label: "Z-Image Turbo (3,9 GB)",
};

pub const Z_TEXT: ModelSpec = ModelSpec {
    file: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
    url: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/a06e946bb6b655725eafa393f4a9745d460374c9/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
    sha256: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597",
    size: 2_497_281_120,
    label: "Qwen3 4B — leitor do texto (2,5 GB)",
};

pub const Z_VAE: ModelSpec = ModelSpec {
    file: "flux_ae.safetensors",
    url: "https://huggingface.co/Comfy-Org/Z-Image-Turbo/resolve/6fc90a3b1b653e935a0d175e260736de25b84df5/split_files/vae/ae.safetensors",
    sha256: "afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38",
    size: 335_304_388,
    label: "VAE do FLUX (335 MB)",
};
pub const KLEIN: ModelSpec = ModelSpec {
    file: "flux-2-klein-4b-Q4_0.gguf",
    url: "https://huggingface.co/leejet/FLUX.2-klein-4B-GGUF/resolve/3b1f5a9dc3abb32238b053aeb3d823c30afdacbd/flux-2-klein-4b-Q4_0.gguf",
    sha256: "d1023499ef3f2f82ff7c50e6778495195c1b6cc34835741778868428111f9ff4",
    size: 2_460_378_560,
    label: "FLUX.2 klein 4B (2,5 GB)",
};

pub const KLEIN_VAE: ModelSpec = ModelSpec {
    file: "flux2-vae.safetensors",
    url: "https://huggingface.co/Comfy-Org/flux2-klein-4B/resolve/5b3abca5c0dc1ecd4eaf3340868d09063fc1c965/split_files/vae/flux2-vae.safetensors",
    sha256: "868fe7b343cc8f3a19dbcfcafbc3d5f888802be3f89bd81b65b3621a066ce8f3",
    size: 336_211_292,
    label: "VAE do FLUX.2 (336 MB)",
};

/// Arquivos do modelo de edição por instrução (FLUX.2 klein 4B, Apache-2.0; o leitor de texto é o mesmo Qwen3 4B do
/// Z-Image).
pub fn edit_files() -> Vec<ModelSpec> {
    vec![KLEIN, Z_TEXT, KLEIN_VAE]
}
// i18n: dentro

/// Qual modelo gera a imagem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Engine {
    Rapido,
    Xl,
    ZImage,
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::ZImage, Engine::Xl, Engine::Rapido];

    pub fn files(self) -> Vec<ModelSpec> {
        match self {
            Engine::Rapido => vec![MODEL],
            Engine::Xl => vec![XL],
            Engine::ZImage => vec![Z_IMAGE, Z_TEXT, Z_VAE],
        }
    }

    pub fn size(self) -> u64 {
        self.files().iter().map(|f| f.size).sum()
    }

    pub fn installed(self) -> bool {
        self.files().iter().all(ai::is_installed)
    }

    /// O melhor modelo que já está no computador (ou o rápido, o menor para baixar).
    pub fn best_installed() -> Engine {
        Engine::ALL.into_iter().find(|e| e.installed()).unwrap_or(Engine::Rapido)
    }

    /// Lado da imagem quadrada. Os modelos novos foram treinados em 1024 px; o Z-Image na qualidade rápida usa
    /// 768 px (numa RX 580 de 8 GB: ~4 min contra ~7 min em 1024 px).
    pub fn base(self, q: Quality) -> u32 {
        match (self, q) {
            (Engine::Rapido, _) => 512,
            (Engine::ZImage, Quality::Rapida) => 768,
            _ => 1024,
        }
    }

    /// Entende o "evitar" (os modelos de passos curtos sem guia o ignoram).
    pub fn uses_negative(self) -> bool {
        self != Engine::ZImage
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Quadrada,
    Retrato,
    Paisagem,
}

impl Shape {
    pub fn size_for(self, e: Engine, q: Quality) -> (u32, u32) {
        match (e.base(q), self) {
            (512, Shape::Quadrada) => (512, 512),
            (512, Shape::Retrato) => (512, 768),
            (512, Shape::Paisagem) => (768, 512),
            (768, Shape::Quadrada) => (768, 768),
            (768, Shape::Retrato) => (640, 960),
            (768, Shape::Paisagem) => (960, 640),
            (_, Shape::Quadrada) => (1024, 1024),
            (_, Shape::Retrato) => (832, 1216),
            (_, Shape::Paisagem) => (1216, 832),
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
    pub engine: Engine,
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

/// Baixa o que faltar de `files` (de 0 a 20% do progresso) e devolve os caminhos.
fn ensure_all(files: &[ModelSpec], on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<PathBuf>, String> {
    let total = files.iter().map(|f| f.size).sum::<u64>().max(1) as f32;
    let mut done = 0u64;
    let mut paths = Vec::new();
    for f in files {
        let (base, part) = (done as f32 / total, f.size as f32 / total);
        paths.push(ai::ensure_model(f, &|p| on_progress((base + p * part) * 0.2), cancelled)?);
        done += f.size;
    }
    Ok(paths)
}

// i18n: fora
/// Argumentos do modelo e do jeito de amostrar de cada um (o recomendado por quem fez o modelo): modelo, amostrador,
/// passos e guia.
fn engine_args(e: Engine, paths: &[PathBuf], fast: bool) -> (Vec<std::ffi::OsString>, u32, &'static str) {
    let mut a: Vec<std::ffi::OsString> = Vec::new();
    let (sampler, steps, cfg): (&[&str], u32, &str) = match e {
        Engine::Rapido => {
            a.extend(["-m".into(), paths[0].clone().into_os_string()]);
            (&["--sampling-method", "lcm"], if fast { 4 } else { 8 }, if fast { "1.0" } else { "1.5" })
        }
        Engine::Xl => {
            a.extend(["-m".into(), paths[0].clone().into_os_string()]);
            (&["--sampling-method", "dpm++2m", "--scheduler", "karras", "--vae-tiling", "--flash-attn", "on"], if fast { 6 } else { 8 }, "2.0")
        }
        Engine::ZImage => {
            for (flag, p) in ["--diffusion-model", "--llm", "--vae"].iter().zip(paths) {
                a.extend([(*flag).into(), p.clone().into_os_string()]);
            }
            // sem a atenção "flash" a memória da placa estoura (ErrorOutOfDeviceMemory numa de 8 GB)
            (&["--vae-tiling", "--flash-attn", "on"], 8, "1.0")
        }
    };
    a.extend(sampler.iter().map(|s| (*s).into()));
    (a, steps, cfg)
}
// i18n: dentro

/// Nome livre para `count` imagens em `dir` ("imagem_<data>_0.png", com "-2", "-3"… se o minuto já foi usado).
fn free_base(dir: &Path, prefix: &str, count: u32) -> String {
    let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
    let taken = |b: &str| (0..count).any(|i| dir.join(format!("{b}_{i}.png")).exists());
    let mut base = format!("{prefix}_{stamp}");
    let mut n = 2;
    while taken(&base) {
        base = format!("{prefix}_{stamp}-{n}");
        n += 1;
    }
    base
}

/// Gera as imagens em `dir` e devolve os arquivos.
pub fn generate(req: &Request, dir: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<PathBuf>, String> {
    if req.prompt.trim().is_empty() {
        return Err("descreva a imagem.".into());
    }
    let exe = sd_cli()?;
    let paths = ensure_all(&req.engine.files(), on_progress, cancelled)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("não foi possível criar a pasta: {e}"))?;
    let (w, h) = req.shape.size_for(req.engine, req.quality);
    let (model_args, steps, cfg) = engine_args(req.engine, &paths, req.quality == Quality::Rapida);
    let count = req.count.clamp(1, 8);
    let base = free_base(dir, "imagem", count);
    let negative = if req.negative.trim().is_empty() { BASE_NEGATIVE.to_string() } else { format!("{}, {BASE_NEGATIVE}", req.negative.trim()) };
    let mut cmd = tools::command(&exe);
    cmd.args(&model_args).args(["-p", req.prompt.trim()]);
    if req.engine.uses_negative() {
        cmd.args(["-n", &negative]);
    }
    cmd.args(["--steps", &steps.to_string(), "--cfg-scale", cfg])
        .args(["-W", &w.to_string(), "-H", &h.to_string(), "-b", &count.to_string(), "-s", &req.seed.unwrap_or(-1).to_string(), "-o"])
        .arg(dir.join(format!("{base}_%d.png")));
    run_sd(cmd, dir, &base, count, on_progress, cancelled)
}

/// Como editar uma imagem.
#[derive(Clone, Debug)]
pub enum EditMode {
    /// Escrever o que mudar ("deixe de noite"): FLUX.2 klein.
    Instruction,
    /// Repintar só a área branca da máscara (mesmo tamanho da imagem) com o pedido.
    Paint(image::GrayImage),
    /// Nova versão parecida; `strength` de 0 (igual) a 1 (outra imagem).
    Vary(f32),
}

#[derive(Clone, Debug)]
pub struct EditRequest {
    pub prompt: String,
    pub mode: EditMode,
    /// Modelo para "pintar" e "variar" (a edição por instrução usa sempre o FLUX.2 klein).
    pub engine: Engine,
    pub quality: Quality,
    pub count: u32,
    pub seed: Option<i64>,
}

/// Tamanho de trabalho: a proporção da imagem, com o lado maior do modelo e múltiplos de 16.
fn edit_size(w: u32, h: u32, side: u32) -> (u32, u32) {
    let s = side as f32 / w.max(h) as f32;
    let r = |v: u32| (((v as f32 * s) / 16.0).round() as u32 * 16).max(256);
    (r(w), r(h))
}

/// Edita `src` e salva as versões em `dir` (o nome começa com o do original e "_editada").
pub fn edit(src: &Path, req: &EditRequest, dir: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<PathBuf>, String> {
    if req.prompt.trim().is_empty() {
        return Err("escreva o que mudar na imagem.".into());
    }
    let exe = sd_cli()?;
    let img = crate::imaging::load(src)?;
    let instruction = matches!(req.mode, EditMode::Instruction);
    let files = if instruction { edit_files() } else { req.engine.files() };
    let paths = ensure_all(&files, on_progress, cancelled)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("não foi possível criar a pasta: {e}"))?;
    let side = if instruction { if req.quality == Quality::Rapida { 768 } else { 1024 } } else { req.engine.base(req.quality) };
    let (w, h) = edit_size(img.width(), img.height(), side);
    // a imagem (e a máscara) já no tamanho de trabalho, em arquivos temporários
    let tmp = std::env::temp_dir().join(format!("conver_editar_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let input = tmp.join("entrada.png");
    img.resize_exact(w, h, image::imageops::FilterType::Lanczos3).to_rgb8().save(&input).map_err(|e| e.to_string())?;
    let count = req.count.clamp(1, 4);
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "imagem".into());
    let base = free_base(dir, &format!("{stem}_editada"), count);
    let mut cmd = tools::command(&exe);
    // i18n: fora
    match &req.mode {
        EditMode::Instruction => {
            for (flag, p) in ["--diffusion-model", "--llm", "--vae"].iter().zip(&paths) {
                cmd.arg(flag).arg(p);
            }
            cmd.arg("-r").arg(&input).args(["--sampling-method", "euler", "--steps", "4", "--cfg-scale", "1.0", "--vae-tiling", "--flash-attn", "on"]);
        }
        EditMode::Paint(mask) => {
            let (model_args, steps, cfg) = engine_args(req.engine, &paths, req.quality == Quality::Rapida);
            let m = tmp.join("mascara.png");
            image::imageops::resize(mask, w, h, image::imageops::FilterType::Nearest).save(&m).map_err(|e| e.to_string())?;
            cmd.args(&model_args).arg("-i").arg(&input).arg("--mask").arg(&m).args(["--strength", "0.95", "--steps", &steps.to_string(), "--cfg-scale", cfg]);
        }
        EditMode::Vary(strength) => {
            let (model_args, steps, cfg) = engine_args(req.engine, &paths, req.quality == Quality::Rapida);
            cmd.args(&model_args).arg("-i").arg(&input).args(["--strength", &format!("{:.2}", strength.clamp(0.1, 0.95)), "--steps", &steps.to_string(), "--cfg-scale", cfg]);
        }
    }
    cmd.args(["-p", req.prompt.trim()])
        .args(["-W", &w.to_string(), "-H", &h.to_string(), "-b", &count.to_string(), "-s", &req.seed.unwrap_or(-1).to_string(), "-o"])
        .arg(dir.join(format!("{base}_%d.png")));
    // i18n: dentro
    let r = run_sd(cmd, dir, &base, count, on_progress, cancelled);
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

/// Roda o gerador, acompanha o progresso e devolve as imagens salvas.
fn run_sd(mut cmd: std::process::Command, dir: &Path, base: &str, count: u32, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<PathBuf>, String> {
    let mut child = cmd
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
