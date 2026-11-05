//! Testes da leva 1.5: remover ruído, organizar páginas do PDF e legendas.

use serde_json::json;

use crate::captions::{self, Cue, Effect, Style};
use crate::tests_v12::{ctx_call, ffmpeg, speech, tmp};
use crate::transcribe::{WModel, Word};

fn mean_volume(f: &std::path::Path) -> f64 {
    let ff = crate::tools::find_tool("ffmpeg").unwrap();
    let out = crate::tools::command(&ff).args(["-hide_banner", "-i"]).arg(f).args(["-af", "volumedetect", "-f", "null", "-"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    text.split("mean_volume:").nth(1).and_then(|s| s.trim().split(' ').next()).and_then(|v| v.parse().ok()).unwrap()
}

#[test]
fn audio_denoise() {
    let d = tmp("ruido");
    let noisy = d.join("ruidoso.wav");
    ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=300:duration=3", "-f", "lavfi", "-i", "anoisesrc=d=3:c=pink:a=0.25",
        "-filter_complex", "[0][1]amix=inputs=2:normalize=0", &noisy.to_string_lossy()], None);
    // vídeo com o mesmo áudio: a imagem tem de ser copiada
    let vid = d.join("video.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=blue:s=320x240:d=3", "-i", &noisy.to_string_lossy(), "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", &vid.to_string_lossy()], None);
    for level in ["light", "medium", "strong"] {
        let r = ctx_call("audio_denoise", json!({ "files": [noisy.display().to_string(), vid.display().to_string()], "strength": level, "out_dir": d.join(level).display().to_string() })).unwrap();
        assert_eq!(r["ok"], 2, "{r}");
        let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
        let (before, after) = (mean_volume(&noisy), mean_volume(&out));
        println!("{level}: {before:.1} dB → {after:.1} dB  ({})", r["results"][0]["info"]);
        // o leve só tira chiado (mexe pouco num ruído grave e forte como este); os de IA tiram bem mais
        let min = if level == "light" { 0.3 } else { 4.0 };
        assert!(after < before - min, "{level}: o ruído não diminuiu o bastante");
        let v = crate::video::probe(std::path::Path::new(r["results"][1]["output"].as_str().unwrap()));
        assert!(v.has_video && v.has_audio && v.size == Some((320, 240)));
    }
}

#[test]
fn pdf_organize_pages() {
    let d = tmp("organizar");
    // 3 páginas de tamanhos diferentes, para reconhecer cada uma no resultado
    let mut imgs = Vec::new();
    for (i, (w, h)) in [(400, 300), (300, 400), (500, 500)].iter().enumerate() {
        let f = d.join(format!("p{i}.png"));
        image::RgbImage::from_pixel(*w, *h, image::Rgb([200, 30 * i as u8, 10])).save(&f).unwrap();
        imgs.push(f.display().to_string());
    }
    let r = ctx_call("images_to_pdf", json!({ "files": imgs, "out_dir": d.display().to_string() })).unwrap();
    let pdf = r["output"].as_str().unwrap().to_string();
    assert_eq!(crate::pdf::page_count(std::path::Path::new(&pdf)), Some(3));
    // nova ordem: 3, 1, 3 (repetida), sem a 2; a 3 girada 90°
    let r = ctx_call("pdf_organize", json!({ "files": [pdf], "order": "3, 1, 3", "rotate": { "3": 90 } })).unwrap();
    println!("{r}");
    let out = r["output"].as_str().unwrap();
    let doc = lopdf::Document::load(out).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 3);
    let info = |n: u32| {
        let dict = doc.get_dictionary(pages[&n]).unwrap();
        let mb: Vec<f32> = dict.get(b"MediaBox").unwrap().as_array().unwrap().iter().map(|o| o.as_float().unwrap_or_else(|_| o.as_i64().unwrap() as f32)).collect();
        let rot = dict.get(b"Rotate").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);
        (mb[2] / mb[3], rot)
    };
    let (a1, r1) = info(1);
    let (a2, r2) = info(2);
    let (a3, r3) = info(3);
    assert!((a1 - 1.0).abs() < 0.01 && r1 == 90, "1ª = página 3 (quadrada) girada: {a1} {r1}");
    assert!((a2 - 4.0 / 3.0).abs() < 0.01 && r2 == 0, "2ª = página 1 (deitada): {a2} {r2}");
    assert!((a3 - 1.0).abs() < 0.01 && r3 == 90, "3ª = a página 3 repetida");
    // o Windows abre o PDF novo
    let imgs = ctx_call("pdf_to_images", json!({ "files": [out], "dpi": 36, "out_dir": d.join("img").display().to_string() })).unwrap();
    assert_eq!(imgs["ok"], 1, "{imgs}");
    assert!(crate::pdf::parse_order("2-", 4).unwrap() == vec![2, 3, 4] && crate::pdf::parse_order("3-1", 4).unwrap() == vec![3, 2, 1]);
    assert!(crate::pdf::parse_order("5", 4).is_err());
}

