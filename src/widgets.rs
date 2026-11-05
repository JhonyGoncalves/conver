//! Componentes de interface reutilizados pelas telas.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align, Align2, Button, Color32, ColorImage, CursorIcon, FontId, Frame, Layout, Margin, ProgressBar,
    Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, vec2,
};

use crate::imaging;
use crate::job::{Job, LogKind};
use crate::theme::{Palette, bold};
use crate::tools;

// ---------------------------------------------------------------- básicos

/// Margem dos campos de texto: com ela o campo tem a mesma altura (30px) que botões e menus.
pub const FIELD_MARGIN: Margin = Margin::symmetric(8, 6);

// ------------------------------------------------- direção (árabe, urdu, persa)
//
// Nos idiomas da direita para a esquerda o layout inteiro é espelhado: menu à direita, rótulos à direita dos
// controles, ação principal à esquerda. As colunas usam `top_down(Align::Max)`, e o egui já faz os
// `ui.horizontal` dentro delas correrem da direita para a esquerda. Layouts explícitos usam os ajudantes abaixo.

pub fn rtl() -> bool {
    crate::i18n::current().rtl
}

/// Linha no sentido da leitura (o `Layout::left_to_right` de sempre).
pub fn fwd(align: Align) -> Layout {
    if rtl() { Layout::right_to_left(align) } else { Layout::left_to_right(align) }
}

/// Linha a partir do fim (o `Layout::right_to_left` de sempre: barras de ação, botões no canto).
pub fn back(align: Align) -> Layout {
    if rtl() { Layout::left_to_right(align) } else { Layout::right_to_left(align) }
}

/// Coluna alinhada pelo começo da leitura.
pub fn down() -> Layout {
    Layout::top_down(if rtl() { Align::Max } else { Align::Min })
}

/// `ui.vertical` que respeita a direção.
pub fn vt<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    ui.allocate_new_ui(egui::UiBuilder::new().layout(down()), add)
}

/// Caixa de seleção com o texto do lado certo (a caixa fica à direita do texto em árabe, urdu e persa).
pub fn check(ui: &mut Ui, value: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    if !rtl() {
        return ui.checkbox(value, text);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut r = ui.add(egui::Checkbox::without_text(value));
        let l = ui.add(egui::Label::new(text).sense(Sense::click())).on_hover_cursor(CursorIcon::PointingHand);
        if l.clicked() && ui.is_enabled() {
            *value = !*value;
            r.mark_changed();
        }
        r.union(l)
    })
    .inner
}

/// `check` só ativo quando `enabled`.
pub fn check_enabled(ui: &mut Ui, enabled: bool, value: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| check(ui, value, text)).inner
}

/// Retângulo espelhado dentro de `outer` (para o que é desenhado à mão).
pub fn mirror(r: Rect, outer: Rect) -> Rect {
    if rtl() { Rect::from_x_y_ranges((outer.left() + outer.right() - r.right())..=(outer.left() + outer.right() - r.left()), r.y_range()) } else { r }
}

pub fn card<R>(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::default()
        .fill(p.card)
        .stroke(Stroke::new(1.0f32, p.border))
        .corner_radius(12)
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

pub fn header(ui: &mut Ui, p: &Palette, title: &str, subtitle: &str) {
    ui.label(RichText::new(crate::i18n::tr(title)).heading().color(p.text));
    ui.label(RichText::new(crate::i18n::tr(subtitle)).color(p.muted));
    ui.add_space(8.0);
}

pub fn primary(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> bool {
    let (fill, color) = if enabled { (p.action, p.on_accent) } else { (p.border, p.muted) };
    let b = Button::new(RichText::new(crate::i18n::tr(text)).family(bold()).size(15.0).color(color))
        .fill(fill)
        .stroke(Stroke::NONE)
        .corner_radius(8)
        .min_size(vec2(150.0, 38.0))
        .sense(if enabled { Sense::click() } else { Sense::hover() });
    let r = ui.add(b);
    if enabled {
        r.on_hover_cursor(CursorIcon::PointingHand).clicked()
    } else {
        false
    }
}

pub fn secondary(ui: &mut Ui, p: &Palette, text: &str, enabled: bool) -> bool {
    let b = Button::new(RichText::new(crate::i18n::tr(text)).color(p.text))
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0f32, p.border))
        .corner_radius(8)
        .min_size(vec2(0.0, 32.0));
    ui.add_enabled(enabled, b).clicked()
}

