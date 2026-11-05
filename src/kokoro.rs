//! Voz natural com o Kokoro 82M (Apache-2.0), no ONNX Runtime do próprio Conver: o texto vira fonemas com o espeak-ng
//! (a DLL que já vem com o Piper, em `bin\piper`) e os fonemas viram som a 24 kHz. Modelo e vozes são baixados no
//! primeiro uso. Só para os idiomas em que o espeak-ng dá bons fonemas ao Kokoro.

use std::ffi::{CString, c_char, c_int, c_void};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use ort::session::Session;
use ort::value::Tensor;

use crate::ai::{self, ModelSpec};
use crate::tools;

pub const RATE: u32 = 24_000;

// i18n: fora
pub const MODEL: ModelSpec = ModelSpec {
    file: "kokoro-v1.0-quantized.onnx",
    url: "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/onnx/model_quantized.onnx",
    sha256: "fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478",
    size: 92_361_116,
    label: "Kokoro 82M (92 MB)",
};

const fn voice(file: &'static str, url: &'static str, sha256: &'static str) -> ModelSpec {
    ModelSpec { file, url, sha256, size: 522_240, label: "Voz do Kokoro (522 KB)" }
}

macro_rules! v {
    ($name:literal, $sha:literal) => {
        voice(concat!("kokoro-", $name, ".bin"), concat!("https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/1939ad2a8e416c0acfeecc08a694d14ef25f2231/voices/", $name, ".bin"), $sha)
    };
}
// i18n: dentro

/// Uma voz do Kokoro.
pub struct KVoice {
    pub id: &'static str,
    /// Nome em português (chave de tradução).
    pub label: &'static str,
    /// Idioma do texto, como em `translate::LANGS`.
    pub lang: &'static str,
    /// Nome da voz no espeak-ng.
    espeak: &'static str,
    pub file: ModelSpec,
}

pub const VOICES: &[KVoice] = &[
    KVoice { id: "pf_dora", label: "Português (Brasil) — Dora, feminina", lang: "pt", espeak: "pt-br", file: v!("pf_dora", "3da7b5b2d91847ebf5646f57631af6ececae3c29a89cd300f06edf9aa6cfe9ee") },
    KVoice { id: "pm_alex", label: "Português (Brasil) — Alex, masculina", lang: "pt", espeak: "pt-br", file: v!("pm_alex", "0175c753f59c54e7fd5a995bedef0c5ff2fb67e0043dd3dcb2ae74ec2acbeb2a") },
    KVoice { id: "pm_santa", label: "Português (Brasil) — Santa, masculina", lang: "pt", espeak: "pt-br", file: v!("pm_santa", "8b012db3185778afe2e45a62cbad69db73021774fe68dda634bcc748a982eede") },
    KVoice { id: "af_heart", label: "Inglês (EUA) — Heart, feminina", lang: "en", espeak: "en-us", file: v!("af_heart", "d583ccff3cdca2f7fae535cb998ac07e9fcb90f09737b9a41fa2734ec44a8f0b") },
    KVoice { id: "am_michael", label: "Inglês (EUA) — Michael, masculina", lang: "en", espeak: "en-us", file: v!("am_michael", "1d1f21dd8da39c30705cd4c75d039d265e9bc4a2a93ed09bc9e1b1225eb95ba1") },
    KVoice { id: "ef_dora", label: "Espanhol (Espanha) — Dora, feminina", lang: "es", espeak: "es", file: v!("ef_dora", "f66ec66bd295acb18372e37008533a9a3228483ccd294e7538d5d9294ac9a532") },
    KVoice { id: "em_alex", label: "Espanhol (Espanha) — Alex, masculina", lang: "es", espeak: "es", file: v!("em_alex", "27809e9eafdcbcfff90a3016c697568676531de2a2c39cee29c96c7bd6b83e95") },
    KVoice { id: "ff_siwis", label: "Francês — Siwis, feminina", lang: "fr", espeak: "fr", file: v!("ff_siwis", "a35f5675ad08948e326ae75fd0ea16ba5d0042e4f76b5f3d1df77d0a48c54861") },
    KVoice { id: "if_sara", label: "Italiano — Sara, feminina", lang: "it", espeak: "it", file: v!("if_sara", "409b69248798fcdc2542330c76953d230710f19b057e59cb82fdc3c4cf71265c") },
    KVoice { id: "im_nicola", label: "Italiano — Nicola, masculina", lang: "it", espeak: "it", file: v!("im_nicola", "bc578e510d52a96d6940d46f12e96d7b3df00905dbea075113226d100e6e1ab0") },
    KVoice { id: "hf_alpha", label: "Hindi — Alpha, feminina", lang: "hi", espeak: "hi", file: v!("hf_alpha", "040be6a4425411cc01fda5fd06693c76bfa78572632852bc8cda9c99232ffb56") },
    KVoice { id: "hm_omega", label: "Hindi — Omega, masculina", lang: "hi", espeak: "hi", file: v!("hm_omega", "b02d9222d9ed00ce26b302173a862c2c93f96cc40b5c422b8d14910b9ff34137") },
];

