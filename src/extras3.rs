//! Telas: gravador de áudio, texto de imagem (OCR) e informações do arquivo.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align, Layout, RichText, Ui, vec2};

use crate::info::{self, Info};
use crate::job::{Job, LogKind};
use crate::media::AFmt;
use crate::recorder::{self, Recording};
use crate::theme::{Palette, bold};
use crate::widgets::{self, Act, FileQueue, Thumbs, card, hint, row, segmented};
use crate::{history, imaging, ocr, tools, video};

fn copy_text(text: &str) -> bool {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())).is_ok()
}

// ========================================================= gravador de áudio

pub struct RecorderPage {
    devices: Option<Vec<String>>,
    device: usize,
    fmt: AFmt,
    folder: PathBuf,
    rec: Option<Recording>,
    msg: Option<(bool, String)>,
    last: Option<PathBuf>,
}

impl RecorderPage {
    pub fn new() -> Self {
        let music = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Music")).filter(|p| p.is_dir());
        Self {
            devices: None,
            device: 0,
            fmt: AFmt::Mp3,
            folder: music.unwrap_or_else(tools::exe_dir),
            rec: None,
            msg: None,
            last: None,
        }
    }

    pub fn recording(&self) -> bool {
        self.rec.is_some()
    }

    fn stop(&mut self) {
        let Some(rec) = self.rec.take() else { return };
        let secs = rec.seconds();
        let device = self.devices.as_ref().and_then(|d| d.get(self.device).cloned()).unwrap_or_default();
        match rec.stop() {
            Ok(path) => {
                let info = format!("{} • {}", video::fmt_time(secs), tools::human_size(tools::file_size(&path)));
                history::record("Gravador de áudio", &device, &path, &info);
                self.msg = Some((true, format!("✔ Gravação salva: {}  ({info})", tools::file_name(&path))));
                self.last = Some(path);
            }
            Err(e) => self.msg = Some((false, format!("✖ {e}"))),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(ui, p, "Gravador de áudio", "Grave o microfone (ou outra entrada de áudio do PC) direto em MP3, WAV ou M4A.");
        if self.devices.is_none() {
            self.devices = Some(recorder::devices());
        }
        let recording = self.rec.is_some();
        // o ffmpeg parou sozinho? (microfone desconectado, em uso por outro programa…)
        match self.rec.as_mut().map(|r| r.check()) {
            Some(Err(e)) => {
                self.rec = None;
                self.msg = Some((false, format!("✖ A gravação parou: {e}")));
            }
            Some(Ok(false)) => self.stop(),
            _ => {}
        }
        let devices = self.devices.clone().unwrap_or_default();
        card(ui, p, |ui| {
            ui.add_enabled_ui(!recording, |ui| {
                row(ui, p, "Microfone:", |ui| {
                    if devices.is_empty() {
                        ui.label(RichText::new(crate::i18n::tr("nenhuma entrada de áudio encontrada")).color(p.accent));
                    } else {
                        self.device = self.device.min(devices.len() - 1);
                        egui::ComboBox::from_id_salt("mic").selected_text(crate::i18n::tr(&devices[self.device])).width(320.0).show_ui(ui, |ui| {
                            for (i, d) in devices.iter().enumerate() {
                                ui.selectable_value(&mut self.device, i, crate::i18n::tr(d));
                            }
                        });
                    }
                    if widgets::secondary(ui, p, "Atualizar lista", true) {
                        self.devices = Some(recorder::devices());
                    }
                });
                row(ui, p, "Formato:", |ui| {
                    segmented(ui, p, &mut self.fmt, &[(AFmt::Mp3, "MP3"), (AFmt::Wav, "WAV (sem perda)"), (AFmt::M4a, "M4A")]);
                });
                row(ui, p, "Salvar em:", |ui| {
                    ui.label(RichText::new(crate::i18n::tr(self.folder.display().to_string())).color(p.text));
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
            ui.vertical_centered(|ui| {
                let secs = self.rec.as_ref().map(|r| r.seconds()).unwrap_or(0.0);
                let (dot, color) = if recording { ("●  Gravando", p.accent) } else { ("Pronto para gravar", p.muted) };
                ui.label(RichText::new(crate::i18n::tr(dot)).family(bold()).color(color));
                ui.label(RichText::new(crate::i18n::tr(video::fmt_time(secs))).family(bold()).size(52.0).color(p.text));
                if self.rec.as_ref().is_some_and(|r| r.silent()) {
                    ui.label(
                        RichText::new(crate::i18n::tr("⚠ Nenhum som está chegando desta entrada. Pare e escolha outro microfone na lista."))
                            .color(p.accent),
                    );
                }
                ui.add_space(6.0);
                if recording {
                    if widgets::primary(ui, p, "■  Parar", true) {
                        self.stop();
                    }
                } else if widgets::primary(ui, p, "●  Gravar", !devices.is_empty()) {
                    let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
                    let dst = tools::unique_path(self.folder.join(format!("gravacao_{stamp}.{}", self.fmt.ext())));
                    let ctx = ui.ctx().clone();
                    match Recording::start(&devices[self.device], self.fmt, &dst, move || ctx.request_repaint()) {
                        Ok(rec) => {
                            self.rec = Some(rec);
                            self.msg = None;
                        }
                        Err(e) => self.msg = Some((false, format!("✖ {e}"))),
                    }
                }
                ui.add_space(4.0);
            });
        });
        if let Some((good, text)) = &self.msg {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent }));
                if let (true, Some(last)) = (*good, &self.last) {
                    if widgets::secondary(ui, p, "Ouvir", true) {
                        tools::open_file(last);
                    }
                    if widgets::secondary(ui, p, "Mostrar na pasta", true) {
                        tools::show_in_folder(last);
                    }
                }
            });
        }
        hint(ui, p, "Dica: se a lista estiver vazia, confira em Configurações do Windows › Privacidade › Microfone se os apps podem usar o microfone.");
        if recording {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

// ======================================================= texto de imagem (OCR)

pub struct OcrPage {
    pub queue: FileQueue,
    langs: Option<Vec<(String, String)>>,
    lang: usize,
    results: Arc<Mutex<HashMap<PathBuf, Result<String, String>>>>,
    shown: Option<PathBuf>,
    text: String,
    job: Job,
    msg: Option<String>,
}

impl OcrPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(imaging::IMAGE_EXTS, "Imagens"),
            langs: None,
            lang: 0,
            results: Arc::default(),
            shown: None,
            text: String::new(),
            job: Job::default(),
            msg: None,
        }
    }

