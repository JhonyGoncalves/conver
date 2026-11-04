//! Testes das ferramentas novas (rodar com `cargo test --release -- --nocapture --test-threads 1`).

use std::path::{Path, PathBuf};

use crate::imaging::{self, Aspect, ImageOp, PageSize};
use crate::media::{self, ACodec, AFmt};
use crate::rename::{self, Case, Rules};
use crate::video::{self, VQuality};
use crate::{qr, tools};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn image_new_tools() {
    let d = tmp("img_new");
    let img = image::RgbImage::from_fn(800, 600, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90]));
    let jpg = d.join("foto.jpg");
    img.save(&jpg).unwrap();

    // girar 90° troca largura e altura; recorte 1:1 fica quadrado
    let out = imaging::process(&jpg, &ImageOp::Transform { rotate: 90, flip_h: true, flip_v: false, aspect: Aspect::Livre }).unwrap();
    assert_eq!(out.dims, (600, 800));
    let out = imaging::process(&jpg, &ImageOp::Transform { rotate: 0, flip_h: false, flip_v: false, aspect: Aspect::Quadrado }).unwrap();
    assert_eq!(out.dims, (600, 600));
    let out = imaging::process(&jpg, &ImageOp::Transform { rotate: 0, flip_h: false, flip_v: true, aspect: Aspect::R16x9 }).unwrap();
    assert_eq!(out.dims, (800, 450));
    println!("transform: {}", out.info);

    // JPEG com EXIF (orientação normal + texto do fabricante) e comentário
    let raw = std::fs::read(&jpg).unwrap();
    let mut tiff = b"II*\0\x08\0\0\0\x02\0".to_vec();
    tiff.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0]); // Orientation = 1
    tiff.extend_from_slice(&[0x0F, 0x01, 2, 0, 6, 0, 0, 0, 0x26, 0, 0, 0]); // Make -> offset 38
    tiff.extend_from_slice(&[0, 0, 0, 0]);
    tiff.extend_from_slice(b"Canon\0");
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    let mut with_exif = raw[..2].to_vec();
    with_exif.extend_from_slice(&[0xFF, 0xE1]);
    with_exif.extend_from_slice(&((app1.len() + 2) as u16).to_be_bytes());
    with_exif.extend_from_slice(&app1);
    with_exif.extend_from_slice(&[0xFF, 0xFE, 0, 9]);
    with_exif.extend_from_slice(b"secreto");
    with_exif.extend_from_slice(&raw[2..]);
    let ejpg = d.join("exif.jpg");
    std::fs::write(&ejpg, &with_exif).unwrap();
    let (info, dst) = imaging::process_and_save(&ejpg, &ImageOp::StripMeta, None).unwrap();
    let clean = std::fs::read(&dst).unwrap();
    println!("strip jpg: {info}");
    assert!(!contains(&clean, b"Exif") && !contains(&clean, b"Canon") && !contains(&clean, b"secreto"));
    // sem recodificar: o resto do arquivo é idêntico ao original
    assert_eq!(&clean[2..], &raw[2..]);

    // PNG com bloco de texto
    let png = d.join("t.png");
    img.save(&png).unwrap();
    let raw = std::fs::read(&png).unwrap();
    let mut chunk_data = b"tEXtAuthor\0Fulano".to_vec();
    let crc = crc32(&chunk_data);
    let mut chunk = ((chunk_data.len() - 4) as u32).to_be_bytes().to_vec();
    chunk.append(&mut chunk_data);
    chunk.extend_from_slice(&crc.to_be_bytes());
    let mut with_text = raw[..33].to_vec(); // assinatura + IHDR
    with_text.extend_from_slice(&chunk);
    with_text.extend_from_slice(&raw[33..]);
    let tpng = d.join("texto.png");
    std::fs::write(&tpng, &with_text).unwrap();
    image::open(&tpng).unwrap();
    let (info, dst) = imaging::process_and_save(&tpng, &ImageOp::StripMeta, None).unwrap();
    println!("strip png: {info}");
    let clean = std::fs::read(&dst).unwrap();
    assert!(!contains(&clean, b"Fulano"));
    image::open(&dst).unwrap();
    assert_eq!(clean, raw);

    // sem metadados
    let (info, _) = imaging::process_and_save(&jpg, &ImageOp::StripMeta, None).unwrap();
    assert_eq!(info, "nenhum metadado encontrado");

    // PDF com 3 páginas: cada offset do xref aponta para "N 0 obj"
    let pdf = d.join("doc.pdf");
    let files = vec![jpg.clone(), png.clone(), ejpg.clone()];
    for size in PageSize::ALL {
        let n = imaging::images_to_pdf(&files, &pdf, size, &|_| {}, &|| false).unwrap();
        assert_eq!(n, 3);
        let b = std::fs::read(&pdf).unwrap();
        assert!(b.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&b);
        assert!(text.contains("/Count 3"));
        let sx = text.rfind("startxref\n").unwrap() + 10;
        let xref: usize = text[sx..].lines().next().unwrap().parse().unwrap();
        assert!(b[xref..].starts_with(b"xref"));
        let table = String::from_utf8_lossy(&b[xref..]).to_string();
        for (i, line) in table.lines().skip(3).take(11).enumerate() {
            let off: usize = line[..10].parse().unwrap();
            assert!(b[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "offset do objeto {}", i + 1);
        }
    }
    println!("pdf: {} bytes", tools::file_size(&pdf));
}

