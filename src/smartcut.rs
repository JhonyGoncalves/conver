//! Corte inteligente: recodifica só o começo e o fim de cada parte, até o quadro-chave mais próximo, e copia o miolo sem
//! perda. Fica no ponto exato e quase tão rápido quanto copiar. Só para H.264 de 8 bits sem rotação, onde a emenda é
//! segura. As partes viram MPEG-TS, que leva os parâmetros do vídeo junto de cada quadro-chave, e depois de emendar o
//! resultado é conferido nas emendas: se algo falhar, quem chamou recorre ao corte Preciso.

use std::path::{Path, PathBuf};

use crate::media::{encode_to, path_s};
use crate::tools;
use crate::video::{Probe, s};

const R_EXT: &str = "só funciona com MP4, MOV e MKV";
const R_NONE: &str = "o arquivo não tem vídeo";
const R_CODEC: &str = "só funciona com vídeo H.264";
const R_BITS: &str = "só funciona com vídeo de 8 bits";
const R_INTER: &str = "não funciona com vídeo entrelaçado";
const R_ROT: &str = "não funciona com vídeo girado, como os de celular";
const R_PROF: &str = "perfil de H.264 não aceito";

// i18n: fora

/// `ffmpeg -i` soma cerca de 0,13 s de folga ao procurar em vídeos com quadros B (`seek_timestamp -= 3/23 s`): para cair
/// exatamente num quadro-chave ao copiar, o pedido vai adiantado nessa medida.
const SEEK_SLACK: f64 = 0.1314;
/// Parte mais curta que vale copiar: menos que isso vai tudo recodificado.
const MIN_COPY: f64 = 0.5;

/// O que a cabeça e o fim recodificados precisam ter em comum com o vídeo.
pub struct Params {
    profile: &'static str,
    color: Vec<String>,
}

fn run_log(args: &[String]) -> Result<std::process::Output, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    tools::command(&ff).args(args).output().map_err(|e| e.to_string())
}

