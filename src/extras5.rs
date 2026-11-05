//! Telas da leva 1.5: legendas animadas e apagar objeto.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::captions::{Look, Place, Style, Tint};
use crate::job::Job;
use crate::pages::run_batch;
use crate::theme::{Palette, bold};
use crate::transcribe::{self, WModel};
use crate::widgets::{self, Act, Durations, FileQueue, OutDir, card, hint, row, segmented};

// ============================================================ legendas animadas

pub struct CaptionsPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    style: Style,
    model: WModel,
    lang: usize,
    keep_ass: bool,
}

impl CaptionsPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(crate::video::VIDEO_EXTS, "Vídeos"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            style: Style::default(),
            model: WModel::Rapido,
            lang: 1, // português
            keep_ass: false,
        }
    }

    /// Prévia do estilo: um quadro 9:16 com uma frase de exemplo, a palavra do meio destacada.
    fn preview(&self, ui: &mut Ui, p: &Palette) {
        let (rect, _) = ui.allocate_exact_size(vec2(150.0, 266.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 10, Color32::from_rgb(0x2A, 0x33, 0x3D));
        painter.rect_stroke(rect, 10, Stroke::new(1.0f32, p.border), StrokeKind::Inside);
        let hl = match self.style.tint {
            Tint::Amarelo => Color32::from_rgb(0xFF, 0xE1, 0x00),
            Tint::Verde => Color32::from_rgb(0x6A, 0xE0, 0x47),
            Tint::Cinabrio => p.accent,
            Tint::Azul => Color32::from_rgb(0x38, 0xB2, 0xFF),
        };
        let sample = ["isso", "fica", "incrível"];
        let words: Vec<String> = sample.iter().map(|w| if self.style.uppercase { w.to_uppercase() } else { w.to_string() }).collect();
        let y = match self.style.place {
            Place::Baixo => rect.bottom() - 48.0,
            Place::Meio => rect.center().y,
            Place::Cima => rect.top() + 44.0,
        };
        let size = if self.style.look == Look::UmaPalavra { 21.0 } else { 15.0 };
        let font = FontId::new(size, bold());
        let shown: Vec<(String, bool)> = match self.style.look {
            Look::UmaPalavra => vec![(words[1].clone(), true)],
            _ => words.iter().enumerate().map(|(i, w)| (w.clone(), i == 1)).collect(),
        };
        let galleys: Vec<_> = shown.iter().map(|(w, _)| painter.layout_no_wrap(w.clone(), font.clone(), Color32::WHITE)).collect();
        let gap = 5.0;
        let total: f32 = galleys.iter().map(|g| g.size().x).sum::<f32>() + gap * (galleys.len().saturating_sub(1)) as f32;
        let mut x = rect.center().x - total / 2.0;
        for ((_, on), g) in shown.iter().zip(galleys) {
            let r = egui::Rect::from_min_size(egui::pos2(x, y - g.size().y / 2.0), g.size());
            let color = match (self.style.look, on) {
                (Look::Caixa, true) => {
                    painter.rect_filled(r.expand2(vec2(4.0, 1.0)), 5, hl);
                    Color32::from_gray(16)
                }
                (_, true) => hl,
                _ => Color32::WHITE,
            };
            // contorno escuro, como no vídeo
            for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
                if !(self.style.look == Look::Caixa && *on) {
                    painter.galley_with_override_text_color(r.min + d, g.clone(), Color32::from_gray(16));
                }
            }
            painter.galley_with_override_text_color(r.min, g.clone(), color);
            x += r.width() + gap;
        }
        painter.text(rect.left_top() + vec2(8.0, 8.0), Align2::LEFT_TOP, crate::i18n::tr("prévia"), FontId::proportional(10.5), Color32::from_gray(170));
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &Durations) {
        let title = "Legendas animadas";
        widgets::header(ui, p, title, "Legendas palavra por palavra no estilo Reels e TikTok: a palavra falada fica destacada. Tudo no seu PC, sem internet.");
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 170.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width((ui.available_width() - 170.0).max(320.0));
                    row(ui, p, "Estilo:", |ui| {
                        segmented(ui, p, &mut self.style.look, &[(Look::Destaque, "Palavra colorida"), (Look::Caixa, "Caixa"), (Look::UmaPalavra, "Uma palavra por vez")]);
                    });
                    row(ui, p, "Cor do destaque:", |ui| {
                        segmented(ui, p, &mut self.style.tint, &[(Tint::Amarelo, "Amarelo"), (Tint::Verde, "Verde"), (Tint::Cinabrio, "Vermelho"), (Tint::Azul, "Azul")]);
                    });
                    row(ui, p, "Posição:", |ui| {
                        segmented(ui, p, &mut self.style.place, &[(Place::Baixo, "Embaixo"), (Place::Meio, "No meio"), (Place::Cima, "Em cima")]);
                    });
                    if self.style.look != Look::UmaPalavra {
                        row(ui, p, "Palavras juntas:", |ui| {
                            segmented(ui, p, &mut self.style.words_per_line, &[(2, "2"), (3, "3"), (4, "4"), (5, "5")]);
                        });
                    }
                    row(ui, p, "Texto:", |ui| {
                        ui.checkbox(&mut self.style.uppercase, crate::i18n::tr("Em maiúsculas"));
                        ui.add_space(8.0);
                        ui.checkbox(&mut self.keep_ass, crate::i18n::tr("Salvar também a legenda (.ass)"));
                    });
                    row(ui, p, "Idioma falado:", |ui| {
                        let name = transcribe::LANGS[self.lang].1;
                        egui::ComboBox::from_id_salt("caps_lang").selected_text(crate::i18n::tr(name)).width(240.0).show_ui(ui, |ui| {
                            for (i, (_, n)) in transcribe::LANGS.iter().enumerate() {
                                ui.selectable_value(&mut self.lang, i, crate::i18n::tr(*n));
                            }
                        });
                    });
                    row(ui, p, "Modelo:", |ui| {
                        let opts: Vec<(WModel, &str)> = WModel::ALL.iter().map(|m| (*m, m.label())).collect();
                        segmented(ui, p, &mut self.model, &opts);
                    });
                });
                ui.add_space(8.0);
                self.preview(ui, p);
            });
            hint(ui, p, "O Whisper descobre quando cada palavra é falada; depois a legenda é gravada dentro do vídeo (o original não muda).");
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        match widgets::action_bar(ui, p, &self.job, "Legendar", !self.queue.files.is_empty()) {
            Act::Start => {
                let (files, out, model, style, keep) = (self.queue.files.clone(), self.out.path.clone(), self.model, self.style, self.keep_ass);
                let lang = transcribe::LANGS[self.lang].0.to_string();
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| crate::captions::animated(src, out.as_deref(), model, &lang, style, keep, progress, &|| ctl.cancelled()));
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ apagar objeto

pub struct EraserPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    /// Arquivo aberto, a imagem original e como ela está agora (depois de cada "Apagar").
    src: Option<PathBuf>,
    original: Option<image::RgbImage>,
    img: Option<image::RgbImage>,
    tex: Option<egui::TextureHandle>,
    tex_orig: Option<egui::TextureHandle>,
    /// Traços pintados, em pixels da imagem: (x, y, raio).
    strokes: Vec<Vec<(f32, f32, f32)>>,
    brush: f32,
    result: Arc<Mutex<Option<Result<image::RgbImage, String>>>>,
    show_original: bool,
    edited: bool,
    msg: Option<(bool, String)>,
}