fn ffmpeg_make(args: &[&str], dst: &Path) {
    let ff = tools::find_tool("ffmpeg").expect("ffmpeg");
    let ok = tools::command(&ff).args(["-y", "-loglevel", "error"]).args(args).arg(dst).status().unwrap().success();
    assert!(ok, "falha ao gerar {}", dst.display());
}

fn dur(p: &Path) -> f64 {
    video::duration(p).unwrap_or(-1.0)
}

#[test]
fn media_new_tools() {
    let d = tmp("media_new");
    let v = d.join("v.mp4");
    ffmpeg_make(
        &["-f", "lavfi", "-i", "testsrc2=duration=10:size=640x360:rate=30", "-f", "lavfi", "-i", "sine=duration=10",
          "-shortest", "-c:v", "libx264", "-g", "30", "-c:a", "aac"],
        &v,
    );
    let mute = d.join("mudo_vertical.mp4");
    ffmpeg_make(&["-f", "lavfi", "-i", "testsrc=duration=4:size=360x640:rate=25", "-c:v", "libx264"], &mute);
    let wav = d.join("a.wav");
    ffmpeg_make(&["-f", "lavfi", "-i", "sine=frequency=440:duration=6", "-af", "volume=0.1"], &wav);
    let (np, no) = (|_f: f32| {}, || false);

    // cortar
    let (info, t1) = media::trim(&v, None, 2.0, Some(6.0), true, &np, &no).unwrap();
    println!("trim preciso: {info} = {:.2}s", dur(&t1));
    assert!((dur(&t1) - 4.0).abs() < 0.15);
    let (info, t2) = media::trim(&v, None, 2.0, Some(6.0), false, &np, &no).unwrap();
    println!("trim rápido: {info} = {:.2}s", dur(&t2));
    assert!((dur(&t2) - 4.0).abs() < 1.2);
    let (_, t3) = media::trim(&v, None, 7.0, None, true, &np, &no).unwrap();
    assert!((dur(&t3) - 3.0).abs() < 0.15);
    assert!(media::trim(&v, None, 5.0, Some(3.0), true, &np, &no).is_err());
    assert!(media::trim(&v, None, 50.0, None, true, &np, &no).is_err());
    assert_eq!(video::parse_timecode("1:30"), Some(90.0));
    assert_eq!(video::parse_timecode("01:02:03,5"), Some(3723.5));
    assert_eq!(video::parse_timecode("abc"), None);

    // juntar: com recodificação (resoluções diferentes, um sem áudio) e rápido (iguais)
    let (info, m1) = media::merge(&[v.clone(), mute.clone()], None, true, &np, &no).unwrap();
    let p1 = video::probe(&m1);
    println!("merge recodificado: {info} = {:.2}s {:?}", dur(&m1), p1.size);
    assert!((dur(&m1) - 14.0).abs() < 0.3 && p1.has_audio && p1.size == Some((640, 360)));
    let (info, m2) = media::merge(&[v.clone(), v.clone()], None, false, &np, &no).unwrap();
    println!("merge rápido: {info} = {:.2}s", dur(&m2));
    assert!((dur(&m2) - 20.0).abs() < 0.3);
    assert!(media::merge(&[v.clone()], None, true, &np, &no).is_err());

    // velocidade
    let (info, s2) = media::speed(&v, None, 2.0, true, &np, &no).unwrap();
    println!("velocidade: {info} = {:.2}s", dur(&s2));
    assert!((dur(&s2) - 5.0).abs() < 0.2 && video::probe(&s2).has_audio);
    let (_, s4) = media::speed(&v, None, 0.25, true, &np, &no).unwrap();
    assert!((dur(&s4) - 40.0).abs() < 0.5);
    let (_, s5) = media::speed(&v, None, 1.5, false, &np, &no).unwrap();
    assert!(!video::probe(&s5).has_audio);

    // remover áudio / frames
    let (info, ma) = media::remove_audio(&v, None, &np, &no).unwrap();
    println!("sem áudio: {info}");
    assert!(!video::probe(&ma).has_audio && video::probe(&ma).has_video);
    let (info, first) = media::extract_frames(&v, None, 2.0, false, &np, &no).unwrap();
    println!("frames: {info}");
    let n = std::fs::read_dir(first.parent().unwrap()).unwrap().count();
    assert!((5..=6).contains(&n), "{n} frames");
    image::open(&first).unwrap();
    let (_, firstp) = media::extract_frames(&v, None, 5.0, true, &np, &no).unwrap();
    assert!(firstp.extension().unwrap() == "png");

    // áudio: converter todos os formatos (de áudio e de vídeo)
    for f in AFmt::ALL {
        let (info, o) = media::audio_convert(&wav, None, f, VQuality::Media, &np, &no).unwrap();
        assert!((dur(&o) - 6.0).abs() < 0.2, "{}", tools::file_name(&o));
        println!("{}: {info}", tools::file_name(&o));
    }
    let (_, from_video) = media::audio_convert(&v, None, AFmt::Mp3, VQuality::Alta, &np, &no).unwrap();
    assert!(!video::probe(&from_video).has_video);
    assert!(media::audio_convert(&mute, None, AFmt::Mp3, VQuality::Alta, &np, &no).is_err());

    // cortar áudio
    let (_, at) = media::trim(&wav, None, 1.0, Some(3.5), true, &np, &no).unwrap();
    assert!((dur(&at) - 2.5).abs() < 0.1 && tools::ext_lower(&at) == "wav");

    // normalizar: o tom baixo (volume 0.1) deve ficar mais alto
    let (info, nw) = media::normalize(&wav, None, -16, &np, &no).unwrap();
    println!("normalizar: {info}");
    let level = |p: &Path| -> f64 {
        let ff = tools::find_tool("ffmpeg").unwrap();
        let out = tools::command(&ff).args(["-hide_banner", "-i"]).arg(p).args(["-af", "volumedetect", "-f", "null", "NUL"]).output().unwrap();
        let t = String::from_utf8_lossy(&out.stderr).to_string();
        let i = t.find("mean_volume: ").unwrap() + 13;
        t[i..].split_whitespace().next().unwrap().parse().unwrap()
    };
    println!("volume médio: {:.1} dB → {:.1} dB", level(&wav), level(&nw));
    assert!(level(&nw) > level(&wav) + 6.0);
    let (_, nv) = media::normalize(&v, None, -14, &np, &no).unwrap();
    assert!(video::probe(&nv).has_video && video::probe(&nv).has_audio);

    // comprimir áudio
    for (c, ext) in [(ACodec::Mp3, "mp3"), (ACodec::Aac, "m4a"), (ACodec::Opus, "opus")] {
        let (info, o) = media::audio_compress(&wav, None, c, 64, true, &np, &no).unwrap();
        println!("comprimir {ext}: {info}");
        assert_eq!(tools::ext_lower(&o), ext);
        assert!(tools::file_size(&o) < tools::file_size(&wav) / 5);
    }
}

