//! Colar da área de transferência: arquivos copiados no Explorer entram como estão; um print ou imagem copiada vira um
//! PNG em Imagens\Conver\Prints, e os resultados saem ao lado dele.

use std::path::{Path, PathBuf};

use image::RgbaImage;

use crate::tools;

/// Pasta dos prints colados.
pub fn prints_dir() -> PathBuf {
    // i18n: fora
    crate::imagegen::default_dir().join("Prints")
    // i18n: dentro
}

/// O ajuste de opacidade do print: vários programas (e a tecla PrintScreen) mandam a imagem com o canal alfa todo em
/// zero, sem que isso signifique transparência. Sem o ajuste o PNG sairia invisível.
fn fix_alpha(img: &mut RgbaImage) {
    if img.pixels().all(|p| p[3] == 0) {
        for p in img.pixels_mut() {
            p[3] = 255;
        }
    }
}

/// Salva a imagem colada na pasta `dir` e devolve o caminho.
pub(crate) fn save(mut img: RgbaImage, dir: &Path) -> Result<PathBuf, String> {
    fix_alpha(&mut img);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
    let path = tools::unique_path(dir.join(format!("print_{stamp}.png")));
    img.save(&path).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok(path)
}

/// O que há para colar. Vazio se a área de transferência só tiver texto, ou nada.
pub fn from_clipboard() -> Result<Vec<PathBuf>, String> {
    let mut c = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    if let Ok(files) = c.get().file_list() {
        let files: Vec<PathBuf> = files.into_iter().filter(|f| f.exists()).collect();
        if !files.is_empty() {
            return Ok(files);
        }
    }
    let Ok(img) = c.get_image() else { return Ok(Vec::new()) };
    let rgba = RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()).ok_or("imagem inválida.")?;
    Ok(vec![save(rgba, &prints_dir())?])
}

/// Tecla V apertada agora nesta janela. O egui só avisa de um Ctrl+V quando há texto para colar, então com um print
/// na área de transferência ele não diz nada: é preciso olhar a tecla direto.
pub fn v_key_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
    unsafe { (GetKeyState(0x56) as u16) & 0x8000 != 0 }
}
