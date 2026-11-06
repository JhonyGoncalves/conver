//! Reconhecer um objeto por um clique: SlimSAM (Segment Anything reduzido, ONNX, Apache-2.0). O codificador olha a
//! imagem uma vez (cerca de 2 s na CPU); depois cada clique só passa pelo decodificador, que é instantâneo. Usado em
//! "Remover fundo" para tirar uma parte da imagem deixando transparente, sem nada novo no lugar.

use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use image::{GrayImage, ImageBuffer, Luma, RgbImage, imageops::FilterType};
use ort::session::Session;
use ort::value::Tensor;

use crate::ai::{self, ModelSpec};
use crate::bgremove::Dot;

pub const ENCODER: ModelSpec = ModelSpec {
    file: "slimsam_encoder.onnx",
    url: "https://huggingface.co/Xenova/slimsam-50-uniform/resolve/3959c85fa93ab1c3dedd879c833aae1931961e19/onnx/vision_encoder.onnx",
    sha256: "4ffb40bdee86cbfb7ec96ecefc6f0cdeba3f1f160a8f091084cdb1cde3ac7fdd",
    size: 96_229_054,
    label: "SlimSAM (113 MB)",
};

pub const DECODER: ModelSpec = ModelSpec {
    file: "slimsam_decoder.onnx",
    url: "https://huggingface.co/Xenova/slimsam-50-uniform/resolve/3959c85fa93ab1c3dedd879c833aae1931961e19/onnx/prompt_encoder_mask_decoder.onnx",
    sha256: "f4514391764fbd56e08e119060d874ecd7d52994bfb1968af159e12d4943b5bb",
    size: 16_557_892,
    label: "SlimSAM (113 MB)",
};

/// Lado do quadrado que o modelo vê, e o da máscara que ele devolve.
const SIDE: u32 = 1024;
const OUT: u32 = 256;
/// Pontos usados de cada objeto: num traço longo, alguns espalhados por ele bastam.
const MAX_POINTS: usize = 6;

type Tensors = (Vec<i64>, Vec<f32>, Vec<i64>, Vec<f32>);

fn sessions() -> Result<&'static (Mutex<Session>, Mutex<Session>), String> {
    static S: OnceLock<(Mutex<Session>, Mutex<Session>)> = OnceLock::new();
    if let Some(s) = S.get() {
        return Ok(s);
    }
    let never = || false;
    let enc = ai::ensure_model(&ENCODER, &|_| {}, &never)?;
    let dec = ai::ensure_model(&DECODER, &|_| {}, &never)?;
    let pair = (Mutex::new(ai::session(&enc)?), Mutex::new(ai::session(&dec)?));
    Ok(S.get_or_init(|| pair))
}

fn scaled(w: u32, h: u32) -> (u32, u32, f32) {
    let k = SIDE as f32 / w.max(h) as f32;
    (((w as f32 * k).round() as u32).clamp(1, SIDE), ((h as f32 * k).round() as u32).clamp(1, SIDE), k)
}