pub fn voice_by_id(id: &str) -> Option<&'static KVoice> {
    VOICES.iter().find(|v| v.id == id)
}

/// Primeira voz de um idioma.
pub fn voice_for(lang: &str) -> Option<&'static KVoice> {
    VOICES.iter().find(|v| v.lang == lang)
}

pub fn installed(v: &KVoice) -> bool {
    ai::is_installed(&MODEL) && ai::is_installed(&v.file)
}

/// Vocabulário do modelo: cada fonema e seu número.
const VOCAB: &[(char, i64)] = &[
    (';', 1), (':', 2), (',', 3), ('.', 4), ('!', 5), ('?', 6), ('—', 9), ('…', 10), ('"', 11), ('(', 12), (')', 13), ('“', 14), ('”', 15), (' ', 16),
    ('\u{303}', 17), ('ʣ', 18), ('ʥ', 19), ('ʦ', 20), ('ʨ', 21), ('ᵝ', 22), ('ꭧ', 23), ('A', 24), ('I', 25), ('O', 31), ('Q', 33), ('S', 35), ('T', 36),
    ('W', 39), ('Y', 41), ('ᵊ', 42), ('a', 43), ('b', 44), ('c', 45), ('d', 46), ('e', 47), ('f', 48), ('h', 50), ('i', 51), ('j', 52), ('k', 53), ('l', 54),
    ('m', 55), ('n', 56), ('o', 57), ('p', 58), ('q', 59), ('r', 60), ('s', 61), ('t', 62), ('u', 63), ('v', 64), ('w', 65), ('x', 66), ('y', 67), ('z', 68),
    ('ɑ', 69), ('ɐ', 70), ('ɒ', 71), ('æ', 72), ('β', 75), ('ɔ', 76), ('ɕ', 77), ('ç', 78), ('ɖ', 80), ('ð', 81), ('ʤ', 82), ('ə', 83), ('ɚ', 85), ('ɛ', 86),
    ('ɜ', 87), ('ɟ', 90), ('ɡ', 92), ('ɥ', 99), ('ɨ', 101), ('ɪ', 102), ('ʝ', 103), ('ɯ', 110), ('ɰ', 111), ('ŋ', 112), ('ɳ', 113), ('ɲ', 114), ('ɴ', 115),
    ('ø', 116), ('ɸ', 118), ('θ', 119), ('œ', 120), ('ɹ', 123), ('ɾ', 125), ('ɻ', 126), ('ʁ', 128), ('ɽ', 129), ('ʂ', 130), ('ʃ', 131), ('ʈ', 132), ('ʧ', 133),
    ('ʊ', 135), ('ʋ', 136), ('ʌ', 138), ('ɣ', 139), ('ɤ', 140), ('χ', 142), ('ʎ', 143), ('ʒ', 147), ('ʔ', 148), ('ˈ', 156), ('ˌ', 157), ('ː', 158), ('ʰ', 162),
    ('ʲ', 164), ('↓', 169), ('→', 171), ('↗', 172), ('↘', 173), ('ᵻ', 177),
];

fn token(c: char) -> Option<i64> {
    VOCAB.iter().find(|(k, _)| *k == c).map(|(_, v)| *v)
}

