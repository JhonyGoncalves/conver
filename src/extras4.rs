//! Telas: transcrever (Whisper), separar voz e instrumental, gravar a tela e predefinições.

use std::path::PathBuf;

use eframe::egui::{self, Align2, FontId, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, vec2};
use serde_json::{Map, Value};

use crate::job::{Job, LogKind};
use crate::pages::run_batch;
use crate::screenrec::{self, Area, Quality, Session};
use crate::separate::{self, Stems};
use crate::theme::{Palette, bold};
use crate::transcribe::{self, Outputs, WModel};
use crate::widgets::{self, Act, Durations, FileQueue, OutDir, card, hint, row, segmented};
use crate::{ai, history, ops, presets, recorder, tools, video};

const AV_EXTS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "ts", "mp3", "wav", "flac", "ogg", "oga",
    "m4a", "aac", "opus", "wma", "aiff", "aif", "amr",
];

fn model_note(ui: &mut Ui, p: &Palette, spec: &ai::ModelSpec) {
    if ai::is_installed(spec) {
        hint(ui, p, &format!("✔ Modelo {} pronto neste computador.", spec.label));
    } else {
        hint(ui, p, &format!("O modelo {} é baixado uma vez, na primeira vez que você usar (a assinatura é conferida).", spec.label));
    }
}

// ================================================================ transcrever

pub struct TranscribePage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    model: WModel,
    lang: usize,
    translate: bool,
    outputs: Outputs,
}

impl TranscribePage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(AV_EXTS, "Áudios e vídeos"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            model: WModel::Rapido,
            lang: 0,
            translate: false,
            outputs: Outputs { txt: true, srt: true, vtt: false },
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &Durations) {
        let title = "Transcrever / legendar";
        widgets::header(ui, p, title, "Transforme a fala de áudios e vídeos em texto e legendas (SRT/VTT) com o Whisper, no seu PC e sem internet.");
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 190.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Modelo:", |ui| {
                let opts: Vec<(WModel, &str)> = WModel::ALL.iter().map(|m| (*m, m.label())).collect();
                segmented(ui, p, &mut self.model, &opts);
            });
            row(ui, p, "Idioma falado:", |ui| {
                let name = transcribe::LANGS[self.lang].1;
                egui::ComboBox::from_id_salt("whisper_lang").selected_text(crate::i18n::tr(name)).width(240.0).show_ui(ui, |ui| {
                    for (i, (_, n)) in transcribe::LANGS.iter().enumerate() {
                        ui.selectable_value(&mut self.lang, i, crate::i18n::tr(*n));
                    }
                });
                crate::widgets::check(ui, &mut self.translate, crate::i18n::tr("Traduzir para o inglês"));
            });
            row(ui, p, "Salvar como:", |ui| {
                crate::widgets::check(ui, &mut self.outputs.txt, crate::i18n::tr("Texto (.txt)"));
                crate::widgets::check(ui, &mut self.outputs.srt, crate::i18n::tr("Legenda (.srt)"));
                crate::widgets::check(ui, &mut self.outputs.vtt, crate::i18n::tr("Legenda web (.vtt)"));
            });
            hint(ui, p, "A legenda .srt fica com o mesmo nome do vídeo: o Queimar legenda e os players a encontram sozinhos.");
            model_note(ui, p, &self.model.spec());
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let can = !self.queue.files.is_empty() && (self.outputs.txt || self.outputs.srt || self.outputs.vtt);
        match widgets::action_bar(ui, p, &self.job, "Transcrever", can) {
            Act::Start => {
                let (files, out, model, outputs, translate) = (self.queue.files.clone(), self.out.path.clone(), self.model, self.outputs, self.translate);
                let lang = transcribe::LANGS[self.lang].0.to_string();
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| {
                        transcribe::transcribe(src, out.as_deref(), model, &lang, translate, outputs, progress, &|| ctl.cancelled())
                    });
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ separar voz

pub struct SeparatePage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    stems: Stems,
    fmt: &'static str,
}