/// Botões lado a lado em que só um fica selecionado.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, p: &Palette, value: &mut T, opts: &[(T, &str)]) -> bool {
    let mut changed = false;
    // quando as opções não cabem na linha (muitas opções, ou textos longos em outro idioma), vira um menu suspenso
    let labels: Vec<String> = opts.iter().map(|(_, l)| crate::i18n::tr(*l)).collect();
    let font = egui::TextStyle::Button.resolve(ui.style());
    let needed: f32 = labels
        .iter()
        .map(|l| ui.fonts(|f| f.layout_no_wrap(l.clone(), font.clone(), p.text).size().x) + 2.0 * ui.spacing().button_padding.x + 2.0)
        .sum::<f32>()
        + 10.0;
    if needed > ui.available_width() {
        let current = opts.iter().position(|(v, _)| *v == *value).map(|i| labels[i].clone()).unwrap_or_default();
        let id = ui.id().with(("segmentado", opts.first().map(|o| o.1).unwrap_or_default()));
        egui::ComboBox::from_id_salt(id).selected_text(current).width(ui.available_width().clamp(120.0, 300.0)).show_ui(ui, |ui| {
            for ((v, _), label) in opts.iter().zip(&labels) {
                if ui.selectable_label(*value == *v, label).clicked() && *value != *v {
                    *value = *v;
                    changed = true;
                }
            }
        });
        return changed;
    }
    Frame::default()
        .fill(p.input)
        .stroke(Stroke::new(1.0f32, p.border))
        .corner_radius(9)
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            // a primeira opção fica no começo da leitura (à direita em árabe, urdu e persa)
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (v, label) in opts {
                    let sel = *value == *v;
                    // a escolhida fica em cinábrio suave: o vermelho cheio é só da ação (botão, menu, progresso)
                    let text = RichText::new(crate::i18n::tr(*label)).color(p.text);
                    let b = Button::new(if sel { text.family(bold()) } else { text })
                        .fill(if sel { p.accent_soft } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .corner_radius(6)
                        .min_size(vec2(0.0, 28.0));
                    if ui.add(b).on_hover_cursor(CursorIcon::PointingHand).clicked() && !sel {
                        *value = *v;
                        changed = true;
                    }
                }
            });
        });
    changed
}

/// Linha "rótulo: controle" com largura de rótulo fixa.
pub fn row<R>(ui: &mut Ui, p: &Palette, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(150.0, 30.0), fwd(Align::Center), |ui| {
            ui.set_min_size(vec2(150.0, 30.0));
            ui.label(RichText::new(crate::i18n::tr(label)).color(p.text));
        });
        add(ui)
    })
    .inner
}

pub fn hint(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(crate::i18n::tr(text)).size(12.5).color(p.muted));
}

pub fn fit(size: (f32, f32), max: egui::Vec2) -> egui::Vec2 {
    let s = (max.x / size.0).min(max.y / size.1).min(1.0);
    vec2(size.0 * s, size.1 * s)
}

// ---------------------------------------------------------- miniaturas

#[derive(Clone)]
pub enum Thumb {
    Loading,
    Ready { tex: TextureHandle, dims: (u32, u32) },
    Failed,
}

/// Cache de miniaturas, carregadas por 3 threads em segundo plano.
pub struct Thumbs {
    map: Arc<Mutex<HashMap<PathBuf, Thumb>>>,
    tx: Sender<PathBuf>,
}

