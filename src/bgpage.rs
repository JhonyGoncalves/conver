//! Tela "Remover fundo": recorte automático com ajuste fino (precisão, borda, suavização, anticerrilhado), pincel
//! para manter ou tirar partes e para apagar objetos com a IA do "Apagar objeto", tudo com prévia ao vivo.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use image::{GrayImage, RgbImage, RgbaImage};

use crate::bgremove::{Dot, Refine, Strokes};
use crate::i18n::tr;
use crate::imaging::Background;
use crate::job::{Job, LogKind};
use crate::pages::run_batch;
use crate::theme::{Palette, bold};
use crate::widgets::{self, Act, FileQueue, OutDir, card, hint, row, segmented};

/// O que o pincel faz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    /// Sem pincel: só olhar.
    Ver,
    /// Pintar o que deve ficar (volta para o recorte).
    Manter,
    /// Pintar o que deve sair.
    Tirar,
    /// Pintar um objeto para a IA apagar (preencher com o fundo).
    Objeto,
}

/// Lado maior da imagem usada na prévia.
const PREVIEW_SIDE: u32 = 900;

type Stage = (String, Result<(RgbImage, GrayImage), String>);

pub struct BgPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    bg: Background,
    bg_color: [u8; 3],
    refine: Refine,
    /// Só apaga os objetos pintados e mantém o fundo.
    only_erase: bool,
    brush: Brush,
    brush_size: f32,
    /// Traços de cada arquivo, na ordem (para desfazer).
    strokes: HashMap<PathBuf, Vec<(Brush, Vec<Dot>)>>,
    show_original: bool,
    /// Arquivo aberto na prévia e a imagem reduzida.
    cur: Option<PathBuf>,
    base: Option<RgbImage>,
    tex_orig: Option<egui::TextureHandle>,
    /// Etapa pesada da prévia (apagar objetos + máscara da IA), feita numa thread: chave e resultado.
    stage: Option<(String, RgbImage, GrayImage)>,
    pending: Arc<Mutex<Option<Stage>>>,
    busy: Arc<AtomicBool>,
    wanted: String,
    /// Resultado mostrado e o que gerou ele.
    tex: Option<egui::TextureHandle>,
    tex_key: String,
    err: Option<String>,
}

