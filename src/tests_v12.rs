//! Testes da versão 1.2: PDF (comprimir, senha, imagens, OCR), IA (ampliar, separar voz, transcrever),
//! gravação de tela, predefinições e o núcleo usado pela linha de comando e pelo MCP.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::imaging::Level;
use crate::{ops, pdf_extra, tools};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ffmpeg(args: &[&str], cwd: Option<&Path>) {
    let ff = tools::find_tool("ffmpeg").expect("ffmpeg");
    let mut c = tools::command(&ff);
    c.args(["-y", "-loglevel", "error"]).args(args);
    if let Some(d) = cwd {
        c.current_dir(d);
    }
    assert!(c.status().unwrap().success(), "ffmpeg {args:?}");
}

fn ctx_call(name: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let ctx = ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &|m| println!("  {m}") };
    ops::call(name, &args, &ctx)
}

/// PDF de 2 páginas "escaneadas" (imagens com texto, sem camada de texto).
fn scanned_pdf(d: &Path) -> PathBuf {
    let fonts = PathBuf::from(std::env::var("WINDIR").unwrap()).join("Fonts");
    let mut pages = Vec::new();
    for (i, text) in ["Relatorio anual Conver 2026", "Segunda pagina com texto"].iter().enumerate() {
        let out = d.join(format!("pag{i}.jpg"));
        ffmpeg(
            &["-f", "lavfi", "-i", "color=white:s=1240x1754", "-vf",
              &format!("drawtext=fontfile=arial.ttf:text='{text}':fontsize=64:fontcolor=black:x=100:y=300,noise=alls=20:allf=t"),
              "-frames:v", "1", "-q:v", "2", &out.to_string_lossy()],
            Some(&fonts),
        );
        pages.push(out);
    }
    let pdf = d.join("escaneado.pdf");
    crate::imaging::images_to_pdf(&pages, &pdf, crate::imaging::PageSize::A4Retrato, &|_| {}, &|| false).unwrap();
    pdf
}

#[test]
fn pdf_tools() {
    let d = tmp("pdf12");
    let pdf = scanned_pdf(&d);
    for sub in ["de_novo", "x", "y", "errada"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    let before = tools::file_size(&pdf);

    // comprimir
    let (info, small) = pdf_extra::compress(&pdf, None, Level::Forte, &|_| {}, &|| false).unwrap();
    println!("comprimir: {info} ({} → {})", tools::human_size(before), tools::human_size(tools::file_size(&small)));
    assert!(tools::file_size(&small) < before);
    assert_eq!(crate::pdf::page_count(&small), Some(2));

    // páginas como imagens (leitor de PDF do Windows)
    let (info, first) = pdf_extra::to_images(&pdf, None, pdf_extra::PageFmt::Png, 72, None, "", &|_| {}, &|| false).unwrap();
    println!("imagens: {info}");
    let img = image::open(&first).unwrap();
    assert!((590..=600).contains(&img.width()), "largura {}", img.width());
    assert_eq!(std::fs::read_dir(first.parent().unwrap()).unwrap().count(), 2);

    // PDF pesquisável: o texto reconhecido passa a existir no PDF
    let (info, searchable) = pdf_extra::make_searchable(&pdf, None, "", true, &|_| {}, &|| false).unwrap();
    println!("ocr: {info}");
    let doc = lopdf::Document::load(&searchable).unwrap();
    let text = doc.extract_text(&[1, 2]).unwrap();
    println!("texto extraído: {text:?}");
    assert!(text.to_lowercase().contains("conver") && text.to_lowercase().contains("segunda"));
    // de novo, pulando páginas com texto: nada a fazer
    let (info2, _) = pdf_extra::make_searchable(&searchable, Some(&d.join("de_novo")), "", true, &|_| {}, &|| false).unwrap();
    assert!(info2.contains("2 já tinham texto"), "{info2}");

    // senha
    let (_, locked) = pdf_extra::protect(&searchable, None, "s3nh@", "", true, false).unwrap();
    let opened = lopdf::Document::load(&locked).unwrap();
    assert!(opened.is_encrypted() || opened.encryption_state.is_some());
    assert!(pdf_extra::to_images(&locked, Some(&d.join("x")), pdf_extra::PageFmt::Png, 50, None, "", &|_| {}, &|| false).is_err());
    let r = pdf_extra::to_images(&locked, Some(&d.join("y")), pdf_extra::PageFmt::Jpg, 50, Some("1"), "s3nh@", &|_| {}, &|| false);
    assert!(r.is_ok(), "{r:?}");
    assert!(pdf_extra::unlock(&locked, Some(&d.join("errada")), "outra").is_err());
    let (_, free) = pdf_extra::unlock(&locked, None, "s3nh@").unwrap();
    let doc = lopdf::Document::load(&free).unwrap();
    assert!(!doc.is_encrypted());
    assert!(doc.extract_text(&[1]).unwrap().to_lowercase().contains("conver"));
}

#[test]
fn upscale_image() {
    let d = tmp("up");
    let src = d.join("pequena.png");
    // padrão com bordas nítidas e transparência
    image::RgbaImage::from_fn(100, 60, |x, y| {
        let on = (x / 10 + y / 10) % 2 == 0;
        image::Rgba(if on { [220, 40, 40, 255] } else { [20, 20, 200, 128] })
    })
    .save(&src)
    .unwrap();
    let t = std::time::Instant::now();
    let r = ctx_call("image_upscale", json!({ "files": [src.display().to_string()], "scale": 4 })).unwrap();
    println!("{r} em {:.1}s", t.elapsed().as_secs_f32());
    let out = PathBuf::from(r["results"][0]["output"].as_str().unwrap());
    let img = image::open(&out).unwrap().to_rgba8();
    assert_eq!((img.width(), img.height()), (400, 240));
    // cor e transparência preservadas no meio de um quadrado
    let p = img.get_pixel(20, 20);
    assert!(p[0] > 180 && p[2] < 90 && p[3] > 240, "{p:?}");
    assert!(img.get_pixel(60, 20)[3] < 160);
}

/// Fala sintetizada pelo próprio Windows (para testar transcrição e separação).
fn speech(d: &Path, text: &str) -> PathBuf {
    let wav = d.join("fala.wav");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
         $s.SetOutputToWaveFile('{}'); $s.Speak('{}'); $s.Dispose()",
        wav.display(),
        text
    );
    let ok = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &script]).status().unwrap().success();
    assert!(ok && wav.is_file(), "síntese de voz do Windows");
    wav
}

