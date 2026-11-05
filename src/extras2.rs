//! Telas de PDF (juntar e dividir) e da pasta vigiada.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText, Ui, vec2};

use crate::imaging::{Fmt, Level};
use crate::job::Job;
use crate::media::AFmt;
use crate::pages::{run_batch, run_single};
use crate::theme::Palette;
use crate::video::{VFmt, VLevel};
use crate::watch::{self, Action, Options};
use crate::widgets::{self, Act, FileQueue, OutDir, card, hint, row, segmented};
use crate::{pdf, pdf_extra, tools};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfTool {
    Merge,
    Split,
    Compress,
    ToImages,
    Protect,
    Unlock,
    Ocr,
    Organize,
}

impl PdfTool {
    pub const ALL: [PdfTool; 8] =
        [PdfTool::Merge, PdfTool::Split, PdfTool::Compress, PdfTool::ToImages, PdfTool::Protect, PdfTool::Unlock, PdfTool::Ocr, PdfTool::Organize];
}

/// Organizar páginas: a ordem atual (página original, rotação extra) e as miniaturas.
#[derive(Default)]
struct Organizer {
    file: Option<PathBuf>,
    order: Vec<(u32, i64)>,
    /// Miniaturas prontas (feitas em segundo plano), ainda não enviadas para a tela.
    ready: Arc<Mutex<Vec<(u32, egui::ColorImage)>>>,
    tex: HashMap<u32, egui::TextureHandle>,
    stop: Arc<AtomicBool>,
}

pub struct PdfPage {
    tool: PdfTool,
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    name: String,
    each_page: bool,
    ranges: String,
    pages: HashMap<PathBuf, Option<usize>>,
    level: Level,
    img_png: bool,
    dpi: u32,
    password: String,
    password2: String,
    owner: String,
    allow_print: bool,
    allow_copy: bool,
    ocr_langs: Option<Vec<(String, String)>>,
    ocr_lang: usize,
    skip_text: bool,
    org: Organizer,
}

impl PdfPage {
    pub fn new(tool: PdfTool) -> Self {
        let q = FileQueue::new(&["pdf"], "PDF");
        Self {
            tool,
            queue: if tool == PdfTool::Merge { q.ordered() } else { q },
            out: OutDir::same_as_source(),
            job: Job::default(),
            name: "documento_juntado".into(),
            each_page: false,
            ranges: "1-3".into(),
            pages: HashMap::new(),
            level: Level::Media,
            img_png: true,
            dpi: 150,
            password: String::new(),
            password2: String::new(),
            owner: String::new(),
            allow_print: true,
            allow_copy: false,
            ocr_langs: None,
            ocr_lang: 0,
            skip_text: true,
            org: Organizer::default(),
        }
    }