impl Thumbs {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel::<PathBuf>();
        let rx = Arc::new(Mutex::new(rx));
        let map: Arc<Mutex<HashMap<PathBuf, Thumb>>> = Arc::default();
        for _ in 0..3 {
            let (rx, map, ctx) = (rx.clone(), map.clone(), ctx.clone());
            std::thread::spawn(move || {
                loop {
                    let path = match rx.lock().unwrap().recv() {
                        Ok(p) => p,
                        Err(_) => break,
                    };
                    let thumb = match imaging::load(&path) {
                        Ok(img) => {
                            let t = imaging::thumbnail(&img, 720);
                            let ci = ColorImage::from_rgba_unmultiplied(
                                [t.width() as usize, t.height() as usize],
                                t.as_raw(),
                            );
                            let tex = ctx.load_texture(path.to_string_lossy(), ci, TextureOptions::LINEAR);
                            Thumb::Ready { tex, dims: (img.width(), img.height()) }
                        }
                        Err(_) => Thumb::Failed,
                    };
                    map.lock().unwrap().insert(path, thumb);
                    ctx.request_repaint();
                }
            });
        }
        Self { map, tx }
    }

    pub fn get(&self, path: &Path) -> Thumb {
        let mut map = self.map.lock().unwrap();
        if let Some(t) = map.get(path) {
            return t.clone();
        }
        map.insert(path.to_path_buf(), Thumb::Loading);
        let _ = self.tx.send(path.to_path_buf());
        Thumb::Loading
    }
}

/// Cache da duração de áudios e vídeos, lida em segundo plano.
pub struct Durations {
    map: Arc<Mutex<HashMap<PathBuf, Option<f64>>>>,
    tx: Sender<PathBuf>,
}

impl Durations {
    pub fn new(ctx: &egui::Context) -> Self {
        let (tx, rx) = channel::<PathBuf>();
        let map: Arc<Mutex<HashMap<PathBuf, Option<f64>>>> = Arc::default();
        let (m, ctx) = (map.clone(), ctx.clone());
        std::thread::spawn(move || {
            while let Ok(path) = rx.recv() {
                let d = crate::video::duration(&path);
                m.lock().unwrap().insert(path, d);
                ctx.request_repaint();
            }
        });
        Self { map, tx }
    }

    /// `None` enquanto não foi lida (ou se não deu para ler).
    pub fn get(&self, path: &Path) -> Option<f64> {
        let mut map = self.map.lock().unwrap();
        if let Some(d) = map.get(path) {
            return *d;
        }
        map.insert(path.to_path_buf(), None);
        let _ = self.tx.send(path.to_path_buf());
        None
    }
}

// -------------------------------------------------------- lista de arquivos

pub struct FileQueue {
    pub files: Vec<PathBuf>,
    pub selected: usize,
    /// Extensões aceitas; vazia = qualquer arquivo.
    pub exts: &'static [&'static str],
    kind: &'static str,
    /// Mostra botões para mudar a ordem (juntar vídeos, PDF).
    pub reorder: bool,
}

impl FileQueue {
    pub fn new(exts: &'static [&'static str], kind: &'static str) -> Self {
        Self { files: Vec::new(), selected: 0, exts, kind, reorder: false }
    }

    pub fn ordered(mut self) -> Self {
        self.reorder = true;
        self
    }

    pub fn accepts(&self, p: &Path) -> bool {
        p.is_file() && (self.exts.is_empty() || self.exts.contains(&tools::ext_lower(p).as_str()))
    }

    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let before = self.files.len();
        for p in paths {
            if self.accepts(&p) && !self.files.contains(&p) {
                self.files.push(p);
            }
        }
        self.files.len() - before
    }

    pub fn pick(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title(crate::i18n::tr("Selecione os arquivos"));
        if !self.exts.is_empty() {
            dialog = dialog.add_filter(crate::i18n::tr(self.kind), self.exts);
        }
        if let Some(paths) = dialog.add_filter(crate::i18n::tr("Todos os arquivos"), &["*"]).pick_files() {
            let first_new = self.files.len();
            if self.add(paths) > 0 {
                self.selected = first_new;
            }
        }
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        self.files.get(self.selected)
    }
}

