//! Operações com imagens: redimensionar, comprimir, diminuir qualidade e converter.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage, RgbaImage};
use rayon::prelude::*;

use crate::tools;

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "jfif", "webp", "bmp", "gif", "tif", "tiff", "ico"];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Fmt {
    Png,
    Jpg,
    Webp,
    Bmp,
    Gif,
    Tiff,
    Ico,
}

impl Fmt {
    pub const ALL: [Fmt; 7] = [Fmt::Png, Fmt::Jpg, Fmt::Webp, Fmt::Bmp, Fmt::Gif, Fmt::Tiff, Fmt::Ico];

    pub fn label(self) -> &'static str {
        match self {
            Fmt::Png => "PNG",
            Fmt::Jpg => "JPG",
            Fmt::Webp => "WEBP",
            Fmt::Bmp => "BMP",
            Fmt::Gif => "GIF",
            Fmt::Tiff => "TIFF",
            Fmt::Ico => "ICO",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Fmt::Png => "png",
            Fmt::Jpg => "jpg",
            Fmt::Webp => "webp",
            Fmt::Bmp => "bmp",
            Fmt::Gif => "gif",
            Fmt::Tiff => "tiff",
            Fmt::Ico => "ico",
        }
    }

    pub fn from_path(p: &Path) -> Fmt {
        match tools::ext_lower(p).as_str() {
            "jpg" | "jpeg" | "jfif" => Fmt::Jpg,
            "webp" => Fmt::Webp,
            "bmp" => Fmt::Bmp,
            "gif" => Fmt::Gif,
            "tif" | "tiff" => Fmt::Tiff,
            "ico" => Fmt::Ico,
            _ => Fmt::Png,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Level {
    Leve,
    Media,
    Forte,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Leve, Level::Media, Level::Forte];

    pub fn label(self) -> &'static str {
        match self {
            Level::Leve => "Leve",
            Level::Media => "Média",
            Level::Forte => "Forte",
        }
    }

    fn quality(self) -> u8 {
        match self {
            Level::Leve => 85,
            Level::Media => 70,
            Level::Forte => 50,
        }
    }

    fn png_colors(self) -> usize {
        match self {
            Level::Leve => 256,
            Level::Media => 128,
            Level::Forte => 64,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum ResizeMode {
    Percent(u32),
    Pixels { w: u32, h: u32, keep: bool },
}

/// Proporção para o recorte centralizado.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Aspect {
    Livre,
    Quadrado,
    R4x3,
    R3x2,
    R16x9,
    R3x4,
    R9x16,
}

impl Aspect {
    pub const ALL: [Aspect; 7] =
        [Aspect::Livre, Aspect::Quadrado, Aspect::R4x3, Aspect::R3x2, Aspect::R16x9, Aspect::R3x4, Aspect::R9x16];

    pub fn label(self) -> &'static str {
        match self {
            Aspect::Livre => "Sem recorte",
            Aspect::Quadrado => "1:1",
            Aspect::R4x3 => "4:3",
            Aspect::R3x2 => "3:2",
            Aspect::R16x9 => "16:9",
            Aspect::R3x4 => "3:4",
            Aspect::R9x16 => "9:16",
        }
    }

    fn ratio(self) -> Option<f64> {
        Some(match self {
            Aspect::Livre => return None,
            Aspect::Quadrado => 1.0,
            Aspect::R4x3 => 4.0 / 3.0,
            Aspect::R3x2 => 3.0 / 2.0,
            Aspect::R16x9 => 16.0 / 9.0,
            Aspect::R3x4 => 3.0 / 4.0,
            Aspect::R9x16 => 9.0 / 16.0,
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum ImageOp {
    Resize(ResizeMode),
    Compress(Level),
    Quality(u8),
    Convert(Fmt),
    /// Giro em graus (0, 90, 180, 270), espelhamentos e recorte centralizado.
    Transform { rotate: u16, flip_h: bool, flip_v: bool, aspect: Aspect },
    StripMeta,
    /// Remove o fundo com IA; `hard` deixa a borda mais definida.
    RemoveBg { bg: Background, hard: bool },
    /// Brilho, contraste e saturação de -100 a 100, mais um filtro pronto.
    Adjust { brightness: i16, contrast: i16, saturation: i16, filter: Filter },
}

/// O que fica no lugar do fundo removido.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Background {
    Transparente,
    Cor(u8, u8, u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Filter {
    Nenhum,
    PretoBranco,
    Sepia,
    Inverter,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::Nenhum, Filter::PretoBranco, Filter::Sepia, Filter::Inverter];

    pub fn label(self) -> &'static str {
        match self {
            Filter::Nenhum => "Nenhum",
            Filter::PretoBranco => "Preto e branco",
            Filter::Sepia => "Sépia",
            Filter::Inverter => "Inverter cores",
        }
    }
}

/// Aplica brilho, contraste, saturação e filtro, preservando a transparência.
fn adjust(img: &DynamicImage, brightness: i16, contrast: i16, saturation: i16, filter: Filter) -> DynamicImage {
    let mut rgba = img.to_rgba8();
    let b = brightness as f32 * 2.55;
    // contraste: fator de 0 (cinza) a ~3 (forte)
    let c = (contrast as f32 / 100.0 + 1.0).powf(if contrast > 0 { 1.6 } else { 1.0 });
    let s = saturation as f32 / 100.0 + 1.0;
    rgba.par_chunks_mut(4).for_each(|px| {
        let (mut r, mut g, mut bl) = (px[0] as f32, px[1] as f32, px[2] as f32);
        r += b;
        g += b;
        bl += b;
        r = (r - 128.0) * c + 128.0;
        g = (g - 128.0) * c + 128.0;
        bl = (bl - 128.0) * c + 128.0;
        let lum = 0.299 * r + 0.587 * g + 0.114 * bl;
        r = lum + (r - lum) * s;
        g = lum + (g - lum) * s;
        bl = lum + (bl - lum) * s;
        match filter {
            Filter::Nenhum => {}
            Filter::PretoBranco => {
                let l = 0.299 * r + 0.587 * g + 0.114 * bl;
                (r, g, bl) = (l, l, l);
            }
            Filter::Sepia => {
                (r, g, bl) = (
                    0.393 * r + 0.769 * g + 0.189 * bl,
                    0.349 * r + 0.686 * g + 0.168 * bl,
                    0.272 * r + 0.534 * g + 0.131 * bl,
                );
            }
            Filter::Inverter => {
                (r, g, bl) = (255.0 - r, 255.0 - g, 255.0 - bl);
            }
        }
        px[0] = r.clamp(0.0, 255.0) as u8;
        px[1] = g.clamp(0.0, 255.0) as u8;
        px[2] = bl.clamp(0.0, 255.0) as u8;
    });
    DynamicImage::ImageRgba8(rgba)
}

/// Resultado de uma operação, ainda em memória (usado tanto para salvar quanto para a prévia).
pub struct Output {
    pub bytes: Vec<u8>,
    pub fmt: Fmt,
    pub suffix: String,
    pub info: String,
    pub dims: (u32, u32),
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Abre a imagem respeitando a rotação gravada pela câmera (EXIF).
pub fn load(path: &Path) -> Result<DynamicImage, String> {
    load_oriented(path).map(|(img, _)| img)
}

/// Como `load`, dizendo também se a imagem precisou ser girada por causa do EXIF.
fn load_oriented(path: &Path) -> Result<(DynamicImage, bool), String> {
    let reader = ImageReader::open(path).map_err(err)?.with_guessed_format().map_err(err)?;
    let mut decoder = reader.into_decoder().map_err(err)?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(err)?;
    let rotated = orientation.is_some_and(|o| o != Orientation::NoTransforms);
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok((img, rotated))
}

pub fn thumbnail(img: &DynamicImage, max: u32) -> RgbaImage {
    if img.width() <= max && img.height() <= max {
        img.to_rgba8()
    } else {
        img.thumbnail(max, max).to_rgba8()
    }
}

fn has_alpha(img: &DynamicImage) -> bool {
    img.color().has_alpha() && img.to_rgba8().pixels().any(|p| p[3] < 255)
}

/// Remove a transparência pintando sobre fundo branco.
fn flatten(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (o, p) in out.pixels_mut().zip(rgba.pixels()) {
        let a = p[3] as u32;
        for c in 0..3 {
            o[c] = ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8;
        }
    }
    out
}

/// PNG com paleta reduzida (grande ganho de tamanho, mantém transparência).
fn quantized_png(img: &DynamicImage, colors: usize) -> Result<Vec<u8>, String> {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let nq = color_quant::NeuQuant::new(10, colors, rgba.as_raw());
    let indices: Vec<u8> = rgba.as_raw().par_chunks_exact(4).map(|p| nq.index_of(p) as u8).collect();
    let map = nq.color_map_rgba();
    let mut palette = Vec::with_capacity(colors * 3);
    let mut trns = Vec::with_capacity(colors);
    for c in map.chunks_exact(4) {
        palette.extend_from_slice(&c[..3]);
        trns.push(c[3]);
    }
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w, h);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Best);
        enc.set_filter(png::FilterType::NoFilter);
        enc.set_palette(palette);
        if trns.iter().any(|&a| a < 255) {
            enc.set_trns(trns);
        }
        let mut writer = enc.write_header().map_err(err)?;
        writer.write_image_data(&indices).map_err(err)?;
    }
    Ok(buf)
}

/// Codifica a imagem no formato pedido. `quality` vale para JPG e WEBP.
pub fn encode(img: &DynamicImage, fmt: Fmt, quality: Option<u8>) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    match fmt {
        Fmt::Png => {
            let enc = PngEncoder::new_with_quality(&mut buf, CompressionType::Best, PngFilter::Adaptive);
            let img = if img.color().has_alpha() {
                DynamicImage::ImageRgba8(img.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(img.to_rgb8())
            };
            img.write_with_encoder(enc).map_err(err)?;
        }
        Fmt::Jpg => {
            let enc = JpegEncoder::new_with_quality(&mut buf, quality.unwrap_or(92).clamp(1, 100));
            DynamicImage::ImageRgb8(flatten(img)).write_with_encoder(enc).map_err(err)?;
        }
        Fmt::Webp => {
            let rgba = img.to_rgba8();
            let enc = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
            let mem = enc.encode(quality.unwrap_or(90).clamp(1, 100) as f32);
            buf.extend_from_slice(&mem);
        }
        Fmt::Bmp => {
            DynamicImage::ImageRgb8(flatten(img))
                .write_to(&mut Cursor::new(&mut buf), ImageFormat::Bmp)
                .map_err(err)?;
        }
        Fmt::Gif => {
            DynamicImage::ImageRgba8(img.to_rgba8())
                .write_to(&mut Cursor::new(&mut buf), ImageFormat::Gif)
                .map_err(err)?;
        }
        Fmt::Tiff => {
            let img = if img.color().has_alpha() {
                DynamicImage::ImageRgba8(img.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(img.to_rgb8())
            };
            img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Tiff).map_err(err)?;
        }
        Fmt::Ico => {
            // ícones têm no máximo 256x256
            let img = if img.width() > 256 || img.height() > 256 {
                img.resize(256, 256, FilterType::Lanczos3)
            } else {
                img.clone()
            };
            DynamicImage::ImageRgba8(img.to_rgba8())
                .write_to(&mut Cursor::new(&mut buf), ImageFormat::Ico)
                .map_err(err)?;
        }
    }
    Ok(buf)
}

fn target_dims(w: u32, h: u32, mode: &ResizeMode) -> Result<(u32, u32), String> {
    let r = |v: f64| (v.round() as u32).max(1);
    match *mode {
        ResizeMode::Percent(p) => Ok((r(w as f64 * p as f64 / 100.0), r(h as f64 * p as f64 / 100.0))),
        ResizeMode::Pixels { w: tw, h: th, keep: true } => {
            let ratio = match (tw, th) {
                (0, 0) => return Err("Informe a largura e/ou a altura.".into()),
                (tw, 0) => tw as f64 / w as f64,
                (0, th) => th as f64 / h as f64,
                (tw, th) => (tw as f64 / w as f64).min(th as f64 / h as f64),
            };
            Ok((r(w as f64 * ratio), r(h as f64 * ratio)))
        }
        ResizeMode::Pixels { w: tw, h: th, keep: false } => {
            if tw == 0 || th == 0 {
                Err("Sem manter a proporção, informe largura E altura.".into())
            } else {
                Ok((tw, th))
            }
        }
    }
}

/// Executa a operação e devolve o arquivo resultante em memória.
pub fn process(path: &Path, op: &ImageOp) -> Result<Output, String> {
    let (img, rotated) = load_oriented(path)?;
    let src_fmt = Fmt::from_path(path);
    let (w, h) = (img.width(), img.height());
    let before = tools::file_size(path);

    match op {
        ImageOp::Resize(mode) => {
            let (nw, nh) = target_dims(w, h, mode)?;
            let out = img.resize_exact(nw, nh, FilterType::Lanczos3);
            Ok(Output {
                bytes: encode(&out, src_fmt, None)?,
                fmt: src_fmt,
                suffix: format!("_{nw}x{nh}"),
                info: format!("{w}x{h} → {nw}x{nh}"),
                dims: (nw, nh),
            })
        }
        ImageOp::Compress(level) => {
            let (fmt, mut bytes, note) = match src_fmt {
                Fmt::Png => (Fmt::Png, quantized_png(&img, level.png_colors())?, ""),
                Fmt::Jpg | Fmt::Webp => (src_fmt, encode(&img, src_fmt, Some(level.quality()))?, ""),
                // BMP e TIFF não têm compressão útil: viram PNG
                Fmt::Bmp | Fmt::Tiff => (Fmt::Png, quantized_png(&img, level.png_colors())?, " • salvo como PNG"),
                Fmt::Gif | Fmt::Ico => (src_fmt, encode(&img, src_fmt, None)?, ""),
            };
            if fmt == src_fmt && bytes.len() as u64 >= before {
                // não conseguiu reduzir: mantém o original
                bytes = std::fs::read(path).map_err(err)?;
            }
            let info = format!("{}{note}", tools::savings(before, bytes.len() as u64));
            Ok(Output { bytes, fmt, suffix: "_comprimida".into(), info, dims: (w, h) })
        }
        ImageOp::Quality(q) => {
            let fmt = match src_fmt {
                Fmt::Jpg | Fmt::Webp => src_fmt,
                _ if has_alpha(&img) => Fmt::Webp,
                _ => Fmt::Jpg,
            };
            let bytes = encode(&img, fmt, Some(*q))?;
            let info = format!("qualidade {q} • {}", tools::savings(before, bytes.len() as u64));
            Ok(Output { bytes, fmt, suffix: format!("_q{q}"), info, dims: (w, h) })
        }
        ImageOp::Convert(fmt) => {
            let bytes = encode(&img, *fmt, None)?;
            let suffix = if *fmt == src_fmt { "_convertida" } else { "" };
            let info = format!("{} → {} • {}", src_fmt.label(), fmt.label(), tools::human_size(bytes.len() as u64));
            Ok(Output { bytes, fmt: *fmt, suffix: suffix.into(), info, dims: (w, h) })
        }
        ImageOp::Transform { rotate, flip_h, flip_v, aspect } => {
            let mut out = match rotate {
                90 => img.rotate90(),
                180 => img.rotate180(),
                270 => img.rotate270(),
                _ => img,
            };
            if *flip_h {
                out = out.fliph();
            }
            if *flip_v {
                out = out.flipv();
            }
            if let Some(r) = aspect.ratio() {
                let (cw, ch) = (out.width() as f64, out.height() as f64);
                let (nw, nh) = if cw / ch > r { ((ch * r).round(), ch) } else { (cw, (cw / r).round()) };
                let (nw, nh) = (nw.max(1.0) as u32, nh.max(1.0) as u32);
                out = out.crop_imm((out.width() - nw) / 2, (out.height() - nh) / 2, nw, nh);
            }
            let dims = (out.width(), out.height());
            let mut parts = Vec::new();
            if *rotate != 0 {
                parts.push(format!("girada {rotate}°"));
            }
            if *flip_h {
                parts.push("espelhada ↔".into());
            }
            if *flip_v {
                parts.push("espelhada ↕".into());
            }
            if aspect.ratio().is_some() {
                parts.push(format!("recorte {}", aspect.label()));
            }
            let what = if parts.is_empty() { "sem alterações".to_string() } else { parts.join(", ") };
            Ok(Output {
                bytes: encode(&out, src_fmt, Some(95))?,
                fmt: src_fmt,
                suffix: "_editada".into(),
                info: format!("{what} • {}×{}", dims.0, dims.1),
                dims,
            })
        }
        ImageOp::StripMeta => {
            let original = std::fs::read(path).map_err(err)?;
            let stripped = match src_fmt {
                // a rotação da câmera fica no EXIF: se ela existir, aplica no pixel antes de apagar
                Fmt::Jpg if rotated => Some(encode(&img, Fmt::Jpg, Some(95))?),
                Fmt::Jpg => strip_jpeg(&original),
                Fmt::Png => strip_png(&original),
                Fmt::Webp if rotated => Some(encode(&img, Fmt::Webp, Some(95))?),
                Fmt::Webp => strip_webp(&original),
                // TIFF e GIF regravados não levam metadados; BMP e ICO não têm
                Fmt::Tiff | Fmt::Gif => Some(encode(&img, src_fmt, None)?),
                Fmt::Bmp | Fmt::Ico => None,
            };
            let bytes = stripped.unwrap_or_else(|| original.clone());
            let removed = original.len().saturating_sub(bytes.len());
            let info = if rotated {
                "metadados removidos (rotação da câmera aplicada na imagem)".to_string()
            } else if removed > 0 {
                format!("metadados removidos ({})", tools::human_size(removed as u64))
            } else {
                "nenhum metadado encontrado".to_string()
            };
            Ok(Output { bytes, fmt: src_fmt, suffix: "_limpa".into(), info, dims: (w, h) })
        }
        ImageOp::RemoveBg { bg, hard } => {
            let mut mask = crate::bgremove::mask(&img)?;
            if *hard {
                crate::bgremove::harden(&mut mask);
            }
            let mut rgba = img.to_rgba8();
            for (p, a) in rgba.pixels_mut().zip(mask.pixels()) {
                p[3] = (p[3] as u16 * a[0] as u16 / 255) as u8;
            }
            let kept = mask.pixels().filter(|a| a[0] > 127).count() as f64 / mask.len().max(1) as f64 * 100.0;
            let (out, fmt) = match bg {
                // transparência só existe em PNG e WEBP
                Background::Transparente => {
                    (DynamicImage::ImageRgba8(rgba), if src_fmt == Fmt::Webp { Fmt::Webp } else { Fmt::Png })
                }
                Background::Cor(r, g, b) => {
                    let mut flat = RgbImage::new(w, h);
                    for (o, p) in flat.pixels_mut().zip(rgba.pixels()) {
                        let a = p[3] as u32;
                        for (c, bgc) in [*r, *g, *b].into_iter().enumerate() {
                            o[c] = ((p[c] as u32 * a + bgc as u32 * (255 - a)) / 255) as u8;
                        }
                    }
                    let fmt = if matches!(src_fmt, Fmt::Ico | Fmt::Gif) { Fmt::Png } else { src_fmt };
                    (DynamicImage::ImageRgb8(flat), fmt)
                }
            };
            Ok(Output {
                bytes: encode(&out, fmt, Some(95))?,
                fmt,
                suffix: "_sem_fundo".into(),
                info: format!("fundo removido • {kept:.0}% da imagem mantida"),
                dims: (w, h),
            })
        }
        ImageOp::Adjust { brightness, contrast, saturation, filter } => {
            let has_a = img.color().has_alpha();
            let mut out = adjust(&img, *brightness, *contrast, *saturation, *filter);
            if !has_a {
                out = DynamicImage::ImageRgb8(out.to_rgb8());
            }
            let mut parts = Vec::new();
            for (name, v) in [("brilho", brightness), ("contraste", contrast), ("saturação", saturation)] {
                if *v != 0 {
                    parts.push(format!("{name} {v:+}"));
                }
            }
            if *filter != Filter::Nenhum {
                parts.push(filter.label().to_lowercase());
            }
            let what = if parts.is_empty() { "sem alterações".to_string() } else { parts.join(", ") };
            Ok(Output {
                bytes: encode(&out, src_fmt, Some(95))?,
                fmt: src_fmt,
                suffix: "_ajustada".into(),
                info: what,
                dims: (w, h),
            })
        }
    }
}

/// JPEG sem EXIF/XMP (APP1), IPTC (APP13), comentários e outros APPn privados.
/// Mantém JFIF (APP0), perfil de cor (APP2) e Adobe (APP14). Não recodifica a imagem.
fn strip_jpeg(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1; // preenchimento
            continue;
        }
        if marker == 0xDA {
            out.extend_from_slice(&b[i..]); // início dos dados da imagem
            return Some(out);
        }
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            out.extend_from_slice(&b[i..i + 2]);
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([*b.get(i + 2)?, *b.get(i + 3)?]) as usize;
        let end = (i + 2 + len).min(b.len());
        let drop = matches!(marker, 0xE1 | 0xE3..=0xED | 0xEF | 0xFE);
        if !drop {
            out.extend_from_slice(&b[i..end]);
        }
        i = end;
    }
    None
}

