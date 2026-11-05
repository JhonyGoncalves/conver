//! Classificar imagens sem internet com o SigLIP 2 (Google, Apache-2.0): o usuário escreve as opções (em qualquer
//! um dos 109 idiomas do modelo) e o Conver diz qual combina mais com cada imagem, com a probabilidade de cada uma.
//! Dois codificadores ONNX (imagem em 4 bits, texto em int8; o int8 da imagem usa ConvInteger, que o ONNX Runtime não roda na CPU): um transforma a imagem num vetor, o outro transforma cada opção num vetor; a
//! semelhança entre eles, na escala aprendida pelo modelo, vira a pontuação. O tokenizador (BPE do Gemma, com
//! "byte fallback") é implementado aqui, para não depender de outra biblioteca.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

use crate::ai::{self, ModelSpec};

pub const VISION: ModelSpec = ModelSpec {
    file: "siglip2-vision-q4.onnx",
    url: const_format_url::VISION,
    sha256: "03e61be280fb24114facd87a1e1e6b2b654aad49845686da2f930c72740914aa",
    size: 63_267_466,
    label: "SigLIP 2 — imagens (63 MB)",
};

pub const TEXT: ModelSpec = ModelSpec {
    file: "siglip2-text-int8.onnx",
    url: const_format_url::TEXT,
    sha256: "3a0603d3a00c05a80a6ded4743c16aaac7b1e62cdcc7e362e7ce418659b96400",
    size: 283_438_275,
    label: "SigLIP 2 — textos (283 MB)",
};

pub const TOKENIZER: ModelSpec = ModelSpec {
    file: "siglip2-tokenizer.json",
    url: const_format_url::TOKENIZER,
    sha256: "cb9140fae3ac5122c972d37adf83e1248471a38147ad76f8215c8872c6fd8322",
    size: 34_363_039,
    label: "SigLIP 2 — vocabulário (34 MB)",
};

/// As URLs completas (o `concat!` não aceita constantes, por isso ficam escritas por inteiro).
mod const_format_url {
    // i18n: fora
    pub const VISION: &str = "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/ba1f3b0843f24bc5417d38e19c37b287d719b2f4/onnx/vision_model_q4.onnx";
    pub const TEXT: &str = "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/ba1f3b0843f24bc5417d38e19c37b287d719b2f4/onnx/text_model_int8.onnx";
    pub const TOKENIZER: &str = "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/ba1f3b0843f24bc5417d38e19c37b287d719b2f4/tokenizer.json";
    // i18n: dentro
}

pub const MODELS: [&ModelSpec; 3] = [&VISION, &TEXT, &TOKENIZER];

/// Escala e viés aprendidos (logit_scale = 4.724453, logit_bias = -16.771725 no google/siglip2-base-patch16-224).
const SCALE: f32 = 112.663_1;
const BIAS: f32 = -16.771_725;
const SIDE: usize = 224;
const MAX_TOKENS: usize = 64;

pub fn installed() -> bool {
    MODELS.iter().all(|m| ai::find_model(m.file).is_some())
}

/// Baixa o que faltar dos três arquivos (de 0 a 1 no progresso, pelo tamanho).
pub fn ensure(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let total: u64 = MODELS.iter().map(|m| m.size).sum();
    let mut done = 0u64;
    for m in MODELS {
        let base = done as f32 / total as f32;
        let part = m.size as f32 / total as f32;
        ai::ensure_model(m, &|p| on_progress(base + p * part), cancelled)?;
        done += m.size;
    }
    on_progress(1.0);
    Ok(())
}

// ------------------------------------------------------------------ tokenizador

struct Bpe {
    vocab: HashMap<String, i64>,
    ranks: HashMap<(String, String), usize>,
}

fn tokenizer() -> Result<&'static Bpe, String> {
    static T: OnceLock<Result<Bpe, String>> = OnceLock::new();
    T.get_or_init(|| {
        let path = ai::ensure_model(&TOKENIZER, &|_| {}, &|| false)?;
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let j: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("vocabulário inválido: {e}"))?;
        let model = &j["model"];
        let vocab: HashMap<String, i64> = model["vocab"]
            .as_object()
            .ok_or("vocabulário inválido.")?
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_i64()?)))
            .collect();
        // as junções vêm como "a b" (formato antigo) ou ["a", "b"]
        let ranks = model["merges"]
            .as_array()
            .ok_or("vocabulário inválido.")?
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let pair = match m {
                    serde_json::Value::String(s) => {
                        let (a, b) = s.split_once(' ')?;
                        (a.to_string(), b.to_string())
                    }
                    serde_json::Value::Array(v) => (v.first()?.as_str()?.to_string(), v.get(1)?.as_str()?.to_string()),
                    _ => return None,
                };
                Some((pair, i))
            })
            .collect();
        Ok(Bpe { vocab, ranks })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