impl BgPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(crate::imaging::IMAGE_EXTS, "Imagens"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            bg: Background::Transparente,
            bg_color: [0x22, 0x88, 0xee],
            refine: Refine::default(),
            only_erase: false,
            brush: Brush::Ver,
            brush_size: 30.0,
            strokes: HashMap::new(),
            show_original: false,
            cur: None,
            base: None,
            tex_orig: None,
            stage: None,
            pending: Arc::default(),
            busy: Arc::default(),
            wanted: String::new(),
            tex: None,
            tex_key: String::new(),
            err: None,
        }
    }

    fn strokes_of(&self, f: &PathBuf) -> Strokes {
        let mut s = Strokes::default();
        for (kind, dots) in self.strokes.get(f).into_iter().flatten() {
            match kind {
                Brush::Manter => s.keep.extend(dots),
                Brush::Tirar => s.remove.extend(dots),
                Brush::Objeto => s.erase.extend(dots),
                Brush::Ver => {}
            }
        }
        s
    }

    /// Abre o arquivo escolhido na lista (imagem reduzida para a prévia).
    fn open(&mut self, ctx: &egui::Context) {
        let sel = self.queue.selected_path().cloned();
        if sel == self.cur {
            return;
        }
        self.cur = sel.clone();
        self.base = None;
        self.tex_orig = None;
        self.tex = None;
        self.tex_key.clear();
        self.stage = None;
        self.err = None;
        if let Some(f) = sel {
            match crate::imaging::load(&f) {
                Ok(img) => {
                    let rgb = img.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE).to_rgb8();
                    let rgba = image::DynamicImage::ImageRgb8(rgb.clone()).to_rgba8();
                    self.tex_orig = Some(texture(ctx, "fundo_original", &rgba));
                    self.base = Some(rgb);
                }
                Err(e) => self.err = Some(e),
            }
        }
    }

    /// Mantém a prévia em dia: a etapa pesada numa thread (quando mudam o arquivo ou os objetos a apagar), o resto
    /// (ajustes, pincel, fundo) na hora.
    fn update_preview(&mut self, ctx: &egui::Context) {
        let (Some(f), Some(base)) = (self.cur.clone(), self.base.clone()) else { return };
        let s = self.strokes_of(&f);
        let key1 = format!("{f:?}|{:?}|{}", s.erase, self.only_erase);
        if let Some((k, r)) = self.pending.lock().unwrap().take() {
            match r {
                Ok((img, raw)) => {
                    self.stage = Some((k, img, raw));
                    self.err = None;
                }
                Err(e) => self.err = Some(e),
            }
        }
        if self.stage.as_ref().is_none_or(|(k, _, _)| *k != key1) {
            // uma de cada vez; um pedido que deu erro não é repetido até algo mudar
            if !self.busy.load(Ordering::Relaxed) && self.wanted != key1 {
                self.wanted = key1.clone();
                self.busy.store(true, Ordering::Relaxed);
                let (pending, busy, ctx, only) = (self.pending.clone(), self.busy.clone(), ctx.clone(), self.only_erase);
                std::thread::spawn(move || {
                    let r = crate::bgremove::erase_objects(&base, &s, &|| false).and_then(|img| {
                        let raw = if only { GrayImage::from_pixel(img.width(), img.height(), image::Luma([255])) } else { crate::bgremove::mask(&image::DynamicImage::ImageRgb8(img.clone()))? };
                        Ok((img, raw))
                    });
                    *pending.lock().unwrap() = Some((key1, r));
                    busy.store(false, Ordering::Relaxed);
                    ctx.request_repaint();
                });
            }
            return;
        }
        let Some((_, img, raw)) = &self.stage else { return };
        let key2 = format!("{key1}|{:?}|{:?}|{:?}|{:?}", self.refine, s.keep, s.remove, self.bg);
        if key2 == self.tex_key {
            return;
        }
        let (w, h) = img.dimensions();
        let mask = if self.only_erase {
            raw.clone()
        } else {
            let keep = (!s.keep.is_empty()).then(|| crate::bgremove::paint_mask(w, h, &s.keep));
            let remove = (!s.remove.is_empty()).then(|| crate::bgremove::paint_mask(w, h, &s.remove));
            crate::bgremove::refine(raw, &self.refine, keep.as_ref(), remove.as_ref())
        };
        let out = compose(img, &mask, self.bg);
        self.tex = Some(texture(ctx, "fundo_resultado", &out));
        self.tex_key = key2;
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Remover fundo";
        widgets::header(ui, p, title, "Tire o fundo de fotos de pessoas, produtos e animais com inteligência artificial, no seu PC e sem internet.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 96.0);
        self.open(ui.ctx());
        self.update_preview(ui.ctx());
        let running = self.job.state().running;
        ui.add_space(4.0);
        card(ui, p, |ui| {
            // pincel
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tr("Pincel:")).color(p.text));
                segmented(ui, p, &mut self.brush, &[(Brush::Ver, "Só ver"), (Brush::Manter, "Manter"), (Brush::Tirar, "Tirar"), (Brush::Objeto, "Apagar objeto (IA)")]);
                if self.brush != Brush::Ver {
                    ui.add(egui::Slider::new(&mut self.brush_size, 4.0..=120.0).show_value(false));
                }
                let has = self.cur.as_ref().is_some_and(|f| self.strokes.get(f).is_some_and(|v| !v.is_empty()));
                if widgets::secondary(ui, p, "Desfazer", has && !running) {
                    if let Some(v) = self.cur.as_ref().and_then(|f| self.strokes.get_mut(f)) {
                        v.pop();
                    }
                }
                if widgets::secondary(ui, p, "Limpar", has && !running) {
                    if let Some(f) = &self.cur {
                        self.strokes.remove(f);
                    }
                }
                widgets::check(ui, &mut self.show_original, tr("Ver o original"));
            });
            self.canvas(ui, p, running);
            let tip = match self.brush {
                Brush::Ver => "Escolha \"Manter\" ou \"Tirar\" e pinte para corrigir o recorte; \"Apagar objeto\" tira uma coisa da foto e a IA preenche com o fundo.",
                Brush::Manter => "Pinte o que deve ficar na imagem (verde).",
                Brush::Tirar => "Pinte o que deve sair junto com o fundo (vermelho).",
                Brush::Objeto => "Pinte um objeto, pessoa ou texto: a IA apaga e preenche com o que estaria atrás.",
            };
            hint(ui, p, tip);
        });
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Fundo novo:", |ui| {
                let [r, g, b] = self.bg_color;
                let mut kind = match self.bg {
                    Background::Transparente => 0,
                    Background::Cor(255, 255, 255) => 1,
                    Background::Cor(0, 0, 0) => 2,
                    Background::Cor(..) => 3,
                };
                if segmented(ui, p, &mut kind, &[(0, "Transparente"), (1, "Branco"), (2, "Preto"), (3, "Cor")]) {
                    self.bg = match kind {
                        0 => Background::Transparente,
                        1 => Background::Cor(255, 255, 255),
                        2 => Background::Cor(0, 0, 0),
                        _ => Background::Cor(r, g, b),
                    };
                }
                if kind == 3 && ui.color_edit_button_srgb(&mut self.bg_color).changed() {
                    let [r, g, b] = self.bg_color;
                    self.bg = Background::Cor(r, g, b);
                }
            });
            row(ui, p, "O que fazer:", |ui| {
                segmented(ui, p, &mut self.only_erase, &[(false, "Tirar o fundo"), (true, "Só apagar os objetos pintados")]);
            });
            if !self.only_erase {
                row(ui, p, "Borda:", |ui| {
                    segmented(ui, p, &mut self.refine.hard, &[(true, "Definida"), (false, "Suave (cabelo, pelos)")]);
                });
                row(ui, p, "Precisão:", |ui| {
                    ui.add(egui::Slider::new(&mut self.refine.precision, 0.0..=100.0).step_by(1.0).max_decimals(0).suffix(" %"));
                });
                hint(ui, p, "Mais alta corta mais (só o que é com certeza o assunto); mais baixa mantém mais (sombras, fios soltos).");
                row(ui, p, "Ajustar a borda:", |ui| {
                    ui.add(egui::Slider::new(&mut self.refine.grow, -20..=20).suffix(" px"));
                });
                row(ui, p, "Suavizar a borda:", |ui| {
                    ui.add(egui::Slider::new(&mut self.refine.feather, 0.0..=20.0).step_by(0.5).max_decimals(1).suffix(" px"));
                });
                row(ui, p, "Anticerrilhado:", |ui| {
                    widgets::check(ui, &mut self.refine.antialias, tr("Borda lisa, sem \"escadinha\""));
                });
                if widgets::secondary(ui, p, "Voltar ao padrão", self.refine != Refine::default()) {
                    self.refine = Refine::default();
                }
            }
            hint(ui, p, "Os ajustes valem para todas as imagens da lista; as pinceladas, só para a imagem em que você pintou.");
            if self.strokes.values().flatten().any(|(k, _)| *k == Brush::Objeto) {
                crate::ai::model_hint(ui, p, &crate::inpaint::MODEL);
            }
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let ready = !self.queue.files.is_empty() && (!self.only_erase || self.strokes.values().flatten().any(|(k, _)| *k == Brush::Objeto));
        match widgets::action_bar(ui, p, &self.job, if self.only_erase { "Apagar objetos" } else { "Remover fundo" }, ready) {
            Act::Start => {
                let files = self.queue.files.clone();
                let strokes: HashMap<PathBuf, Strokes> = files.iter().map(|f| (f.clone(), self.strokes_of(f))).collect();
                let (out, bg, refine, only) = (self.out.path.clone(), self.bg, self.refine, self.only_erase);
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| {
                        let s = strokes.get(src).cloned().unwrap_or_default();
                        if only && s.erase.is_empty() {
                            return Err(tr("nada pintado para apagar nesta imagem."));
                        }
                        let img = crate::imaging::load(src)?;
                        progress(0.1);
                        let rgb = crate::bgremove::erase_objects(&img.to_rgb8(), &s, &|| ctl.cancelled())?;
                        progress(0.5);
                        let (w, h) = rgb.dimensions();
                        let mask = if only { GrayImage::from_pixel(w, h, image::Luma([255])) } else { crate::bgremove::final_mask(&rgb, &refine, &s)? };
                        progress(0.8);
                        let rgba = compose(&rgb, &mask, if only { Background::Cor(0, 0, 0) } else { bg });
                        let transparent = !only && bg == Background::Transparente;
                        let ext = if transparent { "png" } else if matches!(crate::tools::ext_lower(src).as_str(), "jpg" | "jpeg") { "jpg" } else { "png" };
                        let dst = crate::tools::unique_path(crate::tools::out_path(src, out.as_deref(), if only { "_apagado" } else { "_sem_fundo" }, ext));
                        let saved = if ext == "jpg" { image::DynamicImage::ImageRgba8(rgba).to_rgb8().save(&dst) } else { rgba.save(&dst) };
                        saved.map_err(|e| format!("não foi possível salvar: {e}"))?;
                        let kept = mask.pixels().filter(|a| a[0] > 127).count() as f64 / mask.len().max(1) as f64 * 100.0;
                        let info = if only { format!("{w}×{h}") } else { format!("fundo removido • {kept:.0}% da imagem mantida") };
                        Ok((info, dst))
                    });
                    if ctl.cancelled() {
                        ctl.log(LogKind::Info, "Cancelado.");
                    }
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }

    /// Prévia grande, onde se pinta.
    fn canvas(&mut self, ui: &mut Ui, p: &Palette, running: bool) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 420.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, p.input);
        let Some(base) = &self.base else {
            let t = self.err.clone().unwrap_or_else(|| tr("Adicione uma imagem para começar."));
            painter.text(rect.center(), Align2::CENTER_CENTER, t, FontId::proportional(14.0), if self.err.is_some() { p.accent } else { p.muted });
            return;
        };
        let (iw, ih) = (base.width() as f32, base.height() as f32);
        let shown = widgets::fit((iw, ih), rect.size() - vec2(12.0, 12.0));
        let r = egui::Rect::from_center_size(rect.center(), shown);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let tex = if self.show_original { self.tex_orig.clone() } else { self.tex.clone().or_else(|| self.tex_orig.clone()) };
        if !self.show_original && self.tex.is_some() && self.bg == Background::Transparente && !self.only_erase {
            widgets::checkerboard(&painter, r);
        }
        if let Some(t) = tex {
            painter.image(t.id(), r, uv, Color32::WHITE);
        }
        let waiting = !self.show_original && (self.tex.is_none() || self.busy.load(Ordering::Relaxed));
        // pintar (em proporções da imagem)
        if !running && self.brush != Brush::Ver {
            if let (Some(f), Some(pos)) = (self.cur.clone(), resp.interact_pointer_pos().filter(|q| r.contains(*q))) {
                let list = self.strokes.entry(f).or_default();
                if resp.drag_started() || resp.clicked() || list.last().is_none_or(|(k, _)| *k != self.brush) {
                    list.push((self.brush, Vec::new()));
                }
                if resp.dragged() || resp.clicked() || resp.drag_started() {
                    let d = Dot { x: (pos.x - r.left()) / r.width(), y: (pos.y - r.top()) / r.height(), r: self.brush_size / 2.0 / r.width() };
                    let last = &mut list.last_mut().unwrap().1;
                    // pontos no meio, para o traço não ficar com buracos ao arrastar rápido
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
                painter.circle_stroke(h, self.brush_size / 2.0, Stroke::new(1.5f32, p.accent));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
        // traços por cima: verde = manter, vermelho = tirar, cinza = objeto a apagar
        if !self.show_original {
            if let Some(list) = self.cur.as_ref().and_then(|f| self.strokes.get(f)) {
                for (kind, dots) in list {
                    let c = match kind {
                        Brush::Manter => p.ok,
                        Brush::Tirar => p.accent,
                        _ => p.muted,
                    };
                    let c = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 90);
                    for d in dots {
                        painter.circle_filled(egui::pos2(r.left() + d.x * r.width(), r.top() + d.y * r.height()), d.r * r.width(), c);
                    }
                }
            }
        }
        if waiting {
            painter.rect_filled(egui::Rect::from_min_size(r.left_top() + vec2(8.0, 8.0), vec2(150.0, 24.0)), 6, Color32::from_black_alpha(150));
            painter.text(r.left_top() + vec2(16.0, 20.0), Align2::LEFT_CENTER, tr("Atualizando a prévia…"), FontId::new(12.0, bold()), Color32::WHITE);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        painter.rect_stroke(r, 0, Stroke::new(1.0f32, p.border), StrokeKind::Outside);
    }
}

/// Junta a imagem e a máscara: transparente ou sobre uma cor.
fn compose(img: &RgbImage, mask: &GrayImage, bg: Background) -> RgbaImage {
    RgbaImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = mask.get_pixel(x, y)[0] as u32;
        match bg {
            Background::Transparente => image::Rgba([p[0], p[1], p[2], a as u8]),
            Background::Cor(r, g, b) => {
                let mix = |c: u8, k: u8| ((c as u32 * a + k as u32 * (255 - a)) / 255) as u8;
                image::Rgba([mix(p[0], r), mix(p[1], g), mix(p[2], b), 255])
            }
        }
    })
}

fn texture(ctx: &egui::Context, name: &str, img: &RgbaImage) -> egui::TextureHandle {
    let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
}
