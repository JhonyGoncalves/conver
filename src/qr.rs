//! Gerador de QR Code.

use image::{Rgba, RgbaImage};
use qrcode::{Color, EcLevel, QrCode};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    Baixa,
    Media,
    Alta,
}

impl Correction {
    fn level(self) -> EcLevel {
        match self {
            Correction::Baixa => EcLevel::L,
            Correction::Media => EcLevel::M,
            Correction::Alta => EcLevel::H,
        }
    }
}

/// Desenha o QR Code com ~`size` px de lado e borda de 4 módulos.
pub fn render(text: &str, size: u32, fg: [u8; 3], correction: Correction) -> Result<RgbaImage, String> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), correction.level())
        .map_err(|_| "texto grande demais para um QR Code.".to_string())?;
    let modules = code.width() as u32;
    let colors = code.to_colors();
    let total = modules + 8;
    let scale = (size / total).max(1);
    let px = total * scale;
    let mut img = RgbaImage::from_pixel(px, px, Rgba([255, 255, 255, 255]));
    for (i, c) in colors.iter().enumerate() {
        if *c == Color::Dark {
            let (mx, my) = (i as u32 % modules + 4, i as u32 / modules + 4);
            for y in my * scale..(my + 1) * scale {
                for x in mx * scale..(mx + 1) * scale {
                    img.put_pixel(x, y, Rgba([fg[0], fg[1], fg[2], 255]));
                }
            }
        }
    }
    Ok(img)
}
