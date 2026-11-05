//! Tela "Legendas": automático (transcreve e grava de uma vez) ou manual (transcreve, você corrige as frases e
//! depois grava), com estilos prontos, estilos salvos e ajuste fino de fonte, cores, contorno, faixa e posição.
//! A prévia é um quadro do próprio vídeo com a legenda desenhada pelo mesmo ffmpeg que grava.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde::{Deserialize, Serialize};

use crate::captions::{self, Cue, Effect, Source, Style};
use crate::i18n::tr;
use crate::job::{Job, LogKind};
use crate::pages::run_batch;
use crate::theme::Palette;
use crate::transcribe::{self, WModel, Word};
use crate::widgets::{self, Act, Durations, FileQueue, OutDir, card, hint, row, segmented};

/// O que fica salvo entre uma abertura e outra (legendas.json).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    style: Style,
    /// Estilos salvos pelo usuário: (nome, estilo).
    mine: Vec<(String, Style)>,
    manual: bool,
    model: usize,
    lang: String,
    keep_ass: bool,
}

impl Default for Saved {
    fn default() -> Self {
        Saved { style: Style::default(), mine: Vec::new(), manual: false, model: 0, lang: "pt".into(), keep_ass: false }
    }
}

fn saved_path() -> PathBuf {
    crate::tools::app_dir().join("legendas.json")
}

/// Prévia renderizada em segundo plano, refeita (com uma pequena espera) quando algo muda.
#[derive(Default)]
struct Preview {
    tex: Option<egui::TextureHandle>,
    shown: String,
    want: String,
    since: Option<Instant>,
    busy: Arc<AtomicBool>,
    slot: Arc<Mutex<Option<(String, Result<egui::ColorImage, String>)>>>,
    /// Tamanho do vídeo de cada arquivo (para a legenda ficar na mesma escala que no vídeo final).
    sizes: Arc<Mutex<HashMap<PathBuf, ((u32, u32), f64)>>>,
    err: Option<String>,
}

pub struct CaptionsPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    saved: Saved,
    saved_json: String,
    fonts: Vec<&'static str>,
    /// Frases de cada vídeo (modo manual).
    cues: HashMap<PathBuf, Vec<Cue>>,
    sel: Option<usize>,
    words: Arc<Mutex<Option<(PathBuf, Result<Vec<Word>, String>)>>>,
    msg: Option<(bool, String)>,
    prev: Preview,
    new_name: String,
}

