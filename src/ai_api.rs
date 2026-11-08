//! IA na nuvem para os fluxos de automação: Anthropic (Claude) ou qualquer API compatível com a da OpenAI
//! (OpenAI, OpenRouter, Groq, Ollama, LM Studio…). A configuração fica em `ia.json`, na pasta de dados; a chave é
//! guardada protegida pelo Windows (DPAPI): só o mesmo usuário, neste PC, consegue abri-la.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Anthropic,
    /// Qualquer API no formato /v1/chat/completions.
    OpenAi,
}

impl Provider {
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-sonnet-5-5",
            Provider::OpenAi => "",
        }
    }

    pub fn default_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://api.anthropic.com/v1",
            Provider::OpenAi => "https://api.openai.com/v1",
        }
    }
}

/// Serviços conhecidos compatíveis com a OpenAI: nome, endereço da API e um modelo para começar (vazio = escolher
/// na lista do "Testar conexão").
// i18n: fora
pub const PRESETS: &[(&str, &str, &str)] = &[
    ("OpenAI", "https://api.openai.com/v1", ""),
    ("DeepSeek", "https://api.deepseek.com", "deepseek-flash"),
    ("OpenRouter", "https://openrouter.ai/api/v1", ""),
    ("Groq", "https://api.groq.com/openai/v1", ""),
    ("Gemini", "https://generativelanguage.googleapis.com/v1beta/openai", ""),
    ("Ollama", "http://localhost:11434/v1", ""),
    ("LM Studio", "http://localhost:1234/v1", ""),
];
// i18n: dentro

/// Um endereço que parece de página do site (onde se cria a chave), e não da API.
pub fn url_problem(url: &str) -> Option<&'static str> {
    let u = url.trim().to_lowercase();
    if u.is_empty() {
        return None;
    }
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return Some("O endereço precisa começar com https:// (ou http:// para uma IA no próprio PC).");
    }
    let site = ["platform.", "console.", "dashboard", "/api_keys", "/api-keys", "/keys", "/settings", "aistudio.", "/login"];
    site.iter().any(|s| u.contains(s)).then_some("Isso parece o endereço da página do site, não o da API. Use um dos botões acima ou o endereço da documentação do serviço (para a DeepSeek: https://api.deepseek.com).")
}

/// Configuração salva (a chave vai protegida).
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub provider: Provider,
    pub model: String,
    /// Vazio = o endereço padrão do provedor.
    pub base_url: String,
    /// Chave protegida pelo DPAPI, em hexadecimal.
    pub key_protected: String,
}

/// Configuração pronta para usar (chave aberta). Vive só na memória.
#[derive(Clone, Debug)]
pub struct Config {
    pub provider: Provider,
    pub model: String,
    pub base_url: String,
    pub key: String,
}

fn path() -> std::path::PathBuf {
    tools::app_dir().join("ia.json")
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let p = path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&p, text).map_err(|e| format!("não foi possível salvar a configuração da IA: {e}"))
    }

    pub fn has_key(&self) -> bool {
        !self.key_protected.is_empty()
    }

    pub fn set_key(&mut self, key: &str) -> Result<(), String> {
        let key = key.trim();
        self.key_protected = if key.is_empty() { String::new() } else { hex(&protect(key.as_bytes())?) };
        Ok(())
    }

    /// Configuração pronta, ou o motivo de não estar (sem chave, sem modelo…).
    pub fn config(&self) -> Result<Config, String> {
        let key = if self.key_protected.is_empty() {
            String::new()
        } else {
            let raw = unhex(&self.key_protected).ok_or("a chave salva está corrompida; informe de novo.")?;
            String::from_utf8(unprotect(&raw)?).map_err(|_| "a chave salva está corrompida; informe de novo.".to_string())?
        };
        let model = if self.model.trim().is_empty() { self.provider.default_model().to_string() } else { self.model.trim().to_string() };
        let base_url = if self.base_url.trim().is_empty() { self.provider.default_url().to_string() } else { self.base_url.trim().trim_end_matches('/').to_string() };
        if model.is_empty() {
            return Err("informe o modelo da IA em Configurações → Inteligência artificial.".into());
        }
        // servidores locais (Ollama, LM Studio) não pedem chave
        let local = base_url.contains("localhost") || base_url.contains("127.0.0.1");
        if key.is_empty() && !local {
            return Err("configure a chave da API em Configurações → Inteligência artificial.".into());
        }
        Ok(Config { provider: self.provider, model, base_url, key })
    }
}

// ------------------------------------------------------------- mensagens

