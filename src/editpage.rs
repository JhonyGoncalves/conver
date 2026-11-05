//! Tela "Editar imagem": mudar uma imagem com IA no próprio PC. Três jeitos: escrever o que mudar (FLUX.2 klein),
//! pintar uma área e dizer o que colocar ali, ou criar uma versão parecida (com os modelos de "Gerar imagem").

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::bgremove::Dot;
use crate::i18n::tr;
use crate::imagegen::{EditMode, EditRequest, Engine, Quality};
use crate::job::{Job, LogKind};
use crate::theme::{Palette, bold};
use crate::widgets::{self, Act, FileQueue, OutDir, card, row, segmented};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Instrucao,
    Pintar,
    Variar,
}

pub struct EditPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    mode: Mode,
    prompt: String,
    engine: Engine,
    quality: Quality,
    /// Quanto a versão nova muda (variar): 0,5 / 0,7 / 0,85 (com 0,55 o Z-Image quase não muda nada).
    strength: u8,
    count: u32,
    translate: bool,
    brush: f32,
    /// Área pintada (modo pintar), em proporções da imagem.
    strokes: Vec<Vec<Dot>>,
    cur: Option<PathBuf>,
    dims: (u32, u32),
    tex: Option<egui::TextureHandle>,
    results: Arc<Mutex<Vec<PathBuf>>>,
    shown: Vec<(PathBuf, egui::TextureHandle)>,
}

