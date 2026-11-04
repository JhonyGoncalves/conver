//! Linha de comando (`conver-cli <ferramenta> arquivos… --opção valor`) e servidor MCP (`conver-cli mcp`),
//! para scripts e agentes de IA usarem todas as ferramentas do Conver sem abrir a janela.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};

use crate::ops::{self, Ctx};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Ponto de entrada do modo `--cli`. Devolve o código de saída.
pub fn main(args: Vec<String>) -> i32 {
    let first = args.first().map(|s| s.to_lowercase());
    match first.as_deref() {
        None | Some("help" | "ajuda" | "--help" | "-h" | "/?") => {
            if let Some(t) = args.get(1) {
                return help_tool(t);
            }
            usage();
            0
        }
        Some("--version" | "-v" | "version" | "versao") => {
            println!("Conver {VERSION}");
            0
        }
        Some("mcp") => {
            mcp_server();
            0
        }
        Some("list" | "tools" | "ferramentas") => {
            for t in ops::TOOLS {
                println!("{:<24} {}", t.name, t.title);
            }
            0
        }
        Some("presets" | "predefinicoes") => run("preset_list", &args[1..]),
        Some("preset" | "predefinicao") => run("preset_run", &args[1..]),
        Some(name) => run(name, &args[1..]),
    }
}

fn usage() {
    eprintln!(
        "Conver {VERSION} — linha de comando\n\n\
         Uso:\n  conver-cli <ferramenta> <arquivos…> [--opção valor] [--json]\n  conver-cli help <ferramenta>     opções de uma ferramenta\n  \
         conver-cli preset \"<nome>\" <arquivos…>\n  conver-cli presets                lista as predefinições\n  \
         conver-cli mcp                    servidor MCP (stdio) para agentes de IA\n\n\
         Exemplos:\n  conver-cli image_convert fotos\\*.png --format webp\n  conver-cli video_compress aula.mp4 --target_mb 50\n  \
         conver-cli transcribe entrevista.mp3 --language pt --formats srt,txt\n  conver-cli download https://youtu.be/… --kind mp3\n\n\
         Ferramentas:"
    );
    for t in ops::TOOLS {
        eprintln!("  {:<24} {}", t.name, t.title);
    }
}

