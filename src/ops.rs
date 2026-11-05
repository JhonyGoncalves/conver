//! Todas as ferramentas do Conver como operações com argumentos JSON e um esquema (JSON Schema).
//! É o núcleo usado pela linha de comando (conver-cli), pelo servidor MCP (agentes de IA) e pelas
//! predefinições. Os nomes e as descrições ficam em inglês, que é o que os agentes entendem melhor.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::imaging::{self, Aspect, Background, Filter, Fmt, ImageOp, Level, PageSize, ResizeMode};
use crate::media::{self, ACodec, AFmt};
use crate::media_extra::{self, SubSize, VAspect};
use crate::video::{self, Codec, CompressMode, CompressOpts, Res, VFmt, VLevel, VQuality};
use crate::{download, history, info, ocr, pdf, pdf_extra, qr, screenrec, separate, tools, transcribe};

/// Retornos de progresso, cancelamento e mensagens para quem chamou.
pub struct Ctx<'a> {
    pub progress: &'a (dyn Fn(f32) + Sync),
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    pub log: &'a (dyn Fn(&str) + Sync),
}

pub struct Tool {
    pub name: &'static str,
    /// Nome em português (tela de predefinições e ajuda da linha de comando).
    pub title: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub run: fn(&Args, &Ctx) -> Result<Value, String>,
}

// ================================================================ argumentos

pub struct Args<'a>(pub &'a Map<String, Value>);

impl Args<'_> {
    fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k).filter(|v| !v.is_null())
    }

    pub fn str(&self, k: &str) -> Option<String> {
        self.get(k).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    }

    fn str_or(&self, k: &str, d: &str) -> String {
        self.str(k).unwrap_or_else(|| d.to_string())
    }

    fn num(&self, k: &str) -> Result<Option<f64>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::Number(n)) => Ok(n.as_f64()),
            Some(Value::String(s)) => {
                let s = s.trim().replace(',', ".");
                s.parse::<f64>().map(Some).map_err(|_| format!("\"{k}\" deve ser um número (recebido: {s})."))
            }
            Some(v) => Err(format!("\"{k}\" deve ser um número (recebido: {v}).")),
        }
    }

    fn num_or(&self, k: &str, d: f64) -> Result<f64, String> {
        Ok(self.num(k)?.unwrap_or(d))
    }

    fn bool_or(&self, k: &str, d: bool) -> Result<bool, String> {
        match self.get(k) {
            None => Ok(d),
            Some(Value::Bool(b)) => Ok(*b),
            Some(Value::String(s)) => match s.trim().to_lowercase().as_str() {
                "true" | "1" | "yes" | "sim" | "on" => Ok(true),
                "false" | "0" | "no" | "nao" | "não" | "off" => Ok(false),
                _ => Err(format!("\"{k}\" deve ser true ou false.")),
            },
            Some(Value::Number(n)) => Ok(n.as_f64().unwrap_or(0.0) != 0.0),
            Some(v) => Err(format!("\"{k}\" deve ser true ou false (recebido: {v}).")),
        }
    }

    /// Valor de uma lista fechada (sem diferenciar maiúsculas).
    fn choice<T: Copy>(&self, k: &str, options: &[(&str, T)], d: T) -> Result<T, String> {
        let Some(v) = self.str(k) else { return Ok(d) };
        let v = v.trim().to_lowercase();
        options.iter().find(|(n, _)| *n == v).map(|(_, t)| *t).ok_or_else(|| {
            let names: Vec<&str> = options.iter().map(|(n, _)| *n).collect();
            format!("\"{k}\" inválido: \"{v}\". Use um de: {}.", names.join(", "))
        })
    }

    fn strings(&self, k: &str) -> Vec<String> {
        match self.get(k) {
            Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    /// "files" (lista) ou "file" (um só). Pastas viram os arquivos de dentro.
    pub fn files(&self) -> Result<Vec<PathBuf>, String> {
        let mut raw = self.strings("files");
        raw.extend(self.strings("file"));
        if raw.is_empty() {
            return Err("informe os arquivos em \"files\".".into());
        }
        let mut out = Vec::new();
        for f in raw {
            let p = PathBuf::from(f.trim());
            if p.is_dir() {
                let mut inner: Vec<PathBuf> = std::fs::read_dir(&p)
                    .map_err(|e| format!("{}: {e}", p.display()))?
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_file())
                    .collect();
                inner.sort();
                out.extend(inner);
            } else if p.is_file() {
                out.push(p);
            } else {
                return Err(format!("arquivo não encontrado: {}", p.display()));
            }
        }
        Ok(out)
    }

    fn file(&self, k: &str) -> Result<PathBuf, String> {
        let p = PathBuf::from(self.str(k).ok_or_else(|| format!("informe \"{k}\"."))?);
        if p.is_file() { Ok(p) } else { Err(format!("arquivo não encontrado: {}", p.display())) }
    }

    fn out_dir(&self) -> Result<Option<PathBuf>, String> {
        match self.str("out_dir").filter(|s| !s.trim().is_empty()) {
            None => Ok(None),
            Some(d) => {
                let p = PathBuf::from(d);
                std::fs::create_dir_all(&p).map_err(|e| format!("não foi possível criar a pasta {}: {e}", p.display()))?;
                Ok(Some(p))
            }
        }
    }
}

fn filter_ext(files: Vec<PathBuf>, exts: &[&str]) -> Result<Vec<PathBuf>, String> {
    let n = files.len();
    let ok: Vec<PathBuf> = files.into_iter().filter(|f| exts.contains(&tools::ext_lower(f).as_str())).collect();
    if ok.is_empty() {
        return Err(format!("nenhum dos {n} arquivo(s) tem um formato aceito ({}).", exts.join(", ")));
    }
    Ok(ok)
}

fn images(a: &Args) -> Result<Vec<PathBuf>, String> {
    filter_ext(a.files()?, imaging::IMAGE_EXTS)
}

const MEDIA_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "ts", "gif", "mp3", "wav", "flac", "ogg",
    "oga", "m4a", "aac", "opus", "wma", "aiff", "aif", "amr",
];

fn media_files(a: &Args) -> Result<Vec<PathBuf>, String> {
    filter_ext(a.files()?, MEDIA_EXTS)
}

fn pdfs(a: &Args) -> Result<Vec<PathBuf>, String> {
    filter_ext(a.files()?, &["pdf"])
}

// ================================================================ resultados

fn result_item(input: &Path, r: &Result<(String, PathBuf), String>) -> Value {
    match r {
        Ok((info, out)) => json!({ "input": input.display().to_string(), "output": out.display().to_string(), "info": info }),
        Err(e) => json!({ "input": input.display().to_string(), "error": e }),
    }
}

/// Aplica `each` a cada arquivo, registrando no histórico, e devolve a lista de resultados.
fn batch(
    tool: &str,
    files: &[PathBuf],
    ctx: &Ctx,
    mut each: impl FnMut(&Path, &dyn Fn(f32)) -> Result<(String, PathBuf), String>,
) -> Result<Value, String> {
    let n = files.len();
    let mut items = Vec::new();
    let (mut ok, mut failed) = (0, 0);
    for (i, f) in files.iter().enumerate() {
        if (ctx.cancelled)() {
            return Err("Cancelado.".into());
        }
        (ctx.log)(&format!("[{}/{n}] {}", i + 1, tools::file_name(f)));
        let progress = |p: f32| (ctx.progress)((i as f32 + p.clamp(0.0, 1.0)) / n as f32);
        let r = each(f, &progress);
        match &r {
            Ok((info, out)) => {
                ok += 1;
                history::record(tool, &tools::file_name(f), out, info);
                (ctx.log)(&format!("✔ {} → {}   {info}", tools::file_name(f), out.display()));
            }
            Err(e) => {
                failed += 1;
                (ctx.log)(&format!("✖ {}: {e}", tools::file_name(f)));
            }
        }
        items.push(result_item(f, &r));
    }
    (ctx.progress)(1.0);
    if ok == 0 && failed > 0 {
        let first = items.first().and_then(|v| v["error"].as_str()).unwrap_or("falhou").to_string();
        return Err(if n == 1 { first } else { format!("todos os {n} arquivos falharam; o primeiro erro: {first}") });
    }
    Ok(json!({ "ok": ok, "failed": failed, "results": items }))
}

fn single(tool: &str, input: &str, r: Result<(String, PathBuf), String>) -> Result<Value, String> {
    let (info, out) = r?;
    history::record(tool, input, &out, &info);
    Ok(json!({ "output": out.display().to_string(), "info": info }))
}

// ================================================================ esquemas

fn schema(props: Value, required: &[&str]) -> Value {
    let mut p = props;
    if let Value::Object(m) = &mut p {
        if !m.contains_key("out_dir") && required.contains(&"files") {
            m.insert("out_dir".into(), json!({ "type": "string", "description": "Output folder (default: same folder as each input)." }));
        }
    }
    json!({ "type": "object", "properties": p, "required": required })
}

fn files_prop(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": format!("Absolute paths of the {what} (folders are expanded).") })
}

fn enum_prop(desc: &str, values: &[&str], default: &str) -> Value {
    json!({ "type": "string", "enum": values, "default": default, "description": desc })
}

fn int_prop(desc: &str, min: i64, max: i64, default: i64) -> Value {
    json!({ "type": "integer", "minimum": min, "maximum": max, "default": default, "description": desc })
}

fn num_prop(desc: &str) -> Value {
    json!({ "type": "number", "description": desc })
}

fn bool_prop(desc: &str, default: bool) -> Value {
    json!({ "type": "boolean", "default": default, "description": desc })
}

