//! Texto para voz no próprio PC com o Piper (rhasspy/piper, MIT; vozes em ONNX de ~63 MB, baixadas no primeiro uso
//! e conferidas pelo SHA-256). Bem mais rápido que o tempo real, só no processador.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::ai::{self, ModelSpec};
use crate::tools;

pub struct Voice {
    pub id: &'static str,
    /// Nome em português (chave de tradução).
    pub label: &'static str,
    pub lang: &'static str,
    pub onnx: ModelSpec,
    pub json: ModelSpec,
}

// i18n: fora
pub const VOICES: &[Voice] = &[
    Voice {
        id: "pt_BR-faber-medium",
        label: "Português (Brasil) — Faber, masculina",
        lang: "pt",
        onnx: ModelSpec { file: "pt_BR-faber-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/pt/pt_BR/faber/medium/pt_BR-faber-medium.onnx", sha256: "858555e3a064209c57088fe6bd70c4c3dc54d03eaa00c45d5ecaf43a33f95aa7", size: 63201294, label: "voz pt_BR-faber-medium (63 MB)" },
        json: ModelSpec { file: "pt_BR-faber-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/pt/pt_BR/faber/medium/pt_BR-faber-medium.onnx.json", sha256: "7e694de195ae3fc36dd732c445eb04fb49b649854893cb5506b978f0d50a1d6f", size: 4855, label: "voz pt_BR-faber-medium (configuração)" },
    },
    Voice {
        id: "pt_BR-cadu-medium",
        label: "Português (Brasil) — Cadu, masculina",
        lang: "pt",
        onnx: ModelSpec { file: "pt_BR-cadu-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/pt/pt_BR/cadu/medium/pt_BR-cadu-medium.onnx", sha256: "765f0809a6ea9035d4a6d0d008dbf8876e68b2dd32029312672fa8f405bdb535", size: 62950044, label: "voz pt_BR-cadu-medium (62 MB)" },
        json: ModelSpec { file: "pt_BR-cadu-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/pt/pt_BR/cadu/medium/pt_BR-cadu-medium.onnx.json", sha256: "5fe03aa3d4901880554905b12075713cd552598c8a350455a1ec73f8b4e6be19", size: 5040, label: "voz pt_BR-cadu-medium (configuração)" },
    },
    Voice {
        id: "pt_BR-jeff-medium",
        label: "Português (Brasil) — Jeff, masculina",
        lang: "pt",
        onnx: ModelSpec { file: "pt_BR-jeff-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/pt/pt_BR/jeff/medium/pt_BR-jeff-medium.onnx", sha256: "3a6f4c46355813c2b7bbc4d16b6d13d60ed72074b952a393baace82a7d0c94b5", size: 62950044, label: "voz pt_BR-jeff-medium (62 MB)" },
        json: ModelSpec { file: "pt_BR-jeff-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/pt/pt_BR/jeff/medium/pt_BR-jeff-medium.onnx.json", sha256: "7bf8145b572b36806f5ce0f1d3322b6711975bc7d0473e8d36fced4a9ec0030d", size: 5041, label: "voz pt_BR-jeff-medium (configuração)" },
    },
    Voice {
        id: "en_US-amy-medium",
        label: "Inglês (EUA) — Amy, feminina",
        lang: "en",
        onnx: ModelSpec { file: "en_US-amy-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/en/en_US/amy/medium/en_US-amy-medium.onnx", sha256: "b3a6e47b57b8c7fbe6a0ce2518161a50f59a9cdd8a50835c02cb02bdd6206c18", size: 63201294, label: "voz en_US-amy-medium (63 MB)" },
        json: ModelSpec { file: "en_US-amy-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/en/en_US/amy/medium/en_US-amy-medium.onnx.json", sha256: "95a23eb4d42909d38df73bb9ac7f45f597dbfcde2d1bf9526fdeaf5466977d77", size: 4882, label: "voz en_US-amy-medium (configuração)" },
    },
    Voice {
        id: "en_US-lessac-medium",
        label: "Inglês (EUA) — Lessac, feminina",
        lang: "en",
        onnx: ModelSpec { file: "en_US-lessac-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/en/en_US/lessac/medium/en_US-lessac-medium.onnx", sha256: "5efe09e69902187827af646e1a6e9d269dee769f9877d17b16b1b46eeaaf019f", size: 63201294, label: "voz en_US-lessac-medium (63 MB)" },
        json: ModelSpec { file: "en_US-lessac-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json", sha256: "efe19c417bed055f2d69908248c6ba650fa135bc868b0e6abb3da181dab690a0", size: 4885, label: "voz en_US-lessac-medium (configuração)" },
    },
    Voice {
        id: "es_ES-davefx-medium",
        label: "Espanhol (Espanha) — Davefx, masculina",
        lang: "es",
        onnx: ModelSpec { file: "es_ES-davefx-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/es/es_ES/davefx/medium/es_ES-davefx-medium.onnx", sha256: "6658b03b1a6c316ee4c265a9896abc1393353c2d9e1bca7d66c2c442e222a917", size: 63201294, label: "voz es_ES-davefx-medium (63 MB)" },
        json: ModelSpec { file: "es_ES-davefx-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/es/es_ES/davefx/medium/es_ES-davefx-medium.onnx.json", sha256: "0e0dda87c732f6f38771ff274a6380d9252f327dca77aa2963d5fbdf9ec54842", size: 4817, label: "voz es_ES-davefx-medium (configuração)" },
    },
    Voice {
        id: "es_MX-claude-high",
        label: "Espanhol (México) — Claude, masculina",
        lang: "es",
        onnx: ModelSpec { file: "es_MX-claude-high.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/es/es_MX/claude/high/es_MX-claude-high.onnx", sha256: "3ef40a71ea63852cd8ab7e6fa7d2ecdcfa67a0b47c9c48e3f10e02ee02083ea0", size: 63122309, label: "voz es_MX-claude-high (63 MB)" },
        json: ModelSpec { file: "es_MX-claude-high.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/es/es_MX/claude/high/es_MX-claude-high.onnx.json", sha256: "1afc81f703c0e4cb3b4d7c0dca096b8b54a98806807f0170cf5eb5557723c12d", size: 4963, label: "voz es_MX-claude-high (configuração)" },
    },
    Voice {
        id: "fr_FR-siwis-medium",
        label: "Francês — Siwis, feminina",
        lang: "fr",
        onnx: ModelSpec { file: "fr_FR-siwis-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/fr/fr_FR/siwis/medium/fr_FR-siwis-medium.onnx", sha256: "641d1ab097da2b81128c076810edb052b385decc8be3381814802a64a73baf99", size: 63201294, label: "voz fr_FR-siwis-medium (63 MB)" },
        json: ModelSpec { file: "fr_FR-siwis-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/fr/fr_FR/siwis/medium/fr_FR-siwis-medium.onnx.json", sha256: "39479916c2db192b5ac9764daddd0c744d83e023ad890c6976c0633ae4df8959", size: 4875, label: "voz fr_FR-siwis-medium (configuração)" },
    },
    Voice {
        id: "de_DE-thorsten-medium",
        label: "Alemão — Thorsten, masculina",
        lang: "de",
        onnx: ModelSpec { file: "de_DE-thorsten-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/de/de_DE/thorsten/medium/de_DE-thorsten-medium.onnx", sha256: "7e64762d8e5118bb578f2eea6207e1a35a8e0c30595010b666f983fc87bb7819", size: 63201294, label: "voz de_DE-thorsten-medium (63 MB)" },
        json: ModelSpec { file: "de_DE-thorsten-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/de/de_DE/thorsten/medium/de_DE-thorsten-medium.onnx.json", sha256: "974adee790533adb273a1ac88f49027d2a1b8f0f2cf4905954a4791e79264e85", size: 4819, label: "voz de_DE-thorsten-medium (configuração)" },
    },
    Voice {
        id: "it_IT-paola-medium",
        label: "Italiano — Paola, feminina",
        lang: "it",
        onnx: ModelSpec { file: "it_IT-paola-medium.onnx", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/it/it_IT/paola/medium/it_IT-paola-medium.onnx", sha256: "6fc918b5a0ea6137382833dddfa567bffbe6a5060c02043c87192ee59c04210c", size: 63511038, label: "voz it_IT-paola-medium (63 MB)" },
        json: ModelSpec { file: "it_IT-paola-medium.onnx.json", url: "https://huggingface.co/rhasspy/piper-voices/resolve/e7229f7d33a10e145e109b886d661984201ac3c4/it/it_IT/paola/medium/it_IT-paola-medium.onnx.json", sha256: "aea19c0a7fce29fbc359b93f10e7902854401e4c95ae2ea328ae516b15d296cf", size: 7099, label: "voz it_IT-paola-medium (configuração)" },
    },
];
// i18n: dentro

// nomes das vozes, para a tradução (a lista acima fica fora da extração por causa dos endereços)
#[allow(dead_code)]
const VOICE_NAMES: &[&str] = &[
    "Português (Brasil) — Faber, masculina",
    "Português (Brasil) — Cadu, masculina",
    "Português (Brasil) — Jeff, masculina",
    "Inglês (EUA) — Amy, feminina",
    "Inglês (EUA) — Lessac, feminina",
    "Espanhol (Espanha) — Davefx, masculina",
    "Espanhol (México) — Claude, masculina",
    "Francês — Siwis, feminina",
    "Alemão — Thorsten, masculina",
    "Italiano — Paola, feminina",
];

pub fn voice(id: &str) -> Option<&'static Voice> {
    VOICES.iter().find(|v| v.id == id || v.id.eq_ignore_ascii_case(id))
}