/// Confere se este vídeo pode ser cortado assim. O erro diz por que não.
pub fn check(src: &Path) -> Result<Params, String> {
    if !matches!(tools::ext_lower(src).as_str(), "mp4" | "mov" | "mkv") {
        return Err(R_EXT.into());
    }
    let out = run_log(&s(&["-hide_banner", "-i", &path_s(src)]))?;
    let text = String::from_utf8_lossy(&out.stderr);
    let Some(line) = text.lines().find(|l| l.trim_start().starts_with("Stream #") && l.contains("Video:") && !l.contains("attached pic")) else {
        return Err(R_NONE.into());
    };
    if !line.contains("Video: h264") {
        return Err(R_CODEC.into());
    }
    if !line.contains("yuv420p,") && !line.contains("yuv420p(") {
        return Err(R_BITS.into());
    }
    if line.contains("top first") || line.contains("bottom first") || line.contains("interlaced") {
        return Err(R_INTER.into());
    }
    if text.contains("displaymatrix") {
        return Err(R_ROT.into());
    }
    let profile = if line.contains("(High)") {
        "high"
    } else if line.contains("(Main)") {
        "main"
    } else if line.contains("Baseline)") {
        "baseline"
    } else {
        return Err(R_PROF.into());
    };
    let mut color = Vec::new();
    if line.contains("bt709") {
        color.extend(s(&["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]));
    }
    if line.contains("(tv") {
        color.extend(s(&["-color_range", "tv"]));
    } else if line.contains("(pc") {
        color.extend(s(&["-color_range", "pc"]));
    }
    Ok(Params { profile, color })
}

fn pts_times(text: &str) -> Vec<f64> {
    text.split("pts_time:").skip(1).filter_map(|t| t.split_whitespace().next()?.parse().ok()).collect()
}

/// Quadros-chave do vídeo inteiro, para mostrar na tela. Instantâneo, mas o tempo pode errar por um ou dois quadros
/// (`-discard` faz o ffmpeg estimar os tempos). Para cortar, usa-se `keyframes_in`.
pub fn keyframes_quick(src: &Path) -> Vec<f64> {
    let args = s(&["-hide_banner", "-discard", "nokey", "-skip_frame", "nokey", "-i", &path_s(src), "-map", "0:v:0", "-fps_mode", "passthrough", "-vf", "showinfo", "-f", "null", "-"]);
    run_log(&args).map(|o| pts_times(&String::from_utf8_lossy(&o.stderr))).unwrap_or_default()
}

/// Tempos exatos dos quadros-chave entre `from` e `to`.
fn keyframes_in(src: &Path, from: f64, to: f64) -> Result<Vec<f64>, String> {
    let args = s(&["-hide_banner", "-ss", &format!("{from:.3}"), "-t", &format!("{:.3}", to - from), "-copyts", "-skip_frame", "nokey", "-i", &path_s(src), "-map", "0:v:0", "-fps_mode", "passthrough", "-vf", "showinfo", "-f", "null", "-"]);
    Ok(pts_times(&String::from_utf8_lossy(&run_log(&args)?.stderr)))
}

/// Primeiro quadro-chave depois de `a` e último antes de `b`, procurando numa janela curta e depois em toda a parte.
fn copy_span(src: &Path, a: f64, b: f64) -> Result<Option<(f64, f64)>, String> {
    let near = |k: f64, t: f64| (k - t).abs() < 0.02;
    let window = 15.0f64.min(b - a);
    let mut first = keyframes_in(src, a, a + window)?.into_iter().find(|k| *k >= a || near(*k, a));
    if first.is_none() && window < b - a {
        first = keyframes_in(src, a, b)?.into_iter().find(|k| *k >= a || near(*k, a));
    }
    let Some(k1) = first else { return Ok(None) };
    let tail = (b - k1).min(30.0);
    let mut last = keyframes_in(src, b - tail, b)?.into_iter().rfind(|k| *k < b && *k > k1);
    if last.is_none() && tail < b - k1 {
        last = keyframes_in(src, k1, b)?.into_iter().rfind(|k| *k < b && *k > k1);
    }
    Ok(last.filter(|k2| k2 - k1 >= MIN_COPY).map(|k2| (k1, k2)))
}

/// Quantos pacotes de vídeo vão do quadro-chave `k1` até o `k2`, sem incluir este.
fn packets_between(src: &Path, k1: f64, k2: f64) -> Result<usize, String> {
    let span = k2 - k1;
    let args = s(&["-v", "error", "-ss", &format!("{:.3}", k1 + SEEK_SLACK), "-t", &format!("{:.3}", span + 1.0), "-i", &path_s(src), "-map", "0:v:0", "-c", "copy", "-f", "framecrc", "-"]);
    let out = run_log(&args)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let tb = text.lines().find_map(|l| {
        let (n, d) = l.strip_prefix("#tb 0: ")?.split_once('/')?;
        Some(n.trim().parse::<f64>().ok()? / d.trim().parse::<f64>().ok()?)
    });
    let tb = tb.ok_or("não foi possível ler os quadros do vídeo.")?;
    let lines: Vec<&str> = text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).collect();
    let pts = |l: &str| l.split(',').nth(2).and_then(|p| p.trim().parse::<f64>().ok()).map(|p| p * tb);
    let first = lines.first().and_then(|l| pts(l)).ok_or("não foi possível ler os quadros do vídeo.")?;
    lines
        .iter()
        .position(|l| !l.contains("F=") && pts(l).is_some_and(|p| p - first >= span - 0.02))
        .ok_or_else(|| "não foi possível achar o quadro-chave final.".to_string())
}

/// Recodifica de `a` até pouco antes de `b`, em MPEG-TS.
fn encode_piece(src: &Path, p: &Params, a: f64, b: f64, dst: &Path, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let input = s(&["-ss", &format!("{a:.3}"), "-to", &format!("{:.3}", b - 0.002), "-i", &path_s(src)]);
    let mut output = s(&["-map", "0:v:0", "-c:v", "libx264", "-preset", "medium", "-crf", "17", "-profile:v", p.profile, "-pix_fmt", "yuv420p", "-fps_mode", "passthrough"]);
    output.extend(p.color.iter().cloned());
    output.extend(s(&["-f", "mpegts"]));
    encode_to(input, output, dst, None, &|_| {}, cancelled)
}

/// Copia do quadro-chave `k1` até antes do `k2`, em MPEG-TS.
fn copy_piece(src: &Path, k1: f64, k2: f64, dst: &Path, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    let n = packets_between(src, k1, k2)?;
    let input = s(&["-ss", &format!("{:.3}", k1 + SEEK_SLACK), "-i", &path_s(src)]);
    let output = s(&["-map", "0:v:0", "-c", "copy", "-frames:v", &n.to_string(), "-bsf:v", "h264_mp4toannexb", "-f", "mpegts"]);
    encode_to(input, output, dst, None, &|_| {}, cancelled)
}

/// Decodifica em volta de cada emenda: qualquer erro do decodificador reprova o arquivo.
fn verify(dst: &Path, seams: &[f64], total: f64) -> Result<(), String> {
    let info = crate::video::probe(dst);
    let got = info.duration.ok_or("o resultado não abre.")?;
    if (got - total).abs() > 0.4 {
        return Err(format!("duração errada: {got:.2} s em vez de {total:.2} s"));
    }
    for t in seams {
        let args = s(&["-v", "error", "-ss", &format!("{:.3}", (t - 2.0).max(0.0)), "-t", "4", "-i", &path_s(dst), "-map", "0:v:0", "-f", "null", "-"]);
        let log = String::from_utf8_lossy(&run_log(&args)?.stderr).into_owned();
        if !log.trim().is_empty() {
            return Err(log.lines().next().unwrap_or_default().to_string());
        }
    }
    Ok(())
}

/// Grava as `parts` em `dst`. Com erro, `dst` não fica.
pub fn write(
    src: &Path,
    info: &Probe,
    params: &Params,
    parts: &[(f64, f64)],
    dst: &Path,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let dir: PathBuf = std::env::temp_dir().join(format!("conver_inteligente_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut pieces = Vec::new();
        let mut seams = Vec::new();
        let mut elapsed = 0.0;
        for (i, &(a, b)) in parts.iter().enumerate() {
            let name = |what: &str| dir.join(format!("{i}_{what}.ts"));
            if let Some((k1, k2)) = copy_span(src, a, b)? {
                if k1 - a > 0.01 {
                    encode_piece(src, params, a, k1, &name("a"), cancelled)?;
                    pieces.push(name("a"));
                    seams.push(elapsed + k1 - a);
                }
                copy_piece(src, k1, k2, &name("b"), cancelled)?;
                pieces.push(name("b"));
                encode_piece(src, params, k2, b, &name("c"), cancelled)?;
                pieces.push(name("c"));
                seams.push(elapsed + k2 - a);
            } else {
                encode_piece(src, params, a, b, &name("a"), cancelled)?;
                pieces.push(name("a"));
            }
            elapsed += b - a;
            if i + 1 < parts.len() {
                seams.push(elapsed);
            }
            on_progress((i + 1) as f32 / parts.len() as f32 * 0.7);
        }
        let list = dir.join("lista.txt");
        let body: String = pieces.iter().map(|p| format!("file '{}'\n", path_s(p).replace('\'', "'\\''"))).collect();
        std::fs::write(&list, body).map_err(|e| e.to_string())?;
        let video = dir.join("video.mkv");
        encode_to(s(&["-f", "concat", "-safe", "0", "-i", &path_s(&list)]), s(&["-c", "copy"]), &video, None, &|_| {}, cancelled)?;
        on_progress(0.75);
        let mut input = s(&["-i", &path_s(&video)]);
        let mut output = s(&["-map", "0:v", "-c", "copy"]);
        if info.has_audio {
            let audio = dir.join("audio.m4a");
            let only_audio = Probe { has_video: false, ..*info };
            let codec = s(&["-vn", "-c:a", "aac", "-b:a", "192k"]);
            crate::media::write_parts(src, &only_audio, parts, &audio, true, codec, &|_| {}, cancelled)?;
            input.extend(s(&["-i", &path_s(&audio)]));
            output.extend(s(&["-map", "1:a"]));
        }
        if tools::ext_lower(dst) != "mkv" {
            output.extend(s(&["-movflags", "+faststart"]));
        }
        encode_to(input, output, dst, None, &|_| {}, cancelled)?;
        on_progress(0.9);
        verify(dst, &seams, elapsed)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    if result.is_err() {
        let _ = std::fs::remove_file(dst);
    }
    result
}
// i18n: dentro