fn str_prop(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}

// ================================================================ listas fechadas

const IMG_FMTS: &[(&str, Fmt)] = &[
    ("png", Fmt::Png), ("jpg", Fmt::Jpg), ("jpeg", Fmt::Jpg), ("webp", Fmt::Webp), ("bmp", Fmt::Bmp), ("gif", Fmt::Gif),
    ("tiff", Fmt::Tiff), ("ico", Fmt::Ico),
];
const LEVELS: &[(&str, Level)] = &[("light", Level::Leve), ("medium", Level::Media), ("strong", Level::Forte)];
const VLEVELS: &[(&str, VLevel)] = &[("light", VLevel::Leve), ("medium", VLevel::Media), ("strong", VLevel::Forte)];
const QUALITIES: &[(&str, VQuality)] = &[("high", VQuality::Alta), ("medium", VQuality::Media), ("low", VQuality::Baixa)];
const VFMTS: &[(&str, VFmt)] = &[
    ("mp4", VFmt::Mp4), ("mkv", VFmt::Mkv), ("avi", VFmt::Avi), ("mov", VFmt::Mov), ("webm", VFmt::Webm), ("flv", VFmt::Flv),
    ("wmv", VFmt::Wmv), ("gif", VFmt::Gif), ("mp3", VFmt::Mp3),
];
const AFMTS: &[(&str, AFmt)] = &[
    ("mp3", AFmt::Mp3), ("wav", AFmt::Wav), ("flac", AFmt::Flac), ("ogg", AFmt::Ogg), ("m4a", AFmt::M4a), ("opus", AFmt::Opus),
];
const ASPECTS: &[(&str, Aspect)] = &[
    ("none", Aspect::Livre), ("1:1", Aspect::Quadrado), ("4:3", Aspect::R4x3), ("3:2", Aspect::R3x2), ("16:9", Aspect::R16x9),
    ("3:4", Aspect::R3x4), ("9:16", Aspect::R9x16),
];
const VASPECTS: &[(&str, VAspect)] = &[
    ("original", VAspect::Original), ("9:16", VAspect::Vertical), ("1:1", VAspect::Quadrado), ("4:5", VAspect::R4x5),
    ("16:9", VAspect::Wide),
];

fn names<T>(list: &[(&'static str, T)]) -> Vec<&'static str> {
    let mut v: Vec<&str> = list.iter().map(|(n, _)| *n).collect();
    v.dedup();
    v.retain(|n| *n != "jpeg");
    v
}

fn parse_color(s: &str) -> Result<(u8, u8, u8), String> {
    let h = s.trim().trim_start_matches('#');
    let named = match h.to_lowercase().as_str() {
        "white" | "branco" => Some("ffffff"),
        "black" | "preto" => Some("000000"),
        _ => None,
    };
    let h = named.unwrap_or(h);
    if h.len() != 6 {
        return Err(format!("cor inválida: \"{s}\" (use #rrggbb)."));
    }
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| format!("cor inválida: \"{s}\" (use #rrggbb)."));
    Ok((c(0)?, c(2)?, c(4)?))
}

fn time_arg(a: &Args, k: &str) -> Result<Option<f64>, String> {
    match a.str(k).filter(|s| !s.trim().is_empty()) {
        None => Ok(None),
        Some(s) => video::parse_timecode(&s).map(Some).ok_or_else(|| format!("\"{k}\" inválido: \"{s}\" (use segundos ou 1:30).")),
    }
}

// ================================================================ ferramentas

fn image_op(tool: &str, a: &Args, ctx: &Ctx, op: ImageOp) -> Result<Value, String> {
    let files = images(a)?;
    let out = a.out_dir()?;
    batch(tool, &files, ctx, |f, _| imaging::process_and_save(f, &op, out.as_deref()))
}

fn t_image_convert(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let fmt = a.choice("format", IMG_FMTS, Fmt::Png)?;
    image_op("Converter imagem", a, ctx, ImageOp::Convert(fmt))
}

fn t_image_resize(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let mode = match a.num("percent")? {
        Some(p) => ResizeMode::Percent(p.clamp(1.0, 1000.0) as u32),
        None => ResizeMode::Pixels {
            w: a.num_or("width", 0.0)?.max(0.0) as u32,
            h: a.num_or("height", 0.0)?.max(0.0) as u32,
            keep: a.bool_or("keep_aspect", true)?,
        },
    };
    image_op("Redimensionar", a, ctx, ImageOp::Resize(mode))
}

fn t_image_compress(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let l = a.choice("level", LEVELS, Level::Media)?;
    image_op("Comprimir imagem", a, ctx, ImageOp::Compress(l))
}

fn t_image_quality(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let q = a.num_or("quality", 75.0)?.clamp(1.0, 100.0) as u8;
    image_op("Diminuir qualidade", a, ctx, ImageOp::Quality(q))
}

fn t_image_transform(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let rotate = a.num_or("rotate", 0.0)? as i64;
    let rotate = rotate.rem_euclid(360) as u16;
    if rotate % 90 != 0 {
        return Err("\"rotate\" deve ser 0, 90, 180 ou 270.".into());
    }
    let op = ImageOp::Transform {
        rotate,
        flip_h: a.bool_or("flip_horizontal", false)?,
        flip_v: a.bool_or("flip_vertical", false)?,
        aspect: a.choice("crop_aspect", ASPECTS, Aspect::Livre)?,
    };
    image_op("Girar e recortar", a, ctx, op)
}

fn t_image_adjust(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let v = |k: &str| -> Result<i16, String> { Ok(a.num_or(k, 0.0)?.clamp(-100.0, 100.0) as i16) };
    let filter = a.choice(
        "filter",
        &[("none", Filter::Nenhum), ("grayscale", Filter::PretoBranco), ("sepia", Filter::Sepia), ("invert", Filter::Inverter)],
        Filter::Nenhum,
    )?;
    let op = ImageOp::Adjust { brightness: v("brightness")?, contrast: v("contrast")?, saturation: v("saturation")?, filter };
    image_op("Ajustar cores", a, ctx, op)
}

fn t_image_strip(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    image_op("Remover metadados", a, ctx, ImageOp::StripMeta)
}

fn t_image_remove_bg(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let bg = match a.str("background").map(|s| s.trim().to_lowercase()) {
        None => Background::Transparente,
        Some(s) if s.is_empty() || s == "transparent" || s == "transparente" => Background::Transparente,
        Some(s) => {
            let (r, g, b) = parse_color(&s)?;
            Background::Cor(r, g, b)
        }
    };
    let hard = a.choice("edge", &[("hard", true), ("soft", false)], true)?;
    image_op("Remover fundo", a, ctx, ImageOp::RemoveBg { bg, hard })
}

fn t_image_upscale(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let scale = a.num_or("scale", 2.0)? as u32;
    if scale != 2 && scale != 4 {
        return Err("\"scale\" deve ser 2 ou 4.".into());
    }
    let files = images(a)?;
    let out = a.out_dir()?;
    batch("Ampliar com IA", &files, ctx, |f, progress| {
        crate::upscale::with_progress(progress, ctx.cancelled, || imaging::process_and_save(f, &ImageOp::Upscale { scale }, out.as_deref()))
    })
}

fn t_images_to_pdf(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = images(a)?;
    let page = a.choice(
        "page_size",
        &[("image", PageSize::Imagem), ("a4_portrait", PageSize::A4Retrato), ("a4_landscape", PageSize::A4Paisagem)],
        PageSize::Imagem,
    )?;
    let dst = output_file(a, &files[0], "imagens", "pdf")?;
    let n = imaging::images_to_pdf(&files, &dst, page, ctx.progress, ctx.cancelled)?;
    single("Imagens para PDF", &format!("{} imagens", files.len()), Ok((format!("{n} páginas • {}", tools::human_size(tools::file_size(&dst))), dst)))
}

/// Arquivo de saída único: "output" (caminho completo) ou "name" na pasta de saída (ou na do primeiro arquivo).
fn output_file(a: &Args, first: &Path, default_name: &str, ext: &str) -> Result<PathBuf, String> {
    if let Some(o) = a.str("output").filter(|s| !s.trim().is_empty()) {
        let p = PathBuf::from(o);
        let p = if tools::ext_lower(&p) == ext { p } else { p.with_extension(ext) };
        if let Some(parent) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        return Ok(tools::unique_path(p));
    }
    let folder = a.out_dir()?.or_else(|| first.parent().map(Path::to_path_buf)).unwrap_or_default();
    let name: String = a.str_or("name", default_name).chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect();
    let name = if name.trim().is_empty() { default_name.to_string() } else { name };
    Ok(tools::unique_path(folder.join(format!("{name}.{ext}"))))
}

fn t_image_ocr(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = images(a)?;
    let lang = a.str_or("language", "");
    let mut out = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if (ctx.cancelled)() {
            return Err("Cancelado.".into());
        }
        let r = ocr::recognize(f, &lang);
        out.push(match r {
            Ok(text) => json!({ "input": f.display().to_string(), "text": text }),
            Err(e) => json!({ "input": f.display().to_string(), "error": e }),
        });
        (ctx.progress)((i + 1) as f32 / files.len() as f32);
    }
    Ok(json!({ "results": out }))
}

fn t_qr(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let text = a.str("text").filter(|t| !t.is_empty()).ok_or("informe \"text\".")?;
    let size = a.num_or("size", 800.0)?.clamp(64.0, 4000.0) as u32;
    let (r, g, b) = parse_color(&a.str_or("color", "#000000"))?;
    let img = qr::render(&text, size, [r, g, b], qr::Correction::Media)?;
    let dst = match a.str("output").filter(|s| !s.trim().is_empty()) {
        Some(o) => tools::unique_path(PathBuf::from(o).with_extension("png")),
        None => {
            let dir = a.out_dir()?.unwrap_or_else(|| dirs_downloads().unwrap_or_else(std::env::temp_dir));
            tools::unique_path(dir.join("qrcode.png"))
        }
    };
    img.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    single("QR Code", &text, Ok((format!("{size}×{size} px", size = img.width()), dst)))
}

fn dirs_downloads() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads")).filter(|p| p.is_dir())
}

