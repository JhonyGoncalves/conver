//! Só a legenda, como o DownSub: lista as faixas do vídeo (do autor, automática no idioma original e as traduções
//! automáticas), baixa uma só e monta o arquivo aqui. A automática do YouTube vem "rolando" (cada linha aparece de novo
//! na seguinte), por isso ela é lida no formato json3, com o tempo de cada palavra, e remontada em frases.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::job::{Ctl, LogKind};
use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// Escrita pelo autor (ou pela comunidade).
    Manual,
    /// Gerada pelo YouTube no idioma falado.
    Auto,
    /// Tradução automática do YouTube.
    Translated,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub code: String,
    pub name: String,
    pub source: Source,
    url: String,
    ext: String,
}

// i18n: fora
impl Track {
    /// Identificador usado pela tela e pelo MCP: o código, com "auto:" ou "tr:" quando não é do autor.
    pub fn id(&self) -> String {
        match self.source {
            Source::Manual => self.code.clone(),
            Source::Auto => format!("auto:{}", self.code),
            Source::Translated => format!("tr:{}", self.code),
        }
    }
}

// i18n: dentro
#[derive(Clone, Debug)]
pub struct Video {
    pub title: String,
    pub language: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Srt,
    Txt,
    Vtt,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Srt => "srt",
            Format::Txt => "txt",
            Format::Vtt => "vtt",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: u64,
    pub end: u64,
    pub text: String,
}

// i18n: fora
const NATIVE_FIXES: &[(&str, &str)] = &[("iw", "he"), ("jw", "jv"), ("in", "id"), ("zh-Hans", "zh-CN"), ("zh-Hant", "zh-TW"), ("fil", "fil-PH")];
// i18n: dentro

/// Nome do idioma na própria língua ("Português (Brasil)", "English", "日本語"), pelo Windows.
pub fn native_name(code: &str, fallback: &str) -> String {
    let tag = NATIVE_FIXES.iter().find(|(a, _)| *a == code).map(|(_, b)| *b).unwrap_or(code);
    let name = windows::Globalization::Language::CreateLanguage(&windows::core::HSTRING::from(tag))
        .and_then(|l| l.NativeName())
        .map(|n| n.to_string())
        .unwrap_or_default();
    let name = if name.is_empty() || name.eq_ignore_ascii_case(tag) { fallback.to_string() } else { name };
    let mut c = name.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => code.to_string(),
    }
}

fn best_format(list: &[Value]) -> Option<(String, String)> {
    for want in ["json3", "vtt", "srt"] {
        if let Some(f) = list.iter().find(|f| f["ext"].as_str() == Some(want)) {
            if let Some(u) = f["url"].as_str() {
                return Some((u.to_string(), want.to_string()));
            }
        }
    }
    None
}

// i18n: fora
fn track_name(list: &[Value], code: &str) -> String {
    let yt = list.iter().find_map(|f| f["name"].as_str()).unwrap_or(code);
    let yt = yt.trim_end_matches(" (Original)").trim_end_matches(" (auto-generated)");
    native_name(code.trim_end_matches("-orig"), yt)
}

// i18n: dentro
fn lang_rank(code: &str) -> u8 {
    if code.starts_with("pt") {
        0
    } else if code.starts_with("en") {
        1
    } else {
        2
    }
}