// ------------------------------------------------------------------- espeak-ng

type Init = unsafe extern "C" fn(c_int, c_int, *const c_char, c_int) -> c_int;
type SetVoice = unsafe extern "C" fn(*const c_char) -> c_int;
type ToPhonemes = unsafe extern "C" fn(*mut *const c_void, c_int, c_int) -> *const c_char;

struct Espeak {
    set_voice: SetVoice,
    to_phonemes: ToPhonemes,
}

// a biblioteca não é segura entre threads: todo uso passa pelo Mutex
unsafe impl Send for Espeak {}

/// Caminho curto (8.3): o espeak-ng abre arquivos com a página de código do sistema, e o curto não tem acentos.
fn short_path(p: &std::path::Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    use windows::core::PCWSTR;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf)) } as usize;
    if n == 0 || n >= buf.len() { p.display().to_string() } else { String::from_utf16_lossy(&buf[..n]) }
}

// i18n: fora
fn load_espeak() -> Result<Espeak, String> {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::{HSTRING, s};
    let dll = tools::find_tool_file("piper\\espeak-ng.dll").ok_or("espeak-ng.dll não encontrado (pasta bin\\piper).")?;
    let data: PathBuf = dll.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    let cdata = CString::new(short_path(&data)).map_err(|e| e.to_string())?;
    unsafe {
        let lib = LoadLibraryW(&HSTRING::from(dll.as_os_str())).map_err(|e| format!("não foi possível abrir o espeak-ng: {e}"))?;
        let (init, set, to) = (GetProcAddress(lib, s!("espeak_Initialize")), GetProcAddress(lib, s!("espeak_SetVoiceByName")), GetProcAddress(lib, s!("espeak_TextToPhonemes")));
        let (Some(init), Some(set), Some(to)) = (init, set, to) else {
            return Err("esta versão do espeak-ng não tem as funções esperadas.".into());
        };
        let init: Init = std::mem::transmute(init);
        // 1 = AUDIO_OUTPUT_RETRIEVAL: nada de placa de som, só os fonemas
        if init(1, 0, cdata.as_ptr(), 0) < 0 {
            return Err("não foi possível iniciar o espeak-ng.".into());
        }
        Ok(Espeak { set_voice: std::mem::transmute::<_, SetVoice>(set), to_phonemes: std::mem::transmute::<_, ToPhonemes>(to) })
    }
}

// i18n: dentro

fn espeak() -> Result<&'static Mutex<Espeak>, String> {
    static E: OnceLock<Result<Mutex<Espeak>, String>> = OnceLock::new();
    E.get_or_init(|| load_espeak().map(Mutex::new)).as_ref().map_err(Clone::clone)
}

/// Fonemas de um trecho sem pontuação (IPA, com "^" ligando os que formam um som só).
fn ipa(e: &Espeak, text: &str) -> String {
    const CHARS_UTF8: c_int = 1;
    // bit 1: IPA; bit 7 + bits 8-23: "^" entre as letras de um mesmo fonema
    const MODE: c_int = 0x02 | 0x80 | ((b'^' as c_int) << 8);
    let Ok(c) = CString::new(text) else { return String::new() };
    let mut ptr = c.as_ptr() as *const c_void;
    let mut out = String::new();
    while !ptr.is_null() {
        let r = unsafe { (e.to_phonemes)(&mut ptr, CHARS_UTF8, MODE) };
        if r.is_null() {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&unsafe { std::ffi::CStr::from_ptr(r) }.to_string_lossy());
    }
    out
}

/// Do espeak para os fonemas do Kokoro (a mesma troca que o misaki, a biblioteca oficial, faz).
fn to_kokoro(s: &str) -> String {
    let mut o = s.to_string();
    for (a, b) in [("a^ɪ", "I"), ("a^ʊ", "W"), ("d^z", "ʣ"), ("d^ʑ", "ʥ"), ("d^ʒ", "ʤ"), ("e^ɪ", "A"), ("o^ʊ", "O"), ("ə^ʊ", "Q"), ("s^s", "S"), ("t^s", "ʦ"), ("t^ɕ", "ʨ"), ("t^ʃ", "ʧ"), ("ɔ^ɪ", "Y")] {
        o = o.replace(a, b);
    }
    o.replace('^', "")
}

