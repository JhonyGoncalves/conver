//! Testes da atualização feita de dentro do app.

use std::path::PathBuf;

use crate::updater::{self, Mode};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join("conver_tests").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn update_basics() {
    let d = tmp("upd_mode");
    assert_eq!(updater::test_mode_of(&d), Mode::Manual);
    std::fs::write(d.join("unins000.exe"), b"x").unwrap();
    assert_eq!(updater::test_mode_of(&d), Mode::Instalado);
    std::fs::write(d.join("portatil.txt"), b"x").unwrap();
    assert_eq!(updater::test_mode_of(&d), Mode::Portatil);

    let sums = "8c6503b00fcf2744dbb0b7de7a114e2c891fd3412aa867af524a9205cd4602ba  Conver-1.2.0-instalador.exe\r\n\
                4bd389cb4f86d84b1f5ac3479cde95f7a0b6e5f993f1071682729549c9c1b043 *Conver-1.2.0-portatil.zip\n";
    assert_eq!(updater::test_expected_sum(sums, "Conver-1.2.0-portatil.zip").unwrap(), "4bd389cb4f86d84b1f5ac3479cde95f7a0b6e5f993f1071682729549c9c1b043");
    assert!(updater::test_expected_sum(sums, "Conver-1.2.0-instalador.exe").is_some());
    assert!(updater::test_expected_sum(sums, "outro.zip").is_none());

    // a cópia por cima mantém os dados do usuário e o portatil.txt de quem já usa
    let (src, dst) = (d.join("novo"), d.join("atual"));
    for p in [&src, &dst] {
        std::fs::create_dir_all(p.join("bin")).unwrap();
        std::fs::create_dir_all(p.join("dados")).unwrap();
    }
    std::fs::write(src.join("Conver.exe"), b"novo").unwrap();
    std::fs::write(src.join("bin").join("ffmpeg.exe"), b"ff novo").unwrap();
    std::fs::write(src.join("dados").join("config.json"), b"{}").unwrap();
    std::fs::write(src.join("portatil.txt"), b"novo").unwrap();
    std::fs::write(dst.join("Conver.exe"), b"velho").unwrap();
    std::fs::write(dst.join("dados").join("config.json"), b"{\"dark\":true}").unwrap();
    std::fs::write(dst.join("portatil.txt"), b"meu").unwrap();
    let n = updater::test_copy_over(&src, &dst).unwrap();
    assert_eq!(n, 2);
    assert_eq!(std::fs::read(dst.join("Conver.exe")).unwrap(), b"novo");
    assert_eq!(std::fs::read(dst.join("bin").join("ffmpeg.exe")).unwrap(), b"ff novo");
    assert_eq!(std::fs::read(dst.join("dados").join("config.json")).unwrap(), b"{\"dark\":true}");
    assert_eq!(std::fs::read(dst.join("portatil.txt")).unwrap(), b"meu");
}

/// De ponta a ponta com a release publicada: baixa o .zip portátil, confere, extrai e troca os arquivos de uma
/// pasta portátil falsa, esperando um processo "antigo" terminar. Precisa de internet (≈130 MB).
#[test]
#[ignore]
fn update_portable_end_to_end() {
    let d = tmp("upd_e2e");
    let rel = crate::integration::release_by_tag("v1.2.0").unwrap();
    assert!(rel.asset("-portatil.zip").is_some() && rel.asset("-instalador.exe").is_some() && rel.asset("-SHA256.txt").is_some());
    let t = std::time::Instant::now();
    let prep = updater::prepare(&rel, Mode::Portatil, &|_| {}, &|| false).unwrap();
    println!("baixado e conferido em {:.0}s: {}", t.elapsed().as_secs_f32(), prep.path.display());
    assert!(prep.path.join("Conver.exe").is_file() && prep.path.join("conver-cli.exe").is_file());

    // um arquivo com a soma errada não passa
    let mut bad = rel.clone();
    for a in bad.assets.iter_mut() {
        if a.name.ends_with("-SHA256.txt") {
            a.url = rel.url.clone(); // a "lista" vira a página da release: sem a soma do zip
        }
    }
    assert!(updater::prepare(&bad, Mode::Portatil, &|_| {}, &|| false).is_err());

    // pasta portátil "antiga"
    let old = d.join("Conver");
    std::fs::create_dir_all(old.join("dados")).unwrap();
    std::fs::write(old.join("Conver.exe"), b"versao antiga").unwrap();
    std::fs::write(old.join("portatil.txt"), b"x").unwrap();
    std::fs::write(old.join("dados").join("historico.json"), b"[1]").unwrap();
    // processo "antigo" que termina em ~2 s
    let child = std::process::Command::new("ping").args(["-n", "3", "127.0.0.1"]).stdout(std::process::Stdio::null()).spawn().unwrap();
    // SAFETY: variável de ambiente lida só por run_apply neste teste (testes rodam com 1 thread)
    unsafe { std::env::set_var("CONVER_NAO_REABRIR", "1") };
    let t = std::time::Instant::now();
    let code = updater::run_apply(&[prep.path.display().to_string(), old.display().to_string(), child.id().to_string()]);
    println!("aplicado em {:.1}s", t.elapsed().as_secs_f32());
    assert_eq!(code, 0);
    assert!(t.elapsed().as_secs_f32() >= 1.0, "deveria esperar o processo antigo");
    assert!(std::fs::metadata(old.join("Conver.exe")).unwrap().len() > 1_000_000);
    assert!(old.join("bin").join("ffmpeg.exe").is_file() && old.join("modelos").join("silueta.onnx").is_file());
    assert_eq!(std::fs::read(old.join("dados").join("historico.json")).unwrap(), b"[1]");
    let _ = std::fs::remove_dir_all(prep.path.parent().unwrap().parent().unwrap());
}