fn help_tool(name: &str) -> i32 {
    let Some(t) = ops::find(name) else {
        eprintln!("ferramenta desconhecida: {name}  (veja: conver-cli list)");
        return 2;
    };
    let schema = (t.schema)();
    println!("{} — {}\n{}\n", t.name, t.title, t.description);
    let required: Vec<&str> = schema["required"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if let Some(props) = schema["properties"].as_object() {
        for (k, p) in props {
            let mut line = format!("  --{k:<22} {}", p["description"].as_str().unwrap_or(""));
            if let Some(e) = p["enum"].as_array() {
                line.push_str(&format!(" [{}]", e.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("|")));
            }
            if let Some(d) = p.get("default") {
                line.push_str(&format!(" (padrão: {d})"));
            }
            if required.contains(&k.as_str()) {
                line.push_str(" *obrigatório");
            }
            println!("{line}");
        }
    }
    println!("\nArquivos podem ser passados direto (sem --files); curingas como *.jpg funcionam.");
    0
}

/// Expande curingas (* e ?) do nome do arquivo, como o terminal do Linux faria.
fn expand_glob(arg: &str) -> Vec<String> {
    if !arg.contains(['*', '?']) {
        return vec![arg.to_string()];
    }
    let p = PathBuf::from(arg);
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let pat = p.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file() && wildcard(&pat, &e.file_name().to_string_lossy().to_lowercase()))
                .map(|e| e.path().display().to_string())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn wildcard(p: &str, s: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (p.chars().collect(), s.chars().collect());
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(st) = star {
            pi = st + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Converte os argumentos da linha de comando em JSON, de acordo com o esquema da ferramenta.
fn parse_args(tool: &ops::Tool, args: &[String]) -> Result<(Map<String, Value>, bool), String> {
    let schema = (tool.schema)();
    let props = schema["properties"].as_object().cloned().unwrap_or_default();
    let positional_key = if props.contains_key("files") {
        "files"
    } else if props.contains_key("urls") {
        "urls"
    } else if props.contains_key("name") {
        "name"
    } else if props.contains_key("text") {
        "text"
    } else {
        ""
    };
    let mut map = Map::new();
    let mut json_out = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--json" {
            json_out = true;
        } else if a == "--args" {
            let raw = args.get(i + 1).ok_or("--args precisa de um JSON.")?;
            let v: Value = serde_json::from_str(raw).map_err(|e| format!("--args: JSON inválido ({e})"))?;
            if let Value::Object(m) = v {
                map.extend(m);
            }
            i += 1;
        } else if let Some(k) = a.strip_prefix("--") {
            let (k, inline) = match k.split_once('=') {
                Some((k, v)) => (k.replace('-', "_"), Some(v.to_string())),
                None => (k.replace('-', "_"), None),
            };
            let ty = props.get(&k).and_then(|p| p["type"].as_str()).unwrap_or("string").to_string();
            let next_is_value = args.get(i + 1).is_some_and(|n| !n.starts_with("--"));
            let raw = match inline {
                Some(v) => v,
                None if ty == "boolean" && !(next_is_value && matches!(args[i + 1].to_lowercase().as_str(), "true" | "false" | "sim" | "nao" | "não" | "1" | "0")) => "true".into(),
                None if next_is_value => {
                    i += 1;
                    args[i].clone()
                }
                None => return Err(format!("--{k} precisa de um valor.")),
            };
            let value = match ty.as_str() {
                "array" => {
                    let mut list = match map.remove(&k) {
                        Some(Value::Array(a)) => a,
                        _ => Vec::new(),
                    };
                    let parts: Vec<String> = if k == "files" { expand_glob(&raw) } else { raw.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect() };
                    list.extend(parts.into_iter().map(Value::String));
                    Value::Array(list)
                }
                "object" => serde_json::from_str(&raw).map_err(|e| format!("--{k}: JSON inválido ({e})"))?,
                _ => Value::String(raw),
            };
            map.insert(k, value);
        } else {
            positional.push(a.clone());
        }
        i += 1;
    }
    if !positional.is_empty() {
        match positional_key {
            "files" => {
                let mut list = match map.remove("files") {
                    Some(Value::Array(a)) => a,
                    _ => Vec::new(),
                };
                for p in &positional {
                    let found = expand_glob(p);
                    if found.is_empty() {
                        return Err(format!("nenhum arquivo corresponde a {p}"));
                    }
                    list.extend(found.into_iter().map(Value::String));
                }
                // preset_run: o primeiro argumento solto é o nome
                if tool.name == "preset_run" && !map.contains_key("name") && !list.is_empty() {
                    let name = list.remove(0);
                    map.insert("name".into(), name);
                }
                map.insert("files".into(), Value::Array(list));
            }
            "urls" => {
                map.insert("urls".into(), Value::Array(positional.into_iter().map(Value::String).collect()));
            }
            "" => return Err(format!("argumento inesperado: {}", positional[0])),
            key => {
                map.insert(key.into(), Value::String(positional.join(" ")));
            }
        }
    }
    Ok((map, json_out))
}

fn run(name: &str, args: &[String]) -> i32 {
    let Some(tool) = ops::find(name) else {
        eprintln!("ferramenta desconhecida: {name}  (veja: conver-cli list)");
        return 2;
    };
    let (map, json_out) = match parse_args(tool, args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("erro: {e}");
            return 2;
        }
    };
    let last = Mutex::new(-1i32);
    let progress = |p: f32| {
        let pct = (p * 100.0).floor() as i32;
        let mut l = last.lock().unwrap();
        if pct != *l {
            *l = pct;
            eprint!("\r  {pct:3}%");
            let _ = std::io::stderr().flush();
        }
    };
    let log = |m: &str| {
        eprintln!("\r{m}      ");
    };
    let ctx = Ctx { progress: &progress, cancelled: &|| false, log: &log };
    let result = ops::call(tool.name, &Value::Object(map), &ctx);
    eprint!("\r      \r");
    match result {
        Ok(v) => {
            if json_out {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                print_human(&v);
            }
            if v["failed"].as_u64().unwrap_or(0) > 0 { 1 } else { 0 }
        }
        Err(e) => {
            if json_out {
                println!("{}", json!({ "error": e }));
            } else {
                eprintln!("✖ {e}");
            }
            1
        }
    }
}

/// Resumo legível do resultado (o detalhe de cada arquivo já saiu no log).
fn print_human(v: &Value) {
    if let Some(items) = v["results"].as_array() {
        for it in items {
            if let Some(t) = it["text"].as_str() {
                println!("{t}");
            } else if let Some(t) = it["info"].as_str().filter(|_| it.get("output").is_none()) {
                println!("{t}\n");
            } else if let Some(files) = it["files"].as_array() {
                for f in files {
                    println!("{}", f.as_str().unwrap_or_default());
                }
            } else if let Some(e) = it["error"].as_str() {
                if it.get("url").is_some() {
                    eprintln!("✖ {}: {e}", it["url"].as_str().unwrap_or_default());
                }
            }
        }
        if let (Some(ok), Some(failed)) = (v["ok"].as_u64(), v["failed"].as_u64()) {
            eprintln!("Concluído: {ok} ok, {failed} com erro.");
        }
    } else if let Some(o) = v["output"].as_str() {
        println!("{o}   {}", v["info"].as_str().unwrap_or_default());
    } else if let Some(list) = v["presets"].as_array() {
        for p in list {
            println!("{:<30} {:<18} {}", p["name"].as_str().unwrap_or_default(), p["tool"].as_str().unwrap_or_default(), p["args"]);
        }
    } else {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    }
}

// ================================================================ servidor MCP

const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

