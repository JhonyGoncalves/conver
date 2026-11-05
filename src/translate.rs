//! Traduzir legendas e dublar: a IA de linguagem local traduz o .srt (ou a transcrição de um vídeo) e, se pedido,
//! o Piper lê cada frase na outra língua; cada fala entra no tempo da legenda (acelerada quando não cabe) e o
//! resultado sai como áudio ou como o vídeo dublado, com o som original baixinho por baixo.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::captions::{self, Cue};
use crate::tts::{self, Voice};
use crate::tts::AnyVoice;
use crate::{ai, tools, video};

// i18n: fora
/// Idiomas de destino: (código, nome em inglês para a IA, nome na tela). Os que têm voz do Piper também dublam.
pub const LANGS: &[(&str, &str, &str)] = &[
    ("pt", "Brazilian Portuguese", "Português"),
    ("en", "English", "Inglês"),
    ("es", "Spanish", "Espanhol"),
    ("fr", "French", "Francês"),
    ("de", "German", "Alemão"),
    ("it", "Italian", "Italiano"),
    ("ru", "Russian", "Russo"),
    ("ja", "Japanese", "Japonês"),
    ("zh", "Simplified Chinese", "Chinês"),
    ("ko", "Korean", "Coreano"),
    ("ar", "Arabic", "Árabe"),
    ("hi", "Hindi", "Híndi"),
];
// i18n: dentro

pub fn lang(code: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    LANGS.iter().find(|l| l.0 == code)
}

/// A primeira voz do Piper para o idioma, se houver.
pub fn voice_for(code: &str) -> Option<AnyVoice> {
    tts::best_voice(code)
}

/// As frases de um arquivo: .srt/.vtt direto; vídeo ou áudio passam antes pela transcrição (Whisper).
pub fn cues_of(src: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<Cue>, String> {
    match tools::ext_lower(src).as_str() {
        "srt" | "vtt" => {
            let text = std::fs::read_to_string(src).map_err(|e| format!("não foi possível ler a legenda: {e}"))?;
            Ok(captions::parse_subtitle(&text))
        }
        _ => {
            let words = crate::transcribe::word_timestamps(src, crate::transcribe::WModel::Rapido, "auto", on_progress, cancelled)?;
            Ok(captions::cues_from_words(&words))
        }
    }
}

// ------------------------------------------------------------------ WAV

/// Lê um WAV PCM de 16 bits mono (como o Piper grava): amostras e taxa.
fn read_wav(p: &Path) -> Result<(Vec<i16>, u32), String> {
    let b = std::fs::read(p).map_err(|e| e.to_string())?;
    if b.len() < 44 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("WAV inválido".into());
    }
    let (mut rate, mut i) = (22_050u32, 12usize);
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let len = u32::from_le_bytes([b[i + 4], b[i + 5], b[i + 6], b[i + 7]]) as usize;
        let body = &b[i + 8..(i + 8 + len).min(b.len())];
        if id == b"fmt " && body.len() >= 8 {
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
        }
        if id == b"data" {
            return Ok((body.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect(), rate));
        }
        i += 8 + len + (len & 1);
    }
    Err("WAV sem dados".into())
}

// i18n: fora
pub(crate) fn write_wav(p: &Path, samples: &[i16], rate: u32) -> Result<(), String> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(p).map_err(|e| e.to_string())?);
    let data = (samples.len() * 2) as u32;
    let mut h = Vec::with_capacity(44);
    h.extend(b"RIFF");
    h.extend((36 + data).to_le_bytes());
    h.extend(b"WAVEfmt ");
    h.extend(16u32.to_le_bytes());
    h.extend(1u16.to_le_bytes()); // PCM
    h.extend(1u16.to_le_bytes()); // mono
    h.extend(rate.to_le_bytes());
    h.extend((rate * 2).to_le_bytes());
    h.extend(2u16.to_le_bytes());
    h.extend(16u16.to_le_bytes());
    h.extend(b"data");
    h.extend(data.to_le_bytes());
    f.write_all(&h).map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    f.write_all(&bytes).map_err(|e| e.to_string())
}
// i18n: dentro

// ------------------------------------------------------------------ dublagem

