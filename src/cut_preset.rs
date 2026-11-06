//! Arquivo de cortes: um JSON que diz todos os cortes de um vídeo, para ler em "Cortar trecho", na linha de comando,
//! no MCP e nos fluxos. Ao lado de um vídeo, `<nome do vídeo>.cortes.json` é achado sozinho.
//!
//! ```json
//! {
//!   "output": "separate",
//!   "precise": true,
//!   "cuts": [
//!     { "start": "0:10", "end": "1:00" },
//!     { "start": "0:20", "end": "0:30", "action": "remove" },
//!     { "start": 95.5 }
//!   ]
//! }
//! ```
//!
//! `output` (`together` ou `separate`), `precise` e `end` são opcionais; `action` é `keep` (padrão) ou `remove`. Tempos em
//! segundos ou como 1:30. Também vale só a lista, sem o objeto. `name` e `comment` são anotações livres. Um campo
//! desconhecido é erro: um "inicio" no lugar de "start" não pode virar um corte errado sem aviso.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::media::Cut;
use crate::video::{self, parse_timecode};

/// Extensão do arquivo achado sozinho ao lado do vídeo.
const SIDE: &str = ".cortes.json";

#[derive(Clone, Debug, PartialEq)]
pub struct CutPreset {
    pub cuts: Vec<Cut>,
    pub together: bool,
    /// `None` = quem usa decide.
    pub precise: Option<bool>,
}

/// `<nome do vídeo>.cortes.json` ao lado do vídeo, se existir.
pub fn beside(src: &Path) -> Option<PathBuf> {
    let name = src.file_stem()?.to_string_lossy().into_owned();
    Some(src.with_file_name(format!("{name}{SIDE}"))).filter(|f| f.is_file())
}

/// Nome sugerido ao salvar os cortes de um vídeo.
pub fn file_name_for(src: Option<&Path>) -> String {
    let name = src.and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "video".into());
    format!("{name}{SIDE}")
}

fn time(cut: &Map<String, Value>, key: &str, n: usize) -> Result<Option<f64>, String> {
    let bad = || format!("corte {}: \"{}\" inválido (use segundos ou 1:30).", n, key);
    match cut.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(v)) => v.as_f64().filter(|v| *v >= 0.0).map(Some).ok_or_else(bad),
        Some(Value::String(s)) => parse_timecode(s).map(Some).ok_or_else(bad),
        Some(_) => Err(bad()),
    }
}

fn read_cut(v: &Value, n: usize) -> Result<Cut, String> {
    let c = v.as_object().ok_or_else(|| format!("corte {}: era esperado um objeto com \"start\" e \"end\".", n))?;
    if let Some(k) = c.keys().find(|k| !matches!(k.as_str(), "start" | "end" | "action" | "name" | "comment")) {
        return Err(format!("corte {}: campo desconhecido \"{}\".", n, k));
    }
    let start = time(c, "start", n)?.ok_or_else(|| format!("corte {}: falta \"start\".", n))?;
    let end = time(c, "end", n)?;
    if end.is_some_and(|e| e <= start) {
        return Err(format!("corte {}: o fim precisa ser depois do início.", n));
    }
    let keep = match c.get("action").and_then(Value::as_str) {
        None if c.get("action").is_none() => true,
        Some("keep") => true,
        Some("remove") => false,
        _ => return Err(format!("corte {}: \"action\" deve ser \"keep\" ou \"remove\".", n)),
    };
    Ok(Cut { keep, start, end })
}

/// Lê o texto de um arquivo de cortes.
pub fn parse(text: &str) -> Result<CutPreset, String> {
    let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("JSON inválido: {}", e))?;
    let (list, top) = match &v {
        Value::Array(list) => (list, None),
        Value::Object(o) => {
            if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "cuts" | "output" | "precise" | "name" | "comment")) {
                return Err(format!("campo desconhecido \"{}\".", k));
            }
            (o.get("cuts").and_then(Value::as_array).ok_or("falta a lista \"cuts\".")?, Some(o))
        }
        _ => return Err("falta a lista \"cuts\".".into()),
    };
    if list.is_empty() {
        return Err("a lista \"cuts\" está vazia.".into());
    }
    let cuts = list.iter().enumerate().map(|(i, c)| read_cut(c, i + 1)).collect::<Result<Vec<_>, _>>()?;
    let together = match top.and_then(|o| o.get("output")) {
        None => true,
        Some(v) if v == "together" => true,
        Some(v) if v == "separate" => false,
        Some(_) => return Err("\"output\" deve ser \"together\" ou \"separate\".".into()),
    };
    let precise = match top.and_then(|o| o.get("precise")) {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err("\"precise\" deve ser true ou false.".into()),
    };
    Ok(CutPreset { cuts, together, precise })
}

/// Lê um arquivo de cortes.
pub fn read(path: &Path) -> Result<CutPreset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("não foi possível ler {}: {}", crate::tools::file_name(path), e))?;
    parse(&text)
}

/// O JSON de um conjunto de cortes (o mesmo que `parse` lê).
pub fn to_json(p: &CutPreset) -> String {
    let cuts: Vec<Value> = p
        .cuts
        .iter()
        .map(|c| {
            let mut o = json!({ "start": video::fmt_time_exact(c.start) });
            if let Some(e) = c.end {
                o["end"] = json!(video::fmt_time_exact(e));
            }
            if !c.keep {
                o["action"] = json!("remove");
            }
            o
        })
        .collect();
    let mut o = json!({ "output": if p.together { "together" } else { "separate" } });
    if let Some(b) = p.precise {
        o["precise"] = json!(b);
    }
    o["cuts"] = json!(cuts);
    serde_json::to_string_pretty(&o).unwrap_or_default()
}