impl Bpe {
    /// Os ids do texto: espaço vira "▁", junções do BPE pela ordem aprendida, bytes para o que não está no
    /// vocabulário, `<eos>` no fim e `<pad>` até 64.
    fn encode(&self, text: &str) -> Vec<i64> {
        let norm = text.replace(' ', "\u{2581}");
        let mut syms: Vec<String> = norm.chars().map(|c| c.to_string()).collect();
        loop {
            let best = syms
                .windows(2)
                .enumerate()
                .filter_map(|(i, w)| self.ranks.get(&(w[0].clone(), w[1].clone())).map(|r| (*r, i)))
                .min();
            let Some((_, i)) = best else { break };
            let merged = format!("{}{}", syms[i], syms[i + 1]);
            syms[i] = merged;
            syms.remove(i + 1);
        }
        let mut ids: Vec<i64> = Vec::new();
        for s in &syms {
            match self.vocab.get(s) {
                Some(id) => ids.push(*id),
                None => {
                    for b in s.as_bytes() {
                        ids.push(self.vocab.get(&format!("<0x{b:02X}>")).copied().unwrap_or(3));
                    }
                }
            }
        }
        ids.truncate(MAX_TOKENS - 1);
        ids.push(1); // <eos>
        ids.resize(MAX_TOKENS, 0); // <pad>
        ids
    }
}

// ------------------------------------------------------------------ modelos

fn vision() -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(&VISION, &|_| {}, &|| false)?;
    let s = ai::session(&path)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

fn text() -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(&TEXT, &|_| {}, &|| false)?;
    let s = ai::session(&path)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

/// A saída que é o vetor final (as exportações usam nomes diferentes; o último recurso é a primeira 2D).
fn embeddings(out: &ort::session::SessionOutputs, n: usize) -> Result<Vec<Vec<f32>>, String> {
    let names = ["image_embeds", "text_embeds", "pooler_output"];
    for name in names {
        if let Some(v) = out.get(name) {
            let (shape, data) = v.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            if shape.len() == 2 && shape[0] as usize == n {
                let d = shape[1] as usize;
                return Ok(data.chunks(d).map(normalize).collect());
            }
        }
    }
    Err("a IA não devolveu o vetor esperado.".into())
}

fn normalize(v: &[f32]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter().map(|x| x / n).collect()
}

/// Vetor de uma imagem (224×224, sem recorte, valores de -1 a 1).
pub fn image_embedding(img: &DynamicImage) -> Result<Vec<f32>, String> {
    let rgb = img.resize_exact(SIDE as u32, SIDE as u32, FilterType::Triangle).to_rgb8();
    let mut data = vec![0f32; 3 * SIDE * SIDE];
    for (x, y, p) in rgb.enumerate_pixels() {
        for c in 0..3 {
            data[c * SIDE * SIDE + y as usize * SIDE + x as usize] = p[c] as f32 / 127.5 - 1.0;
        }
    }
    // a sessão antes do tensor: é ela que carrega o onnxruntime.dll certo (o do System32 é antigo)
    let session = vision()?;
    let t = Tensor::from_array(([1usize, 3, SIDE, SIDE], data)).map_err(|e| e.to_string())?;
    let mut s = session.lock().unwrap();
    let out = s.run(ort::inputs!["pixel_values" => t]).map_err(|e| format!("falha na IA: {e}"))?;
    Ok(embeddings(&out, 1)?.remove(0))
}

