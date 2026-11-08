//! Telas da leva 2.1: Ícones e favicon, Vertical automático e Melhores momentos.

use eframe::egui::{self, RichText, Ui};

use crate::job::{Job, LogKind};
use crate::pages::run_batch;
use crate::theme::Palette;
use crate::widgets::{self, Act, FileQueue, OutDir, card, hint, row, segmented};

/// Página simples da leva: lista de arquivos, ajustes e a barra de ação rodando uma função por arquivo.
fn run_simple(ui: &mut Ui, p: &Palette, job: &Job, files: &[std::path::PathBuf], label: &str, title: &'static str, work: impl Fn(&std::path::Path, &dyn Fn(f32), &dyn Fn() -> bool) -> Result<(String, std::path::PathBuf), String> + Send + 'static) {
    match widgets::action_bar(ui, p, job, label, !files.is_empty()) {
        Act::Start => {
            let files = files.to_vec();
            job.start(ui.ctx(), move |ctl| {
                run_batch(ctl, title, &files, |src, progress| work(src, progress, &|| ctl.cancelled()));
                if ctl.cancelled() {
                    ctl.log(LogKind::Info, "Cancelado.");
                }
            });
        }
        Act::Cancel => job.cancel(),
        Act::None => {}
    }
}

pub struct ReframePage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    opts: crate::reframe::Opts,
}

impl ReframePage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(crate::video::VIDEO_EXTS, "Vídeos"), out: OutDir::same_as_source(), job: Job::default(), opts: Default::default() }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &widgets::Durations) {
        use crate::reframe::Aspect;
        let title = "Vertical automático";
        widgets::header(ui, p, title, "Transforme vídeos horizontais em verticais para Reels, Shorts e TikTok, seguindo o rosto de quem fala em vez de cortar sempre o meio.");
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 120.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Formato:", |ui| {
                segmented(ui, p, &mut self.opts.aspect, &[(Aspect::Vertical, "9:16 (Reels, Shorts, TikTok)"), (Aspect::Portrait, "4:5 (feed)"), (Aspect::Square, "1:1")]);
            });
            row(ui, p, "Acompanhar:", |ui| {
                segmented(ui, p, &mut self.opts.follow, &[(0.12, "Suave"), (0.25, "Normal"), (0.45, "Rápido")]);
            });
            hint(ui, p, "A câmera fica parada enquanto o rosto está no meio e acompanha quando ele sai; numa troca de cena, pula direto. Sem rosto, fica o centro. Sai em 1080 px de largura, com o áudio original.");
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let (out, o) = (self.out.path.clone(), self.opts);
        run_simple(ui, p, &self.job, &self.queue.files, "Fazer vertical", title, move |src, pr, c| crate::reframe::reframe(src, out.as_deref(), o, pr, c));
    }
}

pub struct HighlightsPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    opts: crate::highlights::Opts,
}

impl HighlightsPage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(crate::media::MEDIA_EXTS, "Vídeos e áudios"), out: OutDir::same_as_source(), job: Job::default(), opts: Default::default() }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, durs: &widgets::Durations) {
        use crate::highlights::Brain;
        let title = "Melhores momentos";
        widgets::header(ui, p, title, "A IA ouve vídeos longos (podcasts, lives, aulas) e escolhe os trechos mais fortes para virar cortes, com um título para cada um.");
        widgets::file_list(ui, p, &mut self.queue, None, Some(durs), 120.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Quantos momentos:", |ui| {
                ui.add(egui::Slider::new(&mut self.opts.count, 1..=15));
            });
            row(ui, p, "Duração de cada um:", |ui| {
                ui.add(egui::DragValue::new(&mut self.opts.min_len).range(5.0..=self.opts.max_len).speed(1.0).suffix(" s"));
                ui.label(RichText::new(crate::i18n::tr("a")).color(p.muted));
                ui.add(egui::DragValue::new(&mut self.opts.max_len).range(self.opts.min_len..=600.0).speed(1.0).suffix(" s"));
            });
            row(ui, p, "IA que escolhe:", |ui| {
                segmented(ui, p, &mut self.opts.brain, &[(Brain::Local, "Local (sem internet)"), (Brain::Api, "A das Configurações")]);
            });
            row(ui, p, "Depois:", |ui| {
                segmented(ui, p, &mut self.opts.cut, &[(false, "Só sugerir (revisar em Cortar trecho)"), (true, "Já cortar")]);
            });
            hint(
                ui,
                p,
                "A fala é transcrita com o Whisper e lida em partes de ~10 minutos. Os momentos vão para um <vídeo>.cortes.json ao lado do vídeo: abra o vídeo em Cortar trecho para ver e ajustar cada um na linha do tempo. A IA local (1,3 GB) e o Whisper são baixados no primeiro uso.",
            );
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let (out, o) = (self.out.path.clone(), self.opts);
        run_simple(ui, p, &self.job, &self.queue.files, "Achar os momentos", title, move |src, pr, c| crate::highlights::find(src, out.as_deref(), o, pr, c));
    }
}

pub struct RestorePage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    opts: crate::restore::RestoreOpts,
}

