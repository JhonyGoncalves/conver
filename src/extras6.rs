//! Telas da leva 1.7: marca d'água em lote.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::i18n::tr;
use crate::job::Job;
use crate::pages::run_batch;
use crate::theme::Palette;
use crate::watermark::{self, Mark, Opts, Pos};
use crate::widgets::{self, Act, FileQueue, OutDir, card, hint, row, segmented};

// i18n: fora
pub const WATERMARK_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "bmp", "gif", "tif", "tiff", "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "ts", "pdf",
];
// i18n: dentro

/// Prévia calculada em segundo plano (com uma pequena espera depois da última mudança).
#[derive(Default)]
struct LivePreview {
    tex: Option<egui::TextureHandle>,
    shown: String,
    want: String,
    since: Option<Instant>,
    busy: Arc<AtomicBool>,
    slot: Arc<Mutex<Option<(String, Result<egui::ColorImage, String>)>>>,
    err: Option<String>,
}

impl LivePreview {
    /// Pede uma prévia nova se `key` mudou; `make` roda numa thread e devolve um PNG.
    fn update(&mut self, ctx: &egui::Context, key: String, make: impl FnOnce() -> Result<Vec<u8>, String> + Send + 'static) {
        if let Some((k, r)) = self.slot.lock().unwrap().take() {
            match r {
                Ok(img) => {
                    self.tex = Some(ctx.load_texture("previa_marca", img, egui::TextureOptions::LINEAR));
                    self.err = None;
                }
                Err(e) => self.err = Some(e),
            }
            self.shown = k;
        }
        if key != self.want {
            self.want = key;
            self.since = Some(Instant::now());
        }
        if self.want == self.shown || self.busy.load(Ordering::Relaxed) {
            return;
        }
        if !self.since.is_some_and(|t| t.elapsed() > Duration::from_millis(250)) {
            ctx.request_repaint_after(Duration::from_millis(260));
            return;
        }
        self.busy.store(true, Ordering::Relaxed);
        let (busy, slot, ctx, key) = (self.busy.clone(), self.slot.clone(), ctx.clone(), self.want.clone());
        std::thread::spawn(move || {
            let r = make().and_then(|png| {
                let img = image::load_from_memory(&png).map_err(|e| e.to_string())?.to_rgba8();
                Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
            });
            *slot.lock().unwrap() = Some((key, r));
            busy.store(false, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }

    fn show(&self, ui: &mut Ui, p: &Palette, size: egui::Vec2, empty: &str) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, p.input);
        match &self.tex {
            Some(tex) => {
                let [w, h] = tex.size();
                let shown = widgets::fit((w as f32, h as f32), rect.size() - vec2(8.0, 8.0));
                let r = egui::Rect::from_center_size(rect.center(), shown);
                widgets::checkerboard(&painter, r);
                painter.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                painter.rect_stroke(r, 0, Stroke::new(1.0f32, p.border), StrokeKind::Outside);
            }
            None => {
                painter.text(rect.center(), Align2::CENTER_CENTER, tr(empty), FontId::proportional(12.0), p.muted);
            }
        }
        if self.want != self.shown {
            painter.circle_filled(rect.right_top() + vec2(-9.0, 9.0), 3.5, p.accent);
        }
        if let Some(e) = &self.err {
            painter.text(rect.center_bottom() - vec2(0.0, 8.0), Align2::CENTER_BOTTOM, tr(e), FontId::proportional(11.0), p.accent);
        }
    }
}

/// Imagem base da prévia: a própria imagem, um quadro do vídeo ou a primeira página do PDF (PNG temporário).
fn base_image(src: &std::path::Path) -> Result<PathBuf, String> {
    let ext = crate::tools::ext_lower(src);
    let tmp = std::env::temp_dir().join(format!("conver_base_{}.png", std::process::id()));
    if crate::imaging::IMAGE_EXTS.contains(&ext.as_str()) {
        let img = image::open(src).map_err(|e| e.to_string())?;
        img.thumbnail(1280, 1280).save(&tmp).map_err(|e| e.to_string())?;
    } else if ext == "pdf" {
        let mut first: Option<Vec<u8>> = None;
        crate::pdf_extra::thumbnails(src, 72.0, |_, png| {
            first = Some(png);
            false
        })?;
        std::fs::write(&tmp, first.ok_or("não foi possível ler a primeira página.")?).map_err(|e| e.to_string())?;
    } else {
        crate::classify::video_frame(src)?.save(&tmp).map_err(|e| e.to_string())?;
    }
    Ok(tmp)
}

// ============================================================== marca d'água

pub struct WatermarkPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    logo_mode: bool,
    text: String,
    color: [u8; 3],
    logo: Option<PathBuf>,
    pos: Pos,
    opacity: u8,
    text_size: f32,
    logo_size: f32,
    margin: f32,
    prev: LivePreview,
}

