//! Restaurar foto antiga e colorir foto em preto e branco, com IA no próprio PC.
//!
//! Restaurar: cada rosto achado pelo YuNet é alinhado pelos 5 pontos (olhos, nariz, cantos da boca) no molde do FFHQ,
//! passa pelo GFPGAN 1.4 (512×512) e volta para a foto com a borda suavizada. O resto da foto pode ser ampliado 2× pelo
//! Real-ESRGAN antes.
//!
//! Colorir: o DDColor (versão leve, 256×256) recebe a foto em tons de cinza e devolve as cores (canais a e b do Lab);
//! as cores são ampliadas e juntadas com o brilho (L) da foto original, que fica na resolução cheia.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, Rgb, RgbImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

use crate::ai::{self, ModelSpec};
use crate::faces::FaceBox;
use crate::tools;

// i18n: fora
pub const GFPGAN: ModelSpec = ModelSpec {
    file: "GFPGANv1.4.onnx",
    url: "https://huggingface.co/Neus/GFPGANv1.4/resolve/944e127f1eae51e3471cc943e9df3be135e681a9/GFPGANv1.4.onnx",
    sha256: "cd7311b8d9e13cdb1e208b12363182da58c7bf45e26d1aa67bbeac4751aae92e",
    size: 340_256_686,
    label: "GFPGAN 1.4 (340 MB)",
};

/// O DDColor vem num zip (o .onnx e os pesos em `ddcolor.data`): é extraído na pasta `ddcolor`.
pub const DDCOLOR: ModelSpec = ModelSpec {
    file: "ddcolor-onnx-float.zip",
    url: "https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/ddcolor/releases/v0.66.0/ddcolor-onnx-float.zip",
    sha256: "512e788cab02db6796ede174af0db2302247130284a3a00eab9aafddafa48a49",
    size: 206_251_342,
    label: "DDColor (206 MB)",
};

// i18n: dentro

/// Molde do FFHQ: onde ficam olho direito, olho esquerdo, nariz e cantos da boca num rosto de 512×512.
const TEMPLATE: [(f32, f32); 5] = [(192.98138, 239.94708), (318.90277, 240.1936), (256.63416, 314.01935), (201.26117, 371.41043), (313.08905, 371.15118)];

fn gfpgan(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let path = ai::ensure_model(&GFPGAN, on_progress, cancelled)?;
    let s = ai::session(&path)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

fn ddcolor(on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<&'static Mutex<Session>, String> {
    static S: OnceLock<Mutex<Session>> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let zip = ai::ensure_model(&DDCOLOR, on_progress, cancelled)?;
    let dir = zip.with_file_name("ddcolor");
    let onnx = dir.join("ddcolor-onnx-float").join("ddcolor.onnx");
    if !onnx.is_file() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // i18n: fora
        let tar = std::env::var("WINDIR").map(|w| PathBuf::from(w).join("System32").join("tar.exe")).unwrap_or_else(|_| PathBuf::from("tar.exe"));
        // i18n: dentro
        let ok = tools::command(&tar).arg("-xf").arg(&zip).arg("-C").arg(&dir).status().map_err(|e| e.to_string())?;
        if !ok.success() || !onnx.is_file() {
            return Err("não foi possível extrair o modelo DDColor.".into());
        }
    }
    let s = ai::session(&onnx)?;
    Ok(S.get_or_init(|| Mutex::new(s)))
}

// ------------------------------------------------------------------ restaurar

/// Transformação de semelhança (escala, giro e deslocamento) que leva `from` em `to` pelos mínimos quadrados:
/// devolve (a, b, tx, ty) com x' = a·x − b·y + tx e y' = b·x + a·y + ty.
pub fn similarity(from: &[(f32, f32)], to: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    let n = from.len() as f32;
    let (mx, my) = from.iter().fold((0.0, 0.0), |(a, b), p| (a + p.0 / n, b + p.1 / n));
    let (ux, uy) = to.iter().fold((0.0, 0.0), |(a, b), p| (a + p.0 / n, b + p.1 / n));
    let (mut num_a, mut num_b, mut den) = (0.0, 0.0, 0.0);
    for (p, q) in from.iter().zip(to) {
        let (x, y) = (p.0 - mx, p.1 - my);
        let (u, v) = (q.0 - ux, q.1 - uy);
        num_a += x * u + y * v;
        num_b += x * v - y * u;
        den += x * x + y * y;
    }
    let (a, b) = (num_a / den.max(1e-6), num_b / den.max(1e-6));
    (a, b, ux - (a * mx - b * my), uy - (b * mx + a * my))
}