/// Gera a faixa dublada (WAV) com as falas nos tempos das legendas. Devolve (caminho, falas aceleradas).
fn dub_track(cues: &[Cue], voice: AnyVoice, total: f64, work: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(PathBuf, usize), String> {
    match voice {
        AnyVoice::Piper(v) => speak_cues_piper(cues, v, work, on_progress, cancelled)?,
        AnyVoice::Kokoro(v) => {
            for (i, c) in cues.iter().enumerate() {
                if cancelled() {
                    return Err("Cancelado.".into());
                }
                let s = crate::kokoro::speak(c.text.trim(), v, 1.0, &|_| {}, cancelled)?;
                if !s.is_empty() {
                    let pcm: Vec<i16> = s.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
                    write_wav(&work.join(format!("fala_{i:04}.wav")), &pcm, crate::kokoro::RATE)?;
                }
                on_progress(0.7 * (i + 1) as f32 / cues.len().max(1) as f32);
            }
        }
    }
    join_cues(cues, total, work, on_progress)
}

/// Um arquivo por fala, com um Piper só.
fn speak_cues_piper(cues: &[Cue], voice: &Voice, work: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let exe = tools::find_tool_file("piper\\piper.exe").ok_or("Piper não encontrado (pasta bin\\piper).")?;
    let model = ai::ensure_model(&voice.onnx, &|p| on_progress(p * 0.2), cancelled)?;
    ai::ensure_model(&voice.json, &|_| {}, cancelled)?;
    // um Piper só, uma frase por linha em JSON, cada uma no seu arquivo
    // i18n: fora
    let mut child = tools::command(&exe)
        .args(["--model"])
        .arg(&model)
        .args(["--json-input", "--sentence_silence", "0"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o Piper: {e}"))?;
    {
        let mut stdin = child.stdin.take().unwrap();
        for (i, c) in cues.iter().enumerate() {
            let line = serde_json::json!({ "text": c.text.trim(), "output_file": work.join(format!("fala_{i:04}.wav")).display().to_string() });
            stdin.write_all(format!("{line}\n").as_bytes()).map_err(|e| e.to_string())?;
        }
    }
    // i18n: dentro
    loop {
        if cancelled() {
            let _ = child.kill();
            return Err("Cancelado.".into());
        }
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        let done = (0..cues.len()).filter(|i| work.join(format!("fala_{i:04}.wav")).is_file()).count();
        on_progress(0.2 + 0.5 * done as f32 / cues.len().max(1) as f32);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    Ok(())
}

/// Junta as falas (`fala_0000.wav`…) numa faixa só, cada uma no tempo da legenda, acelerando a que não couber.
fn join_cues(cues: &[Cue], total: f64, work: &Path, on_progress: &dyn Fn(f32)) -> Result<(PathBuf, usize), String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let mut rate = 22_050u32;
    let mut track: Vec<i16> = Vec::new();
    let (mut cursor, mut faster) = (0usize, 0usize);
    for (i, c) in cues.iter().enumerate() {
        let f = work.join(format!("fala_{i:04}.wav"));
        let Ok((mut s, r)) = read_wav(&f) else { continue };
        rate = r;
        if track.is_empty() {
            track = vec![0; (total * r as f64) as usize];
        }
        // o espaço da fala: até a próxima legenda começar (ou o fim da legenda + meio segundo, na última)
        let slot = cues.get(i + 1).map_or(c.end - c.start + 0.5, |n| n.start - c.start).max(0.3);
        let dur = s.len() as f64 / r as f64;
        if dur > slot * 1.03 {
            let tempo = (dur / slot).min(1.5);
            let fast = work.join(format!("fala_{i:04}_r.wav"));
            let ok = tools::command(&ff)
                .args(["-v", "error", "-y", "-i"])
                .arg(&f)
                .args(["-af", &format!("atempo={tempo:.3}")])
                .arg(&fast)
                .status()
                .is_ok_and(|st| st.success());
            if let (true, Ok((s2, _))) = (ok, read_wav(&fast)) {
                s = s2;
                faster += 1;
            }
        }
        let at = ((c.start * r as f64) as usize).max(cursor);
        if track.len() < at + s.len() {
            track.resize(at + s.len(), 0);
        }
        track[at..at + s.len()].copy_from_slice(&s);
        cursor = at + s.len();
        on_progress(0.7 + 0.2 * (i + 1) as f32 / cues.len() as f32);
    }
    if track.iter().all(|s| *s == 0) {
        return Err("a voz não gerou o áudio.".into());
    }
    let out = work.join("dublagem.wav");
    write_wav(&out, &track, rate)?;
    Ok((out, faster))
}

/// Opções de traduzir e dublar.
#[derive(Clone)]
pub struct Opts {
    /// Código do idioma de destino (`None` = não traduzir: só dublar a legenda como está).
    pub to: Option<String>,
    /// Voz para dublar (`None` = não dublar).
    pub voice: Option<AnyVoice>,
    /// Vídeo para receber a dublagem (se não houver, sai só o áudio).
    pub video: Option<PathBuf>,
    /// Volume do som original por baixo da dublagem (0 a 1).
    pub original: f32,
}

/// Traduz (e dubla) um arquivo: .srt/.vtt, ou vídeo/áudio (transcrito antes). Devolve o arquivo principal criado.
pub fn run(src: &Path, out_dir: Option<&Path>, o: &Opts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let is_sub = matches!(tools::ext_lower(src).as_str(), "srt" | "vtt");
    // o vídeo para dublar: o informado, o próprio arquivo (se é vídeo) ou um vídeo de mesmo nome ao lado da legenda
    let video = o.video.clone().or_else(|| {
        if !is_sub {
            return video::probe(src).has_video.then(|| src.to_path_buf());
        }
        let stem = src.file_stem()?.to_string_lossy().to_string();
        let dir = src.parent()?;
        video::VIDEO_EXTS.iter().map(|e| dir.join(format!("{stem}.{e}"))).find(|p| p.is_file())
    });
    let mut cues = cues_of(src, &|p| on_progress(p * 0.2), cancelled)?;
    if cues.is_empty() {
        return Err("nenhuma frase para traduzir.".into());
    }
    let mut made: Vec<PathBuf> = Vec::new();
    let suffix = o.to.as_deref().map(|c| format!("_{c}")).unwrap_or_default();
    if let Some(code) = &o.to {
        let (_, name, _) = lang(code).ok_or_else(|| format!("idioma desconhecido: {code}"))?;
        let llm = crate::llm::Llm::start(&|p| on_progress(0.2 + p * 0.15), cancelled)?;
        let lines: Vec<String> = cues.iter().map(|c| c.text.clone()).collect();
        let done = llm.translate(&lines, name, &|p| on_progress(0.35 + p * 0.35), cancelled)?;
        for (c, t) in cues.iter_mut().zip(done) {
            c.text = t.trim().to_string();
            c.words.clear();
        }
        let srt = tools::out_path(src, out_dir, &suffix, "srt");
        std::fs::write(&srt, captions::to_srt(&cues)).map_err(|e| format!("não foi possível salvar a legenda: {e}"))?;
        made.push(srt);
    }
    if let Some(voice) = o.voice {
        let work = std::env::temp_dir().join(format!("conver_dublagem_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let total = video.as_ref().and_then(|v| video::probe(v).duration).unwrap_or(0.0).max(cues.last().map_or(0.0, |c| c.end + 1.0));
        let base = if o.to.is_some() { 0.7 } else { 0.2 };
        let r = dub_track(&cues, voice, total, &work, &|p| on_progress(base + p * (0.95 - base)), cancelled);
        let result = r.and_then(|(wav, faster)| {
            let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
            let mut cmd = tools::command(&ff);
            cmd.args(["-v", "error", "-y"]);
            let dst = match &video {
                Some(v) => {
                    let dst = tools::out_path(v, out_dir, &format!("_dublado{suffix}"), &crate::media::keep_or_mp4(v));
                    // i18n: fora
                    let mix = if o.original > 0.0 && video::probe(v).has_audio {
                        format!("[0:a]volume={:.2}[a0];[a0][1:a]amix=inputs=2:duration=first:normalize=0[a]", o.original)
                    } else {
                        "[1:a]apad[a]".to_string()
                    };
                    cmd.arg("-i").arg(v).arg("-i").arg(&wav).args(["-filter_complex", &mix, "-map", "0:v", "-map", "[a]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"]);
                    // i18n: dentro
                    dst
                }
                None => {
                    let dst = tools::out_path(src, out_dir, &format!("_dublado{suffix}"), "mp3");
                    cmd.arg("-i").arg(&wav).args(["-c:a", "libmp3lame", "-b:a", "160k"]);
                    dst
                }
            };
            let st = cmd.arg(&dst).status().map_err(|e| e.to_string())?;
            if !st.success() {
                let _ = std::fs::remove_file(&dst);
                return Err("não foi possível juntar a dublagem.".to_string());
            }
            Ok((dst, faster))
        });
        let _ = std::fs::remove_dir_all(&work);
        let (dst, faster) = result?;
        made.insert(0, dst);
        on_progress(1.0);
        let extra = if faster > 0 { format!(" • {faster} fala(s) acelerada(s) para caber") } else { String::new() };
        return Ok((format!("{} frase(s) dublada(s){extra}", cues.len()), made.remove(0)));
    }
    on_progress(1.0);
    let main = made.pop().ok_or("nada para fazer: escolha traduzir e/ou dublar.")?;
    Ok((format!("{} frase(s) traduzida(s)", cues.len()), main))
}
