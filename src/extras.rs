//! Telas extras: renomear em lote, QR Code e histórico.

use std::path::PathBuf;

use eframe::egui::{
    self, Align, Color32, ColorImage, Frame, Margin, RichText, Stroke, TextureHandle, TextureOptions, Ui, vec2,
};

use crate::history;
use crate::qr::{self, Correction};
use crate::rename::{self, Case, Rules};
use crate::theme::{CHARCOAL, CINNABAR, Palette, bold};
use crate::tools;
use crate::widgets::{self, FileQueue, card, hint, row, segmented};

// ======================================================== renomear em lote

pub struct RenamePage {
    pub queue: FileQueue,
    rules: Rules,
    undo: Vec<(PathBuf, PathBuf)>,
    msg: Option<(bool, String)>,
}

impl RenamePage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(&[], "Arquivos"), rules: Rules::default(), undo: Vec::new(), msg: None }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(
            ui,
            p,
            "Renomear em lote",
            "Renomeie vários arquivos de uma vez. Confira os novos nomes antes de aplicar; dá para desfazer.",
        );
        let names: Vec<String> =
            self.queue.files.iter().enumerate().map(|(i, f)| rename::new_name(f, i, &self.rules)).collect();
        let problems = rename::problems(&self.queue.files, &names);

        let height = 260.0;
        ui.horizontal_top(|ui| {
            let left = (ui.available_width() * 0.42).max(300.0);
            ui.allocate_ui_with_layout(vec2(left, height + 80.0), crate::widgets::down(), |ui| {
                widgets::file_list(ui, p, &mut self.queue, None, None, height);
            });
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height + 80.0), crate::widgets::down(), |ui| {
                card(ui, p, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("Novos nomes")).family(bold()).color(p.text));
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_salt("rename_preview")
                        .max_height(height)
                        .min_scrolled_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.queue.files.is_empty() {
                                ui.label(RichText::new(crate::i18n::tr("Adicione arquivos para ver os novos nomes.")).color(p.muted));
                            }
                            for ((f, n), prob) in self.queue.files.iter().zip(&names).zip(&problems) {
                                let old = tools::file_name(f);
                                ui.add(egui::Label::new(RichText::new(crate::i18n::tr(&old)).size(12.5).color(p.muted)).truncate());
                                let color = if prob.is_some() {
                                    p.accent
                                } else if *n == old {
                                    p.muted
                                } else {
                                    p.text
                                };
                                let text = match prob {
                                    Some(why) => format!("→ {n}   ({why})"),
                                    None => format!("→ {n}"),
                                };
                                ui.add(egui::Label::new(RichText::new(crate::i18n::tr(text)).color(color)).truncate());
                                ui.add_space(4.0);
                            }
                        });
                });
            });
        });
        ui.add_space(4.0);

        let r = &mut self.rules;
        card(ui, p, |ui| {
            row(ui, p, "Nome base:", |ui| {
                ui.add(egui::TextEdit::singleline(&mut r.base).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("vazio = manter o nome atual")).desired_width(240.0));
            });
            row(ui, p, "Trocar texto:", |ui| {
                ui.add(egui::TextEdit::singleline(&mut r.find).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("procurar")).desired_width(150.0));
                ui.label(crate::i18n::tr("por"));
                ui.add(egui::TextEdit::singleline(&mut r.replace).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("substituir")).desired_width(150.0));
            });
            row(ui, p, "Adicionar:", |ui| {
                ui.add(egui::TextEdit::singleline(&mut r.prefix).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("antes (prefixo)")).desired_width(150.0));
                ui.add(egui::TextEdit::singleline(&mut r.suffix).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("depois (sufixo)")).desired_width(150.0));
            });
            row(ui, p, "Numeração:", |ui| {
                crate::widgets::check(ui, &mut r.numbering, crate::i18n::tr("Numerar"));
                ui.add_enabled_ui(r.numbering, |ui| {
                    ui.label(crate::i18n::tr("começando em"));
                    ui.add(egui::DragValue::new(&mut r.start).range(0..=999_999));
                    ui.label(crate::i18n::tr("dígitos"));
                    ui.add(egui::DragValue::new(&mut r.digits).range(1..=6));
                    ui.label(crate::i18n::tr("separador"));
                    ui.add(egui::TextEdit::singleline(&mut r.sep).margin(crate::widgets::FIELD_MARGIN).desired_width(40.0));
                });
            });
            row(ui, p, "Letras:", |ui| {
                segmented(ui, p, &mut r.case, &[(Case::Manter, "Manter"), (Case::Minusculas, "minúsculas"), (Case::Maiusculas, "MAIÚSCULAS")]);
            });
            hint(ui, p, "A extensão (.jpg, .mp4…) é sempre mantida. Os arquivos são renomeados na mesma pasta.");
        });
        ui.add_space(4.0);

        let changed = names.iter().zip(&self.queue.files).filter(|(n, f)| **n != tools::file_name(f)).count();
        let ok = !self.queue.files.is_empty() && changed > 0 && problems.iter().all(Option::is_none);
        ui.horizontal(|ui| {
            if let Some((good, text)) = &self.msg {
                ui.label(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent }));
            } else if changed > 0 {
                ui.label(RichText::new(crate::i18n::tr(format!("{changed} arquivo(s) serão renomeados."))).color(p.muted));
            }
            ui.with_layout(crate::widgets::back(Align::Center), |ui| {
                if widgets::primary(ui, p, "Renomear", ok) {
                    match rename::apply(&self.queue.files, &names) {
                        Ok(undo) => {
                            self.msg = Some((true, format!("✔ {} arquivo(s) renomeados.", undo.len())));
                            for (new, old) in &undo {
                                if let Some(f) = self.queue.files.iter_mut().find(|f| *f == old) {
                                    *f = new.clone();
                                }
                            }
                            self.undo = undo;
                            self.rules = Rules::default();
                        }
                        Err(e) => self.msg = Some((false, format!("✖ {e}"))),
                    }
                }
                if !self.undo.is_empty() && widgets::secondary(ui, p, "Desfazer", true) {
                    let files: Vec<PathBuf> = self.undo.iter().map(|(new, _)| new.clone()).collect();
                    let olds: Vec<String> = self.undo.iter().map(|(_, old)| tools::file_name(old)).collect();
                    match rename::apply(&files, &olds) {
                        Ok(back) => {
                            for (restored, renamed) in &back {
                                if let Some(f) = self.queue.files.iter_mut().find(|f| *f == renamed) {
                                    *f = restored.clone();
                                }
                            }
                            self.msg = Some((true, "Nomes originais restaurados.".into()));
                            self.undo.clear();
                        }
                        Err(e) => self.msg = Some((false, format!("✖ {e}"))),
                    }
                }
            });
        });
    }
}