fn bilinear(img: &RgbImage, x: f32, y: f32) -> [f32; 3] {
    let (w, h) = img.dimensions();
    let x = x.clamp(0.0, w as f32 - 1.001);
    let y = y.clamp(0.0, h as f32 - 1.001);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |xx: u32, yy: u32| img.get_pixel(xx.min(w - 1), yy.min(h - 1)).0;
    let (a, b, c, d) = (p(x0, y0), p(x0 + 1, y0), p(x0, y0 + 1), p(x0 + 1, y0 + 1));
    let mut out = [0f32; 3];
    for i in 0..3 {
        out[i] = a[i] as f32 * (1.0 - fx) * (1.0 - fy) + b[i] as f32 * fx * (1.0 - fy) + c[i] as f32 * (1.0 - fx) * fy + d[i] as f32 * fx * fy;
    }
    out
}

/// Restaura um rosto: recorta alinhado, passa pelo GFPGAN e cola de volta com a borda suave.
fn restore_face(img: &mut RgbImage, f: &FaceBox, strength: f32, session: &Mutex<Session>) -> Result<(), String> {
    let Some(marks) = f.landmarks else { return Ok(()) };
    let (a, b, tx, ty) = similarity(&marks, &TEMPLATE);
    let det = a * a + b * b;
    // do molde para a foto (inversa da semelhança)
    let to_src = |u: f32, v: f32| {
        let (x, y) = (u - tx, v - ty);
        ((a * x + b * y) / det, (-b * x + a * y) / det)
    };
    let mut data = vec![0f32; 3 * 512 * 512];
    for v in 0..512u32 {
        for u in 0..512u32 {
            let (x, y) = to_src(u as f32, v as f32);
            let px = bilinear(img, x, y);
            for c in 0..3 {
                data[c * 512 * 512 + (v * 512 + u) as usize] = px[c] / 127.5 - 1.0;
            }
        }
    }
    let t = Tensor::from_array(([1usize, 3, 512, 512], data)).map_err(|e| e.to_string())?;
    let out: Vec<f32> = {
        let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
        let r = s.run(ort::inputs![t]).map_err(|e| format!("falha na IA: {e}"))?;
        r[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?.1.to_vec()
    };
    let face = RgbImage::from_fn(512, 512, |u, v| {
        let i = (v * 512 + u) as usize;
        let ch = |c: usize| ((out[c * 512 * 512 + i].clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8;
        Rgb([ch(0), ch(1), ch(2)])
    });
    // cola de volta: cada pixel da foto dentro do quadrado do rosto pega o pixel restaurado
    let corners = [(0.0, 0.0), (512.0, 0.0), (0.0, 512.0), (512.0, 512.0)].map(|(u, v)| to_src(u, v));
    let (w, h) = img.dimensions();
    let x0 = corners.iter().map(|c| c.0).fold(f32::MAX, f32::min).max(0.0) as u32;
    let x1 = (corners.iter().map(|c| c.0).fold(f32::MIN, f32::max).ceil() as u32).min(w);
    let y0 = corners.iter().map(|c| c.1).fold(f32::MAX, f32::min).max(0.0) as u32;
    let y1 = (corners.iter().map(|c| c.1).fold(f32::MIN, f32::max).ceil() as u32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let (u, v) = (a * x as f32 - b * y as f32 + tx, b * x as f32 + a * y as f32 + ty);
            if !(0.0..511.0).contains(&u) || !(0.0..511.0).contains(&v) {
                continue;
            }
            // borda suave: some aos poucos nos últimos 12% do quadrado
            let edge = (u.min(511.0 - u).min(v).min(511.0 - v) / (512.0 * 0.12)).clamp(0.0, 1.0);
            let k = edge * edge * (3.0 - 2.0 * edge) * strength;
            let r = bilinear(&face, u, v);
            let p = img.get_pixel_mut(x, y);
            for c in 0..3 {
                p[c] = (p[c] as f32 * (1.0 - k) + r[c] * k).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RestoreOpts {
    /// Quanto do rosto restaurado entra (0,5 a 1): menos deixa mais da foto original.
    pub strength: f32,
    /// Amplia a foto 2× com o Real-ESRGAN antes (bom para fotos pequenas e escaneadas).
    pub upscale: bool,
}

impl Default for RestoreOpts {
    fn default() -> Self {
        Self { strength: 0.85, upscale: true }
    }
}

/// Restaura os rostos (e, se pedido, amplia a foto). Devolve a foto e quantos rostos foram restaurados.
pub fn restore(img: &DynamicImage, o: RestoreOpts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(RgbImage, usize), String> {
    let session = gfpgan(&|p| on_progress(p * 0.3), cancelled)?;
    let mut rgb = if o.upscale && img.width().max(img.height()) < 2400 {
        crate::upscale::upscale(img, 2, &|p| on_progress(0.3 + p * 0.4), cancelled)?.to_rgb8()
    } else {
        img.to_rgb8()
    };
    let faces = crate::faces::detect(&rgb, 0.5)?;
    for (i, f) in faces.iter().enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        restore_face(&mut rgb, f, o.strength, session)?;
        on_progress(0.7 + 0.3 * (i + 1) as f32 / faces.len() as f32);
    }
    Ok((rgb, faces.len()))
}

pub fn restore_file(src: &Path, out_dir: Option<&Path>, o: RestoreOpts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let img = crate::imaging::load(src)?;
    let (out, n) = restore(&img, o, on_progress, cancelled)?;
    let dst = tools::out_path(src, out_dir, "_restaurada", "png");
    out.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    let faces = if n == 0 { crate::i18n::tr("nenhum rosto encontrado") } else { crate::i18n::tr(format!("{} rosto(s) restaurado(s)", n)) };
    Ok((format!("{} • {}×{}", faces, out.width(), out.height()), dst))
}

// ------------------------------------------------------------------ colorir

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB (0..1) → Lab (D65).
pub fn rgb_to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(to_linear);
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Lab (D65) → sRGB (0..1).
pub fn lab_to_rgb(lab: [f32; 3]) -> [f32; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let (fx, fz) = (fy + lab[1] / 500.0, fy - lab[2] / 200.0);
    let g = |t: f32| if t.powi(3) > 0.008_856 { t.powi(3) } else { (t - 16.0 / 116.0) / 7.787 };
    let (x, y, z) = (g(fx) * 0.950_47, g(fy), g(fz) * 1.088_83);
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let gg = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    [r, gg, b].map(|c| to_srgb(c.clamp(0.0, 1.0)).clamp(0.0, 1.0))
}

/// Colore a foto: as cores vêm do modelo em 256×256 e o brilho fica o da foto original.
pub fn colorize(img: &DynamicImage, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<RgbImage, String> {
    let session = ddcolor(&|p| on_progress(p * 0.7), cancelled)?;
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    let lab: Vec<[f32; 3]> = rgb.pixels().map(|p| rgb_to_lab(p.0.map(|c| c as f32 / 255.0))).collect();
    // a entrada é a foto em cinza (só o L, convertido de volta para RGB)
    let gray = RgbImage::from_fn(w, h, |x, y| {
        let l = lab[(y * w + x) as usize][0];
        let v = lab_to_rgb([l, 0.0, 0.0]);
        Rgb(v.map(|c| (c * 255.0).round() as u8))
    });
    let small = image::imageops::resize(&gray, 256, 256, FilterType::Triangle);
    let mut data = vec![0f32; 3 * 256 * 256];
    for (x, y, p) in small.enumerate_pixels() {
        for c in 0..3 {
            data[c * 256 * 256 + (y * 256 + x) as usize] = p[c] as f32 / 255.0;
        }
    }
    on_progress(0.8);
    let t = Tensor::from_array(([1usize, 3, 256, 256], data)).map_err(|e| e.to_string())?;
    let ab: Vec<f32> = {
        let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
        let r = s.run(ort::inputs!["image" => t]).map_err(|e| format!("falha na IA: {e}"))?;
        r[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?.1.to_vec()
    };
    if ab.len() != 2 * 256 * 256 {
        return Err("resposta inesperada do modelo.".into());
    }
    // amplia a e b para o tamanho da foto (o resize do image corta valores fora de 0..1: vai e volta dessa faixa)
    let plane = |c: usize| -> image::ImageBuffer<image::Luma<f32>, Vec<f32>> {
        let small = image::ImageBuffer::from_fn(256, 256, |x, y| image::Luma([(ab[c * 256 * 256 + (y * 256 + x) as usize] + 128.0) / 256.0]));
        let mut big = image::imageops::resize(&small, w, h, FilterType::CatmullRom);
        big.pixels_mut().for_each(|p| p[0] = p[0] * 256.0 - 128.0);
        big
    };
    let (pa, pb) = (plane(0), plane(1));
    on_progress(0.95);
    Ok(RgbImage::from_fn(w, h, |x, y| {
        let l = lab[(y * w + x) as usize][0];
        let v = lab_to_rgb([l, pa.get_pixel(x, y)[0], pb.get_pixel(x, y)[0]]);
        Rgb(v.map(|c| (c * 255.0).round() as u8))
    }))
}

pub fn colorize_file(src: &Path, out_dir: Option<&Path>, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let img = crate::imaging::load(src)?;
    let out = colorize(&img, on_progress, cancelled)?;
    let ext = if matches!(tools::ext_lower(src).as_str(), "jpg" | "jpeg") { "jpg" } else { "png" };
    let dst = tools::out_path(src, out_dir, "_colorida", ext);
    out.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok((format!("{}×{}", out.width(), out.height()), dst))
}
