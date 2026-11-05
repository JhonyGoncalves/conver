//! Classificar sons sem internet com o CLAP (LAION, Apache-2.0): compara o som com frases como "This is a sound of
//! applause" e diz qual opção combina mais (fala, música, silêncio, aplausos…). O CLAP só entende inglês: as opções
//! em português passam por um pequeno dicionário de sons comuns (e, quando há, pela IA local de linguagem).
//! O espectrograma (mel, Slaney, dB) segue exatamente o `ClapFeatureExtractor` do Transformers.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use ort::session::Session;
use ort::value::Tensor;
use realfft::RealFftPlanner;

use crate::ai::{self, ModelSpec};
use crate::classify::Choice;
use crate::tools;

pub const AUDIO: ModelSpec = ModelSpec {
    file: "clap-htsat-unfused-audio.onnx",
    url: "https://huggingface.co/Xenova/clap-htsat-unfused/resolve/c28f2883575e590e04d3146ff0713c2448d691ba/onnx/audio_model.onnx",
    sha256: "a1c2b43c44f71e0fa841a4b86700886c199bf87699ea45632c4d831bc6c88957",
    size: 117_528_416,
    label: "CLAP — sons (118 MB)",
};

pub const TEXT: ModelSpec = ModelSpec {
    file: "clap-htsat-unfused-text-int8.onnx",
    url: "https://huggingface.co/Xenova/clap-htsat-unfused/resolve/c28f2883575e590e04d3146ff0713c2448d691ba/onnx/text_model_quantized.onnx",
    sha256: "1a3df8b197e249816e08415fd040434c44762b2eea7eb7bf8a48a0f0bf3c14e5",
    size: 126_603_263,
    label: "CLAP — textos (127 MB)",
};

pub const TOKENIZER: ModelSpec = ModelSpec {
    file: "clap-tokenizer.json",
    url: "https://huggingface.co/Xenova/clap-htsat-unfused/resolve/c28f2883575e590e04d3146ff0713c2448d691ba/tokenizer.json",
    sha256: "dc239041d98de27ffc3975473a1a23e3db4c937b23c138c38bbc66588bd247e5",
    size: 2_108_774,
    label: "CLAP — vocabulário (2 MB)",
};

pub const MODELS: [&ModelSpec; 3] = [&AUDIO, &TEXT, &TOKENIZER];

/// exp(logit_scale_a) do laion/clap-htsat-unfused.
const SCALE: f32 = 18.661_177;
const SR: usize = 48_000;
const SAMPLES: usize = 480_000; // 10 s
const N_FFT: usize = 1024;
const HOP: usize = 480;
const MELS: usize = 64;

pub fn installed() -> bool {
    MODELS.iter().all(|m| ai::find_model(m.file).is_some())
}

pub fn ensure(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let total: u64 = MODELS.iter().map(|m| m.size).sum();
    let mut done = 0u64;
    for m in MODELS {
        let (base, part) = (done as f32 / total as f32, m.size as f32 / total as f32);
        ai::ensure_model(m, &|p| on_progress(base + p * part), cancelled)?;
        done += m.size;
    }
    Ok(())
}

// ------------------------------------------------------------------ tokenizador (BPE em bytes, do GPT-2/RoBERTa)

struct Bpe {
    vocab: HashMap<String, i64>,
    ranks: HashMap<(String, String), usize>,
    byte_char: [char; 256],
}

/// O mapa byte → caractere do GPT-2 (os bytes "imprimíveis" ficam como estão; os outros vão para 256+).
fn bytes_to_unicode() -> [char; 256] {
    let mut out = ['\0'; 256];
    let printable = |b: u32| (33..=126).contains(&b) || (161..=172).contains(&b) || (174..=255).contains(&b);
    let mut n = 0u32;
    for b in 0..256u32 {
        out[b as usize] = if printable(b) {
            char::from_u32(b).unwrap()
        } else {
            n += 1;
            char::from_u32(255 + n).unwrap()
        };
    }
    out
}

fn tokenizer() -> Result<&'static Bpe, String> {
    static T: OnceLock<Result<Bpe, String>> = OnceLock::new();
    T.get_or_init(|| {
        let path = ai::ensure_model(&TOKENIZER, &|_| {}, &|| false)?;
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let vocab = j["model"]["vocab"].as_object().ok_or("vocabulário inválido.")?.iter().filter_map(|(k, v)| Some((k.clone(), v.as_i64()?))).collect();
        let ranks = j["model"]["merges"]
            .as_array()
            .ok_or("vocabulário inválido.")?
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let (a, b) = match m {
                    serde_json::Value::String(s) => s.split_once(' ').map(|(a, b)| (a.to_string(), b.to_string()))?,
                    serde_json::Value::Array(v) => (v.first()?.as_str()?.to_string(), v.get(1)?.as_str()?.to_string()),
                    _ => return None,
                };
                Some(((a, b), i))
            })
            .collect();
        Ok(Bpe { vocab, ranks, byte_char: bytes_to_unicode() })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