impl CaptionsPage {
    pub fn new() -> Self {
        let mut saved: Saved = std::fs::read_to_string(saved_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        // modo de captura (compilação de depuração): CONVER_DEMO_LEGENDAS=1 abre no modo manual com frases de exemplo
        if demo() {
            saved.manual = true;
        }
        let saved_json = serde_json::to_string(&saved).unwrap_or_default();
        Self {
            queue: FileQueue::new(crate::video::VIDEO_EXTS, "Vídeos"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            saved,
            saved_json,
            fonts: captions::installed_fonts(),
            cues: HashMap::new(),
            sel: None,
            words: Arc::default(),
            msg: None,
            prev: Preview::default(),
            new_name: String::new(),
        }
    }

    fn model(&self) -> WModel {
        WModel::ALL[self.saved.model.min(WModel::ALL.len() - 1)]
    }

    fn lang_index(&self) -> usize {
        transcribe::LANGS.iter().position(|(c, _)| *c == self.saved.lang).unwrap_or(0)
    }

    // ------------------------------------------------------------ prévia

    fn preview(&mut self, ui: &mut Ui, p: &Palette, file: Option<&PathBuf>, durs: &Durations) {
        // o que mostrar: a frase escolhida (manual) ou uma frase de exemplo
        let cue = file.and_then(|f| self.cues.get(f)).and_then(|c| self.sel.and_then(|i| c.get(i)));
        let (text, at) = match cue {
            Some(c) => (c.text.clone(), c.start + 0.05),
            None => (tr("Assim fica a legenda no seu vídeo").to_string(), file.and_then(|f| durs.get(f)).map_or(1.0, |d| d * 0.35)),
        };
        let st = self.saved.style.clone();
        let key = format!("{file:?}|{at:.2}|{text}|{}", serde_json::to_string(&st).unwrap_or_default());
        if key != self.prev.want {
            self.prev.want = key.clone();
            self.prev.since = Some(Instant::now());
        }
        // resultado pronto
        if let Some((k, r)) = self.prev.slot.lock().unwrap().take() {
            match r {
                Ok(img) => {
                    self.prev.tex = Some(ui.ctx().load_texture("legenda_previa", img, egui::TextureOptions::LINEAR));
                    self.prev.err = None;
                }
                Err(e) => self.prev.err = Some(e),
            }
            self.prev.shown = k;
        }
        let waited = self.prev.since.is_some_and(|t| t.elapsed() > Duration::from_millis(220));
        if self.prev.want != self.prev.shown && !self.prev.busy.load(Ordering::Relaxed) {
            if waited {
                self.prev.busy.store(true, Ordering::Relaxed);
                let (busy, slot, sizes, ctx) = (self.prev.busy.clone(), self.prev.slot.clone(), self.prev.sizes.clone(), ui.ctx().clone());
                let (file, st) = (file.cloned(), st.clone());
                std::thread::spawn(move || {
                    let size = match &file {
                        Some(f) => {
                            let known = sizes.lock().unwrap().get(f).copied();
                            known.or_else(|| {
                                let pr = crate::video::probe(f);
                                let v = (pr.size?, pr.duration.unwrap_or(0.0));
                                sizes.lock().unwrap().insert(f.clone(), v);
                                Some(v)
                            })
                        }
                        None => Some(((1080, 1920), 0.0)),
                    };
                    let r = match size {
                        Some((wh, dur)) => {
                            let at = if dur > 0.0 { at.min((dur - 0.2).max(0.0)) } else { at };
                            captions::render_preview(file.as_deref(), at, wh, &text, Some(1), &st, 640).and_then(|png| {
                                let img = image::load_from_memory(&png).map_err(|e| e.to_string())?.to_rgba8();
                                Ok(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
                            })
                        }
                        None => Err("o arquivo não tem vídeo.".into()),
                    };
                    *slot.lock().unwrap() = Some((key, r));
                    busy.store(false, Ordering::Relaxed);
                    ctx.request_repaint();
                });
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(240));
            }
        }

        // quadro
        let (rect, resp) = ui.allocate_exact_size(vec2(220.0, 392.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, p.input);
        let Some(tex) = &self.prev.tex else {
            painter.text(rect.center(), Align2::CENTER_CENTER, tr("Preparando…"), FontId::proportional(12.0), p.muted);
            return;
        };
        let [tw, th] = tex.size();
        let shown = widgets::fit((tw as f32, th as f32), rect.size() - vec2(8.0, 8.0));
        let r = egui::Rect::from_center_size(rect.center(), shown);
        painter.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        painter.rect_stroke(r, 0, Stroke::new(1.0f32, p.border), StrokeKind::Outside);
        // arrastar na prévia muda a posição
        let target = egui::pos2(r.left() + r.width() * st.x / 100.0, r.top() + r.height() * st.y / 100.0);
        if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
            let s = &mut self.saved.style;
            s.x = (((pos.x - r.left()) / r.width()) * 100.0).clamp(0.0, 100.0).round();
            s.y = (((pos.y - r.top()) / r.height()) * 100.0).clamp(0.0, 100.0).round();
        }
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            let c = Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 200);
            painter.line_segment([egui::pos2(r.left(), target.y), egui::pos2(r.right(), target.y)], Stroke::new(1.0f32, c));
            painter.line_segment([egui::pos2(target.x, r.top()), egui::pos2(target.x, r.bottom())], Stroke::new(1.0f32, c));
        }
        if self.prev.want != self.prev.shown {
            painter.circle_filled(r.right_top() + vec2(-9.0, 9.0), 3.5, p.accent);
        }
        if let Some(e) = &self.prev.err {
            painter.text(r.center_bottom() - vec2(0.0, 8.0), Align2::CENTER_BOTTOM, tr(e), FontId::proportional(11.0), p.accent);
        }
    }

    // ------------------------------------------------------------ estilo

