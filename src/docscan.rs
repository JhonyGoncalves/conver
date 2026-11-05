//! Escanear documento: foto de papel tirada com o celular vira documento reto e limpo. Acha a folha (a maior área
//! clara, pelo limiar de Otsu), pega os quatro cantos, endireita a perspectiva (homografia) e limpa: documento em
//! preto e branco (tira a sombra dividindo pelo fundo estimado), tons de cinza ou cor. Sem bibliotecas de visão.

use std::path::{Path, PathBuf};

use image::{GrayImage, Rgb, RgbImage};

use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Preto e branco, fundo branco (melhor para texto e OCR).
    Documento,
    Cinza,
    Cor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    Jpg,
    Png,
    Pdf,
}

#[derive(Clone, Copy, Debug)]
pub struct Opts {
    pub mode: Mode,
    /// `false` = não recorta (usa a foto inteira, só limpa).
    pub crop: bool,
}

type Pt = (f32, f32);

fn gray_of(img: &RgbImage) -> GrayImage {
    GrayImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        image::Luma([(0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8])
    })
}

/// Limiar de Otsu (o que melhor separa claro de escuro).
fn otsu(g: &GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for p in g.pixels() {
        hist[p[0] as usize] += 1;
    }
    let total = g.len() as f64;
    let sum: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let (mut sum_b, mut w_b, mut best, mut t) = (0.0, 0.0, 0.0, 127u8);
    for (i, &c) in hist.iter().enumerate() {
        w_b += c as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += i as f64 * c as f64;
        let (m_b, m_f) = (sum_b / w_b, (sum - sum_b) / w_f);
        let between = w_b * w_f * (m_b - m_f).powi(2);
        if between > best {
            best = between;
            t = i as u8;
        }
    }
    t
}

/// Os quatro cantos da folha (cima-esquerda, cima-direita, baixo-direita, baixo-esquerda), em pixels da imagem.
/// `None` quando não há uma área clara grande o bastante para ser uma folha.
pub fn detect(img: &RgbImage) -> Option<[Pt; 4]> {
    let (w, h) = img.dimensions();
    let scale = (400.0 / w.max(h) as f32).min(1.0);
    let (sw, sh) = (((w as f32 * scale) as u32).max(8), ((h as f32 * scale) as u32).max(8));
    let small = image::imageops::resize(img, sw, sh, image::imageops::FilterType::Triangle);
    let g = gray_of(&small);
    let t = otsu(&g);
    let bright = |x: u32, y: u32| g.get_pixel(x, y)[0] > t;
    // maior região clara ligada (a folha costuma ser mais clara que a mesa)
    let mut seen = vec![false; (sw * sh) as usize];
    let mut best: Vec<(u32, u32)> = Vec::new();
    for sy in 0..sh {
        for sx in 0..sw {
            let i = (sy * sw + sx) as usize;
            if seen[i] || !bright(sx, sy) {
                continue;
            }
            let mut comp = Vec::new();
            let mut stack = vec![(sx, sy)];
            seen[i] = true;
            while let Some((x, y)) = stack.pop() {
                comp.push((x, y));
                let mut push = |nx: u32, ny: u32| {
                    let j = (ny * sw + nx) as usize;
                    if !seen[j] && bright(nx, ny) {
                        seen[j] = true;
                        stack.push((nx, ny));
                    }
                };
                if x > 0 {
                    push(x - 1, y);
                }
                if y > 0 {
                    push(x, y - 1);
                }
                if x + 1 < sw {
                    push(x + 1, y);
                }
                if y + 1 < sh {
                    push(x, y + 1);
                }
            }
            if comp.len() > best.len() {
                best = comp;
            }
        }
    }
    if (best.len() as f32) < 0.12 * (sw * sh) as f32 {
        return None;
    }
    // cantos: os extremos de x+y e x−y
    let key = |f: &dyn Fn(f32, f32) -> f32| best.iter().map(|&(x, y)| (x as f32, y as f32)).max_by(|a, b| f(a.0, a.1).total_cmp(&f(b.0, b.1))).unwrap();
    let tl = key(&|x, y| -(x + y));
    let br = key(&|x, y| x + y);
    let tr = key(&|x, y| x - y);
    let bl = key(&|x, y| y - x);
    let up = |p: Pt| ((p.0 + 0.5) / scale, (p.1 + 0.5) / scale);
    let quad = [up(tl), up(tr), up(br), up(bl)];
    // a folha ocupa quase a foto inteira e já está reta: não há o que recortar
    let area = polygon_area(&quad);
    (area < 0.97 * (w * h) as f32).then_some(quad)
}

