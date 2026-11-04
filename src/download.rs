//! Download de vídeos/músicas (YouTube, YouTube Music, TikTok, Instagram, X…) com o yt-dlp.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::job::{Ctl, LogKind};
use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Mp4,
    Mp3,
}

/// Arquivos extras baixados junto com o vídeo.
#[derive(Clone, Copy, Default)]
pub struct Extras {
    /// Legendas em português e inglês (.srt), incluindo as automáticas.
    pub subtitles: bool,
    /// Imagem de capa (.jpg).
    pub cover: bool,
}

/// Separa os links de um texto (um por linha, ou separados por espaço).
pub fn parse_links(text: &str) -> Vec<String> {
    let mut links: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c: char| c == ',' || c == ';' || c == '"' || c == '\'');
        if (w.starts_with("http://") || w.starts_with("https://")) && !links.iter().any(|l| l == w) {
            links.push(w.to_string());
        }
    }
    links
}

pub const MP4_QUALITIES: &[&str] = &["Melhor", "1080p", "720p", "480p", "360p"];
pub const MP3_QUALITIES: &[&str] = &["320 kbps", "192 kbps", "128 kbps"];

pub fn version() -> Option<String> {
    let out = tools::command(&tools::find_tool("yt-dlp")?).arg("--version").output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn field(v: Option<&str>) -> Option<&str> {
    v.filter(|s| !s.is_empty() && *s != "NA")
}

pub fn download(
    url: &str,
    out_dir: &Path,
    kind: Kind,
    quality: usize,
    playlist: bool,
    cookies: Option<&str>,
    extras: Extras,
    ctl: &Ctl,
) -> Result<Vec<PathBuf>, String> {
    let ytdlp = tools::find_tool("yt-dlp").ok_or("yt-dlp não encontrado (deveria estar na pasta bin).")?;
    let ffmpeg = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado (deveria estar na pasta bin).")?;

    let mut cmd = tools::command(&ytdlp);
    cmd.env("PYTHONIOENCODING", "utf-8")
        .args(["--encoding", "utf-8", "--quiet", "--progress", "--newline", "--no-warnings"])
        .args(["--windows-filenames", "--no-overwrites", "--no-mtime"])
        .arg(if playlist { "--yes-playlist" } else { "--no-playlist" })
        .arg("--ffmpeg-location")
        .arg(&ffmpeg)
        .arg("-P")
        .arg(out_dir)
        // títulos do TikTok/Instagram são a legenda inteira: limita o tamanho do nome
        .args(["-o", "%(title).120B.%(ext)s"])
        .args([
            "--progress-template",
            "download:PROG|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress._speed_str)s|%(info.playlist_index)s|%(info.n_entries)s|%(info.title)s",
        ])
        .args(["--print", "after_move:DONE|%(filepath)s"]);

    // O YouTube exige um interpretador de JavaScript; usa o que estiver instalado.
    for rt in ["deno", "node", "bun"] {
        if let Some(p) = tools::find_in_path(rt) {
            cmd.arg("--js-runtimes").arg(format!("{rt}:{}", p.display()));
        }
    }

    // login do navegador, para sites que exigem conta (Instagram, Facebook…)
    if let Some(browser) = cookies {
        cmd.args(["--cookies-from-browser", browser]);
    }
    // um item com problema (ou uma legenda que falhou) não interrompe o resto
    cmd.arg("--ignore-errors");
    if extras.subtitles {
        cmd.args(["--write-subs", "--write-auto-subs", "--sub-langs", "pt.*,en.*,-live_chat", "--convert-subs", "srt"]);
    }
    if extras.cover {
        cmd.args(["--write-thumbnail", "--convert-thumbnails", "jpg"]);
    }

    match kind {
        Kind::Mp3 => {
            let kbps = MP3_QUALITIES[quality].split(' ').next().unwrap_or("192");
            cmd.args(["-f", "bestaudio/best", "-x", "--audio-format", "mp3", "--audio-quality"])
                .arg(format!("{kbps}K"))
                .args(["--embed-metadata", "--embed-thumbnail"]);
        }
        Kind::Mp4 => {
            let sort = match MP4_QUALITIES[quality] {
                "Melhor" => "res,vcodec:h264,acodec:aac".to_string(),
                q => format!("res:{},vcodec:h264,acodec:aac", q.trim_end_matches('p')),
            };
            cmd.args(["-f", "bv*+ba/b", "-S", &sort, "--merge-output-format", "mp4"]);
        }
    }
    cmd.arg("--").arg(url).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("não foi possível iniciar o yt-dlp: {e}"))?;
    let stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        BufReader::new(stderr)
            .split(b'\n')
            .map_while(Result::ok)
            .map(|l| String::from_utf8_lossy(&l).trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
    });

    let mut files = Vec::new();
    for line in BufReader::new(child.stdout.take().unwrap()).split(b'\n').map_while(Result::ok) {
        if ctl.cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Cancelado.".into());
        }
        let line = String::from_utf8_lossy(&line).trim().to_string();
        if let Some(rest) = line.strip_prefix("PROG|") {
            let f: Vec<&str> = rest.splitn(7, '|').collect();
            let num = |i: usize| field(f.get(i).copied()).and_then(|v| v.parse::<f64>().ok());
            let total = num(1).or(num(2));
            if let (Some(done), Some(total)) = (num(0), total) {
                if total > 0.0 {
                    ctl.progress((done / total) as f32);
                    if done >= total {
                        ctl.status("Processando com o ffmpeg…");
                        continue;
                    }
                }
            }
            let title = field(f.get(6).copied()).unwrap_or("");
            let speed = field(f.get(3).copied()).unwrap_or("").trim();
            let prefix = match (field(f.get(4).copied()), field(f.get(5).copied())) {
                (Some(i), Some(n)) => format!("[{i}/{n}] "),
                _ => String::new(),
            };
            ctl.status(format!("{prefix}Baixando: {title}   {speed}"));
        } else if let Some(path) = line.strip_prefix("DONE|") {
            let p = PathBuf::from(path);
            ctl.log(LogKind::Ok, format!("✔ {}", tools::file_name(&p)));
            files.push(p);
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let errors = err_thread.join().unwrap_or_default();
    for e in errors.iter().filter(|l| l.starts_with("ERROR")) {
        ctl.log(LogKind::Err, e.trim_start_matches("ERROR: ").to_string());
    }
    if !status.success() && files.is_empty() {
        let all = errors.join(" ").to_lowercase();
        if all.contains("cookies") || all.contains("logged-in") || all.contains("login") || all.contains("sign in") {
            return Err(
                "este site pediu login. Em \"Login do navegador\", escolha o navegador em que você está logado no site e tente de novo."
                    .into(),
            );
        }
        return Err(errors
            .iter()
            .rev()
            .find(|l| l.starts_with("ERROR"))
            .map(|l| l.trim_start_matches("ERROR: ").to_string())
            .unwrap_or_else(|| "o download falhou.".into()));
    }
    Ok(files)
}