/// Lista com miniaturas (se `thumbs` for dado) ou duração (se `durs` for dado), seleção e remoção.
/// Vazia, vira uma área para soltar arquivos.
pub fn file_list(
    ui: &mut Ui,
    p: &Palette,
    q: &mut FileQueue,
    thumbs: Option<&Thumbs>,
    durs: Option<&Durations>,
    height: f32,
) {
    card(ui, p, |ui| {
        ui.horizontal(|ui| {
            let n = q.files.len();
            let text = match n {
                0 => "Nenhum arquivo".to_string(),
                1 => "1 arquivo".to_string(),
                n => format!("{n} arquivos"),
            };
            ui.label(RichText::new(crate::i18n::tr(text)).family(bold()).color(p.text));
            ui.with_layout(back(Align::Center), |ui| {
                if secondary(ui, p, "Limpar", n > 0) {
                    q.files.clear();
                    q.selected = 0;
                }
                if secondary(ui, p, "+ Adicionar", true) {
                    q.pick();
                }
                if q.reorder && n > 1 {
                    let i = q.selected;
                    if secondary(ui, p, "↓", i + 1 < n) {
                        q.files.swap(i, i + 1);
                        q.selected += 1;
                    }
                    if secondary(ui, p, "↑", i > 0) {
                        q.files.swap(i, i - 1);
                        q.selected -= 1;
                    }
                    if ui.available_width() > 70.0 {
                        ui.label(RichText::new(crate::i18n::tr("ordem:")).size(12.5).color(p.muted));
                    }
                }
            });
        });
        ui.add_space(4.0);

        if q.files.is_empty() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
            let hovered = resp.hovered();
            let painter = ui.painter();
            painter.rect_filled(rect, 10, if hovered { p.accent_soft } else { p.input });
            dashed_border(painter, rect.shrink(1.0), if hovered { p.accent } else { p.border });
            painter.text(rect.center() - vec2(0.0, 22.0), Align2::CENTER_CENTER, crate::i18n::tr("⬇"), FontId::proportional(30.0), p.accent);
            painter.text(
                rect.center() + vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                crate::i18n::tr("Arraste arquivos para cá"),
                FontId::new(16.0, bold()),
                p.text,
            );
            painter.text(
                rect.center() + vec2(0.0, 38.0),
                Align2::CENTER_CENTER,
                crate::i18n::tr("ou clique para selecionar"),
                FontId::proportional(13.0),
                p.muted,
            );
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                q.pick();
            }
            return;
        }

        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt(("files", q.kind, q.exts.len()))
            .max_height(height)
            .min_scrolled_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (i, path) in q.files.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                    let selected = i == q.selected;
                    let fill = if selected {
                        p.accent_soft
                    } else if resp.hovered() {
                        p.input
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, 8, fill);

                    let mut x = rect.left() + 8.0;
                    let mut dims = None;
                    if let Some(thumbs) = thumbs {
                        let slot = mirror(Rect::from_min_size(egui::pos2(x, rect.center().y - 20.0), vec2(40.0, 40.0)), rect);
                        ui.painter().rect_filled(slot, 6, p.input);
                        if let Thumb::Ready { tex, dims: d } = thumbs.get(path) {
                            let s = tex.size_vec2();
                            let size = fit((s.x, s.y), vec2(40.0, 40.0));
                            let r = Rect::from_center_size(slot.center(), size);
                            ui.painter().image(
                                tex.id(),
                                r,
                                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            dims = Some(d);
                        }
                        x += 50.0;
                    } else {
                        // etiqueta com a extensão do arquivo
                        let slot = mirror(Rect::from_min_size(egui::pos2(x, rect.center().y - 14.0), vec2(42.0, 28.0)), rect);
                        ui.painter().rect_filled(slot, 6, p.action);
                        let ext: String = tools::ext_lower(path).to_uppercase().chars().take(4).collect();
                        ui.painter().text(slot.center(), Align2::CENTER_CENTER, crate::i18n::tr(ext), FontId::new(11.0, bold()), p.on_accent);
                        x += 52.0;
                    }

                    let close_ltr = Rect::from_center_size(egui::pos2(rect.right() - 20.0, rect.center().y), vec2(26.0, 26.0));
                    let close = mirror(close_ltr, rect);
                    let text_clip = mirror(Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(close_ltr.left() - 6.0, rect.bottom())), rect);
                    let painter = ui.painter().with_clip_rect(text_clip);
                    // o texto começa no início da leitura (à direita em árabe, urdu e persa)
                    let (tx, anchor) = if rtl() { (text_clip.right(), Align2::RIGHT_CENTER) } else { (x, Align2::LEFT_CENTER) };
                    painter.text(
                        egui::pos2(tx, rect.center().y - 9.0),
                        anchor,
                        crate::i18n::tr(tools::file_name(path)),
                        FontId::proportional(14.0),
                        p.text,
                    );
                    let size = tools::human_size(tools::file_size(path));
                    let length = durs.and_then(|d| d.get(path)).map(crate::video::fmt_time);
                    let sub = match (dims, length) {
                        (Some((w, h)), _) => format!("{w}×{h}  •  {size}"),
                        (None, Some(t)) => format!("{t}  •  {size}"),
                        (None, None) => size,
                    };
                    let sub = if q.reorder { format!("{}º  •  {sub}", i + 1) } else { sub };
                    painter.text(
                        egui::pos2(tx, rect.center().y + 10.0),
                        anchor,
                        crate::i18n::tr(sub),
                        FontId::proportional(12.0),
                        p.muted,
                    );

                    let close_resp = ui.interact(close, ui.id().with(("rm", i)), Sense::click());
                    if resp.hovered() || close_resp.hovered() || selected {
                        if close_resp.hovered() {
                            ui.painter().rect_filled(close, 6, p.action);
                        }
                        ui.painter().text(
                            close.center(),
                            Align2::CENTER_CENTER,
                            crate::i18n::tr("×"),
                            FontId::proportional(18.0),
                            if close_resp.hovered() { p.on_accent } else { p.muted },
                        );
                    }
                    if close_resp.on_hover_text(crate::i18n::tr("Remover da lista")).clicked() {
                        remove = Some(i);
                    } else if resp.clicked() {
                        q.selected = i;
                    }
                }
            });
        if let Some(i) = remove {
            q.files.remove(i);
            if q.selected >= i && q.selected > 0 {
                q.selected -= 1;
            }
        }
    });
}

