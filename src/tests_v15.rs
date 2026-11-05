//! Testes dos fluxos de automação por nodes e da IA (com um servidor de IA falso, local).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::ai_api::{self, Config, Provider};
use crate::flow::{self, Flow, Inputs, Link, Node, RunState};
use crate::ops;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub(crate) fn ctx_run(f: &Flow, inputs: &Inputs, out: Option<&std::path::Path>, ai: &Result<Config, String>) -> Value {
    let state = Mutex::new(RunState::default());
    let log = |s: &str| println!("{s}");
    let ctx = ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &log };
    let r = flow::run(f, inputs, out, &state, &ctx, ai).unwrap();
    // a pasta de trabalho fica nos dados do usuário: não deixa sobra dos testes
    if let Some(w) = r["work_dir"].as_str() {
        let _ = std::fs::remove_dir_all(w);
    }
    r
}

pub(crate) fn node(id: u32, kind: &str, args: Value) -> Node {
    Node { id, kind: kind.into(), title: String::new(), args: args.as_object().cloned().unwrap_or_default(), pos: [0.0, 0.0] }
}

pub(crate) fn link(from: u32, port: &str, to: u32) -> Link {
    Link { from, port: port.into(), to }
}

/// Servidor de IA falso: responde no formato da OpenAI ou da Anthropic e guarda os pedidos recebidos.
/// Aprova (no ai_gate) só o arquivo "verde.png".
fn fake_ai(provider: Provider) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let body = String::from_utf8_lossy(&body).to_string();
            let gate = body.contains("approve");
            let answer = if gate {
                if body.contains("verde.png") { r#"{"approve": true, "reason": "é verde"}"# } else { r#"{"approve": false, "reason": "não é verde"}"# }
            } else {
                "RESPOSTA DA IA"
            };
            seen2.lock().unwrap().push(body);
            let json = match provider {
                Provider::OpenAi => json!({ "choices": [{ "message": { "role": "assistant", "content": answer } }] }),
                Provider::Anthropic => json!({ "content": [{ "type": "text", "text": answer }] }),
            }
            .to_string();
            let mut s = stream;
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len());
        }
    });
    (url, seen)
}

#[test]
fn flow_tools_chain() {
    let d = tmp("flow_chain");
    for (name, w) in [("a.png", 400), ("b.png", 300)] {
        image::RgbImage::from_pixel(w, 200, image::Rgb([10, 200, 30])).save(d.join(name)).unwrap();
    }
    std::fs::write(d.join("nota.txt"), "olá").unwrap();
    // entrada → filtro (só imagens) → redimensionar 50% → converter para JPG → salvar; o texto vai para "no"
    let f = Flow {
        trigger: Default::default(),
        name: "teste - cadeia".into(),
        description: String::new(),
        nodes: vec![
            node(1, "input", json!({})),
            node(2, "filter", json!({ "types": ["image"] })),
            node(3, "tool:image_resize", json!({ "percent": 50 })),
            node(4, "tool:image_convert", json!({ "format": "jpg" })),
            node(5, "output", json!({ "folder": d.join("saida").display().to_string() })),
            node(6, "output", json!({ "folder": d.join("outros").display().to_string() })),
        ],
        links: vec![link(1, "out", 2), link(2, "yes", 3), link(3, "out", 4), link(4, "out", 5), link(2, "no", 6)],
    };
    f.validate().unwrap();
    let r = ctx_run(&f, &Inputs { files: vec![d.clone()], texts: vec![] }, None, &Err("sem IA".into()));
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
    assert_eq!(r["failed_nodes"], 0);
    let mut out: Vec<String> = std::fs::read_dir(d.join("saida")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out.iter().all(|n| n.ends_with(".jpg")));
    let small = image::open(d.join("saida").join(&out[0])).unwrap();
    assert_eq!(small.width(), 200); // a.png 400 → 50%
    assert!(d.join("outros").join("nota.txt").is_file());
}

