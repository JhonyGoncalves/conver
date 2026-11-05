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