    /// Grade de páginas: arrastar para mudar a ordem, girar e apagar.
    fn organizer(&mut self, ui: &mut Ui, p: &Palette, file: Option<PathBuf>, pages: Option<usize>) {
        let (Some(file), Some(total)) = (file, pages) else {
            hint(ui, p, "Adicione um PDF para ver as páginas.");
            return;
        };
        let o = &mut self.org;
        if o.file.as_ref() != Some(&file) {
            o.stop.store(true, Ordering::Relaxed);
            *o = Organizer { file: Some(file.clone()), order: (1..=total as u32).map(|n| (n, 0)).collect(), ..Default::default() };
            let (ready, stop, ctx) = (o.ready.clone(), o.stop.clone(), ui.ctx().clone());
            std::thread::spawn(move || {
                let _ = pdf_extra::thumbnails(&file, 28.0, |n, png| {
                    if let Ok(img) = image::load_from_memory(&png) {
                        let rgba = img.to_rgba8();
                        let ci = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
                        ready.lock().unwrap().push((n, ci));
                        ctx.request_repaint();
                    }
                    !stop.load(Ordering::Relaxed)
                });
            });
        }
        for (n, img) in o.ready.lock().unwrap().drain(..) {
            o.tex.insert(n, ui.ctx().load_texture(format!("pdf_org_{n}"), img, egui::TextureOptions::LINEAR));
        }
        ui.horizontal(|ui| {
            hint(ui, p, "Arraste as páginas para mudar a ordem. ⟲ ⟳ giram e × apaga.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::secondary(ui, p, "Desfazer tudo", true) {
                    o.order = (1..=total as u32).map(|n| (n, 0)).collect();
                }
                if widgets::secondary(ui, p, "Girar todas ⟳", !o.order.is_empty()) {
                    for x in o.order.iter_mut() {
                        x.1 = (x.1 + 90).rem_euclid(360);
                    }
                }
            });
        });
        ui.add_space(4.0);
        let mut moved: Option<(usize, usize)> = None;
        let mut remove = None;
        let mut rotate: Option<(usize, i64)> = None;
        egui::ScrollArea::vertical().id_salt("pdf_org").max_height(430.0).auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for (i, (page, rot)) in o.order.iter().enumerate() {
                    let frame = egui::Frame::default().fill(p.input).stroke(egui::Stroke::new(1.0f32, p.border)).corner_radius(8).inner_margin(egui::Margin::same(6));
                    let card = frame.show(ui, |ui| {
                        ui.set_width(112.0);
                        ui.vertical_centered(|ui| {
                            ui.dnd_drag_source(egui::Id::new(("pdf_org_drag", i)), i, |ui| {
                                let (rect, _) = ui.allocate_exact_size(vec2(106.0, 138.0), egui::Sense::hover());
                                match o.tex.get(page) {
                                    Some(tex) => {
                                        let s = tex.size_vec2();
                                        let turned = rot % 180 != 0;
                                        let shown = widgets::fit(if turned { (s.y, s.x) } else { (s.x, s.y) }, rect.size() - vec2(4.0, 4.0));
                                        let unrotated = if turned { vec2(shown.y, shown.x) } else { shown };
                                        let r = egui::Rect::from_center_size(rect.center(), unrotated);
                                        egui::Image::new(tex).rotate((*rot as f32).to_radians(), vec2(0.5, 0.5)).paint_at(ui, r);
                                    }
                                    None => {
                                        ui.painter().rect_filled(rect.shrink(8.0), 4, p.card);
                                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "…", egui::FontId::proportional(18.0), p.muted);
                                    }
                                }
                                ui.label(RichText::new(crate::i18n::tr(format!("{}  •  pág. {page}", i + 1))).size(12.0).color(p.text));
                            });
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                ui.add_space(6.0);
                                if ui.small_button("⟲").on_hover_text(crate::i18n::tr("Girar para a esquerda")).clicked() {
                                    rotate = Some((i, -90));
                                }
                                if ui.small_button("⟳").on_hover_text(crate::i18n::tr("Girar para a direita")).clicked() {
                                    rotate = Some((i, 90));
                                }
                                if ui.small_button("×").on_hover_text(crate::i18n::tr("Apagar a página")).clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    });
                    // soltar uma página em cima de outra: ela vai para essa posição
                    if let Some(from) = card.response.dnd_release_payload::<usize>() {
                        moved = Some((*from, i));
                    }
                    if card.response.dnd_hover_payload::<usize>().is_some_and(|f| *f != i) {
                        ui.painter().rect_stroke(card.response.rect, 8, egui::Stroke::new(2.0f32, p.accent), egui::StrokeKind::Outside);
                    }
                }
            });
        });
        if let Some((from, to)) = moved {
            if from != to && from < o.order.len() {
                let item = o.order.remove(from);
                o.order.insert(to.min(o.order.len()), item);
            }
        }
        if let Some((i, d)) = rotate {
            o.order[i].1 = (o.order[i].1 + d).rem_euclid(360);
        }
        if let Some(i) = remove {
            o.order.remove(i);
        }
        let removed = total.saturating_sub(o.order.len());
        hint(ui, p, &format!("{} página(s) no PDF novo{}. O original não é alterado.", o.order.len(), if removed > 0 { format!(", {removed} apagada(s)") } else { String::new() }));
    }

    fn count(&mut self, f: &PathBuf) -> Option<usize> {
        *self.pages.entry(f.clone()).or_insert_with(|| pdf::page_count(f))
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let (title, subtitle, action) = match self.tool {
            PdfTool::Merge => ("Juntar PDFs", "Una vários PDFs num só, na ordem da lista.", "Juntar"),
            PdfTool::Split => (
                "Dividir PDF",
                "Extraia páginas específicas (ex.: 1-3, 7) ou separe cada página num PDF próprio.",
                "Dividir",
            ),
            PdfTool::Compress => (
                "Comprimir PDF",
                "Deixe PDFs menores recomprimindo as imagens de dentro deles (fotos, páginas escaneadas).",
                "Comprimir",
            ),
            PdfTool::ToImages => ("PDF para imagens", "Salve as páginas do PDF como imagens PNG ou JPG.", "Converter"),
            PdfTool::Protect => (
                "Proteger PDF com senha",
                "Coloque uma senha para abrir o PDF e escolha se ele pode ser impresso ou copiado.",
                "Proteger",
            ),
            PdfTool::Unlock => ("Tirar senha do PDF", "Remova a senha de um PDF seu (é preciso saber a senha atual).", "Tirar senha"),
            PdfTool::Ocr => (
                "PDF pesquisável (OCR)",
                "Reconhece o texto de PDFs escaneados com o OCR do Windows: depois dá para pesquisar, selecionar e copiar.",
                "Reconhecer texto",
            ),
            PdfTool::Organize => ("Organizar páginas", "Mude a ordem, gire e apague páginas de um PDF vendo as miniaturas.", "Salvar PDF"),
        };
        widgets::header(ui, p, title, subtitle);
        widgets::file_list(ui, p, &mut self.queue, None, None, 190.0);
        ui.add_space(4.0);
        let selected = self.queue.selected_path().cloned();
        let selected_pages = selected.as_ref().and_then(|f| self.count(f));
        let total_pages: usize = self.queue.files.clone().iter().filter_map(|f| self.count(f)).sum();
        card(ui, p, |ui| match self.tool {
            PdfTool::Organize => self.organizer(ui, p, selected.clone(), selected_pages),
            PdfTool::Merge => {
                row(ui, p, "Nome do PDF:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.name).margin(crate::widgets::FIELD_MARGIN).desired_width(240.0));
                    ui.label(RichText::new(crate::i18n::tr(".pdf")).color(p.muted));
                });
                if !self.queue.files.is_empty() {
                    hint(ui, p, &format!("O PDF final terá {total_pages} páginas. Use ↑ e ↓ para mudar a ordem."));
                }
            }
            PdfTool::Split => {
                row(ui, p, "Modo:", |ui| {
                    segmented(ui, p, &mut self.each_page, &[(false, "Extrair páginas"), (true, "Uma página por arquivo")]);
                });
                if !self.each_page {
                    row(ui, p, "Páginas:", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.ranges).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("1-3, 7, 10-")).desired_width(200.0));
                        hint(ui, p, "\"10-\" = da 10 até o fim");
                    });
                    if let Some(n) = selected_pages {
                        if let Err(e) = pdf::parse_ranges(&self.ranges, n as u32) {
                            ui.label(RichText::new(crate::i18n::tr(e)).color(p.accent));
                        }
                    }
                } else {
                    hint(ui, p, "As páginas são salvas numa pasta <nome do PDF>_paginas.");
                }
                if let Some(n) = selected_pages {
                    hint(ui, p, &format!("O PDF selecionado tem {n} páginas."));
                }
            }
            PdfTool::Compress => {
                row(ui, p, "Nível:", |ui| {
                    segmented(ui, p, &mut self.level, &[(Level::Leve, "Leve"), (Level::Media, "Média"), (Level::Forte, "Forte")]);
                });
                hint(ui, p, "Leve = quase sem perda  •  Forte = imagens com até 1200 px, bem menores. O texto não perde qualidade.");
            }
            PdfTool::ToImages => {
                row(ui, p, "Formato:", |ui| {
                    segmented(ui, p, &mut self.img_png, &[(true, "PNG"), (false, "JPG")]);
                });
                row(ui, p, "Resolução:", |ui| {
                    segmented(ui, p, &mut self.dpi, &[(96, "96 dpi (tela)"), (150, "150 dpi"), (300, "300 dpi (impressão)")]);
                });
                row(ui, p, "Páginas:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.ranges).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("todas")).desired_width(200.0));
                    hint(ui, p, "vazio = todas  •  ex.: 1-3, 7");
                });
                row(ui, p, "Senha:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.password).margin(crate::widgets::FIELD_MARGIN).password(true).hint_text(crate::i18n::tr("só se o PDF tiver")).desired_width(200.0));
                });
                hint(ui, p, "As imagens são salvas numa pasta <nome do PDF>_imagens.");
            }
            PdfTool::Protect => {
                row(ui, p, "Senha:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.password).margin(crate::widgets::FIELD_MARGIN).password(true).desired_width(200.0));
                });
                row(ui, p, "Repita a senha:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.password2).margin(crate::widgets::FIELD_MARGIN).password(true).desired_width(200.0));
                    if !self.password2.is_empty() && self.password != self.password2 {
                        ui.label(RichText::new(crate::i18n::tr("as senhas não conferem")).color(p.accent));
                    }
                });
                row(ui, p, "Permitir:", |ui| {
                    ui.checkbox(&mut self.allow_print, crate::i18n::tr("Imprimir"));
                    ui.add_space(8.0);
                    ui.checkbox(&mut self.allow_copy, crate::i18n::tr("Copiar o texto"));
                });
                row(ui, p, "Senha do dono:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.owner).margin(crate::widgets::FIELD_MARGIN).password(true).hint_text(crate::i18n::tr("opcional")).desired_width(200.0));
                    hint(ui, p, "libera tudo para quem souber");
                });
                hint(ui, p, "Guarde a senha: sem ela não há como abrir o PDF.");
            }
            PdfTool::Unlock => {
                row(ui, p, "Senha atual:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.password).margin(crate::widgets::FIELD_MARGIN).password(true).desired_width(200.0));
                });
                hint(ui, p, "Use só em PDFs seus ou que você tem autorização para alterar.");
            }
            PdfTool::Ocr => {
                if self.ocr_langs.is_none() {
                    self.ocr_langs = Some(crate::ocr::languages());
                }
                let langs = self.ocr_langs.clone().unwrap_or_default();
                row(ui, p, "Idioma do texto:", |ui| {
                    let auto = crate::i18n::tr("Automático (idiomas do Windows)");
                    let current = if self.ocr_lang == 0 { auto.clone() } else { langs.get(self.ocr_lang - 1).map(|l| l.1.clone()).unwrap_or_default() };
                    egui::ComboBox::from_id_salt("pdf_ocr_lang").selected_text(current).width(260.0).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.ocr_lang, 0, auto);
                        for (i, (_, name)) in langs.iter().enumerate() {
                            ui.selectable_value(&mut self.ocr_lang, i + 1, name);
                        }
                    });
                });
                row(ui, p, "Páginas com texto:", |ui| {
                    segmented(ui, p, &mut self.skip_text, &[(true, "Pular"), (false, "Reconhecer de novo")]);
                });
                hint(ui, p, "A aparência do PDF não muda: o texto reconhecido fica numa camada invisível por cima das páginas.");
            }
        });
        if selected.is_some() && selected_pages.is_none() {
            ui.label(RichText::new(crate::i18n::tr("Não foi possível ler o PDF selecionado (pode estar danificado ou protegido).")).color(p.accent));
        }
        ui.add_space(4.0);
        self.out.ui(ui, p);

        let has = !self.queue.files.is_empty();
        let can = match self.tool {
            PdfTool::Merge => self.queue.files.len() >= 2,
            PdfTool::Split => has && (self.each_page || !self.ranges.trim().is_empty()),
            PdfTool::Protect => has && !self.password.is_empty() && self.password == self.password2,
            PdfTool::Unlock => has && !self.password.is_empty(),
            PdfTool::Organize => selected.is_some() && !self.org.order.is_empty() && self.org.file == selected,
            _ => has,
        };
        match widgets::action_bar(ui, p, &self.job, action, can) {
            Act::Start => {
                let files = self.queue.files.clone();
                let out = self.out.path.clone();
                match self.tool {
                    PdfTool::Merge => {
                        let name: String = self.name.trim().chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect();
                        let name = if name.is_empty() { "documento_juntado".to_string() } else { name };
                        let folder = out
                            .or_else(|| files.first().and_then(|f| f.parent()).map(|p| p.to_path_buf()))
                            .unwrap_or_default();
                        self.job.start(ui.ctx(), move |ctl| {
                            run_single(ctl, title, &format!("{} PDFs", files.len()), || {
                                let dst = tools::unique_path(folder.join(format!("{name}.pdf")));
                                let n = pdf::merge(&files, &dst, &|| ctl.cancelled())?;
                                Ok((format!("{n} páginas • {}", tools::human_size(tools::file_size(&dst))), dst))
                            });
                        });
                    }
                    PdfTool::Split => {
                        let ranges = (!self.each_page).then(|| self.ranges.clone());
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &files, |src, progress| {
                                pdf::split(src, out.as_deref(), ranges.as_deref(), progress, &|| ctl.cancelled())
                            });
                        });
                    }
                    PdfTool::Compress => {
                        let level = self.level;
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &files, |src, progress| pdf_extra::compress(src, out.as_deref(), level, progress, &|| ctl.cancelled()));
                        });
                    }
                    PdfTool::ToImages => {
                        let (fmt, dpi) = (if self.img_png { pdf_extra::PageFmt::Png } else { pdf_extra::PageFmt::Jpg }, self.dpi);
                        let (ranges, pw) = (self.ranges.trim().to_string(), self.password.clone());
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &files, |src, progress| {
                                let r = (!ranges.is_empty()).then_some(ranges.as_str());
                                pdf_extra::to_images(src, out.as_deref(), fmt, dpi, r, &pw, progress, &|| ctl.cancelled())
                            });
                        });
                    }
                    PdfTool::Protect => {
                        let (pw, owner, print, copy) = (self.password.clone(), self.owner.clone(), self.allow_print, self.allow_copy);
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &files, |src, _| pdf_extra::protect(src, out.as_deref(), &pw, &owner, print, copy));
                        });
                    }
                    PdfTool::Unlock => {
                        let pw = self.password.clone();
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &files, |src, _| pdf_extra::unlock(src, out.as_deref(), &pw));
                        });
                    }
                    PdfTool::Organize => {
                        let (src, order) = (self.org.file.clone().unwrap_or_default(), self.org.order.clone());
                        self.job.start(ui.ctx(), move |ctl| {
                            run_single(ctl, title, &tools::file_name(&src), || pdf::organize(&src, out.as_deref(), &order));
                        });
                    }
                    PdfTool::Ocr => {
                        let lang = if self.ocr_lang == 0 {
                            String::new()
                        } else {
                            self.ocr_langs.as_ref().and_then(|l| l.get(self.ocr_lang - 1)).map(|l| l.0.clone()).unwrap_or_default()
                        };
                        let skip = self.skip_text;
                        self.job.start(ui.ctx(), move |ctl| {
                            run_batch(ctl, title, &files, |src, progress| {
                                pdf_extra::make_searchable(src, out.as_deref(), &lang, skip, progress, &|| ctl.cancelled())
                            });
                        });
                    }
                }
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ============================================================ pasta vigiada