#[test]
fn flow_validation() {
    let mut f = Flow { name: "x".into(), nodes: vec![node(1, "input", json!({})), node(2, "tool:nao_existe", json!({}))], ..Default::default() };
    assert!(f.validate().unwrap_err().contains("tipo desconhecido"));
    f.nodes[1].kind = "tool:image_resize".into();
    f.links = vec![link(1, "yes", 2)];
    assert!(f.validate().unwrap_err().contains("não tem a saída"));
    f.links = vec![link(1, "out", 2), link(2, "out", 1)];
    let e = f.validate().unwrap_err();
    assert!(e.contains("não recebe entradas") || e.contains("ciclo"), "{e}");
    f.nodes.push(node(3, "tool:image_convert", json!({})));
    f.links = vec![link(1, "out", 2), link(2, "out", 3)];
    f.validate().unwrap();
    assert!(f.would_cycle(3, 2) && !f.would_cycle(1, 3));
    assert_eq!(f.order().unwrap(), vec![1, 2, 3]);
    // todos os tipos de node têm esquema, e nenhuma ferramenta de fluxo entra como node
    let kinds = flow::kinds();
    assert!(kinds.len() > 40);
    assert!(kinds.iter().all(|k| k.schema["type"] == "object" && !k.kind.starts_with("tool:flow_")));
    assert!(kinds.iter().all(|k| k.schema["properties"].get("files").is_none() || k.kind == "input"));
}

#[test]
fn flow_ai_nodes() {
    for provider in [Provider::OpenAi, Provider::Anthropic] {
        let (url, seen) = fake_ai(provider);
        let ai = Ok(Config { provider, model: "teste".into(), base_url: url, key: "k".into() });
        let d = tmp("flow_ai");
        image::RgbImage::from_pixel(64, 64, image::Rgb([0, 255, 0])).save(d.join("verde.png")).unwrap();
        image::RgbImage::from_pixel(64, 64, image::Rgb([255, 0, 0])).save(d.join("vermelho.png")).unwrap();
        // entrada → IA avalia (só a imagem "verde" passa) → IA escreve uma legenda salva em .txt → saída
        let f = Flow {
            trigger: Default::default(),
            name: "teste - ia".into(),
            description: String::new(),
            nodes: vec![
                node(1, "input", json!({})),
                node(2, "ai_gate", json!({ "criteria": "A cor está certa?" })),
                node(3, "ai_prompt", json!({ "prompt": "Legenda para {file}", "save_as": "txt" })),
                node(4, "output", json!({ "folder": d.join("ok").display().to_string() })),
                node(5, "output", json!({ "folder": d.join("nao").display().to_string() })),
            ],
            links: vec![link(1, "out", 2), link(2, "yes", 3), link(3, "out", 4), link(2, "no", 5)],
        };
        let r = ctx_run(&f, &Inputs { files: vec![d.join("verde.png"), d.join("vermelho.png")], texts: vec![] }, None, &ai);
        assert_eq!(r["failed_nodes"], 0, "{r}");
        assert!(d.join("ok").join("verde.png").is_file() && d.join("ok").join("verde.txt").is_file());
        assert_eq!(std::fs::read_to_string(d.join("ok").join("verde.txt")).unwrap(), "RESPOSTA DA IA");
        assert!(d.join("nao").join("vermelho.png").is_file() && !d.join("nao").join("vermelho.txt").exists());
        let bodies = seen.lock().unwrap();
        assert_eq!(bodies.len(), 3, "2 avaliações + 1 legenda");
        // a imagem vai junto, no formato de cada API
        let img_key = if provider == Provider::OpenAi { "image_url" } else { "\"base64\"" };
        assert!(bodies.iter().all(|b| b.contains(img_key)), "{provider:?}");
        assert!(bodies[2].contains("Legenda para verde.png"));
    }
    // sem IA configurada, o node de IA falha com uma mensagem clara e o resto não roda
    let f = Flow { name: "teste - sem ia".into(), nodes: vec![node(1, "text", json!({ "text": "oi" })), node(2, "ai_prompt", json!({ "prompt": "x" })), node(3, "output", json!({}))], links: vec![link(1, "out", 2), link(2, "out", 3)], ..Default::default() };
    let r = ctx_run(&f, &Inputs::default(), None, &Err("configure a chave".into()));
    assert_eq!(r["nodes"][1]["status"], "failed");
    assert_eq!(r["nodes"][2]["status"], "skipped");
}