fn polygon_area(q: &[Pt; 4]) -> f32 {
    let mut a = 0.0;
    for i in 0..4 {
        let (p, n) = (q[i], q[(i + 1) % 4]);
        a += p.0 * n.1 - n.0 * p.1;
    }
    a.abs() / 2.0
}

fn dist(a: Pt, b: Pt) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Homografia que leva os cantos do retângulo `w`×`h` aos pontos `q` (resolve o sistema 8×8).
fn homography(w: f32, h: f32, q: &[Pt; 4]) -> [f64; 9] {
    let src = [(0.0, 0.0), (w as f64, 0.0), (w as f64, h as f64), (0.0, h as f64)];
    let mut m = [[0f64; 9]; 8];
    for i in 0..4 {
        let (x, y) = src[i];
        let (u, v) = (q[i].0 as f64, q[i].1 as f64);
        m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
        m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
    }
    // eliminação de Gauss com pivô
    for c in 0..8 {
        let piv = (c..8).max_by(|&a, &b| m[a][c].abs().total_cmp(&m[b][c].abs())).unwrap();
        m.swap(c, piv);
        let d = m[c][c];
        if d.abs() < 1e-12 {
            continue;
        }
        for k in c..9 {
            m[c][k] /= d;
        }
        for r in 0..8 {
            if r != c {
                let f = m[r][c];
                for k in c..9 {
                    m[r][k] -= f * m[c][k];
                }
            }
        }
    }
    [m[0][8], m[1][8], m[2][8], m[3][8], m[4][8], m[5][8], m[6][8], m[7][8], 1.0]
}

/// Endireita a folha: a área entre os cantos vira um retângulo do tamanho dos lados.
pub fn warp(img: &RgbImage, q: &[Pt; 4]) -> RgbImage {
    let ow = dist(q[0], q[1]).max(dist(q[3], q[2])).round().max(8.0);
    let oh = dist(q[0], q[3]).max(dist(q[1], q[2])).round().max(8.0);
    let hm = homography(ow, oh, q);
    let (w, h) = (img.width() as f64, img.height() as f64);
    let sample = |x: f64, y: f64| -> Rgb<u8> {
        let (x, y) = (x.clamp(0.0, w - 1.001), y.clamp(0.0, h - 1.001));
        let (x0, y0) = (x.floor() as u32, y.floor() as u32);
        let (fx, fy) = (x - x0 as f64, y - y0 as f64);
        let p = |dx, dy| img.get_pixel(x0 + dx, y0 + dy);
        let mut out = [0u8; 3];
        for (c, o) in out.iter_mut().enumerate() {
            let v = p(0, 0)[c] as f64 * (1.0 - fx) * (1.0 - fy) + p(1, 0)[c] as f64 * fx * (1.0 - fy) + p(0, 1)[c] as f64 * (1.0 - fx) * fy + p(1, 1)[c] as f64 * fx * fy;
            *o = v.round() as u8;
        }
        Rgb(out)
    };
    RgbImage::from_fn(ow as u32, oh as u32, |x, y| {
        let (x, y) = (x as f64 + 0.5, y as f64 + 0.5);
        let d = hm[6] * x + hm[7] * y + hm[8];
        sample((hm[0] * x + hm[1] * y + hm[2]) / d, (hm[3] * x + hm[4] * y + hm[5]) / d)
    })
}

/// Média de cada pixel numa janela quadrada de raio `r` (imagem integral).
fn box_mean(g: &GrayImage, r: u32) -> Vec<f32> {
    let (w, h) = (g.width() as usize, g.height() as usize);
    let mut ii = vec![0u64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0u64;
        for x in 0..w {
            row += g.get_pixel(x as u32, y as u32)[0] as u64;
            ii[(y + 1) * (w + 1) + x + 1] = ii[y * (w + 1) + x + 1] + row;
        }
    }
    let r = r as usize;
    let mut out = vec![0f32; w * h];
    for y in 0..h {
        let (y0, y1) = (y.saturating_sub(r), (y + r + 1).min(h));
        for x in 0..w {
            let (x0, x1) = (x.saturating_sub(r), (x + r + 1).min(w));
            let s = ii[y1 * (w + 1) + x1] + ii[y0 * (w + 1) + x0] - ii[y0 * (w + 1) + x1] - ii[y1 * (w + 1) + x0];
            out[y * w + x] = s as f32 / ((x1 - x0) * (y1 - y0)) as f32;
        }
    }
    out
}

