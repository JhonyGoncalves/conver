//! Mais edição de vídeo e áudio: GIF de trecho, girar/recortar, legenda, música de fundo e juntar áudios.

use std::path::{Path, PathBuf};

use crate::imaging::Filter;
use crate::media::{AFmt, audio_args, encode_to, keep_or_mp4, path_s};
use crate::tools;
use crate::video::{VQuality, fmt_time, probe, run, s};

/// GIF de um trecho do vídeo (paleta otimizada para cores bonitas).
pub fn video_gif(
    src: &Path,
    out_dir: Option<&Path>,
    start: f64,
    length: f64,
    width: u32,
    fps: u32,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = probe(src);
    if !info.has_video {
        return Err("o arquivo não tem vídeo.".into());
    }
    if info.duration.is_some_and(|d| start >= d) {
        return Err("o início passa do fim do vídeo.".into());
    }
    let length = info.duration.map(|d| length.min(d - start)).unwrap_or(length);
    let dst = tools::out_path(src, out_dir, "", "gif");
    let filter = format!(
        "fps={fps},scale='min({width},iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4"
    );
    encode_to(
        s(&["-ss", &format!("{start:.3}"), "-t", &format!("{length:.3}"), "-i", &path_s(src)]),
        s(&["-vf", &filter, "-loop", "0"]),
        &dst,
        Some(length),
        on_progress,
        cancelled,
    )?;
    Ok((
        format!("{} → {} • {} fps • {}", fmt_time(start), fmt_time(start + length), fps, tools::human_size(tools::file_size(&dst))),
        dst,
    ))
}

/// Proporção para recortar vídeos.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VAspect {
    Original,
    Vertical,
    Quadrado,
    R4x5,
    Wide,
}

impl VAspect {
    pub const ALL: [VAspect; 5] = [VAspect::Original, VAspect::Vertical, VAspect::Quadrado, VAspect::R4x5, VAspect::Wide];

    pub fn label(self) -> &'static str {
        match self {
            VAspect::Original => "Sem recorte",
            VAspect::Vertical => "9:16 (Reels/TikTok)",
            VAspect::Quadrado => "1:1 (feed)",
            VAspect::R4x5 => "4:5 (Instagram)",
            VAspect::Wide => "16:9 (YouTube)",
        }
    }

    fn ratio(self) -> Option<(u32, u32)> {
        match self {
            VAspect::Original => None,
            VAspect::Vertical => Some((9, 16)),
            VAspect::Quadrado => Some((1, 1)),
            VAspect::R4x5 => Some((4, 5)),
            VAspect::Wide => Some((16, 9)),
        }
    }
}

