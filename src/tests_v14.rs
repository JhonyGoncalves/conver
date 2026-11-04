//! Testes do medidor do gravador de áudio e da prévia da gravação de tela.

use crate::recorder::{self, Monitor};
use crate::screenrec::{self, Area};

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