impl RestorePage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(crate::imaging::IMAGE_EXTS, "Imagens"), out: OutDir::same_as_source(), job: Job::default(), opts: Default::default() }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, thumbs: &widgets::Thumbs) {
        let title = "Restaurar foto";
        widgets::header(ui, p, title, "Recupere rostos borrados, desgastados ou com pouca resolução em fotos antigas e escaneadas, com IA no próprio PC.");
        widgets::file_list(ui, p, &mut self.queue, Some(thumbs), None, 120.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            row(ui, p, "Intensidade:", |ui| {
                segmented(ui, p, &mut self.opts.strength, &[(0.6, "Leve"), (0.85, "Normal"), (1.0, "Forte")]);
            });
            row(ui, p, "Resto da foto:", |ui| {
                segmented(ui, p, &mut self.opts.upscale, &[(true, "Ampliar 2× (mais nítida)"), (false, "Manter o tamanho")]);
            });
            hint(ui, p, "Os rostos passam pelo GFPGAN (340 MB) e o resto da foto pode ser ampliado pelo Real-ESRGAN. Funciona melhor com rostos de frente. Sai em PNG, com o sufixo _restaurada.");
            crate::ai::model_hint(ui, p, &crate::restore::GFPGAN);
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let (out, o) = (self.out.path.clone(), self.opts);
        run_simple(ui, p, &self.job, &self.queue.files, "Restaurar", title, move |src, pr, c| crate::restore::restore_file(src, out.as_deref(), o, pr, c));
    }
}

pub struct ColorizePage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
}

impl ColorizePage {
    pub fn new() -> Self {
        Self { queue: FileQueue::new(crate::imaging::IMAGE_EXTS, "Imagens"), out: OutDir::same_as_source(), job: Job::default() }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, thumbs: &widgets::Thumbs) {
        let title = "Colorir foto";
        widgets::header(ui, p, title, "Dê cor a fotos em preto e branco com IA, no seu PC e sem internet.");
        widgets::file_list(ui, p, &mut self.queue, Some(thumbs), None, 120.0);
        ui.add_space(4.0);
        card(ui, p, |ui| {
            hint(ui, p, "A IA (DDColor, 206 MB) escolhe as cores; o brilho e os detalhes continuam os da foto original, na resolução cheia. Funciona com fotos coloridas também (as cores são refeitas).");
            crate::ai::model_hint(ui, p, &crate::restore::DDCOLOR);
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        let out = self.out.path.clone();
        run_simple(ui, p, &self.job, &self.queue.files, "Colorir", title, move |src, pr, c| crate::restore::colorize_file(src, out.as_deref(), pr, c));
    }
}

pub struct IconsPage {
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    opts: crate::icons::Opts,
    /// Miniaturas dos tamanhos, da imagem escolhida.
    preview: Option<(std::path::PathBuf, bool, Vec<egui::TextureHandle>)>,
}

impl IconsPage {
    pub fn new() -> Self {
        Self {
            queue: FileQueue::new(crate::imaging::IMAGE_EXTS, "Imagens"),
            out: OutDir::same_as_source(),
            job: Job::default(),
            opts: crate::icons::Opts { web: false, crop: false },
            preview: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        let title = "Ícones e favicon";
        widgets::header(ui, p, title, "De uma imagem, um ícone .ico com todos os tamanhos do Windows, ou o pacote completo de ícones para um site.");
        widgets::file_list(ui, p, &mut self.queue, None, None, 120.0);
        ui.add_space(4.0);
        // miniaturas nos tamanhos de verdade, para ver como o ícone fica pequeno
        if let Some(src) = self.queue.selected_path().cloned() {
            if self.preview.as_ref().is_none_or(|(f, c, _)| *f != src || *c != self.opts.crop) {
                let texs = crate::imaging::load(&src).ok().map(|img| {
                    let sq = crate::icons::square(&img.to_rgba8(), self.opts.crop);
                    [16u32, 32, 48, 128]
                        .iter()
                        .map(|s| {
                            let t = image::imageops::resize(&sq, *s, *s, image::imageops::FilterType::Lanczos3);
                            let ci = egui::ColorImage::from_rgba_unmultiplied([*s as usize, *s as usize], t.as_raw());
                            ui.ctx().load_texture(format!("icone_{s}"), ci, egui::TextureOptions::NEAREST)
                        })
                        .collect()
                });
                self.preview = Some((src, self.opts.crop, texs.unwrap_or_default()));
            }
        } else {
            self.preview = None;
        }
        card(ui, p, |ui| {
            row(ui, p, "Gerar:", |ui| {
                segmented(ui, p, &mut self.opts.web, &[(false, "Ícone .ico (Windows)"), (true, "Pacote para sites")]);
            });
            row(ui, p, "Imagem não quadrada:", |ui| {
                segmented(ui, p, &mut self.opts.crop, &[(false, "Caber inteira (bordas transparentes)"), (true, "Cortar o centro")]);
            });
            if let Some((_, _, texs)) = &self.preview {
                if !texs.is_empty() {
                    row(ui, p, "Como fica:", |ui| {
                        for t in texs {
                            ui.add(egui::Image::new((t.id(), t.size_vec2())));
                            ui.add_space(6.0);
                        }
                    });
                }
            }
            hint(
                ui,
                p,
                if self.opts.web {
                    "Uma pasta com favicon.ico, PNGs de 16 a 512 px, o ícone da Apple, o site.webmanifest e as linhas para colar no <head> do site."
                } else {
                    "Um .ico com 16, 24, 32, 48, 64, 128 e 256 px, para atalhos, pastas e programas. Use uma imagem de pelo menos 256 px."
                },
            );
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        match widgets::action_bar(ui, p, &self.job, "Gerar ícones", !self.queue.files.is_empty()) {
            Act::Start => {
                let (files, out, o) = (self.queue.files.clone(), self.out.path.clone(), self.opts);
                self.job.start(ui.ctx(), move |ctl| {
                    run_batch(ctl, title, &files, |src, progress| {
                        progress(0.3);
                        crate::icons::make(src, out.as_deref(), o)
                    });
                    if ctl.cancelled() {
                        ctl.log(LogKind::Info, "Cancelado.");
                    }
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
        let _ = RichText::new("");
    }
}