pub fn video_transform(
    src: &Path,
    out_dir: Option<&Path>,
    rotate: u16,
    flip_h: bool,
    aspect: VAspect,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = probe(src);
    if !info.has_video {
        return Err("o arquivo não tem vídeo.".into());
    }
    let mut filters = Vec::new();
    match rotate {
        90 => filters.push("transpose=1".to_string()),
        180 => filters.push("hflip,vflip".to_string()),
        270 => filters.push("transpose=2".to_string()),
        _ => {}
    }
    if flip_h {
        filters.push("hflip".into());
    }
    if let Some((a, b)) = aspect.ratio() {
        // recorte centralizado; larguras/alturas pares para o H.264
        filters.push(format!(
            "crop='trunc(min(iw,ih*{a}/{b})/2)*2':'trunc(min(ih,iw*{b}/{a})/2)*2'"
        ));
    }
    if filters.is_empty() {
        return Err("escolha um giro, espelhamento ou recorte.".into());
    }
    let dst = tools::out_path(src, out_dir, "_editado", &keep_or_mp4(src));
    let mut out = s(&["-vf", &filters.join(","), "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p"]);
    out.extend(if info.has_audio { s(&["-c:a", "copy"]) } else { s(&["-an"]) });
    encode_to(s(&["-i", &path_s(src)]), out, &dst, info.duration, on_progress, cancelled)?;
    let size = probe(&dst).size.map(|(w, h)| format!(" • {w}×{h}")).unwrap_or_default();
    Ok((format!("editado{size}"), dst))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SubSize {
    Pequena,
    Media,
    Grande,
}

/// Procura uma legenda com o mesmo nome do vídeo (video.srt, video.pt.srt…).
pub fn find_subtitle(video: &Path) -> Option<PathBuf> {
    let stem = video.file_stem()?.to_string_lossy().to_lowercase();
    let dir = video.parent()?;
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(tools::ext_lower(p).as_str(), "srt" | "ass" | "ssa" | "vtt"))
        .filter(|p| {
            let n = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            n == stem || n.starts_with(&format!("{stem}."))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Grava a legenda dentro da imagem do vídeo.
pub fn burn_subtitles(
    src: &Path,
    out_dir: Option<&Path>,
    subtitle: &Path,
    size: SubSize,
    top: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = probe(src);
    if !info.has_video {
        return Err("o arquivo não tem vídeo.".into());
    }
    let bytes = std::fs::read(subtitle).map_err(|e| format!("não foi possível ler a legenda: {e}"))?;
    // copia para a pasta temporária com nome simples: o filtro de legenda não lida bem com caminhos do Windows
    let ext = tools::ext_lower(subtitle);
    let tmp = std::env::temp_dir();
    let name = format!("conver_sub_{}.{ext}", std::process::id());
    std::fs::write(tmp.join(&name), &bytes).map_err(|e| e.to_string())?;
    let charenc = if std::str::from_utf8(&bytes).is_ok() { "" } else { ":charenc=CP1252" };
    let font = match size {
        SubSize::Pequena => 16,
        SubSize::Media => 22,
        SubSize::Grande => 30,
    };
    // o force_style das legendas .srt usa a numeração antiga do SSA: 2 = embaixo no centro, 6 = em cima no centro
    let align = if top { 6 } else { 2 };
    let style = if matches!(ext.as_str(), "ass" | "ssa") {
        String::new() // legendas .ass já têm o próprio estilo
    } else {
        format!(":force_style='FontName=Arial,FontSize={font},Alignment={align},MarginV=28,Outline=2,Shadow=1'")
    };
    let filter = format!("subtitles={name}{charenc}{style}");
    let dst = tools::out_path(src, out_dir, "_legendado", &keep_or_mp4(src));
    let mut args = s(&["-i", &path_s(src), "-vf", &filter, "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-pix_fmt", "yuv420p"]);
    args.extend(if info.has_audio { s(&["-c:a", "copy"]) } else { s(&["-an"]) });
    args.push(path_s(&dst));
    let r = run(args, info.duration, Some(&tmp), on_progress, cancelled);
    let _ = std::fs::remove_file(tmp.join(&name));
    if let Err(e) = r {
        let _ = std::fs::remove_file(&dst);
        return Err(e);
    }
    Ok((format!("legenda: {}", tools::file_name(subtitle)), dst))
}

/// Coloca uma música no vídeo, misturada com o som original ou no lugar dele.
pub fn add_music(
    src: &Path,
    out_dir: Option<&Path>,
    music: &Path,
    replace: bool,
    volume: u32,
    looped: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = probe(src);
    if !info.has_video {
        return Err("o arquivo não tem vídeo.".into());
    }
    if !probe(music).has_audio {
        return Err(format!("{} não tem áudio.", tools::file_name(music)));
    }
    let dur = info.duration.ok_or("não foi possível ler a duração do vídeo.")?;
    let mut input = s(&["-i", &path_s(src)]);
    if looped {
        input.extend(s(&["-stream_loop", "-1"]));
    }
    input.extend(s(&["-i", &path_s(music)]));
    let vol = format!("{:.2}", volume as f64 / 100.0);
    let mix = !replace && info.has_audio;
    let filter = if mix {
        format!("[1:a]volume={vol}[m];[0:a][m]amix=inputs=2:duration=first:normalize=0[a]")
    } else {
        format!("[1:a]volume={vol}[a]")
    };
    let dst = tools::out_path(src, out_dir, "_com_musica", &keep_or_mp4(src));
    encode_to(
        input,
        s(&["-filter_complex", &filter, "-map", "0:v:0", "-map", "[a]", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-t",
            &format!("{dur:.3}")]),
        &dst,
        Some(dur),
        on_progress,
        cancelled,
    )?;
    let how = if mix { "misturada com o som original" } else { "no lugar do som original" };
    Ok((format!("{} {how} • volume {volume}%", tools::file_name(music)), dst))
}

/// Junta áudios (ou o som de vídeos) na ordem dada.
pub fn merge_audio(
    files: &[PathBuf],
    out_dir: Option<&Path>,
    fmt: AFmt,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    if files.len() < 2 {
        return Err("adicione pelo menos 2 arquivos.".into());
    }
    let infos: Vec<_> = files.iter().map(|f| probe(f)).collect();
    if let Some(i) = infos.iter().position(|i| !i.has_audio) {
        return Err(format!("{} não tem áudio.", tools::file_name(&files[i])));
    }
    let total: f64 = infos.iter().filter_map(|i| i.duration).sum();
    let mut input = Vec::new();
    let mut filter = String::new();
    for (i, f) in files.iter().enumerate() {
        input.extend(["-i".to_string(), path_s(f)]);
        filter.push_str(&format!("[{i}:a]aresample=48000,aformat=channel_layouts=stereo[a{i}];"));
    }
    for i in 0..files.len() {
        filter.push_str(&format!("[a{i}]"));
    }
    filter.push_str(&format!("concat=n={}:v=0:a=1[a]", files.len()));
    let dst = tools::out_path(&files[0], out_dir, "_juntado", fmt.ext());
    let mut out = s(&["-filter_complex", &filter, "-map", "[a]"]);
    // os argumentos de codificação de áudio, sem o "-vn -map_metadata 0" inicial
    out.extend(audio_args(fmt, VQuality::Alta).into_iter().skip(3));
    encode_to(input, out, &dst, Some(total), on_progress, cancelled)?;
    Ok((format!("{} arquivos • {} • {}", files.len(), fmt_time(total), tools::human_size(tools::file_size(&dst))), dst))
}

/// Cor e desfoque de um vídeo. Brilho, contraste e saturação vão de −100 a 100, como nas imagens; desfoque de 0 a 100.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct VideoAdjust {
    pub brightness: i16,
    pub contrast: i16,
    pub saturation: i16,
    pub filter: Filter,
    pub blur: u8,
}

impl VideoAdjust {
    pub fn is_neutral(self) -> bool {
        self == Self { brightness: 0, contrast: 0, saturation: 0, filter: Filter::Nenhum, blur: 0 }
    }

    // i18n: fora
    /// Filtros do ffmpeg. O desfoque acompanha a altura, para ficar igual em 720p e em 1080p.
    fn chain(self, height: u32) -> String {
        let mut parts = Vec::new();
        if (self.brightness, self.contrast, self.saturation) != (0, 0, 0) {
            let (b, c, sat) = (self.brightness as f64 / 200.0, 1.0 + self.contrast as f64 / 100.0, 1.0 + self.saturation as f64 / 100.0);
            parts.push(format!("eq=brightness={b:.3}:contrast={c:.3}:saturation={sat:.3}"));
        }
        match self.filter {
            Filter::Nenhum => {}
            Filter::PretoBranco => parts.push("hue=s=0".into()),
            Filter::Sepia => parts.push("colorchannelmixer=.393:.769:.189:0:.349:.686:.168:0:.272:.534:.131".into()),
            Filter::Inverter => parts.push("negate".into()),
        }
        if self.blur > 0 {
            let sigma = (self.blur as f64 / 100.0 * height as f64 / 18.0).max(0.5);
            parts.push(format!("gblur=sigma={sigma:.2}"));
        }
        parts.join(",")
    }
    // i18n: dentro

    fn label(self) -> String {
        let mut parts = Vec::new();
        for (name, v) in [("brilho", self.brightness), ("contraste", self.contrast), ("saturação", self.saturation)] {
            if v != 0 {
                parts.push(format!("{name} {v:+}"));
            }
        }
        if self.filter != Filter::Nenhum {
            parts.push(self.filter.label().to_lowercase());
        }
        if self.blur > 0 {
            parts.push(format!("desfoque {}", self.blur));
        }
        parts.join(" • ")
    }
}

/// Aplica cor e desfoque ao vídeo inteiro. O som continua o mesmo.
pub fn adjust_video(
    src: &Path,
    out_dir: Option<&Path>,
    adjust: VideoAdjust,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    if adjust.is_neutral() {
        return Err("escolha pelo menos um ajuste.".into());
    }
    let info = probe(src);
    if !info.has_video {
        return Err("o arquivo não tem vídeo.".into());
    }
    let chain = adjust.chain(info.size.map_or(720, |(_, h)| h));
    let dst = tools::out_path(src, out_dir, "_ajustado", &keep_or_mp4(src));
    encode_to(
        s(&["-i", &path_s(src)]),
        s(&["-map", "0:v:0", "-map", "0:a:0?", "-vf", &chain, "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt", "yuv420p",
            "-c:a", "aac", "-b:a", "192k"]),
        &dst,
        info.duration,
        on_progress,
        cancelled,
    )?;
    Ok((adjust.label(), dst))
}

/// Um quadro do vídeo (em `pos`, de 0 a 1 da duração) com os ajustes, em PNG, para a prévia.
pub fn adjust_preview(src: &Path, pos: f32, adjust: VideoAdjust) -> Result<Vec<u8>, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let info = probe(src);
    let at = info.duration.unwrap_or(0.0) * pos.clamp(0.0, 0.99) as f64;
    let chain = adjust.chain(info.size.map_or(720, |(_, h)| h));
    let filter = if chain.is_empty() { "scale=-2:360".to_string() } else { format!("{chain},scale=-2:360") };
    let out = tools::command(&ff)
        .args(["-v", "error", "-nostdin", "-ss", &format!("{at:.2}"), "-i"])
        .arg(src)
        .args(["-frames:v", "1", "-vf", &filter, "-f", "image2pipe", "-c:v", "png", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    if out.stdout.is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(out.stdout)
}