    /// Cola um print da área de transferência como imagem na lista.
    fn paste_image(&mut self) -> Result<(), String> {
        let img = arboard::Clipboard::new().and_then(|mut c| c.get_image()).map_err(|_| "a área de transferência não tem imagem.")?;
        let rgba = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()).ok_or("imagem inválida.")?;
        let dir = tools::app_dir().join("prints");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
        let path = tools::unique_path(dir.join(format!("print_{stamp}.png")));
        rgba.save(&path).map_err(|e| e.to_string())?;
        let first = self.queue.files.len();
        self.queue.add([path]);
        self.queue.selected = first;
        Ok(())
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, thumbs: &Thumbs) {
        widgets::header(
            ui,
            p,
            "Texto de imagem (OCR)",
            "Leia o texto de prints, fotos de documentos e placas usando o reconhecimento do próprio Windows, sem internet.",
        );
        if self.langs.is_none() {
            self.langs = Some(ocr::languages());
        }
        // mostra o resultado da imagem selecionada (sem apagar o que o usuário estiver editando)
        let selected = self.queue.selected_path().cloned();
        let result = selected.as_ref().and_then(|s| self.results.lock().unwrap().get(s).cloned());
        if selected != self.shown || (self.text.is_empty() && result.is_some()) {
            self.text = match &result {
                Some(Ok(t)) => t.clone(),
                _ => String::new(),
            };
            self.shown = selected.clone();
        }

        let height = 300.0;
        ui.horizontal_top(|ui| {
            let left = (ui.available_width() * 0.40).max(300.0);
            ui.allocate_ui_with_layout(vec2(left, height + 80.0), Layout::top_down(Align::Min), |ui| {
                widgets::file_list(ui, p, &mut self.queue, Some(thumbs), None, height);
            });
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height + 80.0), Layout::top_down(Align::Min), |ui| {
                card(ui, p, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Texto encontrado")).family(bold()).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::secondary(ui, p, "Salvar .txt", !self.text.is_empty()) {
                                let name = selected.as_ref().and_then(|s| s.file_stem()).map(|s| s.to_string_lossy().into_owned());
                                if let Some(dst) = rfd::FileDialog::new()
                                    .set_file_name(format!("{}.txt", name.unwrap_or_else(|| "texto".into())))
                                    .add_filter(crate::i18n::tr("Texto"), &["txt"])
                                    .save_file()
                                {
                                    self.msg = Some(match std::fs::write(&dst, &self.text) {
                                        Ok(()) => format!("✔ Salvo em {}", dst.display()),
                                        Err(e) => format!("✖ {e}"),
                                    });
                                }
                            }
                            if widgets::secondary(ui, p, "Copiar", !self.text.is_empty()) && copy_text(&self.text) {
                                self.msg = Some("✔ Texto copiado.".into());
                            }
                        });
                    });
                    match &result {
                        Some(Err(e)) => {
                            ui.label(RichText::new(crate::i18n::tr(format!("✖ {e}"))).color(p.accent));
                        }
                        None if selected.is_some() => hint(ui, p, "Clique em \"Ler texto\" para reconhecer o texto desta imagem."),
                        Some(Ok(t)) if t.trim().is_empty() => hint(ui, p, "Nenhum texto encontrado nesta imagem."),
                        _ => {}
                    }
                    egui::ScrollArea::vertical().id_salt("ocr_text").max_height(height - 40.0).show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(12)
                                .hint_text(crate::i18n::tr("O texto aparece aqui. Você pode corrigir antes de copiar.")),
                        );
                    });
                });
            });
        });
        ui.add_space(4.0);
        let langs = self.langs.clone().unwrap_or_default();
        card(ui, p, |ui| {
            row(ui, p, "Idioma:", |ui| {
                let names: Vec<String> = std::iter::once("Automático (idiomas do Windows)".to_string())
                    .chain(langs.iter().map(|(_, n)| n.clone()))
                    .collect();
                self.lang = self.lang.min(names.len() - 1);
                egui::ComboBox::from_id_salt("ocr_lang").selected_text(crate::i18n::tr(&names[self.lang])).width(260.0).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut self.lang, i, crate::i18n::tr(n));
                    }
                });
            });
            row(ui, p, "Print:", |ui| {
                if widgets::secondary(ui, p, "Colar imagem (Ctrl+V)", true) {
                    if let Err(e) = self.paste_image() {
                        self.msg = Some(format!("✖ {e}"));
                    }
                }
                hint(ui, p, "tire um print com Win+Shift+S e cole aqui");
            });
            if langs.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Nenhum idioma de OCR instalado no Windows. Instale em Configurações › Hora e idioma › Idioma.")).color(p.accent));
            } else {
                hint(ui, p, "Outros idiomas: Configurações do Windows › Hora e idioma › Idioma › adicionar o idioma com \"Reconhecimento óptico\".");
            }
        });
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::V)) && !ui.ctx().wants_keyboard_input() {
            if let Err(e) = self.paste_image() {
                self.msg = Some(format!("✖ {e}"));
            }
        }
        if let Some(m) = &self.msg {
            ui.label(RichText::new(crate::i18n::tr(m)).color(if m.starts_with('✔') { p.ok } else { p.accent }));
        }

        match widgets::action_bar(ui, p, &self.job, "Ler texto", !self.queue.files.is_empty()) {
            Act::Start => {
                let files = self.queue.files.clone();
                let lang = if self.lang == 0 { String::new() } else { langs[self.lang - 1].0.clone() };
                let results = self.results.clone();
                self.text.clear();
                self.job.start(ui.ctx(), move |ctl| {
                    let n = files.len();
                    let mut ok = 0;
                    for (i, f) in files.iter().enumerate() {
                        if ctl.cancelled() {
                            break;
                        }
                        let name = tools::file_name(f);
                        ctl.status(format!("[{}/{n}] Lendo {name}…", i + 1));
                        let r = ocr::recognize(f, &lang);
                        match &r {
                            Ok(t) => {
                                ok += 1;
                                let lines = t.lines().filter(|l| !l.trim().is_empty()).count();
                                ctl.log(LogKind::Ok, format!("✔ {name}   {lines} linha(s)"));
                            }
                            Err(e) => ctl.log(LogKind::Err, format!("✖ {name}: {e}")),
                        }
                        results.lock().unwrap().insert(f.clone(), r);
                        ctl.progress((i + 1) as f32 / n as f32);
                    }
                    ctl.status(format!("Concluído: {ok} de {n} imagem(ns)."));
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ===================================================== informações do arquivo

pub struct InfoPage {
    pub queue: FileQueue,
    cache: HashMap<PathBuf, Info>,
    msg: Option<String>,
}

impl InfoPage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(&[], "Arquivos"), cache: HashMap::new(), msg: None }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(
            ui,
            p,
            "Informações do arquivo",
            "Veja os detalhes de imagens, vídeos, áudios e PDFs: resolução, duração, codecs, câmera, localização da foto e mais.",
        );
        let height = 420.0;
        ui.horizontal_top(|ui| {
            let left = (ui.available_width() * 0.38).max(280.0);
            ui.allocate_ui_with_layout(vec2(left, height + 80.0), Layout::top_down(Align::Min), |ui| {
                widgets::file_list(ui, p, &mut self.queue, None, None, height);
            });
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height + 80.0), Layout::top_down(Align::Min), |ui| {
                card(ui, p, |ui| {
                    let Some(path) = self.queue.selected_path().cloned() else {
                        ui.label(RichText::new(crate::i18n::tr("Adicione ou arraste um arquivo para ver as informações.")).color(p.muted));
                        return;
                    };
                    let info = self.cache.entry(path.clone()).or_insert_with(|| info::read(&path));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tr(tools::file_name(&path))).family(bold()).color(p.text));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::secondary(ui, p, "Copiar tudo", true) && copy_text(&info.as_text()) {
                                self.msg = Some("✔ Informações copiadas.".into());
                            }
                            if let Some((lat, lon)) = info.gps {
                                if widgets::secondary(ui, p, "Ver no mapa", true) {
                                    let url = format!("https://www.openstreetmap.org/?mlat={lat}&mlon={lon}#map=16/{lat}/{lon}");
                                    let _ = std::process::Command::new("explorer").arg(url).spawn();
                                }
                            }
                        });
                    });
                    egui::ScrollArea::vertical().id_salt("info").max_height(height).auto_shrink([false, true]).show(ui, |ui| {
                        for sec in &info.sections {
                            ui.add_space(6.0);
                            ui.label(RichText::new(crate::i18n::tr(sec.title)).family(bold()).size(13.0).color(p.accent));
                            egui::Grid::new(("info_grid", sec.title)).num_columns(2).spacing([16.0, 4.0]).striped(false).show(ui, |ui| {
                                for (k, v) in &sec.rows {
                                    ui.label(RichText::new(crate::i18n::tr(k)).color(p.muted));
                                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr(v)).color(p.text)).wrap());
                                    ui.end_row();
                                }
                            });
                        }
                        if info.gps.is_some() {
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(crate::i18n::tr("⚠ Esta foto tem a localização de onde foi tirada. Use \"Remover metadados\" antes de compartilhar."))
                                    .color(p.accent),
                            );
                        }
                    });
                });
            });
        });
        if let Some(m) = &self.msg {
            ui.label(RichText::new(crate::i18n::tr(m)).color(p.ok));
        }
    }
}