#[test]
fn rename_and_qr() {
    let d = tmp("ren");
    let files: Vec<PathBuf> = ["b.txt", "a.TXT", "c.jpg"].iter().map(|n| d.join(n)).collect();
    for f in &files {
        std::fs::write(f, tools::file_name(f)).unwrap();
    }
    let rules = Rules { base: "Foto Praia".into(), numbering: true, case: Case::Minusculas, ..Default::default() };
    let names: Vec<String> = files.iter().enumerate().map(|(i, f)| rename::new_name(f, i, &rules)).collect();
    assert_eq!(names, ["foto praia_001.txt", "foto praia_002.TXT", "foto praia_003.jpg"]);
    assert!(rename::problems(&files, &names).iter().all(Option::is_none));
    // nomes repetidos são detectados
    let same = vec!["x.txt".to_string(), "x.txt".to_string(), "y.jpg".to_string()];
    assert_eq!(rename::problems(&files, &same)[1], Some("nome repetido"));

    let undo = rename::apply(&files, &names).unwrap();
    assert!(d.join("foto praia_002.TXT").is_file() && !d.join("a.TXT").exists());
    assert_eq!(std::fs::read_to_string(d.join("foto praia_003.jpg")).unwrap(), "c.jpg");
    // desfazer
    let back: Vec<PathBuf> = undo.iter().map(|(n, _)| n.clone()).collect();
    let olds: Vec<String> = undo.iter().map(|(_, o)| tools::file_name(o)).collect();
    rename::apply(&back, &olds).unwrap();
    assert!(files.iter().all(|f| f.is_file()));
    // trocar nomes entre os próprios arquivos (a ↔ b) funciona
    let swap = vec!["a.TXT".to_string(), "b.txt".to_string(), "c.jpg".to_string()];
    assert!(rename::problems(&files, &swap).iter().all(Option::is_none));
    rename::apply(&files, &swap).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("a.TXT")).unwrap(), "b.txt");

    let img = qr::render("https://example.com/conver", 600, [0xF1, 0x30, 0x30], qr::Correction::Media).unwrap();
    println!("qr: {}x{}", img.width(), img.height());
    assert!(img.width() >= 500 && img.width() <= 600 && img.width() == img.height());
    assert!(qr::render(&"x".repeat(5000), 600, [0, 0, 0], qr::Correction::Alta).is_err());
    assert!(!tools::now_string().is_empty());
}

/// Download de um site que não é o YouTube (precisa de internet): `cargo test --release -- --ignored other_site`
#[test]
#[ignore]
fn other_site_download() {
    use crate::download;
    use crate::job::Job;
    let d = tmp("dl_other");
    let ctx = eframe::egui::Context::default();
    let job = Job::default();
    let url = std::env::var("CONVER_TEST_URL").unwrap_or_else(|_| "https://vimeo.com/76979871".into());
    let dd = d.clone();
    job.start(&ctx, move |ctl| {
        let r = download::download(&url, &dd, download::Kind::Mp4, 4, false, None, Default::default(), ctl);
        println!("{r:?}");
    });
    while job.state().running {
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let st = job.state();
    println!("status: {} log: {:?}", st.status, st.log.iter().map(|l| &l.1).collect::<Vec<_>>());
    drop(st);
    assert!(std::fs::read_dir(&d).unwrap().flatten().any(|e| tools::ext_lower(&e.path()) == "mp4"));
}