/// Fonemas do texto, com a pontuação preservada (é ela que dá as pausas).
pub fn phonemize(text: &str, v: &KVoice) -> Result<String, String> {
    let e = espeak()?.lock().unwrap_or_else(|p| p.into_inner());
    let name = CString::new(v.espeak).map_err(|e| e.to_string())?;
    if unsafe { (e.set_voice)(name.as_ptr()) } != 0 {
        return Err("esta voz não está disponível.".into());
    }
    let mut out = String::new();
    let mut piece = String::new();
    let mut flush = |piece: &mut String, punct: Option<char>| {
        let t = piece.trim();
        if t.chars().any(char::is_alphanumeric) {
            if !out.is_empty() && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&to_kokoro(&ipa(&e, t)));
        }
        if let Some(p) = punct {
            out.push(p);
        }
        piece.clear();
    };
    for c in text.chars() {
        match c {
            ',' | '.' | ';' | ':' | '!' | '?' => flush(&mut piece, Some(c)),
            '…' => flush(&mut piece, Some('…')),
            '\n' | '\r' => flush(&mut piece, Some(',')),
            c => piece.push(c),
        }
    }
    flush(&mut piece, None);
    Ok(out)
}

// ---------------------------------------------------------------------- modelo

fn session(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(&MODEL, on_progress, cancelled)?;
    let s = ai::session(&path)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

/// Estilo da voz para um trecho de `n` fonemas: o arquivo guarda 510 estilos de 256 números, um para cada tamanho.
fn style(voice: &[u8], n: usize) -> Vec<f32> {
    let row = n.saturating_sub(1).min(509) * 256 * 4;
    voice[row..row + 256 * 4].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

/// Divide os fonemas em trechos que cabem no modelo (510), cortando nas pausas.
fn chunks(ids: &[i64]) -> Vec<&[i64]> {
    const MAX: usize = 400;
    let breaks = [1i64, 2, 3, 4, 5, 6, 10];
    let mut out = Vec::new();
    let mut rest = ids;
    while rest.len() > MAX {
        let cut = rest[..MAX].iter().rposition(|t| breaks.contains(t)).map_or(MAX, |i| i + 1);
        out.push(&rest[..cut]);
        rest = &rest[cut..];
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// Fala `text` com a voz `v` e devolve as amostras (24 kHz, de −1 a 1).
pub fn speak(text: &str, v: &KVoice, speed: f32, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, String> {
    // o modelo primeiro: o ONNX Runtime certo precisa estar carregado antes de qualquer tensor
    let model = session(&|p| on_progress(p * 0.9), cancelled)?;
    let vpath = ai::ensure_model(&v.file, &|_| {}, cancelled)?;
    let voice = std::fs::read(&vpath).map_err(|e| e.to_string())?;
    if voice.len() < 510 * 256 * 4 {
        return Err("arquivo de voz inválido.".into());
    }
    let ps = phonemize(text, v)?;
    let ids: Vec<i64> = ps.chars().filter_map(token).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut audio = Vec::new();
    for part in chunks(&ids) {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        let mut input = Vec::with_capacity(part.len() + 2);
        input.push(0i64);
        input.extend_from_slice(part);
        input.push(0);
        let n = input.len();
        let tokens = Tensor::from_array(([1usize, n], input)).map_err(|e| e.to_string())?;
        let st = Tensor::from_array(([1usize, 256], style(&voice, part.len()))).map_err(|e| e.to_string())?;
        let sp = Tensor::from_array(([1usize], vec![speed.clamp(0.5, 2.0)])).map_err(|e| e.to_string())?;
        let mut s = model.lock().unwrap_or_else(|e| e.into_inner());
        let out = s.run(ort::inputs!["input_ids" => tokens, "style" => st, "speed" => sp]).map_err(|e| e.to_string())?;
        let (_, wav) = out[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        audio.extend_from_slice(wav);
    }
    Ok(audio)
}
