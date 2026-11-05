//! Testes da classificação local de imagens (SigLIP 2).

use std::path::PathBuf;

use crate::classify;

/// Imagens de teste do protótipo (geradas pelo próprio Conver + um quadro de vídeo e uma captura da tela).
fn samples() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("CONVER_CLASSIFY_IMGS")?);
    d.is_dir().then_some(d)
}

#[test]
fn tokenizer_basics() {
    if !classify::installed() {
        eprintln!("modelo SigLIP 2 não instalado; pulando");
        return;
    }
    // o vetor de um texto não muda com o cache e textos diferentes dão vetores diferentes
    let a = classify::text_embeddings(&["uma foto de gato.".into(), "uma foto de carro.".into()]).unwrap();
    let b = classify::text_embeddings(&["uma foto de gato.".into()]).unwrap();
    assert_eq!(a[0], b[0]);
    let cos: f32 = a[0].iter().zip(&a[1]).map(|(x, y)| x * y).sum();
    assert!(cos < 0.99 && a[0].len() >= 512, "{cos} {}", a[0].len());
}

#[test]
#[ignore] // precisa do modelo (412 MB) e das imagens em CONVER_CLASSIFY_IMGS
fn classify_prototype() {
    let Some(dir) = samples() else { panic!("defina CONVER_CLASSIFY_IMGS") };
    let truth = [
        ("gato.png", "gato"),
        ("cachorro.png", "cachorro"),
        ("comida.png", "comida"),
        ("praia.png", "praia"),
        ("carro.png", "carro"),
        ("documento.png", "documento"),
        ("pessoa.png", "pessoa"),
        ("print.png", "captura de tela"),
    ];
    let labels: Vec<String> = truth.iter().map(|(_, l)| l.to_string()).collect();
    let t = std::time::Instant::now();
    classify::text_embeddings(&labels.iter().map(|l| classify::prompt(l)).collect::<Vec<_>>()).unwrap();
    println!("textos (8 opções, primeira vez com carga do modelo): {:.2}s", t.elapsed().as_secs_f32());
    let mut ok = 0;
    for (f, want) in truth {
        let img = image::open(dir.join(f)).unwrap();
        let t = std::time::Instant::now();
        let r = classify::classify_image(&img, &labels).unwrap();
        let ms = t.elapsed().as_millis();
        let top: Vec<String> = r.iter().take(3).map(|c| format!("{} {:.0}%", c.label, c.prob * 100.0)).collect();
        println!("{f:14} {ms:5} ms  →  {}", top.join(" | "));
        if r[0].label == want {
            ok += 1;
        }
    }
    println!("acertos: {ok}/8");
    // modelos de comparação: o mesmo com rótulos em inglês e sem a frase em volta
    for (name, labs) in [
        ("inglês", vec!["cat", "dog", "food", "beach", "car", "document", "person", "screenshot"]),
        ("frases", vec!["um gato dormindo", "um cachorro", "um prato de comida", "uma praia ao pôr do sol", "um carro esportivo", "um papel com texto impresso", "um homem de uniforme", "a tela de um programa de computador"]),
    ] {
        let labs: Vec<String> = labs.into_iter().map(String::from).collect();
        let mut hit = 0;
        for (i, (f, _)) in truth.iter().enumerate() {
            let r = classify::classify_file(&dir.join(f), &labs).unwrap();
            if r[0].label == labs[i] {
                hit += 1;
            } else {
                println!("  {name}: {f} → {} ({:.0}%)", r[0].label, r[0].prob * 100.0);
            }
        }
        println!("{name}: {hit}/8");
    }
    assert!(ok >= 7);
}

#[test]
fn classify_node_ports() {
    use crate::flow::{Flow, Link, Node, node_outputs};
    assert_eq!(classify::parse_options("gato, Cachorro\n gato ;praia\n\n"), vec!["gato", "Cachorro", "praia"]);
    let node = |id: u32, kind: &str, args: serde_json::Value| Node { id, kind: kind.into(), title: String::new(), args: args.as_object().unwrap().clone(), pos: [0.0, 0.0] };
    let c = node(2, "classify", serde_json::json!({ "options": "documento\nselfie" }));
    assert_eq!(node_outputs(&c), vec!["documento", "selfie", "uncertain"]);
    let mut f = Flow {
        trigger: Default::default(),
        name: "t".into(),
        description: String::new(),
        nodes: vec![node(1, "input", serde_json::json!({})), c, node(3, "output", serde_json::json!({}))],
        links: vec![Link { from: 1, port: "out".into(), to: 2 }, Link { from: 2, port: "selfie".into(), to: 3 }, Link { from: 2, port: "uncertain".into(), to: 3 }],
    };
    assert!(f.validate().is_ok(), "{:?}", f.validate());
    f.links.push(Link { from: 2, port: "paisagem".into(), to: 3 });
    let e = f.validate().unwrap_err();
    assert!(e.contains("paisagem") && e.contains("documento, selfie, uncertain"), "{e}");
}