#[test]
fn captions_ass_build() {
    let w = |t: &str, s: f64, e: f64| Word { start: s, end: e, text: t.into() };
    let words = vec![w("Olá,", 0.0, 0.3), w("isto", 0.35, 0.6), w("é", 0.62, 0.7), w("um", 0.72, 0.8), w("teste.", 0.85, 1.2), w("Depois", 2.5, 2.8)];
    let cues = captions::cues_from_words(&words);
    assert_eq!(cues.len(), 2, "a frase termina no ponto e a pausa começa outra");
    assert_eq!(cues[0].text, "Olá, isto é um teste.");
    assert!((cues[0].end - 1.55).abs() < 0.01, "fica um pouco depois da última palavra: {}", cues[0].end);
    let ass = captions::build_ass(&cues, 1080, 1920, &Style::default());
    let lines: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue:")).collect();
    println!("{}", lines.join("\n"));
    assert_eq!(lines.len(), 6, "uma linha por palavra");
    assert!(lines[0].contains("OLÁ,") && lines[0].contains("ISTO") && lines[0].contains("É") && !lines[0].contains("UM"), "grupo de 3");
    assert!(lines[3].contains("UM") && lines[3].contains("TESTE.") && !lines[3].contains("DEPOIS"));
    assert!(lines[0].contains("0:00:00.00,0:00:00.35"), "fica destacada até a próxima palavra");
    assert!(lines[0].contains("\\an5\\pos(540,1536)"), "posição no centro, a 80% da altura");
    assert!(ass.contains("PlayResX: 1080") && ass.contains("&H0000E1FF&") && ass.contains("Segoe UI Black"));
    // uma palavra por vez
    let one = captions::build_ass(&cues, 1920, 1080, &captions::presets()[2].1);
    assert!(one.lines().filter(|l| l.starts_with("Dialogue:")).all(|l| l.rsplit('}').next().unwrap().split(' ').count() == 1));
    // legenda parada: uma linha por grupo, sem destaque
    let plain = Style { animated: false, words_per_line: 4, ..Style::default() };
    let still = captions::build_ass(&cues, 1080, 1920, &plain);
    let sl: Vec<&str> = still.lines().filter(|l| l.starts_with("Dialogue:")).collect();
    assert_eq!(sl.len(), 3, "{sl:?}");
    assert!(sl.iter().all(|l| !l.contains("\\c&H")), "sem cor de destaque");
    // aparecendo: as palavras seguintes ficam invisíveis
    let rev = captions::build_ass(&cues, 1080, 1920, &Style { effect: Effect::Revelar, ..Style::default() });
    assert!(rev.lines().find(|l| l.starts_with("Dialogue:")).unwrap().matches("\\alpha&HFF&").count() == 2);
    // faixa atrás do texto: BorderStyle 3 com a cor e a opacidade da faixa
    let band = captions::build_ass(&cues, 1080, 1920, &captions::presets()[5].1);
    assert!(band.contains(",3,") && band.contains("&H5A000000&"), "{}", band.lines().find(|l| l.starts_with("Style:")).unwrap());
}

