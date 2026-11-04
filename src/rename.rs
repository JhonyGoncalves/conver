//! Renomear vários arquivos de uma vez.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Manter,
    Minusculas,
    Maiusculas,
}

#[derive(Clone, PartialEq)]
pub struct Rules {
    /// Se não estiver vazio, substitui o nome original.
    pub base: String,
    pub find: String,
    pub replace: String,
    pub prefix: String,
    pub suffix: String,
    pub numbering: bool,
    pub start: u32,
    pub digits: usize,
    pub sep: String,
    pub case: Case,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            base: String::new(),
            find: String::new(),
            replace: String::new(),
            prefix: String::new(),
            suffix: String::new(),
            numbering: false,
            start: 1,
            digits: 3,
            sep: "_".into(),
            case: Case::Manter,
        }
    }
}

const INVALID: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Novo nome (com extensão) do arquivo `i` da lista.
pub fn new_name(path: &Path, i: usize, r: &Rules) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut name = if r.base.trim().is_empty() { stem } else { r.base.trim().to_string() };
    if !r.find.is_empty() {
        name = name.replace(&r.find, &r.replace);
    }
    name = match r.case {
        Case::Manter => name,
        Case::Minusculas => name.to_lowercase(),
        Case::Maiusculas => name.to_uppercase(),
    };
    name = format!("{}{name}{}", r.prefix, r.suffix);
    if r.numbering {
        let n = r.start as usize + i;
        name = format!("{name}{}{n:0width$}", r.sep, width = r.digits);
    }
    let name: String = name.chars().filter(|c| !INVALID.contains(c)).collect();
    format!("{}{ext}", name.trim())
}

/// Problema com o nome novo, se houver (vazio, repetido ou já existente).
pub fn problems(files: &[PathBuf], names: &[String]) -> Vec<Option<&'static str>> {
    let batch: HashSet<&PathBuf> = files.iter().collect();
    let mut seen = HashSet::new();
    files
        .iter()
        .zip(names)
        .map(|(f, n)| {
            let target = f.with_file_name(n);
            let key = target.to_string_lossy().to_lowercase();
            if Path::new(n).file_stem().is_none_or(|s| s.is_empty()) {
                Some("nome vazio")
            } else if !seen.insert(key) {
                Some("nome repetido")
            } else if target.exists() && !batch.contains(&target) && target.to_string_lossy().to_lowercase() != f.to_string_lossy().to_lowercase() {
                Some("já existe um arquivo com esse nome")
            } else {
                None
            }
        })
        .collect()
}

/// Renomeia em duas etapas (passando por nomes temporários) para permitir trocas entre os próprios arquivos.
/// Devolve os pares (novo, antigo) para desfazer.
pub fn apply(files: &[PathBuf], names: &[String]) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let pairs: Vec<(PathBuf, PathBuf)> =
        files.iter().zip(names).map(|(f, n)| (f.clone(), f.with_file_name(n))).filter(|(a, b)| a != b).collect();
    let mut temps = Vec::new();
    for (i, (from, _)) in pairs.iter().enumerate() {
        let tmp = from.with_file_name(format!(".conver_tmp_{}_{i}", std::process::id()));
        if let Err(e) = std::fs::rename(from, &tmp) {
            for (orig, t) in temps.iter().rev() {
                let _ = std::fs::rename(t, orig);
            }
            return Err(format!("{}: {e}", from.display()));
        }
        temps.push((from.clone(), tmp));
    }
    let mut undo = Vec::new();
    for ((from, to), (_, tmp)) in pairs.iter().zip(&temps) {
        match std::fs::rename(tmp, to) {
            Ok(()) => undo.push((to.clone(), from.clone())),
            Err(e) => {
                let _ = std::fs::rename(tmp, from);
                return Err(format!("{}: {e}", to.display()));
            }
        }
    }
    Ok(undo)
}
