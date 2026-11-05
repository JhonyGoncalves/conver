//! Boas-vindas: só na primeira vez que o Conver abre. Pergunta para que a pessoa vai usar o app e sugere os
//! modelos de IA que valem a pena baixar (nada é baixado sem ela escolher). Dá para pular, e tudo continua em
//! Configurações → Modelos de IA.

use std::collections::HashSet;

use eframe::egui::{self, Align2, CursorIcon, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use crate::i18n::tr;
use crate::models::{BUNDLED, Downloads, Pack, Purpose};
use crate::theme::{Palette, bold};
use crate::widgets;

pub enum Outcome {
    Stay,
    /// Terminou (com os pacotes escolhidos para baixar).
    Done(Vec<Pack>),
}

pub struct Welcome {
    step: u8,
    purposes: HashSet<Purpose>,
    packs: HashSet<Pack>,
    /// O que a pessoa desmarcou de propósito (as sugestões não marcam de novo).
    unpicked: HashSet<Pack>,
}

impl Welcome {
    pub fn new() -> Self {
        Self { step: 0, purposes: HashSet::new(), packs: HashSet::new(), unpicked: HashSet::new() }
    }

    /// Para o modo de captura: abre direto num passo, com algumas escolhas.
    pub fn demo(step: u8) -> Self {
        let mut w = Self::new();
        w.step = step;
        for p in [Purpose::Social, Purpose::Audio] {
            w.toggle(p);
        }
        w
    }

    fn toggle(&mut self, p: Purpose) {
        if !self.purposes.remove(&p) {
            self.purposes.insert(p);
        }
        // as sugestões acompanham as finalidades, sem desfazer o que a pessoa mudou à mão
        let suggested: HashSet<Pack> = self.purposes.iter().flat_map(|p| p.suggests().iter().copied()).collect();
        self.packs = suggested.into_iter().filter(|p| !self.unpicked.contains(p)).collect();
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, downloads: &Downloads) -> Outcome {
        let width = ui.available_width().min(780.0);
        let mut out = Outcome::Stay;
        // coluna central
        let margin = ((ui.available_width() - width) / 2.0).max(0.0);
        let col = Rect::from_min_size(ui.cursor().min + vec2(margin, 0.0), vec2(width, ui.available_height()));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(col).layout(widgets::down()), |ui| {
            ui.set_width(width);
            ui.add_space(18.0);
            match self.step {
                0 => self.purposes_ui(ui, p, &mut out),
                _ => self.packs_ui(ui, p, downloads, &mut out),
            }
        });
        out
    }

    fn purposes_ui(&mut self, ui: &mut Ui, p: &Palette, out: &mut Outcome) {
        widgets::header(ui, p, "Boas-vindas ao Conver", "Para que você vai usar o Conver? Marque quantas quiser: com isso o Conver sugere os modelos de IA que valem a pena baixar. Nada muda nas ferramentas, todas continuam no menu.");
        ui.add_space(6.0);
        let gap = 10.0;
        let w = (ui.available_width() - gap) / 2.0;
        for pair in Purpose::ALL.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for purpose in pair {
                    if tile(ui, p, w, tr(purpose.title()), tr(purpose.examples()), self.purposes.contains(purpose)) {
                        self.toggle(*purpose);
                    }
                }
            });
            ui.add_space(gap - ui.spacing().item_spacing.y);
        }
        ui.add_space(10.0);
        let any = !self.purposes.is_empty();
        ui.horizontal(|ui| {
            if widgets::secondary(ui, p, "Pular", true) {
                *out = Outcome::Done(Vec::new());
            }
            ui.with_layout(widgets::back(egui::Align::Center), |ui| {
                if widgets::primary(ui, p, "Continuar", true) {
                    self.step = 1;
                }
                if !any {
                    ui.label(RichText::new(tr("Sem nada marcado, o Conver mostra todos os modelos.")).size(12.5).color(p.muted));
                }
            });
        });
    }

    fn packs_ui(&mut self, ui: &mut Ui, p: &Palette, downloads: &Downloads, out: &mut Outcome) {
        widgets::header(
            ui,
            p,
            "Modelos de IA",
            "Algumas ferramentas usam modelos que não vêm no download do Conver. Baixe agora o que vai usar, ou deixe para a primeira vez que abrir a ferramenta: ela avisa o tamanho antes.",
        );
        // primeiro os sugeridos, depois o resto
        let mut order: Vec<Pack> = Pack::ALL.to_vec();
        if !self.purposes.is_empty() {
            let suggested: HashSet<Pack> = self.purposes.iter().flat_map(|p| p.suggests().iter().copied()).collect();
            order.sort_by_key(|k| !suggested.contains(k));
        }
        widgets::card(ui, p, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (i, pack) in order.iter().enumerate() {
                if i > 0 {
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);
                }
                pack_row(ui, p, *pack, &mut self.packs, &mut self.unpicked, downloads);
            }
        });
        ui.add_space(4.0);
        widgets::hint(ui, p, BUNDLED);
        ui.add_space(10.0);
        let chosen: Vec<Pack> = order.iter().copied().filter(|k| self.packs.contains(k) && !k.installed()).collect();
        let total: u64 = chosen.iter().map(|k| k.size()).sum();
        ui.horizontal(|ui| {
            if widgets::secondary(ui, p, "Voltar", true) {
                self.step = 0;
            }
            ui.with_layout(widgets::back(egui::Align::Center), |ui| {
                if chosen.is_empty() {
                    if widgets::primary(ui, p, "Começar", true) {
                        *out = Outcome::Done(Vec::new());
                    }
                } else {
                    let label = tr(format!("Baixar {} e começar", crate::tools::human_size(total)));
                    if widgets::primary(ui, p, &label, true) {
                        *out = Outcome::Done(chosen.clone());
                    }
                    if widgets::secondary(ui, p, "Começar sem baixar", true) {
                        *out = Outcome::Done(Vec::new());
                    }
                    ui.label(RichText::new(tr("O download continua em segundo plano enquanto você usa o Conver.")).size(12.5).color(p.muted));
                }
            });
        });
    }
}