/// Limpa a folha já endireitada.
pub fn enhance(img: &RgbImage, mode: Mode) -> RgbImage {
    match mode {
        Mode::Documento | Mode::Cinza => {
            let g = gray_of(img);
            // fundo estimado (a média de uma janela grande): dividir por ele tira sombras e papel amarelado
            let r = (g.width().max(g.height()) / 24).max(8);
            let bg = box_mean(&g, r);
            let w = g.width() as usize;
            RgbImage::from_fn(g.width(), g.height(), |x, y| {
                let v = g.get_pixel(x, y)[0] as f32;
                let b = bg[y as usize * w + x as usize].max(1.0);
                let ratio = (v / b).min(1.2);
                let out = if mode == Mode::Documento {
                    if ratio < 0.82 { 0.0 } else { 255.0 }
                } else {
                    // cinza: fundo branco e o texto mais escuro
                    (((ratio - 0.55) / 0.40).clamp(0.0, 1.0) * 255.0).powf(1.0)
                };
                let o = out as u8;
                Rgb([o, o, o])
            })
        }
        Mode::Cor => {
            // estica cada canal entre o 1% mais escuro e o 99% mais claro (papel branco, cores vivas)
            let mut lo = [0u8; 3];
            let mut hi = [255u8; 3];
            for c in 0..3 {
                let mut hist = [0u64; 256];
                for p in img.pixels() {
                    hist[p[c] as usize] += 1;
                }
                let total = img.pixels().len() as u64;
                let (mut acc, mut l, mut h) = (0u64, 0usize, 255usize);
                for (i, &n) in hist.iter().enumerate() {
                    acc += n;
                    if acc * 100 >= total {
                        l = i;
                        break;
                    }
                }
                acc = 0;
                for (i, &n) in hist.iter().enumerate().rev() {
                    acc += n;
                    if acc * 100 >= total * 4 {
                        h = i;
                        break;
                    }
                }
                lo[c] = l as u8;
                hi[c] = (h.max(l + 10)) as u8;
            }
            RgbImage::from_fn(img.width(), img.height(), |x, y| {
                let p = img.get_pixel(x, y);
                let mut o = [0u8; 3];
                for c in 0..3 {
                    let v = (p[c] as f32 - lo[c] as f32) / (hi[c] as f32 - lo[c] as f32);
                    o[c] = (v.clamp(0.0, 1.0) * 255.0) as u8;
                }
                Rgb(o)
            })
        }
    }
}

/// Uma foto escaneada (imagem em memória): recorta (se achar a folha), endireita e limpa.
/// Devolve a imagem e se a folha foi achada.
pub fn scan(img: &RgbImage, o: &Opts) -> (RgbImage, bool) {
    let quad = if o.crop { detect(img) } else { None };
    let flat = match &quad {
        Some(q) => warp(img, q),
        None => img.clone(),
    };
    (enhance(&flat, o.mode), quad.is_some())
}

fn load(src: &Path) -> Result<RgbImage, String> {
    Ok(crate::imaging::load(src)?.to_rgb8())
}

/// Escaneia um arquivo e salva como JPG ou PNG ao lado (ou em `out_dir`), com o sufixo `_escaneado`.
pub fn process(src: &Path, out_dir: Option<&Path>, o: &Opts, out: Out) -> Result<(String, PathBuf), String> {
    let (img, found) = scan(&load(src)?, o);
    let ext = if out == Out::Png { "png" } else { "jpg" };
    let dir = out_dir.map(Path::to_path_buf).or_else(|| src.parent().map(Path::to_path_buf)).unwrap_or_default();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dst = tools::unique_path(dir.join(format!("{}_escaneado.{ext}", src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default())));
    image::DynamicImage::ImageRgb8(img.clone()).save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    let size = format!("{}×{}", img.width(), img.height());
    let info = if found || !o.crop { size } else { format!("{size} • {}", "folha não encontrada (foto inteira)") };
    Ok((info, dst))
}

/// Escaneia vários arquivos e junta tudo num PDF só (uma página por foto, na ordem).
pub fn to_pdf(files: &[PathBuf], dst: &Path, o: &Opts, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<usize, String> {
    let tmp = std::env::temp_dir().join(format!("conver_scan_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut pages = Vec::new();
    let r = (|| -> Result<usize, String> {
        for (i, f) in files.iter().enumerate() {
            if cancelled() {
                return Err("Cancelado.".into());
            }
            let (img, _) = scan(&load(f)?, o);
            let page = tmp.join(format!("{i:04}.jpg"));
            image::DynamicImage::ImageRgb8(img).save(&page).map_err(|e| e.to_string())?;
            pages.push(page);
            on_progress((i + 1) as f32 / files.len() as f32 * 0.8);
        }
        crate::imaging::images_to_pdf(&pages, dst, crate::imaging::PageSize::Imagem, &|p| on_progress(0.8 + p * 0.2), cancelled)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    r
}