#[test]
fn ai_helpers() {
    assert_eq!(ai_api::base64(b"Man"), "TWFu");
    assert_eq!(ai_api::base64(b"Ma"), "TWE=");
    assert_eq!(ai_api::base64(b"M"), "TQ==");
    assert_eq!(ai_api::parse_verdict(r#"aqui: {"approve": false, "reason": "borrada"}"#), (false, "borrada".into()));
    assert!(ai_api::parse_verdict("Sim, está ótimo").0);
    assert!(!ai_api::parse_verdict("Não").0);
    assert_eq!(ai_api::test_protect_roundtrip("sk-segredo-123"), "sk-segredo-123");
    let mut s = ai_api::Settings::default();
    assert!(s.config().is_err()); // sem chave
    s.set_key("sk-x").unwrap();
    assert!(s.key_protected.len() > 20 && !s.key_protected.contains("sk-x"));
    let c = s.config().unwrap();
    assert_eq!((c.key.as_str(), c.model.as_str()), ("sk-x", "claude-sonnet-5-5"));
    // servidor local (Ollama) não precisa de chave, mas precisa de modelo
    let local = ai_api::Settings { provider: Provider::OpenAi, base_url: "http://localhost:11434/v1".into(), ..Default::default() };
    assert!(local.config().is_err());
    assert!(ai_api::Settings { model: "llama3.1".into(), ..local }.config().is_ok());
}

#[test]
fn flow_ops_mcp() {
    let d = tmp("flow_ops");
    image::RgbImage::from_pixel(120, 80, image::Rgb([1, 2, 3])).save(d.join("x.png")).unwrap();
    let log = |_: &str| {};
    let ctx = ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &log };
    let types = ops::call("flow_node_types", &json!({}), &ctx).unwrap();
    assert!(types["kinds"].as_array().unwrap().iter().any(|k| k["kind"] == "ai_gate" && k["outputs"] == json!(["yes", "no"])));
    let name = "teste mcp - converter";
    let bad = ops::call("flow_save", &json!({ "flow": { "name": name, "nodes": [{ "id": 1, "kind": "tool:xyz" }] } }), &ctx);
    assert!(bad.is_err());
    let flow_json = json!({ "name": name, "nodes": [
        { "id": 1, "kind": "input" }, { "id": 2, "kind": "tool:image_convert", "args": { "format": "webp" } } ],
        "links": [{ "from": 1, "to": 2 }] });
    let saved = ops::call("flow_save", &json!({ "flow": flow_json }), &ctx).unwrap();
    assert_eq!(saved["saved"], name);
    let got = ops::call("flow_get", &json!({ "name": name }), &ctx).unwrap();
    assert_eq!(got["nodes"][1]["args"]["format"], "webp");
    assert_ne!(got["nodes"][1]["pos"], json!([0.0, 0.0]), "posição automática");
    let listed = ops::call("flow_list", &json!({}), &ctx).unwrap();
    assert!(listed["flows"].as_array().unwrap().iter().any(|f| f["name"] == name));
    let run = ops::call("flow_run", &json!({ "name": name, "files": [d.join("x.png").display().to_string()] }), &ctx).unwrap();
    let out = run["results"][0]["file"].as_str().unwrap();
    assert!(out.ends_with(".webp") && std::path::Path::new(out).is_file(), "{run}");
    let _ = std::fs::remove_dir_all(run["work_dir"].as_str().unwrap());
    ops::call("flow_delete", &json!({ "name": name }), &ctx).unwrap();
    assert!(ops::call("flow_get", &json!({ "name": name }), &ctx).is_err());
    // os exemplos são fluxos válidos
    for f in flow::examples() {
        f.validate().unwrap();
    }
}