impl WatermarkPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(WATERMARK_EXTS, "Imagens, vídeos e PDFs"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            logo_mode: false,
            text: String::new(),
            color: [0xFF, 0xFF, 0xFF],
            logo: None,
            pos: Pos::BottomRight,
            opacity: 70,
            text_size: 5.0,
            logo_size: 18.0,
            margin: 3.0,
            prev: LivePreview::default(),
        }
    }

    fn opts(&self) -> Opts {
        let mark = if self.logo_mode {
            Mark::Logo { path: self.logo.clone().unwrap_or_default() }
        } else {
            Mark::Text { text: self.text.clone(), color: self.color }
        };
        Opts { mark, pos: self.pos, opacity: self.opacity, size: if self.logo_mode { self.logo_size } else { self.text_size }, margin: self.margin }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Marca d'água";
        widgets::header(ui, p, title, "Coloque seu logo ou um texto em várias imagens, vídeos e PDFs de uma vez, com posição e transparência.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 120.0);
        ui.add_space(4.0);
        let opts = self.opts();
        let sel = self.queue.selected_path().cloned();
        if let Some(src) = sel.clone() {
            let key = format!("{src:?}|{opts:?}");
            let o = opts.clone();
            let ready = match &o.mark {
                Mark::Text { text, .. } => !text.trim().is_empty(),
                Mark::Logo { path } => path.is_file(),
            };
            self.prev.update(ui.ctx(), key, move || {
                let base = base_image(&src)?;
                // sem texto ou logo ainda: mostra a imagem como está
                if ready { watermark::preview(&base, &o, 420) } else { std::fs::read(&base).map_err(|e| e.to_string()) }
            });
        }
        card(ui, p, |ui| {
            ui.horizontal_top(|ui| {
                crate::widgets::vt(ui, |ui| {
                    ui.set_width((ui.available_width() - 250.0).max(330.0));
                    row(ui, p, "Marca:", |ui| {
                        segmented(ui, p, &mut self.logo_mode, &[(false, "Texto"), (true, "Logo (imagem)")]);
                    });
                    if self.logo_mode {
                        row(ui, p, "Logo:", |ui| {
                            if widgets::secondary(ui, p, "Escolher imagem…", true) {
                                if let Some(f) = rfd::FileDialog::new().add_filter("PNG, JPG, WEBP", &["png", "jpg", "jpeg", "webp"]).pick_file() {
                                    self.logo = Some(f);
                                }
                            }
                            let name = self.logo.as_ref().map(|l| crate::tools::file_name(l)).unwrap_or_else(|| tr("nenhuma"));
                            ui.label(RichText::new(name).color(p.muted));
                        });
                        hint(ui, p, "Um PNG com fundo transparente fica melhor.");
                    } else {
                        row(ui, p, "Texto:", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.text).hint_text(tr("ex.: © Seu nome 2026")).desired_width(220.0).margin(widgets::FIELD_MARGIN));
                            egui::color_picker::color_edit_button_srgb(ui, &mut self.color);
                        });
                    }
                    row(ui, p, "Posição:", |ui| {
                        crate::widgets::vt(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                            for line in Pos::GRID {
                                ui.horizontal(|ui| {
                                    for pos in line {
                                        let on = self.pos == pos;
                                        let b = egui::Button::new("").min_size(vec2(26.0, 20.0)).fill(if on { p.accent } else { p.input }).corner_radius(4);
                                        if ui.add(b).clicked() {
                                            self.pos = pos;
                                        }
                                    }
                                });
                            }
                        });
                        ui.add_space(10.0);
                        let diag = self.pos == Pos::Diagonal;
                        if ui.selectable_label(diag, tr("Diagonal, no meio")).clicked() {
                            self.pos = if diag { Pos::Center } else { Pos::Diagonal };
                        }
                    });
                    row(ui, p, "Tamanho:", |ui| {
                        let v = if self.logo_mode { &mut self.logo_size } else { &mut self.text_size };
                        let range = if self.logo_mode { 3.0..=80.0 } else { 1.0..=25.0 };
                        ui.add(egui::Slider::new(v, range).step_by(0.5).suffix(" %"));
                    });
                    row(ui, p, "Opacidade:", |ui| {
                        ui.add(egui::Slider::new(&mut self.opacity, 5..=100).suffix(" %"));
                    });
                    row(ui, p, "Margem:", |ui| {
                        ui.add(egui::Slider::new(&mut self.margin, 0.0..=15.0).step_by(0.5).suffix(" %"));
                    });
                });
                ui.add_space(8.0);
                crate::widgets::vt(ui, |ui| {
                    ui.set_width(232.0);
                    self.prev.show(ui, p, vec2(232.0, 300.0), if sel.is_some() { "Preparando…" } else { "Adicione arquivos para ver a prévia." });
                });
            });
            hint(ui, p, "Imagens e PDFs ficam com a mesma qualidade; vídeos são regravados com a marca (o áudio é copiado).");
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let ready = !self.queue.files.is_empty() && if self.logo_mode { self.logo.is_some() } else { !self.text.trim().is_empty() };
        match widgets::action_bar(ui, p, &self.job, "Aplicar", ready) {
            Act::Start => {
                let (files, out, o) = (self.queue.files.clone(), self.out.path.clone(), opts);
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| watermark::apply(src, out.as_deref(), &o, progress, &|| ctl.cancelled()));
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ============================================================== desfocar rostos

// i18n: fora
pub const FACE_EXTS: &[&str] = &["png", "jpg", "jpeg", "jfif", "webp", "bmp", "tif", "tiff", "mp4", "mkv", "avi", "mov", "webm", "flv", "wmv", "m4v", "mpg", "mpeg", "3gp", "ts"];
// i18n: dentro

pub struct FacesPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    style: crate::faces::Style,
    sensitivity: f32,
    prev: LivePreview,
    found: Arc<Mutex<Option<usize>>>,
}