/// Uma linha de pacote: marcar, nome, onde é usado, tamanho e estado. Usada aqui e em Configurações.
pub fn pack_row(ui: &mut Ui, p: &Palette, pack: Pack, chosen: &mut HashSet<Pack>, unpicked: &mut HashSet<Pack>, downloads: &Downloads) {
    let installed = pack.installed();
    ui.horizontal(|ui| {
        let mut on = installed || chosen.contains(&pack);
        let r = ui.add_enabled(!installed && !downloads.queued(pack), egui::Checkbox::without_text(&mut on));
        if r.changed() {
            if on {
                chosen.insert(pack);
                unpicked.remove(&pack);
            } else {
                chosen.remove(&pack);
                unpicked.insert(pack);
            }
        }
        widgets::vt(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(RichText::new(tr(pack.title())).family(bold()).color(p.text));
            ui.label(RichText::new(tr(pack.used_by())).size(12.5).color(p.muted));
        });
        ui.with_layout(widgets::back(egui::Align::Center), |ui| {
            let status = if installed {
                RichText::new(tr(format!("✔ Baixado • {}", crate::tools::human_size(pack.disk().max(pack.size()))))).color(p.ok)
            } else if downloads.current() == Some(pack) {
                RichText::new(tr(format!("Baixando… {:.0}%", downloads.progress() * 100.0))).color(p.text)
            } else if downloads.queued(pack) {
                RichText::new(tr("Na fila")).color(p.muted)
            } else {
                RichText::new(crate::tools::human_size(pack.size())).color(p.muted)
            };
            ui.label(status);
        });
    });
}

/// Cartão de finalidade que liga e desliga.
fn tile(ui: &mut Ui, p: &Palette, w: f32, title: String, sub: String, on: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 74.0), Sense::click());
    let painter = ui.painter();
    let hovered = resp.hovered();
    painter.rect_filled(rect, 12, if on { p.accent_soft } else { p.card });
    let stroke = if on { Stroke::new(1.5f32, p.accent) } else if hovered { Stroke::new(1.0f32, p.muted) } else { Stroke::new(1.0f32, p.border) };
    painter.rect_stroke(rect, 12, stroke, StrokeKind::Inside);
    // a marca fica no fim da linha; o texto, no começo da leitura
    let rtl = widgets::rtl();
    let mark = widgets::mirror(Rect::from_center_size(egui::pos2(rect.right() - 24.0, rect.top() + 24.0), vec2(18.0, 18.0)), rect);
    painter.rect_filled(mark, 5, if on { p.action } else { p.input });
    painter.rect_stroke(mark, 5, Stroke::new(1.0f32, if on { p.action } else { p.border }), StrokeKind::Inside);
    if on {
        let c = mark.center();
        painter.line_segment([c + vec2(-4.5, 0.0), c + vec2(-1.5, 3.5)], Stroke::new(2.0f32, p.on_accent));
        painter.line_segment([c + vec2(-1.5, 3.5), c + vec2(4.5, -3.5)], Stroke::new(2.0f32, p.on_accent));
    }
    let (x, anchor) = if rtl { (rect.right() - 16.0, Align2::RIGHT_CENTER) } else { (rect.left() + 16.0, Align2::LEFT_CENTER) };
    let clip = Rect::from_min_max(rect.min + vec2(if rtl { 44.0 } else { 0.0 }, 0.0), rect.max - vec2(if rtl { 0.0 } else { 44.0 }, 0.0));
    let painter = painter.with_clip_rect(clip);
    painter.text(egui::pos2(x, rect.top() + 25.0), anchor, title, FontId::new(15.0, bold()), p.text);
    painter.text(egui::pos2(x, rect.top() + 50.0), anchor, sub, FontId::proportional(12.5), p.muted);
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}