/// Fundo quadriculado, como nos editores de imagem, para mostrar áreas transparentes.
pub fn checkerboard(painter: &egui::Painter, rect: Rect) {
    let cell = 10.0;
    painter.rect_filled(rect, 0, Color32::from_gray(250));
    let (cols, rows) = ((rect.width() / cell).ceil() as i32, (rect.height() / cell).ceil() as i32);
    for y in 0..rows {
        for x in 0..cols {
            if (x + y) % 2 == 0 {
                let min = rect.min + vec2(x as f32 * cell, y as f32 * cell);
                let r = Rect::from_min_max(min, (min + vec2(cell, cell)).min(rect.max));
                painter.rect_filled(r, 0, Color32::from_gray(214));
            }
        }
    }
}

fn dashed_border(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.5f32, color);
    let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    for w in pts.windows(2) {
        painter.add(egui::Shape::dashed_line(&[w[0], w[1]], stroke, 8.0, 6.0));
    }
}

// ------------------------------------------------------- nível de áudio ao vivo

/// Altura relativa (0..1) de um nível 0..1 numa escala de −60 a 0 dB.
fn level_frac(v: f32) -> f32 {
    ((crate::recorder::db(v) + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Forma de onda dos últimos segundos, rolando da direita para a esquerda; blocos que estouraram em cinábrio.
pub fn waveform(ui: &mut Ui, p: &Palette, lv: &crate::recorder::Levels, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 8, p.input);
    painter.rect_stroke(rect, 8, Stroke::new(1.0f32, p.border), StrokeKind::Inside);
    let inner = rect.shrink2(vec2(8.0, 6.0));
    let (mid, half) = (inner.center().y, inner.height() / 2.0);
    // linhas de −6 dB: acima delas o som está perto de estourar
    let guide = level_frac(0.5) * half;
    for y in [mid - guide, mid + guide] {
        painter.add(egui::Shape::dashed_line(&[egui::pos2(inner.left(), y), egui::pos2(inner.right(), y)], Stroke::new(1.0f32, p.border), 4.0, 4.0));
    }
    painter.line_segment([egui::pos2(inner.left(), mid), egui::pos2(inner.right(), mid)], Stroke::new(1.0f32, p.border));
    if lv.blocks.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::tr("A faixa de áudio aparece aqui"), FontId::proportional(13.0), p.muted);
        return;
    }
    let step = 3.0;
    let n = (inner.width() / step) as usize;
    let shown: Vec<&crate::recorder::Block> = lv.blocks.iter().rev().take(n).collect();
    for (i, b) in shown.iter().enumerate() {
        let x = inner.right() - i as f32 * step - 1.0;
        let h = (level_frac(b.peak) * half).max(0.5);
        let color = if b.clipped { p.accent } else { p.muted };
        painter.line_segment([egui::pos2(x, mid - h), egui::pos2(x, mid + h)], Stroke::new(2.0f32, color));
    }
}

