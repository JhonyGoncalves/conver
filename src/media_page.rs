//! Telas de vídeo e de áudio (todas usam o ffmpeg).

use eframe::egui::{self, RichText, Ui};

use std::path::PathBuf;

use crate::media::{self, ACodec, AFmt};
use crate::media_extra::{self, SubSize, VAspect};
use crate::pages::{run_batch, run_single};
use crate::theme::Palette;
use crate::tools;
use crate::video::{self, Codec, CompressMode, CompressOpts, Res, VFmt, VLevel, VQuality};
use crate::widgets::{self, Act, Durations, FileQueue, OutDir, card, hint, row, segmented};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum MTool {
    VConvert,
    VCompress,
    VTrim,
    Merge,
    Speed,
    AudioFrames,
    AConvert,
    ATrim,
    Normalize,
    ACompress,
    Gif,
    VTransform,
    Subtitles,
    Music,
    AMerge,
    Denoise,
}

impl MTool {
    fn texts(self) -> (&'static str, &'static str, &'static str) {
        match self {
            MTool::VConvert => (
                "Converter formato de vídeo",
                "Converta entre MP4, MKV, AVI, MOV, WEBM, FLV, WMV, GIF ou extraia o áudio em MP3.",
                "Converter",
            ),
            MTool::VCompress => (
                "Comprimir vídeo",
                "Deixe vídeos menores escolhendo um nível de compressão ou o tamanho final em MB.",
                "Comprimir",
            ),
            MTool::VTrim => ("Cortar trecho de vídeo", "Escolha o início e o fim e salve só aquele pedaço do vídeo.", "Cortar"),
            MTool::Merge => ("Juntar vídeos", "Una vários vídeos num só, na ordem da lista.", "Juntar"),
            MTool::Speed => (
                "Mudar velocidade",
                "Acelere ou deixe em câmera lenta (0,25× a 4×). O áudio acompanha sem ficar com voz fina ou grossa.",
                "Aplicar",
            ),
            MTool::AudioFrames => (
                "Remover áudio / extrair frames",
                "Deixe o vídeo mudo ou salve imagens do vídeo a cada intervalo de tempo.",
                "Aplicar",
            ),
            MTool::AConvert => (
                "Converter formato de áudio",
                "Converta entre MP3, WAV, FLAC, OGG, M4A e OPUS. Também tira o áudio de vídeos.",
                "Converter",
            ),
            MTool::ATrim => ("Cortar áudio", "Salve só um trecho de uma música ou gravação.", "Cortar"),
            MTool::Normalize => (
                "Normalizar volume",
                "Deixe o volume de vários áudios (ou vídeos) no mesmo nível, sem estourar.",
                "Normalizar",
            ),
            MTool::ACompress => (
                "Comprimir áudio",
                "Reduza o tamanho de arquivos de áudio escolhendo a taxa de bits.",
                "Comprimir",
            ),
            MTool::Gif => ("Vídeo para GIF", "Transforme só um trecho do vídeo num GIF animado.", "Criar GIF"),
            MTool::VTransform => (
                "Girar e recortar vídeo",
                "Gire vídeos gravados de lado ou recorte para 9:16 (Reels/TikTok), 1:1, 4:5 ou 16:9.",
                "Aplicar",
            ),
            MTool::Subtitles => (
                "Queimar legenda",
                "Grave uma legenda (.srt, .ass ou .vtt) dentro do vídeo, para aparecer em qualquer player ou rede social.",
                "Legendar",
            ),
            MTool::Music => (
                "Música de fundo",
                "Coloque uma música no vídeo, misturada com o som original ou no lugar dele.",
                "Aplicar",
            ),
            MTool::AMerge => ("Juntar áudios", "Una várias músicas ou gravações num arquivo só, na ordem da lista.", "Juntar"),
            MTool::Denoise => (
                "Remover ruído",
                "Tire chiado, ventilador, barulho de rua e de fundo de gravações de voz, em áudios e vídeos.",
                "Limpar",
            ),
        }
    }