/// As faixas de um vídeo a partir do JSON do yt-dlp (`-J`).
pub fn tracks_from_info(info: &Value) -> Video {
    let title = info["title"].as_str().unwrap_or("legenda").to_string();
    let language = info["language"].as_str().map(str::to_string);
    let mut tracks = Vec::new();
    if let Some(m) = info["subtitles"].as_object() {
        for (code, list) in m {
            if code == "live_chat" || code == "danmaku" {
                continue;
            }
            let list = list.as_array().map(Vec::as_slice).unwrap_or(&[]);
            if let Some((url, ext)) = best_format(list) {
                tracks.push(Track { code: code.clone(), name: track_name(list, code), source: Source::Manual, url, ext });
            }
        }
    }
    if let Some(m) = info["automatic_captions"].as_object() {
        // a do idioma falado vem como "<código>-orig"; sem ela, é a do idioma do vídeo. Com a dublagem automática do
        // YouTube pode haver mais de uma ("en-orig" da faixa dublada e "pt-orig"): vale a do idioma do vídeo.
        let base = |c: &str| c.split('-').next().unwrap_or(c).to_lowercase();
        let origs: Vec<&String> = m.keys().filter(|k| k.ends_with("-orig")).collect();
        let lang_base = language.as_deref().map(base);
        let orig: Option<String> = origs
            .iter()
            .find(|k| lang_base.is_some() && Some(base(k)) == lang_base)
            .or_else(|| origs.first())
            .map(|k| k.to_string())
            .or_else(|| language.clone().filter(|l| m.contains_key(l)));
        let spoken = orig.as_deref().map(|o| o.trim_end_matches("-orig").to_string());
        for (code, list) in m {
            let list = list.as_array().map(Vec::as_slice).unwrap_or(&[]);
            let Some((url, ext)) = best_format(list) else { continue };
            let source = if Some(code) == orig.as_ref() {
                Source::Auto
            } else if spoken.as_deref() == Some(code.as_str()) {
                continue; // a mesma fala "traduzida" para o próprio idioma
            } else if code.ends_with("-orig") && m.contains_key(code.trim_end_matches("-orig")) {
                continue; // a "original" de uma faixa dublada: a tradução para esse idioma já está na lista
            } else {
                Source::Translated
            };
            let clean = code.trim_end_matches("-orig").to_string();
            tracks.push(Track { code: clean, name: track_name(list, code), source, url, ext });
        }
    }
    let order = |s: Source| match s {
        Source::Manual => 0,
        Source::Auto => 1,
        Source::Translated => 2,
    };
    tracks.sort_by(|a, b| (order(a.source), lang_rank(&a.code), a.name.to_lowercase()).cmp(&(order(b.source), lang_rank(&b.code), b.name.to_lowercase())));
    Video { title, language, tracks }
}

/// Escolhe a faixa: `want` é um id da lista ("pt-BR", "auto:en", "tr:pt"), um código de idioma ("pt") ou vazio/"original"
/// (o idioma falado: a do autor, senão a automática). Tradução automática só quando pedida.
pub fn pick<'a>(v: &'a Video, want: &str) -> Option<&'a Track> {
    let want = want.trim();
    if let Some(t) = v.tracks.iter().find(|t| t.id() == want) {
        return Some(t);
    }
    let base = |c: &str| c.split('-').next().unwrap_or(c).to_lowercase();
    let not_tr = |t: &&Track| t.source != Source::Translated;
    if !(want.is_empty() || want == "auto" || want == "original") {
        let lang = base(want.trim_start_matches(PREFIXES[0]).trim_start_matches(PREFIXES[1]));
        if let Some(t) = v.tracks.iter().filter(not_tr).find(|t| base(&t.code) == lang) {
            return Some(t);
        }
        // pediu um idioma que o vídeo não tem: a tradução automática para ele
        if let Some(t) = v.tracks.iter().find(|t| t.source == Source::Translated && base(&t.code) == lang) {
            return Some(t);
        }
    }
    let spoken = v.tracks.iter().find(|t| t.source == Source::Auto).map(|t| base(&t.code)).or_else(|| v.language.as_deref().map(base));
    if let Some(s) = spoken {
        if let Some(t) = v.tracks.iter().filter(not_tr).find(|t| base(&t.code) == s) {
            return Some(t);
        }
    }
    v.tracks.iter().find(not_tr)
}

/// Lista os vídeos do link (um, ou todos os da playlist) com as faixas de legenda de cada um.
pub fn list(url: &str, playlist: bool, cookies: Option<&str>) -> Result<Vec<Video>, String> {
    let mut cmd = crate::download::ytdlp_command(cookies)?;
    cmd.args(["-J", "--skip-download", "--ignore-errors"]).arg(if playlist { "--yes-playlist" } else { "--no-playlist" });
    cmd.arg("--").arg(url);
    let out = cmd.output().map_err(|e| format!("não foi possível iniciar o yt-dlp: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let info: Value = serde_json::from_str(text.trim()).map_err(|_| {
        let err = String::from_utf8_lossy(&out.stderr);
        err.lines().rev().find(|l| l.starts_with("ERROR")).map(|l| l.trim_start_matches("ERROR: ").to_string()).unwrap_or_else(|| "não foi possível ler as informações do vídeo.".into())
    })?;
    let mut videos = Vec::new();
    if let Some(entries) = info["entries"].as_array() {
        videos.extend(entries.iter().filter(|e| e.is_object()).map(tracks_from_info));
    } else {
        videos.push(tracks_from_info(&info));
    }
    Ok(videos)
}