/// Divide como a expressão do GPT-2: contrações, letras, números e o resto, cada um com o espaço antes.
fn pre_tokenize(text: &str) -> Vec<String> {
    #[derive(PartialEq, Clone, Copy)]
    enum K {
        L,
        N,
        O,
        S,
    }
    let kind = |c: char| if c.is_whitespace() { K::S } else if c.is_alphabetic() { K::L } else if c.is_numeric() { K::N } else { K::O };
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\'' {
            let rest: String = chars[i + 1..].iter().take(2).collect::<String>().to_lowercase();
            if let Some(c) = ["re", "ve", "ll", "s", "t", "m", "d"].iter().find(|c| rest.starts_with(**c)) {
                out.push(chars[i..i + 1 + c.len()].iter().collect());
                i += 1 + c.len();
                continue;
            }
        }
        let start = i;
        // um espaço só antes de uma palavra vai junto dela
        if chars[i] == ' ' && i + 1 < chars.len() && kind(chars[i + 1]) != K::S {
            i += 1;
        }
        let k = kind(chars[i]);
        if k == K::S {
            // espaços: o último fica para a próxima palavra
            let mut j = i;
            while j < chars.len() && kind(chars[j]) == K::S {
                j += 1;
            }
            let end = if j < chars.len() && j - i > 1 { j - 1 } else { j };
            out.push(chars[start..end].iter().collect());
            i = end;
            continue;
        }
        while i < chars.len() && kind(chars[i]) == k {
            i += 1;
        }
        out.push(chars[start..i].iter().collect());
    }
    out
}

impl Bpe {
    fn encode(&self, text: &str) -> Vec<i64> {
        let mut ids = vec![0]; // <s>
        for piece in pre_tokenize(text) {
            let mut syms: Vec<String> = piece.bytes().map(|b| self.byte_char[b as usize].to_string()).collect();
            loop {
                let best = syms.windows(2).enumerate().filter_map(|(i, w)| self.ranks.get(&(w[0].clone(), w[1].clone())).map(|r| (*r, i))).min();
                let Some((_, i)) = best else { break };
                syms[i] = format!("{}{}", syms[i], syms[i + 1]);
                syms.remove(i + 1);
            }
            ids.extend(syms.iter().map(|s| self.vocab.get(s).copied().unwrap_or(3)));
        }
        ids.truncate(76);
        ids.push(2); // </s>
        ids
    }
}

// ------------------------------------------------------------------ espectrograma

/// Banco de filtros mel na escala Slaney, normalizado (como o `librosa.filters.mel` padrão).
fn mel_filters() -> &'static Vec<Vec<f32>> {
    static F: OnceLock<Vec<Vec<f32>>> = OnceLock::new();
    F.get_or_init(|| {
        let hz_to_mel = |f: f64| if f < 1000.0 { 3.0 * f / 200.0 } else { 15.0 + (f / 1000.0).ln() * 27.0 / 6.4f64.ln() };
        let mel_to_hz = |m: f64| if m < 15.0 { 200.0 * m / 3.0 } else { 1000.0 * ((m - 15.0) * 6.4f64.ln() / 27.0).exp() };
        let (lo, hi) = (hz_to_mel(50.0), hz_to_mel(14_000.0));
        let pts: Vec<f64> = (0..MELS + 2).map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (MELS + 1) as f64)).collect();
        let bins = N_FFT / 2 + 1;
        let fft_f: Vec<f64> = (0..bins).map(|i| i as f64 * (SR as f64 / 2.0) / (bins - 1) as f64).collect();
        (0..MELS)
            .map(|m| {
                let (a, b, c) = (pts[m], pts[m + 1], pts[m + 2]);
                let norm = 2.0 / (c - a);
                fft_f.iter().map(|&f| ((f - a) / (b - a)).min((c - f) / (c - b)).max(0.0) * norm).map(|v| v as f32).collect()
            })
            .collect()
    })
}

