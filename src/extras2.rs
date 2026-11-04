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
use crate::{pdf, tools};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum PdfTool {
    Merge,
    Split,
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
                    ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(240.0));
                    ui.label(RichText::new(".pdf").color(p.muted));
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
                        ui.add(egui::TextEdit::singleline(&mut self.ranges).hint_text("1-3, 7, 10-").desired_width(200.0));
                        hint(ui, p, "\"10-\" = da 10 até o fim");
                    });
                    if let Some(n) = selected_pages {
                        if let Err(e) = pdf::parse_ranges(&self.ranges, n as u32) {
                            ui.label(RichText::new(e).color(p.accent));
                        }
                    }
                } else {
                    hint(ui, p, "As páginas são salvas numa pasta <nome do PDF>_paginas.");
                }
                if let Some(n) = selected_pages {
                    hint(ui, p, &format!("O PDF selecionado tem {n} páginas."));
                }
            }
        });
        if selected.is_some() && selected_pages.is_none() {
            ui.label(RichText::new("Não foi possível ler o PDF selecionado (pode estar danificado ou protegido).").color(p.accent));
        }
        ui.add_space(4.0);
        self.out.ui(ui, p);

        let can = match self.tool {
            PdfTool::Merge => self.queue.files.len() >= 2,
            PdfTool::Split => !self.queue.files.is_empty() && (self.each_page || !self.ranges.trim().is_empty()),
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
                    ui.label(RichText::new(text).color(if self.folder.is_some() { p.text } else { p.accent }));
                    if widgets::secondary(ui, p, "Escolher…", true) {
                        if let Some(d) = rfd::FileDialog::new().set_title("Pasta para vigiar").pick_folder() {
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
                ui.checkbox(&mut self.existing, "Processar também os arquivos que já estão na pasta");
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
