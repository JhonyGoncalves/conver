//! IA de linguagem local: o llama-server (llama.cpp, MIT, Vulkan) com o Qwen 3.5 2B (Apache-2.0), rodando como
//! processo separado numa porta livre de 127.0.0.1, só enquanto um trabalho precisa dele. Usada para traduzir
//! legendas, classificar textos (escolha entre opções, com a probabilidade de cada uma) e traduzir opções para o CLAP.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::ai::{self, ModelSpec};
use crate::classify::Choice;
use crate::tools;

pub const MODEL: ModelSpec = ModelSpec {
    file: "Qwen3.5-2B-Q4_K_M.gguf",
    url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-Q4_K_M.gguf",
    sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
    size: 1_280_835_840,
    label: "Qwen 3.5 2B (1,3 GB)",
};

// i18n: fora
/// O llama.cpp não vai no download do Conver: é baixado (com o modelo) só por quem usa a IA de linguagem.
pub const RUNTIME: ModelSpec = ModelSpec {
    file: "llama-b12121-bin-win-vulkan-x64.zip",
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b12121/llama-b12121-bin-win-vulkan-x64.zip",
    sha256: "73111d348dcbfa86050611c28b8bef523c849d80a6e5529e25b67f1b2055e918",
    size: 37_683_434,
    label: "llama.cpp b12121 (38 MB)",
};

/// O que se tira do zip: o servidor e as bibliotecas (sem os outros programas do llama.cpp).
const RUNTIME_FILES: &[&str] = &[
    "llama-server.exe", "llama-server-impl.dll", "llama.dll", "llama-common.dll", "mtmd.dll", "ggml.dll", "ggml-base.dll",
    "ggml-vulkan.dll", "ggml-rpc.dll", "libomp.dll", "LICENSE-LLVM-OpenMP", "ggml-cpu-x64.dll", "ggml-cpu-sse42.dll",
    "ggml-cpu-sandybridge.dll", "ggml-cpu-ivybridge.dll", "ggml-cpu-piledriver.dll", "ggml-cpu-haswell.dll",
    "ggml-cpu-skylakex.dll", "ggml-cpu-cannonlake.dll", "ggml-cpu-cascadelake.dll", "ggml-cpu-cooperlake.dll",
    "ggml-cpu-icelake.dll", "ggml-cpu-alderlake.dll", "ggml-cpu-sapphirerapids.dll", "ggml-cpu-zen4.dll",
];

// i18n: dentro

/// Pasta do llama.cpp baixado (nos dados do usuário).
pub fn runtime_dir() -> PathBuf {
    tools::app_dir().join("ferramentas").join("llm")
}

fn server_exe() -> Option<PathBuf> {
    // uma cópia junto do programa (desenvolvimento) ou a baixada
    tools::find_tool_file("llm\\llama-server.exe").or_else(|| Some(runtime_dir().join("llama-server.exe")).filter(|p| p.is_file()))
}

pub fn runtime_installed() -> bool {
    server_exe().is_some()
}

/// Modelo e programa no computador (nada a baixar).
pub fn installed() -> bool {
    runtime_installed() && ai::find_model(MODEL.file).is_some()
}

/// Baixa o llama.cpp (o zip oficial, conferido pelo SHA-256) e tira dele só o que o Conver usa.
pub fn ensure_runtime(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    if let Some(exe) = server_exe() {
        return Ok(exe);
    }
    install_runtime(&runtime_dir(), on_progress, cancelled)
}

/// Baixa e extrai o llama.cpp em `dir`.
pub(crate) fn install_runtime(dir: &std::path::Path, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    let dir = dir.to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip = dir.with_file_name(RUNTIME.file);
    ai::download_verified(RUNTIME.url, &zip, RUNTIME.sha256, RUNTIME.size, RUNTIME.label, &|p| on_progress(p * 0.95), cancelled)?;
    // o tar do Windows (10 1803 ou mais novo) abre .zip
    let tar = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\tar.exe");
    let st = tools::command(&tar).arg("-xf").arg(&zip).arg("-C").arg(&dir).args(RUNTIME_FILES).status();
    let _ = std::fs::remove_file(&zip);
    let exe = dir.join("llama-server.exe");
    if !st.is_ok_and(|s| s.success()) || !exe.is_file() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("não foi possível extrair o llama.cpp.".into());
    }
    on_progress(1.0);
    Ok(exe)
}

