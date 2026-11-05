//! Transcrever áudio e vídeo (fala → texto e legendas) com o Whisper, via whisper.cpp, no próprio PC.
//! O modelo é baixado na primeira vez (conferindo o SHA-256).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::ai::{self, ModelSpec};
use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WModel {
    Rapido,
    Equilibrado,
    Preciso,
}

impl WModel {
    pub const ALL: [WModel; 3] = [WModel::Rapido, WModel::Equilibrado, WModel::Preciso];

    pub fn label(self) -> &'static str {
        match self {
            WModel::Rapido => "Rápido (82 MB)",
            WModel::Equilibrado => "Equilibrado (264 MB)",
            WModel::Preciso => "Preciso (574 MB)",
        }
    }

    pub fn spec(self) -> ModelSpec {
        match self {
            WModel::Rapido => ModelSpec {
                file: "ggml-base-q8_0.bin",
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q8_0.bin",
                sha256: "c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9",
                size: 81_768_585,
                label: "Whisper base (82 MB)",
            },
            WModel::Equilibrado => ModelSpec {
                file: "ggml-small-q8_0.bin",
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q8_0.bin",
                sha256: "49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f",
                size: 264_464_607,
                label: "Whisper small (264 MB)",
            },
            WModel::Preciso => ModelSpec {
                file: "ggml-large-v3-turbo-q5_0.bin",
                url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
                sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
                size: 574_041_195,
                label: "Whisper large-v3 turbo (574 MB)",
            },
        }
    }
}

/// Formatos de saída.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Outputs {
    pub txt: bool,
    pub srt: bool,
    pub vtt: bool,
}

/// Idiomas mais comuns (código do Whisper, nome). "auto" = detectar.
pub const LANGS: &[(&str, &str)] = &[
    ("auto", "Detectar automaticamente"),
    ("pt", "Português"),
    ("en", "Inglês"),
    ("es", "Espanhol"),
    ("fr", "Francês"),
    ("de", "Alemão"),
    ("it", "Italiano"),
    ("ru", "Russo"),
    ("zh", "Chinês"),
    ("ja", "Japonês"),
    ("ko", "Coreano"),
    ("ar", "Árabe"),
    ("hi", "Hindi"),
    ("tr", "Turco"),
    ("pl", "Polonês"),
    ("uk", "Ucraniano"),
    ("nl", "Holandês"),
    ("id", "Indonésio"),
    ("vi", "Vietnamita"),
    ("fa", "Persa"),
];

fn whisper_cli() -> Result<PathBuf, String> {
    tools::find_tool_file(r"whisper\whisper-cli.exe").ok_or_else(|| "whisper-cli.exe não encontrado (pasta bin\\whisper).".to_string())
}

