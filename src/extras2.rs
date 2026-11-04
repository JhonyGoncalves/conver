//! Telas de PDF (juntar e dividir) e da pasta vigiada.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, RichText, Ui};

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
}

impl PdfTool {
    pub const ALL: [PdfTool; 7] =
        [PdfTool::Merge, PdfTool::Split, PdfTool::Compress, PdfTool::ToImages, PdfTool::Protect, PdfTool::Unlock, PdfTool::Ocr];
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
        }
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
        };
        widgets::header(ui, p, title, subtitle);
        widgets::file_list(ui, p, &mut self.queue, None, None, 190.0);
        ui.add_space(4.0);
        let selected = self.queue.selected_path().cloned();
        let selected_pages = selected.as_ref().and_then(|f| self.count(f));
        let total_pages: usize = self.queue.files.clone().iter().filter_map(|f| self.count(f)).sum();
        card(ui, p, |ui| match self.tool {
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