/// Um pedaço da mensagem enviada: texto ou imagem (JPEG/PNG em base64).
pub enum Part {
    Text(String),
    Image { mime: &'static str, base64: String },
}

/// Pede uma resposta à IA. `system` diz como ela deve se comportar.
pub fn ask(cfg: &Config, system: &str, parts: &[Part], max_tokens: u32) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(180)).build();
    let (url, body, req) = match cfg.provider {
        Provider::Anthropic => {
            let content: Vec<Value> = parts
                .iter()
                .map(|p| match p {
                    Part::Text(t) => json!({ "type": "text", "text": t }),
                    Part::Image { mime, base64 } => json!({ "type": "image", "source": { "type": "base64", "media_type": mime, "data": base64 } }),
                })
                .collect();
            let body = json!({ "model": cfg.model, "max_tokens": max_tokens, "system": system, "messages": [{ "role": "user", "content": content }] });
            let url = format!("{}/messages", cfg.base_url);
            let req = agent.post(&url).set("x-api-key", &cfg.key).set("anthropic-version", "2023-06-01");
            (url, body, req)
        }
        Provider::OpenAi => {
            let content: Vec<Value> = parts
                .iter()
                .map(|p| match p {
                    Part::Text(t) => json!({ "type": "text", "text": t }),
                    Part::Image { mime, base64 } => json!({ "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{base64}") } }),
                })
                .collect();
            let body = json!({ "model": cfg.model, "max_tokens": max_tokens,
                "messages": [{ "role": "system", "content": system }, { "role": "user", "content": content }] });
            let url = format!("{}/chat/completions", cfg.base_url);
            let mut req = agent.post(&url);
            if !cfg.key.is_empty() {
                req = req.set("Authorization", &format!("Bearer {}", cfg.key));
            }
            (url, body, req)
        }
    };
    let resp = req.set("content-type", "application/json").send_string(&body.to_string());
    let text = match resp {
        Ok(r) => r.into_string().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&body)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().or(v["error"].as_str()).map(String::from))
                .unwrap_or_else(|| body.chars().take(300).collect());
            return Err(match code {
                401 | 403 => format!("a IA recusou a chave (erro {code}): {msg}"),
                404 => format!("modelo ou endereço não encontrado (erro 404): {msg}"),
                405 => format!("este endereço não aceita pedidos de IA (erro 405): confira se é o endereço da API, não o da página do site. {}", msg),
                429 => format!("limite de uso da IA atingido (erro 429): {msg}"),
                _ => format!("erro {code} da IA: {msg}"),
            });
        }
        Err(e) => return Err(format!("não foi possível falar com a IA em {url}: {e}")),
    };
    let v: Value = serde_json::from_str(&text).map_err(|_| "resposta inválida da IA.".to_string())?;
    let answer = match cfg.provider {
        Provider::Anthropic => v["content"].as_array().map(|c| c.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("")),
        Provider::OpenAi => v["choices"][0]["message"]["content"].as_str().map(String::from),
    };
    answer.map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).ok_or_else(|| "a IA não devolveu texto.".to_string())
}

/// Modelos que a conta pode usar (`GET /models`), em ordem alfabética.
pub fn list_models(cfg: &Config) -> Result<Vec<String>, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).build();
    let url = format!("{}/models", cfg.base_url);
    let mut req = agent.get(&url);
    match cfg.provider {
        Provider::Anthropic => req = req.set("x-api-key", &cfg.key).set("anthropic-version", "2023-06-01"),
        Provider::OpenAi if !cfg.key.is_empty() => req = req.set("Authorization", &format!("Bearer {}", cfg.key)),
        Provider::OpenAi => {}
    }
    let text = match req.call() {
        Ok(r) => r.into_string().map_err(|e| e.to_string())?,
        Err(ureq::Error::Status(code, _)) => return Err(format!("erro {} ao pedir a lista de modelos", code)),
        Err(e) => return Err(format!("não foi possível falar com a IA em {url}: {e}")),
    };
    let v: Value = serde_json::from_str(&text).map_err(|_| "resposta inválida da IA.".to_string())?;
    let mut ids: Vec<String> = v["data"].as_array().or(v["models"].as_array()).into_iter().flatten().filter_map(|m| m["id"].as_str().or(m["name"].as_str()).map(String::from)).collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Resposta de sim ou não da IA, com o motivo. Aceita JSON ({"approve": true, "reason": "…"}) ou texto.
pub fn parse_verdict(answer: &str) -> (bool, String) {
    if let (Some(a), Some(b)) = (answer.find('{'), answer.rfind('}')) {
        if let Ok(v) = serde_json::from_str::<Value>(&answer[a..=b]) {
            let ok = ["approve", "approved", "aprovado", "ok", "pass", "result"].iter().find_map(|k| match &v[*k] {
                Value::Bool(b) => Some(*b),
                Value::String(s) => Some(matches!(s.to_lowercase().as_str(), "true" | "yes" | "sim" | "approve" | "approved" | "pass")),
                _ => None,
            });
            if let Some(ok) = ok {
                let reason = ["reason", "motivo", "explanation"].iter().find_map(|k| v[*k].as_str()).unwrap_or_default().to_string();
                return (ok, reason);
            }
        }
    }
    let low = answer.trim().to_lowercase();
    let ok = low.starts_with("sim") || low.starts_with("yes") || low.starts_with("aprov") || low.starts_with("true");
    (ok, answer.trim().chars().take(300).collect())
}

// ------------------------------------------------------- base64 e DPAPI

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(data, true)
}

fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi(data, false)
}

fn dpapi(data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` aponta para `data`, que vive durante a chamada; a saída é alocada pelo Windows e liberada aqui
    unsafe {
        let r = if encrypt {
            CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        } else {
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output)
        };
        r.map_err(|e| format!("não foi possível proteger a chave: {e}"))?;
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        Ok(bytes)
    }
}

#[cfg(test)]
pub(crate) fn test_protect_roundtrip(s: &str) -> String {
    String::from_utf8(unprotect(&protect(s.as_bytes()).unwrap()).unwrap()).unwrap()
}
