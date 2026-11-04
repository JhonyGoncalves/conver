//! Testes da versão 1.1: remover fundo e idiomas.

use std::path::PathBuf;

use crate::imaging::{self, Background, ImageOp};
use crate::tools;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn i18n_files() {
    use crate::i18n::{self, tr};
    let problems = i18n::check_files();
    for p in &problems {
        println!("{p}");
    }
    i18n::set("en");
    let cases = [
        ("Comprimir", "Compress"),
        ("3 arquivos", "3 files"),
        ("Concluído: 2 de 5 arquivo(s).", "Done: 2 of 5 file(s)."),
        ("✔ foto.jpg   qualidade 40 • 1.2 MB → 300 KB (-75%)", "✔ foto.jpg   quality 40 • 1.2 MB → 300 KB (-75%)"),
        ("✖ v.mp4: o arquivo não tem áudio.", "✖ v.mp4: the file has no audio."),
        ("[1/3] Lendo print.png…", "[1/3] Reading print.png…"),
        ("foto_de_ferias.jpg", "foto_de_ferias.jpg"),
        ("Resultado: 400×300  •  24.2 KB   (800x600 → 400x300)", "Result: 400×300  •  24.2 KB   (800x600 → 400x300)"),
    ];
    for (pt, en) in cases {
        assert_eq!(tr(pt), en, "tradução de {pt:?}");
    }
    // escrita da direita para a esquerda: letras árabes unidas (formas de apresentação) e ordem visual
    i18n::set("ar");
    let ar = tr("Comprimir");
    println!("ar: {ar}");
    assert!(ar.chars().any(|c| ('\u{FE70}'..='\u{FEFF}').contains(&c)) || ar == "Comprimir");
    // parênteses na ordem certa depois de inverter o texto ("(OCR)", não ")OCR(")
    let ocr = tr("Texto de imagem (OCR)");
    println!("ar: {ocr}");
    assert!(ocr.contains("(OCR)"), "{ocr}");
    // outros idiomas carregam sem erro e traduzem o menu
    for (code, word) in [("zh", "压缩"), ("ja", "圧縮"), ("ko", "압축"), ("ru", "Сжать"), ("de", "Komprimieren"), ("sw", "Bana")] {
        i18n::set(code);
        assert_eq!(tr("Comprimir"), word, "{code}");
    }
    i18n::set("fa");
    assert!(!tr("Dividir PDF").contains('\u{200C}'));
    i18n::set("pt");
    assert_eq!(tr("Comprimir"), "Comprimir");
    assert!(problems.is_empty(), "{} problema(s) nos arquivos de tradução", problems.len());
}

#[test]
fn remove_background() {
    let d = tmp("bg");
    // objeto (círculo vermelho com sombra) sobre um fundo verde com textura
    let (w, h) = (480u32, 360u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let (dx, dy) = (x as f32 - 240.0, y as f32 - 180.0);
        let r = (dx * dx + dy * dy).sqrt();
        if r < 100.0 {
            let shade = (1.0 - r / 140.0) * 60.0;
            image::Rgb([(190.0 + shade) as u8, (30.0 + shade / 2.0) as u8, 40])
        } else {
            let n = ((x * 7 + y * 13) % 23) as u8;
            image::Rgb([60 + n, 150 + n, 70 + n])
        }
    });
    let src = d.join("objeto.jpg");
    img.save(&src).unwrap();

    let t = std::time::Instant::now();
    let (info, out) = imaging::process_and_save(&src, &ImageOp::RemoveBg { bg: Background::Transparente, hard: true }, None).unwrap();
    println!("{info} em {:.1}s -> {}", t.elapsed().as_secs_f32(), tools::file_name(&out));
    assert_eq!(tools::ext_lower(&out), "png");
    let res = image::open(&out).unwrap().to_rgba8();
    let alpha = |x: u32, y: u32| res.get_pixel(x, y)[3];
    println!("alfa: centro {} | cantos {} {} {} {}", alpha(240, 180), alpha(5, 5), alpha(w - 5, 5), alpha(5, h - 5), alpha(w - 5, h - 5));
    assert!(alpha(240, 180) > 200, "o objeto deveria ficar");
    assert!([alpha(5, 5), alpha(w - 5, 5), alpha(5, h - 5), alpha(w - 5, h - 5)].iter().all(|a| *a < 60), "o fundo deveria sair");

    // fundo branco mantém JPG
    let t = std::time::Instant::now();
    let (_, white) = imaging::process_and_save(&src, &ImageOp::RemoveBg { bg: Background::Cor(255, 255, 255), hard: false }, None).unwrap();
    println!("segunda imagem (modelo já carregado): {:.1}s", t.elapsed().as_secs_f32());
    assert_eq!(tools::ext_lower(&white), "jpg");
    let px = image::open(&white).unwrap().to_rgb8();
    assert!(px.get_pixel(5, 5).0.iter().all(|c| *c > 200));
}