#[test]
fn captions_edit_and_srt() {
    let w = |t: &str, s: f64, e: f64| Word { start: s, end: e, text: t.into() };
    let orig = vec![w("ola", 1.0, 1.3), w("mundo", 1.4, 1.9), w("bonito", 2.0, 2.6)];
    // corrigiu a grafia: o tempo de cada palavra é o do Whisper
    let fixed = Cue { start: 1.0, end: 2.9, text: "Olá mundo bonito".into(), words: orig.clone() };
    let cw = captions::cue_words(&fixed);
    assert_eq!(cw.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(), ["Olá", "mundo", "bonito"]);
    assert!((cw[1].start - 1.4).abs() < 1e-6 && (cw[2].start - 2.0).abs() < 1e-6);
    // mudou o número de palavras: o tempo é repartido dentro da frase
    let changed = Cue { text: "Olá mundo tão bonito".into(), ..fixed.clone() };
    let cw = captions::cue_words(&changed);
    assert_eq!(cw.len(), 4);
    assert!((cw[0].start - 1.0).abs() < 1e-6 && (cw[3].end - 2.9).abs() < 1e-6 && cw.windows(2).all(|p| p[1].start >= p[0].start));
    // a frase foi adiantada: as palavras acompanham
    let moved = Cue { start: 0.5, end: 2.4, ..fixed.clone() };
    assert!((captions::cue_words(&moved)[1].start - 0.9).abs() < 1e-6);
    // .srt de ida e volta, com tags e um .vtt
    let srt = captions::to_srt(&[fixed.clone(), Cue { start: 3.25, end: 4.0, text: "Segunda frase".into(), words: vec![] }]);
    assert!(srt.contains("00:00:01,000 --> 00:00:02,900\nOlá mundo bonito"), "{srt}");
    let back = captions::parse_subtitle(&srt);
    assert_eq!(back.len(), 2);
    assert!((back[1].start - 3.25).abs() < 1e-6 && back[1].text == "Segunda frase");
    let vtt = captions::parse_subtitle("WEBVTT\n\n00:01.500 --> 00:03.000 align:middle\n<i>Linha um</i>\nlinha dois\n");
    assert_eq!(vtt.len(), 1);
    assert!((vtt[0].start - 1.5).abs() < 1e-6 && vtt[0].text == "Linha um linha dois", "{vtt:?}");
}

#[test]
fn captions_style_args() {
    let m = json!({ "preset": "classic", "text_color": "#112233", "highlight_color": "green", "x": 30, "y": 20, "font": "Arial", "font_size": 5, "background_color": "#000000", "background_opacity": 40 });
    let st = crate::ops::caption_style(&crate::ops::Args(m.as_object().unwrap())).unwrap();
    assert!(!st.animated && st.text == [0x11, 0x22, 0x33] && st.highlight == captions::GREEN && st.font == "Arial");
    assert!(st.x == 30.0 && st.y == 20.0 && st.size == 5.0 && st.background && st.back_opacity == 40);
    // nomes antigos: style/color/position
    let m = json!({ "style": "box", "color": "red", "position": "top" });
    let st = crate::ops::caption_style(&crate::ops::Args(m.as_object().unwrap())).unwrap();
    assert!(st.animated && st.effect == Effect::Caixa && st.highlight == captions::RED && st.y == 14.0);
    assert!(crate::ops::caption_style(&crate::ops::Args(json!({ "text_color": "roxo" }).as_object().unwrap())).is_err());
    assert!(crate::ops::find("video_animated_captions").is_some_and(|t| t.name == "video_captions"));
}