/// Programa e modelo, baixando o que faltar (de 0 a 1 no progresso, pelo tamanho).
pub fn ensure(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(PathBuf, PathBuf), String> {
    let total = (RUNTIME.size + MODEL.size) as f32;
    let share = if runtime_installed() { 0.0 } else { RUNTIME.size as f32 / total };
    let exe = ensure_runtime(&|p| on_progress(p * share), cancelled)?;
    let model = ai::ensure_model(&MODEL, &|p| on_progress(share + p * (1.0 - share)), cancelled)?;
    Ok((exe, model))
}

/// Um llama-server rodando. Ao sair de escopo, o processo é encerrado.
pub struct Llm {
    child: Child,
    port: u16,
    agent: ureq::Agent,
}

impl Drop for Llm {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> Result<u16, String> {
    std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| e.to_string())
}

impl Llm {
    /// Baixa o modelo se faltar (0–90% do progresso) e sobe o servidor, primeiro na placa de vídeo (Vulkan) e, se
    /// não der, só no processador.
    pub fn start(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Llm, String> {
        let (exe, model) = ensure(&|p| on_progress(p * 0.9), cancelled)?;
        let mut last = String::new();
        for gpu_layers in ["99", "0"] {
            let port = free_port()?;
            // i18n: fora
            let mut child = tools::command(&exe)
                .arg("-m")
                .arg(&model)
                .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", "8192", "-ngl", gpu_layers, "--jinja", "-np", "1", "--threads", &ai::threads().to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("não foi possível iniciar a IA local: {e}"))?;
            // i18n: dentro
            let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(600)).build();
            let t = Instant::now();
            loop {
                if cancelled() {
                    let _ = child.kill();
                    return Err("Cancelado.".into());
                }
                if let Ok(Some(_)) = child.try_wait() {
                    let mut err = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = e.read_to_string(&mut err);
                    }
                    last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("o processo fechou").to_string();
                    break;
                }
                let ok = agent.get(&format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(2)).call().is_ok_and(|r| r.status() == 200);
                if ok {
                    on_progress(1.0);
                    // o stderr não é mais lido: descarta numa thread para o buffer não encher
                    if let Some(mut e) = child.stderr.take() {
                        std::thread::spawn(move || {
                            let mut sink = [0u8; 4096];
                            while e.read(&mut sink).is_ok_and(|n| n > 0) {}
                        });
                    }
                    return Ok(Llm { child, port, agent });
                }
                if t.elapsed() > Duration::from_secs(180) {
                    let _ = child.kill();
                    last = "a IA local demorou demais para começar.".into();
                    break;
                }
                on_progress(0.9 + 0.1 * (t.elapsed().as_secs_f32() / 30.0).min(0.99));
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        Err(format!("não foi possível iniciar a IA local: {last}"))
    }

    /// Uma conversa de uma pergunta. `schema` (JSON Schema) obriga a resposta a seguir aquele formato.
    fn request(&self, system: &str, user: &str, schema: Option<&Value>, max_tokens: u32, logprobs: bool) -> Result<Value, String> {
        // i18n: fora
        let mut body = json!({
            "messages": [{ "role": "system", "content": system }, { "role": "user", "content": user }],
            "temperature": 0.1,
            "max_tokens": max_tokens,
            "chat_template_kwargs": { "enable_thinking": false },
        });
        if let Some(s) = schema {
            body["response_format"] = json!({ "type": "json_schema", "json_schema": { "name": "resposta", "schema": s } });
        }
        if logprobs {
            body["logprobs"] = json!(true);
            body["top_logprobs"] = json!(10);
        }
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
        // i18n: dentro
        let resp = self
            .agent
            .post(&url)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("a IA local não respondeu: {e}"))?;
        let text = resp.into_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn chat(&self, system: &str, user: &str, schema: Option<&Value>, max_tokens: u32) -> Result<String, String> {
        let v = self.request(system, user, schema, max_tokens, false)?;
        v["choices"][0]["message"]["content"].as_str().map(|s| s.trim().to_string()).ok_or_else(|| "a IA local não devolveu texto.".into())
    }

    /// Escolhe uma das opções para o texto, com a probabilidade de cada uma (pela primeira parte de cada opção nas
    /// probabilidades que o modelo deu ao escolher, como os classificadores "de escolha").
    pub fn classify(&self, text: &str, options: &[String]) -> Result<Vec<Choice>, String> {
        let clipped: String = text.chars().take(6000).collect();
        // i18n: fora
        let system = "You classify texts. Pick exactly one of the given options for the text. Answer only with the JSON.";
        let user = format!("Options: {}\n\nText:\n\"\"\"\n{clipped}\n\"\"\"", serde_json::to_string(options).unwrap_or_default());
        let schema = json!({ "type": "object", "properties": { "choice": { "type": "string", "enum": options } }, "required": ["choice"] });
        // i18n: dentro
        let v = self.request(system, &user, Some(&schema), 64, true)?;
        let content = v["choices"][0]["message"]["content"].as_str().unwrap_or_default();
        let chosen = serde_json::from_str::<Value>(content).ok().and_then(|j| j["choice"].as_str().map(String::from)).ok_or("a IA local não escolheu uma opção.")?;
        // probabilidades: o token onde a opção começa (logo depois de "choice":") e as alternativas que o modelo via ali
        let mut probs = vec![0f32; options.len()];
        let mut acc = String::new();
        for tok in v["choices"][0]["logprobs"]["content"].as_array().into_iter().flatten() {
            let t = tok["token"].as_str().unwrap_or_default();
            if acc.trim_end().ends_with("\"choice\":\"") || acc.trim_end().ends_with("\"choice\": \"") {
                for alt in tok["top_logprobs"].as_array().into_iter().flatten() {
                    let (at, lp) = (alt["token"].as_str().unwrap_or_default(), alt["logprob"].as_f64().unwrap_or(f64::MIN));
                    if at.is_empty() {
                        continue;
                    }
                    let matching: Vec<usize> = options.iter().enumerate().filter(|(_, o)| o.starts_with(at) || at.starts_with(o.as_str())).map(|(i, _)| i).collect();
                    for i in &matching {
                        probs[*i] += lp.exp() as f32 / matching.len() as f32;
                    }
                }
                break;
            }
            acc.push_str(t);
        }
        let sum: f32 = probs.iter().sum();
        if sum <= 0.0 {
            // sem probabilidades: só a escolha
            probs = options.iter().map(|o| if *o == chosen { 1.0 } else { 0.0 }).collect();
        } else {
            probs.iter_mut().for_each(|p| *p /= sum);
        }
        let mut out: Vec<Choice> = options.iter().zip(probs).map(|(o, p)| Choice { label: o.clone(), prob: p }).collect();
        out.sort_by(|a, b| (b.label == chosen).cmp(&(a.label == chosen)).then(b.prob.total_cmp(&a.prob)));
        Ok(out)
    }

    /// Traduz uma lista de frases curtas (legendas) mantendo a ordem e a quantidade. Vai em blocos de 20.
    pub fn translate(&self, lines: &[String], language: &str, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<String>, String> {
        let mut out = Vec::with_capacity(lines.len());
        for (b, chunk) in lines.chunks(20).enumerate() {
            if cancelled() {
                return Err("Cancelado.".into());
            }
            // i18n: fora
            let system = format!(
                "You translate video subtitles into {language}. Translate each item of the JSON array the way a professional subtitler would: natural and idiomatic, as a native speaker would say it (never word by word), keeping the meaning, tone and similar length; the items are consecutive lines of the same video; keep names as they are. Answer only with a JSON array with the same number of items, in the same order."
            );
            let schema = json!({ "type": "array", "items": { "type": "string" }, "minItems": chunk.len(), "maxItems": chunk.len() });
            // i18n: dentro
            let user = serde_json::to_string(chunk).unwrap_or_default();
            let text = self.chat(&system, &user, Some(&schema), 2048)?;
            let v: Vec<String> = serde_json::from_str(&text).map_err(|_| "a IA local não devolveu a tradução no formato esperado.".to_string())?;
            if v.len() != chunk.len() {
                return Err("a tradução veio com um número diferente de frases.".into());
            }
            out.extend(v);
            on_progress(((b + 1) * 20).min(lines.len()) as f32 / lines.len().max(1) as f32);
        }
        Ok(out)
    }
}
