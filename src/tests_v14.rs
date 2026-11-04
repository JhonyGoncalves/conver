//! Testes do medidor do gravador de áudio e da prévia da gravação de tela.

use crate::recorder::{self, Monitor};
use crate::screenrec::{self, Area};
use std::path::PathBuf;

/// Escuta um som gerado pelo ffmpeg até ele acabar e devolve (amostras, pico, estouros).
fn listen(src: &str) -> (u64, f32, u32) {
    let input: Vec<String> = ["-f", "lavfi", "-i", src].map(String::from).to_vec();
    let mut m = Monitor::start_input(&input, "teste", || {}).unwrap();
    let t = std::time::Instant::now();
    while m.check().is_ok() && t.elapsed().as_secs() < 20 {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    std::thread::sleep(std::time::Duration::from_millis(200)); // o leitor termina de ler o que sobrou
    let lv = m.meter.lock().unwrap();
    (lv.samples, lv.blocks.iter().map(|b| b.peak).fold(0.0, f32::max), lv.clips)
}

#[test]
fn recorder_levels() {
    // seno com amplitude 0,25 (−12 dB) por 2 s: nada estoura
    let (samples, peak, clips) = listen("aevalsrc=0.25*sin(2*PI*440*t):s=16000:d=2");
    println!("baixo: {samples} amostras, pico {:.1} dB, {clips} estouros", recorder::db(peak));
    assert!((31_000..=33_000).contains(&samples), "{samples}");
    assert!((recorder::db(peak) + 12.0).abs() < 0.5, "{peak}");
    assert_eq!(clips, 0);
    // passando do máximo por 1 s: estoura em todos os blocos
    let (_, peak, clips) = listen("aevalsrc=1.3*sin(2*PI*440*t):s=16000:d=1");
    println!("alto: pico {:.1} dB, {clips} estouros", recorder::db(peak));
    assert!(clips >= 15, "{clips}");
    assert!(recorder::db(0.0) <= -90.0 && (recorder::db(1.0)).abs() < 1e-4);
}

#[test]
fn screen_preview_grab() {
    // tela inteira e uma área: a imagem vem reduzida e com a proporção certa
    let full = screenrec::grab(&Area::TelaInteira, 640).expect("captura da tela");
    println!("tela: {}x{} (original {}x{})", full.width, full.height, full.source.0, full.source.1);
    assert!(full.width <= 640 && full.width > 0 && full.height > 0);
    assert_eq!(full.rgba.len(), (full.width * full.height * 4) as usize);
    let r = (full.source.0 as f32 / full.source.1 as f32) - (full.width as f32 / full.height as f32);
    assert!(r.abs() < 0.05, "proporção {r}");
    let area = screenrec::grab(&Area::Regiao { x: 0, y: 0, w: 400, h: 300 }, 640).expect("captura da área");
    assert_eq!((area.width, area.height, area.source), (400, 300, (400, 300)));
    // não é tudo preto
    assert!(full.rgba.chunks_exact(4).any(|px| px[0] > 10 || px[1] > 10 || px[2] > 10));
    // uma janela que existe (a primeira da lista) e uma que não existe
    if let Some(w) = screenrec::windows().first() {
        // (sem imprimir o título: pode ser uma janela pessoal do usuário)
        let _ = screenrec::grab(&Area::Janela(w.clone()), 640);
    }
    assert!(screenrec::grab(&Area::Janela("janela que não existe 123".into()), 640).is_none());
}

#[test]
fn screen_monitors_pause_quality() {
    // monitores: pelo menos um, o principal primeiro
    let mons = screenrec::monitors();
    println!("{} monitor(es): {:?}", mons.len(), mons.iter().map(|m| (m.w, m.h, m.primary)).collect::<Vec<_>>());
    assert!(!mons.is_empty() && mons[0].primary && mons[0].w > 0);
    // grava uma área, pausa no meio e continua: o MP4 final junta os dois trechos, sem o tempo pausado
    let d = std::env::temp_dir().join("conver_tests").join("tela_pausa");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let dst = d.join("pausa.mp4");
    let mut o = screenrec::Options::new(Area::Regiao { x: 0, y: 0, w: 320, h: 240 });
    o.fps = 15;
    o.quality = screenrec::Quality::Leve;
    let mut s = screenrec::Session::start(&o, &dst, || {}).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1800));
    s.pause().unwrap();
    assert!(s.paused());
    std::thread::sleep(std::time::Duration::from_millis(1500));
    s.resume().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1800));
    let out = s.stop().unwrap();
    let p = crate::video::probe(&out);
    println!("pausa: {p:?}");
    let _ = std::fs::remove_file(&out); // não guarda o que estava na tela
    let dur = p.duration.unwrap_or(0.0);
    assert!(p.has_video && (2.4..4.6).contains(&dur), "duração {dur} (devia ser ~3,6 s, sem os 1,5 s pausados)");
    assert_eq!(p.size, Some((320, 240)));
    // resolução máxima: uma área de 640×480 limitada a 240 de altura sai 320×240
    let r = crate::ops::call(
        "screen_record",
        &serde_json::json!({ "seconds": 1.5, "region": { "x": 0, "y": 0, "width": 640, "height": 480 }, "fps": 10, "quality": "low", "resolution": "720p",
            "output": d.join("res.mp4").display().to_string() }),
        &crate::ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &|_| {} },
    )
    .unwrap();
    let out = PathBuf::from(r["output"].as_str().unwrap());
    let p = crate::video::probe(&out);
    let _ = std::fs::remove_file(&out);
    assert_eq!(p.size, Some((640, 480)), "720p não aumenta uma área menor");
    let mut o = screenrec::Options::new(Area::Regiao { x: 0, y: 0, w: 640, h: 480 });
    o.max_height = Some(240);
    o.fps = 10;
    let dst = d.join("menor.mp4");
    let s = screenrec::Session::start(&o, &dst, || {}).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let out = s.stop().unwrap();
    let p = crate::video::probe(&out);
    let _ = std::fs::remove_file(&out);
    assert_eq!(p.size, Some((320, 240)));
    // o monitor pelo MCP: número inexistente dá erro claro
    let e = crate::ops::call("screen_record", &serde_json::json!({ "seconds": 1, "monitor": 99 }), &crate::ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &|_| {} });
    assert!(e.unwrap_err().contains("monitor"));
    let devs = crate::ops::call("list_devices", &serde_json::json!({}), &crate::ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &|_| {} }).unwrap();
    assert_eq!(devs["monitors"].as_array().unwrap().len(), mons.len());
}

/// Com blocos de 20 ms no DirectShow, o som chega aos poucos (antes vinha de meio em meio segundo e o medidor
/// andava aos trancos). Usa a primeira entrada que estiver entregando som; só mede o intervalo, não grava nada.
#[test]
fn recorder_low_latency() {
    for dev in recorder::devices() {
        let times: std::sync::Arc<std::sync::Mutex<Vec<std::time::Instant>>> = Default::default();
        let t2 = times.clone();
        let m = Monitor::start(&dev, move || t2.lock().unwrap().push(std::time::Instant::now())).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3500));
        drop(m);
        let t = times.lock().unwrap();
        if t.len() < 10 {
            continue; // entrada sem nada conectado: o DirectShow não entrega nada
        }
        let gaps: Vec<u128> = t.windows(2).skip(3).map(|w| (w[1] - w[0]).as_millis()).collect();
        let max = gaps.iter().copied().max().unwrap_or(0);
        println!("{dev}: {} entregas, maior intervalo {max} ms", t.len());
        assert!(max < 200, "o som ainda chega em trancos de {max} ms");
        return;
    }
    println!("nenhuma entrada de áudio entregando som: pulando");
}