// ================================================================ QR Code

const QR_COLORS: [(&str, [u8; 3]); 3] =
    [("Preto", [0, 0, 0]), ("Charcoal", [CHARCOAL.r(), CHARCOAL.g(), CHARCOAL.b()]), ("Cinnabar", [CINNABAR.r(), CINNABAR.g(), CINNABAR.b()])];

pub struct QrPage {
    text: String,
    size: u32,
    color: usize,
    corr: Correction,
    key: Option<(String, u32, usize, Correction)>,
    current: Option<Result<(image::RgbaImage, TextureHandle), String>>,
    msg: Option<(bool, String)>,
}

impl QrPage {
    pub fn new() -> Self {
        Self { text: String::new(), size: 600, color: 0, corr: Correction::Media, key: None, current: None, msg: None }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(ui, p, "Gerador de QR Code", "Crie um QR Code de um link, texto, Wi-Fi ou contato e salve como imagem PNG.");

        let key = (self.text.clone(), self.size, self.color, self.corr);
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.msg = None;
            self.current = (!self.text.trim().is_empty()).then(|| {
                qr::render(&self.text, self.size, QR_COLORS[self.color].1, self.corr).map(|img| {
                    let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    let tex = ui.ctx().load_texture("qr", ci, TextureOptions::NEAREST);
                    (img, tex)
                })
            });
        }

