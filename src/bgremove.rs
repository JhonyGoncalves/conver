//! Remover fundo de imagens com um modelo de IA (U²-Net "silueta", do projeto rembg), rodando no próprio PC
//! com o ONNX Runtime.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, GrayImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

const SIZE: usize = 320;
pub const MODEL_FILE: &str = "silueta.onnx";

static MODEL: OnceLock<Result<Mutex<Session>, String>> = OnceLock::new();

/// Onde o modelo pode estar: `modelos\` ao lado do Conver.exe (ou, durante o desenvolvimento, na pasta do projeto).
pub fn model_path() -> Option<PathBuf> {
    crate::ai::find_model(MODEL_FILE)
}

fn load() -> Result<Mutex<Session>, String> {
    let path = model_path().ok_or("o modelo de remoção de fundo não foi encontrado (pasta modelos).")?;
    Ok(Mutex::new(crate::ai::session(&path)?))
}

fn model() -> Result<&'static Mutex<Session>, String> {
    MODEL.get_or_init(load).as_ref().map_err(Clone::clone)
}

/// Máscara do primeiro plano (255 = manter), no tamanho da imagem.
pub fn mask(img: &DynamicImage) -> Result<GrayImage, String> {
    // carrega primeiro: o ONNX Runtime precisa saber o caminho da nossa DLL antes de qualquer outra chamada
    // (senão o Windows usaria a onnxruntime.dll antiga que vem no System32)
    let model = model()?;
    let rgb = img.to_rgb8();
    let small = image::imageops::resize(&rgb, SIZE as u32, SIZE as u32, FilterType::Triangle);
    // mesma normalização do rembg: divide pelo maior valor e aplica média/desvio do ImageNet
    let max = small.as_raw().iter().copied().max().unwrap_or(255).max(1) as f32;
    let mean = [0.485f32, 0.456, 0.406];
    let std = [0.229f32, 0.224, 0.225];
    let mut data = vec![0f32; 3 * SIZE * SIZE];
    for (x, y, p) in small.enumerate_pixels() {
        for c in 0..3 {
            data[c * SIZE * SIZE + y as usize * SIZE + x as usize] = (p[c] as f32 / max - mean[c]) / std[c];
        }
    }
    let input = Tensor::from_array(([1usize, 3, SIZE, SIZE], data)).map_err(|e| e.to_string())?;
    let values: Vec<f32> = {
        let mut session = model.lock().unwrap_or_else(|e| e.into_inner());
        let out = session.run(ort::inputs![input]).map_err(|e| e.to_string())?;
        let (_, v) = out[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        v.to_vec()
    };
    if values.len() != SIZE * SIZE {
        return Err("resposta inesperada do modelo.".into());
    }
    let (lo, hi) = values.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let range = (hi - lo).max(1e-6);
    let small_mask = GrayImage::from_fn(SIZE as u32, SIZE as u32, |x, y| {
        let v = (values[y as usize * SIZE + x as usize] - lo) / range;
        image::Luma([(v * 255.0).round().clamp(0.0, 255.0) as u8])
    });
    Ok(image::imageops::resize(&small_mask, img.width(), img.height(), FilterType::CatmullRom))
}

/// Endurece a máscara (menos "névoa" nas bordas), mantendo uma transição curta.
pub fn harden(mask: &mut GrayImage) {
    for p in mask.pixels_mut() {
        let v = p[0] as f32 / 255.0;
        let t = ((v - 0.35) / 0.3).clamp(0.0, 1.0);
        p[0] = (t * t * (3.0 - 2.0 * t) * 255.0) as u8;
    }
}
