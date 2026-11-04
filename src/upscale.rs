//! Ampliar imagens com IA (Real-ESRGAN "general x4v3", BSD-3), rodando no próprio PC com o ONNX Runtime.
//! A imagem é processada em blocos com sobreposição, para caber na memória e não deixar emendas.

use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, RgbImage, RgbaImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

pub const MODEL_FILE: &str = "realesr-general-x4v3.onnx";
const TILE: u32 = 192;
const PAD: u32 = 12;
/// Maior lado do resultado (acima disso a imagem ficaria pesada demais para abrir).
pub const MAX_SIDE: u32 = 16384;

static MODEL: OnceLock<Result<Mutex<Session>, String>> = OnceLock::new();

type Hooks = (&'static dyn Fn(f32), &'static (dyn Fn() -> bool + Sync));

thread_local! {
    /// Progresso e cancelamento de quem chamou (a operação passa por `imaging::process`, que não os recebe).
    static HOOKS: std::cell::Cell<Option<Hooks>> = const { std::cell::Cell::new(None) };
}

/// Executa `work` com progresso e cancelamento disponíveis para a ampliação feita lá dentro.
pub fn with_progress<R>(progress: &dyn Fn(f32), cancelled: &(dyn Fn() -> bool + Sync), work: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            HOOKS.with(|h| h.set(None));
        }
    }
    // SAFETY: as referências só ficam guardadas enquanto `work` roda nesta thread; `Reset` as apaga
    // antes de esta função voltar (inclusive se `work` entrar em pânico).
    let hooks: Hooks = unsafe { (std::mem::transmute::<&dyn Fn(f32), &'static dyn Fn(f32)>(progress), std::mem::transmute::<&(dyn Fn() -> bool + Sync), &'static (dyn Fn() -> bool + Sync)>(cancelled)) };
    HOOKS.with(|h| h.set(Some(hooks)));
    let _reset = Reset;
    work()
}

pub(crate) fn hook_progress(p: f32) {
    if let Some((f, _)) = HOOKS.with(|h| h.get()) {
        f(p);
    }
}

pub(crate) fn hook_cancelled() -> bool {
    HOOKS.with(|h| h.get()).map(|(_, c)| c()).unwrap_or(false)
}

fn model() -> Result<&'static Mutex<Session>, String> {
    MODEL
        .get_or_init(|| {
            let path = crate::ai::find_model(MODEL_FILE).ok_or("o modelo de ampliação não foi encontrado (pasta modelos).")?;
            Ok(Mutex::new(crate::ai::session(&path)?))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Amplia 4× um bloco RGB.
fn run_tile(session: &mut Session, tile: &RgbImage) -> Result<RgbImage, String> {
    let (w, h) = (tile.width() as usize, tile.height() as usize);
    let mut data = vec![0f32; 3 * w * h];
    for (x, y, p) in tile.enumerate_pixels() {
        for c in 0..3 {
            data[c * w * h + y as usize * w + x as usize] = p[c] as f32 / 255.0;
        }
    }
    let input = Tensor::from_array(([1usize, 3, h, w], data)).map_err(|e| e.to_string())?;
    let out = session.run(ort::inputs![input]).map_err(|e| e.to_string())?;
    let (shape, v) = out[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    let (oh, ow) = (shape[2] as usize, shape[3] as usize);
    if oh != h * 4 || ow != w * 4 {
        return Err("resposta inesperada do modelo.".into());
    }
    let plane = oh * ow;
    Ok(RgbImage::from_fn(ow as u32, oh as u32, |x, y| {
        let i = y as usize * ow + x as usize;
        let px = |c: usize| (v[c * plane + i] * 255.0).round().clamp(0.0, 255.0) as u8;
        image::Rgb([px(0), px(1), px(2)])
    }))
}

/// Amplia a imagem 2× ou 4×. A transparência (se houver) é ampliada à parte, sem IA.
pub fn upscale(img: &DynamicImage, scale: u32, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<DynamicImage, String> {
    let scale = if scale >= 4 { 4 } else { 2 };
    let (w, h) = (img.width(), img.height());
    if w.max(h) * scale > MAX_SIDE {
        return Err(format!("a imagem ficaria com mais de {MAX_SIDE} pixels de lado; diminua-a antes ou use 2×."));
    }
    let model = model()?;
    let rgb = img.to_rgb8();
    let mut big = RgbImage::new(w * 4, h * 4);
    let (cols, rows) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let total = (cols * rows) as f32;
    let mut session = model.lock().unwrap_or_else(|e| e.into_inner());
    for ty in 0..rows {
        for tx in 0..cols {
            if cancelled() {
                return Err("Cancelado.".into());
            }
            // bloco com uma margem de cada lado; só o miolo vai para o resultado
            let (x0, y0) = (tx * TILE, ty * TILE);
            let (x1, y1) = ((x0 + TILE).min(w), (y0 + TILE).min(h));
            let (px0, py0) = (x0.saturating_sub(PAD), y0.saturating_sub(PAD));
            let (px1, py1) = ((x1 + PAD).min(w), (y1 + PAD).min(h));
            let tile = image::imageops::crop_imm(&rgb, px0, py0, px1 - px0, py1 - py0).to_image();
            let out = run_tile(&mut session, &tile)?;
            let inner = image::imageops::crop_imm(&out, (x0 - px0) * 4, (y0 - py0) * 4, (x1 - x0) * 4, (y1 - y0) * 4).to_image();
            image::imageops::replace(&mut big, &inner, (x0 * 4) as i64, (y0 * 4) as i64);
            on_progress((ty * cols + tx + 1) as f32 / total);
        }
    }
    drop(session);
    let (nw, nh) = (w * scale, h * scale);
    let rgb_out = if scale == 4 { big } else { image::imageops::resize(&big, nw, nh, FilterType::Lanczos3) };
    if img.color().has_alpha() {
        let alpha = image::imageops::resize(&img.to_luma_alpha8(), nw, nh, FilterType::CatmullRom);
        let rgba = RgbaImage::from_fn(nw, nh, |x, y| {
            let p = rgb_out.get_pixel(x, y);
            image::Rgba([p[0], p[1], p[2], alpha.get_pixel(x, y)[1]])
        });
        Ok(DynamicImage::ImageRgba8(rgba))
    } else {
        Ok(DynamicImage::ImageRgb8(rgb_out))
    }
}
