//! Testes da leva 1.7: melhorar voz, marca d'água, renomear e gatilhos nos fluxos, rostos, tabelas, tradução.

use std::path::Path;

use serde_json::json;

use crate::tests_v12::{ctx_call, ffmpeg, speech, tmp};

/// Volume integrado (LUFS) medido pelo filtro ebur128 do ffmpeg.
pub(crate) fn lufs(f: &Path) -> f64 {
    let ff = crate::tools::find_tool("ffmpeg").unwrap();
    let out = crate::tools::command(&ff).args(["-hide_banner", "-nostats", "-i"]).arg(f).args(["-af", "ebur128", "-f", "null", "-"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    let summary = text.rsplit("Summary:").next().unwrap_or_default();
    summary.split("I:").nth(1).and_then(|s| s.trim().split(' ').next()).and_then(|v| v.parse().ok()).unwrap()
}

#[test]
fn voice_enhance() {
    let d = tmp("voz");
    let wav = speech(&d, "Hello and welcome to the show. Today we are going to talk about audio quality for podcasts.");
    // fala baixa, com ruído rosa por baixo
    let noisy = d.join("baixa.wav");
    ffmpeg(&["-i", &wav.to_string_lossy(), "-f", "lavfi", "-i", "anoisesrc=c=pink:a=0.02", "-filter_complex", "[0]volume=0.25[v];[v][1]amix=inputs=2:duration=first:normalize=0", &noisy.to_string_lossy()], None);
    let before = lufs(&noisy);
    for (mode, target) in [("podcast", -16.0), ("streaming", -14.0)] {
        let r = ctx_call("audio_enhance_voice", json!({ "files": [noisy.display().to_string()], "loudness": mode, "out_dir": d.join(mode).display().to_string() })).unwrap();
        assert_eq!(r["ok"], 1, "{r}");
        let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
        let after = lufs(&out);
        println!("{mode}: {before:.1} → {after:.1} LUFS ({})", r["results"][0]["info"]);
        assert!((after - target).abs() < 1.5, "{mode}: {after}");
        let (a, b) = (crate::video::probe(&noisy).duration.unwrap(), crate::video::probe(&out).duration.unwrap());
        assert!((a - b).abs() < 0.3, "duração mudou: {a} → {b}");
    }
}

/// Fração de pixels claros (texto branco) num retângulo, em frações da imagem.
fn bright(img: &image::RgbImage, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (w, h) = img.dimensions();
    let (a, b, c, d) = ((x0 * w as f32) as u32, (y0 * h as f32) as u32, (x1 * w as f32) as u32, (y1 * h as f32) as u32);
    let mut n = 0;
    for y in b..d {
        for x in a..c {
            let p = img.get_pixel(x, y);
            if p[0] > 200 && p[1] > 200 && p[2] > 200 {
                n += 1;
            }
        }
    }
    n as f32 / ((c - a) * (d - b)).max(1) as f32
}

#[test]
fn watermark_batch() {
    let d = tmp("marca");
    // imagem escura: texto branco embaixo à direita
    let img = d.join("foto.jpg");
    image::RgbImage::from_pixel(800, 600, image::Rgb([30, 40, 60])).save(&img).unwrap();
    let r = ctx_call("watermark", json!({ "files": [img.display().to_string()], "text": "© Conver 2026", "opacity": 100, "size": 8 })).unwrap();
    assert_eq!(r["ok"], 1, "{r}");
    let out = image::open(r["results"][0]["output"].as_str().unwrap()).unwrap().to_rgb8();
    assert_eq!(out.dimensions(), (800, 600));
    let (corner, other) = (bright(&out, 0.5, 0.8, 1.0, 1.0), bright(&out, 0.0, 0.0, 0.5, 0.5));
    println!("imagem: canto {corner:.3}, resto {other:.3}");
    assert!(corner > 0.02 && other == 0.0);
    // logo vermelho na diagonal, PNG com transparência
    let logo = d.join("logo.png");
    image::RgbaImage::from_fn(200, 80, |x, _| if x < 100 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 0, 0]) }).save(&logo).unwrap();
    let r = ctx_call("watermark", json!({ "files": [img.display().to_string()], "logo": logo.display().to_string(), "position": "diagonal", "size": 40, "opacity": 100, "out_dir": d.join("logo").display().to_string() })).unwrap();
    let out = image::open(r["results"][0]["output"].as_str().unwrap()).unwrap().to_rgb8();
    let red = out.pixels().filter(|p| p[0] > 200 && p[1] < 60).count();
    println!("logo: {red} pixels vermelhos");
    assert!(red > 5000 && out.get_pixel(5, 5)[0] < 60, "logo no meio, cantos intactos");
    // vídeo curto com som: a marca entra e o áudio é copiado
    let vid = d.join("clipe.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0x1E283C:s=640x360:d=2", "-f", "lavfi", "-i", "sine=d=2", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", &vid.to_string_lossy()], None);
    let r = ctx_call("watermark", json!({ "files": [vid.display().to_string()], "text": "AO VIVO", "position": "top_left", "opacity": 100, "size": 10 })).unwrap();
    let vout = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    let pv = crate::video::probe(&vout);
    assert!(pv.has_audio && pv.size == Some((640, 360)), "{pv:?}");
    let frame = d.join("quadro.png");
    ffmpeg(&["-ss", "1", "-i", &vout.to_string_lossy(), "-frames:v", "1", &frame.to_string_lossy()], None);
    let f = image::open(&frame).unwrap().to_rgb8();
    assert!(bright(&f, 0.0, 0.0, 0.5, 0.3) > 0.02 && bright(&f, 0.5, 0.5, 1.0, 1.0) == 0.0);
    // PDF com uma página em pé e outra girada: a marca aparece embaixo à direita nas duas, como a página é vista
    let pngs: Vec<String> = (0..2).map(|i| {
        let p = d.join(format!("p{i}.png"));
        image::RgbImage::from_pixel(600, 800, image::Rgb([20, 20, 20])).save(&p).unwrap();
        p.display().to_string()
    }).collect();
    let r = ctx_call("images_to_pdf", json!({ "files": pngs, "out_dir": d.display().to_string() })).unwrap();
    let pdf = std::path::PathBuf::from(r["output"].as_str().unwrap());
    let mut doc = lopdf::Document::load(&pdf).unwrap();
    let second = *doc.get_pages().get(&2).unwrap();
    doc.get_dictionary_mut(second).unwrap().set("Rotate", 90);
    doc.save(&pdf).unwrap();
    let r = ctx_call("watermark", json!({ "files": [pdf.display().to_string()], "text": "CONFIDENCIAL", "opacity": 100, "size": 6, "out_dir": d.join("pdf").display().to_string() })).unwrap();
    assert_eq!(r["ok"], 1, "{r}");
    let marked = r["results"][0]["output"].as_str().unwrap().to_string();
    let imgs = ctx_call("pdf_to_images", json!({ "files": [marked], "dpi": 50, "out_dir": d.join("render").display().to_string() })).unwrap();
    assert_eq!(imgs["ok"], 1, "{imgs}");
    let mut pages: Vec<std::path::PathBuf> = Vec::new();
    let mut stack = vec![d.join("render")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { stack.push(p) } else if crate::tools::ext_lower(&p) == "png" { pages.push(p) }
        }
    }
    pages.sort();
    for p in &pages {
        let im = image::open(p).unwrap().to_rgb8();
        let (corner, top) = (bright(&im, 0.4, 0.85, 1.0, 1.0), bright(&im, 0.0, 0.0, 1.0, 0.5));
        println!("{}: {:?} canto {corner:.3}, metade de cima {top:.3}", p.display(), im.dimensions());
        assert!(corner > 0.01 && top == 0.0, "marca fora do lugar em {}", p.display());
    }
    assert_eq!(pages.len(), 2);
}