fn t_video_convert(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let fmt = a.choice("format", VFMTS, VFmt::Mp4)?;
    let q = a.choice("quality", QUALITIES, VQuality::Media)?;
    let out = a.out_dir()?;
    batch("Converter vídeo", &files, ctx, |f, p| video::convert(f, out.as_deref(), fmt, q, p, ctx.cancelled))
}

fn t_video_compress(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let mode = match a.num("target_mb")? {
        Some(mb) if mb > 0.0 => CompressMode::TargetMb(mb),
        _ => CompressMode::Level(a.choice("level", VLEVELS, VLevel::Media)?),
    };
    let opts = CompressOpts {
        mode,
        res: a.choice("resolution", &[("original", Res::Original), ("1080p", Res::P1080), ("720p", Res::P720), ("480p", Res::P480)], Res::Original)?,
        codec: a.choice("codec", &[("h264", Codec::H264), ("h265", Codec::H265)], Codec::H264)?,
        audio_kbps: a.num_or("audio_kbps", 128.0)?.clamp(32.0, 320.0) as u32,
    };
    let out = a.out_dir()?;
    batch("Comprimir vídeo", &files, ctx, |f, p| video::compress(f, out.as_deref(), opts, p, ctx.cancelled))
}

fn t_trim(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let start = time_arg(a, "start")?.unwrap_or(0.0);
    let end = time_arg(a, "end")?;
    let precise = a.bool_or("precise", true)?;
    let out = a.out_dir()?;
    batch("Cortar", &files, ctx, |f, p| media::trim(f, out.as_deref(), start, end, precise, p, ctx.cancelled))
}

fn t_video_merge(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    if files.len() < 2 {
        return Err("informe pelo menos 2 vídeos, na ordem.".into());
    }
    let precise = a.bool_or("precise", true)?;
    let out = a.out_dir()?;
    single("Juntar vídeos", &format!("{} vídeos", files.len()), media::merge(&files, out.as_deref(), precise, ctx.progress, ctx.cancelled))
}

fn t_audio_merge(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    if files.len() < 2 {
        return Err("informe pelo menos 2 áudios, na ordem.".into());
    }
    let fmt = a.choice("format", AFMTS, AFmt::Mp3)?;
    let out = a.out_dir()?;
    single("Juntar áudios", &format!("{} áudios", files.len()), media_extra::merge_audio(&files, out.as_deref(), fmt, ctx.progress, ctx.cancelled))
}

fn t_speed(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let factor = a.num_or("factor", 2.0)?;
    if !(0.25..=4.0).contains(&factor) {
        return Err("\"factor\" deve ficar entre 0.25 e 4.".into());
    }
    let keep = a.bool_or("keep_audio", true)?;
    let out = a.out_dir()?;
    batch("Velocidade", &files, ctx, |f, p| media::speed(f, out.as_deref(), factor, keep, p, ctx.cancelled))
}

fn t_remove_audio(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let out = a.out_dir()?;
    batch("Remover áudio", &files, ctx, |f, p| media::remove_audio(f, out.as_deref(), p, ctx.cancelled))
}

fn t_frames(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let every = a.num_or("every_seconds", 1.0)?.max(0.04);
    let png = a.choice("format", &[("png", true), ("jpg", false)], false)?;
    let out = a.out_dir()?;
    batch("Extrair frames", &files, ctx, |f, p| media::extract_frames(f, out.as_deref(), every, png, p, ctx.cancelled))
}

fn t_gif(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let start = time_arg(a, "start")?.unwrap_or(0.0);
    let length = a.num_or("duration", 5.0)?.clamp(0.5, 60.0);
    let width = a.num_or("width", 480.0)?.clamp(64.0, 1920.0) as u32;
    let fps = a.num_or("fps", 12.0)?.clamp(1.0, 50.0) as u32;
    let out = a.out_dir()?;
    batch("Vídeo para GIF", &files, ctx, |f, p| media_extra::video_gif(f, out.as_deref(), start, length, width, fps, p, ctx.cancelled))
}

fn t_video_transform(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let rotate = (a.num_or("rotate", 0.0)? as i64).rem_euclid(360) as u16;
    if rotate % 90 != 0 {
        return Err("\"rotate\" deve ser 0, 90, 180 ou 270.".into());
    }
    let flip = a.bool_or("flip_horizontal", false)?;
    let aspect = a.choice("aspect", VASPECTS, VAspect::Original)?;
    let out = a.out_dir()?;
    batch("Girar e recortar vídeo", &files, ctx, |f, p| media_extra::video_transform(f, out.as_deref(), rotate, flip, aspect, p, ctx.cancelled))
}

fn t_burn_subs(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let given = a.str("subtitle").map(PathBuf::from);
    let size = a.choice("size", &[("small", SubSize::Pequena), ("medium", SubSize::Media), ("large", SubSize::Grande)], SubSize::Media)?;
    let top = a.choice("position", &[("bottom", false), ("top", true)], false)?;
    let out = a.out_dir()?;
    batch("Legendas no vídeo", &files, ctx, |f, p| {
        let sub = given.clone().or_else(|| media_extra::find_subtitle(f)).ok_or("nenhuma legenda informada nem encontrada ao lado do vídeo.")?;
        media_extra::burn_subtitles(f, out.as_deref(), &sub, size, top, p, ctx.cancelled)
    })
}

fn t_add_music(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let music = a.file("music")?;
    let replace = a.bool_or("replace_audio", false)?;
    let volume = a.num_or("volume", 30.0)?.clamp(0.0, 200.0) as u32;
    let looped = a.bool_or("loop", true)?;
    let out = a.out_dir()?;
    batch("Música de fundo", &files, ctx, |f, p| media_extra::add_music(f, out.as_deref(), &music, replace, volume, looped, p, ctx.cancelled))
}

fn t_audio_convert(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let fmt = a.choice("format", AFMTS, AFmt::Mp3)?;
    let q = a.choice("quality", QUALITIES, VQuality::Alta)?;
    let out = a.out_dir()?;
    batch("Converter áudio", &files, ctx, |f, p| media::audio_convert(f, out.as_deref(), fmt, q, p, ctx.cancelled))
}

fn t_normalize(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let lufs = a.num_or("lufs", -14.0)?.clamp(-30.0, -5.0) as i32;
    let out = a.out_dir()?;
    batch("Normalizar volume", &files, ctx, |f, p| media::normalize(f, out.as_deref(), lufs, p, ctx.cancelled))
}

fn t_denoise(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let level = a.choice("strength", &[("light", media::Denoise::Leve), ("medium", media::Denoise::Media), ("strong", media::Denoise::Forte)], media::Denoise::Media)?;
    let out = a.out_dir()?;
    batch("Remover ruído", &files, ctx, |f, p| media::denoise(f, out.as_deref(), level, p, ctx.cancelled))
}

fn t_audio_compress(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = media_files(a)?;
    let codec = a.choice("codec", &[("mp3", ACodec::Mp3), ("aac", ACodec::Aac), ("opus", ACodec::Opus)], ACodec::Mp3)?;
    let kbps = a.num_or("kbps", 96.0)?.clamp(16.0, 320.0) as u32;
    let mono = a.bool_or("mono", false)?;
    let out = a.out_dir()?;
    batch("Comprimir áudio", &files, ctx, |f, p| media::audio_compress(f, out.as_deref(), codec, kbps, mono, p, ctx.cancelled))
}

fn t_separate(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    use separate::Stems;
    let files = media_files(a)?;
    let stems = a.choice("stems", &[("both", Stems::Ambos), ("vocals", Stems::Voz), ("instrumental", Stems::Instrumental)], Stems::Ambos)?;
    let fmt = a.choice("format", &[("wav", "wav"), ("mp3", "mp3"), ("flac", "flac")], "wav")?;
    let out = a.out_dir()?;
    batch("Separar voz", &files, ctx, |f, p| separate::separate(f, out.as_deref(), stems, fmt, p, ctx.cancelled))
}

fn t_transcribe(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    use transcribe::{Outputs, WModel};
    let files = media_files(a)?;
    let model = a.choice("model", &[("fast", WModel::Rapido), ("balanced", WModel::Equilibrado), ("accurate", WModel::Preciso)], WModel::Rapido)?;
    let lang = a.str_or("language", "auto");
    let translate = a.bool_or("translate_to_english", false)?;
    let wanted = a.strings("formats");
    let outputs = if wanted.is_empty() {
        Outputs { txt: true, srt: true, vtt: false }
    } else {
        let has = |f: &str| wanted.iter().any(|w| w.eq_ignore_ascii_case(f));
        Outputs { txt: has("txt"), srt: has("srt"), vtt: has("vtt") }
    };
    let out = a.out_dir()?;
    let mut v = batch("Transcrever", &files, ctx, |f, p| transcribe::transcribe(f, out.as_deref(), model, &lang, translate, outputs, p, ctx.cancelled))?;
    // devolve também o texto, para o agente não precisar abrir o arquivo
    if let Some(items) = v["results"].as_array_mut() {
        for it in items {
            if let Some(o) = it["output"].as_str() {
                let txt = Path::new(o).with_extension("txt");
                let srt = Path::new(o).with_extension("srt");
                let text = std::fs::read_to_string(&txt).ok().or_else(|| std::fs::read_to_string(&srt).ok());
                if let Some(t) = text {
                    let t: String = t.chars().take(20_000).collect();
                    it["text"] = Value::String(t.trim().to_string());
                }
            }
        }
    }
    Ok(v)
}