    fn style_controls(&mut self, ui: &mut Ui, p: &Palette) {
        // estilos prontos e salvos
        let presets = captions::presets();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for (name, st) in &presets {
                if widgets::secondary(ui, p, name, self.saved.style != *st) {
                    let (x, y) = (self.saved.style.x, self.saved.style.y);
                    self.saved.style = st.clone();
                    // quem já escolheu a posição na mão não perde ao trocar de estilo, exceto "uma palavra"
                    if *name != "Uma palavra por vez" {
                        (self.saved.style.x, self.saved.style.y) = (x, y);
                    }
                }
            }
            let mut remove = None;
            for (i, (name, st)) in self.saved.mine.iter().enumerate() {
                let r = ui.add_enabled(self.saved.style != *st, egui::Button::new(RichText::new(name).color(p.text)).corner_radius(6));
                if r.clicked() {
                    self.saved.style = st.clone();
                }
                if r.secondary_clicked() {
                    remove = Some(i);
                }
                r.on_hover_text(tr("Clique com o botão direito para apagar este estilo."));
            }
            if let Some(i) = remove {
                self.saved.mine.remove(i);
            }
        });
        ui.add_space(6.0);
        let st = &mut self.saved.style;
        row(ui, p, "Animação:", |ui| {
            crate::widgets::check(ui, &mut st.animated, tr("Destacar palavra por palavra"));
        });
        if st.animated {
            row(ui, p, "Efeito:", |ui| {
                segmented(ui, p, &mut st.effect, &[(Effect::Cor, "Palavra colorida"), (Effect::Caixa, "Caixa"), (Effect::Revelar, "Aparecendo")]);
                ui.add_space(8.0);
                crate::widgets::check(ui, &mut st.pop, tr("Pulinho"));
            });
        }
        row(ui, p, "Cores:", |ui| {
            color(ui, p, "Texto", &mut st.text);
            if st.animated {
                color(ui, p, "Destaque", &mut st.highlight);
            }
            if !st.background {
                color(ui, p, "Contorno", &mut st.outline_color);
            }
        });
        row(ui, p, "Posição:", |ui| {
            for (label, y) in [("Em cima", 14.0), ("No meio", 50.0), ("Embaixo", 80.0)] {
                if widgets::secondary(ui, p, label, (st.y - y).abs() > 0.5 || (st.x - 50.0).abs() > 0.5) {
                    st.x = 50.0;
                    st.y = y;
                }
            }
            ui.label(RichText::new(format!("{:.0}% × {:.0}%", st.x, st.y)).color(p.muted).small());
        });
        row(ui, p, "Palavras juntas:", |ui| {
            ui.add(egui::Slider::new(&mut st.words_per_line, 1..=10));
            ui.add_space(8.0);
            crate::widgets::check(ui, &mut st.uppercase, tr("Em maiúsculas"));
        });
        egui::CollapsingHeader::new(RichText::new(tr("Mais ajustes")).color(p.text)).id_salt("caps_more").default_open(demo()).show(ui, |ui| {
            row(ui, p, "Fonte:", |ui| {
                egui::ComboBox::from_id_salt("caps_font").selected_text(st.font.clone()).width(200.0).show_ui(ui, |ui| {
                    for f in &self.fonts {
                        ui.selectable_value(&mut st.font, f.to_string(), *f);
                    }
                });
                crate::widgets::check(ui, &mut st.bold, tr("Negrito"));
                crate::widgets::check(ui, &mut st.italic, tr("Itálico"));
            });
            row(ui, p, "Tamanho:", |ui| {
                ui.add(egui::Slider::new(&mut st.size, 2.0..=14.0).step_by(0.1).suffix(" %"));
            });
            row(ui, p, "Contorno:", |ui| {
                ui.add_enabled(!st.background, egui::Slider::new(&mut st.outline, 0.0..=30.0).step_by(1.0).suffix(" %"));
            });
            row(ui, p, "Sombra:", |ui| {
                ui.add_enabled(!st.background, egui::Slider::new(&mut st.shadow, 0.0..=30.0).step_by(1.0).suffix(" %"));
            });
            row(ui, p, "Faixa atrás:", |ui| {
                crate::widgets::check(ui, &mut st.background, "");
                if st.background {
                    color(ui, p, "Cor", &mut st.back_color);
                    ui.add(egui::Slider::new(&mut st.back_opacity, 0..=100).suffix(" %"));
                }
            });
            row(ui, p, "Horizontal:", |ui| {
                ui.add(egui::Slider::new(&mut st.x, 0.0..=100.0).step_by(1.0).suffix(" %"));
            });
            row(ui, p, "Vertical:", |ui| {
                ui.add(egui::Slider::new(&mut st.y, 0.0..=100.0).step_by(1.0).suffix(" %"));
            });
            row(ui, p, "Salvar estilo:", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_name).hint_text(tr("nome do estilo")).desired_width(170.0));
                let name = self.new_name.trim().to_string();
                if widgets::secondary(ui, p, "Salvar", !name.is_empty()) {
                    let style = st.clone();
                    match self.saved.mine.iter_mut().find(|(n, _)| *n == name) {
                        Some(slot) => slot.1 = style,
                        None => self.saved.mine.push((name, style)),
                    }
                    self.new_name.clear();
                }
            });
        });
    }

    // ------------------------------------------------------------ frases (manual)

    fn editor(&mut self, ui: &mut Ui, p: &Palette, file: Option<&PathBuf>) {
        let running = self.job.state().running;
        let model = self.model();
        ui.horizontal(|ui| {
            if widgets::primary(ui, p, "Transcrever o vídeo", file.is_some() && !running) {
                if let Some(f) = file.cloned() {
                    let (slot, lang) = (self.words.clone(), self.saved.lang.clone());
                    self.msg = None;
                    self.job.start(ui.ctx(), move |ctl| {
                        ctl.status("Transcrevendo…");
                        let r = transcribe::word_timestamps(&f, model, &lang, &|x| ctl.progress(x), &|| ctl.cancelled());
                        match &r {
                            Ok(w) => ctl.status(format!("{} palavras", w.len())),
                            Err(e) => {
                                ctl.log(LogKind::Err, format!("✖ {e}"));
                                ctl.status("Erro.");
                            }
                        }
                        *slot.lock().unwrap() = Some((f, r));
                    });
                }
            }
            if widgets::secondary(ui, p, "Abrir legenda…", file.is_some() && !running) {
                if let (Some(f), Some(s)) = (file, rfd::FileDialog::new().add_filter("SRT / VTT", &["srt", "vtt"]).pick_file()) {
                    match std::fs::read_to_string(&s) {
                        Ok(t) => {
                            let c = captions::parse_subtitle(&t);
                            self.msg = Some((!c.is_empty(), if c.is_empty() { "✖ nenhuma frase encontrada no arquivo.".into() } else { format!("✔ {} frases", c.len()) }));
                            if !c.is_empty() {
                                self.cues.insert(f.clone(), c);
                                self.sel = Some(0);
                            }
                        }
                        Err(e) => self.msg = Some((false, format!("✖ não foi possível ler a legenda: {e}"))),
                    }
                }
            }
            let cues = file.and_then(|f| self.cues.get(f));
            if widgets::secondary(ui, p, "Salvar .srt…", cues.is_some_and(|c| !c.is_empty())) {
                if let (Some(f), Some(c)) = (file, cues) {
                    let name = format!("{}.srt", f.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default());
                    let mut dlg = rfd::FileDialog::new().add_filter("SRT", &["srt"]).set_file_name(name);
                    if let Some(dir) = f.parent() {
                        dlg = dlg.set_directory(dir);
                    }
                    if let Some(dst) = dlg.save_file() {
                        self.msg = Some(match std::fs::write(&dst, captions::to_srt(c)) {
                            Ok(()) => (true, format!("✔ Salvo: {}", crate::tools::file_name(&dst))),
                            Err(e) => (false, format!("✖ não foi possível salvar: {e}")),
                        });
                    }
                }
            }
        });
        let Some(f) = file else {
            hint(ui, p, "Adicione um vídeo para começar.");
            return;
        };
        let Some(cues) = self.cues.get_mut(f) else {
            hint(ui, p, "Transcreva o vídeo (ou abra uma legenda .srt) para ver e corrigir as frases aqui.");
            return;
        };
        hint(ui, p, "Corrija o texto e os tempos de cada frase. Clique no número para ver a frase na prévia.");
        let mut action: Option<(usize, bool)> = None; // (frase, true = inserir depois / false = apagar)
        let n = cues.len();
        egui::ScrollArea::vertical().id_salt("caps_cues").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, c) in cues.iter_mut().enumerate() {
                let selected = self.sel == Some(i);
                ui.horizontal(|ui| {
                    let label = RichText::new(format!("{:>3}", i + 1)).monospace().color(if selected { p.accent } else { p.muted });
                    if ui.add(egui::Label::new(label).sense(Sense::click())).clicked() {
                        self.sel = Some(i);
                    }
                    time_edit(ui, &mut c.start);
                    ui.label(RichText::new("→").color(p.muted));
                    time_edit(ui, &mut c.end);
                    if c.end < c.start + 0.1 {
                        c.end = c.start + 0.1;
                    }
                    // botões à direita; o texto ocupa o que sobra
                    ui.with_layout(crate::widgets::back(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text(tr("Apagar a frase")).clicked() {
                            action = Some((i, false));
                        }
                        if ui.small_button("＋").on_hover_text(tr("Nova frase depois desta")).clicked() {
                            action = Some((i, true));
                        }
                        let r = ui.add(egui::TextEdit::singleline(&mut c.text).desired_width(ui.available_width()));
                        if r.gained_focus() || r.changed() {
                            self.sel = Some(i);
                        }
                    });
                });
            }
        });
        match action {
            Some((i, true)) => {
                let start = cues[i].end;
                let end = cues.get(i + 1).map_or(start + 2.0, |nx| nx.start.max(start + 0.5));
                cues.insert(i + 1, Cue { start, end, text: String::new(), words: Vec::new() });
                self.sel = Some(i + 1);
            }
            Some((i, false)) => {
                cues.remove(i);
                self.sel = if cues.is_empty() { None } else { Some(i.min(cues.len() - 1)) };
            }
            None => {}
        }
        let words: usize = cues.iter().map(|c| c.text.split_whitespace().count()).sum();
        ui.label(RichText::new(format!("{n} frases  •  {words} palavras")).color(p.muted).small());
    }

    // ------------------------------------------------------------ tela

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &Durations) {
        let title = "Legendas";
        widgets::header(ui, p, title, "Legende vídeos com o Whisper, no seu PC e sem internet: grave direto ou revise o texto antes, com legendas paradas ou animadas palavra por palavra.");
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 140.0);
        let file = self.queue.selected_path().cloned();
        // terminou a transcrição do modo manual
        if let Some((f, r)) = self.words.lock().unwrap().take() {
            match r {
                Ok(w) => {
                    let c = captions::cues_from_words(&w);
                    self.msg = Some((!c.is_empty(), if c.is_empty() { "✖ nenhuma fala encontrada no vídeo.".into() } else { format!("✔ {} frases. Corrija o que precisar e grave no vídeo.", c.len()) }));
                    self.sel = (!c.is_empty()).then_some(0);
                    self.cues.insert(f, c);
                }
                Err(e) => self.msg = Some((false, format!("✖ {e}"))),
            }
        }
        if demo() {
            if let Some(f) = file.as_ref().filter(|f| !self.cues.contains_key(*f)) {
                // i18n: fora
                let w = |t: &str, a: f64, b: f64| Word { start: a, end: b, text: t.into() };
                let words = [w("Olá", 0.4, 0.7), w("pessoal,", 0.75, 1.2), w("hoje", 1.3, 1.5), w("eu", 1.55, 1.6), w("vou", 1.65, 1.8), w("mostrar", 1.85, 2.3),
                    w("uma", 2.35, 2.5), w("coisa", 2.55, 2.9), w("incrível.", 2.95, 3.6), w("Fica", 4.4, 4.6), w("até", 4.65, 4.8), w("o", 4.85, 4.9), w("final!", 4.95, 5.5)];
                self.cues.insert(f.clone(), captions::cues_from_words(&words));
                // i18n: dentro
                self.sel = Some(0);
            }
        }
        if self.sel.is_some_and(|i| file.as_ref().and_then(|f| self.cues.get(f)).is_none_or(|c| i >= c.len())) {
            self.sel = None;
        }
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Modo:", |ui| {
                segmented(ui, p, &mut self.saved.manual, &[(false, "Automático"), (true, "Revisar o texto antes")]);
            });
            hint(
                ui,
                p,
                if self.saved.manual {
                    "Transcreve o vídeo escolhido e mostra as frases para você corrigir antes de gravar."
                } else {
                    "Transcreve e grava a legenda em todos os vídeos da lista, sem parar."
                },
            );
            row(ui, p, "Idioma falado:", |ui| {
                let mut li = self.lang_index();
                egui::ComboBox::from_id_salt("caps_lang").selected_text(tr(transcribe::LANGS[li].1)).width(220.0).show_ui(ui, |ui| {
                    for (i, (_, n)) in transcribe::LANGS.iter().enumerate() {
                        ui.selectable_value(&mut li, i, tr(n));
                    }
                });
                self.saved.lang = transcribe::LANGS[li].0.to_string();
            });
            row(ui, p, "Modelo:", |ui| {
                let opts: Vec<(usize, &str)> = WModel::ALL.iter().enumerate().map(|(i, m)| (i, m.label())).collect();
                segmented(ui, p, &mut self.saved.model, &opts);
            });
        });
        if self.saved.manual {
            ui.add_space(4.0);
            card(ui, p, |ui| self.editor(ui, p, file.as_ref()));
        }
        ui.add_space(4.0);
        card(ui, p, |ui| {
            ui.horizontal_top(|ui| {
                crate::widgets::vt(ui, |ui| {
                    ui.set_width((ui.available_width() - 250.0).max(320.0));
                    self.style_controls(ui, p);
                });
                ui.add_space(8.0);
                crate::widgets::vt(ui, |ui| {
                    ui.set_width(220.0);
                    self.preview(ui, p, file.as_ref(), durs);
                    ui.add(egui::Label::new(RichText::new(tr("Clique ou arraste na prévia para mudar a posição.")).color(p.muted).small()).wrap());
                });
            });
        });
        if let Some((good, text)) = &self.msg {
            ui.label(RichText::new(tr(text)).color(if *good { p.ok } else { p.accent }));
        }
        ui.add_space(4.0);
        self.out.ui(ui, p);
        crate::widgets::check(ui, &mut self.saved.keep_ass, tr("Salvar também a legenda (.ass)"));
        let manual = self.saved.manual;
        let can = if manual { file.as_ref().and_then(|f| self.cues.get(f)).is_some_and(|c| !c.is_empty()) } else { !self.queue.files.is_empty() };
        let label = if manual { "Gravar no vídeo" } else { "Legendar" };
        match widgets::action_bar(ui, p, &self.job, label, can) {
            Act::Start => {
                let (out, style, keep) = (self.out.path.clone(), self.saved.style.clone(), self.saved.keep_ass);
                if manual {
                    if let Some(f) = file.clone() {
                        let cues = self.cues.get(&f).cloned().unwrap_or_default();
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &[f], |src, progress| captions::burn(src, out.as_deref(), Source::Cues(cues.clone()), &style, keep, progress, &|| ctl.cancelled()));
                        });
                    }
                } else {
                    let (files, model, lang) = (self.queue.files.clone(), self.model(), self.saved.lang.clone());
                    self.job.start(ui.ctx(), move |ctl| {
                        run_batch(ctl, title, &files, |src, progress| {
                            captions::burn(src, out.as_deref(), Source::Transcribe { model, lang: lang.clone() }, &style, keep, progress, &|| ctl.cancelled())
                        });
                    });
                }
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
        // guarda o estilo e as escolhas quando mudam
        let now = serde_json::to_string(&self.saved).unwrap_or_default();
        if now != self.saved_json && !crate::captura::active() {
            let _ = std::fs::write(saved_path(), serde_json::to_string_pretty(&self.saved).unwrap_or_default());
            self.saved_json = now;
        }
    }
}

/// Botão de cor com o nome ao lado.
fn color(ui: &mut Ui, p: &Palette, label: &str, c: &mut [u8; 3]) {
    egui::color_picker::color_edit_button_srgb(ui, c);
    ui.label(RichText::new(tr(label)).color(p.text));
    ui.add_space(6.0);
}

/// Tempo editável como m:ss.cc (arrastar ou digitar).
fn time_edit(ui: &mut Ui, t: &mut f64) {
    ui.add(
        egui::DragValue::new(t)
            .speed(0.02)
            .range(0.0..=86_400.0)
            .custom_formatter(|v, _| format!("{}:{:05.2}", (v / 60.0).floor() as u64, v % 60.0))
            .custom_parser(|s| {
                let s = s.trim().replace(',', ".");
                match s.split_once(':') {
                    Some((m, sec)) => Some(m.trim().parse::<f64>().ok()? * 60.0 + sec.trim().parse::<f64>().ok()?),
                    None => s.parse().ok(),
                }
            }),
    );
}

fn demo() -> bool {
    crate::captura::active() && std::env::var_os("CONVER_DEMO_LEGENDAS").is_some()
}