/// Log-mel em dB de 10 s de áudio a 48 kHz: 1001 quadros × 64 faixas.
fn log_mel(samples: &[f32]) -> Vec<f32> {
    let pad = N_FFT / 2;
    // centro: o sinal é espelhado nas bordas (modo "reflect")
    let mut x = Vec::with_capacity(samples.len() + 2 * pad);
    x.extend((1..=pad).rev().map(|i| samples[i.min(samples.len() - 1)]));
    x.extend_from_slice(samples);
    x.extend((0..pad).map(|i| samples[samples.len().saturating_sub(2 + i)]));
    let window: Vec<f32> = (0..N_FFT).map(|n| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / N_FFT as f32).cos()).collect();
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let (mut inp, mut spec) = (fft.make_input_vec(), fft.make_output_vec());
    let frames = 1 + samples.len() / HOP;
    let filters = mel_filters();
    let mut out = Vec::with_capacity(frames * MELS);
    for f in 0..frames {
        let s = f * HOP;
        for (k, v) in inp.iter_mut().enumerate() {
            *v = x.get(s + k).copied().unwrap_or(0.0) * window[k];
        }
        let _ = fft.process(&mut inp, &mut spec);
        let power: Vec<f32> = spec.iter().map(|c| c.norm_sqr()).collect();
        for filt in filters {
            let e: f32 = filt.iter().zip(&power).map(|(a, b)| a * b).sum();
            out.push(10.0 * e.max(1e-10).log10());
        }
    }
    out
}

/// Até `windows` trechos de 10 s espalhados pelo arquivo (áudio a 48 kHz, mono). Trechos curtos são repetidos até
/// completar 10 s, como no CLAP ("repeatpad").
fn read_windows(src: &Path, windows: usize) -> Result<Vec<Vec<f32>>, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let dur = crate::video::probe(src).duration.unwrap_or(10.0).max(0.1);
    let n = ((dur / 10.0).ceil() as usize).clamp(1, windows);
    let mut out = Vec::new();
    for i in 0..n {
        let start = if n == 1 { 0.0 } else { (dur - 10.0).max(0.0) * i as f64 / (n - 1) as f64 };
        let o = tools::command(&ff)
            .args(["-v", "error", "-nostdin", "-ss", &format!("{start:.2}"), "-t", "10", "-i"])
            .arg(src)
            .args(["-vn", "-ac", "1", "-ar", "48000", "-f", "f32le", "-"])
            .output()
            .map_err(|e| e.to_string())?;
        let mut s: Vec<f32> = o.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        if s.is_empty() {
            continue;
        }
        if s.len() < SAMPLES {
            let reps = SAMPLES / s.len();
            let one = s.clone();
            for _ in 1..reps {
                s.extend_from_slice(&one);
            }
            s.resize(SAMPLES, 0.0);
        }
        s.truncate(SAMPLES);
        out.push(s);
    }
    if out.is_empty() {
        return Err("o arquivo não tem áudio.".into());
    }
    Ok(out)
}

// ------------------------------------------------------------------ modelos

fn session(spec: &'static ModelSpec, slot: &'static OnceLock<Mutex<Session>>) -> Result<&'static Mutex<Session>, String> {
    if let Some(s) = slot.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(spec, &|_| {}, &|| false)?;
    let s = ai::session(&path)?;
    Ok(slot.get_or_init(|| Mutex::new(s)))
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / n).collect()
}

fn first_vec(out: &ort::session::SessionOutputs, name: &str) -> Result<Vec<f32>, String> {
    let v = out.get(name).ok_or_else(|| format!("saída {name} ausente"))?;
    Ok(v.try_extract_tensor::<f32>().map_err(|e| e.to_string())?.1.to_vec())
}

/// Vetor de um som (média dos trechos analisados).
pub fn audio_embedding(src: &Path) -> Result<Vec<f32>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    let session = session(&AUDIO, &S)?;
    let mut acc = vec![0f32; 512];
    let wins = read_windows(src, 6)?;
    for w in &wins {
        let mel = log_mel(w);
        let frames = mel.len() / MELS;
        let t = Tensor::from_array(([1usize, 1, frames, MELS], mel)).map_err(|e| e.to_string())?;
        let mut s = session.lock().unwrap();
        let out = s.run(ort::inputs!["input_features" => t]).map_err(|e| format!("falha na IA: {e}"))?;
        for (a, v) in acc.iter_mut().zip(normalize(&first_vec(&out, "audio_embeds")?)) {
            *a += v;
        }
    }
    Ok(normalize(&acc))
}

