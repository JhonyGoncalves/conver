//! Testes das conversões (rodar com `cargo test --release -- --nocapture --test-threads 1`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::imaging::{self, Fmt, ImageOp, Level, ResizeMode};
use crate::job::Job;
use crate::video::{self, Codec, CompressMode, CompressOpts, Res, VFmt, VLevel, VQuality};
use crate::{download, tools};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sample_images(dir: &Path) -> (PathBuf, PathBuf) {
    let img = image::RgbaImage::from_fn(800, 600, |x, y| {
        let a = if x < 100 { 0 } else { 255 };
        image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x * y) % 256) as u8, a])
    });
    let png = dir.join("a.png");
    img.save(&png).unwrap();
    let jpg = dir.join("b.jpg");
    image::DynamicImage::ImageRgba8(img).to_rgb8().save(&jpg).unwrap();
    (png, jpg)
}

#[test]
fn image_ops() {
    let d = tmp("img");
    let (png, jpg) = sample_images(&d);
    let ops = [
        ImageOp::Resize(ResizeMode::Percent(50)),
        ImageOp::Resize(ResizeMode::Pixels { w: 300, h: 0, keep: true }),
        ImageOp::Resize(ResizeMode::Pixels { w: 100, h: 100, keep: false }),
        ImageOp::Compress(Level::Leve),
        ImageOp::Compress(Level::Forte),
        ImageOp::Quality(20),
    ];
    for src in [&png, &jpg] {
        for op in &ops {
            let (info, dst) = imaging::process_and_save(src, op, None).unwrap();
            image::open(&dst).unwrap();
            println!("{:?} {op:?}: {info}", tools::file_name(&dst));
        }
        for f in Fmt::ALL {
            let (info, dst) = imaging::process_and_save(src, &ImageOp::Convert(f), Some(&d)).unwrap();
            image::open(&dst).unwrap();
            println!("{:?}: {info}", tools::file_name(&dst));
        }
    }
    // pixels sem proporção e sem altura deve falhar
    assert!(imaging::process(&png, &ImageOp::Resize(ResizeMode::Pixels { w: 10, h: 0, keep: false })).is_err());
    let out = imaging::process(&png, &ImageOp::Resize(ResizeMode::Pixels { w: 300, h: 0, keep: true })).unwrap();
    assert_eq!(out.dims, (300, 225));
}

fn sample_video(dir: &Path, secs: u32) -> PathBuf {
    let v = dir.join("v.mp4");
    let ff = tools::find_tool("ffmpeg").expect("ffmpeg");
    let ok = tools::command(&ff)
        .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
        .arg(format!("testsrc2=duration={secs}:size=1280x720:rate=30"))
        .args(["-f", "lavfi", "-i"])
        .arg(format!("sine=duration={secs}"))
        .args(["-shortest", "-c:v", "libx264", "-crf", "12"])
        .arg(&v)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    v
}

#[test]
fn video_ops() {
    let d = tmp("vid");
    let v = sample_video(&d, 10);
    assert!((video::duration(&v).unwrap() - 10.0).abs() < 0.2);
    let calls = Mutex::new(0);
    let prog = |_f: f32| *calls.lock().unwrap() += 1;
    let no = || false;
    for f in VFmt::ALL {
        let (info, dst) = video::convert(&v, None, f, VQuality::Baixa, &prog, &no).unwrap();
        println!("{}: {info}", tools::file_name(&dst));
    }
    for codec in [Codec::H264, Codec::H265] {
        let o = CompressOpts { mode: CompressMode::Level(VLevel::Forte), res: Res::P480, codec, audio_kbps: 96 };
        let (info, dst) = video::compress(&v, None, o, &prog, &no).unwrap();
        println!("level {}: {info}", tools::file_name(&dst));
        let o = CompressOpts { mode: CompressMode::TargetMb(1.0), res: Res::Original, codec, audio_kbps: 64 };
        let (info, dst) = video::compress(&v, None, o, &prog, &no).unwrap();
        let mb = tools::file_size(&dst) as f64 / 1024.0 / 1024.0;
        println!("target 1MB {}: {info} = {mb:.2} MB", tools::file_name(&dst));
        assert!(mb > 0.7 && mb < 1.15, "tamanho alvo fora: {mb}");
    }
    let o = CompressOpts { mode: CompressMode::TargetMb(0.01), res: Res::Original, codec: Codec::H264, audio_kbps: 64 };
    assert!(video::compress(&v, None, o, &prog, &no).is_err());
    assert!(*calls.lock().unwrap() > 10);
    // cancelar
    let r = video::compress(
        &v,
        None,
        CompressOpts { mode: CompressMode::Level(VLevel::Leve), res: Res::Original, codec: Codec::H265, audio_kbps: 128 },
        &prog,
        &|| true,
    );
    assert_eq!(r.unwrap_err(), "Cancelado.");
}

#[test]
fn youtube_download() {
    let d = tmp("dl");
    let ctx = eframe::egui::Context::default();
    let job = Job::default();
    let result: Arc<Mutex<Vec<String>>> = Arc::default();
    for (kind, q) in [(download::Kind::Mp3, 1), (download::Kind::Mp4, 4)] {
        let (d, r) = (d.clone(), result.clone());
        job.start(&ctx, move |ctl| {
            let files = download::download("https://www.youtube.com/watch?v=jNQXAC9IVRw", &d, kind, q, false, None, Default::default(), ctl);
            r.lock().unwrap().push(format!("{files:?}"));
        });
        while job.state().running {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let st = job.state();
        println!("status: {} log: {:?}", st.status, st.log.iter().map(|l| &l.1).collect::<Vec<_>>());
    }
    let r = result.lock().unwrap();
    println!("{r:?}");
    assert!(d.join("Me at the zoo.mp3").is_file());
    assert!(d.join("Me at the zoo.mp4").is_file());
}