/// Vetores dos textos (com cache: as mesmas opções não são calculadas de novo).
pub fn text_embeddings(texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Vec<f32>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let missing: Vec<String> = {
        let c = cache.lock().unwrap();
        let mut m: Vec<String> = texts.iter().filter(|t| !c.contains_key(*t)).cloned().collect();
        m.dedup();
        m
    };
    if !missing.is_empty() {
        let tok = tokenizer()?;
        let ids: Vec<i64> = missing.iter().flat_map(|t| tok.encode(t)).collect();
        let session = text()?;
        let t = Tensor::from_array(([missing.len(), MAX_TOKENS], ids)).map_err(|e| e.to_string())?;
        let mut s = session.lock().unwrap();
        let out = s.run(ort::inputs!["input_ids" => t]).map_err(|e| format!("falha na IA: {e}"))?;
        let vecs = embeddings(&out, missing.len())?;
        let mut c = cache.lock().unwrap();
        for (k, v) in missing.into_iter().zip(vecs) {
            c.insert(k, v);
        }
    }
    let c = cache.lock().unwrap();
    Ok(texts.iter().map(|t| c[t].clone()).collect())
}

/// O texto que vai para o modelo: a opção dentro de uma frase simples (o SigLIP 2 foi treinado com textos em
/// minúsculas, no estilo das legendas de fotos).
// i18n: fora
pub fn prompt(label: &str) -> String {
    let l = label.trim().to_lowercase();
    if l.split_whitespace().count() > 4 { l } else { format!("uma foto de {l}.") }
}
// i18n: dentro

/// Resultado para uma imagem: cada opção com a probabilidade relativa (as de todas as opções somam 1).
#[derive(Clone, Debug)]
pub struct Choice {
    pub label: String,
    pub prob: f32,
}

/// Compara a imagem com as opções. Volta em ordem, da mais provável para a menos.
pub fn classify_image(img: &DynamicImage, labels: &[String]) -> Result<Vec<Choice>, String> {
    if labels.is_empty() {
        return Err("informe pelo menos duas opções.".into());
    }
    let prompts: Vec<String> = labels.iter().map(|l| prompt(l)).collect();
    let tv = text_embeddings(&prompts)?;
    let iv = image_embedding(img)?;
    let logits: Vec<f32> = tv.iter().map(|t| t.iter().zip(&iv).map(|(a, b)| a * b).sum::<f32>() * SCALE + BIAS).collect();
    let max = logits.iter().cloned().fold(f32::MIN, f32::max);
    let exps: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    let mut out: Vec<Choice> = labels
        .iter()
        .zip(&exps)
        .map(|(l, e)| Choice { label: l.clone(), prob: e / sum })
        .collect();
    out.sort_by(|a, b| b.prob.total_cmp(&a.prob));
    Ok(out)
}

