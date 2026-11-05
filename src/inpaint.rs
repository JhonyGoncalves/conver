//! Apagar objeto: a IA LaMa (ONNX, 208 MB, Apache-2.0) preenche a área pintada com o que "deveria" estar atrás.
//! O modelo trabalha em 512×512: recortamos só a região em volta da máscara (com contexto), passamos pelo modelo e
//! colamos de volta apenas os pixels pintados, com a borda suavizada; o resto da foto fica intacto, na resolução
//! original.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::{GrayImage, Luma, RgbImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

use crate::ai::{self, ModelSpec};
use crate::tools;

pub const MODEL: ModelSpec = ModelSpec {
    file: "lama_fp32.onnx",
    url: "https://huggingface.co/Carve/LaMa-ONNX/resolve/main/lama_fp32.onnx",
    sha256: "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6",
    size: 208_044_816,
    label: "LaMa (208 MB)",
};

const SIDE: u32 = 512;

fn model(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(&MODEL, on_progress, cancelled)?;
    let s = ai::session(&path)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

/// Retângulo (x0, y0, x1, y1) com todos os pixels marcados, ou `None` se a máscara está vazia.
fn bbox(mask: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in mask.enumerate_pixels() {
        if p[0] > 127 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    (x0 != u32::MAX).then_some((x0, y0, x1 + 1, y1 + 1))
}

/// Engrossa a máscara `r` pixels (o modelo vai melhor com uma folga em volta do objeto).
fn dilate(mask: &GrayImage, r: u32) -> GrayImage {
    if r == 0 {
        return mask.clone();
    }
    let (w, h) = mask.dimensions();
    let mut tmp = GrayImage::new(w, h);
    // separável: primeiro na horizontal, depois na vertical (máximo numa janela)
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            let v = (a..=b).map(|i| mask.get_pixel(i, y)[0]).max().unwrap_or(0);
            tmp.put_pixel(x, y, Luma([v]));
        }
    }
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            let v = (a..=b).map(|j| tmp.get_pixel(x, j)[0]).max().unwrap_or(0);
            out.put_pixel(x, y, Luma([v]));
        }
    }
    out
}

/// Apaga o que está marcado em `mask` (branco = apagar), do mesmo tamanho de `img`.
pub fn erase(img: &RgbImage, mask: &GrayImage, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<RgbImage, String> {
    let (w, h) = img.dimensions();
    if mask.dimensions() != (w, h) {
        return Err("a máscara precisa ter o tamanho da imagem.".into());
    }
    let (bx0, by0, bx1, by1) = bbox(mask).ok_or("pinte o que deve ser apagado.")?;
    // região com contexto: o dobro do objeto, pelo menos 256 px, quadrada quando der
    let (bw, bh) = (bx1 - bx0, by1 - by0);
    let side = (bw.max(bh) * 2).max(256).min(w.max(h));
    let cx = (bx0 + bx1) / 2;
    let cy = (by0 + by1) / 2;
    let rw = side.min(w);
    let rh = side.min(h);
    let x0 = cx.saturating_sub(rw / 2).min(w - rw);
    let y0 = cy.saturating_sub(rh / 2).min(h - rh);
    let crop = image::imageops::crop_imm(img, x0, y0, rw, rh).to_image();
    let grow = (rw.max(rh) / 100).max(3);
    let crop_mask = dilate(&image::imageops::crop_imm(mask, x0, y0, rw, rh).to_image(), grow);
    on_progress(0.1);
    let sm = image::imageops::resize(&crop, SIDE, SIDE, FilterType::Lanczos3);
    let mm = image::imageops::resize(&crop_mask, SIDE, SIDE, FilterType::Nearest);
    let n = (SIDE * SIDE) as usize;
    let mut data = vec![0f32; 3 * n];
    for (i, p) in sm.pixels().enumerate() {
        for c in 0..3 {
            data[c * n + i] = p[c] as f32 / 255.0;
        }
    }
    let mdata: Vec<f32> = mm.pixels().map(|p| if p[0] > 127 { 1.0 } else { 0.0 }).collect();
    if cancelled() {
        return Err("Cancelado.".into());
    }
    let session = model(&|p| on_progress(0.1 + p * 0.0), cancelled)?;
    let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
    let image_t = Tensor::from_array(([1usize, 3, SIDE as usize, SIDE as usize], data)).map_err(|e| e.to_string())?;
    let mask_t = Tensor::from_array(([1usize, 1, SIDE as usize, SIDE as usize], mdata)).map_err(|e| e.to_string())?;
    on_progress(0.3);
    let out = s.run(ort::inputs!["image" => image_t, "mask" => mask_t]).map_err(|e| format!("falha na IA: {e}"))?;
    let (_, v) = out[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    // a saída vem em 0..255 (algumas exportações dão 0..1)
    let scale = if v.iter().cloned().fold(0.0f32, f32::max) <= 1.5 { 255.0 } else { 1.0 };
    let mut res = RgbImage::new(SIDE, SIDE);
    for (i, p) in res.pixels_mut().enumerate() {
        for c in 0..3 {
            p[c] = (v[c * n + i] * scale).round().clamp(0.0, 255.0) as u8;
        }
    }
    drop(out);
    drop(s);
    on_progress(0.85);
    let back = image::imageops::resize(&res, rw, rh, FilterType::Lanczos3);
    // borda suave: a máscara engrossada e desfocada decide quanto de cada pixel vem do resultado
    let soft = image::imageops::blur(&crop_mask, (grow as f32 / 2.0).max(1.0));
    let mut outimg = img.clone();
    for y in 0..rh {
        for x in 0..rw {
            let a = soft.get_pixel(x, y)[0] as f32 / 255.0;
            if a <= 0.0 {
                continue;
            }
            let o = outimg.get_pixel_mut(x0 + x, y0 + y);
            let r = back.get_pixel(x, y);
            for c in 0..3 {
                o[c] = (o[c] as f32 * (1.0 - a) + r[c] as f32 * a).round() as u8;
            }
        }
    }
    on_progress(1.0);
    Ok(outimg)
}

/// Máscara a partir de retângulos (x, y, largura, altura) em pixels da imagem.
pub fn mask_from_rects(w: u32, h: u32, rects: &[(i64, i64, i64, i64)]) -> GrayImage {
    let mut m = GrayImage::new(w, h);
    for &(x, y, rw, rh) in rects {
        let (x0, y0) = (x.clamp(0, w as i64) as u32, y.clamp(0, h as i64) as u32);
        let (x1, y1) = ((x + rw).clamp(0, w as i64) as u32, (y + rh).clamp(0, h as i64) as u32);
        for yy in y0..y1 {
            for xx in x0..x1 {
                m.put_pixel(xx, yy, Luma([255]));
            }
        }
    }
    m
}

/// Apaga e salva ao lado (ou em `out_dir`) com o sufixo "_apagado", no mesmo formato.
pub fn erase_file(src: &Path, out_dir: Option<&Path>, mask: &GrayImage, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let img = image::open(src).map_err(|e| format!("não foi possível abrir a imagem: {e}"))?.to_rgb8();
    let mask = if mask.dimensions() == img.dimensions() { mask.clone() } else { image::imageops::resize(mask, img.width(), img.height(), FilterType::Nearest) };
    let out = erase(&img, &mask, on_progress, cancelled)?;
    let ext = match tools::ext_lower(src).as_str() {
        "jpg" | "jpeg" => "jpg",
        "webp" => "webp",
        _ => "png",
    };
    let dst = tools::out_path(src, out_dir, "_apagado", ext);
    out.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok((format!("{}×{}", out.width(), out.height()), dst))
}