    fn exts(self) -> &'static [&'static str] {
        match self {
            MTool::AConvert | MTool::ATrim | MTool::Normalize | MTool::ACompress | MTool::AMerge | MTool::Denoise => media::MEDIA_EXTS,
            _ => video::VIDEO_EXTS,
        }
    }

    fn is_audio(self) -> bool {
        matches!(self, MTool::AConvert | MTool::ATrim | MTool::Normalize | MTool::ACompress | MTool::AMerge | MTool::Denoise)
    }
}

pub struct MediaPage {
    tool: MTool,
    pub queue: FileQueue,
    out: OutDir,
    job: crate::job::Job,
    // converter / comprimir vídeo
    fmt: VFmt,
    q: VQuality,
    by_size: bool,
    level: VLevel,
    target_mb: f64,
    res: Res,
    codec: Codec,
    audio: u32,
    // cortar
    start: String,
    end: String,
    precise: bool,
    // juntar
    merge_precise: bool,
    // velocidade
    factor: f64,
    keep_audio: bool,
    // áudio / frames
    frames: bool,
    every: f64,
    png: bool,
    // áudio
    afmt: AFmt,
    aq: VQuality,
    lufs: i32,
    denoise: media::Denoise,
    acodec: ACodec,
    akbps: u32,
    mono: bool,
    // gif
    gif_start: String,
    gif_len: f64,
    gif_width: u32,
    gif_fps: u32,
    // girar / recortar
    vrotate: u16,
    vflip: bool,
    vaspect: VAspect,
    // legenda
    sub_file: Option<PathBuf>,
    sub_size: SubSize,
    sub_top: bool,
    // música
    music: Option<PathBuf>,
    replace: bool,
    music_vol: u32,
    music_loop: bool,
}

impl MediaPage {
    pub fn new(tool: MTool) -> Self {
        let kind = if tool.is_audio() { "Áudios e vídeos" } else { "Vídeos" };
        let queue = FileQueue::new(tool.exts(), kind);
        Self {
            tool,
            queue: if matches!(tool, MTool::Merge | MTool::AMerge) { queue.ordered() } else { queue },
            out: OutDir::same_as_source(),
            job: Default::default(),
            fmt: VFmt::Mp4,
            q: VQuality::Media,
            by_size: false,
            level: VLevel::Media,
            target_mb: 25.0,
            res: Res::Original,
            codec: Codec::H264,
            audio: 128,
            start: "00:00:00".into(),
            end: String::new(),
            precise: false,
            merge_precise: true,
            factor: 2.0,
            keep_audio: true,
            frames: false,
            every: 5.0,
            png: false,
            afmt: AFmt::Mp3,
            aq: VQuality::Alta,
            lufs: -16,
            denoise: media::Denoise::Media,
            acodec: ACodec::Mp3,
            akbps: 96,
            mono: false,
            gif_start: "00:00:00".into(),
            gif_len: 5.0,
            gif_width: 480,
            gif_fps: 12,
            vrotate: 0,
            vflip: false,
            vaspect: VAspect::Vertical,
            sub_file: None,
            sub_size: SubSize::Media,
            sub_top: false,
            music: None,
            replace: false,
            music_vol: 40,
            music_loop: true,
        }
    }

    fn trim_range(&self) -> Result<(f64, Option<f64>), String> {
        let start = if self.start.trim().is_empty() {
            0.0
        } else {
            video::parse_timecode(&self.start).ok_or("Início inválido. Use o formato 00:01:30 (ou só segundos).")?
        };
        let end = if self.end.trim().is_empty() {
            None
        } else {
            Some(video::parse_timecode(&self.end).ok_or("Fim inválido. Use o formato 00:02:45 (ou só segundos).")?)
        };
        if end.is_some_and(|e| e <= start) {
            return Err("O fim precisa ser depois do início.".into());
        }
        Ok((start, end))
    }