/// Um quadro do vídeo, a 1/3 da duração (o começo costuma ser tela preta ou vinheta).
pub fn video_frame(path: &Path) -> Result<DynamicImage, String> {
    let ff = crate::tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let at = crate::video::probe(path).duration.map_or(0.0, |d| d / 3.0);
    let out = crate::tools::command(&ff)
        .args(["-v", "error", "-nostdin", "-ss", &format!("{at:.2}")])
        .arg("-i")
        .arg(path)
        .args(["-frames:v", "1", "-vf", "scale=448:-2", "-f", "image2pipe", "-c:v", "png", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    image::load_from_memory(&out.stdout).map_err(|_| "não foi possível ler um quadro do vídeo.".to_string())
}

/// Imagens são lidas direto; vídeos, por um quadro.
pub fn classify_file(path: &Path, labels: &[String]) -> Result<Vec<Choice>, String> {
    let img = if crate::video::VIDEO_EXTS.contains(&crate::tools::ext_lower(path).as_str()) {
        video_frame(path)?
    } else {
        image::open(path).map_err(|e| format!("não foi possível abrir a imagem: {e}"))?
    };
    classify_image(&img, labels)
}

/// As opções escritas pelo usuário: uma por linha ou separadas por vírgula, sem repetir.
pub fn parse_options(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for o in text.split(['\n', ',', ';']).map(str::trim).filter(|o| !o.is_empty()) {
        if !out.iter().any(|x| x.to_lowercase() == o.to_lowercase()) {
            out.push(o.to_string());
        }
    }
    out
}

/// A escolha final: a opção mais provável, ou `None` se a confiança ficou abaixo do mínimo (0 a 1).
pub fn decide(choices: &[Choice], min_confidence: f32) -> Option<&Choice> {
    choices.first().filter(|c| c.prob >= min_confidence)
}

// ------------------------------------------------------------------ imagem, som ou texto

/// O que o classificador olha em cada item.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// Pela extensão: imagem → imagem, áudio → som, vídeo → um quadro, texto/PDF → texto.
    Auto,
    Image,
    Sound,
    Text,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum What {
    Image,
    Sound,
    Text,
}

// i18n: fora
pub const TEXT_EXTS: &[&str] = &["txt", "md", "srt", "vtt", "csv", "json", "html", "htm", "log"];
// i18n: dentro

fn what(path: Option<&Path>, has_text: bool, look: Look) -> Option<What> {
    let ext = path.map(crate::tools::ext_lower).unwrap_or_default();
    let image = crate::imaging::IMAGE_EXTS.contains(&ext.as_str());
    let video = crate::video::VIDEO_EXTS.contains(&ext.as_str());
    let audio = crate::media::AUDIO_EXTS.contains(&ext.as_str());
    let text = has_text || TEXT_EXTS.contains(&ext.as_str()) || ext == "pdf";
    match look {
        Look::Image => (image || video).then_some(What::Image),
        Look::Sound => (audio || video).then_some(What::Sound),
        Look::Text => text.then_some(What::Text),
        Look::Auto if image || video => Some(What::Image),
        Look::Auto if audio => Some(What::Sound),
        Look::Auto if text => Some(What::Text),
        Look::Auto => None,
    }
}

/// O texto de um item: o que veio pelo fluxo, o conteúdo de um arquivo de texto ou o texto de um PDF.
fn text_of(path: Option<&Path>, text: Option<&str>) -> Option<String> {
    if let Some(t) = text.filter(|t| !t.trim().is_empty()) {
        return Some(t.to_string());
    }
    let p = path?;
    if crate::tools::ext_lower(p) == "pdf" {
        let doc = lopdf::Document::load(p).ok()?;
        let pages: Vec<u32> = doc.get_pages().keys().copied().take(10).collect();
        return doc.extract_text(&pages).ok().filter(|t| !t.trim().is_empty());
    }
    std::fs::read(p).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Classifica imagens, sons e textos, carregando cada IA só quando aparece um item dela.
pub struct Classifier {
    look: Look,
    options: Vec<String>,
    llm: Option<crate::llm::Llm>,
    english: std::collections::HashMap<String, String>,
}

impl Classifier {
    pub fn new(options: Vec<String>, look: Look) -> Self {
        Classifier { look, options, llm: None, english: Default::default() }
    }

    fn llm(&mut self, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<&crate::llm::Llm, String> {
        if self.llm.is_none() {
            self.llm = Some(crate::llm::Llm::start(on_progress, cancelled)?);
        }
        Ok(self.llm.as_ref().unwrap())
    }

    /// `None` = o item não é do tipo que este classificador olha (vai para "incerto").
    pub fn classify(&mut self, path: Option<&Path>, text: Option<&str>, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Option<Result<Vec<Choice>, String>> {
        let w = what(path, text.is_some_and(|t| !t.trim().is_empty()) && path.is_none(), self.look)?;
        Some(match w {
            What::Image => {
                let r = if installed() { Ok(()) } else { ensure(on_progress, cancelled) };
                r.and_then(|_| classify_file(path.ok_or("sem arquivo")?, &self.options))
            }
            What::Sound => {
                let r = if crate::audioclass::installed() { Ok(()) } else { crate::audioclass::ensure(on_progress, cancelled) };
                // opções fora do dicionário de sons, em outro idioma: a IA local traduz (quando está instalada)
                let need: Vec<String> = self
                    .options
                    .iter()
                    .filter(|o| !self.english.contains_key(*o) && crate::audioclass::english(o, None) == o.trim().to_lowercase() && !o.is_ascii())
                    .cloned()
                    .collect();
                if !need.is_empty() && crate::llm::installed() {
                    if let Ok(llm) = self.llm(on_progress, cancelled) {
                        if let Ok(v) = llm.translate(&need, "English", &|_| {}, cancelled) {
                            self.english.extend(need.into_iter().zip(v));
                        }
                    }
                }
                let map = self.english.clone();
                let tr = move |l: &str| map.iter().find(|(k, _)| k.to_lowercase() == l).map(|(_, v)| v.clone());
                r.and_then(|_| crate::audioclass::classify_file(path.ok_or("sem arquivo")?, &self.options, Some(&tr)))
            }
            What::Text => {
                let Some(t) = text_of(path, text) else { return Some(Err("o item não tem texto.".into())) };
                let options = self.options.clone();
                self.llm(on_progress, cancelled).and_then(|llm| llm.classify(&t, &options))
            }
        })
    }
}