impl SeparatePage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(AV_EXTS, "Músicas"), out: OutDir::same_as_source(), job: Job::default(), stems: Stems::Ambos, fmt: "wav" }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &Durations) {
        let title = "Separar voz e instrumental";
        widgets::header(ui, p, title, "Tire a voz de uma música (karaokê) ou isole a voz com inteligência artificial, no seu PC.");
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 190.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Salvar:", |ui| {
                let opts: Vec<(Stems, &str)> = Stems::ALL.iter().map(|s| (*s, s.label())).collect();
                segmented(ui, p, &mut self.stems, &opts);
            });
            row(ui, p, "Formato:", |ui| {
                segmented(ui, p, &mut self.fmt, &[("wav", "WAV (sem perda)"), ("flac", "FLAC"), ("mp3", "MP3 320 kbps")]);
            });
            hint(ui, p, "Leva cerca de 15 a 30 segundos por minuto de música, dependendo do computador.");
            model_note(ui, p, &separate::MODEL);
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        match widgets::action_bar(ui, p, &self.job, "Separar", !self.queue.files.is_empty()) {
            Act::Start => {
                let (files, out, stems, fmt) = (self.queue.files.clone(), self.out.path.clone(), self.stems, self.fmt);
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| separate::separate(src, out.as_deref(), stems, fmt, progress, &|| ctl.cancelled()));
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ gravar a tela

#[derive(Clone, Copy, PartialEq, Eq)]
enum AreaKind {
    Tela,
    Monitor,
    Janela,
    Regiao,
}

pub struct ScreenPage {
    kind: AreaKind,
    windows: Vec<String>,
    window: usize,
    monitors: Vec<screenrec::Monitor>,
    monitor: usize,
    region: (i32, i32, u32, u32),
    fps: u32,
    quality: Quality,
    max_height: Option<u32>,
    mics: Option<Vec<String>>,
    mic: usize,
    cursor: bool,
    minimize: bool,
    folder: PathBuf,
    rec: Option<Session>,
    msg: Option<(bool, String)>,
    last: Option<PathBuf>,
    preview: screenrec::Preview,
    preview_tex: Option<TextureHandle>,
}

impl ScreenPage {
    pub fn new() -> Self {
        let videos = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Videos")).filter(|p| p.is_dir());
        Self {
            kind: AreaKind::Tela,
            windows: Vec::new(),
            window: 0,
            monitors: Vec::new(),
            monitor: 0,
            region: (0, 0, 1280, 720),
            fps: 30,
            quality: Quality::Media,
            max_height: None,
            mics: None,
            mic: 0,
            cursor: true,
            minimize: true,
            folder: videos.unwrap_or_else(tools::exe_dir),
            rec: None,
            msg: None,
            last: None,
            preview: screenrec::Preview::new(),
            preview_tex: None,
        }
    }

    pub fn recording(&self) -> bool {
        self.rec.is_some()
    }

    /// Modo de captura: prévia da própria janela do Conver (não mostra o resto da tela do usuário).
    pub fn demo(&mut self) {
        self.windows = screenrec::windows();
        if let Some(i) = self.windows.iter().position(|w| w == "Conver") {
            self.kind = AreaKind::Janela;
            self.window = i;
            self.mics = Some(Vec::new());
        }
    }

    /// O que está escolhido para gravar (`None`: janela escolhida, mas a lista está vazia).
    fn area(&self) -> Option<Area> {
        Some(match self.kind {
            AreaKind::Tela => Area::TelaInteira,
            AreaKind::Monitor => self.monitors.get(self.monitor)?.area(),
            AreaKind::Janela => Area::Janela(self.windows.get(self.window)?.clone()),
            AreaKind::Regiao => Area::Regiao { x: self.region.0, y: self.region.1, w: self.region.2, h: self.region.3 },
        })
    }

    /// Prévia ao vivo do que vai ser gravado, encaixada em `size`.
    fn preview_ui(&mut self, ui: &mut Ui, p: &Palette, size: egui::Vec2, recording: bool) {
        let paused = self.rec.as_ref().is_some_and(|r| r.paused());
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect.expand(2.0));
        painter.rect_filled(rect, 8, p.input);
        let Some(area) = self.area() else {
            painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::tr("Escolha uma janela"), FontId::proportional(13.0), p.muted);
            return;
        };
        self.preview.slow.store(recording, std::sync::atomic::Ordering::Relaxed);
        if self.preview.touch(ui.ctx(), area) {
            self.preview_tex = None;
        }
        let mut source = None;
        if let Some(g) = self.preview.frame.lock().unwrap().as_ref() {
            let img = egui::ColorImage::from_rgba_unmultiplied([g.width as usize, g.height as usize], &g.rgba);
            match &mut self.preview_tex {
                Some(t) if t.size() == img.size => t.set(img, TextureOptions::LINEAR),
                _ => self.preview_tex = Some(ui.ctx().load_texture("screen_preview", img, TextureOptions::LINEAR)),
            }
            source = Some(g.source);
        }
        let missing = self.preview.missing.load(std::sync::atomic::Ordering::Relaxed);
        match &self.preview_tex {
            Some(tex) if !missing => {
                let s = tex.size_vec2();
                let fit = widgets::fit((s.x, s.y), rect.shrink(6.0).size());
                let r = egui::Rect::from_center_size(rect.center(), fit);
                painter.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                painter.rect_stroke(r, 0, Stroke::new(1.0f32, p.border), StrokeKind::Outside);
                if let Some((w, h)) = source {
                    let label = crate::i18n::tr(format!("{w}×{h}"));
                    let galley = painter.layout_no_wrap(label, FontId::proportional(11.5), p.on_accent);
                    let pill = egui::Rect::from_min_size(r.right_bottom() - vec2(galley.size().x + 16.0, 26.0), galley.size() + vec2(12.0, 6.0));
                    painter.rect_filled(pill, 5, egui::Color32::from_black_alpha(150));
                    painter.galley(pill.min + vec2(6.0, 3.0), galley, p.on_accent);
                }
                if recording {
                    let (color, label, w) = if paused { (p.muted, crate::i18n::tr("❚❚  PAUSADO"), 96.0) } else { (p.action, crate::i18n::tr("●  REC"), 64.0) };
                    painter.rect_stroke(r, 0, Stroke::new(2.0f32, color), StrokeKind::Outside);
                    let pill = egui::Rect::from_min_size(r.left_top() + vec2(8.0, 8.0), vec2(w, 22.0));
                    painter.rect_filled(pill, 6, color);
                    painter.text(pill.center(), Align2::CENTER_CENTER, label, FontId::new(11.5, crate::theme::bold()), p.on_accent);
                }
            }
            _ => {
                let text = if missing { "A janela está minimizada ou foi fechada." } else { "Carregando a prévia…" };
                painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::tr(text), FontId::proportional(13.0), p.muted);
            }
        }
    }

    fn stop(&mut self) {
        let Some(rec) = self.rec.take() else { return };
        let secs = rec.seconds();
        match rec.stop() {
            Ok(path) => {
                let info = format!("{} • {}", video::fmt_time(secs), tools::human_size(tools::file_size(&path)));
                history::record("Gravar a tela", "tela", &path, &info);
                self.msg = Some((true, format!("✔ Gravação salva: {}  ({info})", tools::file_name(&path))));
                self.last = Some(path);
            }
            Err(e) => self.msg = Some((false, format!("✖ {e}"))),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(ui, p, "Gravar a tela", "Grave a tela inteira, um monitor, uma janela ou uma área em MP4, com ou sem o microfone.");
        // janelas e monitores são rápidos; os microfones vêm do ffmpeg, em segundo plano
        if self.monitors.is_empty() {
            self.windows = screenrec::windows();
            self.monitors = screenrec::monitors();
        }
        if self.mics.is_none() {
            self.mics = recorder::cached_devices(ui.ctx());
        }
        match self.rec.as_mut().map(|r| r.check()) {
            Some(Err(e)) => {
                self.rec = None;
                self.msg = Some((false, format!("✖ A gravação parou: {e}")));
            }
            Some(Ok(false)) => self.stop(),
            _ => {}
        }
        let recording = self.rec.is_some();
        let mics = self.mics.clone().unwrap_or_default();
        card(ui, p, |ui| {
            ui.add_enabled_ui(!recording, |ui| {
                row(ui, p, "Gravar:", |ui| {
                    segmented(ui, p, &mut self.kind, &[(AreaKind::Tela, "Tela inteira"), (AreaKind::Monitor, "Um monitor"), (AreaKind::Janela, "Uma janela"), (AreaKind::Regiao, "Uma área")]);
                });
                match self.kind {
                    AreaKind::Tela => {
                        if self.monitors.len() > 1 {
                            hint(ui, p, "Com mais de um monitor, \"Tela inteira\" grava todos juntos. Para gravar só um, escolha \"Um monitor\".");
                        }
                    }
                    AreaKind::Monitor => {
                        row(ui, p, "Monitor:", |ui| {
                            if self.monitors.is_empty() {
                                ui.label(RichText::new(crate::i18n::tr("nenhum monitor encontrado")).color(p.accent));
                            } else {
                                self.monitor = self.monitor.min(self.monitors.len() - 1);
                                let label = |i: usize, m: &screenrec::Monitor| {
                                    let main = if m.primary { format!(" ({})", crate::i18n::tr("principal")) } else { String::new() };
                                    format!("{} {}{main}  •  {}×{}", crate::i18n::tr("Monitor"), i + 1, m.w, m.h)
                                };
                                let cur = label(self.monitor, &self.monitors[self.monitor]);
                                egui::ComboBox::from_id_salt("rec_monitor").selected_text(cur).width(320.0).show_ui(ui, |ui| {
                                    for (i, m) in self.monitors.iter().enumerate() {
                                        ui.selectable_value(&mut self.monitor, i, label(i, m));
                                    }
                                });
                            }
                            if widgets::secondary(ui, p, "Atualizar lista", true) {
                                self.monitors = screenrec::monitors();
                            }
                        });
                    }
                    AreaKind::Janela => {
                        row(ui, p, "Janela:", |ui| {
                            if self.windows.is_empty() {
                                ui.label(RichText::new(crate::i18n::tr("nenhuma janela encontrada")).color(p.accent));
                            } else {
                                self.window = self.window.min(self.windows.len() - 1);
                                let cur: String = self.windows[self.window].chars().take(60).collect();
                                egui::ComboBox::from_id_salt("rec_window").selected_text(cur).width(360.0).show_ui(ui, |ui| {
                                    for (i, w) in self.windows.iter().enumerate() {
                                        let t: String = w.chars().take(80).collect();
                                        ui.selectable_value(&mut self.window, i, t);
                                    }
                                });
                            }
                            if widgets::secondary(ui, p, "Atualizar lista", true) {
                                self.windows = screenrec::windows();
                            }
                        });
                    }
                    AreaKind::Regiao => {
                        row(ui, p, "Área (px):", |ui| {
                            ui.label("x");
                            ui.add(egui::DragValue::new(&mut self.region.0).range(-10000..=10000));
                            ui.label("y");
                            ui.add(egui::DragValue::new(&mut self.region.1).range(-10000..=10000));
                            ui.label(crate::i18n::tr("Largura"));
                            ui.add(egui::DragValue::new(&mut self.region.2).range(16..=8000));
                            ui.label(crate::i18n::tr("Altura"));
                            ui.add(egui::DragValue::new(&mut self.region.3).range(16..=8000));
                        });
                    }
                }
                row(ui, p, "Qualidade:", |ui| {
                    segmented(ui, p, &mut self.quality, &[(Quality::Alta, "Alta"), (Quality::Media, "Média"), (Quality::Leve, "Leve (arquivo menor)")]);
                });
                row(ui, p, "Resolução máxima:", |ui| {
                    segmented(ui, p, &mut self.max_height, &[(None, "Original"), (Some(1080), "1080p"), (Some(720), "720p")]);
                });
                row(ui, p, "Quadros por segundo:", |ui| {
                    segmented(ui, p, &mut self.fps, &[(15, "15"), (30, "30"), (60, "60")]);
                });
                row(ui, p, "Microfone:", |ui| {
                    let none = crate::i18n::tr(if self.mics.is_none() { "Sem áudio (procurando microfones…)" } else { "Sem áudio" });
                    let cur = if self.mic == 0 { none.clone() } else { mics.get(self.mic - 1).cloned().unwrap_or_default() };
                    egui::ComboBox::from_id_salt("rec_mic").selected_text(cur).width(320.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mic, 0, none);
                        for (i, m) in mics.iter().enumerate() {
                            ui.selectable_value(&mut self.mic, i + 1, m);
                        }
                    });
                });
                row(ui, p, "Opções:", |ui| {
                    crate::widgets::check(ui, &mut self.cursor, crate::i18n::tr("Mostrar o cursor"));
                    ui.add_space(8.0);
                    crate::widgets::check(ui, &mut self.minimize, crate::i18n::tr("Minimizar o Conver ao começar"));
                });
                row(ui, p, "Salvar em:", |ui| {
                    let dir = self.folder.display().to_string();
                    ui.add(egui::TextEdit::singleline(&mut dir.as_str()).margin(widgets::FIELD_MARGIN).desired_width((ui.available_width() - 110.0).clamp(160.0, 460.0)));
                    if widgets::secondary(ui, p, "Escolher…", true) {
                        if let Some(d) = rfd::FileDialog::new().set_title(crate::i18n::tr("Pasta das gravações")).pick_folder() {
                            self.folder = d;
                        }
                    }
                });
            });
        });
        ui.add_space(8.0);
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                let side = 230.0;
                let pw = (ui.available_width() - side - 14.0).max(240.0);
                let ph = (pw * 9.0 / 16.0).min(300.0);
                self.preview_ui(ui, p, vec2(pw, ph), recording);
                ui.add_space(6.0);
                ui.allocate_ui_with_layout(vec2(side, ph), egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.add_space((ph - 150.0).max(0.0) / 2.0);
                    let secs = self.rec.as_ref().map(|r| r.seconds()).unwrap_or(0.0);
                    let paused = self.rec.as_ref().is_some_and(|r| r.paused());
                    let (dot, color) = if paused {
                        ("❚❚  Pausado", p.muted)
                    } else if recording {
                        ("●  Gravando", p.accent)
                    } else {
                        ("Pronto para gravar", p.muted)
                    };
                    ui.label(RichText::new(crate::i18n::tr(dot)).family(bold()).color(color));
                    ui.label(RichText::new(crate::i18n::tr(video::fmt_time(secs))).family(bold()).size(52.0).color(if paused { p.muted } else { p.text }));
                    ui.add_space(6.0);
                    if recording {
                        if widgets::primary(ui, p, "■  Parar", true) {
                            self.stop();
                        } else {
                            ui.add_space(4.0);
                            let label = if paused { "▶  Continuar" } else { "❚❚  Pausar" };
                            if widgets::secondary(ui, p, label, true) {
                                if let Some(rec) = self.rec.as_mut() {
                                    let r = if paused { rec.resume() } else { rec.pause() };
                                    match r {
                                        Ok(()) if paused && self.minimize => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                                        Ok(()) => {}
                                        Err(e) => self.msg = Some((false, format!("✖ {e}"))),
                                    }
                                }
                            }
                        }
                    } else if let (true, Some(area)) = (widgets::primary(ui, p, "●  Gravar", self.area().is_some()), self.area()) {
                        let o = screenrec::Options {
                            area,
                            fps: self.fps,
                            mic: (self.mic > 0).then(|| mics[self.mic - 1].clone()),
                            cursor: self.cursor,
                            quality: self.quality,
                            max_height: self.max_height,
                        };
                        let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
                        let dst = tools::unique_path(self.folder.join(format!("gravacao_tela_{stamp}.mp4")));
                        let ctx = ui.ctx().clone();
                        match Session::start(&o, &dst, move || ctx.request_repaint()) {
                            Ok(rec) => {
                                self.rec = Some(rec);
                                self.msg = None;
                                if self.minimize {
                                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                                }
                            }
                            Err(e) => self.msg = Some((false, format!("✖ {e}"))),
                        }
                    }
                });
            });
        });
        if let Some((good, text)) = &self.msg {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent }));
                if let (true, Some(last)) = (*good, &self.last) {
                    if widgets::secondary(ui, p, "Assistir", true) {
                        tools::open_file(last);
                    }
                    if widgets::secondary(ui, p, "Mostrar na pasta", true) {
                        tools::show_in_folder(last);
                    }
                }
            });
        }
        hint(ui, p, "Para pausar ou parar, volte ao Conver (ele fica na barra de tarefas). Os trechos pausados são juntados num MP4 só.");
        if recording {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

// ================================================================ predefinições

/// Ferramentas que fazem sentido numa predefinição (as que recebem arquivos).
fn preset_tools() -> Vec<&'static ops::Tool> {
    ops::TOOLS
        .iter()
        .filter(|t| (t.schema)()["properties"].get("files").is_some() && !t.name.starts_with("preset_"))
        .collect()
}

