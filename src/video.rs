//! Conversão e compressão de vídeo com o ffmpeg.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::tools;

pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "ts", "gif",
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VFmt {
    Mp4,
    Mkv,
    Avi,
    Mov,
    Webm,
    Flv,
    Wmv,
    Gif,
    Mp3,
}

impl VFmt {
    pub const ALL: [VFmt; 9] =
        [VFmt::Mp4, VFmt::Mkv, VFmt::Avi, VFmt::Mov, VFmt::Webm, VFmt::Flv, VFmt::Wmv, VFmt::Gif, VFmt::Mp3];

    pub fn label(self) -> &'static str {
        match self {
            VFmt::Mp4 => "MP4",
            VFmt::Mkv => "MKV",
            VFmt::Avi => "AVI",
            VFmt::Mov => "MOV",
            VFmt::Webm => "WEBM",
            VFmt::Flv => "FLV",
            VFmt::Wmv => "WMV",
            VFmt::Gif => "GIF",
            VFmt::Mp3 => "MP3 (só áudio)",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VQuality {
    Alta,
    Media,
    Baixa,
}

impl VQuality {
    pub const ALL: [VQuality; 3] = [VQuality::Alta, VQuality::Media, VQuality::Baixa];

    pub fn label(self) -> &'static str {
        match self {
            VQuality::Alta => "Alta",
            VQuality::Media => "Média",
            VQuality::Baixa => "Baixa",
        }
    }

    fn crf(self) -> u32 {
        match self {
            VQuality::Alta => 18,
            VQuality::Media => 23,
            VQuality::Baixa => 28,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VLevel {
    Leve,
    Media,
    Forte,
}

impl VLevel {
    pub const ALL: [VLevel; 3] = [VLevel::Leve, VLevel::Media, VLevel::Forte];

    pub fn label(self) -> &'static str {
        match self {
            VLevel::Leve => "Leve",
            VLevel::Media => "Média",
            VLevel::Forte => "Forte",
        }
    }

    fn crf(self, codec: Codec) -> u32 {
        let base = match self {
            VLevel::Leve => 24,
            VLevel::Media => 28,
            VLevel::Forte => 32,
        };
        if codec == Codec::H265 { base + 2 } else { base }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Res {
    Original,
    P1080,
    P720,
    P480,
}

impl Res {
    pub const ALL: [Res; 4] = [Res::Original, Res::P1080, Res::P720, Res::P480];

    pub fn label(self) -> &'static str {
        match self {
            Res::Original => "Original",
            Res::P1080 => "1080p",
            Res::P720 => "720p",
            Res::P480 => "480p",
        }
    }

    fn lines(self) -> Option<u32> {
        match self {
            Res::Original => None,
            Res::P1080 => Some(1080),
            Res::P720 => Some(720),
            Res::P480 => Some(480),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    H265,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CompressMode {
    Level(VLevel),
    TargetMb(f64),
}

#[derive(Clone, Copy, PartialEq)]
pub struct CompressOpts {
    pub mode: CompressMode,
    pub res: Res,
    pub codec: Codec,
    pub audio_kbps: u32,
}

fn ffmpeg() -> Result<PathBuf, String> {
    tools::find_tool("ffmpeg").ok_or_else(|| "ffmpeg não encontrado (deveria estar na pasta bin).".to_string())
}

/// Informações básicas de um arquivo de mídia.
#[derive(Clone, Copy, Default, Debug)]
pub struct Probe {
    pub duration: Option<f64>,
    pub has_video: bool,
    pub has_audio: bool,
    pub size: Option<(u32, u32)>,
}

/// Lê duração, faixas e resolução da saída do `ffmpeg -i`.
pub fn probe(src: &Path) -> Probe {
    let Some(out) = ffmpeg().ok().and_then(|f| tools::command(&f).args(["-hide_banner", "-i"]).arg(src).output().ok()) else {
        return Probe::default();
    };
    let text = String::from_utf8_lossy(&out.stderr);
    let mut p = Probe::default();
    if let Some(i) = text.find("Duration: ") {
        p.duration = text.get(i + 10..i + 21).and_then(parse_time);
    }
    for line in text.lines().filter(|l| l.trim_start().starts_with("Stream #")) {
        if line.contains("Audio:") {
            p.has_audio = true;
        }
        // capas de MP3 aparecem como vídeo "attached pic"; não contam
        if line.contains("Video:") && !line.contains("attached pic") {
            p.has_video = true;
            if p.size.is_none() {
                p.size = line.split([' ', ',']).find_map(|tok| {
                    let (a, b) = tok.split_once('x')?;
                    if a.len() < 2 || b.is_empty() {
                        return None;
                    }
                    Some((a.parse().ok()?, b.parse().ok()?))
                });
            }
        }
    }
    p
}

pub fn duration(src: &Path) -> Option<f64> {
    probe(src).duration
}

/// Aceita "90", "1:30", "01:02:03" e decimais ("1:30.5" ou "1:30,5").
pub fn parse_timecode(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    if s.is_empty() {
        return None;
    }
    let mut total = 0.0;
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    for part in parts {
        let v: f64 = part.trim().parse().ok().filter(|v: &f64| *v >= 0.0)?;
        total = total * 60.0 + v;
    }
    Some(total)
}

pub fn fmt_time(secs: f64) -> String {
    let t = secs.max(0.0).round() as u64;
    let (h, m, s) = (t / 3600, (t % 3600) / 60, t % 60);
    if h > 0 { format!("{h:02}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Como `fmt_time`, com décimos de segundo quando houver ("01:30.5"). `parse_timecode` lê de volta.
pub fn fmt_time_fine(secs: f64) -> String {
    let tenths = (secs.max(0.0) * 10.0).round() as u64;
    let base = fmt_time((tenths / 10) as f64);
    if tenths % 10 == 0 { base } else { format!("{base}.{}", tenths % 10) }
}

/// Como `fmt_time`, sem arredondar: guarda até os milissegundos ("01:30.125").
pub fn fmt_time_exact(secs: f64) -> String {
    let ms = (secs.max(0.0) * 1000.0).round() as u64;
    let base = fmt_time((ms / 1000) as f64);
    if ms % 1000 == 0 { base } else { format!("{base}.{}", format!("{:03}", ms % 1000).trim_end_matches('0')) }
}

fn parse_time(t: &str) -> Option<f64> {
    let mut parts = t.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let s: f64 = parts.next()?.trim().parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// Roda o ffmpeg informando o progresso (0..1) em `on_progress`.
pub(crate) fn run(
    args: Vec<String>,
    dur: Option<f64>,
    workdir: Option<&Path>,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let mut cmd = tools::command(&ffmpeg()?);
    cmd.args(["-hide_banner", "-y", "-nostats", "-loglevel", "error", "-progress", "pipe:1"])
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = workdir {
        cmd.current_dir(d);
    }
    let mut child = cmd.spawn().map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    let mut stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cancelado.".into());
        }
        if let (Some(us), Some(d)) = (line.strip_prefix("out_time_us="), dur) {
            if let Ok(us) = us.trim().parse::<f64>() {
                on_progress(((us / 1_000_000.0) / d).clamp(0.0, 1.0) as f32);
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let errors = err_thread.join().unwrap_or_default();
    if status.success() {
        Ok(())
    } else {
        let tail: Vec<&str> = errors.lines().rev().take(4).collect();
        Err(format!("falha no ffmpeg: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" | ")))
    }
}

pub(crate) fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn convert_args(fmt: VFmt, q: VQuality) -> (&'static str, Vec<String>) {
    let crf = q.crf().to_string();
    let q_old = (q.crf() / 6).max(2).to_string(); // escala do mpeg4/wmv2
    match fmt {
        VFmt::Mp4 | VFmt::Mov => (
            if fmt == VFmt::Mp4 { "mp4" } else { "mov" },
            s(&["-c:v", "libx264", "-preset", "medium", "-crf", &crf, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a",
                "192k", "-movflags", "+faststart"]),
        ),
        VFmt::Mkv => ("mkv", s(&["-c:v", "libx264", "-preset", "medium", "-crf", &crf, "-c:a", "aac", "-b:a", "192k"])),
        VFmt::Webm => (
            "webm",
            s(&["-c:v", "libvpx-vp9", "-crf", &(q.crf() + 10).to_string(), "-b:v", "0", "-row-mt", "1", "-deadline",
                "good", "-cpu-used", "4", "-c:a", "libopus", "-b:a", "160k"]),
        ),
        VFmt::Avi => ("avi", s(&["-c:v", "mpeg4", "-q:v", &q_old, "-c:a", "libmp3lame", "-b:a", "192k"])),
        VFmt::Flv => ("flv", s(&["-c:v", "libx264", "-crf", &crf, "-c:a", "aac", "-b:a", "160k"])),
        VFmt::Wmv => ("wmv", s(&["-c:v", "wmv2", "-q:v", &q_old, "-c:a", "wmav2", "-b:a", "192k"])),
        VFmt::Gif => (
            "gif",
            s(&["-vf", "fps=12,scale='min(640,iw)':-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse",
                "-loop", "0"]),
        ),
        VFmt::Mp3 => {
            let br = match q {
                VQuality::Alta => "320k",
                VQuality::Media => "192k",
                VQuality::Baixa => "128k",
            };
            ("mp3", s(&["-vn", "-c:a", "libmp3lame", "-b:a", br]))
        }
    }
}

pub fn convert(
    src: &Path,
    out_dir: Option<&Path>,
    fmt: VFmt,
    q: VQuality,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let (ext, codec_args) = convert_args(fmt, q);
    let suffix = if tools::ext_lower(src) == ext { "_convertido" } else { "" };
    let dst = tools::out_path(src, out_dir, suffix, ext);
    let mut args = vec!["-i".to_string(), src.to_string_lossy().into_owned()];
    args.extend(codec_args);
    args.push(dst.to_string_lossy().into_owned());
    if let Err(e) = run(args, duration(src), None, on_progress, cancelled) {
        let _ = std::fs::remove_file(&dst);
        return Err(e);
    }
    Ok((tools::savings(tools::file_size(src), tools::file_size(&dst)), dst))
}

/// Filtro que limita o lado menor do vídeo (funciona com vídeos em pé e deitados), sem ampliar.
fn scale_filter(lines: u32) -> String {
    format!("scale='if(gte(iw,ih),-2,min({lines},iw))':'if(gte(iw,ih),min({lines},ih),-2)'")
}

pub fn compress(
    src: &Path,
    out_dir: Option<&Path>,
    opts: CompressOpts,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let dst = tools::out_path(src, out_dir, "_comprimido", "mp4");
    let dur = duration(src);
    let input = vec!["-i".to_string(), src.to_string_lossy().into_owned()];
    let mut common = Vec::new();
    if let Some(l) = opts.res.lines() {
        common.extend(["-vf".to_string(), scale_filter(l)]);
    }
    let codec = match opts.codec {
        Codec::H264 => s(&["-c:v", "libx264", "-preset", "medium", "-pix_fmt", "yuv420p"]),
        Codec::H265 => s(&["-c:v", "libx265", "-preset", "medium", "-pix_fmt", "yuv420p", "-tag:v", "hvc1"]),
    };
    common.extend(codec);
    let audio = s(&["-c:a", "aac", "-b:a", &format!("{}k", opts.audio_kbps), "-movflags", "+faststart"]);
    let dst_s = dst.to_string_lossy().into_owned();

    let result = match opts.mode {
        CompressMode::Level(level) => {
            let mut args = input.clone();
            args.extend(common.clone());
            args.extend(["-crf".to_string(), level.crf(opts.codec).to_string()]);
            if opts.codec == Codec::H265 {
                args.extend(s(&["-x265-params", "log-level=error"]));
            }
            args.extend(audio.clone());
            args.push(dst_s.clone());
            run(args, dur, None, on_progress, cancelled)
        }
        CompressMode::TargetMb(mb) => {
            let d = dur.ok_or("não foi possível ler a duração do vídeo.")?;
            let total_kbps = mb * 8192.0 / d;
            let video_kbps = (total_kbps - opts.audio_kbps as f64).floor();
            if video_kbps < 50.0 {
                return Err(format!(
                    "{mb} MB é pequeno demais para {:.0}s de vídeo (mínimo ≈ {:.1} MB).",
                    d,
                    (50.0 + opts.audio_kbps as f64) * d / 8192.0
                ));
            }
            let bitrate = format!("{video_kbps}k");
            // dois passos: o 1º analisa o vídeo, o 2º grava com o tamanho certo
            let tmp = std::env::temp_dir();
            let log_name = format!("conver_2pass_{}", std::process::id());
            let pass = |n: u32| -> Vec<String> {
                match opts.codec {
                    Codec::H264 => s(&["-pass", &n.to_string(), "-passlogfile", &log_name]),
                    Codec::H265 => s(&["-x265-params", &format!("pass={n}:stats={log_name}.log:log-level=error")]),
                }
            };
            let mut a1 = input.clone();
            a1.extend(common.clone());
            a1.extend(["-b:v".to_string(), bitrate.clone()]);
            a1.extend(pass(1));
            a1.extend(s(&["-an", "-f", "null", "NUL"]));
            let r = run(a1, dur, Some(&tmp), &|p| on_progress(p * 0.5), cancelled).and_then(|_| {
                let mut a2 = input.clone();
                a2.extend(common.clone());
                a2.extend(["-b:v".to_string(), bitrate.clone()]);
                a2.extend(pass(2));
                a2.extend(audio.clone());
                a2.push(dst_s.clone());
                run(a2, dur, Some(&tmp), &|p| on_progress(0.5 + p * 0.5), cancelled)
            });
            if let Ok(entries) = std::fs::read_dir(&tmp) {
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().starts_with(&log_name) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
            r
        }
    };
    if let Err(e) = result {
        let _ = std::fs::remove_file(&dst);
        return Err(e);
    }
    Ok((tools::savings(tools::file_size(src), tools::file_size(&dst)), dst))
}