fn t_pdf_merge(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let dst = output_file(a, &files[0], "documento_juntado", "pdf")?;
    let n = pdf::merge(&files, &dst, ctx.cancelled)?;
    single("Juntar PDFs", &format!("{} PDFs", files.len()), Ok((format!("{n} páginas • {}", tools::human_size(tools::file_size(&dst))), dst)))
}

fn t_pdf_split(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let ranges = a.str("pages").filter(|s| !s.trim().is_empty());
    let out = a.out_dir()?;
    batch("Dividir PDF", &files, ctx, |f, p| pdf::split(f, out.as_deref(), ranges.as_deref(), p, ctx.cancelled))
}

fn music_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Music")).filter(|p| p.is_dir())
}

fn t_tts(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let voice = match a.str("voice").filter(|s| !s.trim().is_empty()) {
        Some(v) => crate::tts::voice(v.trim()).ok_or_else(|| {
            let ids: Vec<&str> = crate::tts::VOICES.iter().map(|v| v.id).collect();
            format!("voz desconhecida: {v}. Use uma de: {}.", ids.join(", "))
        })?,
        None => crate::tts::default_voice(&a.str_or("language", "pt")),
    };
    let speed = a.num_or("speed", 1.0)? as f32;
    let fmt = a.choice("format", &[("mp3", crate::tts::OutFmt::Mp3), ("wav", crate::tts::OutFmt::Wav)], crate::tts::OutFmt::Mp3)?;
    let ext = if fmt == crate::tts::OutFmt::Mp3 { "mp3" } else { "wav" };
    let out = a.out_dir()?;
    if let Some(text) = a.str("text").filter(|t| !t.trim().is_empty()) {
        let dir = out.or_else(music_dir).unwrap_or_else(std::env::temp_dir);
        let dst = match a.str("output").filter(|s| !s.trim().is_empty()) {
            Some(p) => tools::unique_path(PathBuf::from(p).with_extension(ext)),
            None => tools::unique_path(dir.join(format!("narracao_{}.{ext}", tools::now_string().replace([':', ' ', '/'], "-")))),
        };
        return single("Texto para voz", "texto", crate::tts::speak(&text, voice, speed, fmt, &dst, ctx.progress, ctx.cancelled));
    }
    let files = filter_ext(a.files().map_err(|_| "informe \"text\" ou arquivos de texto em \"files\".".to_string())?, &["txt", "md", "srt", "vtt"])?;
    batch("Texto para voz", &files, ctx, |f, p| {
        let text = crate::tts::text_from_file(f)?;
        let dst = tools::out_path(f, out.as_deref(), "_narracao", ext);
        crate::tts::speak(&text, voice, speed, fmt, &dst, p, ctx.cancelled)
    })
}

fn t_image_generate(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    use crate::imagegen::{Quality, Request, Shape};
    let mut prompt = a.str("prompt").filter(|s| !s.trim().is_empty()).ok_or("informe \"prompt\" (a descrição da imagem).")?;
    // o modelo entende inglês: com a IA configurada, traduz o pedido
    if a.bool_or("translate", false)? {
        let cfg = crate::ai_api::Settings::load().config()?;
        prompt = crate::ai_api::ask(&cfg, "Translate the user's image description to English for an image generator. Reply with only the translation.", &[crate::ai_api::Part::Text(prompt.clone())], 300)?;
    }
    let req = Request {
        prompt,
        negative: a.str_or("negative", ""),
        shape: a.choice("shape", &[("square", Shape::Quadrada), ("portrait", Shape::Retrato), ("landscape", Shape::Paisagem)], Shape::Quadrada)?,
        quality: a.choice("quality", &[("fast", Quality::Rapida), ("better", Quality::Melhor)], Quality::Rapida)?,
        count: a.num_or("count", 1.0)?.clamp(1.0, 8.0) as u32,
        seed: a.num("seed")?.map(|s| s as i64),
    };
    let dir = a.out_dir()?.unwrap_or_else(crate::imagegen::default_dir);
    let files = crate::imagegen::generate(&req, &dir, ctx.progress, ctx.cancelled)?;
    for f in &files {
        history::record("Gerar imagem", &req.prompt.chars().take(60).collect::<String>(), f, &format!("{}×{}", req.shape.size().0, req.shape.size().1));
    }
    Ok(json!({ "prompt": req.prompt, "results": files.iter().map(|f| json!({ "output": f.display().to_string() })).collect::<Vec<_>>() }))
}