// i18n: fora
const UA: (&str, &str) = ("User-Agent", "Mozilla/5.0");
const K_APPEND: &str = "aAppend";
const PREFIXES: [&str; 2] = ["auto:", "tr:"];
const K_OFFSET: &str = "tOffsetMs";
const K_START: &str = "tStartMs";
const K_DUR: &str = "dDurationMs";
// i18n: dentro

/// Baixa o texto da faixa. O YouTube às vezes responde 429 (pedidos demais), sobretudo nas traduções: tenta de novo.
fn fetch(track: &Track, ctl: &Ctl) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60)).build();
    let waits = [0u64, 4, 10, 20];
    for (i, w) in waits.iter().enumerate() {
        for _ in 0..*w * 4 {
            if ctl.cancelled() {
                return Err("Cancelado.".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        match agent.get(&track.url).set(UA.0, UA.1).call() {
            Ok(r) => return r.into_string().map_err(|e| e.to_string()),
            Err(ureq::Error::Status(429, _)) if i + 1 < waits.len() => {
                ctl.status(format!("O YouTube pediu uma pausa; tentando de novo em {} s…", waits[i + 1]));
            }
            Err(ureq::Error::Status(429, _)) => {
                return Err(if track.source == Source::Translated {
                    "o YouTube limitou os pedidos de tradução automática. Tente de novo em alguns minutos, ou baixe a do idioma original e use Traduzir e dublar.".into()
                } else {
                    "o YouTube limitou os pedidos. Tente de novo em alguns minutos.".into()
                })
            }
            Err(ureq::Error::Status(code, _)) => return Err(format!("erro {} ao baixar a legenda", code)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("o download da legenda falhou.".into())
}

// ------------------------------------------------------------------ leitura

/// json3 do YouTube. Nas automáticas (palavras com `tOffsetMs` e linhas em `aAppend`) remonta as frases pelas palavras;
/// nas do autor mantém cada fala como veio.
pub fn parse_json3(text: &str) -> Result<Vec<Cue>, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "a legenda veio num formato inesperado.".to_string())?;
    let events = v["events"].as_array().cloned().unwrap_or_default();
    let rolling = events.iter().any(|e| e[K_APPEND].as_i64() == Some(1) || e["segs"].as_array().is_some_and(|s| s.iter().any(|g| g.get(K_OFFSET).is_some())));
    if rolling {
        let mut words: Vec<(u64, String)> = Vec::new();
        for e in &events {
            let start = e[K_START].as_u64().unwrap_or(0);
            let Some(segs) = e["segs"].as_array() else { continue };
            for g in segs {
                let t = g["utf8"].as_str().unwrap_or("");
                if t.trim().is_empty() {
                    continue;
                }
                words.push((start + g[K_OFFSET].as_u64().unwrap_or(0), t.replace('\n', " ")));
            }
        }
        words.sort_by_key(|w| w.0);
        return Ok(regroup(&words));
    }
    let mut cues = Vec::new();
    for e in &events {
        let Some(segs) = e["segs"].as_array() else { continue };
        let text: String = segs.iter().filter_map(|g| g["utf8"].as_str()).collect();
        let text = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        if text.is_empty() {
            continue;
        }
        let start = e[K_START].as_u64().unwrap_or(0);
        let end = start + e[K_DUR].as_u64().unwrap_or(2000);
        cues.push(Cue { start, end, text });
    }
    Ok(tidy(cues))
}

const MAX_CHARS: usize = 84;
const LINE: usize = 42;
const MAX_MS: u64 = 6000;

/// Palavras soltas → falas de até duas linhas, quebrando no fim da frase, numa pausa ou no limite de tamanho.
pub fn regroup(words: &[(u64, String)]) -> Vec<Cue> {
    let mut cues: Vec<Cue> = Vec::new();
    let mut cur = String::new();
    let mut start = 0u64;
    for (i, (t, w)) in words.iter().enumerate() {
        let piece = w.trim();
        if cur.is_empty() {
            start = *t;
            cur = piece.to_string();
        } else {
            cur.push(' ');
            cur.push_str(piece);
        }
        let next = words.get(i + 1);
        let gap = next.map(|(n, _)| n.saturating_sub(*t)).unwrap_or(u64::MAX);
        let next_len = next.map(|(_, n)| n.trim().chars().count() + 1).unwrap_or(0);
        let sentence = piece.ends_with(['.', '?', '!', '…']) && cur.chars().count() >= 24;
        let full = cur.chars().count() + next_len > MAX_CHARS;
        let long = next.is_some_and(|(n, _)| n.saturating_sub(start) > MAX_MS);
        // pausa só quebra se a fala já tiver corpo (senão sobra uma palavra solta)
        let pause = gap > 3000 || (gap > 1500 && cur.chars().count() >= 15);
        if next.is_none() || sentence || full || long || pause {
            let end = match next {
                Some((n, _)) => (*n).min(*t + 1500).max(*t + 300),
                None => *t + 1500,
            };
            cues.push(Cue { start, end, text: two_lines(&cur) });
            cur.clear();
        }
    }
    for i in 1..cues.len() {
        let next = cues[i].start;
        let c = &mut cues[i - 1];
        c.end = c.end.min(next).max(c.start + 1);
    }
    cues
}

/// Quebra em duas linhas perto do meio quando passa de uma linha.
fn two_lines(text: &str) -> String {
    let n = text.chars().count();
    if n <= LINE {
        return text.to_string();
    }
    let half = n / 2;
    let mut best: Option<usize> = None;
    for (ci, (bi, c)) in text.char_indices().enumerate() {
        if c == ' ' && best.is_none_or(|b: usize| ci.abs_diff(half) < text[..b].chars().count().abs_diff(half)) {
            best = Some(bi);
        }
    }
    match best {
        Some(b) => format!("{}\n{}", &text[..b], &text[b + 1..]),
        None => text.to_string(),
    }
}

fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (h.parse::<u64>().ok()?, m.parse::<u64>().ok()?, *s),
        [m, s] => (0, m.parse::<u64>().ok()?, *s),
        _ => return None,
    };
    let secs: f64 = sec.parse().ok()?;
    Some((h * 3600 + m * 60) * 1000 + (secs * 1000.0).round() as u64)
}

// i18n: fora
fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' if inside => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ").replace("&#39;", "'").replace("&quot;", "\"")
}

