//! Testes da terceira leva de ferramentas (rodar com `cargo test --release -- --nocapture --test-threads 1`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::imaging::{self, Filter, Fmt, ImageOp, Level, PageSize};
use crate::job::Job;
use crate::media::AFmt;
use crate::media_extra::{self, SubSize, VAspect};
use crate::video::{self, VFmt, VLevel};
use crate::watch::{self, Action, Options};
use crate::{download, pdf, tools};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
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
fn color_adjust() {
    let d = tmp("adjust");
    let img = image::RgbImage::from_fn(200, 100, |x, _| image::Rgb([(x % 256) as u8, 100, 50]));
    let src = d.join("a.png");
    img.save(&src).unwrap();
    let mean = |p: &Path| {
        let i = image::open(p).unwrap().to_rgb8();
        i.pixels().map(|p| p.0.iter().map(|v| *v as u64).sum::<u64>()).sum::<u64>() as f64 / (i.len() as f64)
    };
    let op = |b, c, s, f| ImageOp::Adjust { brightness: b, contrast: c, saturation: s, filter: f };
    let (info, bright) = imaging::process_and_save(&src, &op(50, 0, 0, Filter::Nenhum), None).unwrap();
    println!("ajuste: {info}");
    assert!(mean(&bright) > mean(&src) + 30.0);
    let (_, gray) = imaging::process_and_save(&src, &op(0, 0, 0, Filter::PretoBranco), None).unwrap();
    assert!(image::open(&gray).unwrap().to_rgb8().pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
    let (_, desat) = imaging::process_and_save(&src, &op(0, 0, -100, Filter::Nenhum), None).unwrap();
    assert!(image::open(&desat).unwrap().to_rgb8().pixels().all(|p| p[0].abs_diff(p[1]) <= 1));
    let (_, inv) = imaging::process_and_save(&src, &op(0, 0, 0, Filter::Inverter), None).unwrap();
    assert_eq!(image::open(&inv).unwrap().to_rgb8().get_pixel(10, 10).0, [245, 155, 205]);
    let (_, flat) = imaging::process_and_save(&src, &op(0, -100, 0, Filter::Nenhum), None).unwrap();
    assert!(image::open(&flat).unwrap().to_rgb8().pixels().all(|p| p.0 == [128, 128, 128]));
}

#[test]
fn video_extras() {
    let d = tmp("vid_extra");
    let v = d.join("v.mp4");
    ffmpeg_make(
        &["-f", "lavfi", "-i", "testsrc2=duration=8:size=640x360:rate=30", "-f", "lavfi", "-i", "sine=frequency=300:duration=8",
          "-shortest", "-c:v", "libx264", "-c:a", "aac"],
        &v,
    );
    let music = d.join("musica.wav");
    ffmpeg_make(&["-f", "lavfi", "-i", "sine=frequency=880:duration=3"], &music);
    let (np, no) = (|_f: f32| {}, || false);

    // GIF de trecho
    let (info, gif) = media_extra::video_gif(&v, None, 2.0, 3.0, 320, 10, &np, &no).unwrap();
    let g = video::probe(&gif);
    println!("gif: {info} {:?} {:?}", g.size, g.duration);
    assert_eq!(g.size, Some((320, 180)));
    assert!((dur(&gif) - 3.0).abs() < 0.3);

    // girar e recortar
    let (_, r) = media_extra::video_transform(&v, None, 90, false, VAspect::Original, &np, &no).unwrap();
    assert_eq!(video::probe(&r).size, Some((360, 640)));
    let (info, c) = media_extra::video_transform(&v, None, 0, true, VAspect::Vertical, &np, &no).unwrap();
    println!("recorte 9:16: {info}");
    assert_eq!(video::probe(&c).size, Some((202, 360)));
    let (_, q) = media_extra::video_transform(&v, None, 270, false, VAspect::Quadrado, &np, &no).unwrap();
    assert_eq!(video::probe(&q).size, Some((360, 360)));
    assert!(video::probe(&q).has_audio);
    assert!(media_extra::video_transform(&v, None, 0, false, VAspect::Original, &np, &no).is_err());

    // legenda: em UTF-8 (com o mesmo nome do vídeo) e em Windows-1252
    let srt = "1\n00:00:01,000 --> 00:00:04,000\nOlá, legenda com acentuação: ção ã é\n\n2\n00:00:05,000 --> 00:00:07,000\nSegunda linha\n";
    std::fs::write(d.join("v.srt"), srt).unwrap();
    assert_eq!(media_extra::find_subtitle(&v), Some(d.join("v.srt")));
    let (info, s1) = media_extra::burn_subtitles(&v, None, &d.join("v.srt"), SubSize::Grande, false, &np, &no).unwrap();
    println!("legenda: {info}");
    assert!((dur(&s1) - 8.0).abs() < 0.2);
    let latin: Vec<u8> = srt.chars().map(|c| c as u32 as u8).collect(); // Latin-1/CP1252
    let srt2 = d.join("outra legenda (pt).srt");
    std::fs::write(&srt2, latin).unwrap();
    let (_, s2) = media_extra::burn_subtitles(&v, None, &srt2, SubSize::Pequena, true, &np, &no).unwrap();
    assert!(tools::file_size(&s2) > 0);
    // a legenda mudou a imagem: o quadro em 2 s deve ser diferente do original
    let frame = |p: &Path, name: &str| {
        let out = d.join(name);
        ffmpeg_make(&["-ss", "2", "-i", &p.to_string_lossy(), "-frames:v", "1"], &out);
        image::open(&out).unwrap().to_rgb8()
    };
    let (f0, f1) = (frame(&v, "f0.png"), frame(&s1, "f1.png"));
    let diff = f0.pixels().zip(f1.pixels()).filter(|(a, b)| a.0.iter().zip(b.0.iter()).any(|(x, y)| x.abs_diff(*y) > 60)).count();
    println!("pixels alterados pela legenda: {diff}");
    assert!(diff > 500);

    // música: misturar (com repetição) e substituir
    let (info, m1) = media_extra::add_music(&v, None, &music, false, 50, true, &np, &no).unwrap();
    println!("música: {info} = {:.2}s", dur(&m1));
    assert!((dur(&m1) - 8.0).abs() < 0.2 && video::probe(&m1).has_audio);
    let (_, m2) = media_extra::add_music(&v, None, &music, true, 100, false, &np, &no).unwrap();
    assert!((dur(&m2) - 8.0).abs() < 0.3);

    // juntar áudios (um deles vem de um vídeo)
    let (info, j) = media_extra::merge_audio(&[music.clone(), v.clone(), music.clone()], None, AFmt::Mp3, &np, &no).unwrap();
    println!("juntar áudios: {info} = {:.2}s", dur(&j));
    assert!((dur(&j) - 14.0).abs() < 0.3 && !video::probe(&j).has_video);
    let (_, jw) = media_extra::merge_audio(&[music.clone(), music.clone()], None, AFmt::Wav, &np, &no).unwrap();
    assert!((dur(&jw) - 6.0).abs() < 0.1);
}

fn media_boxes(p: &Path) -> Vec<(f32, f32)> {
    let doc = lopdf::Document::load(p).unwrap();
    doc.get_pages()
        .into_values()
        .map(|id| {
            let d = doc.get_dictionary(id).unwrap();
            let mb = d.get(b"MediaBox").unwrap().as_array().unwrap();
            (mb[2].as_float().unwrap(), mb[3].as_float().unwrap())
        })
        .collect()
}

#[test]
fn pdf_tools() {
    let d = tmp("pdf");
    let img = image::RgbImage::from_fn(300, 200, |x, y| image::Rgb([x as u8, y as u8, 120]));
    let i1 = d.join("i.png");
    img.save(&i1).unwrap();
    let (a, b) = (d.join("a.pdf"), d.join("b.pdf"));
    imaging::images_to_pdf(&[i1.clone(), i1.clone(), i1.clone()], &a, PageSize::A4Retrato, &|_| {}, &|| false).unwrap();
    imaging::images_to_pdf(&[i1.clone(), i1.clone()], &b, PageSize::A4Paisagem, &|_| {}, &|| false).unwrap();
    let m = d.join("m.pdf");
    let n = pdf::merge(&[a.clone(), b.clone()], &m, &|| false).unwrap();
    assert_eq!(n, 5);
    assert_eq!(pdf::page_count(&m), Some(5));
    let boxes = media_boxes(&m);
    println!("páginas: {boxes:?}");
    assert_eq!(boxes, vec![(595.0, 842.0), (595.0, 842.0), (595.0, 842.0), (842.0, 595.0), (842.0, 595.0)]);
    // ordem invertida
    let m2 = d.join("m2.pdf");
    pdf::merge(&[b.clone(), a.clone()], &m2, &|| false).unwrap();
    assert_eq!(media_boxes(&m2)[0], (842.0, 595.0));

    assert_eq!(pdf::parse_ranges("1-3, 5", 9).unwrap(), vec![1, 2, 3, 5]);
    assert_eq!(pdf::parse_ranges("8-", 9).unwrap(), vec![8, 9]);
    assert_eq!(pdf::parse_ranges("-2;4", 9).unwrap(), vec![1, 2, 4]);
    assert!(pdf::parse_ranges("3-1", 9).is_err() && pdf::parse_ranges("12", 9).is_err() && pdf::parse_ranges("x", 9).is_err());

    let (info, part) = pdf::split(&m, None, Some("3-4"), &|_| {}, &|| false).unwrap();
    println!("dividir: {info} -> {}", tools::file_name(&part));
    assert_eq!(media_boxes(&part), vec![(595.0, 842.0), (842.0, 595.0)]);
    let (info, first) = pdf::split(&m, None, None, &|_| {}, &|| false).unwrap();
    println!("dividir tudo: {info}");
    let files: Vec<_> = std::fs::read_dir(first.parent().unwrap()).unwrap().flatten().collect();
    assert_eq!(files.len(), 5);
    assert!(files.iter().all(|f| pdf::page_count(&f.path()) == Some(1)));
    // arquivo que não é PDF
    std::fs::write(d.join("falso.pdf"), "nada").unwrap();
    assert!(pdf::merge(&[a.clone(), d.join("falso.pdf")], &d.join("x.pdf"), &|| false).is_err());
}

#[test]
fn links_and_watch() {
    let links = download::parse_links("https://a.com/1\n  https://b.com/2, https://a.com/1\ntexto qualquer\nhttp://c.com");
    assert_eq!(links, vec!["https://a.com/1", "https://b.com/2", "http://c.com"]);

    let d = tmp("watch");
    let old = d.join("antiga.png");
    image::RgbImage::from_pixel(50, 50, image::Rgb([1, 2, 3])).save(&old).unwrap();
    let ctx = eframe::egui::Context::default();
    let job = Job::default();
    let options = Options {
        action: Action::ConvertImages,
        img_level: Level::Media,
        img_fmt: Fmt::Webp,
        vid_level: VLevel::Media,
        vid_fmt: VFmt::Mp4,
        aud_fmt: AFmt::Mp3,
    };
    let folder = d.clone();
    job.start(&ctx, move |ctl| watch::run(ctl, &folder, options, false));
    std::thread::sleep(Duration::from_millis(500));
    image::RgbImage::from_pixel(80, 60, image::Rgb([200, 10, 10])).save(d.join("nova.png")).unwrap();
    let out = d.join(watch::OUT_FOLDER).join("nova.webp");
    let t = Instant::now();
    while !out.is_file() && t.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(200));
    }
    job.cancel();
    while job.state().running {
        std::thread::sleep(Duration::from_millis(100));
    }
    let st = job.state();
    println!("pasta vigiada: {} {:?}", st.status, st.log.iter().map(|l| &l.1).collect::<Vec<_>>());
    drop(st);
    assert!(out.is_file());
    assert_eq!(image::open(&out).unwrap().width(), 80);
    // arquivo que já estava na pasta não foi processado
    assert!(!d.join(watch::OUT_FOLDER).join("antiga.webp").exists());
}

/// Legendas e capa junto do download (precisa de internet).
#[test]
#[ignore]
fn download_extras() {
    let d = tmp("dl_extras");
    let ctx = eframe::egui::Context::default();
    let job = Job::default();
    let dd = d.clone();
    job.start(&ctx, move |ctl| {
        let extras = download::Extras { subtitles: true, cover: true };
        let r = download::download("https://www.youtube.com/watch?v=jNQXAC9IVRw", &dd, download::Kind::Mp4, 4, false, None, extras, ctl);
        println!("{r:?}");
    });
    while job.state().running {
        std::thread::sleep(Duration::from_millis(200));
    }
    let names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    println!("arquivos: {names:?}");
    assert!(names.iter().any(|n| n.ends_with(".mp4")));
    assert!(names.iter().any(|n| n.ends_with(".jpg")));
    assert!(names.iter().any(|n| n.ends_with(".srt")));
}
