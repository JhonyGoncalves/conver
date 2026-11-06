//! Edição de vídeo (cortar, juntar, velocidade, áudio, frames) e ferramentas de áudio, com o ffmpeg.

use std::path::{Path, PathBuf};

use crate::tools;
use crate::video::{VQuality, duration, fmt_time, probe, run, s};

pub(crate) fn path_s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Executa o ffmpeg com `input` + `output` e grava em `dst`; apaga `dst` se der erro.
pub(crate) fn encode_to(
    input: Vec<String>,
    output: Vec<String>,
    dst: &Path,
    dur: Option<f64>,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let mut args = input;
    args.extend(output);
    args.push(path_s(dst));
    run(args, dur, None, on_progress, cancelled).inspect_err(|_| {
        let _ = std::fs::remove_file(dst);
    })
}

/// Codificação de qualidade alta para regravar um arquivo mantendo a extensão (quando possível).
fn reencode_for(src: &Path, has_video: bool) -> (String, Vec<String>) {
    let ext = tools::ext_lower(src);
    if !has_video {
        return match AFmt::from_ext(&ext) {
            Some(fmt) => (ext, audio_args(fmt, VQuality::Alta)),
            None => ("mp3".into(), audio_args(AFmt::Mp3, VQuality::Alta)),
        };
    }
    match ext.as_str() {
        "webm" => (
            "webm".into(),
            s(&["-c:v", "libvpx-vp9", "-crf", "28", "-b:v", "0", "-row-mt", "1", "-cpu-used", "4", "-c:a", "libopus", "-b:a",
                "160k"]),
        ),
        "mkv" | "mov" => (
            ext,
            s(&["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k"]),
        ),
        _ => (
            "mp4".into(),
            s(&["-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k",
                "-movflags", "+faststart"]),
        ),
    }
}

pub(crate) fn keep_or_mp4(src: &Path) -> String {
    match tools::ext_lower(src).as_str() {
        e @ ("mkv" | "mov") => e.to_string(),
        _ => "mp4".into(),
    }
}

// ============================================================ edição de vídeo

/// Salva só o trecho entre `start` e `end` (fim do arquivo se `None`).
/// Sem `precise`, copia sem recodificar: instantâneo, mas o corte cai no quadro-chave mais próximo.
pub fn trim(
    src: &Path,
    out_dir: Option<&Path>,
    start: f64,
    end: Option<f64>,
    precise: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = probe(src);
    if let Some(d) = info.duration {
        if start >= d {
            return Err(format!("o início ({}) passa do fim do arquivo ({}).", fmt_time(start), fmt_time(d)));
        }
    }
    if end.is_some_and(|e| e <= start) {
        return Err("o fim precisa ser depois do início.".into());
    }
    let stop = match (end, info.duration) {
        (Some(e), Some(d)) => Some(e.min(d)),
        (e, d) => e.or(d),
    };
    let mut input = s(&["-ss", &format!("{start:.3}")]);
    if let Some(e) = end {
        input.extend(s(&["-to", &format!("{e:.3}")]));
    }
    input.extend(["-i".to_string(), path_s(src)]);
    let (ext, codec) = if precise {
        reencode_for(src, info.has_video)
    } else {
        (tools::ext_lower(src), s(&["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"]))
    };
    let dst = tools::out_path(src, out_dir, "_trecho", &ext);
    encode_to(input, codec, &dst, stop.map(|e| e - start), on_progress, cancelled)?;
    let range = format!("{} → {}", fmt_time(start), stop.map(fmt_time).unwrap_or_else(|| "fim".into()));
    Ok((format!("{range} • {}", tools::human_size(tools::file_size(&dst))), dst))
}

/// Como cortar: copiando (rápido, cai no quadro-chave), recodificando tudo (exato) ou só as pontas de cada parte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CutMode {
    #[default]
    Rapido,
    Preciso,
    Inteligente,
}

/// Um trecho marcado: para manter ou para tirar. Sem `end`, vai até o fim do arquivo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub keep: bool,
    pub start: f64,
    pub end: Option<f64>,
}

