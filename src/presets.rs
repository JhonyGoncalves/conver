//! Predefinições: configurações favoritas salvas (ferramenta + argumentos), usadas na tela de
//! predefinições, na linha de comando e pelo MCP. Ficam em `predefinicoes.json`, na pasta de dados.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tools;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub name: String,
    /// Nome da ferramenta em `ops::TOOLS`.
    pub tool: String,
    pub args: Value,
    #[serde(default)]
    pub description: String,
}

fn path() -> std::path::PathBuf {
    tools::app_dir().join("predefinicoes.json")
}

/// Predefinições que vêm prontas (o usuário pode apagar ou mudar).
pub fn defaults() -> Vec<Preset> {
    let p = |name: &str, tool: &str, args: Value, description: &str| Preset {
        name: name.into(),
        tool: tool.into(),
        args,
        description: description.into(),
    };
    vec![
        p("Instagram (1080 px)", "image_resize", json!({ "width": 1080, "height": 1350, "keep_aspect": true }), "Fotos com até 1080 × 1350 px, o tamanho do feed."),
        p("Fotos para a web", "image_convert", json!({ "format": "webp" }), "WEBP: bem menor que JPG e PNG, aceito por todos os navegadores."),
        p("WhatsApp (vídeo até 16 MB)", "video_compress", json!({ "target_mb": 15, "resolution": "720p" }), "Cabe no limite de vídeo do WhatsApp."),
        p("Discord (vídeo até 10 MB)", "video_compress", json!({ "target_mb": 9.5, "resolution": "720p" }), "Cabe no limite de envio gratuito do Discord."),
        p("Podcast (MP3 mono)", "audio_compress", json!({ "codec": "mp3", "kbps": 64, "mono": true }), "Voz em MP3 mono de 64 kbps: arquivos pequenos."),
        p("Legendas automáticas", "transcribe", json!({ "model": "fast", "language": "auto", "formats": ["srt"] }), "Cria um .srt com o mesmo nome do vídeo."),
    ]
}

pub fn load() -> Vec<Preset> {
    match std::fs::read_to_string(path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| defaults()),
        Err(_) => defaults(),
    }
}

pub fn store(list: &[Preset]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    let p = path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, text).map_err(|e| format!("não foi possível salvar as predefinições: {e}"))
}

pub fn find(name: &str) -> Option<Preset> {
    let n = name.trim().to_lowercase();
    load().into_iter().find(|p| p.name.to_lowercase() == n)
}

/// Salva (ou substitui, pelo nome) uma predefinição.
pub fn save(p: Preset) -> Result<(), String> {
    let mut list = load();
    match list.iter_mut().find(|x| x.name.eq_ignore_ascii_case(&p.name)) {
        Some(x) => *x = p,
        None => list.push(p),
    }
    store(&list)
}

pub fn remove(name: &str) -> Result<(), String> {
    let mut list = load();
    list.retain(|p| !p.name.eq_ignore_ascii_case(name));
    store(&list)
}