fn t_image_erase(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = images(a)?;
    let mask_file = a.str("mask").filter(|s| !s.trim().is_empty());
    let rects: Vec<(i64, i64, i64, i64)> = match a.get("regions") {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|r| match r {
                Value::Array(v) if v.len() == 4 => Some((v[0].as_i64()?, v[1].as_i64()?, v[2].as_i64()?, v[3].as_i64()?)),
                Value::Object(m) => Some((m.get("x")?.as_i64()?, m.get("y")?.as_i64()?, m.get("width")?.as_i64()?, m.get("height")?.as_i64()?)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    if mask_file.is_none() && rects.is_empty() {
        return Err("informe \"regions\" ([[x, y, largura, altura], …]) ou \"mask\" (imagem: branco = apagar).".into());
    }
    let out = a.out_dir()?;
    batch("Apagar objeto", &files, ctx, |f, p| {
        let (w, h) = image::image_dimensions(f).map_err(|e| e.to_string())?;
        let mask = match &mask_file {
            Some(m) => image::open(m).map_err(|e| format!("máscara: {e}"))?.to_luma8(),
            None => crate::inpaint::mask_from_rects(w, h, &rects),
        };
        crate::inpaint::erase_file(f, out.as_deref(), &mask, p, ctx.cancelled)
    })
}

/// Opções das legendas animadas a partir dos argumentos.
pub fn caption_style(a: &Args) -> Result<crate::captions::Style, String> {
    use crate::captions::{Look, Place, Style, Tint};
    Ok(Style {
        look: a.choice("style", &[("highlight", Look::Destaque), ("box", Look::Caixa), ("word", Look::UmaPalavra)], Look::Destaque)?,
        place: a.choice("position", &[("bottom", Place::Baixo), ("middle", Place::Meio), ("top", Place::Cima)], Place::Baixo)?,
        tint: a.choice("color", &[("yellow", Tint::Amarelo), ("green", Tint::Verde), ("red", Tint::Cinabrio), ("blue", Tint::Azul)], Tint::Amarelo)?,
        words_per_line: a.num_or("words_per_line", 3.0)?.clamp(1.0, 6.0) as usize,
        uppercase: a.bool_or("uppercase", true)?,
    })
}

fn t_captions(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    use transcribe::WModel;
    let files = filter_ext(a.files()?, video::VIDEO_EXTS)?;
    let model = a.choice("model", &[("fast", WModel::Rapido), ("balanced", WModel::Equilibrado), ("accurate", WModel::Preciso)], WModel::Rapido)?;
    let lang = a.str_or("language", "auto");
    let style = caption_style(a)?;
    let keep = a.bool_or("keep_ass", false)?;
    let out = a.out_dir()?;
    batch("Legendas animadas", &files, ctx, |f, p| crate::captions::animated(f, out.as_deref(), model, &lang, style, keep, p, ctx.cancelled))
}

fn t_pdf_organize(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let src = files.first().ok_or("informe o PDF.")?;
    let total = pdf::page_count(src).ok_or("não foi possível ler o PDF.")? as u32;
    let order = match a.str("order").filter(|s| !s.trim().is_empty()) {
        Some(o) => pdf::parse_order(&o.replace(['[', ']'], ""), total)?,
        None => (1..=total).collect(),
    };
    let rotations: std::collections::HashMap<u32, i64> = match a.get("rotate") {
        Some(Value::Object(m)) => m.iter().filter_map(|(k, v)| Some((k.trim().parse().ok()?, v.as_i64().or_else(|| v.as_str()?.parse().ok())?))).collect(),
        _ => Default::default(),
    };
    for r in rotations.values() {
        if r % 90 != 0 {
            return Err("\"rotate\": use 90, 180, 270 (ou -90).".into());
        }
    }
    let plan: Vec<(u32, i64)> = order.iter().map(|n| (*n, *rotations.get(n).unwrap_or(&0))).collect();
    let out = a.out_dir()?;
    single("Organizar páginas", &tools::file_name(src), pdf::organize(src, out.as_deref(), &plan))
}

fn t_pdf_compress(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let l = a.choice("level", LEVELS, Level::Media)?;
    let out = a.out_dir()?;
    batch("Comprimir PDF", &files, ctx, |f, p| pdf_extra::compress(f, out.as_deref(), l, p, ctx.cancelled))
}

fn t_pdf_images(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let fmt = a.choice("format", &[("png", pdf_extra::PageFmt::Png), ("jpg", pdf_extra::PageFmt::Jpg)], pdf_extra::PageFmt::Png)?;
    let dpi = a.num_or("dpi", 150.0)?.clamp(36.0, 600.0) as u32;
    let ranges = a.str("pages").filter(|s| !s.trim().is_empty());
    let pw = a.str_or("password", "");
    let out = a.out_dir()?;
    batch("PDF para imagens", &files, ctx, |f, p| pdf_extra::to_images(f, out.as_deref(), fmt, dpi, ranges.as_deref(), &pw, p, ctx.cancelled))
}

fn t_pdf_protect(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let pw = a.str("password").filter(|s| !s.is_empty()).ok_or("informe \"password\".")?;
    let owner = a.str_or("owner_password", "");
    let print = a.bool_or("allow_print", true)?;
    let copy = a.bool_or("allow_copy", false)?;
    let out = a.out_dir()?;
    batch("Proteger PDF", &files, ctx, |f, _| pdf_extra::protect(f, out.as_deref(), &pw, &owner, print, copy))
}

fn t_pdf_unlock(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let pw = a.str_or("password", "");
    let out = a.out_dir()?;
    batch("Tirar senha do PDF", &files, ctx, |f, _| pdf_extra::unlock(f, out.as_deref(), &pw))
}

fn t_pdf_ocr(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let files = pdfs(a)?;
    let lang = a.str_or("language", "");
    let skip = a.bool_or("skip_pages_with_text", true)?;
    let out = a.out_dir()?;
    batch("PDF pesquisável", &files, ctx, |f, p| pdf_extra::make_searchable(f, out.as_deref(), &lang, skip, p, ctx.cancelled))
}

fn t_download(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let mut urls = a.strings("urls");
    urls.extend(a.strings("url"));
    let urls: Vec<String> = urls.iter().flat_map(|u| download::parse_links(u)).collect();
    if urls.is_empty() {
        return Err("informe os links em \"urls\" (começando com http).".into());
    }
    let kind = a.choice("kind", &[("mp4", download::Kind::Mp4), ("mp3", download::Kind::Mp3)], download::Kind::Mp4)?;
    let qualities = if kind == download::Kind::Mp4 { download::MP4_QUALITIES } else { download::MP3_QUALITIES };
    let qname = a.str_or("quality", qualities[0]).to_lowercase();
    let quality = qualities
        .iter()
        .position(|q| q.to_lowercase() == qname || q.to_lowercase().starts_with(&qname) || (qname == "best" && *q == "Melhor"))
        .ok_or_else(|| format!("\"quality\" inválido. Use: best, {}", qualities.iter().skip(usize::from(kind == download::Kind::Mp4)).copied().collect::<Vec<_>>().join(", ")))?;
    let playlist = a.bool_or("playlist", false)?;
    let extras = download::Extras { subtitles: a.bool_or("subtitles", false)?, cover: a.bool_or("cover", false)? };
    let cookies = a.str("browser_cookies").filter(|s| !s.trim().is_empty());
    let dir = a.out_dir()?.or_else(dirs_downloads).ok_or("informe \"out_dir\".")?;

    let job = crate::job::Job::default();
    let mut items = Vec::new();
    for (i, url) in urls.iter().enumerate() {
        if (ctx.cancelled)() {
            return Err("Cancelado.".into());
        }
        (ctx.log)(&format!("[{}/{}] {url}", i + 1, urls.len()));
        // o download roda numa thread; aqui só repassamos o progresso e o cancelamento
        let r = std::thread::scope(|s| {
            let job2 = job.clone();
            let (u, d, c) = (url.clone(), dir.clone(), cookies.clone());
            let h = s.spawn(move || {
                let ctl = crate::job::Ctl::headless(&job2);
                download::download(&u, &d, kind, quality, playlist, c.as_deref(), extras, &ctl)
            });
            while !h.is_finished() {
                if (ctx.cancelled)() {
                    job.cancel();
                }
                let p = job.state().progress;
                (ctx.progress)((i as f32 + p) / urls.len() as f32);
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            h.join().unwrap_or_else(|_| Err("erro inesperado.".into()))
        });
        match r {
            Ok(paths) => {
                for p in &paths {
                    history::record("Download", url, p, &tools::human_size(tools::file_size(p)));
                }
                items.push(json!({ "url": url, "files": paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>() }));
            }
            Err(e) => items.push(json!({ "url": url, "error": e })),
        }
    }
    Ok(json!({ "results": items }))
}

fn t_info(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let files = a.files()?;
    let out: Vec<Value> = files.iter().map(|f| json!({ "input": f.display().to_string(), "info": info::read(f).as_text() })).collect();
    Ok(json!({ "results": out }))
}

fn t_screen_record(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let seconds = a.num("seconds")?.ok_or("informe \"seconds\" (duração da gravação).")?;
    if !(1.0..=4.0 * 3600.0).contains(&seconds) {
        return Err("\"seconds\" deve ficar entre 1 e 14400.".into());
    }
    let area = if let Some(t) = a.str("window").filter(|s| !s.trim().is_empty()) {
        screenrec::Area::Janela(t)
    } else if let Some(Value::Object(r)) = a.get("region") {
        let g = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        screenrec::Area::Regiao { x: g("x") as i32, y: g("y") as i32, w: g("width") as u32, h: g("height") as u32 }
    } else if let Some(n) = a.num("monitor")?.filter(|n| *n >= 1.0) {
        let list = screenrec::monitors();
        let m = list.get(n as usize - 1).ok_or_else(|| format!("\"monitor\" {n}: só há {} monitor(es) (veja list_devices).", list.len()))?;
        m.area()
    } else {
        screenrec::Area::TelaInteira
    };
    let o = screenrec::Options {
        area,
        fps: a.num_or("fps", 30.0)?.clamp(5.0, 60.0) as u32,
        mic: a.str("microphone").filter(|s| !s.trim().is_empty()),
        cursor: a.bool_or("show_cursor", true)?,
        quality: a.choice("quality", &[("high", screenrec::Quality::Alta), ("medium", screenrec::Quality::Media), ("low", screenrec::Quality::Leve)], screenrec::Quality::Media)?,
        max_height: a.choice("resolution", &[("original", None), ("1080p", Some(1080)), ("720p", Some(720))], None)?,
    };
    let dst = match a.str("output").filter(|s| !s.trim().is_empty()) {
        Some(p) => tools::unique_path(PathBuf::from(p).with_extension("mp4")),
        None => {
            let dir = a.out_dir()?.or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Videos"))).unwrap_or_else(std::env::temp_dir);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            tools::unique_path(dir.join(format!("gravacao_tela_{}.mp4", tools::now_string().replace([':', ' ', '/'], "-"))))
        }
    };
    single("Gravar tela", "tela", screenrec::record_for(&o, &dst, seconds, ctx.progress, ctx.cancelled))
}

fn t_list_devices(_a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let monitors: Vec<Value> = screenrec::monitors()
        .iter()
        .enumerate()
        .map(|(i, m)| json!({ "monitor": i + 1, "primary": m.primary, "x": m.x, "y": m.y, "width": m.w, "height": m.h }))
        .collect();
    Ok(json!({ "microphones": crate::recorder::devices(), "windows": screenrec::windows(), "monitors": monitors }))
}

fn t_preset_list(_a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let list: Vec<Value> = crate::presets::load()
        .into_iter()
        .map(|p| json!({ "name": p.name, "tool": p.tool, "args": p.args, "description": p.description }))
        .collect();
    Ok(json!({ "presets": list }))
}

fn t_preset_run(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let name = a.str("name").ok_or("informe \"name\" (use preset_list para ver os nomes).")?;
    let p = crate::presets::find(&name).ok_or_else(|| format!("predefinição não encontrada: {name}"))?;
    let mut args = p.args.as_object().cloned().unwrap_or_default();
    for (k, v) in a.0 {
        if k != "name" {
            args.insert(k.clone(), v.clone());
        }
    }
    call(&p.tool, &Value::Object(args), ctx)
}

fn t_preset_save(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let name = a.str("name").filter(|s| !s.trim().is_empty()).ok_or("informe \"name\".")?;
    let tool = a.str("tool").ok_or("informe \"tool\".")?;
    find(&tool).ok_or_else(|| format!("ferramenta desconhecida: {tool}"))?;
    let mut args = match a.get("args") {
        Some(Value::Object(m)) => m.clone(),
        Some(Value::String(s)) => serde_json::from_str(s).map_err(|e| format!("\"args\" não é um JSON válido: {e}"))?,
        _ => Map::new(),
    };
    args.remove("files");
    args.remove("file");
    crate::presets::save(crate::presets::Preset { name: name.trim().to_string(), tool, args: Value::Object(args), description: a.str_or("description", "") })?;
    Ok(json!({ "saved": name }))
}

// ================================================================ fluxos

fn flow_summary(f: &crate::flow::Flow) -> Value {
    let nodes: Vec<String> = f.nodes.iter().map(|n| if n.title.is_empty() { n.kind.clone() } else { format!("{} ({})", n.title, n.kind) }).collect();
    json!({ "name": f.name, "description": f.description, "nodes": nodes, "links": f.links.len() })
}

fn t_flow_list(_a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    crate::flow::ensure_examples();
    Ok(json!({ "flows": crate::flow::list().iter().map(flow_summary).collect::<Vec<_>>() }))
}

fn t_flow_get(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let name = a.str("name").ok_or("informe \"name\".")?;
    let f = crate::flow::find(&name).ok_or_else(|| format!("fluxo não encontrado: {name}"))?;
    serde_json::to_value(f).map_err(|e| e.to_string())
}

fn t_flow_node_types(_a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let kinds: Vec<Value> = crate::flow::kinds()
        .into_iter()
        .map(|k| json!({ "kind": k.kind, "title": k.title, "category": k.category, "description": k.description,
            "outputs": k.outputs, "has_input": k.has_input, "args": k.schema }))
        .collect();
    Ok(json!({
        "how_it_works": "A flow is a graph of nodes. Each link sends what a node produced (files and texts) from one of its outputs (\"out\", or \"yes\"/\"no\" for ai_gate and filter) to another node. Tool nodes (kind \"tool:<name>\") run that Conver tool on the files that arrive; their \"args\" are the tool's arguments without files/out_dir. String args may use {text} (incoming text) and {file}. Nodes run in link order; \"input\" nodes receive the files given to flow_run. ai_prompt/ai_gate use the AI API configured by the user in Settings.",
        "example": { "name": "Fotos para o feed", "nodes": [
            { "id": 1, "kind": "input", "pos": [40, 120] },
            { "id": 2, "kind": "tool:image_resize", "args": { "width": 1080, "height": 1350 }, "pos": [300, 120] },
            { "id": 3, "kind": "ai_prompt", "title": "Legenda", "args": { "prompt": "Escreva uma legenda curta para Instagram desta foto.", "save_as": "txt" }, "pos": [560, 120] },
            { "id": 4, "kind": "output", "args": { "folder": "C:\\Users\\voce\\Pictures\\Feed" }, "pos": [820, 120] } ],
            "links": [ { "from": 1, "to": 2 }, { "from": 2, "to": 3 }, { "from": 3, "to": 4 } ] },
        "kinds": kinds }))
}

fn t_flow_save(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let raw = a.get("flow").ok_or("informe \"flow\" (o fluxo em JSON; veja flow_node_types).")?;
    let mut f: crate::flow::Flow = match raw {
        Value::String(s) => serde_json::from_str(s),
        v => serde_json::from_value(v.clone()),
    }
    .map_err(|e| format!("\"flow\" inválido: {e}"))?;
    // posições automáticas para quem não informar
    for (i, n) in f.nodes.iter_mut().enumerate() {
        if n.pos == [0.0, 0.0] {
            n.pos = [40.0 + i as f32 * 260.0, 120.0];
        }
    }
    let replaced = crate::flow::find(&f.name).is_some();
    if let Some(old) = a.str("rename_from").filter(|s| !s.trim().is_empty() && !s.eq_ignore_ascii_case(&f.name)) {
        let _ = crate::flow::remove(&old);
    }
    crate::flow::save(&f)?;
    Ok(json!({ "saved": f.name, "replaced": replaced, "nodes": f.nodes.len(), "links": f.links.len() }))
}

fn t_flow_delete(a: &Args, _ctx: &Ctx) -> Result<Value, String> {
    let name = a.str("name").ok_or("informe \"name\".")?;
    let f = crate::flow::find(&name).ok_or_else(|| format!("fluxo não encontrado: {name}"))?;
    crate::flow::remove(&f.name)?;
    Ok(json!({ "deleted": f.name }))
}

fn t_flow_run(a: &Args, ctx: &Ctx) -> Result<Value, String> {
    let name = a.str("name").ok_or("informe \"name\".")?;
    let f = crate::flow::find(&name).ok_or_else(|| format!("fluxo não encontrado: {name}"))?;
    let files = if a.get("files").is_some() || a.get("file").is_some() { a.files()? } else { Vec::new() };
    let inputs = crate::flow::Inputs { files, texts: a.strings("texts") };
    let out = a.out_dir()?;
    let state = std::sync::Mutex::new(crate::flow::RunState::default());
    let ai = crate::ai_api::Settings::load().config();
    crate::flow::run(&f, &inputs, out.as_deref(), &state, ctx, &ai)
}

// ================================================================ tabela

macro_rules! tool {
    ($name:literal, $title:literal, $desc:literal, $schema:expr, $run:path) => {
        Tool { name: $name, title: $title, description: $desc, schema: || $schema, run: $run }
    };
}

pub static TOOLS: &[Tool] = &[
    tool!("image_convert", "Converter imagem", "Convert images to another format (PNG, JPG, WEBP, BMP, GIF, TIFF, ICO).",
        schema(json!({ "files": files_prop("images"), "format": enum_prop("Target format.", &names(IMG_FMTS), "png") }), &["files", "format"]), t_image_convert),
    tool!("image_resize", "Redimensionar", "Resize images by percentage or to a width/height in pixels (keeps the aspect ratio by default).",
        schema(json!({ "files": files_prop("images"), "percent": num_prop("Scale in percent (e.g. 50). Overrides width/height."),
            "width": num_prop("Target width in px (0 = automatic)."), "height": num_prop("Target height in px (0 = automatic)."),
            "keep_aspect": bool_prop("Fit inside width×height keeping the proportion.", true) }), &["files"]), t_image_resize),
    tool!("image_compress", "Comprimir imagem", "Make image files smaller with little visible loss (PNG is quantized, JPG/WEBP re-encoded).",
        schema(json!({ "files": files_prop("images"), "level": enum_prop("Compression level.", &["light", "medium", "strong"], "medium") }), &["files"]), t_image_compress),
    tool!("image_quality", "Diminuir qualidade", "Re-encode images with a specific JPEG/WEBP quality (1-100).",
        schema(json!({ "files": files_prop("images"), "quality": int_prop("Quality.", 1, 100, 75) }), &["files"]), t_image_quality),
    tool!("image_transform", "Girar e recortar", "Rotate, flip and/or center-crop images to an aspect ratio.",
        schema(json!({ "files": files_prop("images"), "rotate": int_prop("Clockwise degrees: 0, 90, 180 or 270.", 0, 270, 0),
            "flip_horizontal": bool_prop("Mirror left-right.", false), "flip_vertical": bool_prop("Mirror top-bottom.", false),
            "crop_aspect": enum_prop("Center crop.", &names(ASPECTS), "none") }), &["files"]), t_image_transform),
    tool!("image_adjust", "Ajustar cores", "Adjust brightness, contrast and saturation (-100..100) and apply a filter.",
        schema(json!({ "files": files_prop("images"), "brightness": int_prop("Brightness.", -100, 100, 0), "contrast": int_prop("Contrast.", -100, 100, 0),
            "saturation": int_prop("Saturation.", -100, 100, 0), "filter": enum_prop("Filter.", &["none", "grayscale", "sepia", "invert"], "none") }), &["files"]), t_image_adjust),
    tool!("image_strip_metadata", "Remover metadados", "Remove EXIF/GPS/XMP metadata from images without re-encoding when possible.",
        schema(json!({ "files": files_prop("images") }), &["files"]), t_image_strip),
    tool!("image_remove_background", "Remover fundo", "Remove the background of photos with a local AI model. Transparent output is saved as PNG.",
        schema(json!({ "files": files_prop("images"), "background": str_prop("\"transparent\" (default) or a color like \"#ffffff\"."),
            "edge": enum_prop("Edge style: hard (crisp) or soft (better for hair).", &["hard", "soft"], "hard") }), &["files"]), t_image_remove_bg),
    tool!("image_upscale", "Ampliar com IA", "Upscale images 2x or 4x with a local AI model (Real-ESRGAN), keeping details sharp.",
        schema(json!({ "files": files_prop("images"), "scale": int_prop("Scale factor: 2 or 4.", 2, 4, 2) }), &["files"]), t_image_upscale),
    tool!("images_to_pdf", "Imagens para PDF", "Combine images (in the given order) into one PDF.",
        schema(json!({ "files": files_prop("images"), "page_size": enum_prop("Page size.", &["image", "a4_portrait", "a4_landscape"], "image"),
            "output": str_prop("Full output path of the PDF (optional)."), "name": str_prop("File name without extension (optional).") }), &["files"]), t_images_to_pdf),
    tool!("image_ocr", "Texto de imagem (OCR)", "Read the text of images with Windows OCR (offline). Returns the text.",
        schema(json!({ "files": files_prop("images"), "language": str_prop("BCP-47 tag like \"pt-BR\" or \"en-US\" (default: user languages).") }), &["files"]), t_image_ocr),
    tool!("qr_code", "Gerador de QR Code", "Create a QR code PNG for a text, link, Wi-Fi string etc.",
        json!({ "type": "object", "properties": { "text": str_prop("Content of the QR code."), "size": int_prop("Size in px.", 64, 4000, 800),
            "color": str_prop("Foreground color #rrggbb (default black)."), "output": str_prop("Output PNG path (default: Downloads/qrcode.png)."),
            "out_dir": str_prop("Output folder.") }, "required": ["text"] }), t_qr),
    tool!("video_convert", "Converter vídeo", "Convert videos to MP4, MKV, AVI, MOV, WEBM, FLV, WMV, GIF or extract MP3.",
        schema(json!({ "files": files_prop("videos"), "format": enum_prop("Target format.", &names(VFMTS), "mp4"),
            "quality": enum_prop("Quality.", &["high", "medium", "low"], "medium") }), &["files", "format"]), t_video_convert),
    tool!("video_compress", "Comprimir vídeo", "Compress videos to MP4 by level or to a target size in MB (two-pass).",
        schema(json!({ "files": files_prop("videos"), "level": enum_prop("Compression level (ignored with target_mb).", &["light", "medium", "strong"], "medium"),
            "target_mb": num_prop("Target file size in MB (optional)."), "resolution": enum_prop("Max resolution.", &["original", "1080p", "720p", "480p"], "original"),
            "codec": enum_prop("Video codec.", &["h264", "h265"], "h264"), "audio_kbps": int_prop("Audio bitrate.", 32, 320, 128) }), &["files"]), t_video_compress),
    tool!("media_trim", "Cortar", "Cut a part of videos or audios (start/end in seconds or mm:ss).",
        schema(json!({ "files": files_prop("videos or audios"), "start": str_prop("Start (e.g. 10 or 0:10)."), "end": str_prop("End (optional; default = until the end)."),
            "precise": bool_prop("Frame-accurate cut (re-encodes). false = instant cut at keyframes.", true) }), &["files"]), t_trim),
    tool!("video_merge", "Juntar vídeos", "Join videos one after another (in the given order) into one MP4.",
        schema(json!({ "files": files_prop("videos, in order"), "precise": bool_prop("Re-encode so different videos match.", true) }), &["files"]), t_video_merge),
    tool!("video_speed", "Velocidade", "Speed up or slow down videos/audios (0.25x to 4x).",
        schema(json!({ "files": files_prop("videos or audios"), "factor": num_prop("Speed factor, e.g. 2 = twice as fast, 0.5 = half."),
            "keep_audio": bool_prop("Keep (and adjust) the audio.", true) }), &["files", "factor"]), t_speed),
    tool!("video_remove_audio", "Remover áudio", "Remove the audio track of videos (no re-encoding).",
        schema(json!({ "files": files_prop("videos") }), &["files"]), t_remove_audio),
    tool!("video_extract_frames", "Extrair frames", "Save frames of videos as images, one every N seconds.",
        schema(json!({ "files": files_prop("videos"), "every_seconds": num_prop("Interval between frames in seconds (default 1)."),
            "format": enum_prop("Image format.", &["jpg", "png"], "jpg") }), &["files"]), t_frames),
    tool!("video_to_gif", "Vídeo para GIF", "Make an animated GIF from a part of videos.",
        schema(json!({ "files": files_prop("videos"), "start": str_prop("Start (seconds or mm:ss)."), "duration": num_prop("Length in seconds (default 5)."),
            "width": int_prop("Width in px.", 64, 1920, 480), "fps": int_prop("Frames per second.", 1, 50, 12) }), &["files"]), t_gif),
    tool!("video_transform", "Girar e recortar vídeo", "Rotate/mirror videos and crop to 9:16, 1:1, 4:5 or 16:9.",
        schema(json!({ "files": files_prop("videos"), "rotate": int_prop("Clockwise degrees: 0, 90, 180, 270.", 0, 270, 0),
            "flip_horizontal": bool_prop("Mirror.", false), "aspect": enum_prop("Crop.", &names(VASPECTS), "original") }), &["files"]), t_video_transform),
    tool!("video_burn_subtitles", "Legendas no vídeo", "Burn subtitles (.srt/.ass/.vtt) into videos. Without \"subtitle\", uses a subtitle file with the same name as the video.",
        schema(json!({ "files": files_prop("videos"), "subtitle": str_prop("Subtitle file path (optional)."),
            "size": enum_prop("Text size.", &["small", "medium", "large"], "medium"), "position": enum_prop("Position.", &["bottom", "top"], "bottom") }), &["files"]), t_burn_subs),
    tool!("video_add_music", "Música de fundo", "Add background music to videos (mixed or replacing the original audio).",
        schema(json!({ "files": files_prop("videos"), "music": str_prop("Music file path."), "replace_audio": bool_prop("Replace the original audio.", false),
            "volume": int_prop("Music volume in percent.", 0, 200, 30), "loop": bool_prop("Repeat the music until the end of the video.", true) }), &["files", "music"]), t_add_music),
    tool!("audio_convert", "Converter áudio", "Convert audios (or the audio of videos) to MP3, WAV, FLAC, OGG, M4A or OPUS.",
        schema(json!({ "files": files_prop("audios or videos"), "format": enum_prop("Target format.", &names(AFMTS), "mp3"),
            "quality": enum_prop("Quality.", &["high", "medium", "low"], "high") }), &["files", "format"]), t_audio_convert),
    tool!("audio_merge", "Juntar áudios", "Join audios one after another into one file.",
        schema(json!({ "files": files_prop("audios, in order"), "format": enum_prop("Output format.", &names(AFMTS), "mp3") }), &["files"]), t_audio_merge),
    tool!("audio_normalize", "Normalizar volume", "Normalize loudness (EBU R128) so files have the same volume.",
        schema(json!({ "files": files_prop("audios or videos"), "lufs": int_prop("Target loudness (-14 streaming, -16 podcast, -23 TV).", -30, -5, -14) }), &["files"]), t_normalize),
    tool!("audio_denoise", "Remover ruído", "Remove background noise (hiss, fan, street, keyboard) from voice recordings in audios and videos. medium/strong use the RNNoise neural network, offline.",
        schema(json!({ "files": files_prop("audios or videos"), "strength": enum_prop("light (only hiss), medium (AI, recommended for voice) or strong.", &["light", "medium", "strong"], "medium") }), &["files"]), t_denoise),
    tool!("audio_compress", "Comprimir áudio", "Make audio files smaller (MP3/AAC/OPUS at a bitrate, optional mono).",
        schema(json!({ "files": files_prop("audios"), "codec": enum_prop("Codec.", &["mp3", "aac", "opus"], "mp3"), "kbps": int_prop("Bitrate.", 16, 320, 96),
            "mono": bool_prop("Convert to mono.", false) }), &["files"]), t_audio_compress),
    tool!("audio_separate", "Separar voz e instrumental", "Split songs into vocals and instrumental with a local AI model (UVR MDX-Net). The model (67 MB) is downloaded on first use.",
        schema(json!({ "files": files_prop("songs (audio or video)"), "stems": enum_prop("What to save.", &["both", "vocals", "instrumental"], "both"),
            "format": enum_prop("Output format.", &["wav", "mp3", "flac"], "wav") }), &["files"]), t_separate),
    tool!("transcribe", "Transcrever / legendar", "Transcribe speech in audios/videos to text and subtitles (TXT, SRT, VTT) with Whisper, offline. The model is downloaded on first use. Returns the text.",
        schema(json!({ "files": files_prop("audios or videos"), "model": enum_prop("fast (82 MB), balanced (264 MB) or accurate (574 MB).", &["fast", "balanced", "accurate"], "fast"),
            "language": str_prop("Spoken language code like \"pt\", \"en\" or \"auto\" (default)."),
            "translate_to_english": bool_prop("Translate the speech to English.", false),
            "formats": json!({ "type": "array", "items": { "type": "string", "enum": ["txt", "srt", "vtt"] }, "description": "Files to create (default txt and srt)." }) }), &["files"]), t_transcribe),
    tool!("pdf_merge", "Juntar PDFs", "Join PDFs (in the given order) into one.",
        schema(json!({ "files": files_prop("PDFs, in order"), "output": str_prop("Full output path (optional)."), "name": str_prop("File name without extension (optional).") }), &["files"]), t_pdf_merge),
    tool!("pdf_split", "Dividir PDF", "Extract pages (e.g. \"1-3, 7, 10-\") into a new PDF, or save every page as its own PDF when \"pages\" is empty.",
        schema(json!({ "files": files_prop("PDFs"), "pages": str_prop("Page ranges like \"1-3, 7\". Empty = one PDF per page.") }), &["files"]), t_pdf_split),
    tool!("text_to_speech", "Texto para voz", "Read text aloud with a natural local voice (Piper, offline; ~63 MB per voice, downloaded on first use) and save MP3/WAV. Give \"text\" or text/subtitle files (.txt, .md, .srt, .vtt).",
        json!({ "type": "object", "properties": {
            "text": str_prop("Text to read (instead of files)."), "files": files_prop("text or subtitle files"),
            "voice": enum_prop("Voice id (default: the first one of \"language\").", &crate::tts::VOICES.iter().map(|v| v.id).collect::<Vec<_>>(), "pt_BR-faber-medium"),
            "language": str_prop("Language code to pick a default voice: pt, en, es, fr, de, it."),
            "speed": num_prop("Speech speed, 0.5 to 2 (1 = normal)."), "format": enum_prop("Output format.", &["mp3", "wav"], "mp3"),
            "output": str_prop("Output file (with \"text\")."), "out_dir": str_prop("Output folder (default: Music, or next to each file).") } }), t_tts),
    tool!("image_generate", "Gerar imagem", "Create images from a text description on this PC (stable-diffusion.cpp on the GPU via Vulkan, DreamShaper 8 LCM model, 2.1 GB downloaded on first use; about 10 s per 512px image on a mid-range GPU). Write the prompt in English, or set translate to use the configured AI.",
        json!({ "type": "object", "properties": {
            "prompt": str_prop("What the image shows (English works best)."), "negative": str_prop("What to avoid (optional)."),
            "shape": enum_prop("square 512×512, portrait 512×768 or landscape 768×512.", &["square", "portrait", "landscape"], "square"),
            "quality": enum_prop("fast (4 steps) or better (8 steps, slower).", &["fast", "better"], "fast"),
            "count": int_prop("How many images.", 1, 8, 1), "seed": num_prop("Seed to repeat a result (default random)."),
            "translate": bool_prop("Translate the prompt to English with the AI configured in Settings.", false),
            "out_dir": str_prop("Folder (default: Pictures\\Conver).") }, "required": ["prompt"] }), t_image_generate),
    tool!("image_erase", "Apagar objeto", "Erase objects, people, text or watermarks from images with a local AI (LaMa inpainting, 208 MB downloaded on first use): the area is filled with what would be behind it. Give rectangles in \"regions\" or a mask image.",
        schema(json!({ "files": files_prop("images"), "regions": json!({ "type": "array", "items": { "type": "array", "items": { "type": "integer" } }, "description": "Rectangles to erase, each [x, y, width, height] in image pixels." }),
            "mask": str_prop("Mask image path (white = erase), instead of regions.") }), &["files"]), t_image_erase),
    tool!("video_animated_captions", "Legendas animadas", "Burn word-by-word animated captions (Reels/TikTok style) into videos: Whisper finds when each word is spoken (offline) and the spoken word is highlighted.",
        schema(json!({ "files": files_prop("videos"), "model": enum_prop("Whisper model: fast, balanced or accurate.", &["fast", "balanced", "accurate"], "fast"),
            "language": str_prop("Spoken language code like \"pt\" or \"auto\" (default)."),
            "style": enum_prop("highlight (word changes color), box (colored box behind the word) or word (one big word at a time).", &["highlight", "box", "word"], "highlight"),
            "position": enum_prop("Where the captions go.", &["bottom", "middle", "top"], "bottom"),
            "color": enum_prop("Highlight color.", &["yellow", "green", "red", "blue"], "yellow"),
            "words_per_line": int_prop("Words shown together.", 1, 6, 3), "uppercase": bool_prop("Uppercase text.", true),
            "keep_ass": bool_prop("Also keep the .ass subtitle file next to the video.", false) }), &["files"]), t_captions),
    tool!("pdf_organize", "Organizar páginas", "Reorder, rotate and delete PDF pages. \"order\" lists the pages of the new PDF (e.g. \"3, 1, 2, 5-8\"; pages left out are deleted, a page may repeat); \"rotate\" maps original page numbers to extra degrees (e.g. {\"2\": 90}).",
        schema(json!({ "files": files_prop("PDF (only the first is used)"), "order": str_prop("New page order, e.g. \"3, 1, 2-5\" (default: all, unchanged)."),
            "rotate": json!({ "type": "object", "description": "Extra rotation per original page number, e.g. {\"1\": 90, \"4\": 180}." }) }), &["files"]), t_pdf_organize),
    tool!("pdf_compress", "Comprimir PDF", "Make PDFs smaller by recompressing the images inside them.",
        schema(json!({ "files": files_prop("PDFs"), "level": enum_prop("Compression level.", &["light", "medium", "strong"], "medium") }), &["files"]), t_pdf_compress),
    tool!("pdf_to_images", "PDF para imagens", "Save PDF pages as PNG or JPG images (rendered by Windows).",
        schema(json!({ "files": files_prop("PDFs"), "format": enum_prop("Image format.", &["png", "jpg"], "png"), "dpi": int_prop("Resolution.", 36, 600, 150),
            "pages": str_prop("Page ranges (default all)."), "password": str_prop("Password, if the PDF is protected.") }), &["files"]), t_pdf_images),
    tool!("pdf_protect", "Proteger PDF com senha", "Protect PDFs with a password (AES-128, opens in every PDF reader).",
        schema(json!({ "files": files_prop("PDFs"), "password": str_prop("Password needed to open."), "owner_password": str_prop("Owner password (optional)."),
            "allow_print": bool_prop("Allow printing.", true), "allow_copy": bool_prop("Allow copying text.", false) }), &["files", "password"]), t_pdf_protect),
    tool!("pdf_unlock", "Tirar senha do PDF", "Remove the password of PDFs you know the password of.",
        schema(json!({ "files": files_prop("PDFs"), "password": str_prop("Current password.") }), &["files", "password"]), t_pdf_unlock),
    tool!("pdf_ocr", "PDF pesquisável (OCR)", "Make scanned PDFs searchable: adds an invisible text layer recognized by Windows OCR.",
        schema(json!({ "files": files_prop("PDFs"), "language": str_prop("OCR language tag like \"pt-BR\" (default: user languages)."),
            "skip_pages_with_text": bool_prop("Skip pages that already have text.", true) }), &["files"]), t_pdf_ocr),
    tool!("download", "Download", "Download videos (MP4) or music (MP3) from YouTube, YouTube Music, TikTok, Instagram, X and hundreds of sites (yt-dlp). Only for content you have the right to download.",
        json!({ "type": "object", "properties": { "urls": json!({ "type": "array", "items": { "type": "string" }, "description": "Links." }),
            "kind": enum_prop("mp4 (video) or mp3 (audio).", &["mp4", "mp3"], "mp4"),
            "quality": str_prop("mp4: best, 1080p, 720p, 480p, 360p. mp3: 320, 192, 128."), "playlist": bool_prop("Download the whole playlist.", false),
            "subtitles": bool_prop("Also save subtitles.", false), "cover": bool_prop("Also save the thumbnail.", false),
            "browser_cookies": str_prop("Browser to take the login from (chrome, edge, firefox) for sites that need an account."),
            "out_dir": str_prop("Folder (default: Downloads).") }, "required": ["urls"] }), t_download),
    tool!("file_info", "Informações do arquivo", "Show details of files: resolution, duration, codecs, camera, GPS, pages…",
        schema(json!({ "files": files_prop("files") }), &["files"]), t_info),
    tool!("screen_record", "Gravar a tela", "Record the screen (whole desktop, one monitor, one window or a region) to MP4 for a number of seconds, optionally with a microphone, at a chosen quality and maximum resolution.",
        json!({ "type": "object", "properties": { "seconds": num_prop("Duration in seconds."), "window": str_prop("Exact title of a window to record (see list_devices)."),
            "region": json!({ "type": "object", "properties": { "x": { "type": "integer" }, "y": { "type": "integer" }, "width": { "type": "integer" }, "height": { "type": "integer" } }, "description": "Screen region." }),
            "monitor": int_prop("Record only this monitor (1 = primary; see list_devices).", 1, 16, 1),
            "quality": enum_prop("Video quality: high (bigger file), medium or low (smaller file).", &["high", "medium", "low"], "medium"),
            "resolution": enum_prop("Maximum resolution (keeps the aspect ratio).", &["original", "1080p", "720p"], "original"),
            "fps": int_prop("Frames per second.", 5, 60, 30), "microphone": str_prop("Microphone name (see list_devices)."),
            "show_cursor": bool_prop("Show the mouse pointer.", true), "output": str_prop("Output MP4 path (default: Videos folder)."), "out_dir": str_prop("Output folder.") },
            "required": ["seconds"] }), t_screen_record),
    tool!("list_devices", "Microfones e janelas", "List microphones, visible window titles and monitors (for screen_record).",
        json!({ "type": "object", "properties": {} }), t_list_devices),
    tool!("preset_list", "Listar predefinições", "List the saved presets (favorite settings) and what tool/arguments each one uses.",
        json!({ "type": "object", "properties": {} }), t_preset_list),
    tool!("preset_run", "Usar predefinição", "Run a saved preset on files. Extra arguments override the preset's.",
        json!({ "type": "object", "properties": { "name": str_prop("Preset name."), "files": files_prop("input files"), "out_dir": str_prop("Output folder.") },
            "required": ["name"] }), t_preset_run),
    tool!("flow_list", "Listar fluxos", "List the automation flows (node graphs) saved in Conver.",
        json!({ "type": "object", "properties": {} }), t_flow_list),
    tool!("flow_get", "Ver fluxo", "Get a flow as JSON (nodes, their args and positions, links), to inspect or edit it with flow_save.",
        json!({ "type": "object", "properties": { "name": str_prop("Flow name.") }, "required": ["name"] }), t_flow_get),
    tool!("flow_node_types", "Tipos de node", "Describe every node kind a flow can use (inputs/outputs, tools with their args, AI steps, filters) plus an example flow. Read this before flow_save.",
        json!({ "type": "object", "properties": {} }), t_flow_node_types),
    tool!("flow_save", "Salvar fluxo", "Create or replace (by name) an automation flow. It is validated (kinds, tool names, links, no cycles) and shows up in the app's Automation screen.",
        json!({ "type": "object", "properties": { "flow": json!({ "type": "object", "description": "The flow: {name, description, nodes: [{id, kind, title, args, pos: [x, y]}], links: [{from, port, to}]}." }),
            "rename_from": str_prop("Old name, when renaming an existing flow.") }, "required": ["flow"] }), t_flow_save),
    tool!("flow_delete", "Apagar fluxo", "Delete a saved flow.",
        json!({ "type": "object", "properties": { "name": str_prop("Flow name.") }, "required": ["name"] }), t_flow_delete),
    tool!("flow_run", "Executar fluxo", "Run a saved flow. \"files\" replace the files saved in its input nodes; \"texts\" are extra texts for them. Returns what each node did and the final files/texts.",
        json!({ "type": "object", "properties": { "name": str_prop("Flow name."), "files": files_prop("input files"),
            "texts": json!({ "type": "array", "items": { "type": "string" }, "description": "Extra input texts (e.g. links)." }),
            "out_dir": str_prop("Folder for output nodes that have no folder set.") }, "required": ["name"] }), t_flow_run),
    tool!("preset_save", "Salvar predefinição", "Save (or replace) a preset: a tool name plus its arguments (without files).",
        json!({ "type": "object", "properties": { "name": str_prop("Preset name."), "tool": str_prop("Tool name, e.g. video_compress."),
            "args": json!({ "type": "object", "description": "Arguments for the tool." }), "description": str_prop("Short description.") },
            "required": ["name", "tool", "args"] }), t_preset_save),
];

pub fn find(name: &str) -> Option<&'static Tool> {
    let n = name.trim().to_lowercase().replace('-', "_");
    TOOLS.iter().find(|t| t.name == n)
}

/// Executa uma ferramenta pelo nome.
pub fn call(name: &str, args: &Value, ctx: &Ctx) -> Result<Value, String> {
    let tool = find(name).ok_or_else(|| format!("ferramenta desconhecida: {name}"))?;
    let empty = Map::new();
    let map = match args {
        Value::Object(m) => m,
        Value::Null => &empty,
        _ => return Err("os argumentos devem ser um objeto JSON.".into()),
    };
    (tool.run)(&Args(map), ctx)
}