/// Argumentos que o formulário não mostra (vêm da lista de arquivos e da pasta de saída).
const HIDDEN: &[&str] = &["files", "out_dir", "output", "name", "password", "owner_password", "region", "browser_cookies"];

pub struct PresetsPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    list: Vec<presets::Preset>,
    selected: usize,
    // formulário de nova predefinição
    editing: bool,
    new_name: String,
    new_desc: String,
    new_tool: usize,
    new_args: Map<String, Value>,
    msg: Option<(bool, String)>,
}

impl PresetsPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(&[], "Arquivos"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            list: presets::load(),
            selected: 0,
            editing: false,
            new_name: String::new(),
            new_desc: String::new(),
            new_tool: 0,
            new_args: Map::new(),
            msg: None,
        }
    }

    /// Campos gerados a partir do esquema da ferramenta.
    fn args_form(ui: &mut Ui, p: &Palette, tool: &ops::Tool, args: &mut Map<String, Value>) {
        let schema = (tool.schema)();
        let Some(props) = schema["properties"].as_object() else { return };
        for (k, prop) in props {
            if HIDDEN.contains(&k.as_str()) {
                continue;
            }
            let label = format!("{}:", k.replace('_', " "));
            let desc = prop["description"].as_str().unwrap_or("");
            let default = prop.get("default").cloned().unwrap_or(Value::Null);
            row(ui, p, &label, |ui| {
                let cur = args.get(k).cloned().unwrap_or(default.clone());
                match prop["type"].as_str().unwrap_or("string") {
                    _ if prop["enum"].is_array() => {
                        let opts: Vec<String> = prop["enum"].as_array().unwrap().iter().filter_map(|v| v.as_str().map(String::from)).collect();
                        let mut v = cur.as_str().map(String::from).unwrap_or_else(|| opts.first().cloned().unwrap_or_default());
                        egui::ComboBox::from_id_salt(("preset_arg", k)).selected_text(v.clone()).width(200.0).show_ui(ui, |ui| {
                            for o in &opts {
                                ui.selectable_value(&mut v, o.clone(), o);
                            }
                        });
                        args.insert(k.clone(), Value::String(v));
                    }
                    "boolean" => {
                        let mut b = cur.as_bool().unwrap_or(false);
                        crate::widgets::check(ui, &mut b, "");
                        args.insert(k.clone(), Value::Bool(b));
                    }
                    "integer" | "number" => {
                        let mut v = cur.as_f64().unwrap_or(0.0);
                        let mut drag = egui::DragValue::new(&mut v).speed(1.0);
                        if let (Some(a), Some(b)) = (prop["minimum"].as_f64(), prop["maximum"].as_f64()) {
                            drag = drag.range(a..=b);
                        }
                        if prop["type"] == "integer" {
                            drag = drag.fixed_decimals(0);
                        }
                        ui.add(drag);
                        let has = args.contains_key(k) || !default.is_null();
                        if has || v != 0.0 {
                            args.insert(k.clone(), if prop["type"] == "integer" { Value::from(v.round() as i64) } else { Value::from(v) });
                        }
                    }
                    "array" => {
                        let mut s = cur.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
                        if ui.add(egui::TextEdit::singleline(&mut s).margin(crate::widgets::FIELD_MARGIN).desired_width(200.0)).changed() || args.contains_key(k) {
                            let list: Vec<Value> = s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(|x| Value::String(x.into())).collect();
                            args.insert(k.clone(), Value::Array(list));
                        }
                    }
                    _ => {
                        let mut s = cur.as_str().map(String::from).unwrap_or_default();
                        ui.add(egui::TextEdit::singleline(&mut s).margin(crate::widgets::FIELD_MARGIN).desired_width(200.0));
                        if s.is_empty() {
                            args.remove(k);
                        } else {
                            args.insert(k.clone(), Value::String(s));
                        }
                    }
                }
                ui.label(RichText::new(desc).size(12.0).color(p.muted));
            });
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Predefinições";
        widgets::header(ui, p, title, "Suas configurações favoritas, prontas para usar com um clique, pela linha de comando ou por um agente de IA (MCP).");
        let tools_list = preset_tools();
        card(ui, p, |ui| {
            if self.list.is_empty() {
                hint(ui, p, "Nenhuma predefinição ainda. Crie a primeira abaixo.");
            }
            self.selected = self.selected.min(self.list.len().saturating_sub(1));
            let mut remove = None;
            for (i, pr) in self.list.iter().enumerate() {
                ui.horizontal(|ui| {
                    let sel = i == self.selected;
                    if ui.radio(sel, RichText::new(&pr.name).family(bold()).color(p.text)).clicked() {
                        self.selected = i;
                    }
                    let tool_title = ops::find(&pr.tool).map(|t| t.title).unwrap_or("?");
                    ui.label(RichText::new(crate::i18n::tr(tool_title)).color(p.accent));
                    ui.label(RichText::new(&pr.description).size(12.5).color(p.muted));
                    ui.with_layout(crate::widgets::back(egui::Align::Center), |ui| {
                        if widgets::secondary(ui, p, "Apagar", true) {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                let name = self.list[i].name.clone();
                match presets::remove(&name) {
                    Ok(()) => self.list = presets::load(),
                    Err(e) => self.msg = Some((false, e)),
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, p, if self.editing { "Fechar formulário" } else { "+ Nova predefinição" }, true) {
                    self.editing = !self.editing;
                }
                if widgets::secondary(ui, p, "Restaurar as que vêm prontas", true) {
                    let mut list = presets::load();
                    for d in presets::defaults() {
                        if !list.iter().any(|x| x.name == d.name) {
                            list.push(d);
                        }
                    }
                    let _ = presets::store(&list);
                    self.list = list;
                }
            });
        });
        if self.editing {
            ui.add_space(4.0);
            card(ui, p, |ui| {
                row(ui, p, "Nome:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_name).margin(crate::widgets::FIELD_MARGIN).desired_width(260.0));
                });
                row(ui, p, "Descrição:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_desc).margin(crate::widgets::FIELD_MARGIN).desired_width(420.0));
                });
                row(ui, p, "Ferramenta:", |ui| {
                    let before = self.new_tool;
                    self.new_tool = self.new_tool.min(tools_list.len() - 1);
                    egui::ComboBox::from_id_salt("preset_tool")
                        .selected_text(crate::i18n::tr(tools_list[self.new_tool].title))
                        .width(260.0)
                        .show_ui(ui, |ui| {
                            for (i, t) in tools_list.iter().enumerate() {
                                ui.selectable_value(&mut self.new_tool, i, crate::i18n::tr(t.title));
                            }
                        });
                    if before != self.new_tool {
                        self.new_args.clear();
                    }
                });
                Self::args_form(ui, p, tools_list[self.new_tool], &mut self.new_args);
                let can = !self.new_name.trim().is_empty();
                if widgets::primary(ui, p, "Salvar predefinição", can) {
                    let pr = presets::Preset {
                        name: self.new_name.trim().to_string(),
                        tool: tools_list[self.new_tool].name.to_string(),
                        args: Value::Object(self.new_args.clone()),
                        description: self.new_desc.trim().to_string(),
                    };
                    match presets::save(pr) {
                        Ok(()) => {
                            self.list = presets::load();
                            self.selected = self.list.iter().position(|x| x.name == self.new_name.trim()).unwrap_or(0);
                            self.msg = Some((true, format!("✔ Predefinição salva: {}", self.new_name.trim())));
                            self.editing = false;
                            self.new_name.clear();
                            self.new_desc.clear();
                        }
                        Err(e) => self.msg = Some((false, e)),
                    }
                }
            });
        }
        if let Some((good, text)) = &self.msg {
            ui.label(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent }));
        }
        ui.add_space(4.0);
        widgets::file_list(ui, p, &mut self.queue, None, None, 150.0);
        ui.add_space(4.0);
        self.out.ui(ui, p);
        if let Some(pr) = self.list.get(self.selected) {
            hint(ui, p, &format!("Linha de comando: conver-cli preset \"{}\" arquivos…", pr.name));
        }
        let can = !self.queue.files.is_empty() && !self.list.is_empty();
        match widgets::action_bar(ui, p, &self.job, "Usar predefinição", can) {
            Act::Start => {
                let pr = self.list[self.selected].clone();
                let files: Vec<Value> = self.queue.files.iter().map(|f| Value::String(f.display().to_string())).collect();
                let out = self.out.path.clone();
                self.job.start(ui.ctx(), move |ctl| {
                    let mut args = pr.args.as_object().cloned().unwrap_or_default();
                    args.insert("files".into(), Value::Array(files));
                    if let Some(o) = out {
                        args.insert("out_dir".into(), Value::String(o.display().to_string()));
                    }
                    ctl.status(format!("Usando: {}", pr.name));
                    let progress = |f: f32| ctl.progress(f);
                    let cancelled = || ctl.cancelled();
                    let log = |m: &str| {
                        if let Some(rest) = m.strip_prefix('✔') {
                            ctl.log(LogKind::Ok, format!("✔{rest}"));
                        } else if m.starts_with('✖') {
                            ctl.log(LogKind::Err, m.to_string());
                        } else {
                            ctl.status(m.to_string());
                        }
                    };
                    let ctx = ops::Ctx { progress: &progress, cancelled: &cancelled, log: &log };
                    match ops::call(&pr.tool, &Value::Object(args), &ctx) {
                        Ok(v) => {
                            let folder = v["results"].as_array().and_then(|a| a.iter().rev().find_map(|r| r["output"].as_str())).or(v["output"].as_str())
                                .and_then(|o| std::path::Path::new(o).parent().map(|d| d.to_path_buf()));
                            ctl.status(format!("Concluído: {} ok, {} com erro.", v["ok"].as_u64().unwrap_or(1), v["failed"].as_u64().unwrap_or(0)));
                            if let Some(f) = folder {
                                ctl.log(LogKind::Info, format!("Salvo em: {}", f.display()));
                                ctl.done_folder(f);
                            }
                        }
                        Err(e) => {
                            ctl.log(LogKind::Err, format!("✖ {e}"));
                            ctl.status("Erro.");
                        }
                    }
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}