impl FacesPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(FACE_EXTS, "Fotos e vídeos"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            style: crate::faces::Style::Desfoque,
            sensitivity: 60.0,
            prev: LivePreview::default(),
            found: Arc::default(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        use crate::faces::Style;
        let title = "Desfocar rostos";
        widgets::header(ui, p, title, "Ache e esconda os rostos de fotos e vídeos para preservar a privacidade, no seu PC e sem internet.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 120.0);
        ui.add_space(4.0);
        let (style, min) = (self.style, self.sensitivity / 100.0);
        if let Some(src) = self.queue.selected_path().cloned() {
            let found = self.found.clone();
            self.prev.update(ui.ctx(), format!("{src:?}|{style:?}|{min}"), move || {
                let base = base_image(&src)?;
                let mut img = image::open(&base).map_err(|e| e.to_string())?.to_rgb8();
                let faces = crate::faces::detect(&img, min)?;
                *found.lock().unwrap() = Some(faces.len());
                crate::faces::hide(&mut img, &faces, style, 0.25);
                let mut png = Vec::new();
                image::DynamicImage::ImageRgb8(img).thumbnail(800, 600).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
                Ok(png)
            });
        }
        card(ui, p, |ui| {
            ui.horizontal_top(|ui| {
                crate::widgets::vt(ui, |ui| {
                    ui.set_width((ui.available_width() - 316.0).max(300.0));
                    row(ui, p, "Esconder com:", |ui| {
                        segmented(ui, p, &mut self.style, &[(Style::Desfoque, "Desfoque"), (Style::Pixelado, "Pixelado"), (Style::Tarja, "Tarja preta")]);
                    });
                    row(ui, p, "Sensibilidade:", |ui| {
                        // mais sensível = aceita rostos com nota menor
                        let mut s = 100.0 - self.sensitivity;
                        if ui.add(egui::Slider::new(&mut s, 5.0..=80.0).show_value(false)).changed() {
                            self.sensitivity = 100.0 - s;
                        }
                    });
                    hint(ui, p, "Mais sensível acha rostos pequenos, de lado ou no escuro, mas pode marcar o que não é rosto.");
                    if let Some(n) = *self.found.lock().unwrap() {
                        ui.label(RichText::new(tr(format!("{n} rosto(s) na prévia"))).color(p.muted));
                    }
                    hint(ui, p, "Vídeos são analisados quadro a quadro (o som é copiado); um vídeo de 1 minuto em 1080p leva cerca de 1 a 2 minutos.");
                });
                ui.add_space(8.0);
                crate::widgets::vt(ui, |ui| {
                    ui.set_width(296.0);
                    self.prev.show(ui, p, vec2(296.0, 240.0), if self.queue.selected_path().is_some() { "Preparando…" } else { "Adicione fotos ou vídeos para ver a prévia." });
                });
            });
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        match widgets::action_bar(ui, p, &self.job, "Esconder rostos", !self.queue.files.is_empty()) {
            Act::Start => {
                let (files, out) = (self.queue.files.clone(), self.out.path.clone());
                let min = self.sensitivity / 100.0;
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| crate::faces::process(src, out.as_deref(), style, min, progress, &|| ctl.cancelled()));
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ============================================================== traduzir legendas e dublar

// i18n: fora
pub const TRANSLATE_EXTS: &[&str] = &["srt", "vtt", "mp4", "mkv", "avi", "mov", "webm", "m4v", "mp3", "wav", "m4a", "flac", "ogg", "opus"];
// i18n: dentro

pub struct TranslatePage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    /// Índice em `translate::LANGS` (`None` = não traduzir).
    to: Option<usize>,
    dub: bool,
    voice: usize,
    original: f32,
}

impl TranslatePage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(TRANSLATE_EXTS, "Legendas, vídeos e áudios"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            to: Some(1),
            dub: false,
            voice: 0,
            original: 20.0,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        use crate::translate::LANGS;
        let title = "Traduzir e dublar";
        widgets::header(ui, p, title, "Traduza legendas para outra língua e, se quiser, duble o vídeo com uma voz natural, tudo no seu PC e sem internet.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 120.0);
        ui.add_space(4.0);
        // vozes para a dublagem: as do idioma de destino (ou todas, sem traduzir)
        let target = self.to.map(|i| LANGS[i].0);
        let voices: Vec<&'static crate::tts::Voice> = crate::tts::VOICES.iter().filter(|v| target.is_none_or(|t| v.lang == t)).collect();
        if self.voice >= voices.len() {
            self.voice = 0;
        }
        card(ui, p, |ui| {
            row(ui, p, "Traduzir para:", |ui| {
                let label = self.to.map(|i| tr(LANGS[i].2)).unwrap_or_else(|| tr("Não traduzir (só dublar)"));
                egui::ComboBox::from_id_salt("tr_to").selected_text(label).width(230.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.to, None, tr("Não traduzir (só dublar)"));
                    for (i, l) in LANGS.iter().enumerate() {
                        ui.selectable_value(&mut self.to, Some(i), tr(l.2));
                    }
                });
            });
            row(ui, p, "Dublar:", |ui| {
                crate::widgets::check_enabled(ui, !voices.is_empty(), &mut self.dub, tr("Gerar a fala com uma voz natural"));
            });
            if voices.is_empty() {
                hint(ui, p, "Ainda não há voz para dublar neste idioma; a legenda traduzida é salva normalmente.");
            }
            if self.dub && !voices.is_empty() {
                row(ui, p, "Voz:", |ui| {
                    egui::ComboBox::from_id_salt("tr_voice").selected_text(tr(voices[self.voice].label)).width(300.0).show_ui(ui, |ui| {
                        for (i, v) in voices.iter().enumerate() {
                            ui.selectable_value(&mut self.voice, i, tr(v.label));
                        }
                    });
                });
                row(ui, p, "Som original:", |ui| {
                    ui.add(egui::Slider::new(&mut self.original, 0.0..=60.0).step_by(5.0).suffix(" %"));
                });
                hint(ui, p, "Cada fala entra na hora da legenda; quando não cabe no tempo, ela é acelerada (no máximo 1,5×). O vídeo com o mesmo nome da legenda recebe a dublagem; sem vídeo, sai um MP3.");
            }
            if self.to.is_some() {
                crate::models::hint(ui, p, crate::models::Pack::Language);
            }
            hint(ui, p, "Vídeos e áudios sem legenda são transcritos antes (Whisper). A tradução é feita por uma IA local pequena: revise nomes e termos técnicos.");
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let ready = !self.queue.files.is_empty() && (self.to.is_some() || (self.dub && !voices.is_empty()));
        match widgets::action_bar(ui, p, &self.job, if self.dub { "Traduzir e dublar" } else { "Traduzir" }, ready) {
            Act::Start => {
                let o = crate::translate::Opts {
                    to: self.to.map(|i| LANGS[i].0.to_string()),
                    voice: if self.dub { voices.get(self.voice).copied() } else { None },
                    video: None,
                    original: self.original / 100.0,
                };
                let (files, out) = (self.queue.files.clone(), self.out.path.clone());
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| crate::translate::run(src, out.as_deref(), &o, progress, &|| ctl.cancelled()));
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ============================================================== escanear documento

// i18n: fora
pub const SCAN_EXTS: &[&str] = &["jpg", "jpeg", "jfif", "png", "webp", "bmp", "tif", "tiff", "heic"];
// i18n: dentro

pub struct ScanPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    mode: crate::docscan::Mode,
    crop: bool,
    format: crate::docscan::Out,
    pdf_name: String,
    prev: LivePreview,
    found: Arc<Mutex<Option<bool>>>,
}

impl ScanPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(SCAN_EXTS, "Fotos de documentos"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            mode: crate::docscan::Mode::Documento,
            crop: true,
            format: crate::docscan::Out::Pdf,
            pdf_name: "documento_escaneado".into(),
            prev: LivePreview::default(),
            found: Arc::default(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        use crate::docscan::{Mode, Opts, Out};
        let title = "Escanear documento";
        widgets::header(ui, p, title, "Foto de papel tirada com o celular vira documento reto e limpo: o Conver acha a folha, endireita e tira as sombras. Para PDF ou OCR.");
        self.queue.reorder = self.format == Out::Pdf;
        widgets::file_list(ui, p, &mut self.queue, None, None, 120.0);
        ui.add_space(4.0);
        let o = Opts { mode: self.mode, crop: self.crop };
        if let Some(src) = self.queue.selected_path().cloned() {
            let found = self.found.clone();
            self.prev.update(ui.ctx(), format!("{src:?}|{o:?}"), move || {
                let img = crate::imaging::load(&src)?.thumbnail(1400, 1400).to_rgb8();
                let (out, ok) = crate::docscan::scan(&img, &o);
                *found.lock().unwrap() = Some(ok);
                let mut png = Vec::new();
                image::DynamicImage::ImageRgb8(out).thumbnail(800, 600).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
                Ok(png)
            });
        }
        card(ui, p, |ui| {
            ui.horizontal_top(|ui| {
                crate::widgets::vt(ui, |ui| {
                    ui.set_width((ui.available_width() - 316.0).max(300.0));
                    row(ui, p, "Limpeza:", |ui| {
                        segmented(ui, p, &mut self.mode, &[(Mode::Documento, "Documento (preto e branco)"), (Mode::Cinza, "Tons de cinza"), (Mode::Cor, "Cor")]);
                    });
                    row(ui, p, "Recorte:", |ui| {
                        segmented(ui, p, &mut self.crop, &[(true, "Achar a folha e endireitar"), (false, "Foto inteira")]);
                    });
                    row(ui, p, "Salvar como:", |ui| {
                        segmented(ui, p, &mut self.format, &[(Out::Pdf, "PDF (tudo num arquivo)"), (Out::Jpg, "JPG"), (Out::Png, "PNG")]);
                    });
                    if self.format == Out::Pdf {
                        row(ui, p, "Nome do PDF:", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.pdf_name).margin(widgets::FIELD_MARGIN).desired_width(200.0));
                            ui.label(RichText::new(".pdf").color(p.muted));
                        });
                        hint(ui, p, "Uma página por foto, na ordem da lista (use ↑ e ↓ para mudar).");
                    }
                    hint(ui, p, "Funciona melhor com a folha inteira na foto, sobre uma mesa mais escura que o papel.");
                    if let Some(false) = *self.found.lock().unwrap() {
                        if self.crop {
                            ui.label(RichText::new(tr("⚠ A folha não foi encontrada nesta foto: ela será só limpa, sem recorte.")).size(12.5).color(p.accent));
                        }
                    }
                });
                ui.add_space(8.0);
                crate::widgets::vt(ui, |ui| {
                    ui.set_width(296.0);
                    self.prev.show(ui, p, vec2(296.0, 240.0), if self.queue.selected_path().is_some() { "Preparando…" } else { "Adicione fotos de documentos para ver a prévia." });
                });
            });
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        match widgets::action_bar(ui, p, &self.job, "Escanear", !self.queue.files.is_empty()) {
            Act::Start => {
                let (files, out, format) = (self.queue.files.clone(), self.out.path.clone(), self.format);
                let name = if self.pdf_name.trim().is_empty() { "documento_escaneado".to_string() } else { self.pdf_name.trim().to_string() };
                self.job.start(ui.ctx(), move |ctl| {
                    if format == Out::Pdf {
                        let first = files[0].clone();
                        run_batch(ctl, title, &[first.clone()], |_, progress| {
                            let dir = out.clone().or_else(|| first.parent().map(|d| d.to_path_buf())).unwrap_or_default();
                            let dst = crate::tools::unique_path(dir.join(format!("{name}.pdf")));
                            let n = crate::docscan::to_pdf(&files, &dst, &o, progress, &|| ctl.cancelled())?;
                            Ok((tr(format!("{n} página(s)")), dst))
                        });
                    } else {
                        run_batch(ctl, title, &files, |src, _| crate::docscan::process(src, out.as_deref(), &o, format));
                    }
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}