/// PNG sem textos (tEXt/zTXt/iTXt), EXIF (eXIf) e data (tIME).
fn strip_png(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 8 || &b[1..4] != b"PNG" {
        return None;
    }
    let mut out = b[..8].to_vec();
    let mut i = 8;
    while i + 8 <= b.len() {
        let len = u32::from_be_bytes(b[i..i + 4].try_into().ok()?) as usize;
        let end = (i + 12 + len).min(b.len());
        let kind = &b[i + 4..i + 8];
        if !matches!(kind, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME") {
            out.extend_from_slice(&b[i..end]);
        }
        i = end;
    }
    Some(out)
}

/// WEBP sem os blocos EXIF e XMP.
fn strip_webp(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WEBP" {
        return None;
    }
    let mut body = b"WEBP".to_vec();
    let mut i = 12;
    while i + 8 <= b.len() {
        let kind: [u8; 4] = b[i..i + 4].try_into().ok()?;
        let len = u32::from_le_bytes(b[i + 4..i + 8].try_into().ok()?) as usize;
        let end = (i + 8 + len + (len & 1)).min(b.len());
        match &kind {
            b"EXIF" | b"XMP " => {}
            b"VP8X" => {
                let mut chunk = b[i..end].to_vec();
                if chunk.len() > 8 {
                    chunk[8] &= !(0x08 | 0x04); // flags de EXIF e XMP
                }
                body.extend_from_slice(&chunk);
            }
            _ => body.extend_from_slice(&b[i..end]),
        }
        i = end;
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Some(out)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    Imagem,
    A4Retrato,
    A4Paisagem,
}

