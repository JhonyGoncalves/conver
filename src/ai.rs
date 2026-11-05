//! Base comum das ferramentas de IA: ONNX Runtime e os modelos (incluídos no download ou baixados na
//! primeira vez, sempre conferindo o SHA-256).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

use ort::session::Session;
use sha2::{Digest, Sha256};

use crate::tools;

/// Um modelo que o Conver sabe onde baixar.
pub struct ModelSpec {
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    /// Nome mostrado ao usuário.
    pub label: &'static str,
}

/// Pastas onde os modelos podem estar: a do programa (incluídos no download), a de dados do usuário
/// (baixados depois) e, durante o desenvolvimento, a pasta Conver do projeto.
fn model_dirs() -> Vec<PathBuf> {
    vec![
        tools::exe_dir().join("modelos"),
        tools::app_dir().join("modelos"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Conver").join("modelos"),
    ]
}

pub fn find_model(file: &str) -> Option<PathBuf> {
    model_dirs().into_iter().map(|d| d.join(file)).find(|p| p.is_file())
}

/// Dica sob a ferramenta: o modelo já está no PC ou vai ser baixado no primeiro uso.
pub fn model_hint(ui: &mut eframe::egui::Ui, p: &crate::theme::Palette, spec: &ModelSpec) {
    if is_installed(spec) {
        crate::widgets::hint(ui, p, &format!("✔ Modelo {} pronto neste computador.", spec.label));
    } else {
        crate::widgets::hint(ui, p, &format!("O modelo {} é baixado uma vez, na primeira vez que você usar (a assinatura é conferida).", spec.label));
    }
}

pub fn is_installed(spec: &ModelSpec) -> bool {
    find_model(spec.file).is_some()
}

/// Devolve o caminho do modelo, baixando-o se ainda não estiver no computador.
pub fn ensure_model(spec: &ModelSpec, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    if let Some(p) = find_model(spec.file) {
        return Ok(p);
    }
    let dir = tools::app_dir().join("modelos");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = dir.join(spec.file);
    download_verified(spec.url, &dst, spec.sha256, spec.size, spec.label, on_progress, cancelled)?;
    Ok(dst)
}

/// Baixa `url` para `dst` conferindo o SHA-256 durante o download. Se algo der errado (ou não conferir),
/// nada fica em `dst`. `label` aparece nas mensagens de erro.
pub fn download_verified(
    url: &str,
    dst: &std::path::Path,
    sha256: &str,
    size_hint: u64,
    label: &str,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let part = dst.with_file_name(format!("{}.baixando", dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    let resp = crate::integration::agent()
        .get(url)
        .timeout(std::time::Duration::from_secs(60 * 30))
        .call()
        .map_err(|e| format!("não foi possível baixar {label} ({e})."))?;
    let total: u64 = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(size_hint).max(1);
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let result = (|| -> Result<(), String> {
        loop {
            if cancelled() {
                return Err("Cancelado.".into());
            }
            let n = reader.read(&mut buf).map_err(|e| format!("o download parou ({e})."))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            on_progress((done as f32 / total as f32).min(0.999));
        }
        file.flush().map_err(|e| e.to_string())?;
        let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(sha256.trim()) {
            return Err(format!("{label} baixado não confere com a assinatura publicada; nada foi alterado. Tente de novo."));
        }
        Ok(())
    })();
    drop(file);
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(dst);
            std::fs::rename(&part, dst).map_err(|e| e.to_string())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Ambiente do ONNX Runtime (bin\onnxruntime.dll), criado uma vez. Precisa existir antes de qualquer
/// tensor, senão o Windows carregaria a onnxruntime.dll antiga do System32.
pub fn env() -> Result<&'static ort::environment::Environment, String> {
    static ENV: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| {
        let dll = tools::find_tool_file("onnxruntime.dll").ok_or("onnxruntime.dll não encontrado (pasta bin).")?;
        ort::init_from(&dll).map_err(|e| e.to_string())?.with_name("conver").build().map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(Clone::clone)
}

pub fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8)
}

pub fn session(path: &std::path::Path) -> Result<Session, String> {
    let env = env()?;
    Session::builder(env)
        .map_err(|e| e.to_string())?
        .with_intra_threads(threads())
        .map_err(|e| e.to_string())?
        .commit_from_file(path)
        .map_err(|e| format!("não foi possível carregar o modelo: {e}"))
}