/// Medidor de nível (−60 a 0 dB) com marca de pico e valor em dB; fica cinábrio quando estoura.
pub fn level_meter(ui: &mut Ui, p: &Palette, lv: &crate::recorder::Levels) {
    use crate::recorder::db;
    let clipped = lv.clipped_recently(2500);
    let now = lv.recent_peak(100);
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 86.0).max(120.0);
        let (area, _) = ui.allocate_exact_size(vec2(w, 30.0), Sense::hover());
        let bar = Rect::from_min_size(area.min + vec2(0.0, 3.0), vec2(w, 10.0));
        let painter = ui.painter();
        painter.rect_filled(bar, 5, p.input);
        painter.rect_stroke(bar, 5, Stroke::new(1.0f32, p.border), StrokeKind::Inside);
        let d = db(now);
        let color = if clipped || d > -3.0 {
            p.accent
        } else if d < -45.0 {
            p.muted
        } else {
            p.ok
        };
        let fw = bar.width() * level_frac(now);
        if fw > 1.0 {
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(fw, bar.height())), 5, color);
        }
        let hx = bar.left() + bar.width() * level_frac(lv.peak_hold());
        if hx > bar.left() + 2.0 {
            painter.line_segment([egui::pos2(hx, bar.top() - 2.0), egui::pos2(hx, bar.bottom() + 2.0)], Stroke::new(2.0f32, p.text));
        }
        // escala embaixo da barra
        for (v, label) in [(-60.0f32, "-60"), (-40.0, "-40"), (-20.0, "-20"), (-6.0, "-6"), (0.0, "0 dB")] {
            let x = bar.left() + bar.width() * ((v + 60.0) / 60.0);
            let align = if v == -60.0 { Align2::LEFT_TOP } else if v == 0.0 { Align2::RIGHT_TOP } else { Align2::CENTER_TOP };
            painter.text(egui::pos2(x, bar.bottom() + 3.0), align, crate::i18n::tr(label), FontId::proportional(10.5), p.muted);
        }
        let text = if lv.blocks.is_empty() || d <= -90.0 { "—".to_string() } else { format!("{d:.1} dB") };
        ui.allocate_ui_with_layout(vec2(80.0, 30.0), back(Align::Min), |ui| {
            ui.label(RichText::new(crate::i18n::tr(text)).monospace().color(p.text));
        });
    });
}

/// Frase sobre o nível atual: (é problema?, texto).
pub fn level_advice(lv: &crate::recorder::Levels) -> (Option<bool>, &'static str) {
    use crate::recorder::db;
    if lv.blocks.len() < 10 {
        return (None, "Aguardando o som do microfone…");
    }
    let recent = db(lv.recent_peak(2500));
    if recent < -45.0 {
        (Some(true), "⚠ Quase nenhum som: fale mais perto ou aumente o volume do microfone no Windows.")
    } else if recent > -3.0 {
        // alto ou estourado: o vermelho da faixa e do medidor já avisa
        (None, "")
    } else if recent < -24.0 {
        (None, "Um pouco baixo: fale mais perto do microfone.")
    } else {
        (Some(false), "✔ Nível bom.")
    }
}

// ----------------------------------------------------------- pasta de saída

pub struct OutDir {
    pub path: Option<PathBuf>,
    allow_same: bool,
}