// i18n: dentro
/// .vtt ou .srt (de qualquer site). Tira as marcações e as linhas repetidas das legendas que "rolam".
pub fn parse_text(text: &str) -> Vec<Cue> {
    let text = text.replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let mut it = lines[ti].split("-->");
        let (Some(a), Some(b)) = (it.next(), it.next()) else { continue };
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b.split_whitespace().next().unwrap_or(""))) else { continue };
        let body: Vec<String> = lines[ti + 1..].iter().map(|l| strip_tags(l).trim().to_string()).filter(|l| !l.is_empty()).collect();
        if body.is_empty() {
            continue;
        }
        cues.push(Cue { start, end, text: body.join("\n") });
    }
    tidy(cues)
}

/// Tira o que repete: falas curtíssimas de transição, linhas que já estavam na fala anterior e falas iguais seguidas.
pub fn tidy(cues: Vec<Cue>) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    for mut c in cues {
        if c.end <= c.start + 50 {
            continue;
        }
        if let Some(prev) = out.last() {
            let before: Vec<&str> = prev.text.lines().collect();
            let lines: Vec<String> = c.text.lines().filter(|l| !before.contains(l)).map(str::to_string).collect();
            if lines.is_empty() {
                // a mesma fala de novo: só estende a anterior
                let last = out.last_mut().unwrap();
                last.end = last.end.max(c.end);
                continue;
            }
            c.text = lines.join("\n");
        }
        if let Some(prev) = out.last_mut() {
            if prev.end > c.start {
                prev.end = c.start.max(prev.start + 1);
            }
        }
        out.push(c);
    }
    out
}