/// Vetor de uma frase em inglês (com cache).
fn text_embedding(text: &str) -> Result<Vec<f32>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<f32>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap().get(text) {
        return Ok(v.clone());
    }
    let session = session(&TEXT, &S)?;
    let ids = tokenizer()?.encode(text);
    let n = ids.len();
    let t = Tensor::from_array(([1usize, n], ids)).map_err(|e| e.to_string())?;
    let mut s = session.lock().unwrap();
    let out = s.run(ort::inputs!["input_ids" => t]).map_err(|e| format!("falha na IA: {e}"))?;
    let v = normalize(&first_vec(&out, "text_embeds")?);
    cache.lock().unwrap().insert(text.to_string(), v.clone());
    Ok(v)
}

// i18n: fora
/// Sons comuns em português → inglês (o CLAP só entende inglês).
const PT_EN: &[(&str, &str)] = &[
    // "speech" foi a frase que melhor reconheceu fala (voz sintética 98%, voz real com música por baixo 41% vs 2% de "people talking")
    ("fala", "speech"), ("voz", "speech"), ("conversa", "speech"), ("pessoa falando", "speech"), ("narração", "narration"), ("narracao", "narration"),
    ("música", "music"), ("musica", "music"), ("canto", "singing"), ("cantando", "singing"), ("instrumental", "instrumental music"),
    ("silêncio", "silence"), ("silencio", "silence"), ("aplausos", "applause"), ("palmas", "applause"), ("risada", "laughter"), ("risos", "laughter"),
    ("ruído", "noise"), ("ruido", "noise"), ("barulho", "noise"), ("chuva", "rain"), ("vento", "wind"), ("trovão", "thunder"),
    ("cachorro", "a dog barking"), ("latido", "a dog barking"), ("gato", "a cat meowing"), ("pássaros", "birds singing"), ("passaros", "birds singing"),
    ("carro", "a car engine"), ("trânsito", "traffic"), ("transito", "traffic"), ("buzina", "a car horn"), ("sirene", "a siren"),
    ("choro", "a baby crying"), ("bebê chorando", "a baby crying"), ("grito", "screaming"), ("gritos", "screaming"), ("tosse", "coughing"),
    ("telefone", "a phone ringing"), ("campainha", "a doorbell"), ("alarme", "an alarm"), ("água", "water"), ("agua", "water"), ("mar", "ocean waves"),
    ("violão", "acoustic guitar"), ("violao", "acoustic guitar"), ("guitarra", "electric guitar"), ("piano", "piano"), ("bateria", "drums"),
    ("teclado", "typing on a keyboard"), ("digitação", "typing on a keyboard"), ("multidão", "a crowd"), ("multidao", "a crowd"), ("tiros", "gunshots"),
    ("explosão", "an explosion"), ("passos", "footsteps"), ("porta", "a door"), ("motor", "an engine"), ("avião", "an airplane"), ("aviao", "an airplane"),
];
// i18n: dentro

/// A opção em inglês para o CLAP: do dicionário de sons comuns, da tradução pela IA local (`translate`) ou como está.
pub fn english(label: &str, translate: Option<&dyn Fn(&str) -> Option<String>>) -> String {
    let l = label.trim().to_lowercase();
    if let Some((_, en)) = PT_EN.iter().find(|(pt, _)| *pt == l) {
        return en.to_string();
    }
    if l.is_ascii() {
        return l;
    }
    translate.and_then(|t| t(&l)).unwrap_or(l)
}

/// Compara o som com as opções. Volta da mais provável para a menos.
pub fn classify_file(src: &Path, labels: &[String], translate: Option<&dyn Fn(&str) -> Option<String>>) -> Result<Vec<Choice>, String> {
    if labels.len() < 2 {
        return Err("informe pelo menos duas opções.".into());
    }
    let a = audio_embedding(src)?;
    let mut logits = Vec::new();
    for l in labels {
        // i18n: fora
        let t = text_embedding(&format!("This is a sound of {}.", english(l, translate)))?;
        // i18n: dentro
        logits.push(t.iter().zip(&a).map(|(x, y)| x * y).sum::<f32>() * SCALE);
    }
    let max = logits.iter().cloned().fold(f32::MIN, f32::max);
    let exps: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    let mut out: Vec<Choice> = labels.iter().zip(&exps).map(|(l, e)| Choice { label: l.clone(), prob: e / sum }).collect();
    out.sort_by(|a, b| b.prob.total_cmp(&a.prob));
    Ok(out)
}

#[cfg(test)]
pub(crate) fn encode_for_test(text: &str) -> Vec<i64> {
    tokenizer().unwrap().encode(text)
}