impl OutDir {
    pub fn same_as_source() -> Self {
        Self { path: None, allow_same: true }
    }

    pub fn fixed(path: PathBuf) -> Self {
        Self { path: Some(path), allow_same: false }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Salvar em:")).color(p.text));
            let text = match &self.path {
                Some(d) => RichText::new(crate::i18n::tr(d.display().to_string())).color(p.text),
                None => RichText::new(crate::i18n::tr("Mesma pasta do arquivo original")).color(p.muted),
            };
            let right = if self.allow_same { 200.0 } else { 110.0 };
            Frame::default()
                .fill(p.input)
                .stroke(Stroke::new(1.0f32, p.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width((ui.available_width() - right).max(120.0));
                    ui.add(egui::Label::new(text).truncate());
                });
            if secondary(ui, p, "Escolher…", true) {
                if let Some(d) = rfd::FileDialog::new().set_title(crate::i18n::tr("Escolha a pasta de saída")).pick_folder() {
                    self.path = Some(d);
                }
            }
            if self.allow_same && secondary(ui, p, "Original", self.path.is_some()) {
                self.path = None;
            }
        });
    }
}

// ----------------------------------------------------------- barra de ação

pub enum Act {
    None,
    Start,
    Cancel,
}

/// Status, barra de progresso, botões Iniciar/Cancelar, "Abrir pasta" e o log.
pub fn action_bar(ui: &mut Ui, p: &Palette, job: &Job, label: &str, can_start: bool) -> Act {
    let mut act = Act::None;
    let st = job.state();
    let running = st.running;
    ui.add_space(4.0);
    ui.label(RichText::new(crate::i18n::tr(&st.status)).color(p.muted));
    let bar = vec2(ui.available_width(), 40.0);
    ui.allocate_ui_with_layout(bar, back(Align::Center), |ui| {
        if primary(ui, p, label, can_start && !running) {
            act = Act::Start;
        }
        if running && secondary(ui, p, "Cancelar", true) {
            act = Act::Cancel;
        }
        if let (false, Some(folder)) = (running, &st.done_folder) {
            if secondary(ui, p, "Abrir pasta", true) {
                tools::open_folder(folder);
            }
        }
        ui.add(
            ProgressBar::new(st.progress)
                .desired_width(ui.available_width() - 8.0)
                .desired_height(10.0)
                // parada em 0 a barra não desenha a bolinha vermelha do começo
                .fill(if st.progress > 0.0 { p.accent } else { Color32::TRANSPARENT })
                .corner_radius(5),
        );
    });
    if !st.log.is_empty() {
        ui.add_space(6.0);
        Frame::default()
            .fill(p.card)
            .stroke(Stroke::new(1.0f32, p.border))
            .corner_radius(10)
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt(label)
                    .max_height(120.0)
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (kind, line) in &st.log {
                            let color = match kind {
                                LogKind::Ok => p.ok,
                                LogKind::Err => p.accent,
                                LogKind::Info => p.muted,
                            };
                            ui.label(RichText::new(crate::i18n::tr(line)).size(13.0).color(color));
                        }
                    });
            });
    }
    drop(st);
    if running {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    act
}

pub fn paint_drop_overlay(ctx: &egui::Context, p: &Palette) {
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
    let screen = ctx.screen_rect();
    painter.rect_filled(screen, 0, p.bg.gamma_multiply(0.92));
    let r = screen.shrink(24.0);
    painter.rect_stroke(r, 16, Stroke::new(3.0f32, p.accent), StrokeKind::Inside);
    painter.text(r.center() - vec2(0.0, 30.0), Align2::CENTER_CENTER, crate::i18n::tr("⬇"), FontId::proportional(48.0), p.accent);
    painter.text(r.center() + vec2(0.0, 24.0), Align2::CENTER_CENTER, crate::i18n::tr("Solte para adicionar"), FontId::new(24.0, bold()), p.text);
    painter.text(
        r.center() + vec2(0.0, 56.0),
        Align2::CENTER_CENTER,
        crate::i18n::tr("Imagens e vídeos vão para a ferramenta certa"),
        FontId::proportional(14.0),
        p.muted,
    );
}
