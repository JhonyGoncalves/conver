//! Melhores momentos: transcreve o vídeo (Whisper, com o tempo de cada palavra), divide a fala em janelas de ~10
//! minutos e pede para uma IA escolher os trechos mais fortes de cada uma, com título e nota. Os escolhidos viram um
//! `<vídeo>.cortes.json` (que "Cortar trecho" abre sozinho, para revisar) e, se pedido, já são cortados.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::captions::{Cue, cues_from_words};
use crate::media::{Cut, CutMode};
use crate::tools;

/// Frases mandadas por vez para a IA (cabe no contexto do modelo local).
const WINDOW: usize = 90;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Brain {
    /// Qwen no próprio PC (baixado na primeira vez).
    #[default]
    Local,
    /// A IA configurada em Configurações (Claude, DeepSeek, OpenAI…).
    Api,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Opts {
    pub count: usize,
    pub min_len: f64,
    pub max_len: f64,
    pub brain: Brain,
    /// Já corta os momentos (um arquivo por momento), além de salvar o .cortes.json.
    pub cut: bool,
    pub model: crate::transcribe::WModel,
}

impl Default for Opts {
    fn default() -> Self {
        Self { count: 5, min_len: 20.0, max_len: 60.0, brain: Brain::Local, cut: false, model: crate::transcribe::WModel::Equilibrado }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Moment {
    pub start: f64,
    pub end: f64,
    pub title: String,
    pub score: f64,
}

// i18n: fora
const SYSTEM: &str = "You are a video editor who finds the best moments of long videos (podcasts, lives, interviews, classes) to post as short clips. A good moment stands alone without context, has a clear idea, a strong opinion, a story, humor, a surprise or useful advice, and ends at a natural point. Answer only with JSON.";

fn prompt(cues: &[Cue], first: usize, wanted: usize, min_len: f64, max_len: f64) -> String {
    let mut s = format!(
        "Below are numbered lines of a transcript, with the time of each line. Choose up to {wanted} best moments. Each moment is a range of lines (start and end line numbers, inclusive) lasting between {min_len:.0} and {max_len:.0} seconds. Give each one a short catchy title in the language of the transcript and a score from 1 to 10.\n\n"
    );
    for (i, c) in cues.iter().enumerate() {
        s.push_str(&format!("[{}] {} {}\n", first + i, crate::video::fmt_time(c.start), c.text.trim()));
    }
    s
}

fn schema() -> Value {
    json!({ "type": "object", "properties": { "moments": { "type": "array", "items": { "type": "object", "properties": {
        "start": { "type": "integer" }, "end": { "type": "integer" }, "title": { "type": "string" }, "score": { "type": "integer" } },
        "required": ["start", "end", "title", "score"] } } }, "required": ["moments"] })
}
// i18n: dentro

/// Lê a resposta da IA (JSON, talvez com texto em volta) e transforma em momentos dentro das regras de duração.
pub fn parse_moments(answer: &str, cues: &[Cue], min_len: f64, max_len: f64) -> Vec<Moment> {
    let (Some(a), Some(b)) = (answer.find('{'), answer.rfind('}')) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&answer[a..=b]) else { return Vec::new() };
    let mut out = Vec::new();
    for m in v["moments"].as_array().into_iter().flatten() {
        let (Some(s), Some(e)) = (m["start"].as_u64(), m["end"].as_u64()) else { continue };
        let (mut s, mut e) = (s as usize, (e as usize).max(s as usize));
        if s >= cues.len() {
            continue;
        }
        e = e.min(cues.len() - 1);
        // curto demais: estica pelas frases seguintes; longo demais: encurta pelo fim
        while cues[e].end - cues[s].start < min_len && e + 1 < cues.len() {
            e += 1;
        }
        while cues[e].end - cues[s].start > max_len && e > s {
            e -= 1;
        }
        if cues[e].end - cues[s].start < min_len * 0.6 {
            if s == 0 {
                continue;
            }
            s -= 1;
        }
        let title = m["title"].as_str().unwrap_or_default().trim().to_string();
        out.push(Moment { start: cues[s].start, end: cues[e].end, title, score: m["score"].as_f64().unwrap_or(5.0) });
    }
    out
}

/// Os melhores sem se sobrepor, em ordem de tempo.
pub fn pick(mut all: Vec<Moment>, count: usize) -> Vec<Moment> {
    all.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut chosen: Vec<Moment> = Vec::new();
    for m in all {
        if chosen.len() >= count {
            break;
        }
        if chosen.iter().all(|c| m.end <= c.start || m.start >= c.end) {
            chosen.push(m);
        }
    }
    chosen.sort_by(|a, b| a.start.total_cmp(&b.start));
    chosen
}

/// O .cortes.json dos momentos (um arquivo por momento, corte Inteligente), com o título de cada um.
pub fn cuts_json(moments: &[Moment]) -> String {
    let cuts: Vec<Value> = moments
        .iter()
        // i18n: fora
        .map(|m| json!({ "start": crate::video::fmt_time_exact(m.start), "end": crate::video::fmt_time_exact(m.end), "name": m.title, "comment": format!("score {}", m.score) }))
        // i18n: dentro
        .collect();
    serde_json::to_string_pretty(&json!({ "output": "separate", "mode": "smart", "cuts": cuts })).unwrap_or_default()
}

/// Acha os melhores momentos de um vídeo ou áudio. Devolve o resumo e o .cortes.json (ou o último corte, com `cut`).
pub fn find(src: &Path, out_dir: Option<&Path>, o: Opts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let words = crate::transcribe::word_timestamps(src, o.model, "auto", &|p| on_progress(p * 0.45), cancelled)?;
    let cues = cues_from_words(&words);
    if cues.len() < 3 {
        return Err("quase não há fala neste arquivo: os melhores momentos são escolhidos pelo que é dito.".into());
    }
    let windows: Vec<(usize, &[Cue])> = cues.chunks(WINDOW).enumerate().map(|(i, c)| (i * WINDOW, c)).collect();
    // de cada janela, uns a mais do que o necessário; depois ficam os de nota maior
    let per = (o.count * 2).div_ceil(windows.len()).clamp(2, 5);
    let mut all = Vec::new();
    let ask: Box<dyn Fn(&str) -> Result<String, String>> = match o.brain {
        Brain::Local => {
            let llm = crate::llm::Llm::start(&|_| {}, cancelled)?;
            let schema = schema();
            Box::new(move |user: &str| llm.chat(SYSTEM, user, Some(&schema), 900))
        }
        Brain::Api => {
            let cfg = crate::ai_api::Settings::load().config()?;
            Box::new(move |user: &str| crate::ai_api::ask(&cfg, SYSTEM, &[crate::ai_api::Part::Text(user.to_string())], 1500))
        }
    };
    for (n, (first, chunk)) in windows.iter().enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        let answer = ask(&prompt(chunk, *first, per, o.min_len, o.max_len))?;
        all.extend(parse_moments(&answer, &cues, o.min_len, o.max_len).into_iter().filter(|m| m.start >= cues[*first].start - 1.0));
        on_progress(0.45 + 0.4 * (n + 1) as f32 / windows.len() as f32);
    }
    let chosen = pick(all, o.count);
    if chosen.is_empty() {
        return Err("a IA não devolveu nenhum momento válido; tente de novo ou use outra IA.".into());
    }
    // o .cortes.json vai ao lado do vídeo, onde "Cortar trecho" o acha sozinho; se já existir um, não é sobrescrito
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut json_path = src.with_file_name(format!("{stem}.cortes.json"));
    if json_path.exists() {
        json_path = tools::unique_path(src.with_file_name(format!("{stem}.momentos.json")));
    }
    std::fs::write(&json_path, cuts_json(&chosen)).map_err(|e| format!("não foi possível salvar: {e}"))?;
    let list = chosen.iter().map(|m| format!("{} {}", crate::video::fmt_time(m.start), m.title)).collect::<Vec<_>>().join(" | ");
    if !o.cut {
        on_progress(1.0);
        return Ok((format!("{} momento(s): {}", chosen.len(), list), json_path));
    }
    let cuts: Vec<Cut> = chosen.iter().map(|m| Cut { keep: true, start: m.start, end: Some(m.end) }).collect();
    let (_, last) = crate::media::cut(src, out_dir, &cuts, false, CutMode::Inteligente, &|p| on_progress(0.85 + p * 0.15), cancelled)?;
    Ok((format!("{} momento(s) cortados: {}", chosen.len(), list), last))
}
