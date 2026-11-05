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

/// Ajustes finos do recorte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refine {
    /// Borda definida (`true`) ou suave, que guarda cabelo e pelos (`false`).
    pub hard: bool,
    /// Precisão de 0 a 100: mais alta corta mais (só o que a IA tem certeza que é o assunto); mais baixa mantém mais
    /// (sombras, fios soltos). 50 = o padrão.
    pub precision: f32,
    /// Engrossa (positivo) ou afina (negativo) o recorte, em pixels.
    pub grow: i32,
    /// Desfoque da borda, em pixels (0 = nenhum).
    pub feather: f32,
    /// Borda sem "escadinha" (um desfoque mínimo, só na transição).
    pub antialias: bool,
}

impl Default for Refine {
    fn default() -> Self {
        Self { hard: true, precision: 50.0, grow: 0, feather: 0.0, antialias: true }
    }
}

/// Máximo (`dilate`) ou mínimo dos vizinhos num quadrado de raio `r` (separável: linhas, depois colunas).
fn morph(mask: &GrayImage, r: u32, dilate: bool) -> GrayImage {
    let (w, h) = mask.dimensions();
    let pick = |a: u8, b: u8| if dilate { a.max(b) } else { a.min(b) };
    let mut tmp = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            let v = (a..=b).map(|i| mask.get_pixel(i, y)[0]).reduce(pick).unwrap_or(0);
            tmp.put_pixel(x, y, image::Luma([v]));
        }
    }
    let mut out = GrayImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
            let v = (a..=b).map(|j| tmp.get_pixel(x, j)[0]).reduce(pick).unwrap_or(0);
            out.put_pixel(x, y, image::Luma([v]));
        }
    }
    out
}

/// Aplica os ajustes à máscara crua do modelo. `keep` e `remove` (mesmo tamanho, branco = pintado) são os traços
/// do pincel: forçam o que fica e o que sai.
pub fn refine(raw: &GrayImage, r: &Refine, keep: Option<&GrayImage>, remove: Option<&GrayImage>) -> GrayImage {
    // precisão: o ponto de corte anda de 0,1 (mantém quase tudo) a 0,9 (só o que é certo); a transição é curta na
    // borda definida e larga na suave
    let c = 0.1 + 0.8 * (r.precision.clamp(0.0, 100.0) / 100.0);
    let half = if r.hard { 0.08 } else { 0.3 };
    let (lo, hi) = (c - half, c + half);
    let mut m = GrayImage::from_fn(raw.width(), raw.height(), |x, y| {
        let v = raw.get_pixel(x, y)[0] as f32 / 255.0;
        let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
        image::Luma([(t * t * (3.0 - 2.0 * t) * 255.0).round() as u8])
    });
    if r.grow != 0 {
        m = morph(&m, r.grow.unsigned_abs(), r.grow > 0);
    }
    for (mask, value) in [(keep, 255u8), (remove, 0u8)] {
        if let Some(k) = mask.filter(|k| k.dimensions() == m.dimensions()) {
            for (o, s) in m.pixels_mut().zip(k.pixels()) {
                if s[0] > 127 {
                    o[0] = value;
                }
            }
        }
    }
    let sigma = r.feather.max(0.0) / 2.0 + if r.antialias { 0.7 } else { 0.0 };
    if sigma > 0.05 {
        m = image::imageops::blur(&m, sigma);
    }
    m
}

/// Ponto de um traço do pincel em proporções da imagem (0 a 1), para valer em qualquer tamanho (prévia ou original).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub x: f32,
    pub y: f32,
    /// Raio em proporção da largura da imagem.
    pub r: f32,
}

/// Máscara (branco = pintado) dos traços, no tamanho `w`×`h`.
pub fn paint_mask(w: u32, h: u32, dots: &[Dot]) -> GrayImage {
    let mut m = GrayImage::new(w, h);
    for d in dots {
        let (cx, cy, r) = (d.x * w as f32, d.y * h as f32, (d.r * w as f32).max(1.0));
        let (x0, x1) = ((cx - r).floor().max(0.0) as u32, ((cx + r).ceil().max(0.0) as u32).min(w));
        let (y0, y1) = ((cy - r).floor().max(0.0) as u32, ((cy + r).ceil().max(0.0) as u32).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r * r {
                    m.put_pixel(x, y, image::Luma([255]));
                }
            }
        }
    }
    m
}

/// Pinceladas de uma imagem: o que fica, o que sai e os objetos que a IA deve apagar (preencher com o fundo).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Strokes {
    pub keep: Vec<Dot>,
    pub remove: Vec<Dot>,
    pub erase: Vec<Dot>,
}

/// Apaga os objetos pintados (LaMa preenche com o fundo). Sem traços de apagar, devolve a imagem como está.
pub fn erase_objects(img: &image::RgbImage, s: &Strokes, cancelled: &dyn Fn() -> bool) -> Result<image::RgbImage, String> {
    if s.erase.is_empty() {
        return Ok(img.clone());
    }
    crate::inpaint::erase(img, &paint_mask(img.width(), img.height(), &s.erase), &|_| {}, cancelled)
}

/// Máscara final de uma imagem (já sem os objetos apagados): a do modelo, com os ajustes e o pincel.
pub fn final_mask(rgb: &image::RgbImage, r: &Refine, s: &Strokes) -> Result<GrayImage, String> {
    let (w, h) = rgb.dimensions();
    let raw = mask(&DynamicImage::ImageRgb8(rgb.clone()))?;
    let keep = (!s.keep.is_empty()).then(|| paint_mask(w, h, &s.keep));
    let remove = (!s.remove.is_empty()).then(|| paint_mask(w, h, &s.remove));
    Ok(refine(&raw, r, keep.as_ref(), remove.as_ref()))
}
