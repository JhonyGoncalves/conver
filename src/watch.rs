//! Pasta vigiada: tudo que cair na pasta é convertido automaticamente para a subpasta "convertidos".

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::imaging::{self, Fmt, ImageOp, Level};
use crate::job::{Ctl, LogKind};
use crate::media::{self, AFmt};
use crate::video::{self, Codec, CompressMode, CompressOpts, Res, VFmt, VLevel, VQuality};
use crate::{history, notify, tools};

pub const OUT_FOLDER: &str = "convertidos";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CompressImages,
    ConvertImages,
    CompressVideos,
    ConvertVideos,
    ConvertAudio,
}

impl Action {
    pub const ALL: [Action; 5] =
        [Action::CompressImages, Action::ConvertImages, Action::CompressVideos, Action::ConvertVideos, Action::ConvertAudio];

    pub fn label(self) -> &'static str {
        match self {
            Action::CompressImages => "Comprimir imagens",
            Action::ConvertImages => "Converter imagens",
            Action::CompressVideos => "Comprimir vídeos",
            Action::ConvertVideos => "Converter vídeos",
            Action::ConvertAudio => "Converter áudios",
        }
    }

    pub fn exts(self) -> &'static [&'static str] {
        match self {
            Action::CompressImages | Action::ConvertImages => imaging::IMAGE_EXTS,
            Action::CompressVideos | Action::ConvertVideos => video::VIDEO_EXTS,
            Action::ConvertAudio => media::MEDIA_EXTS,
        }
    }
}

/// Opções de cada ação.
#[derive(Clone, Copy)]
pub struct Options {
    pub action: Action,
    pub img_level: Level,
    pub img_fmt: Fmt,
    pub vid_level: VLevel,
    pub vid_fmt: VFmt,
    pub aud_fmt: AFmt,
}

fn process(src: &Path, out: &Path, o: &Options, ctl: &Ctl) -> Result<(String, PathBuf), String> {
    let none = |_f: f32| {};
    let cancelled = || ctl.cancelled();
    match o.action {
        Action::CompressImages => imaging::process_and_save(src, &ImageOp::Compress(o.img_level), Some(out)),
        Action::ConvertImages => imaging::process_and_save(src, &ImageOp::Convert(o.img_fmt), Some(out)),
        Action::CompressVideos => {
            let opts = CompressOpts { mode: CompressMode::Level(o.vid_level), res: Res::Original, codec: Codec::H264, audio_kbps: 128 };
            video::compress(src, Some(out), opts, &none, &cancelled)
        }
        Action::ConvertVideos => video::convert(src, Some(out), o.vid_fmt, VQuality::Media, &none, &cancelled),
        Action::ConvertAudio => media::audio_convert(src, Some(out), o.aud_fmt, VQuality::Alta, &none, &cancelled),
    }
}

fn list(folder: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && exts.contains(&tools::ext_lower(p).as_str())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Arquivo que ainda está sendo copiado/baixado não abre para leitura exclusiva no Windows.
fn ready(p: &Path) -> bool {
    std::fs::OpenOptions::new().read(true).write(true).open(p).is_ok()
}

/// Vigia `folder` até o usuário parar. Arquivos que já estavam lá só são processados se `existing`.
pub fn run(ctl: &Ctl, folder: &Path, options: Options, existing: bool) {
    let out = folder.join(OUT_FOLDER);
    if let Err(e) = std::fs::create_dir_all(&out) {
        ctl.log(LogKind::Err, format!("✖ não foi possível criar a pasta {OUT_FOLDER}: {e}"));
        ctl.status("Erro.");
        return;
    }
    let exts = options.action.exts();
    let mut done: HashSet<PathBuf> = HashSet::new();
    if !existing {
        done.extend(list(folder, exts));
    }
    let mut sizes: HashMap<PathBuf, u64> = HashMap::new();
    let mut count = 0;
    ctl.log(LogKind::Info, format!("Vigiando {} — {}", folder.display(), options.action.label().to_lowercase()));
    ctl.log(LogKind::Info, format!("Os resultados vão para a subpasta \"{OUT_FOLDER}\"."));
    ctl.status(format!("Vigiando a pasta… ({count} processados)"));
    ctl.done_folder(out.clone());
    while !ctl.cancelled() {
        for f in list(folder, exts) {
            if ctl.cancelled() {
                break;
            }
            if done.contains(&f) {
                continue;
            }
            // espera o tamanho parar de mudar (cópia ou download terminando)
            let size = tools::file_size(&f);
            let stable = sizes.get(&f) == Some(&size) && size > 0;
            sizes.insert(f.clone(), size);
            if !stable || !ready(&f) {
                continue;
            }
            done.insert(f.clone());
            let name = tools::file_name(&f);
            ctl.status(format!("Processando {name}…"));
            match process(&f, &out, &options, ctl) {
                Ok((info, dst)) => {
                    count += 1;
                    ctl.log(LogKind::Ok, format!("✔ {name}   {info}"));
                    history::record("Pasta vigiada", &name, &dst, &info);
                    notify::finished(&format!("{name}: {info}"), true);
                }
                Err(e) if e == "Cancelado." => break,
                Err(e) => {
                    ctl.log(LogKind::Err, format!("✖ {name}: {e}"));
                    notify::finished(&format!("{name}: {e}"), false);
                }
            }
            ctl.status(format!("Vigiando a pasta… ({count} processados)"));
        }
        // espera ~1,5 s, conferindo se o usuário mandou parar
        for _ in 0..15 {
            if ctl.cancelled() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    ctl.status(format!("Parado. {count} arquivo(s) processado(s)."));
}