        ui.horizontal_top(|ui| {
            let left = (ui.available_width() * 0.55).max(320.0);
            ui.allocate_ui_with_layout(vec2(left, 420.0), crate::widgets::down(), |ui| {
                card(ui, p, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("Conteúdo")).family(bold()).color(p.text));
                    ui.add(
                        egui::TextEdit::multiline(&mut self.text)
                            .hint_text(crate::i18n::tr("https://… ou qualquer texto"))
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if widgets::secondary(ui, p, "Colar", true) {
                            if let Ok(t) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                                self.text = t;
                            }
                        }
                        if widgets::secondary(ui, p, "Modelo de Wi-Fi", true) {
                            self.text = "WIFI:T:WPA;S:NomeDaRede;P:SenhaDaRede;;".into();
                        }
                    });
                    hint(ui, p, "No modelo de Wi-Fi, troque NomeDaRede e SenhaDaRede pelos dados da sua rede.");
                    ui.add_space(8.0);
                    row(ui, p, "Tamanho:", |ui| {
                        ui.add(egui::Slider::new(&mut self.size, 150..=2000).suffix(" px"));
                    });
                    row(ui, p, "Cor:", |ui| {
                        let opts: Vec<(usize, &str)> = QR_COLORS.iter().enumerate().map(|(i, (n, _))| (i, *n)).collect();
                        segmented(ui, p, &mut self.color, &opts);
                    });
                    row(ui, p, "Correção de erro:", |ui| {
                        segmented(
                            ui,
                            p,
                            &mut self.corr,
                            &[(Correction::Baixa, "Baixa"), (Correction::Media, "Média"), (Correction::Alta, "Alta")],
                        );
                    });
                    hint(ui, p, "Correção alta deixa o código mais denso, mas ele continua lendo mesmo se estiver sujo ou amassado.");
                });
            });
            ui.allocate_ui_with_layout(vec2(ui.available_width(), 420.0), crate::widgets::down(), |ui| {
                card(ui, p, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("Prévia")).family(bold()).color(p.text));
                    let side = (ui.available_width() - 20.0).min(300.0);
                    ui.vertical_centered(|ui| {
                        Frame::default().fill(Color32::WHITE).corner_radius(8).inner_margin(Margin::same(6)).show(ui, |ui| {
                            match &self.current {
                                Some(Ok((_, tex))) => {
                                    ui.add(egui::Image::new((tex.id(), vec2(side, side))));
                                }
                                Some(Err(e)) => {
                                    ui.allocate_ui(vec2(side, side), |ui| {
                                        ui.centered_and_justified(|ui| ui.label(RichText::new(crate::i18n::tr(e)).color(p.accent)));
                                    });
                                }
                                None => {
                                    ui.allocate_ui(vec2(side, side), |ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label(RichText::new(crate::i18n::tr("Digite algo para gerar")).color(Color32::GRAY))
                                        });
                                    });
                                }
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let img = match &self.current {
                        Some(Ok((img, _))) => Some(img.clone()),
                        _ => None,
                    };
                    ui.horizontal(|ui| {
                        if widgets::primary(ui, p, "Salvar PNG…", img.is_some()) {
                            if let (Some(img), Some(dst)) = (
                                &img,
                                rfd::FileDialog::new().set_file_name("qrcode.png").add_filter(crate::i18n::tr("PNG"), &["png"]).save_file(),
                            ) {
                                let dst = if tools::ext_lower(&dst) == "png" { dst } else { dst.with_extension("png") };
                                self.msg = Some(match img.save(&dst) {
                                    Ok(()) => {
                                        let text: String = self.text.chars().take(60).collect();
                                        history::record("QR Code", &text, &dst, &format!("{}×{}", img.width(), img.height()));
                                        (true, format!("✔ Salvo em {}", dst.display()))
                                    }
                                    Err(e) => (false, format!("✖ {e}")),
                                });
                            }
                        }
                        if widgets::secondary(ui, p, "Copiar imagem", img.is_some()) {
                            if let Some(img) = &img {
                                let data = arboard::ImageData {
                                    width: img.width() as usize,
                                    height: img.height() as usize,
                                    bytes: std::borrow::Cow::Borrowed(img.as_raw()),
                                };
                                self.msg = Some(match arboard::Clipboard::new().and_then(|mut c| c.set_image(data)) {
                                    Ok(()) => (true, "✔ Copiado; cole com Ctrl+V.".into()),
                                    Err(e) => (false, format!("✖ {e}")),
                                });
                            }
                        }
                    });
                    if let Some((good, text)) = &self.msg {
                        ui.add(egui::Label::new(RichText::new(crate::i18n::tr(text)).color(if *good { p.ok } else { p.accent })).wrap());
                    }
                });
            });
        });
    }
}