pub struct WatchPage {
    folder: Option<PathBuf>,
    options: Options,
    existing: bool,
    job: Job,
}

impl WatchPage {
    pub fn new() -> Self {
        Self {
            folder: None,
            options: Options {
                action: Action::CompressImages,
                img_level: Level::Media,
                img_fmt: Fmt::Jpg,
                vid_level: VLevel::Media,
                vid_fmt: VFmt::Mp4,
                aud_fmt: AFmt::Mp3,
            },
            existing: false,
            job: Job::default(),
        }
    }

    pub fn running(&self) -> bool {
        self.job.state().running
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(
            ui,
            p,
            "Pasta vigiada",
            "Escolha uma pasta: tudo que cair nela é convertido automaticamente enquanto o Conver estiver aberto.",
        );
        let running = self.running();
        card(ui, p, |ui| {
            ui.add_enabled_ui(!running, |ui| {
                row(ui, p, "Pasta:", |ui| {
                    let text = self.folder.as_ref().map(|f| f.display().to_string()).unwrap_or_else(|| "nenhuma escolhida".into());
                    ui.label(RichText::new(crate::i18n::tr(text)).color(if self.folder.is_some() { p.text } else { p.accent }));
                    if widgets::secondary(ui, p, "Escolher…", true) {
                        if let Some(d) = rfd::FileDialog::new().set_title(crate::i18n::tr("Pasta para vigiar")).pick_folder() {
                            self.folder = Some(d);
                        }
                    }
                });
                let o = &mut self.options;
                row(ui, p, "O que fazer:", |ui| {
                    let opts: Vec<(Action, &str)> = Action::ALL.iter().map(|a| (*a, a.label())).collect();
                    segmented(ui, p, &mut o.action, &opts);
                });
                match o.action {
                    Action::CompressImages => row(ui, p, "Nível:", |ui| {
                        let opts: Vec<(Level, &str)> = Level::ALL.iter().map(|l| (*l, l.label())).collect();
                        segmented(ui, p, &mut o.img_level, &opts);
                    }),
                    Action::ConvertImages => row(ui, p, "Converter para:", |ui| {
                        let opts: Vec<(Fmt, &str)> = Fmt::ALL.iter().map(|f| (*f, f.label())).collect();
                        segmented(ui, p, &mut o.img_fmt, &opts);
                    }),
                    Action::CompressVideos => row(ui, p, "Nível:", |ui| {
                        let opts: Vec<(VLevel, &str)> = VLevel::ALL.iter().map(|l| (*l, l.label())).collect();
                        segmented(ui, p, &mut o.vid_level, &opts);
                    }),
                    Action::ConvertVideos => row(ui, p, "Converter para:", |ui| {
                        let opts: Vec<(VFmt, &str)> = VFmt::ALL.iter().map(|f| (*f, f.label())).collect();
                        segmented(ui, p, &mut o.vid_fmt, &opts);
                    }),
                    Action::ConvertAudio => row(ui, p, "Converter para:", |ui| {
                        let opts: Vec<(AFmt, &str)> = AFmt::ALL.iter().map(|f| (*f, f.label())).collect();
                        segmented(ui, p, &mut o.aud_fmt, &opts);
                    }),
                }
                ui.checkbox(&mut self.existing, crate::i18n::tr("Processar também os arquivos que já estão na pasta"));
            });
            hint(
                ui,
                p,
                &format!(
                    "Os resultados vão para a subpasta \"{}\". Os originais não são alterados.",
                    watch::OUT_FOLDER
                ),
            );
        });
        ui.add_space(4.0);
        let label = if running { "Vigiando…" } else { "Começar a vigiar" };
        match widgets::action_bar(ui, p, &self.job, label, self.folder.as_ref().is_some_and(|f| f.is_dir())) {
            Act::Start => {
                let (folder, options, existing) = (self.folder.clone().unwrap_or_default(), self.options, self.existing);
                self.job.start(ui.ctx(), move |ctl| watch::run(ctl, &folder, options, existing));
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
        if running {
            hint(ui, p, "Clique em \"Cancelar\" para parar de vigiar.");
        }
    }
}