impl EraserPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(crate::imaging::IMAGE_EXTS, "Imagens"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            src: None,
            original: None,
            img: None,
            tex: None,
            tex_orig: None,
            strokes: Vec::new(),
            brush: 30.0,
            result: Arc::default(),
            show_original: false,
            edited: false,
            msg: None,
        }
    }

    fn texture(ctx: &egui::Context, name: &str, img: &image::RgbImage) -> egui::TextureHandle {
        let ci = egui::ColorImage::from_rgb([img.width() as usize, img.height() as usize], img.as_raw());
        ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
    }

    /// Máscara (branco = apagar) com os traços pintados.
    fn mask(&self) -> Option<image::GrayImage> {
        let img = self.img.as_ref()?;
        let mut m = image::GrayImage::new(img.width(), img.height());
        for &(cx, cy, r) in self.strokes.iter().flatten() {
            let (x0, x1) = ((cx - r).floor().max(0.0) as u32, ((cx + r).ceil().max(0.0) as u32).min(img.width()));
            let (y0, y1) = ((cy - r).floor().max(0.0) as u32, ((cy + r).ceil().max(0.0) as u32).min(img.height()));
            for y in y0..y1 {
                for x in x0..x1 {
                    let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    if dx * dx + dy * dy <= r * r {
                        m.put_pixel(x, y, image::Luma([255]));
                    }
                }
            }
        }
        Some(m)
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Apagar objeto";
        widgets::header(ui, p, title, "Pinte por cima de um objeto, pessoa, texto ou marca d'água e a IA preenche o fundo. No seu PC, sem internet.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 96.0);
        // abriu outra imagem
        let sel = self.queue.selected_path().cloned();
        if sel != self.src {
            self.src = sel.clone();
            self.strokes.clear();
            self.edited = false;
            self.msg = None;
            self.tex = None;
            self.tex_orig = None;
            self.img = None;
            self.original = None;
            if let Some(f) = &sel {
                match image::open(f) {
                    Ok(i) => {
                        let rgb = i.to_rgb8();
                        self.tex = Some(Self::texture(ui.ctx(), "eraser", &rgb));
                        self.tex_orig = self.tex.clone();
                        self.original = Some(rgb.clone());
                        self.img = Some(rgb);
                    }
                    Err(e) => self.msg = Some((false, format!("✖ não foi possível abrir a imagem: {e}"))),
                }
            }
        }
        // resultado da IA
        let done = self.result.lock().unwrap().take();
        if let Some(r) = done {
            match r {
                Ok(img) => {
                    self.tex = Some(Self::texture(ui.ctx(), "eraser", &img));
                    self.img = Some(img);
                    self.strokes.clear();
                    self.edited = true;
                    self.msg = Some((true, "✔ Pronto. Pinte mais para continuar apagando ou salve a imagem.".into()));
                }
                Err(e) => self.msg = Some((false, format!("✖ {e}"))),
            }
        }
        ui.add_space(4.0);
        let running = self.job.state().running;
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Pincel:")).color(p.text));
                ui.add(egui::Slider::new(&mut self.brush, 4.0..=120.0).show_value(false));
                if widgets::secondary(ui, p, "Desfazer", !self.strokes.is_empty() && !running) {
                    self.strokes.pop();
                }
                if widgets::secondary(ui, p, "Limpar", !self.strokes.is_empty() && !running) {
                    self.strokes.clear();
                }
                ui.add_space(8.0);
                ui.checkbox(&mut self.show_original, crate::i18n::tr("Ver o original"));
            });
            let avail = vec2(ui.available_width(), 440.0);
            let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 8, p.input);
            let tex = if self.show_original { self.tex_orig.clone() } else { self.tex.clone() };
            let Some(tex) = tex else {
                painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::tr("Adicione uma imagem para começar."), FontId::proportional(14.0), p.muted);
                return;
            };
            let (iw, ih) = self.img.as_ref().map(|i| (i.width() as f32, i.height() as f32)).unwrap_or((1.0, 1.0));
            let shown = widgets::fit((iw, ih), rect.size() - vec2(12.0, 12.0));
            let r = egui::Rect::from_center_size(rect.center(), shown);
            let scale = shown.x / iw;
            painter.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            // pintar: cada ponto do arrasto vira um círculo (em pixels da imagem)
            if !running && !self.show_original {
                if let Some(pos) = resp.interact_pointer_pos().filter(|q| r.contains(*q)) {
                    if resp.drag_started() || resp.clicked() || self.strokes.is_empty() {
                        self.strokes.push(Vec::new());
                    }
                    if resp.dragged() || resp.clicked() || resp.drag_started() {
                        let ip = ((pos.x - r.left()) / scale, (pos.y - r.top()) / scale, self.brush / 2.0 / scale);
                        let last = self.strokes.last_mut().unwrap();
                        // pontos intermediários, para o traço não ficar com buracos ao arrastar rápido
                        if let Some(&(lx, ly, _)) = last.last() {
                            let (dx, dy) = (ip.0 - lx, ip.1 - ly);
                            let steps = ((dx * dx + dy * dy).sqrt() / (ip.2 * 0.5).max(1.0)).ceil() as usize;
                            for k in 1..steps {
                                let t = k as f32 / steps as f32;
                                last.push((lx + dx * t, ly + dy * t, ip.2));
                            }
                        }
                        last.push(ip);
                    }
                }
            }
            if !self.show_original {
                let paint = Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 120);
                for &(x, y, rad) in self.strokes.iter().flatten() {
                    painter.circle_filled(egui::pos2(r.left() + x * scale, r.top() + y * scale), rad * scale, paint);
                }
            }
            if let Some(h) = resp.hover_pos().filter(|h| r.contains(*h)) {
                painter.circle_stroke(h, self.brush / 2.0, Stroke::new(1.5f32, p.accent));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            if running {
                painter.rect_filled(r, 0, Color32::from_black_alpha(90));
                painter.text(r.center(), Align2::CENTER_CENTER, crate::i18n::tr("A IA está preenchendo…"), FontId::new(16.0, bold()), Color32::WHITE);
            }
            painter.rect_stroke(r, 0, Stroke::new(1.0f32, p.border), StrokeKind::Outside);
        });
        crate::ai::model_hint(ui, p, &crate::inpaint::MODEL);
        if let Some((good, text)) = &self.msg {
            ui.label(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent }));
        }
        ui.add_space(4.0);
        self.out.ui(ui, p);
        ui.horizontal(|ui| {
            if widgets::secondary(ui, p, "Salvar imagem", self.edited && !running) {
                if let (Some(src), Some(img)) = (&self.src, &self.img) {
                    let ext = match crate::tools::ext_lower(src).as_str() {
                        "jpg" | "jpeg" => "jpg",
                        "webp" => "webp",
                        _ => "png",
                    };
                    let dst = crate::tools::out_path(src, self.out.path.as_deref(), "_apagado", ext);
                    self.msg = Some(match img.save(&dst) {
                        Ok(()) => {
                            crate::history::record(title, &crate::tools::file_name(src), &dst, &format!("{}×{}", img.width(), img.height()));
                            (true, format!("✔ Salvo: {}", crate::tools::file_name(&dst)))
                        }
                        Err(e) => (false, format!("✖ não foi possível salvar: {e}")),
                    });
                }
            }
            if widgets::secondary(ui, p, "Voltar ao original", self.edited && !running) {
                if let Some(o) = self.original.clone() {
                    self.tex = self.tex_orig.clone();
                    self.img = Some(o);
                    self.strokes.clear();
                    self.edited = false;
                    self.msg = None;
                }
            }
        });
        match widgets::action_bar(ui, p, &self.job, "Apagar", self.img.is_some() && !self.strokes.is_empty()) {
            Act::Start => {
                if let (Some(img), Some(mask)) = (self.img.clone(), self.mask()) {
                    let slot = self.result.clone();
                    self.job.start(ui.ctx(), move |ctl| {
                        ctl.status("Apagando…");
                        let r = crate::inpaint::erase(&img, &mask, &|x| ctl.progress(x), &|| ctl.cancelled());
                        match &r {
                            Ok(_) => ctl.status("Pronto."),
                            Err(e) => {
                                ctl.log(crate::job::LogKind::Err, format!("✖ {e}"));
                                ctl.status("Erro.");
                            }
                        }
                        *slot.lock().unwrap() = Some(r);
                    });
                }
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ texto para voz

pub struct TtsPage {
    text: String,
    voice: usize,
    speed: f32,
    mp3: bool,
    out: OutDir,
    job: Job,
    last: Arc<Mutex<Option<PathBuf>>>,
}

impl TtsPage {
    pub fn new() -> Self {
        let lang = crate::i18n::current().code;
        let voice = crate::tts::VOICES.iter().position(|v| v.lang == lang).unwrap_or(0);
        let music = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Music")).filter(|p| p.is_dir());
        Self {
            text: String::new(),
            voice,
            speed: 1.0,
            mp3: true,
            out: OutDir::fixed(music.unwrap_or_else(crate::tools::exe_dir)),
            job: Job::default(),
            last: Arc::default(),
        }
    }

    /// Texto vindo de um arquivo arrastado (.txt, .md, .srt, .vtt).
    pub fn load_file(&mut self, p: &std::path::Path) {
        if let Ok(t) = crate::tts::text_from_file(p) {
            self.text = t;
        }
    }

    pub fn accepts(p: &std::path::Path) -> bool {
        ["txt", "md", "srt", "vtt"].contains(&crate::tools::ext_lower(p).as_str())
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Texto para voz";
        widgets::header(ui, p, title, "Transforme textos e legendas em narração com uma voz natural, no seu PC e sem internet.");
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Texto")).family(bold()).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::secondary(ui, p, "Limpar", !self.text.is_empty()) {
                        self.text.clear();
                    }
                    if widgets::secondary(ui, p, "Abrir arquivo…", true) {
                        if let Some(f) = rfd::FileDialog::new().add_filter(crate::i18n::tr("Texto e legendas"), &["txt", "md", "srt", "vtt"]).pick_file() {
                            self.load_file(&f);
                        }
                    }
                    let n = self.text.chars().count();
                    ui.label(RichText::new(crate::i18n::tr(format!("{n} caracteres  •  ~{} de fala", crate::video::fmt_time(n as f64 / 14.0)))).size(12.0).color(p.muted));
                });
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.text)
                    .hint_text(crate::i18n::tr("Escreva ou cole o texto aqui (ou arraste um .txt ou uma legenda .srt para a janela)."))
                    .desired_rows(9)
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Voz:", |ui| {
                let voices = crate::tts::VOICES;
                self.voice = self.voice.min(voices.len() - 1);
                egui::ComboBox::from_id_salt("tts_voice").selected_text(crate::i18n::tr(voices[self.voice].label)).width(340.0).show_ui(ui, |ui| {
                    for (i, v) in voices.iter().enumerate() {
                        let mark = if crate::tts::installed(v) { "✔ " } else { "" };
                        ui.selectable_value(&mut self.voice, i, format!("{mark}{}", crate::i18n::tr(v.label)));
                    }
                });
            });
            row(ui, p, "Velocidade:", |ui| {
                segmented(ui, p, &mut self.speed, &[(0.8, "Devagar"), (1.0, "Normal"), (1.2, "Rápida"), (1.4, "Bem rápida")]);
            });
            row(ui, p, "Formato:", |ui| {
                segmented(ui, p, &mut self.mp3, &[(true, "MP3"), (false, "WAV (sem perda)")]);
            });
            let v = &crate::tts::VOICES[self.voice];
            if crate::tts::installed(v) {
                hint(ui, p, "✔ Voz pronta neste computador.");
            } else {
                hint(ui, p, "A voz (cerca de 63 MB) é baixada uma vez, na primeira vez que você usar (a assinatura é conferida).");
            }
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let last = self.last.lock().unwrap().clone();
        if let (false, Some(f)) = (self.job.state().running, &last) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr(format!("✔ {}", crate::tools::file_name(f)))).color(p.ok));
                if widgets::secondary(ui, p, "Ouvir", true) {
                    crate::tools::open_file(f);
                }
                if widgets::secondary(ui, p, "Mostrar na pasta", true) {
                    crate::tools::show_in_folder(f);
                }
            });
        }
        match widgets::action_bar(ui, p, &self.job, "Gerar narração", !self.text.trim().is_empty()) {
            Act::Start => {
                let (text, voice, speed) = (self.text.clone(), &crate::tts::VOICES[self.voice], self.speed);
                let fmt = if self.mp3 { crate::tts::OutFmt::Mp3 } else { crate::tts::OutFmt::Wav };
                let dir = self.out.path.clone().unwrap_or_else(std::env::temp_dir);
                let slot = self.last.clone();
                *slot.lock().unwrap() = None;
                self.job.start(ui.ctx(), move |ctl| {
                    let ext = if fmt == crate::tts::OutFmt::Mp3 { "mp3" } else { "wav" };
                    let dst = crate::tools::unique_path(dir.join(format!("narracao_{}.{ext}", crate::tools::now_string().replace([':', ' ', '/'], "-"))));
                    crate::pages::run_single(ctl, title, "texto", || {
                        let r = crate::tts::speak(&text, voice, speed, fmt, &dst, &|x| ctl.progress(x), &|| ctl.cancelled());
                        if let Ok((_, f)) = &r {
                            *slot.lock().unwrap() = Some(f.clone());
                        }
                        r
                    });
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ gerar imagem

pub struct ImageGenPage {
    prompt: String,
    negative: String,
    shape: crate::imagegen::Shape,
    quality: crate::imagegen::Quality,
    count: u32,
    translate: bool,
    out: OutDir,
    job: Job,
    results: Arc<Mutex<Vec<PathBuf>>>,
    shown: Vec<(PathBuf, egui::TextureHandle)>,
}

impl ImageGenPage {
    pub fn new() -> Self {
        Self {
            prompt: String::new(),
            negative: String::new(),
            shape: crate::imagegen::Shape::Quadrada,
            quality: crate::imagegen::Quality::Rapida,
            count: 2,
            translate: true,
            out: OutDir::fixed(crate::imagegen::default_dir()),
            job: Job::default(),
            results: Arc::default(),
            shown: Vec::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        use crate::imagegen::{Quality, Shape};
        let title = "Gerar imagem";
        widgets::header(ui, p, title, "Descreva uma imagem e a IA cria no seu PC, usando a placa de vídeo. Sem internet e sem limite.");
        let ai_ok = crate::ai_api::Settings::load().has_key() || !crate::ai_api::Settings::load().base_url.is_empty();
        card(ui, p, |ui| {
            ui.label(RichText::new(crate::i18n::tr("O que a imagem mostra")).family(bold()).color(p.text));
            ui.add(
                egui::TextEdit::multiline(&mut self.prompt)
                    .hint_text(crate::i18n::tr("ex.: uma cabana de madeira numa floresta com neve ao pôr do sol, luz quente nas janelas"))
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            row(ui, p, "Evitar:", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.negative).margin(widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("opcional: o que não deve aparecer")).desired_width(ui.available_width()));
            });
            row(ui, p, "Formato:", |ui| {
                segmented(ui, p, &mut self.shape, &[(Shape::Quadrada, "Quadrada"), (Shape::Retrato, "Retrato"), (Shape::Paisagem, "Paisagem")]);
            });
            row(ui, p, "Qualidade:", |ui| {
                segmented(ui, p, &mut self.quality, &[(Quality::Rapida, "Rápida"), (Quality::Melhor, "Melhor (mais lenta)")]);
            });
            row(ui, p, "Quantas:", |ui| {
                segmented(ui, p, &mut self.count, &[(1, "1"), (2, "2"), (4, "4")]);
            });
            row(ui, p, "Idioma:", |ui| {
                // sem IA configurada, a opção aparece desmarcada (e desativada)
                let mut off = false;
                let value = if ai_ok { &mut self.translate } else { &mut off };
                ui.add_enabled(ai_ok, egui::Checkbox::new(value, crate::i18n::tr("Traduzir o pedido para o inglês com a IA configurada")));
            });
            if !ai_ok {
                hint(ui, p, "O modelo entende melhor pedidos em inglês. Com uma IA configurada em Configurações, o Conver traduz sozinho.");
            }
            crate::ai::model_hint(ui, p, &crate::imagegen::MODEL);
            hint(ui, p, "Usa a placa de vídeo (Vulkan). Numa placa comum, cada imagem leva uns 10 a 20 segundos, mais uns segundos para carregar o modelo.");
        });
        // imagens prontas
        let new: Vec<PathBuf> = std::mem::take(&mut *self.results.lock().unwrap());
        if !new.is_empty() {
            self.shown.clear();
            for f in new {
                if let Ok(img) = image::open(&f) {
                    let rgba = img.thumbnail(360, 360).to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    let tex = ui.ctx().load_texture(format!("gen_{}", f.display()), ci, egui::TextureOptions::LINEAR);
                    self.shown.push((f, tex));
                }
            }
        }
        if !self.shown.is_empty() {
            ui.add_space(4.0);
            card(ui, p, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (f, tex) in &self.shown {
                        let s = tex.size_vec2();
                        let size = widgets::fit((s.x, s.y), vec2(220.0, 220.0));
                        let r = ui.add(egui::Image::new(tex).fit_to_exact_size(size).corner_radius(6).sense(Sense::click()));
                        if r.on_hover_text(crate::i18n::tr("Clique para abrir")).clicked() {
                            crate::tools::open_file(f);
                        }
                    }
                });
            });
        }
        ui.add_space(4.0);
        self.out.ui(ui, p);
        match widgets::action_bar(ui, p, &self.job, "Gerar", !self.prompt.trim().is_empty()) {
            Act::Start => {
                let mut req = crate::imagegen::Request { prompt: self.prompt.clone(), negative: self.negative.clone(), shape: self.shape, quality: self.quality, count: self.count, seed: None };
                let translate = self.translate && ai_ok;
                let dir = self.out.path.clone().unwrap_or_else(crate::imagegen::default_dir);
                let slot = self.results.clone();
                self.job.start(ui.ctx(), move |ctl| {
                    if translate {
                        ctl.status("Traduzindo o pedido…");
                        match crate::ai_api::Settings::load().config().and_then(|c| {
                            crate::ai_api::ask(&c, "Translate the user's image description to English for an image generator. Reply with only the translation.", &[crate::ai_api::Part::Text(req.prompt.clone())], 300)
                        }) {
                            Ok(t) => {
                                ctl.log(crate::job::LogKind::Info, format!("Pedido em inglês: {t}"));
                                req.prompt = t;
                            }
                            Err(e) => ctl.log(crate::job::LogKind::Info, format!("Sem tradução ({e}); usando o texto como está.")),
                        }
                    }
                    ctl.status("Gerando…");
                    match crate::imagegen::generate(&req, &dir, &|x| ctl.progress(x), &|| ctl.cancelled()) {
                        Ok(files) => {
                            for f in &files {
                                crate::history::record(title, &req.prompt.chars().take(60).collect::<String>(), f, "");
                            }
                            ctl.log(crate::job::LogKind::Ok, format!("✔ {} imagem(ns) em {}", files.len(), dir.display()));
                            ctl.status("Pronto.");
                            ctl.done_folder(dir.clone());
                            *slot.lock().unwrap() = files;
                        }
                        Err(e) => {
                            ctl.log(crate::job::LogKind::Err, format!("✖ {e}"));
                            ctl.status(if e == "Cancelado." { "Cancelado." } else { "Erro." });
                        }
                    }
                    ctl.progress(1.0);
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ ditado ao vivo

pub struct DictationPage {
    devices: Option<Vec<String>>,
    device: usize,
    lang: usize,
    model: WModel,
    run: Option<crate::dictation::Dictation>,
    text: String,
    msg: Option<(bool, String)>,
}

impl DictationPage {
    pub fn new() -> Self {
        Self { devices: None, device: 0, lang: 1, model: WModel::Rapido, run: None, text: String::new(), msg: None }
    }

    pub fn listening(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.running())
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(ui, p, "Ditado ao vivo", "Fale e o texto aparece na tela enquanto você fala, com o Whisper no seu PC. Para ditar, legendar ao vivo ou fazer anotações.");
        if self.devices.is_none() {
            self.devices = crate::recorder::cached_devices(ui.ctx());
        }
        let devices = self.devices.clone().unwrap_or_default();
        let listening = self.listening();
        // o texto confirmado vai para a caixa editável; o provisório aparece embaixo
        let (partial, status, ready) = match &self.run {
            Some(r) => {
                let mut s = r.state.lock().unwrap();
                if !s.text.is_empty() {
                    if !self.text.is_empty() && !self.text.ends_with([' ', '\n']) {
                        self.text.push(' ');
                    }
                    self.text.push_str(&std::mem::take(&mut s.text));
                }
                if let Some(e) = s.error.take() {
                    self.msg = Some((false, format!("✖ {e}")));
                }
                (s.partial.clone(), s.status.clone(), s.ready)
            }
            None => (String::new(), String::new(), false),
        };
        card(ui, p, |ui| {
            ui.add_enabled_ui(!listening, |ui| {
                row(ui, p, "Microfone:", |ui| {
                    if self.devices.is_none() {
                        ui.label(RichText::new(crate::i18n::tr("procurando microfones…")).color(p.muted));
                    } else if devices.is_empty() {
                        ui.label(RichText::new(crate::i18n::tr("nenhuma entrada de áudio encontrada")).color(p.accent));
                    } else {
                        self.device = self.device.min(devices.len() - 1);
                        egui::ComboBox::from_id_salt("dict_mic").selected_text(crate::i18n::tr(&devices[self.device])).width(320.0).show_ui(ui, |ui| {
                            for (i, d) in devices.iter().enumerate() {
                                ui.selectable_value(&mut self.device, i, crate::i18n::tr(d));
                            }
                        });
                    }
                    if widgets::secondary(ui, p, "Atualizar lista", self.devices.is_some()) {
                        crate::recorder::refresh_devices(ui.ctx());
                        self.devices = None;
                    }
                });
                row(ui, p, "Idioma falado:", |ui| {
                    let name = transcribe::LANGS[self.lang].1;
                    egui::ComboBox::from_id_salt("dict_lang").selected_text(crate::i18n::tr(name)).width(240.0).show_ui(ui, |ui| {
                        for (i, (_, n)) in transcribe::LANGS.iter().enumerate() {
                            ui.selectable_value(&mut self.lang, i, crate::i18n::tr(*n));
                        }
                    });
                });
                row(ui, p, "Modelo:", |ui| {
                    segmented(ui, p, &mut self.model, &[(WModel::Rapido, "Rápido (82 MB)"), (WModel::Equilibrado, "Equilibrado (264 MB, mais lento)")]);
                });
            });
            hint(ui, p, "O texto aparece com 1 a 2 segundos de atraso; numa pausa, a frase é confirmada. Nada sai do seu computador.");
        });
        ui.add_space(4.0);
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                let (dot, color) = if ready { ("●  Ouvindo", p.accent) } else if listening { ("Preparando…", p.muted) } else { ("Parado", p.muted) };
                ui.label(RichText::new(crate::i18n::tr(dot)).family(bold()).color(color));
                if !status.is_empty() && listening {
                    ui.label(RichText::new(crate::i18n::tr(&status)).size(12.5).color(p.muted));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if listening {
                        if widgets::primary(ui, p, "■  Parar", true) {
                            if let Some(r) = self.run.as_mut() {
                                r.stop();
                            }
                        }
                    } else if widgets::primary(ui, p, "●  Começar a ditar", !devices.is_empty()) {
                        self.msg = None;
                        let lang = transcribe::LANGS[self.lang].0;
                        self.run = Some(crate::dictation::Dictation::start(&devices[self.device], self.model, lang, ui.ctx()));
                    }
                });
            });
            if let Some(m) = self.run.as_mut().and_then(|r| r.meter()) {
                if listening {
                    let lv = m.lock().unwrap();
                    widgets::level_meter(ui, p, &lv);
                }
            }
            ui.add(egui::TextEdit::multiline(&mut self.text).hint_text(crate::i18n::tr("O texto ditado aparece aqui. Você pode editar à vontade.")).desired_rows(10).desired_width(f32::INFINITY));
            if !partial.is_empty() {
                ui.label(RichText::new(crate::i18n::tr(format!("… {partial}"))).italics().color(p.muted));
            }
            ui.horizontal(|ui| {
                if widgets::secondary(ui, p, "Copiar", !self.text.is_empty()) {
                    let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(self.text.clone()));
                    self.msg = Some((true, "✔ Copiado.".into()));
                }
                if widgets::secondary(ui, p, "Salvar .txt", !self.text.is_empty()) {
                    if let Some(f) = rfd::FileDialog::new().set_file_name("ditado.txt").add_filter("TXT", &["txt"]).save_file() {
                        self.msg = Some(match std::fs::write(&f, &self.text) {
                            Ok(()) => (true, format!("✔ Salvo: {}", crate::tools::file_name(&f))),
                            Err(e) => (false, format!("✖ {e}")),
                        });
                    }
                }
                if widgets::secondary(ui, p, "Limpar", !self.text.is_empty()) {
                    self.text.clear();
                }
            });
        });
        if let Some((good, text)) = &self.msg {
            ui.label(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent }));
        }
        if listening {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
        }
    }
}