#[test]
fn flow_rename_with_options() {
    use crate::flow::{Flow, Inputs};
    use crate::tests_v15::{ctx_run, link, node};
    let Some(imgs) = std::env::var_os("CONVER_CLASSIFY_IMGS").map(std::path::PathBuf::from).filter(|d| d.is_dir() && crate::classify::installed()) else {
        eprintln!("sem as imagens de exemplo ou o modelo SigLIP 2; pulando");
        return;
    };
    let d = tmp("renomear");
    for f in ["gato.png", "cachorro.png", "praia.png"] {
        std::fs::copy(imgs.join(f), d.join(f)).unwrap();
    }
    // classificar → (a marca passa pelo redimensionar) → renomear "{opcao}_{n}" → salvar
    let f = Flow {
        trigger: Default::default(),
        name: "teste - renomear".into(),
        description: String::new(),
        nodes: vec![
            node(1, "input", json!({})),
            node(2, "classify", json!({ "options": "gato\ncachorro\npraia" })),
            node(3, "tool:image_resize", json!({ "percent": 50 })),
            node(4, "rename", json!({ "pattern": "{opcao}_{n}", "digits": 2 })),
            node(5, "output", json!({ "folder": d.join("saida").display().to_string() })),
        ],
        links: vec![link(1, "out", 2), link(2, "gato", 3), link(2, "cachorro", 3), link(2, "praia", 4), link(3, "out", 4), link(4, "out", 5)],
    };
    f.validate().unwrap();
    let r = ctx_run(&f, &Inputs { files: vec![d.join("gato.png"), d.join("cachorro.png"), d.join("praia.png")], texts: vec![] }, None, &Err("sem IA".into()));
    assert_eq!(r["failed_nodes"], 0, "{r}");
    let mut out: Vec<String> = std::fs::read_dir(d.join("saida")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    out.sort();
    println!("{out:?}");
    assert_eq!(out, vec!["cachorro_01.png", "gato_01.png", "praia_01.png"]);
    // o redimensionar realmente passou (metade do tamanho), e o original não mudou de nome
    assert_eq!(image::open(d.join("saida").join("gato_01.png")).unwrap().width(), image::open(d.join("gato.png")).unwrap().width() / 2);
    assert!(d.join("gato.png").is_file());
}

#[test]
fn flow_history_and_retry() {
    let d = tmp("historico");
    image::RgbImage::from_pixel(200, 100, image::Rgb([200, 30, 30])).save(d.join("boa.png")).unwrap();
    std::fs::write(d.join("quebrada.png"), b"isto nao e um png").unwrap();
    let name = "teste - historico";
    let flow = json!({ "name": name, "nodes": [
        { "id": 1, "kind": "input", "args": {} },
        { "id": 2, "kind": "tool:image_resize", "args": { "percent": 50 } },
        { "id": 3, "kind": "rename", "args": { "pattern": "{nome}_metade" } },
        { "id": 4, "kind": "output", "args": { "folder": d.join("saida").display().to_string() } } ],
        "links": [ { "from": 1, "to": 2 }, { "from": 2, "to": 3 }, { "from": 3, "to": 4 } ] });
    ctx_call("flow_save", json!({ "flow": flow })).unwrap();
    let r = ctx_call("flow_run", json!({ "name": name, "files": [d.join("boa.png").display().to_string(), d.join("quebrada.png").display().to_string()] })).unwrap();
    println!("{}", r["failed_inputs"]);
    assert_eq!(r["failed_inputs"].as_array().unwrap().len(), 1);
    assert!(r["failed_inputs"][0].as_str().unwrap().ends_with("quebrada.png"));
    let saved = |prefix: &str| std::fs::read_dir(d.join("saida")).unwrap().flatten().any(|e| {
        let n = e.file_name().to_string_lossy().into_owned();
        n.starts_with(prefix) && n.ends_with("_metade.png")
    });
    assert!(saved("boa_"), "o arquivo bom passou pelo fluxo todo");
    // histórico: a execução aparece com as entradas e a falha
    let h = ctx_call("flow_history", json!({ "name": name })).unwrap();
    println!("{h}");
    let last = &h["runs"][0];
    assert_eq!(last["inputs"].as_array().unwrap().len(), 2);
    assert_eq!(last["failed_inputs"].as_array().unwrap().len(), 1);
    assert!(last["seconds"].as_f64().is_some() && last["started"].is_string());
    // conserta o arquivo e repete só ele
    image::RgbImage::from_pixel(80, 40, image::Rgb([0, 0, 200])).save(d.join("quebrada.png")).unwrap();
    let r = ctx_call("flow_run", json!({ "name": name, "retry_failed": true })).unwrap();
    assert_eq!(r["failed_inputs"].as_array().unwrap().len(), 0, "{r}");
    let h = ctx_call("flow_history", json!({ "name": name })).unwrap();
    assert_eq!(h["runs"][0]["inputs"].as_array().unwrap().len(), 1, "a repetição usou só o arquivo que tinha falhado");
    assert!(saved("quebrada_"));
    assert!(ctx_call("flow_run", json!({ "name": name, "retry_failed": true })).is_err(), "sem falhas para repetir");
    ctx_call("flow_delete", json!({ "name": name })).unwrap();
}

#[test]
fn trigger_times() {
    use crate::triggers::parse_time;
    assert_eq!(parse_time("08:30"), Some((8, 30)));
    assert_eq!(parse_time("8h05"), Some((8, 5)));
    assert_eq!(parse_time("7"), Some((7, 0)));
    assert_eq!(parse_time("24:00"), None);
    assert_eq!(parse_time("ab"), None);
}

#[test]
fn trigger_folder_watch() {
    let d = tmp("gatilho_pasta");
    let (inbox, out) = (d.join("entrada"), d.join("saida"));
    std::fs::create_dir_all(&inbox).unwrap();
    image::RgbImage::from_pixel(100, 100, image::Rgb([9, 9, 9])).save(inbox.join("antiga.png")).unwrap();
    let name = "teste - gatilho pasta";
    // a saída vai para a própria pasta vigiada: não pode disparar de novo
    let flow = json!({ "name": name, "trigger": { "folder": inbox.display().to_string() }, "nodes": [
        { "id": 1, "kind": "input", "args": {} },
        { "id": 2, "kind": "tool:image_resize", "args": { "percent": 50 } },
        { "id": 3, "kind": "output", "args": { "folder": inbox.display().to_string() } },
        { "id": 4, "kind": "output", "args": { "folder": out.display().to_string() } } ],
        "links": [ { "from": 1, "to": 2 }, { "from": 2, "to": 3 }, { "from": 2, "to": 4 } ] });
    ctx_call("flow_save", json!({ "flow": flow })).unwrap();
    let mut w = crate::triggers::new_watches();
    // execuções de testes anteriores com o mesmo nome continuam no histórico: conta a partir daqui
    let base = crate::flow::history(name).len();
    let runs = || crate::flow::history(name).len() - base;
    let tick = |w: &mut _| {
        // só o fluxo deste teste importa; os outros fluxos do usuário com pasta seriam conferidos também
        crate::triggers::tick_for_test(w);
    };
    tick(&mut w); // começa a vigiar: a "antiga.png" não conta
    tick(&mut w);
    assert_eq!(runs(), 0, "arquivo que já estava na pasta não dispara");
    image::RgbImage::from_pixel(200, 120, image::Rgb([200, 9, 9])).save(inbox.join("nova.png")).unwrap();
    tick(&mut w); // vê o arquivo e anota o tamanho
    tick(&mut w); // tamanho parado: roda
    assert_eq!(runs(), 1);
    let made: Vec<String> = std::fs::read_dir(&out).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    println!("saída: {made:?} | status: {:?}", crate::triggers::folder_status(name));
    assert_eq!(made.len(), 1);
    assert!(made[0].starts_with("nova"));
    // o resultado salvo na própria pasta vigiada não dispara outra execução
    tick(&mut w);
    tick(&mut w);
    tick(&mut w);
    assert_eq!(runs(), 1, "o resultado do próprio fluxo disparou o fluxo de novo");
    ctx_call("flow_delete", json!({ "name": name })).unwrap();
}

#[test]
fn trigger_schedule_runs_flow() {
    let d = tmp("gatilho_horario");
    let name = "teste - gatilho horario";
    let flow = json!({ "name": name, "trigger": { "time": "23:58", "days": [1, 3] }, "nodes": [
        { "id": 1, "kind": "text", "args": { "text": "conver agendado" } },
        { "id": 2, "kind": "output", "args": { "folder": d.display().to_string() } } ],
        "links": [ { "from": 1, "to": 2 } ] });
    let r = ctx_call("flow_save", json!({ "flow": flow })).unwrap();
    println!("{r}");
    assert!(crate::triggers::is_scheduled(name), "{r}");
    // roda a tarefa agora, como o Windows faria no horário (o Conver da pasta target/release, sem janela)
    let task = format!("Conver\\Fluxo - {name}");
    let q = crate::tools::command(std::path::Path::new("schtasks.exe")).args(["/Query", "/TN", &task, "/V", "/FO", "LIST"]).output().unwrap();
    let info = String::from_utf8_lossy(&q.stdout);
    assert!(info.contains("--cli flow_run --name"), "{info}");
    let before = crate::flow::history(name).len();
    crate::tools::command(std::path::Path::new("schtasks.exe")).args(["/Run", "/TN", &task]).output().unwrap();
    let t = std::time::Instant::now();
    while crate::flow::history(name).len() == before && t.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    println!("tarefa rodou em {:.1}s", t.elapsed().as_secs_f32());
    assert!(crate::flow::history(name).len() > before, "a tarefa agendada não executou o fluxo");
    assert!(std::fs::read_dir(&d).unwrap().flatten().any(|e| std::fs::read_to_string(e.path()).is_ok_and(|t| t.contains("conver agendado"))));
    // tirar o horário (e apagar o fluxo) remove a tarefa
    ctx_call("flow_delete", json!({ "name": name })).unwrap();
    assert!(!crate::triggers::is_scheduled(name));
}

#[test]
fn faces_detect_and_hide() {
    use crate::faces;
    let Some(imgs) = std::env::var_os("CONVER_CLASSIFY_IMGS").map(std::path::PathBuf::from).filter(|d| d.join("pessoa.png").is_file()) else {
        eprintln!("sem as imagens de exemplo; pulando");
        return;
    };
    // uma pessoa: um rosto, na parte de cima do quadro
    let img = image::open(imgs.join("pessoa.png")).unwrap().to_rgb8();
    let t = std::time::Instant::now();
    let f = faces::detect(&img, 0.6).unwrap();
    println!("astronauta: {} rosto(s) em {} ms: {:?}", f.len(), t.elapsed().as_millis(), f);
    assert_eq!(f.len(), 1);
    assert!(f[0].y + f[0].h < img.height() as f32 * 0.6, "o rosto está na metade de cima");
    // sem rostos: gato, praia, documento
    for name in ["gato.png", "praia.png", "documento.png"] {
        let n = faces::detect(&image::open(imgs.join(name)).unwrap().to_rgb8(), 0.6).unwrap().len();
        assert_eq!(n, 0, "{name}: {n} rosto(s)");
    }
    // grupo: vários rostos
    if imgs.join("selfie_grupo.jpg").is_file() {
        let g = image::open(imgs.join("selfie_grupo.jpg")).unwrap().to_rgb8();
        let n = faces::detect(&g, 0.6).unwrap().len();
        println!("selfie em grupo: {n} rostos");
        assert!(n >= 6);
    }
    // esconder: a área do rosto muda, o resto da imagem fica igual
    let d = tmp("rostos");
    let r = ctx_call("image_hide_faces", json!({ "files": [imgs.join("pessoa.png").display().to_string()], "style": "pixelate", "out_dir": d.display().to_string() })).unwrap();
    assert_eq!(r["ok"], 1, "{r}");
    let out = image::open(r["results"][0]["output"].as_str().unwrap()).unwrap().to_rgb8();
    let fb = f[0];
    let changed = |x0: u32, y0: u32, x1: u32, y1: u32| (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| img.get_pixel(x, y) != out.get_pixel(x, y)).count();
    let (cx, cy) = ((fb.x + fb.w / 2.0) as u32, (fb.y + fb.h / 2.0) as u32);
    assert!(changed(cx - 10, cy - 10, cx + 10, cy + 10) > 300, "o rosto foi pixelado");
    let bottom = img.height() - 40;
    assert_eq!(changed(0, bottom, img.width(), img.height()), 0, "fora do rosto nada muda");
    // vídeo curto
    if let Some(v) = std::env::var_os("CONVER_FACE_VIDEO").map(std::path::PathBuf::from).filter(|v| v.is_file()) {
        let t = std::time::Instant::now();
        let r = ctx_call("image_hide_faces", json!({ "files": [v.display().to_string()], "style": "blur", "out_dir": d.display().to_string() })).unwrap();
        println!("vídeo: {} em {:.1}s", r["results"][0]["info"], t.elapsed().as_secs_f32());
        assert_eq!(r["ok"], 1, "{r}");
        let o = crate::video::probe(std::path::Path::new(r["results"][0]["output"].as_str().unwrap()));
        let i = crate::video::probe(&v);
        assert!(o.has_audio && o.size == i.size && (o.duration.unwrap() - i.duration.unwrap()).abs() < 0.5, "{o:?} vs {i:?}");
    }
}

#[test]
fn pdf_tables_numbers() {
    use crate::pdf_tables::parse_number;
    assert_eq!(parse_number("1.234,56"), Some(1234.56));
    assert_eq!(parse_number("R$ 25,90"), Some(25.9));
    assert_eq!(parse_number("1,234.56"), Some(1234.56));
    assert_eq!(parse_number("3.5"), Some(3.5));
    assert_eq!(parse_number("1.234"), Some(1234.0));
    assert_eq!(parse_number("-7"), Some(-7.0));
    assert_eq!(parse_number("12%"), Some(12.0));
    assert_eq!(parse_number("Arroz"), None);
    assert_eq!(parse_number("12/05"), None);
}

#[test]
fn pdf_tables_extract() {
    let d = tmp("tabelas");
    // uma "página escaneada": título, uma tabela de 3 colunas e um rodapé
    let font = r"C\:/Windows/Fonts/arial.ttf";
    let rows = [("Produto", "Qtd", "Preço"), ("Arroz integral", "2", "25,90"), ("Feijão", "3", "8,49"), ("Café especial", "1", "1.234,56")];
    let mut f = format!("drawtext=fontfile='{font}':text='Relatório de compras':x=120:y=110:fontsize=56:fontcolor=black");
    for (i, (a, b, c)) in rows.iter().enumerate() {
        let y = 260 + i * 90;
        for (x, t) in [(120, a), (900, b), (1250, c)] {
            f.push_str(&format!(",drawtext=fontfile='{font}':text='{t}':x={x}:y={y}:fontsize=44:fontcolor=black"));
        }
    }
    f.push_str(&format!(",drawtext=fontfile='{font}':text='Obrigado pela preferência.':x=120:y=760:fontsize=40:fontcolor=black"));
    let png = d.join("pagina.png");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=white:s=1700x2200", "-frames:v", "1", "-vf", &f, &png.to_string_lossy()], None);
    let r = ctx_call("images_to_pdf", json!({ "files": [png.display().to_string()], "out_dir": d.display().to_string() })).unwrap();
    let pdf = r["output"].as_str().unwrap().to_string();
    let r = ctx_call("pdf_tables", json!({ "files": [pdf.clone()], "format": "csv", "out_dir": d.display().to_string() })).unwrap();
    assert_eq!(r["ok"], 1, "{r}");
    let csv = std::fs::read_to_string(r["results"][0]["output"].as_str().unwrap()).unwrap();
    println!("{csv}");
    let lines: Vec<Vec<String>> = csv.trim_start_matches('\u{feff}').lines().map(|l| l.split(';').map(|c| c.trim().to_string()).collect()).collect();
    assert_eq!(lines.len(), 4, "só as 4 linhas da tabela (título e rodapé ficam de fora)");
    assert!(lines.iter().all(|l| l.len() == 3), "3 colunas em todas as linhas");
    assert_eq!(lines[0], vec!["Produto", "Qtd", "Preço"]);
    assert_eq!(lines[3][0], "Café especial");
    assert_eq!(crate::pdf_tables::parse_number(&lines[3][2]), Some(1234.56));
    // .xlsx também sai
    let r = ctx_call("pdf_tables", json!({ "files": [pdf], "out_dir": d.join("x").display().to_string() })).unwrap();
    let x = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    assert!(x.extension().unwrap() == "xlsx" && crate::tools::file_size(&x) > 3000, "{r}");
}

#[test]
fn audio_classify_clap() {
    if !crate::audioclass::installed() {
        eprintln!("modelo CLAP não instalado; pulando");
        return;
    }
    // ids conhecidos do RoBERTa
    let ids = crate::audioclass::encode_for_test("This is a sound of music.");
    println!("{ids:?}");
    assert_eq!(&ids[..6], &[0, 713, 16, 10, 2369, 9]);
    assert_eq!(&ids[ids.len() - 2..], &[4, 2]);
    let d = tmp("sons");
    let fala = speech(&d, "Good morning everyone. Today we are going to talk about the weather and the news of the week.");
    let musica = d.join("musica.wav");
    // acordes que mudam a cada meio segundo, com batida
    ffmpeg(&["-f", "lavfi", "-i", "aevalsrc='0.3*sin(2*PI*(262+131*mod(floor(t*2),4))*t)+0.2*sin(2*PI*(330+165*mod(floor(t*2),3))*t)+0.2*sin(2*PI*392*t)*lt(mod(t,0.5),0.08)':d=8:s=44100", &musica.to_string_lossy()], None);
    let silencio = d.join("silencio.wav");
    ffmpeg(&["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono", "-t", "5", &silencio.to_string_lossy()], None);
    let ruido = d.join("ruido.wav");
    ffmpeg(&["-f", "lavfi", "-i", "anoisesrc=c=white:a=0.3:d=6", &ruido.to_string_lossy()], None);
    let opts: Vec<String> = ["fala", "música", "silêncio", "ruído"].iter().map(|s| s.to_string()).collect();
    let mut ok = 0;
    for (f, want) in [(&fala, "fala"), (&musica, "música"), (&silencio, "silêncio"), (&ruido, "ruído")] {
        let t = std::time::Instant::now();
        let r = crate::audioclass::classify_file(f, &opts, None).unwrap();
        println!("{} → {} ({:.0}%)  [{}]  {} ms", crate::tools::file_name(f), r[0].label, r[0].prob * 100.0,
            r.iter().map(|c| format!("{} {:.0}%", c.label, c.prob * 100.0)).collect::<Vec<_>>().join(", "), t.elapsed().as_millis());
        if r[0].label == want {
            ok += 1;
        }
    }
    println!("acertos: {ok}/4");
    assert!(ok >= 3);
}

#[test]
#[ignore] // experimento: qual frase em inglês representa melhor "fala"
fn audio_speech_prompts() {
    let d = std::env::temp_dir().join("conver_tests").join("sons");
    let real = std::env::var_os("CONVER_FACE_VIDEO").map(std::path::PathBuf::from);
    for f in [Some(d.join("fala.wav")), real].into_iter().flatten() {
        for fala in ["speech", "a person speaking", "a man speaking", "people talking", "talking", "narration"] {
            let opts: Vec<String> = [fala, "music", "silence", "noise"].iter().map(|s| s.to_string()).collect();
            let r = crate::audioclass::classify_file(&f, &opts, None).unwrap();
            let p = r.iter().find(|c| c.label == fala).unwrap().prob;
            println!("{:<22} {:<20} fala {:.0}%  (1º: {})", crate::tools::file_name(&f), fala, p * 100.0, r[0].label);
        }
    }
}

#[test]
fn classify_op_sounds_and_alias() {
    if !crate::audioclass::installed() {
        eprintln!("modelo CLAP não instalado; pulando");
        return;
    }
    let d = tmp("classificar_sons");
    let fala = speech(&d, "This is a short test of the local sound classifier.");
    let silencio = d.join("silencio.wav");
    ffmpeg(&["-f", "lavfi", "-i", "anullsrc=r=44100:cl=mono", "-t", "3", &silencio.to_string_lossy()], None);
    // o nome antigo da ferramenta continua valendo; "auto" olha o som dos áudios
    let r = ctx_call("image_classify", json!({ "files": [fala.display().to_string(), silencio.display().to_string()], "options": "fala, silêncio, música", "sort_into_folders": true, "out_dir": d.join("pastas").display().to_string() })).unwrap();
    println!("{r}");
    assert_eq!(r["counts"]["fala"], 1);
    assert_eq!(r["counts"]["silêncio"], 1);
    assert!(d.join("pastas").join("fala").join("fala.wav").is_file());
}

#[test]
fn dub_subtitle_without_translation() {
    let Some(voice) = crate::tts::voice("pt_BR-faber-medium").filter(|v| crate::tts::installed(v)) else {
        eprintln!("voz Faber não instalada; pulando");
        return;
    };
    let d = tmp("dublagem");
    let vid = d.join("aula.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0x223344:s=320x240:d=8", "-f", "lavfi", "-i", "sine=f=220:d=8", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", &vid.to_string_lossy()], None);
    // a segunda frase tem pouco tempo: precisa ser acelerada
    std::fs::write(d.join("aula.srt"), "1\n00:00:00,500 --> 00:00:03,000\nOlá, bem-vindos à aula de hoje.\n\n2\n00:00:03,200 --> 00:00:04,400\nVamos falar sobre legendas, traduções e dublagem automática no computador.\n\n3\n00:00:05,500 --> 00:00:07,500\nAté logo!\n").unwrap();
    let r = ctx_call("subtitle_translate", json!({ "files": [d.join("aula.srt").display().to_string()], "dub": true, "voice": voice.id, "original_volume": 10 })).unwrap();
    println!("{r}");
    assert_eq!(r["ok"], 1, "{r}");
    let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    assert!(out.file_name().unwrap().to_string_lossy().contains("_dublado"), "o vídeo de mesmo nome recebeu a dublagem");
    let p = crate::video::probe(&out);
    assert!(p.has_video && p.has_audio && (p.duration.unwrap() - 8.0).abs() < 0.5, "{p:?}");
    assert!(r["results"][0]["info"].as_str().unwrap().contains("acelerada"), "a fala longa foi acelerada");
    assert!(crate::tests_v18::lufs(&out) > -40.0, "tem fala na faixa");
}

#[test]
fn llm_translate_and_dub() {
    if !crate::llm::installed() {
        eprintln!("IA de linguagem não instalada; pulando");
        return;
    }
    let Some(voice) = crate::tts::voice("pt_BR-faber-medium").filter(|v| crate::tts::installed(v)) else {
        eprintln!("voz Faber não instalada; pulando");
        return;
    };
    let d = tmp("traduzir_dublar");
    let vid = d.join("talk.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0x334422:s=320x240:d=8", "-f", "lavfi", "-i", "sine=f=220:d=8", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", &vid.to_string_lossy()], None);
    std::fs::write(d.join("talk.srt"), "1\n00:00:00,500 --> 00:00:03,000\nGood morning, everyone.\n\n2\n00:00:03,200 --> 00:00:05,200\nToday we will learn how to cook rice.\n\n3\n00:00:05,500 --> 00:00:07,500\nSee you tomorrow!\n").unwrap();
    let t = std::time::Instant::now();
    let r = ctx_call("subtitle_translate", json!({ "files": [d.join("talk.srt").display().to_string()], "to": "pt", "dub": true, "voice": voice.id })).unwrap();
    println!("{r}\n{:.1} s", t.elapsed().as_secs_f32());
    assert_eq!(r["ok"], 1, "{r}");
    let srt = std::fs::read_to_string(d.join("talk_pt.srt")).expect("legenda traduzida");
    println!("{srt}");
    let low = srt.to_lowercase();
    assert!(low.contains("arroz"), "tradução de \"rice\"");
    assert!(low.contains("amanhã"), "tradução de \"tomorrow\"");
    assert_eq!(srt.matches("-->").count(), 3, "mesmas 3 falas");
    let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    let p = crate::video::probe(&out);
    assert!(p.has_video && p.has_audio, "{p:?}");
}

#[test]
fn llm_classify_texts() {
    if !crate::llm::installed() {
        eprintln!("IA de linguagem não instalada; pulando");
        return;
    }
    let d = tmp("classificar_textos");
    let textos = [
        ("pedido.txt", "Olá, comprei uma cafeteira há duas semanas e ela chegou quebrada. Quero meu dinheiro de volta urgente!"),
        ("elogio.txt", "Adorei o atendimento de vocês, a entrega foi rapidíssima e o produto é ótimo. Parabéns à equipe!"),
        ("fatura.txt", "Fatura nº 2291 — Vencimento 10/12 — Total a pagar: R$ 349,90. Pague pelo código de barras abaixo."),
    ];
    let files: Vec<String> = textos.iter().map(|(n, t)| {
        let p = d.join(n);
        std::fs::write(&p, t).unwrap();
        p.display().to_string()
    }).collect();
    let t = std::time::Instant::now();
    let r = ctx_call("classify", json!({ "files": files, "options": "reclamação, elogio, cobrança", "look_at": "text", "sort_into_folders": true, "out_dir": d.join("pastas").display().to_string() })).unwrap();
    println!("{r}\n{:.1} s", t.elapsed().as_secs_f32());
    assert!(d.join("pastas").join("reclamação").join("pedido.txt").is_file(), "{r}");
    assert!(d.join("pastas").join("elogio").join("elogio.txt").is_file(), "{r}");
    assert!(d.join("pastas").join("cobrança").join("fatura.txt").is_file(), "{r}");
}


#[test]
#[ignore] // baixa o llama.cpp do GitHub (38 MB)
fn llm_runtime_download() {
    let d = tmp("llama_runtime");
    let _ = std::fs::remove_dir_all(&d);
    let exe = crate::llm::install_runtime(&d.join("llm"), &|_| {}, &|| false).unwrap();
    assert!(exe.is_file());
    let n = std::fs::read_dir(d.join("llm")).unwrap().count();
    println!("{n} arquivos em {}", d.join("llm").display());
    assert_eq!(n, 25, "só o servidor e as bibliotecas");
    assert!(!d.join("llama-b12121-bin-win-vulkan-x64.zip").exists(), "o zip é apagado");
    // o servidor extraído abre
    let out = std::process::Command::new(&exe).arg("--version").output().unwrap();
    println!("{}", String::from_utf8_lossy(&out.stderr).lines().take(3).collect::<Vec<_>>().join(" | "));
}

#[test]
fn welcome_and_models() {
    use crate::models::{Downloads, Pack, Purpose};
    // quem já usava o Conver (config.json sem o campo) não vê as boas-vindas; um config novo vê
    let old: crate::Config = serde_json::from_str(r#"{"dark":true,"lang":"pt"}"#).unwrap();
    assert!(old.welcome_done);
    assert!(!crate::Config::default().welcome_done);
    // finalidades sem IA não sugerem nada; as outras sugerem pacotes que existem no catálogo
    assert!(Purpose::Convert.suggests().is_empty() && Purpose::Docs.suggests().is_empty());
    for p in Purpose::ALL {
        assert!(p.suggests().iter().all(|k| Pack::ALL.contains(k)));
    }
    // tamanhos batem com os arquivos; o pacote de linguagem inclui o llama.cpp
    for k in Pack::ALL {
        assert!(k.size() > 1_000_000, "{k:?}");
        assert!(!k.files().is_empty());
    }
    assert_eq!(Pack::Language.size(), crate::llm::RUNTIME.size + crate::llm::MODEL.size);
    // pacotes já no computador não entram na fila
    let ctx = eframe::egui::Context::default();
    let d = Downloads::default();
    let installed: Vec<Pack> = Pack::ALL.into_iter().filter(|k| k.installed()).collect();
    d.add(&ctx, &installed);
    assert!(!d.busy(), "nada a baixar");
    // moldes com prefixo ("✔ Baixado • {}") traduzem inteiros
    crate::i18n::set("en");
    assert_eq!(crate::i18n::tr("✔ Baixado • 78.0 MB"), "✔ Downloaded • 78.0 MB");
    assert_eq!(crate::i18n::tr("✔ Transcrição (Whisper) pronto neste computador."), "✔ Transcription (Whisper) ready on this computer.");
    crate::i18n::set("pt");
}

#[test]
fn rtl_layout_helpers() {
    use eframe::egui::{Align, Rect, pos2};
    crate::i18n::set("ar");
    assert!(crate::widgets::rtl());
    assert!(crate::widgets::down().prefer_right_to_left());
    assert!(crate::widgets::fwd(Align::Center).prefer_right_to_left());
    assert!(!crate::widgets::back(Align::Center).prefer_right_to_left());
    let outer = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 10.0));
    let r = Rect::from_min_max(pos2(10.0, 0.0), pos2(30.0, 10.0));
    assert_eq!(crate::widgets::mirror(r, outer), Rect::from_min_max(pos2(70.0, 0.0), pos2(90.0, 10.0)));
    crate::i18n::set("pt");
    assert!(!crate::widgets::rtl());
    assert_eq!(crate::widgets::mirror(r, outer), r);
}

#[test]
fn flow_nodes_in_portuguese() {
    // todo ajuste de node tem nome em português, todo valor de lista aparece traduzido e todo tipo tem explicação
    let mut missing = std::collections::BTreeSet::new();
    for k in crate::flow::kinds() {
        if crate::flows_page::node_help(&k.kind).is_empty() {
            missing.insert(format!("ajuda: {}", k.kind));
        }
        for (key, prop) in k.schema["properties"].as_object().into_iter().flatten() {
            if crate::flows_page::arg_label(key) == key {
                missing.insert(format!("rótulo: {key} ({})", k.kind));
            }
            for v in prop["enum"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
                let shown = crate::flows_page::enum_text(v);
                if shown == v && v.chars().any(|c| c.is_ascii_lowercase()) && !v.ends_with('p') {
                    missing.insert(format!("valor: {v} ({key})"));
                }
            }
        }
    }
    assert!(missing.is_empty(), "sem texto em português: {missing:?}");
    assert_eq!(crate::flows_page::enum_text("1080p"), "1080p");
    assert_eq!(crate::flows_page::enum_text("mp4"), "MP4");
}


/// Foto sintética: mesa escura com ruído e uma folha branca torta com linhas de "texto" e uma sombra.
fn fake_paper_photo() -> (image::RgbImage, [(f32, f32); 4]) {
    let quad = [(220.0, 160.0), (1010.0, 110.0), (1060.0, 820.0), (160.0, 770.0)];
    let inside = |x: f32, y: f32| {
        (0..4).all(|i| {
            let (a, b) = (quad[i], quad[(i + 1) % 4]);
            (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0) >= 0.0
        })
    };
    let mut seed = 7u32;
    let img = image::RgbImage::from_fn(1200, 900, |x, y| {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let noise = (seed >> 27) as u8;
        let (fx, fy) = (x as f32, y as f32);
        if inside(fx, fy) {
            // sombra suave no canto de baixo e linhas de texto a cada 40 px
            let shade = (fy / 900.0 * 40.0) as u8;
            let text = (fy as u32 % 40) < 6 && fx > 300.0 && fx < 900.0;
            let v = if text { 40 } else { 235 - shade };
            image::Rgb([v, v, v.saturating_sub(8)])
        } else {
            image::Rgb([70 + noise, 50 + noise, 35 + noise])
        }
    });
    (img, quad)
}

#[test]
fn document_scan() {
    let (img, quad) = fake_paper_photo();
    let found = crate::docscan::detect(&img).expect("achou a folha");
    for (a, b) in found.iter().zip(quad.iter()) {
        let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
        assert!(d < 20.0, "canto {a:?} longe de {b:?} ({d:.1} px)");
    }
    let (doc, ok) = crate::docscan::scan(&img, &crate::docscan::Opts { mode: crate::docscan::Mode::Documento, crop: true });
    assert!(ok);
    println!("cantos {found:?} → {}×{}", doc.width(), doc.height());
    assert!((doc.width() as f32 - 850.0).abs() < 60.0 && (doc.height() as f32 - 680.0).abs() < 60.0);
    // preto e branco: quase tudo branco (papel), um pouco de preto (texto), e nada da mesa nas bordas
    let px: Vec<u8> = doc.pixels().map(|p| p[0]).collect();
    let black = px.iter().filter(|&&v| v == 0).count() as f32 / px.len() as f32;
    let white = px.iter().filter(|&&v| v == 255).count() as f32 / px.len() as f32;
    println!("preto {:.1}% branco {:.1}%", black * 100.0, white * 100.0);
    assert!(white > 0.75 && black > 0.03 && black < 0.25);
    // sem recorte: mesmo tamanho da foto
    let (whole, _) = crate::docscan::scan(&img, &crate::docscan::Opts { mode: crate::docscan::Mode::Cor, crop: false });
    assert_eq!(whole.dimensions(), img.dimensions());
    // salvar como imagem e como PDF
    let d = crate::tests_v12::tmp("escanear");
    let src = d.join("folha.jpg");
    img.save(&src).unwrap();
    doc.save(d.join("folha_escaneada.png")).unwrap();
    let r = crate::tests_v12::ctx_call("document_scan", serde_json::json!({ "files": [src.display().to_string(), src.display().to_string()], "format": "pdf", "mode": "document" })).unwrap();
    println!("{r}");
    let pdf = std::path::PathBuf::from(r["output"].as_str().unwrap());
    assert_eq!(crate::pdf::open(&pdf).unwrap().get_pages().len(), 2);
}

#[test]
fn flow_condition_rename_qr_import() {
    use crate::tests_v15::{link, node};
    let d = crate::tests_v12::tmp("condicao");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let deitada = d.join("deitada.png");
    let empe = d.join("em pe.png");
    image::RgbImage::new(300, 200).save(&deitada).unwrap();
    image::RgbImage::new(200, 300).save(&empe).unwrap();
    // condição: vertical → sim; e o "sim" passa por Renomear em lote (cópia com prefixo, o original fica)
    let mut f = crate::flow::Flow { name: "teste - condicao".into(), ..Default::default() };
    f.nodes = vec![
        node(1, "input", serde_json::json!({ "files": [deitada.display().to_string(), empe.display().to_string()] })),
        node(2, "condition", serde_json::json!({ "check": "orientation", "orientation": "vertical" })),
        node(3, "tool:files_rename", serde_json::json!({ "prefix": "story_", "find": " ", "replace": "-" })),
    ];
    f.links = vec![link(1, "out", 2), link(2, "yes", 3)];
    crate::flow::save(&f).unwrap();
    let r = crate::tests_v12::ctx_call("flow_run", serde_json::json!({ "name": "teste - condicao" })).unwrap();
    let nodes = r["nodes"].as_array().unwrap();
    let cond = nodes.iter().find(|n| n["id"] == 2).unwrap();
    println!("{}", cond["note"]);
    assert_eq!(cond["outputs"]["yes"].as_array().unwrap().len(), 1);
    assert_eq!(cond["outputs"]["no"].as_array().unwrap().len(), 1);
    let renamed = nodes.iter().find(|n| n["id"] == 3).unwrap()["outputs"]["out"][0]["file"].as_str().unwrap().to_string();
    assert!(renamed.ends_with("story_em-pe.png"), "{renamed}");
    assert!(empe.is_file(), "o original continua");
    // duração: imagem não tem duração → "não"
    assert!(!super::flow::condition_passes_for_test(&deitada, "duration", true, 1.0));
    assert!(super::flow::condition_passes_for_test(&deitada, "width", true, 250.0));
    // exportar/importar: o arquivo exportado volta como fluxo; node desconhecido é recusado
    let exp = d.join("fluxo.json");
    std::fs::write(&exp, serde_json::to_string(&f).unwrap()).unwrap();
    let back = crate::flows_page::import_flow(&exp).unwrap();
    assert_eq!(back.nodes.len(), 3);
    let mut bad = f.clone();
    bad.nodes[1].kind = "tool:nao_existe".into();
    std::fs::write(&exp, serde_json::to_string(&bad).unwrap()).unwrap();
    assert!(crate::flows_page::import_flow(&exp).is_err());
    let _ = crate::flow::remove("teste - condicao");
    // QR com correção de erro alta fica mais denso que com a baixa
    let low = crate::tests_v12::ctx_call("qr_code", serde_json::json!({ "text": "https://github.com/JhonyGoncalves/conver", "error_correction": "low", "output": d.join("baixa.png").display().to_string() })).unwrap();
    let high = crate::tests_v12::ctx_call("qr_code", serde_json::json!({ "text": "https://github.com/JhonyGoncalves/conver", "error_correction": "high", "output": d.join("alta.png").display().to_string() })).unwrap();
    assert!(std::path::Path::new(low["output"].as_str().unwrap()).is_file() && std::path::Path::new(high["output"].as_str().unwrap()).is_file());
    assert!(crate::ops::find("audio_record").is_some() && crate::ops::find("document_scan").is_some());
}

#[test]
#[ignore] // cria e roda um fluxo de demonstração para as capturas do editor (apague "Teste - controles" depois)
fn tmp_demo_flow_for_capture() {
    use crate::tests_v15::{link, node};
    let a = std::path::PathBuf::from(std::env::var("CONVER_AMOSTRAS").unwrap());
    let files: Vec<String> = ["foto-viagem-praia.png", "capa.jpg"].iter().map(|f| a.join(f).display().to_string()).collect();
    let mut f = crate::flow::Flow { name: "Teste - controles".into(), ..Default::default() };
    f.nodes = vec![
        node(1, "input", serde_json::json!({ "files": files })),
        node(2, "tool:watermark", serde_json::json!({ "text": "© Conver", "color": "#f13030", "position": "bottom_right", "opacity": 70 })),
        node(3, "condition", serde_json::json!({ "check": "width", "compare": "more", "value": 1500 })),
    ];
    f.links = vec![link(1, "out", 2), link(2, "out", 3)];
    for (i, n) in f.nodes.iter_mut().enumerate() {
        n.pos = [i as f32 * 260.0, 80.0];
    }
    crate::flow::save(&f).unwrap();
    let r = crate::tests_v12::ctx_call("flow_run", serde_json::json!({ "name": "Teste - controles" })).unwrap();
    println!("{}", r["nodes"]);
}

#[test]
#[ignore] // gera imagens com os três modelos (baixa até 13 GB e usa a placa de vídeo)
fn image_models_compare() {
    use crate::imagegen::{Engine, Quality, Request, Shape};
    let d = crate::tests_v12::tmp("modelos_imagem");
    let prompts = [
        ("cabana", "a cozy wooden cabin in a snowy pine forest at sunset, warm light in the windows, smoke from the chimney, photorealistic"),
        ("retrato", "close-up portrait photo of an elderly woman laughing, holding a cup of coffee with both hands, natural window light"),
        ("placa", "a vintage coffee shop sign that says \"CONVER CAFE\" in bold letters, street at night, neon reflections"),
    ];
    for e in [Engine::Rapido, Engine::Xl, Engine::ZImage] {
        if !e.installed() {
            // baixa pelo mesmo caminho do app (SHA-256 conferido)
            let t = std::time::Instant::now();
            for f in e.files() {
                crate::ai::ensure_model(&f, &|_| {}, &|| false).unwrap();
            }
            println!("RESULT {e:?}: baixado em {:.0} s", t.elapsed().as_secs_f32());
        }
        for (name, prompt) in prompts {
            let req = Request { prompt: prompt.into(), negative: String::new(), shape: Shape::Quadrada, quality: Quality::Rapida, count: 1, seed: Some(42), engine: e };
            let out = d.join(format!("{e:?}_{name}"));
            let _ = std::fs::remove_dir_all(&out);
            let t = std::time::Instant::now();
            match crate::imagegen::generate(&req, &out, &|_| {}, &|| false) {
                Ok(files) => {
                    let f = &files[0];
                    let dims = image::image_dimensions(f).unwrap();
                    println!("RESULT {e:?} {name}: {:.1} s {dims:?} {}", t.elapsed().as_secs_f32(), f.display());
                    std::fs::copy(f, d.join(format!("{e:?}_{name}.png"))).unwrap();
                }
                Err(err) => println!("RESULT {e:?} {name}: ERRO {err} ({:.1} s)", t.elapsed().as_secs_f32()),
            }
        }
    }
}

#[test]
#[ignore] // gera uma imagem com o Z-Image pelo caminho do app (minutos na placa de vídeo)
fn image_generate_z_image_op() {
    assert!(crate::imagegen::Engine::ZImage.installed(), "baixe o Z-Image antes");
    assert_eq!(crate::imagegen::Engine::best_installed(), crate::imagegen::Engine::ZImage);
    let d = crate::tests_v12::tmp("gerar_z");
    let _ = std::fs::remove_dir_all(&d);
    let t = std::time::Instant::now();
    let r = crate::tests_v12::ctx_call("image_generate", serde_json::json!({ "prompt": "a red bicycle leaning on a blue wooden door, sunny day", "model": "auto", "seed": 7, "out_dir": d.display().to_string() })).unwrap();
    let f = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    println!("RESULT z_image op: {:.0} s {:?}", t.elapsed().as_secs_f32(), image::image_dimensions(&f).unwrap());
    assert_eq!(image::image_dimensions(&f).unwrap(), (768, 768));
}

#[test]
fn background_refine() {
    use crate::bgremove::{Dot, Refine, paint_mask, refine};
    // máscara crua: um degradê da esquerda (0) para a direita (255)
    let raw = image::GrayImage::from_fn(100, 20, |x, _| image::Luma([(x as f32 / 99.0 * 255.0) as u8]));
    let kept = |m: &image::GrayImage| m.pixels().filter(|p| p[0] > 127).count();
    let base = Refine { antialias: false, ..Default::default() };
    let mid = refine(&raw, &base, false, None, None);
    let strict = refine(&raw, &Refine { precision: 90.0, ..base }, false, None, None);
    let loose = refine(&raw, &Refine { precision: 10.0, ..base }, false, None, None);
    assert!(kept(&strict) < kept(&mid) && kept(&mid) < kept(&loose), "precisão maior corta mais");
    let grown = refine(&raw, &Refine { grow: 5, ..base }, false, None, None);
    assert!(kept(&grown) > kept(&mid), "engrossar mantém mais");
    // pincel: manter à esquerda e tirar à direita
    let keep = paint_mask(100, 20, &[Dot { x: 0.05, y: 0.5, r: 0.04 }]);
    let remove = paint_mask(100, 20, &[Dot { x: 0.95, y: 0.5, r: 0.04 }]);
    let painted = refine(&raw, &base, false, Some(&keep), Some(&remove));
    assert_eq!(painted.get_pixel(5, 10)[0], 255);
    assert_eq!(painted.get_pixel(95, 10)[0], 0);
    // anticerrilhado: a borda definida ganha tons intermediários
    let hard = refine(&raw, &Refine { hard: true, antialias: false, ..base }, false, None, None);
    let smooth = refine(&raw, &Refine { hard: true, antialias: true, ..base }, false, None, None);
    let mids = |m: &image::GrayImage| m.pixels().filter(|p| p[0] > 10 && p[0] < 245).count();
    assert!(mids(&smooth) >= mids(&hard));
}

#[test]
fn background_by_color_keeps_inside() {
    use crate::bgremove::{Detect, Interior, Refine, Strokes, cut, flat_background};
    // um anel ciano sobre fundo escuro: o miolo tem a cor do fundo
    let (bg, line) = (image::Rgb([18u8, 18, 19]), image::Rgb([0u8, 190, 255]));
    let ring = image::RgbImage::from_fn(120, 120, |x, y| {
        let d = ((x as f32 - 60.0).powi(2) + (y as f32 - 60.0).powi(2)).sqrt();
        if (30.0..38.0).contains(&d) { line } else { bg }
    });
    assert_eq!(flat_background(&ring), Some([18, 18, 19]));
    let (clean, m) = cut(&ring, &Refine::default(), &Strokes::default()).unwrap();
    // pixel de borda meio a meio entre o traço e o fundo: fica meio transparente e com a cor cheia do traço
    let mut blend = ring.clone();
    blend.put_pixel(60, 21, image::Rgb([9, 104, 137]));
    let (c, a) = cut(&blend, &Refine { antialias: false, ..Default::default() }, &Strokes::default()).unwrap();
    assert!((100..=155).contains(&a.get_pixel(60, 21)[0]), "alfa da borda: {}", a.get_pixel(60, 21)[0]);
    assert!(c.get_pixel(60, 21)[2] > 235, "cor da borda: {:?}", c.get_pixel(60, 21));
    assert_eq!(m.get_pixel(60, 60)[0], 255, "o interior fica");
    assert_eq!(m.get_pixel(5, 5)[0], 0, "o fundo sai");
    assert_eq!(m.get_pixel(60, 26)[0], 255, "o traço fica");
    assert_eq!(*clean.get_pixel(60, 60), bg, "o interior não muda de cor");
    // tirar o interior, como dentro de um O
    let open = Refine { interior: Interior::Tirar, ..Default::default() };
    assert_eq!(cut(&ring, &open, &Strokes::default()).unwrap().1.get_pixel(60, 60)[0], 0);
    // um vão no contorno: pintar "manter" sobre ele fecha o objeto
    let mut gap = ring.clone();
    for y in 20..32 {
        for x in 57..63 {
            gap.put_pixel(x, y, bg);
        }
    }
    assert_eq!(cut(&gap, &Refine::default(), &Strokes::default()).unwrap().1.get_pixel(60, 60)[0], 0, "com vão grande, o fundo entra");
    // vão mínimo entre dois traços: fecha sozinho, e o lado de fora continua exato
    let mut tiny = ring.clone();
    for y in 20..32 {
        for x in 59..61 {
            tiny.put_pixel(x, y, bg);
        }
    }
    let t = cut(&tiny, &Refine::default(), &Strokes::default()).unwrap().1;
    assert_eq!(t.get_pixel(60, 60)[0], 255, "vão mínimo fecha");
    assert_eq!(t.get_pixel(5, 5)[0], 0);
    assert_eq!(t.get_pixel(60, 18)[0], 0, "fora do traço continua fundo");
    let dot = crate::bgremove::Dot { x: 0.5, y: 22.0 / 120.0, r: 0.06 };
    let closed = Strokes { keep: vec![crate::bgremove::Dot { y: 26.0 / 120.0, ..dot }, dot], ..Default::default() };
    assert_eq!(cut(&gap, &Refine::default(), &closed).unwrap().1.get_pixel(60, 60)[0], 255, "pintado, fecha");
    // só o que foi marcado sai
    let nothing = Refine { detect: Detect::Nada, ..Default::default() };
    let marked = Strokes { remove: vec![crate::bgremove::Dot { x: 0.1, y: 0.1, r: 0.05 }], ..Default::default() };
    let only = cut(&ring, &nothing, &marked).unwrap().1;
    assert!(only.get_pixel(60, 60)[0] == 255 && only.get_pixel(12, 12)[0] == 0 && only.get_pixel(110, 110)[0] == 255);
    // foto com degradê não é fundo de uma cor só
    let photo = image::RgbImage::from_fn(120, 120, |x, y| image::Rgb([(x * 2) as u8, (y * 2) as u8, 90]));
    assert_eq!(flat_background(&photo), None);
}

#[test]
#[ignore] // baixa o SlimSAM (113 MB)
fn segment_click_removes_object() {
    use crate::bgremove::{Detect, Dot, Refine, Strokes, cut};
    // dois círculos separados sobre um fundo liso: clicar em um tira só ele
    let img = image::RgbImage::from_fn(400, 300, |x, y| {
        let inside = |cx: f32, cy: f32| ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < 60.0;
        if inside(110.0, 150.0) { image::Rgb([220, 40, 40]) } else if inside(290.0, 150.0) { image::Rgb([40, 80, 220]) } else { image::Rgb([235, 235, 225]) }
    });
    let s = Strokes { cut: vec![vec![Dot { x: 110.0 / 400.0, y: 0.5, r: 0.01 }]], ..Default::default() };
    let t = std::time::Instant::now();
    let m = cut(&img, &Refine { detect: Detect::Nada, ..Default::default() }, &s).unwrap().1;
    println!("RESULT segment: {:.1} s", t.elapsed().as_secs_f32());
    assert_eq!(m.get_pixel(110, 150)[0], 0, "o objeto clicado sai");
    assert_eq!(m.get_pixel(290, 150)[0], 255, "o outro fica");
    assert_eq!(m.get_pixel(200, 40)[0], 255, "o fundo fica");
    let t = std::time::Instant::now();
    let again = cut(&img, &Refine { detect: Detect::Nada, ..Default::default() }, &s).unwrap().1;
    println!("RESULT segundo clique: {:.2} s", t.elapsed().as_secs_f32());
    assert_eq!(again, m);
    if let Ok(robo) = std::env::var("CONVER_ROBO") {
        let rgb = image::open(&robo).unwrap().to_rgb8();
        let (w, h) = (rgb.width() as f32, rgb.height() as f32);
        let name = std::path::Path::new(&robo).with_extension("");
        let (clean, base) = cut(&rgb, &Refine::default(), &Strokes::default()).unwrap();
        let save = |m: &image::GrayImage, c: &image::RgbImage, suffix: &str| {
            let out = image::RgbaImage::from_fn(rgb.width(), rgb.height(), |x, y| {
                let p = c.get_pixel(x, y);
                image::Rgba([p[0], p[1], p[2], m.get_pixel(x, y)[0]])
            });
            out.save(format!("{}_{suffix}.png", name.display())).unwrap();
        };
        save(&base, &clean, "cor");
        for (suffix, x, y) in [("clique_seta", 122.0, 272.0), ("clique_antena", 145.0, 20.0), ("clique_chave", 245.0, 195.0)] {
            let s = Strokes { cut: vec![vec![Dot { x: x / w, y: y / h, r: 0.01 }]], ..Default::default() };
            let (c, m) = cut(&rgb, &Refine::default(), &s).unwrap();
            save(&m, &c, suffix);
        }
    }
}

#[test]
#[ignore] // edita uma imagem pelo caminho do app com o FLUX.2 klein (minutos na placa de vídeo)
fn image_edit_instruction_op() {
    let d = crate::tests_v12::tmp("editar_imagem");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let src = d.join("cabana.png");
    let base = std::env::var("CONVER_EDIT_SRC").unwrap();
    std::fs::copy(&base, &src).unwrap();
    let t = std::time::Instant::now();
    let r = crate::tests_v12::ctx_call("image_edit", serde_json::json!({ "files": [src.display().to_string()], "prompt": "add a snowman in front of the cabin", "how": "instruction", "seed": 5 })).unwrap();
    let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    println!("RESULT edit op: {:.0} s {:?} {}", t.elapsed().as_secs_f32(), image::image_dimensions(&out).unwrap(), out.display());
    assert!(out.file_name().unwrap().to_string_lossy().starts_with("cabana_editada"));
}

#[test]
#[ignore] // baixa o Kokoro (92 MB) e gera fala em vários idiomas
fn kokoro_voices() {
    use crate::kokoro::{RATE, VOICES, phonemize, speak};
    let d = crate::tests_v12::tmp("kokoro");
    let frases = [
        ("pt", "Olá, bem-vindos ao Conver! Hoje vamos aprender a dublar vídeos com uma voz natural, direto no computador."),
        ("en", "Hello and welcome. Today we will learn how to dub videos with a natural voice."),
        ("es", "Hola, bienvenidos. Hoy vamos a aprender a doblar vídeos con una voz natural."),
        ("fr", "Bonjour et bienvenue. Aujourd'hui, nous allons apprendre à doubler des vidéos."),
        ("it", "Ciao e benvenuti. Oggi impariamo a doppiare i video con una voce naturale."),
    ];
    for (lang, text) in frases {
        for v in VOICES.iter().filter(|v| v.lang == lang).take(2) {
            println!("RESULT {} fonemas: {}", v.id, phonemize(text, v).unwrap());
            let t = std::time::Instant::now();
            let s = speak(text, v, 1.0, &|_| {}, &|| false).unwrap();
            let secs = s.len() as f32 / RATE as f32;
            println!("RESULT {}: {secs:.1} s de fala em {:.1} s", v.id, t.elapsed().as_secs_f32());
            assert!(secs > 2.0 && s.iter().any(|x| x.abs() > 0.05));
            let pcm: Vec<i16> = s.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
            crate::translate::write_wav(&d.join(format!("{}.wav", v.id)), &pcm, RATE).unwrap();
        }
    }
}

#[test]
#[ignore] // usa o Kokoro (baixa o modelo se faltar)
fn dub_with_kokoro() {
    let d = tmp("dublagem_kokoro");
    let vid = d.join("aula.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0x223344:s=320x240:d=10", "-f", "lavfi", "-i", "sine=f=220:d=10", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", &vid.to_string_lossy()], None);
    std::fs::write(d.join("aula.srt"), "1\n00:00:00,500 --> 00:00:04,000\nOlá, bem-vindos à aula de hoje.\n\n2\n00:00:04,200 --> 00:00:09,000\nVamos falar sobre legendas e dublagem no computador.\n").unwrap();
    let t = std::time::Instant::now();
    let r = ctx_call("subtitle_translate", json!({ "files": [d.join("aula.srt").display().to_string()], "dub": true, "voice": "pf_dora", "original_volume": 0 })).unwrap();
    println!("RESULT {r} em {:.1} s", t.elapsed().as_secs_f32());
    assert_eq!(r["ok"], 1, "{r}");
    let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    let p = crate::video::probe(&out);
    assert!(p.has_video && p.has_audio, "{p:?}");
    let s = crate::tests_v18::lufs(&out);
    println!("RESULT volume {s:.1} LUFS: {}", out.display());
    assert!(s > -40.0);
    // Texto para voz com o Kokoro, em MP3
    let n = ctx_call("text_to_speech", json!({ "text": "Olá, esta é a voz natural do Conver.", "voice": "pm_alex", "out_dir": d.display().to_string() })).unwrap();
    println!("RESULT {n}");
    assert!(std::path::Path::new(n["output"].as_str().unwrap()).is_file());
}

#[test]
#[ignore] // usa a placa de vídeo com o modelo rápido
fn image_edit_paint_and_vary() {
    let d = crate::tests_v12::tmp("editar_modos");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let src = d.join("foto.png");
    image::RgbImage::from_fn(600, 400, |x, y| image::Rgb([(x / 3) as u8, (y / 2) as u8, 120])).save(&src).unwrap();
    let mask = d.join("mascara.png");
    let mut m = image::GrayImage::new(600, 400);
    for y in 100..300 {
        for x in 200..400 {
            m.put_pixel(x, y, image::Luma([255]));
        }
    }
    m.save(&mask).unwrap();
    for (how, extra) in [("vary", json!({ "strength": 0.6 })), ("paint", json!({ "mask": mask.display().to_string() }))] {
        let mut a = json!({ "files": [src.display().to_string()], "prompt": "a red ball", "how": how, "model": "fast", "seed": 3, "out_dir": d.display().to_string() });
        for (k, v) in extra.as_object().unwrap() {
            a[k] = v.clone();
        }
        let r = ctx_call("image_edit", a).unwrap();
        let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
        let dims = image::image_dimensions(&out).unwrap();
        println!("RESULT {how}: {dims:?}");
        assert!(dims.0 == 512 && dims.0 % 16 == 0 && dims.1 % 16 == 0, "{dims:?}");
    }
    // pintar sem máscara é erro claro
    assert!(ctx_call("image_edit", json!({ "files": [src.display().to_string()], "prompt": "x", "how": "paint" })).is_err());
}
