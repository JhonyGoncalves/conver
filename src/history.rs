//! Histórico das últimas conversões e downloads, salvo em %APPDATA%\Conver\history.json.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::tools;

const MAX: usize = 300;

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub time: String,
    pub tool: String,
    pub input: String,
    pub output: PathBuf,
    pub info: String,
}

static HISTORY: LazyLock<Mutex<Vec<Entry>>> = LazyLock::new(|| Mutex::new(load()));

fn file() -> PathBuf {
    tools::app_dir().join("history.json")
}

fn load() -> Vec<Entry> {
    std::fs::read_to_string(file())
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

fn save(list: &[Entry]) {
    let _ = std::fs::create_dir_all(tools::app_dir());
    if let Ok(s) = serde_json::to_string(list) {
        let _ = std::fs::write(file(), s);
    }
}

pub fn record(tool: &str, input: &str, output: &Path, info: &str) {
    let mut list = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    list.insert(
        0,
        Entry {
            time: tools::now_string(),
            tool: tool.into(),
            input: input.into(),
            output: output.to_path_buf(),
            info: info.into(),
        },
    );
    list.truncate(MAX);
    save(&list);
}

pub fn entries() -> Vec<Entry> {
    HISTORY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn clear() {
    let mut list = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    list.clear();
    save(&list);
}