/// Voz padrão para um idioma do app (português se não houver).
pub fn default_voice(lang: &str) -> &'static Voice {
    VOICES.iter().find(|v| v.lang == lang).unwrap_or(&VOICES[0])
}

pub fn installed(v: &Voice) -> bool {
    ai::is_installed(&v.onnx) && ai::is_installed(&v.json)
}

fn piper() -> Result<PathBuf, String> {
    tools::find_tool_file(r"piper\piper.exe").ok_or_else(|| "piper.exe não encontrado (pasta bin\\piper).".to_string())
}

/// Tira o que não é fala de uma legenda (.srt/.vtt): números, tempos e marcas.
pub fn text_from_subtitle(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains("-->") && !l.chars().all(|c| c.is_ascii_digit()) && *l != "WEBVTT")
        .map(|l| {
            let mut out = String::new();
            let mut tag = false;
            for c in l.chars() {
                match c {
                    '<' | '{' => tag = true,
                    '>' | '}' => tag = false,
                    _ if !tag => out.push(c),
                    _ => {}
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Lê o texto de um arquivo (.txt, .md, .srt, .vtt).
pub fn text_from_file(p: &Path) -> Result<String, String> {
    let raw = std::fs::read(p).map_err(|e| format!("{}: {e}", tools::file_name(p)))?;
    let s = String::from_utf8(raw.clone()).unwrap_or_else(|_| raw.iter().map(|&b| b as char).collect());
    Ok(match tools::ext_lower(p).as_str() {
        "srt" | "vtt" | "ass" => text_from_subtitle(&s),
        _ => s,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OutFmt {
    Mp3,
    Wav,
}

/// Narra `text` com a voz e salva em `dst` (MP3 ou WAV). `speed` 0,5–2 (1 = normal).
pub fn speak(text: &str, voice: &Voice, speed: f32, fmt: OutFmt, dst: &Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("o texto está vazio.".into());
    }
    let exe = piper()?;
    // 0–30%: baixar a voz (só na primeira vez)
    let model = ai::ensure_model(&voice.onnx, &|p| on_progress(p * 0.3), cancelled)?;
    ai::ensure_model(&voice.json, &|_| {}, cancelled)?;
    on_progress(0.3);
    let wav = if fmt == OutFmt::Wav { dst.to_path_buf() } else { dst.with_extension("piper.wav") };
    // length_scale: maior = mais devagar
    let scale = (1.0 / speed.clamp(0.5, 2.0)).to_string();
    let mut child = tools::command(&exe)
        .args(["--model"])
        .arg(&model)
        .args(["--length_scale", &scale, "--sentence_silence", "0.25", "--output_file"])
        .arg(&wav)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o Piper: {e}"))?;
    {
        // uma frase por linha: o Piper lê linha a linha
        let mut stdin = child.stdin.take().unwrap();
        let lines: String = text.replace("\r", "").lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
        stdin.write_all(lines.as_bytes()).map_err(|e| e.to_string())?;
    }
    let total = text.chars().count().max(1) as f32;
    loop {
        if cancelled() {
            let _ = child.kill();
            let _ = std::fs::remove_file(&wav);
            return Err("Cancelado.".into());
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                // sem progresso do Piper: estima pelo tamanho do WAV (~1,5 s de fala a cada 20 letras)
                let secs = tools::file_size(&wav) as f32 / 44_100.0;
                on_progress(0.3 + 0.6 * (secs / (total / 14.0)).min(1.0));
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        use std::io::Read;
        let _ = e.read_to_string(&mut err);
    }
    if tools::file_size(&wav) < 100 {
        let last = err.lines().filter(|l| l.contains("error") || l.contains("rror")).last().unwrap_or("o Piper não gerou o áudio.").to_string();
        return Err(last);
    }
    if fmt == OutFmt::Mp3 {
        let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
        let out = tools::command(&ff).args(["-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&wav).args(["-c:a", "libmp3lame", "-b:a", "160k"]).arg(dst).output().map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&wav);
        if !out.status.success() {
            return Err("não foi possível converter para MP3.".into());
        }
    }
    on_progress(1.0);
    let secs = crate::video::duration(dst).unwrap_or(0.0);
    Ok((format!("{} • {}", crate::video::fmt_time(secs), tools::human_size(tools::file_size(dst))), dst.to_path_buf()))
}
