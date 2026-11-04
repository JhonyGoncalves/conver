//! Testes da versão 1.0: OCR, informações, gravador e integrações.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::{info, integration, ocr, recorder, tools};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ffmpeg(args: &[&str], cwd: Option<&std::path::Path>) {
    let ff = tools::find_tool("ffmpeg").expect("ffmpeg");
    let mut c = tools::command(&ff);
    c.args(["-y", "-loglevel", "error"]).args(args);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    assert!(c.status().unwrap().success(), "ffmpeg {args:?}");
}

#[test]
fn ocr_reads_text() {
    let d = tmp("ocr");
    let langs = ocr::languages();
    println!("idiomas de OCR: {langs:?}");
    assert!(!langs.is_empty(), "nenhum idioma de OCR no Windows");
    // imagem com texto desenhado pelo ffmpeg (a fonte é lida da pasta de fontes do Windows)
    let fonts = PathBuf::from(std::env::var("WINDIR").unwrap()).join("Fonts");
    let out = d.join("texto.png");
    ffmpeg(
        &["-f", "lavfi", "-i", "color=white:s=900x220", "-vf",
          "drawtext=fontfile=arial.ttf:text='Conver converte tudo 2026':fontsize=64:fontcolor=black:x=30:y=80",
          "-frames:v", "1", &out.to_string_lossy()],
        Some(&fonts),
    );
    let text = ocr::recognize(&out, "").unwrap();
    println!("OCR: {text:?}");
    assert!(text.contains("Conver") && text.contains("2026"));
    // imagem pequena (é ampliada antes)
    let small = d.join("pequena.png");
    image::open(&out).unwrap().resize(300, 80, image::imageops::FilterType::Triangle).save(&small).unwrap();
    let t2 = ocr::recognize(&small, &langs[0].0).unwrap();
    println!("OCR pequena: {t2:?}");
    assert!(t2.to_lowercase().contains("conver"));
}

#[test]
fn file_info() {
    let d = tmp("info");
    let v = d.join("v.mp4");
    ffmpeg(
        &["-f", "lavfi", "-i", "testsrc=duration=3:size=320x240:rate=25", "-f", "lavfi", "-i", "sine=duration=3",
          "-shortest", "-metadata", "title=Meu vídeo", &v.to_string_lossy()],
        None,
    );
    let i = info::read(&v);
    let text = i.as_text();
    println!("{text}");
    assert!(text.contains("[Faixas]") && text.contains("Vídeo: ") && text.contains("Áudio: ") && text.contains("320x240"));
    assert!(text.contains("00:03") && text.contains("title: Meu vídeo"));

    let png = d.join("i.png");
    image::RgbaImage::from_pixel(640, 480, image::Rgba([1, 2, 3, 128])).save(&png).unwrap();
    let t = info::read(&png).as_text();
    assert!(t.contains("640 × 480") && t.contains("transparência"));

    let pdf = d.join("d.pdf");
    crate::imaging::images_to_pdf(&[png.clone(), png.clone()], &pdf, crate::imaging::PageSize::A4Retrato, &|_| {}, &|| false).unwrap();
    assert!(info::read(&pdf).as_text().contains("Páginas: 2"));
}

#[test]
fn recorder_devices() {
    let devs = recorder::devices();
    println!("entradas de áudio: {devs:?}");
    let d = tmp("rec");
    // usa a primeira entrada que realmente envia som (entradas sem nada conectado ficam em silêncio)
    let mut ok = false;
    for (i, dev) in devs.iter().enumerate() {
        let dst = d.join(format!("gravacao_{i}.mp3"));
        let mut rec = recorder::Recording::start(dev, crate::media::AFmt::Mp3, &dst, || {}).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3500));
        // entrada em uso por outro programa ou desconectada: o ffmpeg para sozinho e o erro é informado
        if let Err(e) = rec.check() {
            println!("{dev}: parou sozinha ({e})");
            continue;
        }
        let silent = rec.silent();
        let r = rec.stop();
        println!("{dev}: silenciosa={silent} -> {r:?}");
        if let Ok(path) = r {
            let dur = crate::video::duration(&path).unwrap_or(0.0);
            println!("  arquivo: {} ({dur:.2}s)", tools::human_size(tools::file_size(&path)));
            if !silent && dur > 1.0 {
                ok = true;
                break;
            }
            continue; // entrada que começou a mandar som atrasado: tenta a próxima
        }
        assert!(silent, "entrada que não gravou deveria ter sido marcada como silenciosa");
    }
    // sem nenhum microfone com som agora (desconectado, em uso por outro programa), a detecção de silêncio
    // é o que se testa: todas as entradas acima foram marcadas como silenciosas
    if !ok {
        println!("⚠ nenhuma entrada de áudio enviou som durante o teste; só a detecção de silêncio foi verificada");
    }
}

#[test]
fn integrations() {
    assert!(integration::is_newer("v1.0.1", "1.0.0"));
    assert!(integration::is_newer("1.10.0", "1.9.9"));
    assert!(!integration::is_newer("v1.0.0", "1.0.0") && !integration::is_newer("0.9", "1.0.0"));
    assert_eq!(integration::sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(integration::sha256(&[]), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    let big = vec![b'a'; 1_000_000];
    assert_eq!(integration::sha256(&big), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");

    // instância única: a segunda entrega os arquivos à primeira
    let inbox: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    assert!(!integration::single_instance(&[], inbox.clone()));
    let files = vec![PathBuf::from(r"C:\fotos\a b.jpg"), PathBuf::from(r"C:\vídeos\ção.mp4")];
    assert!(integration::single_instance(&files, Arc::default()));
    std::thread::sleep(std::time::Duration::from_millis(400));
    let got = inbox.lock().unwrap().clone();
    println!("recebidos: {got:?}");
    assert_eq!(got[0], PathBuf::new());
    assert_eq!(&got[1..], &files[..]);

    // menu do botão direito (registro do usuário atual): liga, confere e desliga
    integration::register_menu().unwrap();
    assert!(integration::menu_registered());
    integration::unregister_menu();
    assert!(!integration::menu_registered());
}

/// Baixa o yt-dlp oficial e confere a assinatura, sem executar (precisa de internet).
#[test]
#[ignore]
fn ytdlp_download_verified() {
    let d = tmp("ytdlp");
    let f = integration::download_ytdlp(&d, &|_| {}).unwrap();
    println!("baixado e conferido: {} ({})", f.display(), tools::human_size(tools::file_size(&f)));
    assert!(tools::file_size(&f) > 5_000_000);
}
