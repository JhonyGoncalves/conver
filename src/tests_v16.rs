//! Testes da leva 1.5: remover ruído, organizar páginas do PDF e legendas animadas.

use serde_json::json;

use crate::captions::{self, Look, Place, Style, Tint};
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
    let ass = captions::build_ass(&words, 1080, 1920, Style { look: Look::Destaque, place: Place::Baixo, tint: Tint::Amarelo, words_per_line: 3, uppercase: true });
    let lines: Vec<&str> = ass.lines().filter(|l| l.starts_with("Dialogue:")).collect();
    println!("{}", lines.join("\n"));
    assert_eq!(lines.len(), 6, "uma linha por palavra");
    assert!(lines[0].contains("OLÁ,") && lines[0].contains("ISTO") && lines[0].contains("É") && !lines[0].contains("UM"), "grupo de 3");
    assert!(lines[3].contains("UM") && lines[3].contains("TESTE.") && !lines[3].contains("DEPOIS"), "a frase termina no ponto");
    assert!(lines[5].contains("DEPOIS"), "a pausa longa começa outro grupo");
    assert!(lines[0].contains("0:00:00.00,0:00:00.35"), "fica destacada até a próxima palavra");
    assert!(ass.contains("PlayResX: 1080") && ass.contains("Alignment") && ass.contains("&H0000E1FF&"));
    let one = captions::build_ass(&words, 1920, 1080, Style { look: Look::UmaPalavra, ..Default::default() });
    assert!(one.lines().filter(|l| l.starts_with("Dialogue:")).all(|l| l.split(",,").last().unwrap().split(' ').count() == 1));
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
    assert!(d.join("reels_legendas_animadas.ass").is_file());
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