#[test]
#[ignore] // baixa o modelo do Whisper (82 MB) na primeira vez
fn transcribe_speech() {
    let d = tmp("whisper");
    let wav = speech(&d, "Hello, this is a test of the Conver transcription tool. It runs offline on Windows.");
    let t = std::time::Instant::now();
    let r = ctx_call("transcribe", json!({ "files": [wav.display().to_string()], "language": "en", "formats": ["txt", "srt"] })).unwrap();
    println!("{} em {:.1}s", serde_json::to_string_pretty(&r).unwrap(), t.elapsed().as_secs_f32());
    let text = r["results"][0]["text"].as_str().unwrap().to_lowercase();
    // a voz sintetizada do Windows é robótica; basta reconhecer as palavras principais
    assert!(text.contains("hello") && text.contains("test") && text.contains("offline"), "{text}");
    assert!(d.join("fala.srt").is_file() && d.join("fala.txt").is_file());
    let srt = std::fs::read_to_string(d.join("fala.srt")).unwrap();
    assert!(srt.contains("-->"));
}

#[test]
#[ignore] // usa o modelo de separação (67 MB, baixado na primeira vez)
fn separate_vocals() {
    let d = tmp("sep");
    let voice = speech(&d, "One two three four five six seven eight nine ten. The quick brown fox jumps over the lazy dog.");
    // "música": acorde constante misturado com a voz
    let mix = d.join("musica.wav");
    ffmpeg(
        &["-i", &voice.to_string_lossy(), "-f", "lavfi", "-i", "sine=f=220:d=8", "-f", "lavfi", "-i", "sine=f=330:d=8",
          "-filter_complex", "[0:a]aresample=44100,pan=stereo|c0=c0|c1=c0,apad[v];[1:a][2:a]amix=inputs=2,aresample=44100,pan=stereo|c0=c0|c1=c0,volume=0.6[m];[v][m]amix=inputs=2:duration=shortest",
          "-t", "7", "-ar", "44100", &mix.to_string_lossy()],
        None,
    );
    let t = std::time::Instant::now();
    let r = ctx_call("audio_separate", json!({ "files": [mix.display().to_string()] })).unwrap();
    println!("{r} em {:.1}s", t.elapsed().as_secs_f32());
    let voz = d.join("musica_voz.wav");
    let inst = d.join("musica_instrumental.wav");
    assert!(voz.is_file() && inst.is_file());
    assert!(crate::video::duration(&voz).unwrap() > 6.5);
    // a energia do tom de 220 Hz deve ficar quase toda no instrumental
    let energy = |f: &Path, filter: &str| -> f64 {
        let ff = tools::find_tool("ffmpeg").unwrap();
        let out = tools::command(&ff)
            .args(["-hide_banner", "-i"]).arg(f)
            .args(["-af", &format!("{filter},volumedetect"), "-f", "null", "NUL"])
            .output().unwrap();
        let s = String::from_utf8_lossy(&out.stderr);
        s.split("mean_volume:").nth(1).and_then(|x| x.trim().split(' ').next()).and_then(|v| v.parse().ok()).unwrap_or(-99.0)
    };
    // a fala (harmônicos de 1 a 4 kHz, onde os tons não chegam) deve ir quase toda para a voz
    let speech_voz = energy(&voz, "highpass=f=1000,lowpass=f=4000");
    let speech_inst = energy(&inst, "highpass=f=1000,lowpass=f=4000");
    println!("fala 1-4 kHz: voz {speech_voz} dB, instrumental {speech_inst} dB");
    assert!(speech_voz > speech_inst + 10.0);
    // e o instrumental fica com o acorde (330 Hz, longe da voz sintetizada)
    assert!(energy(&inst, "bandpass=f=330:w=20") > energy(&voz, "bandpass=f=330:w=20"));
}