    fn options_ui(&mut self, ui: &mut Ui, p: &Palette, durs: &Durations) {
        match self.tool {
            MTool::VConvert => {
                row(ui, p, "Formato de saída:", |ui| {
                    egui::ComboBox::from_id_salt("vfmt").selected_text(crate::i18n::tr(self.fmt.label())).width(180.0).show_ui(ui, |ui| {
                        for f in VFmt::ALL {
                            ui.selectable_value(&mut self.fmt, f, crate::i18n::tr(f.label()));
                        }
                    });
                });
                row(ui, p, "Qualidade:", |ui| {
                    let opts: Vec<(VQuality, &str)> = VQuality::ALL.iter().map(|q| (*q, q.label())).collect();
                    segmented(ui, p, &mut self.q, &opts);
                });
            }
            MTool::VCompress => {
                row(ui, p, "Comprimir por:", |ui| {
                    segmented(ui, p, &mut self.by_size, &[(false, "Nível"), (true, "Tamanho final")]);
                });
                if self.by_size {
                    row(ui, p, "Tamanho desejado:", |ui| {
                        ui.add(egui::DragValue::new(&mut self.target_mb).range(1.0..=20000.0).speed(0.5).suffix(" MB"));
                        hint(ui, p, "cada vídeo terá aproximadamente esse tamanho (2 passos, mais lento)");
                    });
                } else {
                    row(ui, p, "Nível:", |ui| {
                        let opts: Vec<(VLevel, &str)> = VLevel::ALL.iter().map(|l| (*l, l.label())).collect();
                        segmented(ui, p, &mut self.level, &opts);
                    });
                }
                row(ui, p, "Resolução máxima:", |ui| {
                    let opts: Vec<(Res, &str)> = Res::ALL.iter().map(|r| (*r, r.label())).collect();
                    segmented(ui, p, &mut self.res, &opts);
                });
                row(ui, p, "Codificação:", |ui| {
                    segmented(
                        ui,
                        p,
                        &mut self.codec,
                        &[(Codec::H264, "H.264 (compatível)"), (Codec::H265, "H.265 (menor, mais lento)")],
                    );
                });
                row(ui, p, "Áudio:", |ui| {
                    segmented(ui, p, &mut self.audio, &[(64, "64 kbps"), (96, "96 kbps"), (128, "128 kbps"), (192, "192 kbps")]);
                });
                hint(ui, p, "O resultado é salvo em MP4 com o sufixo _comprimido.");
            }
            MTool::VTrim | MTool::ATrim => {
                row(ui, p, "Início:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.start).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("00:00:00")).desired_width(110.0));
                    ui.add_space(16.0);
                    ui.label(crate::i18n::tr("Fim:"));
                    ui.add(egui::TextEdit::singleline(&mut self.end).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("até o fim")).desired_width(110.0));
                    if let Some(d) = self.queue.selected_path().and_then(|f| durs.get(f)) {
                        hint(ui, p, &format!("duração do arquivo selecionado: {}", video::fmt_time(d)));
                    }
                });
                row(ui, p, "Modo:", |ui| {
                    segmented(ui, p, &mut self.precise, &[(false, "Rápido (sem perda)"), (true, "Preciso (recodifica)")]);
                });
                hint(
                    ui,
                    p,
                    if self.precise {
                        "Corta exatamente nos tempos informados; demora mais porque regrava o arquivo."
                    } else {
                        "Instantâneo e sem perda de qualidade; em vídeos, o corte pode cair até alguns segundos antes do início."
                    },
                );
                if let Err(e) = self.trim_range() {
                    ui.label(RichText::new(crate::i18n::tr(e)).color(p.accent));
                }
            }
            MTool::Merge => {
                row(ui, p, "Modo:", |ui| {
                    segmented(ui, p, &mut self.merge_precise, &[(true, "Recodificar (recomendado)"), (false, "Rápido (só emendar)")]);
                });
                hint(
                    ui,
                    p,
                    if self.merge_precise {
                        "Funciona com vídeos diferentes: todos ficam na resolução do primeiro, com bordas se precisar. Salva em MP4."
                    } else {
                        "Instantâneo, mas só funciona se todos os vídeos tiverem o mesmo formato, resolução e codificação."
                    },
                );
                hint(ui, p, "Use ↑ e ↓ na lista para mudar a ordem.");
            }
            MTool::Speed => {
                row(ui, p, "Velocidade:", |ui| {
                    segmented(
                        ui,
                        p,
                        &mut self.factor,
                        &[(0.25, "0,25×"), (0.5, "0,5×"), (0.75, "0,75×"), (1.25, "1,25×"), (1.5, "1,5×"), (2.0, "2×"), (3.0, "3×"), (4.0, "4×")],
                    );
                });
                row(ui, p, "Personalizada:", |ui| {
                    ui.add(egui::Slider::new(&mut self.factor, 0.25..=4.0).step_by(0.05).suffix("×"));
                });
                row(ui, p, "Áudio:", |ui| {
                    ui.checkbox(&mut self.keep_audio, crate::i18n::tr("Manter o áudio (acelerado/desacelerado junto)"));
                });
            }
            MTool::AudioFrames => {
                row(ui, p, "O que fazer:", |ui| {
                    segmented(ui, p, &mut self.frames, &[(false, "Remover áudio"), (true, "Extrair frames")]);
                });
                if self.frames {
                    row(ui, p, "Uma imagem a cada:", |ui| {
                        ui.add(egui::DragValue::new(&mut self.every).range(0.1..=3600.0).speed(0.1).suffix(" s"));
                    });
                    row(ui, p, "Formato:", |ui| {
                        segmented(ui, p, &mut self.png, &[(false, "JPG"), (true, "PNG (sem perda)")]);
                    });
                    hint(ui, p, "As imagens são salvas numa pasta <nome do vídeo>_frames.");
                } else {
                    hint(ui, p, "O vídeo é copiado sem recodificar: rápido e sem perda de qualidade.");
                }
            }
            MTool::AConvert => {
                row(ui, p, "Formato de saída:", |ui| {
                    let opts: Vec<(AFmt, &str)> = AFmt::ALL.iter().map(|f| (*f, f.label())).collect();
                    segmented(ui, p, &mut self.afmt, &opts);
                });
                if self.afmt.lossless() {
                    hint(ui, p, "WAV e FLAC são sem perda: não há qualidade para escolher.");
                } else {
                    row(ui, p, "Qualidade:", |ui| {
                        let opts: Vec<(VQuality, &str)> = VQuality::ALL.iter().map(|q| (*q, q.label())).collect();
                        segmented(ui, p, &mut self.aq, &opts);
                    });
                }
            }
            MTool::Normalize => {
                row(ui, p, "Volume alvo:", |ui| {
                    segmented(
                        ui,
                        p,
                        &mut self.lufs,
                        &[(-14, "Alto (−14, streaming)"), (-16, "Padrão (−16)"), (-23, "Suave (−23, TV)")],
                    );
                });
                hint(ui, p, "Em vídeos, a imagem é copiada sem perda; só o áudio é regravado.");
            }
            MTool::Denoise => {
                row(ui, p, "Força:", |ui| {
                    segmented(
                        ui,
                        p,
                        &mut self.denoise,
                        &[(media::Denoise::Leve, "Leve (só chiado)"), (media::Denoise::Media, "Média (IA)"), (media::Denoise::Forte, "Forte")],
                    );
                });
                hint(ui, p, "A opção com IA (RNNoise) foi treinada para voz: limpa bem falas, podcasts e aulas, mas pode abafar música.");
                hint(ui, p, "Em vídeos, a imagem é copiada sem perda; só o áudio é regravado.");
            }
            MTool::ACompress => {
                row(ui, p, "Formato:", |ui| {
                    segmented(
                        ui,
                        p,
                        &mut self.acodec,
                        &[(ACodec::Mp3, "MP3 (compatível)"), (ACodec::Aac, "M4A (AAC)"), (ACodec::Opus, "OPUS (menor)")],
                    );
                });
                row(ui, p, "Taxa de bits:", |ui| {
                    segmented(
                        ui,
                        p,
                        &mut self.akbps,
                        &[(32, "32"), (48, "48"), (64, "64"), (96, "96"), (128, "128"), (160, "160 kbps")],
                    );
                });
                row(ui, p, "Canais:", |ui| {
                    ui.checkbox(&mut self.mono, crate::i18n::tr("Converter para mono (metade do tamanho; bom para voz)"));
                });
                hint(ui, p, "64–96 kbps é ótimo para voz e podcasts; 128–160 kbps para música.");
            }
            MTool::Gif => {
                row(ui, p, "Início:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.gif_start).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("00:00:00")).desired_width(110.0));
                    ui.add_space(16.0);
                    ui.label(crate::i18n::tr("Duração:"));
                    ui.add(egui::DragValue::new(&mut self.gif_len).range(0.5..=60.0).speed(0.1).suffix(" s"));
                    if let Some(d) = self.queue.selected_path().and_then(|f| durs.get(f)) {
                        hint(ui, p, &format!("vídeo selecionado: {}", video::fmt_time(d)));
                    }
                });
                row(ui, p, "Largura:", |ui| {
                    segmented(ui, p, &mut self.gif_width, &[(320, "320 px"), (480, "480 px"), (640, "640 px"), (800, "800 px")]);
                });
                row(ui, p, "Quadros por segundo:", |ui| {
                    segmented(ui, p, &mut self.gif_fps, &[(8, "8"), (12, "12"), (15, "15"), (24, "24")]);
                });
                hint(ui, p, "GIFs ficam grandes rápido: trechos curtos, 480 px e 12 fps costumam ficar abaixo de 5 MB.");
                if video::parse_timecode(&self.gif_start).is_none() && !self.gif_start.trim().is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("Início inválido. Use o formato 00:01:30 (ou só segundos).")).color(p.accent));
                }
            }
            MTool::VTransform => {
                row(ui, p, "Girar:", |ui| {
                    segmented(ui, p, &mut self.vrotate, &[(0, "Não girar"), (90, "90° ↻"), (180, "180°"), (270, "90° ↺")]);
                });
                row(ui, p, "Espelhar:", |ui| {
                    ui.checkbox(&mut self.vflip, crate::i18n::tr("Na horizontal ↔"));
                });
                row(ui, p, "Recortar:", |ui| {
                    let opts: Vec<(VAspect, &str)> = VAspect::ALL.iter().map(|a| (*a, a.label())).collect();
                    segmented(ui, p, &mut self.vaspect, &opts);
                });
                hint(ui, p, "O recorte é centralizado. O áudio é copiado sem perda.");
            }
            MTool::Subtitles => {
                row(ui, p, "Legenda:", |ui| {
                    let text = match &self.sub_file {
                        Some(f) => tools::file_name(f),
                        None => "automática (mesmo nome do vídeo)".into(),
                    };
                    ui.label(RichText::new(crate::i18n::tr(text)).color(p.text));
                    if widgets::secondary(ui, p, "Escolher…", true) {
                        if let Some(f) = rfd::FileDialog::new()
                            .add_filter(crate::i18n::tr("Legendas"), &["srt", "ass", "ssa", "vtt"])
                            .set_title(crate::i18n::tr("Escolha a legenda"))
                            .pick_file()
                        {
                            self.sub_file = Some(f);
                        }
                    }
                    if self.sub_file.is_some() && widgets::secondary(ui, p, "Automática", true) {
                        self.sub_file = None;
                    }
                });
                if self.sub_file.is_none() {
                    let found = self.queue.selected_path().map(|v| media_extra::find_subtitle(v));
                    match found {
                        Some(Some(f)) => hint(ui, p, &format!("Encontrada para o vídeo selecionado: {}", tools::file_name(&f))),
                        Some(None) => {
                            ui.label(
                                RichText::new(crate::i18n::tr("Nenhuma legenda com o mesmo nome do vídeo selecionado. Escolha o arquivo.")).color(p.accent),
                            );
                        }
                        None => hint(ui, p, "Cada vídeo usa a legenda com o mesmo nome dele (video.srt), ou escolha uma para todos."),
                    }
                }
                row(ui, p, "Tamanho do texto:", |ui| {
                    segmented(ui, p, &mut self.sub_size, &[(SubSize::Pequena, "Pequeno"), (SubSize::Media, "Médio"), (SubSize::Grande, "Grande")]);
                });
                row(ui, p, "Posição:", |ui| {
                    segmented(ui, p, &mut self.sub_top, &[(false, "Embaixo"), (true, "Em cima")]);
                });
            }
            MTool::Music => {
                row(ui, p, "Música:", |ui| {
                    let text = self.music.as_ref().map(|f| tools::file_name(f)).unwrap_or_else(|| "nenhuma escolhida".into());
                    ui.label(RichText::new(crate::i18n::tr(text)).color(if self.music.is_some() { p.text } else { p.accent }));
                    if widgets::secondary(ui, p, "Escolher…", true) {
                        if let Some(f) = rfd::FileDialog::new()
                            .add_filter(crate::i18n::tr("Áudio"), media::AUDIO_EXTS)
                            .add_filter(crate::i18n::tr("Todos os arquivos"), &["*"])
                            .set_title(crate::i18n::tr("Escolha a música"))
                            .pick_file()
                        {
                            self.music = Some(f);
                        }
                    }
                });
                row(ui, p, "Som original:", |ui| {
                    segmented(ui, p, &mut self.replace, &[(false, "Misturar com a música"), (true, "Substituir pela música")]);
                });
                row(ui, p, "Volume da música:", |ui| {
                    ui.add(egui::Slider::new(&mut self.music_vol, 0..=200).suffix("%"));
                });
                row(ui, p, "Repetir:", |ui| {
                    ui.checkbox(&mut self.music_loop, crate::i18n::tr("Repetir a música se ela for mais curta que o vídeo"));
                });
                hint(ui, p, "Para música de fundo com fala, 20–40% costuma ficar bom. A imagem é copiada sem perda.");
            }
            MTool::AMerge => {
                row(ui, p, "Formato de saída:", |ui| {
                    let opts: Vec<(AFmt, &str)> = AFmt::ALL.iter().map(|f| (*f, f.label())).collect();
                    segmented(ui, p, &mut self.afmt, &opts);
                });
                hint(ui, p, "Use ↑ e ↓ na lista para mudar a ordem. Também aceita vídeos (usa o som deles).");
            }
        }
        if tools::find_tool("ffmpeg").is_none() {
            ui.label(RichText::new(crate::i18n::tr("⚠ ffmpeg não encontrado — coloque o ffmpeg.exe na pasta bin.")).color(p.accent));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &Durations) {
        let (title, subtitle, action) = self.tool.texts();
        widgets::header(ui, p, title, subtitle);
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 190.0);
        ui.add_space(4.0);
        card(ui, p, |ui| self.options_ui(ui, p, durs));
        ui.add_space(4.0);
        self.out.ui(ui, p);

        let can = match self.tool {
            MTool::Merge | MTool::AMerge => self.queue.files.len() >= 2,
            MTool::Music => !self.queue.files.is_empty() && self.music.is_some(),
            MTool::Gif => !self.queue.files.is_empty() && (self.gif_start.trim().is_empty() || video::parse_timecode(&self.gif_start).is_some()),
            MTool::VTrim | MTool::ATrim => !self.queue.files.is_empty() && self.trim_range().is_ok(),
            _ => !self.queue.files.is_empty(),
        };
        match widgets::action_bar(ui, p, &self.job, action, can) {
            Act::Start => self.start(ui.ctx(), title),
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }

    fn start(&mut self, ctx: &egui::Context, title: &'static str) {
        let files = self.queue.files.clone();
        let out = self.out.path.clone();
        let tool = self.tool;
        if tool == MTool::AMerge {
            let fmt = self.afmt;
            self.job.start(ctx, move |ctl| {
                let input = format!("{} arquivos", files.len());
                run_single(ctl, title, &input, || {
                    media_extra::merge_audio(&files, out.as_deref(), fmt, &|f| ctl.progress(f), &|| ctl.cancelled())
                });
            });
            return;
        }
        if tool == MTool::Merge {
            let precise = self.merge_precise;
            self.job.start(ctx, move |ctl| {
                let input = format!("{} vídeos", files.len());
                run_single(ctl, title, &input, || {
                    media::merge(&files, out.as_deref(), precise, &|f| ctl.progress(f), &|| ctl.cancelled())
                });
            });
            return;
        }
        let (fmt, q) = (self.fmt, self.q);
        let copts = CompressOpts {
            mode: if self.by_size { CompressMode::TargetMb(self.target_mb) } else { CompressMode::Level(self.level) },
            res: self.res,
            codec: self.codec,
            audio_kbps: self.audio,
        };
        let (start, end) = self.trim_range().unwrap_or((0.0, None));
        let (precise, factor, keep_audio) = (self.precise, self.factor, self.keep_audio);
        let (frames, every, png) = (self.frames, self.every, self.png);
        let (afmt, aq, lufs, acodec, akbps, mono) = (self.afmt, self.aq, self.lufs, self.acodec, self.akbps, self.mono);
        let denoise = self.denoise;
        let gif_start = video::parse_timecode(&self.gif_start).unwrap_or(0.0);
        let (gif_len, gif_width, gif_fps) = (self.gif_len, self.gif_width, self.gif_fps);
        let (vrotate, vflip, vaspect) = (self.vrotate, self.vflip, self.vaspect);
        let (sub_file, sub_size, sub_top) = (self.sub_file.clone(), self.sub_size, self.sub_top);
        let (music, replace, music_vol, music_loop) = (self.music.clone(), self.replace, self.music_vol, self.music_loop);
        self.job.start(ctx, move |ctl| {
            run_batch(ctl, title, &files, |src, progress| {
                let cancelled = || ctl.cancelled();
                let out = out.as_deref();
                match tool {
                    MTool::VConvert => video::convert(src, out, fmt, q, progress, &cancelled),
                    MTool::VCompress => video::compress(src, out, copts, progress, &cancelled),
                    MTool::VTrim | MTool::ATrim => media::trim(src, out, start, end, precise, progress, &cancelled),
                    MTool::Speed => media::speed(src, out, factor, keep_audio, progress, &cancelled),
                    MTool::AudioFrames if frames => media::extract_frames(src, out, every, png, progress, &cancelled),
                    MTool::AudioFrames => media::remove_audio(src, out, progress, &cancelled),
                    MTool::AConvert => media::audio_convert(src, out, afmt, aq, progress, &cancelled),
                    MTool::Normalize => media::normalize(src, out, lufs, progress, &cancelled),
                    MTool::ACompress => media::audio_compress(src, out, acodec, akbps, mono, progress, &cancelled),
                    MTool::Denoise => media::denoise(src, out, denoise, progress, &cancelled),
                    MTool::Gif => media_extra::video_gif(src, out, gif_start, gif_len, gif_width, gif_fps, progress, &cancelled),
                    MTool::VTransform => media_extra::video_transform(src, out, vrotate, vflip, vaspect, progress, &cancelled),
                    MTool::Subtitles => {
                        let sub = sub_file.clone().or_else(|| media_extra::find_subtitle(src)).ok_or_else(|| {
                            "nenhuma legenda com o mesmo nome do vídeo; escolha o arquivo de legenda.".to_string()
                        })?;
                        media_extra::burn_subtitles(src, out, &sub, sub_size, sub_top, progress, &cancelled)
                    }
                    MTool::Music => {
                        let m = music.as_deref().ok_or("escolha a música.")?;
                        media_extra::add_music(src, out, m, replace, music_vol, music_loop, progress, &cancelled)
                    }
                    MTool::Merge | MTool::AMerge => unreachable!(),
                }
            });
        });
    }
}