// ================================================================ histórico

pub struct HistoryPage {
    confirm_clear: bool,
}

impl HistoryPage {
    pub fn new() -> Self {
        Self { confirm_clear: false }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let entries = history::entries();
        ui.horizontal(|ui| {
            crate::widgets::vt(ui, |ui| widgets::header(ui, p, "Histórico", "As últimas conversões e downloads feitos no Conver."));
            ui.with_layout(crate::widgets::back(Align::Min), |ui| {
                if self.confirm_clear {
                    if widgets::primary(ui, p, "Sim, limpar", true) {
                        history::clear();
                        self.confirm_clear = false;
                    }
                    if widgets::secondary(ui, p, "Cancelar", true) {
                        self.confirm_clear = false;
                    }
                } else if widgets::secondary(ui, p, "Limpar histórico", !entries.is_empty()) {
                    self.confirm_clear = true;
                }
            });
        });
        if entries.is_empty() {
            card(ui, p, |ui| {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Nada por aqui ainda")).family(bold()).size(16.0).color(p.text));
                    ui.label(RichText::new(crate::i18n::tr("Os arquivos que você converter ou baixar aparecem nesta lista.")).color(p.muted));
                });
                ui.add_space(30.0);
            });
            return;
        }
        for e in entries.iter().take(150) {
            Frame::default()
                .fill(p.card)
                .stroke(Stroke::new(1.0f32, p.border))
                .corner_radius(10)
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        crate::widgets::vt(ui, |ui| {
                            ui.set_width((ui.available_width() - 210.0).max(200.0));
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(crate::i18n::tr(&e.tool)).family(bold()).color(p.text));
                                ui.label(RichText::new(crate::i18n::tr(&e.time)).size(12.5).color(p.muted));
                            });
                            let out = tools::file_name(&e.output);
                            ui.add(egui::Label::new(RichText::new(crate::i18n::tr(format!("{} → {out}", e.input))).color(p.text)).truncate());
                            if !e.info.is_empty() {
                                ui.add(egui::Label::new(RichText::new(crate::i18n::tr(&e.info)).size(12.5).color(p.muted)).truncate());
                            }
                        });
                        ui.with_layout(crate::widgets::back(Align::Center), |ui| {
                            let exists = e.output.exists();
                            if !exists {
                                ui.label(RichText::new(crate::i18n::tr("arquivo não existe mais")).size(12.5).color(p.muted));
                            } else {
                                if widgets::secondary(ui, p, "Mostrar na pasta", true) {
                                    tools::show_in_folder(&e.output);
                                }
                                if widgets::secondary(ui, p, "Abrir", e.output.is_file()) {
                                    tools::open_file(&e.output);
                                }
                            }
                        });
                    });
                });
            ui.add_space(2.0);
        }
    }
}