#[test]
fn screen_record_short() {
    let d = tmp("tela");
    // área pequena no canto, só para conferir que grava um MP4 válido
    let r = ctx_call("screen_record", json!({ "seconds": 2, "region": { "x": 0, "y": 0, "width": 64, "height": 48 }, "fps": 10,
        "output": d.join("tela.mp4").display().to_string() })).unwrap();
    println!("{r}");
    let out = PathBuf::from(r["output"].as_str().unwrap());
    let p = crate::video::probe(&out);
    let _ = std::fs::remove_file(&out); // não guarda o que estava na tela
    assert!(p.has_video && p.duration.unwrap_or(0.0) >= 1.0, "{p:?}");
    assert_eq!(p.size, Some((64, 48)));
}

#[test]
fn ops_and_presets() {
    let d = tmp("ops");
    // todos os esquemas são objetos com propriedades válidas
    for t in ops::TOOLS {
        let s = (t.schema)();
        assert_eq!(s["type"], "object", "{}", t.name);
        assert!(s["properties"].is_object(), "{}", t.name);
        for req in s["required"].as_array().into_iter().flatten() {
            assert!(s["properties"].get(req.as_str().unwrap()).is_some(), "{}: {req}", t.name);
        }
    }
    let img = d.join("a.png");
    image::RgbImage::from_pixel(300, 200, image::Rgb([10, 200, 30])).save(&img).unwrap();
    // argumentos como texto (linha de comando) também funcionam
    let r = ctx_call("image_resize", json!({ "files": [d.display().to_string()], "width": "150", "keep_aspect": "true" })).unwrap();
    assert_eq!(r["ok"], 1);
    assert!(r["results"][0]["info"].as_str().unwrap().contains("150x100"));
    // erros claros
    let e = ctx_call("image_convert", json!({ "files": [img.display().to_string()], "format": "xyz" })).unwrap_err();
    assert!(e.contains("png") && e.contains("webp"), "{e}");
    assert!(ctx_call("nao_existe", json!({})).is_err());

    // predefinição salva e usada (em pasta de dados temporária não dá: usa a real e apaga depois)
    let name = format!("Teste {}", std::process::id());
    ctx_call("preset_save", json!({ "name": name, "tool": "image_convert", "args": { "format": "jpg", "files": ["ignorado"] } })).unwrap();
    let listed = ctx_call("preset_list", json!({})).unwrap();
    let p = listed["presets"].as_array().unwrap().iter().find(|p| p["name"] == name.as_str()).cloned().unwrap();
    assert!(p["args"].get("files").is_none());
    let r = ctx_call("preset_run", json!({ "name": name, "files": [img.display().to_string()], "out_dir": d.join("jpg").display().to_string() })).unwrap();
    assert!(r["results"][0]["output"].as_str().unwrap().ends_with(".jpg"));
    crate::presets::remove(&name).unwrap();
    assert!(crate::presets::find(&name).is_none());
}