/// Transcreve `src` e devolve (descrição, primeiro arquivo salvo). O texto completo fica no .txt.
#[allow(clippy::too_many_arguments)]
pub fn transcribe(
    src: &Path,
    out_dir: Option<&Path>,
    model: WModel,
    lang: &str,
    translate: bool,
    outputs: Outputs,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let outputs = if !outputs.txt && !outputs.srt && !outputs.vtt { Outputs { txt: true, srt: true, vtt: false } } else { outputs };
    let cli = whisper_cli()?;
    let spec = model.spec();
    // 0–15%: baixar o modelo (só na primeira vez)
    let model_path = ai::ensure_model(&spec, &|p| on_progress(p * 0.15), cancelled)?;
    let work = model_path.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    // nomes só com letras simples na pasta do modelo: o whisper.cpp não lida bem com acentos nos caminhos
    let tag = format!("conver_tmp_{}_{}", std::process::id(), tools::file_size(src));
    let wav = work.join(format!("{tag}.wav"));
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let conv = tools::command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(src)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(&wav)
        .output()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    if !conv.status.success() {
        let _ = std::fs::remove_file(&wav);
        return Err("o arquivo não tem áudio.".into());
    }
    on_progress(0.18);

    let lang = if lang.trim().is_empty() { "auto" } else { lang.trim() };
    let mut cmd = tools::command(&cli);
    cmd.current_dir(&work)
        .args(["-m", spec.file, "-f", &format!("{tag}.wav"), "-of", &tag, "-l", lang, "-pp"])
        .args(["-t", &ai::threads().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if translate {
        cmd.arg("-tr");
    }
    // o .txt é sempre gerado (é dele que sai a prévia do texto)
    cmd.arg("-otxt");
    if outputs.srt {
        cmd.arg("-osrt");
    }
    if outputs.vtt {
        cmd.arg("-ovtt");
    }
    let mut child = cmd.spawn().map_err(|e| format!("não foi possível iniciar o Whisper: {e}"))?;
    let stderr = child.stderr.take().unwrap();
    let mut tail = Vec::new();
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            cleanup(&work, &tag);
            return Err("Cancelado.".into());
        }
        if let Some(p) = line.split("progress =").nth(1).and_then(|v| v.trim().trim_end_matches('%').trim().parse::<f32>().ok()) {
            on_progress(0.18 + 0.8 * (p / 100.0).clamp(0.0, 1.0));
        }
        tail.push(line);
        if tail.len() > 6 {
            tail.remove(0);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&wav);
    if !status.success() {
        cleanup(&work, &tag);
        return Err(format!("falha no Whisper: {}", tail.join(" | ")));
    }

    // move os arquivos gerados para o destino, com o nome do original
    let mut saved = Vec::new();
    let mut words = 0;
    for (want, ext) in [(true, "txt"), (outputs.srt, "srt"), (outputs.vtt, "vtt")] {
        let tmp = work.join(format!("{tag}.{ext}"));
        if !tmp.is_file() {
            continue;
        }
        if !want || (ext == "txt" && !outputs.txt) {
            if ext == "txt" {
                words = std::fs::read_to_string(&tmp).map(|t| t.split_whitespace().count()).unwrap_or(0);
            }
            let _ = std::fs::remove_file(&tmp);
            continue;
        }
        // tira as marcações de som que o Whisper escreve, como "(upbeat music)" ou "[Música]"
        if let Ok(text) = std::fs::read_to_string(&tmp) {
            let _ = std::fs::write(&tmp, tidy(ext, &text));
        }
        if ext == "txt" {
            words = std::fs::read_to_string(&tmp).map(|t| t.split_whitespace().count()).unwrap_or(0);
        }
        let dst = tools::out_path(src, out_dir, "", ext);
        if std::fs::rename(&tmp, &dst).is_err() {
            std::fs::copy(&tmp, &dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
            let _ = std::fs::remove_file(&tmp);
        }
        saved.push(dst);
    }
    if saved.is_empty() {
        return Err("o Whisper não gerou nenhum arquivo.".into());
    }
    on_progress(1.0);
    let names: Vec<String> = saved.iter().map(|p| tools::ext_lower(p).to_uppercase()).collect();
    Ok((format!("{words} palavras • {}", names.join(" + ")), saved.remove(0)))
}

/// Uma linha sem as marcações de som (trechos entre parênteses ou colchetes, ♪) e sem o travessão de troca de quem fala.
pub fn tidy_line(line: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for ch in line.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            '♪' | '♫' => {}
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    t.trim_start_matches(['-', '–', '—']).trim().to_string()
}

/// Limpa um .txt, .srt ou .vtt do Whisper: linhas de texto passam por `tidy_line`; blocos de legenda que ficam
/// vazios saem (e o .srt é renumerado).
// i18n: fora
pub fn tidy(ext: &str, text: &str) -> String {
    if ext == "txt" {
        return text.lines().map(tidy_line).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n") + "\n";
    }
    let text = text.replace("\r\n", "\n");
    let mut out: Vec<String> = Vec::new();
    let mut n = 0;
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(i) = lines.iter().position(|l| l.contains("-->")) else {
            // cabeçalho do .vtt
            if !block.trim().is_empty() && ext == "vtt" {
                out.push(block.trim().to_string());
            }
            continue;
        };
        let body: Vec<String> = lines[i + 1..].iter().map(|l| tidy_line(l)).filter(|l| !l.is_empty()).collect();
        if body.is_empty() {
            continue;
        }
        n += 1;
        let head = if ext == "srt" { format!("{n}\n") } else { String::new() };
        out.push(format!("{head}{}\n{}", lines[i], body.join("\n")));
    }
    out.join("\n\n") + "\n"
}
// i18n: dentro

/// Uma palavra falada, com início e fim em segundos.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Palavra por palavra, com o tempo de cada uma (para as legendas animadas).
pub fn word_timestamps(src: &Path, model: WModel, lang: &str, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<Word>, String> {
    let cli = whisper_cli()?;
    let spec = model.spec();
    let model_path = ai::ensure_model(&spec, &|p| on_progress(p * 0.15), cancelled)?;
    let work = model_path.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    let tag = format!("conver_tmp_w{}_{}", std::process::id(), tools::file_size(src));
    let wav = work.join(format!("{tag}.wav"));
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let conv = tools::command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(src)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(&wav)
        .output()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    if !conv.status.success() {
        let _ = std::fs::remove_file(&wav);
        return Err("o arquivo não tem áudio.".into());
    }
    on_progress(0.18);
    let lang = if lang.trim().is_empty() { "auto" } else { lang.trim() };
    // -ml 1 + -sow: um trecho por palavra; -oj: JSON com o início e o fim (em ms) de cada um
    let mut child = tools::command(&cli)
        .current_dir(&work)
        .args(["-m", spec.file, "-f", &format!("{tag}.wav"), "-of", &tag, "-l", lang, "-pp", "-ml", "1", "-sow", "-oj"])
        .args(["-t", &ai::threads().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o Whisper: {e}"))?;
    let stderr = child.stderr.take().unwrap();
    let mut tail = Vec::new();
    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            cleanup(&work, &tag);
            return Err("Cancelado.".into());
        }
        if let Some(p) = line.split("progress =").nth(1).and_then(|v| v.trim().trim_end_matches('%').trim().parse::<f32>().ok()) {
            on_progress(0.18 + 0.8 * (p / 100.0).clamp(0.0, 1.0));
        }
        tail.push(line);
        if tail.len() > 6 {
            tail.remove(0);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let json = std::fs::read_to_string(work.join(format!("{tag}.json")));
    cleanup(&work, &tag);
    if !status.success() {
        return Err(format!("falha no Whisper: {}", tail.join(" | ")));
    }
    let v: serde_json::Value = serde_json::from_str(&json.map_err(|_| "o Whisper não gerou o resultado.".to_string())?).map_err(|e| e.to_string())?;
    let mut words: Vec<Word> = v["transcription"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|seg| {
            let text = seg["text"].as_str()?.trim().to_string();
            let from = seg["offsets"]["from"].as_f64()? / 1000.0;
            let to = seg["offsets"]["to"].as_f64()? / 1000.0;
            // o Whisper marca sons e música entre colchetes: não são palavras
            (!text.is_empty() && !text.starts_with('[') && !text.starts_with('(')).then_some(Word { start: from, end: to.max(from), text })
        })
        .collect();
    // pontuação solta vira parte da palavra anterior
    let mut i = 1;
    while i < words.len() {
        if words[i].text.chars().all(|c| c.is_ascii_punctuation()) {
            let t = words.remove(i).text;
            words[i - 1].text.push_str(&t);
        } else {
            i += 1;
        }
    }
    on_progress(1.0);
    Ok(words)
}

fn cleanup(work: &Path, tag: &str) {
    for ext in ["wav", "txt", "srt", "vtt", "json"] {
        let _ = std::fs::remove_file(work.join(format!("{tag}.{ext}")));
    }
}