#[test]
fn captions_preview_and_srt_burn() {
    let d = tmp("legendas_srt");
    // prévia de verdade (fundo liso): a faixa escura aparece embaixo, e a legenda muda de lugar com y
    let band = captions::presets()[5].1.clone();
    let png = captions::render_preview(None, 0.0, (1080, 1920), "Teste de prévia", None, &band, 640).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    assert_eq!(img.height(), 640);
    let dark = |img: &image::RgbImage, y: u32| (0..img.width()).filter(|&x| img.get_pixel(x, y)[0] < 20).count();
    let y88 = (640.0 * 0.88) as u32;
    assert!(dark(&img, y88) > 40 && dark(&img, 100) == 0, "faixa no lugar certo: {} / {}", dark(&img, y88), dark(&img, 100));
    let top = Style { y: 15.0, ..band };
    let img = image::load_from_memory(&captions::render_preview(None, 0.0, (1080, 1920), "Teste de prévia", None, &top, 640).unwrap()).unwrap().to_rgb8();
    assert!(dark(&img, (640.0 * 0.15) as u32) > 40 && dark(&img, y88) == 0, "a faixa subiu com y");
    // gravar com uma legenda .srt corrigida (sem transcrever): vídeo + .srt de mesmo nome
    let vid = d.join("clipe.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0x223344:s=360x640:r=30:d=3", "-f", "lavfi", "-i", "sine=d=3", "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", &vid.to_string_lossy()], None);
    std::fs::write(d.join("clipe.srt"), "1\n00:00:00,200 --> 00:00:01,400\nPalavra certa aqui\n\n2\n00:00:01,500 --> 00:00:02,800\nOutra frase\n").unwrap();
    let r = ctx_call("video_captions", json!({ "files": [vid.display().to_string(), d.join("clipe.srt").display().to_string()], "preset": "reveal", "highlight_color": "#FF00FF", "keep_ass": true })).unwrap();
    println!("{r}");
    assert_eq!(r["ok"], 1, "{r}");
    let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    assert!(crate::video::probe(&out).size == Some((360, 640)));
    let ass = std::fs::read_to_string(d.join("clipe_legenda.ass")).unwrap();
    assert!(ass.contains("PALAVRA") && ass.contains("OUTRA") && ass.contains("&H00FF00FF&"), "usou o texto do .srt e a cor pedida");
    // o quadro de 1 s tem a cor do destaque (magenta) desenhada
    let frame = d.join("quadro.png");
    ffmpeg(&["-ss", "1.0", "-i", &out.to_string_lossy(), "-frames:v", "1", &frame.to_string_lossy()], None);
    let f = image::open(&frame).unwrap().to_rgb8();
    let magenta = f.pixels().filter(|p| p[0] > 200 && p[1] < 80 && p[2] > 200).count();
    assert!(magenta > 50, "palavra destacada no vídeo: {magenta} pixels");
}