impl EditPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(crate::imaging::IMAGE_EXTS, "Imagens"),
            out: OutDir::fixed(crate::imagegen::default_dir()),
            job: Job::default(),
            mode: Mode::Instrucao,
            prompt: String::new(),
            engine: Engine::best_installed(),
            quality: Quality::Rapida,
            strength: 1,
            count: 1,
            translate: true,
            brush: 40.0,
            strokes: Vec::new(),
            cur: None,
            dims: (1, 1),
            tex: None,
            results: Arc::default(),
            shown: Vec::new(),
        }
    }

    /// Abre uma imagem na tela (vinda de "Gerar imagem" ou de um resultado).
    pub fn open_file(&mut self, f: PathBuf) {
        let _ = self.queue.add(vec![f.clone()]);
        if let Some(i) = self.queue.files.iter().position(|x| *x == f) {
            self.queue.selected = i;
        }
    }

    fn load_selected(&mut self, ctx: &egui::Context) {
        let sel = self.queue.selected_path().cloned();
        if sel == self.cur {
            return;
        }
        self.cur = sel.clone();
        self.strokes.clear();
        self.tex = None;
        if let Some(f) = sel {
            if let Ok(img) = crate::imaging::load(&f) {
                self.dims = (img.width(), img.height());
                let rgba = img.thumbnail(1200, 1200).to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                self.tex = Some(ctx.load_texture("editar_base", ci, egui::TextureOptions::LINEAR));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Editar imagem";
        widgets::header(ui, p, title, "Mude uma imagem com IA no seu PC: escreva o que mudar, pinte uma área e diga o que colocar, ou crie uma versão parecida.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 96.0);
        self.load_selected(ui.ctx());
        let running = self.job.state().running;
        let ai_ok = crate::ai_api::Settings::load().has_key() || !crate::ai_api::Settings::load().base_url.is_empty();
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Como editar:", |ui| {
                segmented(ui, p, &mut self.mode, &[(Mode::Instrucao, "Pedir uma mudança"), (Mode::Pintar, "Pintar e pedir"), (Mode::Variar, "Variar a imagem")]);
            });
            let (label, example) = match self.mode {
                Mode::Instrucao => ("O que mudar", "ex.: deixe de noite com céu estrelado; troque a camisa por uma azul; tire as pessoas do fundo"),
                Mode::Pintar => ("O que colocar na área pintada", "ex.: um chapéu de palha; uma janela; flores vermelhas"),
                Mode::Variar => ("Como deve ficar", "ex.: a mesma cena no outono; estilo aquarela"),
            };
            ui.label(RichText::new(tr(label)).family(bold()).color(p.text));
            ui.add(egui::TextEdit::multiline(&mut self.prompt).hint_text(tr(example)).desired_rows(2).desired_width(f32::INFINITY));
            if self.mode == Mode::Pintar {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Pincel:")).color(p.text));
                    ui.add(egui::Slider::new(&mut self.brush, 8.0..=160.0).show_value(false));
                    if widgets::secondary(ui, p, "Desfazer", !self.strokes.is_empty() && !running) {
                        self.strokes.pop();
                    }
                    if widgets::secondary(ui, p, "Limpar", !self.strokes.is_empty() && !running) {
                        self.strokes.clear();
                    }
                });
            }
            self.canvas(ui, p, running);
            if self.mode != Mode::Instrucao {
                row(ui, p, "Modelo:", |ui| {
                    segmented(ui, p, &mut self.engine, &[(Engine::ZImage, "Z-Image Turbo (melhor)"), (Engine::Xl, "SDXL Lightning"), (Engine::Rapido, "Rápido")]);
                });
            }
            if self.mode == Mode::Variar {
                row(ui, p, "Quanto mudar:", |ui| {
                    segmented(ui, p, &mut self.strength, &[(0, "Pouco"), (1, "Médio"), (2, "Muito")]);
                });
            }
            row(ui, p, "Qualidade:", |ui| {
                segmented(ui, p, &mut self.quality, &[(Quality::Rapida, "Rápida"), (Quality::Melhor, "Melhor (mais lenta)")]);
            });
            row(ui, p, "Quantas:", |ui| {
                segmented(ui, p, &mut self.count, &[(1, "1"), (2, "2")]);
            });
            row(ui, p, "Idioma:", |ui| {
                let mut off = false;
                let value = if ai_ok { &mut self.translate } else { &mut off };
                widgets::check_enabled(ui, ai_ok, value, tr("Traduzir o pedido para o inglês com a IA configurada"));
            });
            let (files, what) = match self.mode {
                Mode::Instrucao => (crate::imagegen::edit_files(), "FLUX.2 klein 4B"),
                _ => (self.engine.files(), ""),
            };
            let installed = files.iter().all(crate::ai::is_installed);
            let size: u64 = files.iter().filter(|f| !crate::ai::is_installed(f)).map(|f| f.size).sum();
            let state = if installed { tr("✔ Pronto neste computador.") } else { tr(format!("Baixado uma vez, na primeira edição ({}).", crate::tools::human_size(size))) };
            let info = match self.mode {
                Mode::Instrucao => tr(format!("Usa o modelo de edição {what}; muda só o que você pedir e mantém o resto. Alguns minutos por imagem."))
                    + "  "
                    + &state,
                Mode::Pintar => tr("Só a área pintada muda; o resto da imagem fica igual.") + "  " + &state,
                Mode::Variar => tr("Cria uma imagem nova parecida com a original: \"Pouco\" mantém quase tudo, \"Muito\" muda bastante.") + "  " + &state,
            };
            ui.label(RichText::new(info).size(12.5).color(p.muted));
        });
        // versões prontas
        let new: Vec<PathBuf> = std::mem::take(&mut *self.results.lock().unwrap());
        if !new.is_empty() {
            self.shown.clear();
            for f in new {
                if let Ok(img) = image::open(&f) {
                    let rgba = img.thumbnail(360, 360).to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                    let tex = ui.ctx().load_texture(format!("editada_{}", f.display()), ci, egui::TextureOptions::LINEAR);
                    self.shown.push((f, tex));
                }
            }
        }
        if !self.shown.is_empty() {
            ui.add_space(4.0);
            let mut use_as_base = None;
            card(ui, p, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (f, tex) in &self.shown {
                        widgets::vt(ui, |ui| {
                            let s = tex.size_vec2();
                            let size = widgets::fit((s.x, s.y), vec2(220.0, 220.0));
                            let r = ui.add(egui::Image::new(tex).fit_to_exact_size(size).corner_radius(6).sense(Sense::click()));
                            if r.on_hover_text(tr("Clique para abrir")).clicked() {
                                crate::tools::open_file(f);
                            }
                            if widgets::secondary(ui, p, "Continuar editando esta", !running) {
                                use_as_base = Some(f.clone());
                            }
                        });
                    }
                });
            });
            if let Some(f) = use_as_base {
                self.open_file(f);
            }
        }
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let painted = self.mode != Mode::Pintar || !self.strokes.is_empty();
        let ready = self.cur.is_some() && !self.prompt.trim().is_empty() && painted;
        match widgets::action_bar(ui, p, &self.job, "Editar", ready) {
            Act::Start => {
                let Some(src) = self.cur.clone() else { return };
                let mode = match self.mode {
                    Mode::Instrucao => EditMode::Instruction,
                    Mode::Pintar => {
                        let dots: Vec<Dot> = self.strokes.iter().flatten().copied().collect();
                        EditMode::Paint(crate::bgremove::paint_mask(self.dims.0, self.dims.1, &dots))
                    }
                    Mode::Variar => EditMode::Vary([0.5, 0.7, 0.85][self.strength as usize]),
                };
                let mut req = EditRequest { prompt: self.prompt.clone(), mode, engine: self.engine, quality: self.quality, count: self.count, seed: None };
                let translate = self.translate && ai_ok;
                let dir = self.out.path.clone().unwrap_or_else(crate::imagegen::default_dir);
                let slot = self.results.clone();
                self.job.start(ui.ctx(), move |ctl| {
                    if translate {
                        ctl.status("Traduzindo o pedido…");
                        // i18n: fora
                        let r = crate::ai_api::Settings::load().config().and_then(|c| {
                            crate::ai_api::ask(&c, "Translate the user's image editing request to English for an image model. Reply with only the translation.", &[crate::ai_api::Part::Text(req.prompt.clone())], 300)
                        });
                        // i18n: dentro
                        match r {
                            Ok(t) => {
                                ctl.log(LogKind::Info, format!("Pedido em inglês: {t}"));
                                req.prompt = t;
                            }
                            Err(e) => ctl.log(LogKind::Info, format!("Sem tradução ({e}); usando o texto como está.")),
                        }
                    }
                    ctl.status("Editando…");
                    match crate::imagegen::edit(&src, &req, &dir, &|x| ctl.progress(x), &|| ctl.cancelled()) {
                        Ok(files) => {
                            for f in &files {
                                crate::history::record(title, &crate::tools::file_name(&src), f, &req.prompt.chars().take(60).collect::<String>());
                            }
                            ctl.log(LogKind::Ok, format!("✔ {} imagem(ns) em {}", files.len(), dir.display()));
                            ctl.status("Pronto.");
                            if let Some(f) = files.first() {
                                ctl.done_folder(f.clone());
                            }
                            *slot.lock().unwrap() = files;
                        }
                        Err(e) => {
                            ctl.log(LogKind::Err, format!("✖ {e}"));
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

    /// A imagem; no modo "pintar", onde se marca a área.
    fn canvas(&mut self, ui: &mut Ui, p: &Palette, running: bool) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 360.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, p.input);
        let Some(tex) = self.tex.clone() else {
            painter.text(rect.center(), Align2::CENTER_CENTER, tr("Adicione uma imagem para começar."), FontId::proportional(14.0), p.muted);
            return;
        };
        let s = tex.size_vec2();
        let shown = widgets::fit((s.x, s.y), rect.size() - vec2(12.0, 12.0));
        let r = egui::Rect::from_center_size(rect.center(), shown);
        painter.image(tex.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        if self.mode == Mode::Pintar {
            if !running {
                if let Some(pos) = resp.interact_pointer_pos().filter(|q| r.contains(*q)) {
                    if resp.drag_started() || resp.clicked() || self.strokes.is_empty() {
                        self.strokes.push(Vec::new());
                    }
                    if resp.dragged() || resp.clicked() || resp.drag_started() {
                        let d = Dot { x: (pos.x - r.left()) / r.width(), y: (pos.y - r.top()) / r.height(), r: self.brush / 2.0 / r.width() };
                        let last = self.strokes.last_mut().unwrap();
                        if let Some(prev) = last.last().copied() {
                            let (dx, dy) = (d.x - prev.x, (d.y - prev.y) * r.height() / r.width());
                            let steps = ((dx * dx + dy * dy).sqrt() / (d.r * 0.5).max(1e-4)).ceil() as usize;
                            for k in 1..steps.min(200) {
                                let t = k as f32 / steps as f32;
                                last.push(Dot { x: prev.x + (d.x - prev.x) * t, y: prev.y + (d.y - prev.y) * t, r: d.r });
                            }
                        }
                        last.push(d);
                    }
                }
                if let Some(h) = resp.hover_pos().filter(|h| r.contains(*h)) {
                    painter.circle_stroke(h, self.brush / 2.0, Stroke::new(1.5f32, p.accent));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                }
            }
            let c = Color32::from_rgba_unmultiplied(p.accent.r(), p.accent.g(), p.accent.b(), 110);
            for d in self.strokes.iter().flatten() {
                painter.circle_filled(egui::pos2(r.left() + d.x * r.width(), r.top() + d.y * r.height()), d.r * r.width(), c);
            }
        }
        if running {
            painter.rect_filled(r, 0, Color32::from_black_alpha(90));
            painter.text(r.center(), Align2::CENTER_CENTER, tr("A IA está editando…"), FontId::new(16.0, bold()), Color32::WHITE);
        }
        painter.rect_stroke(r, 0, Stroke::new(1.0f32, p.border), StrokeKind::Outside);
    }
}
