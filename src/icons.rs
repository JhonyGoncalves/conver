//! Ícones e favicon: de uma imagem, um .ico com vários tamanhos (Windows, atalhos) ou o pacote completo para sites
//! (favicon.ico, PNGs para navegadores, Apple e Android, e o site.webmanifest).

use std::path::{Path, PathBuf};

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{ExtendedColorType, RgbaImage};

use crate::tools;

/// Tamanhos de um .ico completo (o Windows escolhe o melhor para cada lugar).
pub const ICO_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Opts {
    /// Pacote para sites (pasta) em vez de só o .ico.
    pub web: bool,
    /// Imagem que não é quadrada: cortar o centro (`true`) ou caber inteira com bordas transparentes.
    pub crop: bool,
}

/// A imagem em quadrado: o centro, ou a imagem inteira com bordas transparentes.
pub fn square(img: &RgbaImage, crop: bool) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w == h {
        return img.clone();
    }
    if crop {
        let s = w.min(h);
        image::imageops::crop_imm(img, (w - s) / 2, (h - s) / 2, s, s).to_image()
    } else {
        let s = w.max(h);
        let mut out = RgbaImage::new(s, s);
        image::imageops::overlay(&mut out, img, ((s - w) / 2) as i64, ((s - h) / 2) as i64);
        out
    }
}

fn sized(sq: &RgbaImage, size: u32) -> RgbaImage {
    image::imageops::resize(sq, size, size, if size < sq.width() { FilterType::Lanczos3 } else { FilterType::CatmullRom })
}

/// Um .ico com os tamanhos pedidos (cada um guardado como PNG, como o Windows aceita desde o Vista).
pub fn ico_bytes(sq: &RgbaImage, sizes: &[u32]) -> Result<Vec<u8>, String> {
    let mut pngs = Vec::new();
    for &s in sizes {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(sized(sq, s)).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        pngs.push((s, png));
    }
    let frames = pngs.iter().map(|(s, png)| IcoFrame::with_encoded(png.as_slice(), *s, *s, ExtendedColorType::Rgba8)).collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    IcoEncoder::new(&mut out).encode_images(&frames).map_err(|e| e.to_string())?;
    Ok(out)
}

fn save_png(img: &RgbaImage, path: &Path) -> Result<(), String> {
    img.save(path).map_err(|e| format!("não foi possível salvar: {e}"))
}

/// Gera o .ico (ou a pasta do pacote para sites) e devolve o resumo e o caminho.
pub fn make(src: &Path, out_dir: Option<&Path>, o: Opts) -> Result<(String, PathBuf), String> {
    let img = crate::imaging::load(src)?.to_rgba8();
    let sq = square(&img, o.crop);
    let small = sq.width() < 256;
    if !o.web {
        let dst = tools::out_path(src, out_dir, "", "ico");
        std::fs::write(&dst, ico_bytes(&sq, &ICO_SIZES)?).map_err(|e| format!("não foi possível salvar: {e}"))?;
        let mut info = crate::i18n::tr("7 tamanhos (16 a 256 px)");
        if small {
            info.push_str(" • ");
            info.push_str(&crate::i18n::tr("a imagem tem menos de 256 px: os tamanhos grandes ficam borrados"));
        }
        return Ok((info, dst));
    }
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "icone".into());
    let base = out_dir.map(Path::to_path_buf).or_else(|| src.parent().map(Path::to_path_buf)).unwrap_or_default();
    let dir = tools::unique_path(base.join(format!("{stem}_icones")));
    std::fs::create_dir_all(&dir).map_err(|e| format!("não foi possível criar a pasta: {e}"))?;
    std::fs::write(dir.join("favicon.ico"), ico_bytes(&sq, &[16, 32, 48])?).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{stem}.ico")), ico_bytes(&sq, &ICO_SIZES)?).map_err(|e| e.to_string())?;
    for (name, s) in [("favicon-16x16.png", 16), ("favicon-32x32.png", 32), ("android-chrome-192x192.png", 192), ("android-chrome-512x512.png", 512)] {
        save_png(&sized(&sq, s), &dir.join(name))?;
    }
    // a Apple não usa transparência: o fundo vira branco
    let mut apple = RgbaImage::from_pixel(180, 180, image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut apple, &sized(&sq, 180), 0, 0);
    save_png(&apple, &dir.join("apple-touch-icon.png"))?;
    // i18n: fora
    let manifest = format!(
        "{{\n  \"name\": \"{stem}\",\n  \"short_name\": \"{stem}\",\n  \"icons\": [\n    {{ \"src\": \"/android-chrome-192x192.png\", \"sizes\": \"192x192\", \"type\": \"image/png\" }},\n    {{ \"src\": \"/android-chrome-512x512.png\", \"sizes\": \"512x512\", \"type\": \"image/png\" }}\n  ],\n  \"display\": \"standalone\"\n}}\n"
    );
    std::fs::write(dir.join("site.webmanifest"), manifest).map_err(|e| e.to_string())?;
    let html = "<link rel=\"icon\" href=\"/favicon.ico\" sizes=\"any\">\n<link rel=\"icon\" type=\"image/png\" sizes=\"32x32\" href=\"/favicon-32x32.png\">\n<link rel=\"icon\" type=\"image/png\" sizes=\"16x16\" href=\"/favicon-16x16.png\">\n<link rel=\"apple-touch-icon\" sizes=\"180x180\" href=\"/apple-touch-icon.png\">\n<link rel=\"manifest\" href=\"/site.webmanifest\">\n";
    // i18n: dentro
    std::fs::write(dir.join("cole-no-head.html"), html).map_err(|e| e.to_string())?;
    let mut info = crate::i18n::tr("10 arquivos para sites");
    if sq.width() < 512 {
        info.push_str(" • ");
        info.push_str(&crate::i18n::tr("a imagem tem menos de 512 px: os PNGs grandes ficam borrados"));
    }
    Ok((info, dir))
}