// ------------------------------------------------------------------ escrita

// i18n: fora
fn stamp(ms: u64, sep: char) -> String {
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn render(cues: &[Cue], f: Format) -> String {
    let mut s = String::new();
    match f {
        Format::Srt => {
            for (i, c) in cues.iter().enumerate() {
                s += &format!("{}\n{} --> {}\n{}\n\n", i + 1, stamp(c.start, ','), stamp(c.end, ','), c.text);
            }
        }
        Format::Vtt => {
            s += "WEBVTT\n\n";
            for c in cues {
                s += &format!("{} --> {}\n{}\n\n", stamp(c.start, '.'), stamp(c.end, '.'), c.text);
            }
        }
        Format::Txt => {
            // texto corrido; parágrafo novo depois de uma pausa longa
            let mut last_end = 0u64;
            for c in cues {
                let line = c.text.replace('\n', " ");
                if s.is_empty() {
                } else if c.start.saturating_sub(last_end) > 2500 {
                    s += "\n\n";
                } else {
                    s.push(' ');
                }
                s += &line;
                last_end = c.end;
            }
            s.push('\n');
        }
    }
    s
}

fn safe_name(title: &str) -> String {
    let s: String = title.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s: String = s.trim().trim_end_matches('.').chars().take(120).collect();
    if s.is_empty() { "legenda".into() } else { s }
}

// i18n: dentro
fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    p
}

/// Grava a faixa escolhida de um vídeo já listado.
pub fn save(v: &Video, track: &Track, f: Format, out_dir: &Path, ctl: &Ctl) -> Result<PathBuf, String> {
    let raw = fetch(track, ctl)?;
    let cues = if track.ext == "json3" { parse_json3(&raw)? } else { parse_text(&raw) };
    if cues.is_empty() {
        return Err("a legenda veio vazia.".into());
    }
    let path = free_path(out_dir, &format!("{}.{}", safe_name(&v.title), track.code), f.ext());
    std::fs::write(&path, render(&cues, f)).map_err(|e| format!("não foi possível gravar a legenda: {}", e))?;
    Ok(path)
}

/// O "Só a legenda" inteiro: lista, escolhe (`want`, como em `pick`) e grava uma legenda por vídeo.
pub fn download(url: &str, want: &str, f: Format, playlist: bool, cookies: Option<&str>, out_dir: &Path, ctl: &Ctl) -> Result<Vec<PathBuf>, String> {
    ctl.status("Procurando as legendas…");
    let videos = list(url, playlist, cookies)?;
    download_listed(&videos, want, f, out_dir, ctl)
}

/// Como `download`, com os vídeos já listados (a tela listou ao mostrar os idiomas).
pub fn download_listed(videos: &[Video], want: &str, f: Format, out_dir: &Path, ctl: &Ctl) -> Result<Vec<PathBuf>, String> {
    let n = videos.len();
    let mut files = Vec::new();
    let mut last_err = None;
    for (i, v) in videos.iter().enumerate() {
        if ctl.cancelled() {
            return Err("Cancelado.".into());
        }
        let Some(track) = pick(v, want) else {
            ctl.log(LogKind::Err, format!("✖ {}: {}", v.title, crate::i18n::tr("este vídeo não tem legenda.")));
            last_err = Some("este vídeo não tem legenda.".to_string());
            continue;
        };
        ctl.status(format!("Baixando a legenda: {}", v.title));
        match save(v, track, f, out_dir, ctl) {
            Ok(p) => {
                ctl.log(LogKind::Ok, format!("✔ {}", tools::file_name(&p)));
                files.push(p);
            }
            Err(e) if e == "Cancelado." => return Err(e),
            Err(e) => {
                ctl.log(LogKind::Err, format!("✖ {}: {e}", v.title));
                last_err = Some(e);
            }
        }
        ctl.progress((i + 1) as f32 / n.max(1) as f32);
    }
    if files.is_empty() {
        return Err(last_err.unwrap_or_else(|| "nenhuma legenda encontrada.".into()));
    }
    Ok(files)
}
