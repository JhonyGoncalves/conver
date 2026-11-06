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

/// Como achar o que é fundo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Detect {
    /// Fundo de uma cor só (logos, ícones, desenhos) vai pela cor; o resto, pela IA.
    #[default]
    Auto,
    Ia,
    Cor,
    /// Não tira fundo nenhum: só o que foi marcado sai.
    Nada,
}

/// O que fazer com o que fica dentro do contorno do objeto e tem a cor do fundo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interior {
    /// Mantém na detecção por cor; na IA, respeita o que ela decidiu (o vão de um braço é fundo de verdade).
    #[default]
    Auto,
    Manter,
    Tirar,
}

/// Ajustes finos do recorte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Refine {
    pub detect: Detect,
    pub interior: Interior,
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
        Self { detect: Detect::Auto, interior: Interior::Auto, hard: true, precision: 50.0, grow: 0, feather: 0.0, antialias: true }
    }
}

/// Máximo (`dilate`) ou mínimo dos vizinhos num quadrado de raio `r` (separável: linhas, depois colunas).
pub fn morph(mask: &GrayImage, r: u32, dilate: bool) -> GrayImage {
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

/// Distância entre duas cores (maior diferença entre os canais).
fn color_dist(a: &[u8; 3], b: &[u8; 3]) -> u8 {
    (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
}

/// Cor mais típica da borda da imagem (mediana de cada canal) e a fração da borda que tem essa cor.
fn border_color(rgb: &image::RgbImage) -> ([u8; 3], f32) {
    let (w, h) = rgb.dimensions();
    let mut border = Vec::with_capacity(2 * (w + h) as usize);
    border.extend((0..w).flat_map(|x| [rgb.get_pixel(x, 0).0, rgb.get_pixel(x, h - 1).0]));
    border.extend((0..h).flat_map(|y| [rgb.get_pixel(0, y).0, rgb.get_pixel(w - 1, y).0]));
    let mut bg = [0u8; 3];
    for (c, v) in bg.iter_mut().enumerate() {
        let mut values: Vec<u8> = border.iter().map(|p| p[c]).collect();
        values.sort_unstable();
        *v = values[values.len() / 2];
    }
    let same = border.iter().filter(|p| color_dist(p, &bg) <= 10).count();
    (bg, same as f32 / border.len().max(1) as f32)
}

/// Cor do fundo, se a imagem tiver fundo de uma cor só: a borda quase toda igual (logos, ícones, desenhos, capturas).
/// Fotos, mesmo em estúdio, têm ruído ou degradê e não passam.
pub fn flat_background(rgb: &image::RgbImage) -> Option<[u8; 3]> {
    let (bg, share) = border_color(rgb);
    (share >= 0.97).then_some(bg)
}

/// Quanto cada pixel difere da cor do fundo, no formato da máscara da IA: 0 = igual ao fundo, 255 = bem diferente
/// (60 níveis ou mais). Assim a precisão e a borda funcionam do mesmo jeito nos dois modos.
fn color_mask(rgb: &image::RgbImage, bg: &[u8; 3]) -> GrayImage {
    GrayImage::from_fn(rgb.width(), rgb.height(), |x, y| {
        let d = color_dist(&rgb.get_pixel(x, y).0, bg) as u32;
        image::Luma([(d * 255 / 60).min(255) as u8])
    })
}

/// Fecha o que ficou dentro do contorno: só continua fundo o que alcança a borda da imagem sem atravessar o objeto.
/// Desenhos costumam ter vãos mínimos entre os traços, por onde o fundo entraria: a busca anda com o objeto engrossado
/// (uns 0,35% do lado menor) e depois volta essa mesma medida, para o lado de fora continuar exato.
fn fill_inside(m: &mut GrayImage) {
    let (w, h) = m.dimensions();
    let gap = ((w.min(h) as f32 * 0.0035).round() as u32).max(1);
    let wall = morph(m, gap, true);
    let mut outside = GrayImage::new(w, h);
    let mut todo = Vec::new();
    for x in 0..w {
        todo.extend([(x, 0), (x, h - 1)]);
    }
    for y in 0..h {
        todo.extend([(0, y), (w - 1, y)]);
    }
    while let Some((x, y)) = todo.pop() {
        if outside.get_pixel(x, y)[0] != 0 || wall.get_pixel(x, y)[0] >= 200 {
            continue;
        }
        outside.put_pixel(x, y, image::Luma([255]));
        if x > 0 {
            todo.push((x - 1, y));
        }
        if x + 1 < w {
            todo.push((x + 1, y));
        }
        if y > 0 {
            todo.push((x, y - 1));
        }
        if y + 1 < h {
            todo.push((x, y + 1));
        }
    }
    let outside = morph(&outside, gap, true);
    for (p, out) in m.pixels_mut().zip(outside.pixels()) {
        if out[0] == 0 {
            p[0] = 255;
        }
    }
}

/// Limpa a borda de um recorte por cor. Um pixel da borda é uma mistura do objeto com o fundo: a transparência vem de
/// quanto ele se afasta do fundo, comparado à cor cheia do objeto ali perto, e a cor do fundo sai da mistura. Sem isso
/// sobra um halo escuro em volta do objeto.
pub fn clean_edge(rgb: &mut image::RgbImage, mask: &mut GrayImage, bg: &[u8; 3]) {
    let (w, h) = rgb.dimensions();
    let dist = GrayImage::from_fn(w, h, |x, y| image::Luma([color_dist(&rgb.get_pixel(x, y).0, bg)]));
    let full = morph(&dist, 3, true);
    let near_hole = morph(mask, 2, false);
    for ((x, y, m), hole) in mask.enumerate_pixels_mut().zip(near_hole.pixels()) {
        let peak = full.get_pixel(x, y)[0] as f32;
        if m[0] == 0 || hole[0] != 0 || peak < 40.0 {
            continue;
        }
        let a = (dist.get_pixel(x, y)[0] as f32 / peak).min(m[0] as f32 / 255.0);
        m[0] = (a * 255.0).round() as u8;
        if a > 0.02 && a < 0.98 {
            let p = rgb.get_pixel_mut(x, y);
            for c in 0..3 {
                p[c] = ((p[c] as f32 - (1.0 - a) * bg[c] as f32) / a).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Máscara crua de uma imagem conforme o `detect`, com a cor do fundo se a detecção foi por cor.
pub fn raw_mask(rgb: &image::RgbImage, detect: Detect) -> Result<(GrayImage, Option<[u8; 3]>), String> {
    let flat = match detect {
        Detect::Nada => return Ok((GrayImage::from_pixel(rgb.width(), rgb.height(), image::Luma([255])), None)),
        Detect::Ia => None,
        Detect::Auto => flat_background(rgb),
        Detect::Cor => Some(border_color(rgb).0),
    };
    match flat {
        Some(bg) => Ok((color_mask(rgb, &bg), Some(bg))),
        None => Ok((mask(&DynamicImage::ImageRgb8(rgb.clone()))?, None)),
    }
}

/// Aplica os ajustes à máscara crua. `by_color` diz se ela veio da cor do fundo. `keep` e `remove` (mesmo tamanho, branco = pintado) são os traços
/// do pincel: forçam o que fica e o que sai.
pub fn refine(raw: &GrayImage, r: &Refine, by_color: bool, keep: Option<&GrayImage>, remove: Option<&GrayImage>) -> GrayImage {
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
    // o "manter" vem antes de fechar o interior (pintar sobre um vão do contorno fecha o objeto) e o "tirar" vem
    // depois (tem a última palavra)
    let paint = |m: &mut GrayImage, stroke: Option<&GrayImage>, value: u8| {
        if let Some(k) = stroke.filter(|k| k.dimensions() == m.dimensions()) {
            for (o, s) in m.pixels_mut().zip(k.pixels()) {
                if s[0] > 127 {
                    o[0] = value;
                }
            }
        }
    };
    paint(&mut m, keep, 255);
    if r.interior == Interior::Manter || (r.interior == Interior::Auto && by_color) {
        fill_inside(&mut m);
    }
    paint(&mut m, remove, 0);
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

/// Pinceladas de uma imagem: o que fica, o que sai, os objetos que a IA deve apagar (preencher com o fundo) e os que
/// ela deve reconhecer e remover, deixando transparente (`cut`: uma lista de pontos por objeto).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Strokes {
    pub keep: Vec<Dot>,
    pub remove: Vec<Dot>,
    pub erase: Vec<Dot>,
    pub cut: Vec<Vec<Dot>>,
}

/// Apaga os objetos pintados (LaMa preenche com o fundo). Sem traços de apagar, devolve a imagem como está.
pub fn erase_objects(img: &image::RgbImage, s: &Strokes, cancelled: &dyn Fn() -> bool) -> Result<image::RgbImage, String> {
    if s.erase.is_empty() {
        return Ok(img.clone());
    }
    crate::inpaint::erase(img, &paint_mask(img.width(), img.height(), &s.erase), &|_| {}, cancelled)
}

/// Recorte pronto de uma imagem (já sem os objetos apagados): a imagem, limpa da cor do fundo na borda, e a máscara.
pub fn cut(rgb: &image::RgbImage, r: &Refine, s: &Strokes) -> Result<(image::RgbImage, GrayImage), String> {
    let (w, h) = rgb.dimensions();
    let (raw, flat) = raw_mask(rgb, r.detect)?;
    let keep = (!s.keep.is_empty()).then(|| paint_mask(w, h, &s.keep));
    let mut remove = (!s.remove.is_empty()).then(|| paint_mask(w, h, &s.remove));
    if !s.cut.is_empty() {
        remove = Some(merge(remove, crate::segment::objects(rgb, &s.cut)?));
    }
    let mut m = refine(&raw, r, flat.is_some(), keep.as_ref(), remove.as_ref());
    let mut img = rgb.clone();
    if let Some(bg) = flat {
        clean_edge(&mut img, &mut m, &bg);
    }
    Ok((img, m))
}

/// Junta duas máscaras do mesmo tamanho.
pub fn merge(a: Option<GrayImage>, mut b: GrayImage) -> GrayImage {
    if let Some(a) = a {
        for (o, v) in b.pixels_mut().zip(a.pixels()) {
            o[0] = o[0].max(v[0]);
        }
    }
    b
}