impl PageSize {
    pub const ALL: [PageSize; 3] = [PageSize::Imagem, PageSize::A4Retrato, PageSize::A4Paisagem];

    pub fn label(self) -> &'static str {
        match self {
            PageSize::Imagem => "Tamanho da imagem",
            PageSize::A4Retrato => "A4 retrato",
            PageSize::A4Paisagem => "A4 paisagem",
        }
    }
}

/// Junta as imagens num PDF (uma por página). Devolve o número de páginas.
pub fn images_to_pdf(
    files: &[std::path::PathBuf],
    dst: &Path,
    page: PageSize,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<usize, String> {
    use std::fmt::Write as _;
    let mut pdf: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    // objetos 1 = catálogo, 2 = páginas; cada imagem usa 3 objetos (página, imagem, conteúdo)
    let n = files.len();
    let mut objects: Vec<Vec<u8>> = vec![Vec::new(), Vec::new()];
    let mut kids = String::new();
    for (i, f) in files.iter().enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        let img = load(f).map_err(|e| format!("{}: {e}", tools::file_name(f)))?;
        let (w, h) = (img.width(), img.height());
        let jpeg = encode(&img, Fmt::Jpg, Some(90))?;
        let (pw, ph) = match page {
            // 96 dpi: 1 px = 0,75 pt
            PageSize::Imagem => (w as f64 * 0.75, h as f64 * 0.75),
            PageSize::A4Retrato => (595.0, 842.0),
            PageSize::A4Paisagem => (842.0, 595.0),
        };
        let margin = if page == PageSize::Imagem { 0.0 } else { 28.0 };
        let s = ((pw - 2.0 * margin) / w as f64).min((ph - 2.0 * margin) / h as f64);
        let (dw, dh) = (w as f64 * s, h as f64 * s);
        let (x, y) = ((pw - dw) / 2.0, (ph - dh) / 2.0);

        let page_id = objects.len() + 1;
        let (img_id, content_id) = (page_id + 1, page_id + 2);
        let _ = write!(kids, "{page_id} 0 R ");
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /XObject << /Im0 {img_id} 0 R >> >> /Contents {content_id} 0 R >>"
            )
            .into_bytes(),
        );
        let mut image_obj = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        image_obj.extend_from_slice(&jpeg);
        image_obj.extend_from_slice(b"\nendstream");
        objects.push(image_obj);
        let content = format!("q {dw:.2} 0 0 {dh:.2} {x:.2} {y:.2} cm /Im0 Do Q");
        objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).into_bytes());
        on_progress((i + 1) as f32 / n as f32);
    }
    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.trim_end()).into_bytes();

    for (i, obj) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(obj);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for o in &offsets {
        let _ = writeln!(table, "{o:010} 00000 n ");
    }
    let _ = write!(table, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
    pdf.extend_from_slice(table.as_bytes());
    std::fs::write(dst, pdf).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok(n)
}

/// Processa e grava ao lado do original (ou na pasta escolhida).
pub fn process_and_save(path: &Path, op: &ImageOp, out_dir: Option<&Path>) -> Result<(String, std::path::PathBuf), String> {
    let out = process(path, op)?;
    let dst = tools::out_path(path, out_dir, &out.suffix, out.fmt.ext());
    std::fs::write(&dst, &out.bytes).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok((out.info, dst))
}