/// Junta intervalos que se tocam ou se sobrepõem, em ordem.
fn union(mut ranges: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (a, b) in ranges {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// Tira de `part` os intervalos de `holes` (já unidos).
fn subtract(part: (f64, f64), holes: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut from = part.0;
    for &(a, b) in holes {
        if a >= part.1 {
            break;
        }
        if a > from {
            out.push((from, a));
        }
        from = from.max(b);
    }
    if from < part.1 {
        out.push((from, part.1));
    }
    out
}

/// Decide o que sai: uma lista por arquivo, cada uma com as partes dele, em ordem. O que está em "tirar" sai de tudo,
/// mesmo dentro de um trecho para manter. Sem nenhum trecho para manter, vale o arquivo inteiro. Com `together`, tudo
/// vira um arquivo só e o que se repete conta uma vez; sem ele, cada trecho é um arquivo, mesmo um dentro do outro.
pub fn plan_cuts(cuts: &[Cut], dur: f64, together: bool) -> Result<Vec<Vec<(f64, f64)>>, String> {
    let range = |c: &Cut| (c.start.min(dur), c.end.unwrap_or(dur).min(dur));
    let holes = union(cuts.iter().filter(|c| !c.keep).map(range).collect());
    let mut clips: Vec<(f64, f64)> = cuts.iter().filter(|c| c.keep).map(range).filter(|(a, b)| b > a).collect();
    if cuts.iter().all(|c| !c.keep) {
        clips.push((0.0, dur));
    }
    let lists = if together { vec![union(clips)] } else { clips.into_iter().map(|c| vec![c]).collect() };
    let plan: Vec<Vec<(f64, f64)>> = lists
        .into_iter()
        .map(|list| list.into_iter().flat_map(|c| subtract(c, &holes)).filter(|(a, b)| b - a >= 0.05).collect::<Vec<_>>())
        .filter(|list| !list.is_empty())
        .collect();
    if plan.is_empty() {
        return Err("não sobrou nenhum trecho.".into());
    }
    Ok(plan)
}

// i18n: fora
/// Aceita "0:10-1:00; 2:00-" (o fim é opcional) e devolve os trechos do tipo `keep`.
pub fn parse_cuts(text: &str, keep: bool) -> Result<Vec<Cut>, String> {
    text.split([';', ',', '\n'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            let bad = || format!("trecho inválido: \"{t}\" (use 0:10-1:00).");
            let (a, b) = t.split_once('-').ok_or_else(bad)?;
            let start = crate::video::parse_timecode(a).ok_or_else(bad)?;
            let end = if b.trim().is_empty() { None } else { Some(crate::video::parse_timecode(b).ok_or_else(bad)?) };
            if end.is_some_and(|e| e <= start) {
                return Err(bad());
            }
            Ok(Cut { keep, start, end })
        })
        .collect()
}

// i18n: dentro
/// Grava uma lista de partes em `dst`. Uma parte só é um corte comum; várias são emendadas. Com `precise`, cada uma é
/// aberta no ponto exato e tudo é recodificado; sem ele, as partes são copiadas sem perda e emendadas (cada emenda
/// cai no quadro-chave mais próximo).
pub(crate) fn write_parts(
    src: &Path,
    info: &crate::video::Probe,
    parts: &[(f64, f64)],
    dst: &Path,
    precise: bool,
    codec: Vec<String>,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let open = |(a, b): (f64, f64)| {
        let mut input = s(&["-ss", &format!("{a:.3}"), "-to", &format!("{b:.3}")]);
        input.extend(["-i".to_string(), path_s(src)]);
        input
    };
    let total: f64 = parts.iter().map(|(a, b)| b - a).sum();
    if parts.len() == 1 {
        return encode_to(open(parts[0]), codec, dst, Some(total), on_progress, cancelled);
    }
    if precise {
        let input: Vec<String> = parts.iter().flat_map(|p| open(*p)).collect();
        let (v, a) = (info.has_video, info.has_audio);
        let mut filter = String::new();
        for i in 0..parts.len() {
            if v {
                filter.push_str(&format!("[{i}:v:0]"));
            }
            if a {
                filter.push_str(&format!("[{i}:a:0]"));
            }
        }
        filter.push_str(&format!("concat=n={}:v={}:a={}", parts.len(), v as u8, a as u8));
        if v {
            filter.push_str("[v]");
        }
        if a {
            filter.push_str("[a]");
        }
        let mut output = s(&["-filter_complex", &filter]);
        if v {
            output.extend(s(&["-map", "[v]"]));
        }
        if a {
            output.extend(s(&["-map", "[a]"]));
        }
        output.extend(codec);
        return encode_to(input, output, dst, Some(total), on_progress, cancelled);
    }
    // rápido: copia cada parte para um arquivo temporário e emenda
    let dir = std::env::temp_dir().join(format!("conver_cortes_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        let ext = tools::ext_lower(src);
        let mut body = String::new();
        let mut done = 0.0;
        for (i, p) in parts.iter().enumerate() {
            let tmp = dir.join(format!("{i}.{ext}"));
            let span = p.1 - p.0;
            let step = |f: f32| on_progress(((done + span * f as f64) / total) as f32 * 0.9);
            encode_to(open(*p), codec.clone(), &tmp, Some(span), &step, cancelled)?;
            done += span;
            body.push_str(&format!("file '{}'\n", path_s(&tmp).replace('\'', "'\\''")));
        }
        let list = dir.join("lista.txt");
        std::fs::write(&list, body).map_err(|e| e.to_string())?;
        encode_to(s(&["-f", "concat", "-safe", "0", "-i", &path_s(&list)]), s(&["-map", "0", "-c", "copy"]), dst, None, &|_| {}, cancelled)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Vários trechos de um arquivo, conforme `plan_cuts`. Um trecho só continua sendo um `trim` comum.
pub fn cut(
    src: &Path,
    out_dir: Option<&Path>,
    cuts: &[Cut],
    together: bool,
    mode: CutMode,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    if let ([c], true) = (cuts, mode != CutMode::Inteligente) {
        if c.keep {
            return trim(src, out_dir, c.start, c.end, mode == CutMode::Preciso, on_progress, cancelled);
        }
    }
    let info = probe(src);
    let dur = info.duration.ok_or("não foi possível ler a duração do arquivo.")?;
    let plan = plan_cuts(cuts, dur, together)?;
    // o modo Inteligente só vale onde a emenda é segura; no resto, e se a conferência final reprovar, vai o Preciso
    let (smart, mut fallback) = match mode {
        CutMode::Inteligente => match crate::smartcut::check(src) {
            Ok(p) => (Some(p), None),
            Err(e) => (None, Some(e)),
        },
        _ => (None, None),
    };
    let precise = mode != CutMode::Rapido;
    let (ext, codec) = if precise {
        reencode_for(src, info.has_video)
    } else {
        (tools::ext_lower(src), s(&["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"]))
    };
    let length = |list: &[(f64, f64)]| list.iter().map(|(a, b)| b - a).sum::<f64>();
    let total: f64 = plan.iter().map(|l| length(l)).sum();
    let mut done = 0.0;
    let mut last = None;
    let mut size = 0;
    for (i, list) in plan.iter().enumerate() {
        let suffix = if plan.len() == 1 { "_cortado".to_string() } else { format!("_trecho_{}", i + 1) };
        let dst = tools::out_path(src, out_dir, &suffix, &ext);
        let span = length(list);
        let step = |f: f32| on_progress(((done + span * f as f64) / total) as f32);
        let done_smart = match &smart {
            Some(p) => match crate::smartcut::write(src, &info, p, list, &dst, &step, cancelled) {
                Ok(()) => true,
                Err(_) if cancelled() => return Err("Cancelado.".into()),
                Err(e) => {
                    fallback = Some(e);
                    false
                }
            },
            None => false,
        };
        if !done_smart {
            write_parts(src, &info, list, &dst, precise, codec.clone(), &step, cancelled)?;
        }
        done += span;
        size += tools::file_size(&dst);
        last = Some(dst);
    }
    let dst = last.ok_or("não sobrou nenhum trecho.")?;
    let mut text = format!("{} arquivo(s) • {} • {}", plan.len(), fmt_time(total), tools::human_size(size));
    if let Some(why) = fallback {
        let note = format!("feito no modo Preciso: {}", why);
        text.push_str(" • ");
        text.push_str(&note);
    }
    Ok((text, dst))
}

/// Um quadro do vídeo, com `height` pixels de altura.
pub fn frame_at(src: &Path, at: f64, height: u32) -> Result<image::RgbaImage, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let out = tools::command(&ff)
        .args(["-v", "error", "-nostdin", "-ss", &format!("{at:.2}"), "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", &format!("scale=-2:{height}"), "-f", "image2pipe", "-c:v", "png", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    image::load_from_memory(&out.stdout).map(|i| i.to_rgba8()).map_err(|e| e.to_string())
}

/// Junta os vídeos na ordem dada. Com `precise`, recodifica tudo (aceita vídeos diferentes entre si);
/// sem ele, só emenda (todos precisam ter o mesmo formato, resolução e codificação).
pub fn merge(
    files: &[PathBuf],
    out_dir: Option<&Path>,
    precise: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    if files.len() < 2 {
        return Err("adicione pelo menos 2 vídeos.".into());
    }
    let infos: Vec<_> = files.iter().map(|f| probe(f)).collect();
    if let Some(i) = infos.iter().position(|i| !i.has_video) {
        return Err(format!("{} não tem vídeo.", tools::file_name(&files[i])));
    }
    let total: f64 = infos.iter().filter_map(|i| i.duration).sum();
    let first = &files[0];
    let done = |dst: &Path, extra: &str| {
        format!("{} vídeos • {}{extra} • {}", files.len(), fmt_time(total), tools::human_size(tools::file_size(dst)))
    };

    if !precise {
        let list = std::env::temp_dir().join(format!("conver_concat_{}.txt", std::process::id()));
        let body: String = files.iter().map(|f| format!("file '{}'\n", path_s(f).replace('\'', "'\\''"))).collect();
        std::fs::write(&list, body).map_err(|e| e.to_string())?;
        let dst = tools::out_path(first, out_dir, "_juntado", &tools::ext_lower(first));
        let r = encode_to(
            s(&["-f", "concat", "-safe", "0", "-i", &path_s(&list)]),
            s(&["-c", "copy"]),
            &dst,
            Some(total),
            on_progress,
            cancelled,
        );
        let _ = std::fs::remove_file(&list);
        r.map_err(|e| format!("{e} — se os vídeos forem diferentes entre si, use \"Recodificar\""))?;
        return Ok((done(&dst, ""), dst));
    }

    // resolução do primeiro vídeo (números pares); os outros se encaixam com bordas
    let (w, h) = infos[0].size.unwrap_or((1280, 720));
    let (w, h) = ((w / 2 * 2).max(2), (h / 2 * 2).max(2));
    let mut input = Vec::new();
    for f in files {
        input.extend(["-i".to_string(), path_s(f)]);
    }
    // vídeos sem áudio recebem silêncio, senão a junção falha
    let mut extra_idx = files.len();
    let mut filter = String::new();
    for (i, info) in infos.iter().enumerate() {
        filter.push_str(&format!(
            "[{i}:v]scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=30,format=yuv420p[v{i}];"
        ));
        if info.has_audio {
            filter.push_str(&format!("[{i}:a]aresample=48000,aformat=channel_layouts=stereo[a{i}];"));
        } else {
            let d = info.duration.unwrap_or(1.0);
            input.extend(s(&["-f", "lavfi", "-t", &format!("{d:.3}"), "-i", "anullsrc=r=48000:cl=stereo"]));
            filter.push_str(&format!("[{extra_idx}:a]anull[a{i}];"));
            extra_idx += 1;
        }
    }
    for i in 0..files.len() {
        filter.push_str(&format!("[v{i}][a{i}]"));
    }
    filter.push_str(&format!("concat=n={}:v=1:a=1[v][a]", files.len()));
    let dst = tools::out_path(first, out_dir, "_juntado", "mp4");
    encode_to(
        input,
        s(&["-filter_complex", &filter, "-map", "[v]", "-map", "[a]", "-c:v", "libx264", "-preset", "medium", "-crf", "20",
            "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]),
        &dst,
        Some(total),
        on_progress,
        cancelled,
    )?;
    Ok((done(&dst, &format!(" • {w}×{h}")), dst))
}

/// Cadeia de `atempo` (cada um aceita de 0,5 a 2).
fn atempo_chain(mut f: f64) -> String {
    let mut parts = Vec::new();
    while f > 2.0 {
        parts.push("atempo=2.0".to_string());
        f /= 2.0;
    }
    while f < 0.5 {
        parts.push("atempo=0.5".to_string());
        f /= 0.5;
    }
    parts.push(format!("atempo={f:.4}"));
    parts.join(",")
}

pub fn speed_label(factor: f64) -> String {
    format!("{}", (factor * 100.0).round() / 100.0).replace('.', ",")
}

pub fn speed(
    src: &Path,
    out_dir: Option<&Path>,
    factor: f64,
    keep_audio: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = probe(src);
    if !info.has_video {
        return Err("o arquivo não tem vídeo.".into());
    }
    let label = speed_label(factor);
    let dst = tools::out_path(src, out_dir, &format!("_{label}x"), &keep_or_mp4(src));
    let mut out = s(&["-filter:v", &format!("setpts=PTS/{factor:.4}"), "-c:v", "libx264", "-preset", "medium", "-crf", "20",
                      "-pix_fmt", "yuv420p"]);
    if keep_audio && info.has_audio {
        out.extend(s(&["-filter:a", &atempo_chain(factor), "-c:a", "aac", "-b:a", "192k"]));
    } else {
        out.push("-an".into());
    }
    encode_to(s(&["-i", &path_s(src)]), out, &dst, info.duration.map(|d| d / factor), on_progress, cancelled)?;
    let d = info.duration.map(|d| format!(" • {} → {}", fmt_time(d), fmt_time(d / factor))).unwrap_or_default();
    Ok((format!("{label}×{d}"), dst))
}

pub fn remove_audio(
    src: &Path,
    out_dir: Option<&Path>,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let dst = tools::out_path(src, out_dir, "_mudo", &tools::ext_lower(src));
    encode_to(
        s(&["-i", &path_s(src)]),
        s(&["-map", "0", "-map", "-0:a", "-c", "copy"]),
        &dst,
        duration(src),
        on_progress,
        cancelled,
    )?;
    Ok((format!("áudio removido • {}", tools::savings(tools::file_size(src), tools::file_size(&dst))), dst))
}

/// Salva uma imagem a cada `every` segundos numa pasta `<nome>_frames`.
pub fn extract_frames(
    src: &Path,
    out_dir: Option<&Path>,
    every: f64,
    png: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = out_dir.map(Path::to_path_buf).or_else(|| src.parent().map(Path::to_path_buf)).unwrap_or_default();
    let folder = tools::unique_path(parent.join(format!("{stem}_frames")));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let ext = if png { "png" } else { "jpg" };
    let mut out = s(&["-vf", &format!("fps=1/{every:.4}")]);
    if !png {
        out.extend(s(&["-q:v", "2"]));
    }
    let pattern = folder.join(format!("frame_%04d.{ext}"));
    if let Err(e) = encode_to(s(&["-i", &path_s(src)]), out, &pattern, duration(src), on_progress, cancelled) {
        let _ = std::fs::remove_dir_all(&folder);
        return Err(e);
    }
    let n = std::fs::read_dir(&folder).map(|r| r.count()).unwrap_or(0);
    Ok((format!("{n} imagens na pasta {}", tools::file_name(&folder)), folder.join(format!("frame_0001.{ext}"))))
}

// ===================================================================== áudio

/// Áudio e vídeo (as ferramentas de áudio também extraem o som de vídeos).
pub const MEDIA_EXTS: &[&str] = &[
    "mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "opus", "wma", "aiff", "aif", "amr", "mp4", "mkv", "avi", "mov",
    "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "ts",
];

pub const AUDIO_EXTS: &[&str] = &["mp3", "wav", "flac", "ogg", "oga", "m4a", "aac", "opus", "wma", "aiff", "aif", "amr"];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AFmt {
    Mp3,
    Wav,
    Flac,
    Ogg,
    M4a,
    Opus,
}

impl AFmt {
    pub const ALL: [AFmt; 6] = [AFmt::Mp3, AFmt::Wav, AFmt::Flac, AFmt::Ogg, AFmt::M4a, AFmt::Opus];

    pub fn label(self) -> &'static str {
        match self {
            AFmt::Mp3 => "MP3",
            AFmt::Wav => "WAV",
            AFmt::Flac => "FLAC",
            AFmt::Ogg => "OGG",
            AFmt::M4a => "M4A (AAC)",
            AFmt::Opus => "OPUS",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            AFmt::Mp3 => "mp3",
            AFmt::Wav => "wav",
            AFmt::Flac => "flac",
            AFmt::Ogg => "ogg",
            AFmt::M4a => "m4a",
            AFmt::Opus => "opus",
        }
    }

    pub fn lossless(self) -> bool {
        matches!(self, AFmt::Wav | AFmt::Flac)
    }

    fn from_ext(ext: &str) -> Option<AFmt> {
        Some(match ext {
            "mp3" => AFmt::Mp3,
            "wav" => AFmt::Wav,
            "flac" => AFmt::Flac,
            "ogg" | "oga" => AFmt::Ogg,
            "m4a" | "aac" => AFmt::M4a,
            "opus" => AFmt::Opus,
            _ => return None,
        })
    }
}

pub(crate) fn audio_args(fmt: AFmt, q: VQuality) -> Vec<String> {
    let pick = |a: &str, m: &str, b: &str| {
        match q {
            VQuality::Alta => a,
            VQuality::Media => m,
            VQuality::Baixa => b,
        }
        .to_string()
    };
    let mut v = s(&["-vn", "-map_metadata", "0"]);
    v.extend(match fmt {
        AFmt::Mp3 => s(&["-c:a", "libmp3lame", "-b:a", &pick("320k", "192k", "128k")]),
        AFmt::Wav => s(&["-c:a", "pcm_s16le"]),
        AFmt::Flac => s(&["-c:a", "flac", "-compression_level", "8"]),
        AFmt::Ogg => s(&["-c:a", "libvorbis", "-q:a", &pick("8", "5", "3")]),
        AFmt::M4a => s(&["-c:a", "aac", "-b:a", &pick("256k", "192k", "128k")]),
        AFmt::Opus => s(&["-c:a", "libopus", "-b:a", &pick("192k", "128k", "96k")]),
    });
    v
}

fn require_audio(src: &Path) -> Result<crate::video::Probe, String> {
    let info = probe(src);
    if info.has_audio { Ok(info) } else { Err("o arquivo não tem áudio.".into()) }
}

pub fn audio_convert(
    src: &Path,
    out_dir: Option<&Path>,
    fmt: AFmt,
    q: VQuality,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = require_audio(src)?;
    let suffix = if tools::ext_lower(src) == fmt.ext() { "_convertido" } else { "" };
    let dst = tools::out_path(src, out_dir, suffix, fmt.ext());
    encode_to(s(&["-i", &path_s(src)]), audio_args(fmt, q), &dst, info.duration, on_progress, cancelled)?;
    Ok((tools::savings(tools::file_size(src), tools::file_size(&dst)), dst))
}

/// Ajusta o volume para o nível pedido (LUFS) com o filtro loudnorm. Em vídeos, a imagem é copiada sem perda.
pub fn normalize(
    src: &Path,
    out_dir: Option<&Path>,
    lufs: i32,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = require_audio(src)?;
    let (ext, mut out) = if info.has_video {
        (keep_or_mp4(src), s(&["-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]))
    } else {
        reencode_for(src, false)
    };
    out.extend(s(&["-af", &format!("loudnorm=I={lufs}:TP=-1.5:LRA=11"), "-ar", "48000"]));
    let dst = tools::out_path(src, out_dir, "_normalizado", &ext);
    encode_to(s(&["-i", &path_s(src)]), out, &dst, info.duration, on_progress, cancelled)?;
    Ok((format!("volume ajustado para {lufs} LUFS"), dst))
}

/// Força da limpeza de ruído.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Denoise {
    /// Filtro de espectro (afftdn): tira chiado constante sem mexer na voz.
    Leve,
    /// Rede neural RNNoise (arnndn), treinada para voz gravada: tira ventilador, rua, teclado…
    Media,
    /// RNNoise mais o filtro de espectro, para gravações bem ruidosas.
    Forte,
}

/// Modelo do RNNoise (vai junto no download, em `modelos`).
pub const RNNOISE_MODEL: &str = "rnnoise-sh.rnnn";

/// Remove ruído de fundo de áudios e vídeos (em vídeos, a imagem é copiada sem perda).
pub fn denoise(
    src: &Path,
    out_dir: Option<&Path>,
    level: Denoise,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = require_audio(src)?;
    let (ext, mut out) = if info.has_video {
        (keep_or_mp4(src), s(&["-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]))
    } else {
        reencode_for(src, false)
    };
    // o caminho vai dentro do filtro: barras normais, ":" e "'" escapados
    let rnn = |m: &Path| format!("arnndn=m='{}'", m.display().to_string().replace('\\', "/").replace(':', "\\:").replace('\'', "\\'"));
    let model = crate::ai::find_model(RNNOISE_MODEL);
    let (af, label) = match (level, model.as_deref()) {
        (Denoise::Media, Some(m)) => (format!("highpass=f=70,{}", rnn(m)), "ruído removido (IA)"),
        (Denoise::Forte, Some(m)) => (format!("highpass=f=80,{},afftdn=nr=10:nf=-45", rnn(m)), "ruído removido (forte)"),
        // tn=1: acompanha o ruído ao longo do arquivo (nf é só o ponto de partida)
        _ => ("highpass=f=70,afftdn=nr=18:nf=-30:tn=1".to_string(), "chiado removido"),
    };
    out.extend(s(&["-af", &af]));
    let dst = tools::out_path(src, out_dir, "_sem_ruido", &ext);
    encode_to(s(&["-i", &path_s(src)]), out, &dst, info.duration, on_progress, cancelled)?;
    Ok((label.to_string(), dst))
}

/// Voz pronta para podcast: limpa o ruído (opcional, com a IA do RNNoise), tira graves que embolam, dá presença,
/// suaviza os "s", comprime e deixa no volume padrão (`lufs`). Em vídeos, a imagem é copiada sem perda.
pub fn enhance_voice(
    src: &Path,
    out_dir: Option<&Path>,
    clean: bool,
    lufs: i32,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = require_audio(src)?;
    let (ext, mut out) = if info.has_video {
        (keep_or_mp4(src), s(&["-map", "0:v:0", "-map", "0:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k"]))
    } else {
        reencode_for(src, false)
    };
    let mut chain = vec!["highpass=f=80".to_string()];
    if clean {
        match crate::ai::find_model(RNNOISE_MODEL) {
            Some(m) => chain.push(format!("arnndn=m='{}'", m.display().to_string().replace('\\', "/").replace(':', "\\:").replace('\'', "\\'"))),
            None => chain.push("afftdn=nr=12:nf=-35:tn=1".into()),
        }
    }
    // i18n: fora
    chain.extend([
        "equalizer=f=250:t=q:w=1.2:g=-3".to_string(),   // menos "embolado"
        "equalizer=f=3200:t=q:w=1.4:g=3".to_string(),   // presença: a voz vem para a frente
        "deesser=i=0.4".to_string(),                     // "s" menos chiados
        "acompressor=threshold=-21dB:ratio=3:attack=8:release=120:makeup=3".to_string(),
        format!("loudnorm=I={lufs}:TP=-1.5:LRA=9"),
        "aresample=48000".to_string(),
    ]);
    // i18n: dentro
    out.extend(s(&["-af", &chain.join(",")]));
    let dst = tools::out_path(src, out_dir, "_voz", &ext);
    encode_to(s(&["-i", &path_s(src)]), out, &dst, info.duration, on_progress, cancelled)?;
    Ok((format!("voz melhorada • {lufs} LUFS"), dst))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ACodec {
    Mp3,
    Aac,
    Opus,
}

pub fn audio_compress(
    src: &Path,
    out_dir: Option<&Path>,
    codec: ACodec,
    kbps: u32,
    mono: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = require_audio(src)?;
    let (ext, c) = match codec {
        ACodec::Mp3 => ("mp3", "libmp3lame"),
        ACodec::Aac => ("m4a", "aac"),
        ACodec::Opus => ("opus", "libopus"),
    };
    let mut out = s(&["-vn", "-map_metadata", "0", "-c:a", c, "-b:a", &format!("{kbps}k")]);
    if mono {
        out.extend(s(&["-ac", "1"]));
    }
    let dst = tools::out_path(src, out_dir, "_comprimido", ext);
    encode_to(s(&["-i", &path_s(src)]), out, &dst, info.duration, on_progress, cancelled)?;
    Ok((tools::savings(tools::file_size(src), tools::file_size(&dst)), dst))
}