#[test]
#[ignore] // usa o modelo do Whisper (82 MB, baixado na primeira vez)
fn captions_burn_video() {
    let d = tmp("legendas_animadas");
    let wav = speech(&d, "One two three four five. Hello world, this is a caption test.");
    let vid = d.join("reels.mp4");
    ffmpeg(&["-f", "lavfi", "-i", "color=c=0x223344:s=540x960:r=30", "-i", &wav.to_string_lossy(), "-shortest", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", &vid.to_string_lossy()], None);
    let t = std::time::Instant::now();
    let words = crate::transcribe::word_timestamps(&vid, WModel::Rapido, "en", &|_| {}, &|| false).unwrap();
    println!("{} palavras em {:.1}s: {:?}", words.len(), t.elapsed().as_secs_f32(), words.iter().map(|w| format!("{}@{:.2}", w.text, w.start)).collect::<Vec<_>>());
    assert!(words.len() >= 8 && words.windows(2).all(|p| p[1].start >= p[0].start));
    assert!(words.iter().any(|w| w.text.to_lowercase().contains("hello")));
    let r = ctx_call("video_animated_captions", json!({ "files": [vid.display().to_string()], "language": "en", "style": "box", "keep_ass": true })).unwrap();
    println!("{r}");
    let out = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    let p = crate::video::probe(&out);
    assert!(p.has_video && p.has_audio && p.size == Some((540, 960)), "{p:?}");
    assert!(d.join("reels_legenda.ass").is_file());
}

#[test]
#[ignore] // usa o modelo LaMa (208 MB, baixado na primeira vez)
fn erase_object() {
    let d = tmp("apagar");
    // fundo com gradiente e um quadrado vermelho no meio: depois de apagar, não pode sobrar vermelho
    let mut img = image::RgbImage::from_fn(800, 600, |x, y| image::Rgb([(x / 4) as u8, (y / 3) as u8, 150]));
    for y in 250..350 {
        for x in 350..450 {
            img.put_pixel(x, y, image::Rgb([255, 0, 0]));
        }
    }
    let src = d.join("foto.png");
    img.save(&src).unwrap();
    let t = std::time::Instant::now();
    let r = ctx_call("image_erase", json!({ "files": [src.display().to_string()], "regions": [[340, 240, 120, 120]] })).unwrap();
    println!("{r} em {:.1}s", t.elapsed().as_secs_f32());
    let out = image::open(r["results"][0]["output"].as_str().unwrap()).unwrap().to_rgb8();
    assert_eq!(out.dimensions(), (800, 600));
    let red = out.enumerate_pixels().filter(|(x, y, p)| (350..450).contains(x) && (250..350).contains(y) && p[0] > 200 && p[1] < 60).count();
    println!("pixels ainda vermelhos: {red} de 10000");
    assert!(red < 300, "o quadrado não foi apagado");
    // fora da região, a imagem é a mesma
    assert_eq!(out.get_pixel(10, 10), img.get_pixel(10, 10));
    assert_eq!(out.get_pixel(790, 590), img.get_pixel(790, 590));
}

#[test]
fn pdf_thumbnails() {
    let d = tmp("miniaturas");
    let f = d.join("a.png");
    image::RgbImage::from_pixel(300, 400, image::Rgb([10, 200, 30])).save(&f).unwrap();
    let r = ctx_call("images_to_pdf", json!({ "files": [f.display().to_string(), f.display().to_string()], "out_dir": d.display().to_string() })).unwrap();
    let pdf = std::path::PathBuf::from(r["output"].as_str().unwrap());
    // em outra thread, como na tela
    let got = std::thread::spawn(move || {
        let mut sizes = Vec::new();
        let total = crate::pdf_extra::thumbnails(&pdf, 28.0, |n, png| {
            let img = image::load_from_memory(&png).unwrap();
            sizes.push((n, img.width(), img.height()));
            true
        });
        (total, sizes)
    })
    .join()
    .unwrap();
    println!("{got:?}");
    assert_eq!(got.0, Ok(2));
    assert_eq!(got.1.len(), 2);
}

#[test]
#[ignore] // usa o modelo do Whisper (82 MB) e o whisper-server
fn dictation_live() {
    let d = tmp("ditado");
    let wav = speech(&d, "Hello everyone. This is a live dictation test. The text should appear while I am talking.");
    // a fala tocada em tempo real (-re), como se fosse o microfone, seguida de silêncio
    let input: Vec<String> = ["-re", "-i", &wav.to_string_lossy(), "-af", "apad=pad_dur=3"].map(String::from).to_vec();
    let ctx = eframe::egui::Context::default();
    let t = std::time::Instant::now();
    let mut dict = crate::dictation::Dictation::start_input(input, WModel::Rapido, "en", &ctx);
    let mut partials = Vec::new();
    let mut first_text = None;
    while t.elapsed() < std::time::Duration::from_secs(40) {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let s = dict.state.lock().unwrap();
        if let Some(e) = &s.error {
            panic!("{e}");
        }
        if !s.partial.is_empty() && partials.last() != Some(&s.partial) {
            partials.push(s.partial.clone());
        }
        if first_text.is_none() && (!s.partial.is_empty() || !s.text.is_empty()) {
            first_text = Some(t.elapsed());
        }
        if s.text.to_lowercase().contains("should appear") {
            break;
        }
    }
    dict.stop();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let s = dict.state.lock().unwrap();
    println!("primeiro texto em {first_text:?}; provisórios: {partials:?}
final: {}", s.text);
    let low = s.text.to_lowercase();
    // a voz sintetizada do Windows é robótica e o modelo rápido erra algumas palavras: confere as principais
    assert!(low.contains("dictation") && low.contains("text should appear"), "{}", s.text);
    assert!(!partials.is_empty(), "o texto provisório devia aparecer enquanto fala");
}

#[test]
fn tts_text_sources() {
    let srt = "1
00:00:01,000 --> 00:00:02,000
<i>Olá</i> mundo

2
00:00:02,500 --> 00:00:03,000
{\\an8}tudo bem?
";
    assert_eq!(crate::tts::text_from_subtitle(srt), "Olá mundo tudo bem?");
    assert!(crate::tts::voice("pt_BR-faber-medium").is_some() && crate::tts::default_voice("en").id.starts_with("en_"));
    assert!(crate::tts::VOICES.iter().all(|v| v.onnx.sha256.len() == 64 && v.json.sha256.len() == 64));
}

#[test]
#[ignore] // usa o Piper e a voz pt-BR (63 MB, baixada na primeira vez)
fn tts_speak() {
    let d = tmp("tts");
    let t = std::time::Instant::now();
    let r = ctx_call("text_to_speech", json!({ "text": "Olá! Este é o Conver lendo um texto em voz alta, direto no seu computador.", "voice": "pt_BR-faber-medium", "out_dir": d.display().to_string() })).unwrap();
    println!("{r} em {:.1}s", t.elapsed().as_secs_f32());
    let out = std::path::PathBuf::from(r["output"].as_str().unwrap());
    assert!(out.extension().unwrap() == "mp3");
    let dur = crate::video::duration(&out).unwrap_or(0.0);
    assert!((3.0..12.0).contains(&dur), "duração {dur}");
    // mais devagar = mais longo; de um arquivo de legenda
    let srt = d.join("fala.srt");
    std::fs::write(&srt, "1
00:00:00,000 --> 00:00:02,000
Um teste de legenda narrada.
").unwrap();
    let r = ctx_call("text_to_speech", json!({ "files": [srt.display().to_string()], "speed": 0.7, "format": "wav" })).unwrap();
    let wav = std::path::PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    assert!(wav.file_name().unwrap().to_string_lossy().ends_with("_narracao.wav") && crate::video::duration(&wav).unwrap_or(0.0) > 1.5);
}

#[test]
#[ignore] // usa o gerador (sd.exe na placa de vídeo) e o modelo de 2,1 GB
fn image_generate() {
    let d = tmp("gerar_imagem");
    let t = std::time::Instant::now();
    let r = ctx_call("image_generate", json!({ "prompt": "a red apple on a wooden table, photo", "shape": "portrait", "count": 2, "seed": 5, "out_dir": d.display().to_string() })).unwrap();
    println!("{r} em {:.1}s", t.elapsed().as_secs_f32());
    let files: Vec<String> = r["results"].as_array().unwrap().iter().map(|x| x["output"].as_str().unwrap().to_string()).collect();
    assert_eq!(files.len(), 2);
    for f in files {
        assert_eq!(image::image_dimensions(&f).unwrap(), (512, 768));
    }
}