struct Out(Mutex<std::io::Stdout>);

impl Out {
    fn send(&self, v: &Value) {
        let mut o = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    }
}

fn mcp_tools() -> Value {
    let list: Vec<Value> = ops::TOOLS
        .iter()
        .map(|t| {
            let read_only = matches!(t.name, "file_info" | "image_ocr" | "list_devices" | "preset_list");
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": t.name == "download" },
            })
        })
        .collect();
    json!({ "tools": list })
}

const INSTRUCTIONS: &str = "Conver converts and edits local files on Windows: images (convert, resize, compress, remove background, AI upscale, OCR), \
video/audio (convert, compress, trim, merge, GIF, subtitles, music, transcription with Whisper, vocal separation), PDFs (merge, split, compress, \
to images, password, searchable OCR), downloads (yt-dlp) and screen recording. Always pass absolute file paths. Outputs are saved next to the \
inputs unless out_dir is given; results list every output path. Long jobs report progress and can be cancelled.";

/// Servidor MCP por stdio: uma mensagem JSON-RPC por linha. Cada chamada de ferramenta roda numa thread
/// própria, para o servidor continuar respondendo (ping, cancelamento) durante tarefas longas.
fn mcp_server() {
    let out = Arc::new(Out(Mutex::new(std::io::stdout())));
    let running: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> = Arc::default();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                out.send(&json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("JSON inválido: {e}") } }));
                continue;
            }
        };
        // lotes (JSON-RPC batch) são tratados um a um
        let msgs = if let Value::Array(a) = msg { a } else { vec![msg] };
        for m in msgs {
            handle(m, &out, &running);
        }
    }
    // stdin fechado: o cliente encerrou; cancela o que estiver rodando
    for flag in running.lock().unwrap().values() {
        flag.store(true, Ordering::Relaxed);
    }
}

fn handle(m: Value, out: &Arc<Out>, running: &Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>) {
    let id = m.get("id").cloned();
    let method = m["method"].as_str().unwrap_or_default().to_string();
    let params = m.get("params").cloned().unwrap_or(Value::Null);
    let reply = |result: Value| {
        if let Some(id) = &id {
            out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
        }
    };
    let fail = |code: i64, message: &str| {
        if let Some(id) = &id {
            out.send(&json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }));
        }
    };
    match method.as_str() {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or(PROTOCOLS[0]);
            let version = if PROTOCOLS.contains(&asked) { asked } else { PROTOCOLS[0] };
            reply(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "conver", "title": "Conver", "version": VERSION },
                "instructions": INSTRUCTIONS,
            }));
        }
        "ping" => reply(json!({})),
        "tools/list" => reply(mcp_tools()),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default().to_string();
            if ops::find(&name).is_none() {
                fail(-32602, &format!("ferramenta desconhecida: {name}"));
                return;
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let token = params["_meta"]["progressToken"].clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let key = id.as_ref().map(|v| v.to_string()).unwrap_or_default();
            running.lock().unwrap().insert(key.clone(), cancel.clone());
            let (out, running, id) = (out.clone(), running.clone(), id.clone());
            std::thread::spawn(move || {
                let last = Mutex::new((std::time::Instant::now(), -1.0f32));
                let progress = |p: f32| {
                    if token.is_null() {
                        return;
                    }
                    let mut l = last.lock().unwrap();
                    // no máximo ~4 avisos por segundo
                    if p >= 1.0 || (p - l.1 >= 0.01 && l.0.elapsed().as_millis() >= 250) {
                        *l = (std::time::Instant::now(), p);
                        out.send(&json!({ "jsonrpc": "2.0", "method": "notifications/progress",
                            "params": { "progressToken": token, "progress": (p * 100.0).round(), "total": 100 } }));
                    }
                };
                let cancelled = || cancel.load(Ordering::Relaxed);
                let log = |m: &str| eprintln!("{m}");
                let ctx = Ctx { progress: &progress, cancelled: &cancelled, log: &log };
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ops::call(&name, &args, &ctx)))
                    .unwrap_or_else(|_| Err("erro inesperado.".into()));
                running.lock().unwrap().remove(&key);
                if cancel.load(Ordering::Relaxed) {
                    return; // pedido cancelado: o cliente não espera mais resposta
                }
                let result = match r {
                    Ok(v) => json!({
                        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                        "structuredContent": v,
                        "isError": v["failed"].as_u64().unwrap_or(0) > 0 && v["ok"].as_u64() == Some(0),
                    }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                };
                if let Some(id) = id {
                    out.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                }
            });
        }
        "notifications/cancelled" => {
            let key = params["requestId"].to_string();
            if let Some(flag) = running.lock().unwrap().get(&key) {
                flag.store(true, Ordering::Relaxed);
            }
        }
        m if m.starts_with("notifications/") => {}
        "resources/list" => reply(json!({ "resources": [] })),
        "prompts/list" => reply(json!({ "prompts": [] })),
        _ => fail(-32601, &format!("método não suportado: {method}")),
    }
}