/// O que o codificador viu da imagem. Fica guardado o da última imagem, para os cliques seguintes não repetirem.
fn embedding(rgb: &RgbImage) -> Result<Tensors, String> {
    static LAST: Mutex<Option<(u64, Tensors)>> = Mutex::new(None);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rgb.dimensions().hash(&mut h);
    rgb.as_raw().hash(&mut h);
    let key = h.finish();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, t)) = last.as_ref().filter(|(k, _)| *k == key) {
        return Ok(t.clone());
    }
    let (enc, _) = sessions()?;
    let (nw, nh, _) = scaled(rgb.width(), rgb.height());
    let small = image::imageops::resize(rgb, nw, nh, FilterType::Triangle);
    let (side, mean, std) = (SIDE as usize, [0.485f32, 0.456, 0.406], [0.229f32, 0.224, 0.225]);
    // a sobra do quadrado, embaixo e à direita, fica em zero
    let mut data = vec![0f32; 3 * side * side];
    for (x, y, p) in small.enumerate_pixels() {
        for c in 0..3 {
            data[c * side * side + y as usize * side + x as usize] = (p[c] as f32 / 255.0 - mean[c]) / std[c];
        }
    }
    let t = Tensor::from_array(([1usize, 3, side, side], data)).map_err(|e| e.to_string())?;
    let mut s = enc.lock().unwrap_or_else(|e| e.into_inner());
    let out = s.run(ort::inputs!["pixel_values" => t]).map_err(|e| format!("falha na IA: {e}"))?;
    let (sh, v) = out["image_embeddings"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    let (sh2, v2) = out["image_positional_embeddings"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    let tensors = (sh.to_vec(), v.to_vec(), sh2.to_vec(), v2.to_vec());
    *last = Some((key, tensors.clone()));
    Ok(tensors)
}

/// Máscara (branco = objeto) do objeto marcado pelos pontos, no tamanho da imagem.
fn one(rgb: &RgbImage, e: &Tensors, dots: &[Dot]) -> Result<GrayImage, String> {
    let (w, h) = rgb.dimensions();
    let (nw, nh, k) = scaled(w, h);
    let step = dots.len().div_ceil(MAX_POINTS).max(1);
    let picked: Vec<&Dot> = dots.iter().step_by(step).collect();
    let n = picked.len();
    let points: Vec<f32> = picked.iter().flat_map(|d| [d.x * w as f32 * k, d.y * h as f32 * k]).collect();
    let usize_shape = |s: &[i64]| s.iter().map(|v| *v as usize).collect::<Vec<_>>();
    let pts = Tensor::from_array(([1usize, 1, n, 2], points)).map_err(|e| e.to_string())?;
    let labels = Tensor::from_array(([1usize, 1, n], vec![1i64; n])).map_err(|e| e.to_string())?;
    let emb = Tensor::from_array((usize_shape(&e.0), e.1.clone())).map_err(|e| e.to_string())?;
    let pos = Tensor::from_array((usize_shape(&e.2), e.3.clone())).map_err(|e| e.to_string())?;
    let (_, dec) = sessions()?;
    let mut s = dec.lock().unwrap_or_else(|e| e.into_inner());
    let out = s
        .run(ort::inputs!["input_points" => pts, "input_labels" => labels, "image_embeddings" => emb, "image_positional_embeddings" => pos])
        .map_err(|e| format!("falha na IA: {e}"))?;
    let (_, scores) = out["iou_scores"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    let (_, masks) = out["pred_masks"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
    // o modelo dá três palpites (parte, objeto, conjunto): fica o que ele mais confia
    let best = scores.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).map_or(0, |(i, _)| i);
    let area = (OUT * OUT) as usize;
    let Some(m) = masks.get(best * area..(best + 1) * area) else {
        return Err("resposta inesperada do modelo.".into());
    };
    // a máscara cobre o quadrado inteiro: recorta a parte da imagem e leva para o tamanho original
    let crop = |v: u32| (v * OUT).div_ceil(SIDE).clamp(1, OUT);
    let (cw, ch) = (crop(nw), crop(nh));
    let logits: ImageBuffer<Luma<f32>, Vec<f32>> = ImageBuffer::from_fn(cw, ch, |x, y| Luma([m[(y * OUT + x) as usize]]));
    let big = image::imageops::resize(&logits, w, h, FilterType::Triangle);
    let found = GrayImage::from_fn(w, h, |x, y| Luma([if big.get_pixel(x, y)[0] > 0.0 { 255 } else { 0 }]));
    // a máscara do modelo é grosseira (256 px): uma folga para não sobrar um fio do objeto em volta do buraco
    Ok(crate::bgremove::morph(&found, (w.max(h) / OUT).max(2), true))
}

/// Reconhece cada objeto marcado (`objects`: os pontos de cada um) e junta tudo numa máscara, no tamanho da imagem.
pub fn objects(rgb: &RgbImage, objects: &[Vec<Dot>]) -> Result<GrayImage, String> {
    let e = embedding(rgb)?;
    let mut all = GrayImage::new(rgb.width(), rgb.height());
    for dots in objects.iter().filter(|d| !d.is_empty()) {
        all = crate::bgremove::merge(Some(all), one(rgb, &e, dots)?);
    }
    Ok(all)
}
