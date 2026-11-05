//! Telas de imagem e de download.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, ColorImage, RichText, TextureHandle, TextureOptions, Ui, vec2};

use crate::download::{self, Kind};
use crate::history;
use crate::imaging::{self, Aspect, Background, Filter, Fmt, ImageOp, Level, PageSize, ResizeMode};
use crate::job::{Ctl, Job, LogKind};
use crate::theme::{Palette, bold};
use crate::tools;
use crate::widgets::{self, Act, FileQueue, OutDir, Thumb, Thumbs, card, hint, row, segmented};

/// Processa uma lista de arquivos, um de cada vez, registrando cada resultado (e no histórico).
pub fn run_batch(
    ctl: &Ctl,
    tool: &str,
    files: &[PathBuf],
    mut each: impl FnMut(&PathBuf, &dyn Fn(f32)) -> Result<(String, PathBuf), String>,
) {
    let n = files.len();
    let (mut ok, mut last) = (0, None);
    for (i, src) in files.iter().enumerate() {
        if ctl.cancelled() {
            ctl.log(LogKind::Info, "Cancelado pelo usuário.");
            break;
        }
        let name = tools::file_name(src);
        ctl.status(format!("[{}/{n}] {name}", i + 1));
        let progress = |f: f32| ctl.progress((i as f32 + f) / n as f32);
        match each(src, &progress) {
            Ok((info, dst)) => {
                ok += 1;
                ctl.log(LogKind::Ok, format!("✔ {name}   {info}"));
                history::record(tool, &name, &dst, &info);
                last = Some(dst);
            }
            Err(e) => ctl.log(LogKind::Err, format!("✖ {name}: {e}")),
        }
        ctl.progress((i + 1) as f32 / n as f32);
    }
    ctl.status(format!("Concluído: {ok} de {n} arquivo(s)."));
    if let Some(folder) = last.as_deref().and_then(|d| d.parent()) {
        ctl.log(LogKind::Info, format!("Salvo em: {}", folder.display()));
        ctl.done_folder(folder.to_path_buf());
    }
}

/// Uma única saída a partir de vários arquivos (juntar vídeos, PDF).
pub fn run_single(ctl: &Ctl, tool: &str, input: &str, work: impl FnOnce() -> Result<(String, PathBuf), String>) {
    ctl.status("Processando…");
    match work() {
        Ok((info, dst)) => {
            ctl.progress(1.0);
            ctl.log(LogKind::Ok, format!("✔ {}   {info}", tools::file_name(&dst)));
            history::record(tool, input, &dst, &info);
            ctl.status("Concluído.");
            if let Some(folder) = dst.parent() {
                ctl.log(LogKind::Info, format!("Salvo em: {}", folder.display()));
                ctl.done_folder(folder.to_path_buf());
            }
        }
        Err(e) => {
            ctl.log(LogKind::Err, format!("✖ {e}"));
            ctl.status("Erro.");
        }
    }
}

fn alert(text: impl Into<String>) {
    rfd::MessageDialog::new().set_title(crate::i18n::tr("Conver")).set_description(crate::i18n::tr(text.into())).show();
}

// ================================================================ imagem

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImgTool {
    Resize,
    Compress,
    Quality,
    Convert,
    Transform,
    Strip,
    Pdf,
    Adjust,
    RemoveBg,
    Upscale,
}

#[derive(Clone)]
struct PrevData {
    tex: TextureHandle,
    info: String,
    size: u64,
    dims: (u32, u32),
}

type PrevKey = (PathBuf, ImageOp);

#[derive(Default)]
struct PrevShared {
    busy: bool,
    done: Option<(PrevKey, Result<PrevData, String>)>,
}

/// Prévia do resultado, recalculada em segundo plano quando as opções mudam.
struct ResultPreview {
    want: Option<PrevKey>,
    changed: Instant,
    shared: Arc<Mutex<PrevShared>>,
}

impl ResultPreview {
    fn new() -> Self {
        Self { want: None, changed: Instant::now(), shared: Arc::default() }
    }

    fn update(&mut self, ctx: &egui::Context, want: Option<PrevKey>) {
        if want != self.want {
            self.want = want;
            self.changed = Instant::now();
        }
        let Some(key) = self.want.clone() else { return };
        let mut sh = self.shared.lock().unwrap();
        if sh.busy || sh.done.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let wait = Duration::from_millis(350).saturating_sub(self.changed.elapsed());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return;
        }
        sh.busy = true;
        let (shared, ctx) = (self.shared.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = imaging::process(&key.0, &key.1).and_then(|out| {
                let img = image::load_from_memory(&out.bytes).map_err(|e| e.to_string())?;
                let t = imaging::thumbnail(&img, 900);
                let ci = ColorImage::from_rgba_unmultiplied([t.width() as usize, t.height() as usize], t.as_raw());
                let tex = ctx.load_texture("preview", ci, TextureOptions::LINEAR);
                Ok(PrevData { tex, info: out.info, size: out.bytes.len() as u64, dims: out.dims })
            });
            let mut sh = shared.lock().unwrap();
            sh.busy = false;
            sh.done = Some((key, result));
            ctx.request_repaint();
        });
    }

    /// `None` enquanto calcula.
    fn current(&self) -> Option<Result<PrevData, String>> {
        let sh = self.shared.lock().unwrap();
        match (&sh.done, &self.want) {
            (Some((k, r)), Some(w)) if k == w => Some(r.clone()),
            _ => None,
        }
    }
}

pub struct ImagePage {
    tool: ImgTool,
    pub queue: FileQueue,
    out: OutDir,
    job: Job,
    preview: ResultPreview,
    show_result: bool,
    pct: u32,
    px_mode: bool,
    w: String,
    h: String,
    keep: bool,
    level: Level,
    quality: u8,
    target: Fmt,
    rotate: u16,
    flip_h: bool,
    flip_v: bool,
    aspect: Aspect,
    page: PageSize,
    pdf_name: String,
    brightness: i16,
    contrast: i16,
    saturation: i16,
    filter: Filter,
    bg: Background,
    bg_color: [u8; 3],
    hard: bool,
    scale: u32,
}

impl ImagePage {
    pub fn new(tool: ImgTool) -> Self {
        let queue = FileQueue::new(imaging::IMAGE_EXTS, "Imagens");
        Self {
            tool,
            queue: if tool == ImgTool::Pdf { queue.ordered() } else { queue },
            out: OutDir::same_as_source(),
            job: Job::default(),
            preview: ResultPreview::new(),
            show_result: tool != ImgTool::Pdf && tool != ImgTool::Upscale,
            pct: 50,
            px_mode: false,
            w: String::new(),
            h: String::new(),
            keep: true,
            level: Level::Media,
            quality: 40,
            target: Fmt::Png,
            rotate: 90,
            flip_h: false,
            flip_v: false,
            aspect: Aspect::Livre,
            page: PageSize::A4Retrato,
            pdf_name: "imagens".into(),
            brightness: 0,
            contrast: 0,
            saturation: 0,
            filter: Filter::Nenhum,
            bg: Background::Transparente,
            bg_color: [0xF1, 0x30, 0x30],
            hard: true,
            scale: 2,
        }
    }

    fn texts(&self) -> (&'static str, &'static str, &'static str) {
        match self.tool {
            ImgTool::Resize => (
                "Redimensionar imagem",
                "Altere as dimensões de uma ou várias imagens, por porcentagem ou em pixels.",
                "Redimensionar",
            ),
            ImgTool::Compress => (
                "Comprimir imagem",
                "Reduza o tamanho do arquivo mantendo o formato (JPG, PNG, WEBP…).",
                "Comprimir",
            ),
            ImgTool::Quality => (
                "Diminuir qualidade",
                "Escolha de 1 a 100. JPG e WEBP mantêm o formato; os demais viram JPG (ou WEBP, se tiverem transparência).",
                "Aplicar",
            ),
            ImgTool::Convert => ("Converter formato de imagem", "Converta entre PNG, JPG, WEBP, BMP, GIF, TIFF e ICO.", "Converter"),
            ImgTool::Transform => (
                "Girar, espelhar e recortar",
                "Gire, espelhe ou recorte as imagens numa proporção pronta (o recorte é centralizado).",
                "Aplicar",
            ),
            ImgTool::Strip => (
                "Remover metadados",
                "Apaga informações escondidas da foto (EXIF, localização GPS, câmera, data) sem perder qualidade.",
                "Remover",
            ),
            ImgTool::RemoveBg => (
                "Remover fundo",
                "Tire o fundo de fotos de pessoas, produtos e animais com inteligência artificial, no seu PC e sem internet.",
                "Remover fundo",
            ),
            ImgTool::Upscale => (
                "Ampliar com IA",
                "Aumente imagens 2× ou 4× sem borrar: a inteligência artificial recria os detalhes. Roda no seu PC, sem internet.",
                "Ampliar",
            ),
            ImgTool::Adjust => (
                "Ajustes de cor",
                "Mude brilho, contraste e saturação ou aplique um filtro pronto. Confira na prévia antes de salvar.",
                "Aplicar",
            ),
            ImgTool::Pdf => (
                "Imagens para PDF",
                "Junte várias imagens num único PDF, uma por página, na ordem da lista.",
                "Criar PDF",
            ),
        }
    }

    fn op(&self) -> Result<ImageOp, String> {
        Ok(match self.tool {
            ImgTool::Resize if !self.px_mode => ImageOp::Resize(ResizeMode::Percent(self.pct)),
            ImgTool::Resize => {
                let num = |s: &str| -> Result<u32, String> {
                    let s = s.trim();
                    if s.is_empty() {
                        return Ok(0);
                    }
                    s.parse::<u32>()
                        .ok()
                        .filter(|v| *v > 0 && *v <= 30000)
                        .ok_or_else(|| "Largura e altura devem ser números inteiros positivos.".to_string())
                };
                let (w, h) = (num(&self.w)?, num(&self.h)?);
                if w == 0 && h == 0 {
                    return Err("Informe a largura e/ou a altura em pixels.".into());
                }
                if !self.keep && (w == 0 || h == 0) {
                    return Err("Sem manter a proporção, informe largura E altura.".into());
                }
                ImageOp::Resize(ResizeMode::Pixels { w, h, keep: self.keep })
            }
            ImgTool::Compress => ImageOp::Compress(self.level),
            ImgTool::Quality => ImageOp::Quality(self.quality),
            ImgTool::Convert => ImageOp::Convert(self.target),
            ImgTool::Transform => {
                ImageOp::Transform { rotate: self.rotate, flip_h: self.flip_h, flip_v: self.flip_v, aspect: self.aspect }
            }
            ImgTool::Strip => ImageOp::StripMeta,
            ImgTool::RemoveBg => ImageOp::RemoveBg { bg: self.bg, hard: self.hard },
            ImgTool::Upscale => ImageOp::Upscale { scale: self.scale },
            ImgTool::Adjust => ImageOp::Adjust {
                brightness: self.brightness,
                contrast: self.contrast,
                saturation: self.saturation,
                filter: self.filter,
            },
            ImgTool::Pdf => return Err("sem prévia".into()),
        })
    }

    fn options_ui(&mut self, ui: &mut Ui, p: &Palette) {
        match self.tool {
            ImgTool::Resize => {
                row(ui, p, "Modo:", |ui| {
                    segmented(ui, p, &mut self.px_mode, &[(false, "Porcentagem"), (true, "Pixels")]);
                });
                if self.px_mode {
                    row(ui, p, "Tamanho:", |ui| {
                        ui.label(crate::i18n::tr("Largura"));
                        ui.add(egui::TextEdit::singleline(&mut self.w).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("px")).desired_width(70.0));
                        ui.add_space(8.0);
                        ui.label(crate::i18n::tr("Altura"));
                        ui.add(egui::TextEdit::singleline(&mut self.h).margin(crate::widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("px")).desired_width(70.0));
                        ui.add_space(8.0);
                        crate::widgets::check(ui, &mut self.keep, crate::i18n::tr("Manter proporção"));
                    });
                    hint(ui, p, "Com a proporção mantida, a imagem cabe dentro da largura/altura informada.");
                } else {
                    row(ui, p, "Tamanho:", |ui| {
                        ui.add(egui::Slider::new(&mut self.pct, 1..=200).suffix("%"));
                    });
                }
            }
            ImgTool::Compress => {
                row(ui, p, "Nível:", |ui| {
                    let opts: Vec<(Level, &str)> = Level::ALL.iter().map(|l| (*l, l.label())).collect();
                    segmented(ui, p, &mut self.level, &opts);
                });
                hint(ui, p, "Leve = quase sem perda visível  •  Forte = arquivo bem menor");
            }
            ImgTool::Quality => {
                row(ui, p, "Qualidade:", |ui| {
                    ui.add(egui::Slider::new(&mut self.quality, 1..=100));
                });
            }
            ImgTool::Convert => {
                row(ui, p, "Converter para:", |ui| {
                    let opts: Vec<(Fmt, &str)> = Fmt::ALL.iter().map(|f| (*f, f.label())).collect();
                    segmented(ui, p, &mut self.target, &opts);
                });
                if self.target == Fmt::Ico {
                    hint(ui, p, "Ícones têm no máximo 256×256; imagens maiores são reduzidas.");
                }
            }
            ImgTool::Transform => {
                row(ui, p, "Girar:", |ui| {
                    segmented(ui, p, &mut self.rotate, &[(0, "Não girar"), (90, "90° ↻"), (180, "180°"), (270, "90° ↺")]);
                });
                row(ui, p, "Espelhar:", |ui| {
                    crate::widgets::check(ui, &mut self.flip_h, crate::i18n::tr("Na horizontal ↔"));
                    ui.add_space(8.0);
                    crate::widgets::check(ui, &mut self.flip_v, crate::i18n::tr("Na vertical ↕"));
                });
                row(ui, p, "Recortar:", |ui| {
                    let opts: Vec<(Aspect, &str)> = Aspect::ALL.iter().map(|a| (*a, a.label())).collect();
                    segmented(ui, p, &mut self.aspect, &opts);
                });
            }
            ImgTool::Strip => {
                hint(ui, p, "JPG, PNG e WEBP: só os metadados são apagados, a imagem não é recomprimida.");
                hint(ui, p, "Se a foto tiver rotação da câmera, ela é aplicada antes, para a imagem não ficar de lado.");
            }
            ImgTool::RemoveBg => {
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
                row(ui, p, "Borda:", |ui| {
                    segmented(ui, p, &mut self.hard, &[(true, "Definida"), (false, "Suave (cabelo, pelos)")]);
                });
                hint(ui, p, "Transparente salva em PNG. A primeira imagem demora alguns segundos enquanto o modelo carrega.");
                if crate::bgremove::model_path().is_none() {
                    ui.label(RichText::new(crate::i18n::tr("⚠ Modelo de IA não encontrado: coloque o silueta.onnx na pasta modelos.")).color(p.accent));
                }
            }
            ImgTool::Upscale => {
                row(ui, p, "Ampliar:", |ui| {
                    segmented(ui, p, &mut self.scale, &[(2, "2×"), (4, "4×")]);
                });
                hint(ui, p, "Ótimo para fotos pequenas, prints e imagens antigas. Imagens grandes levam mais tempo (a prévia também).");
                if crate::ai::find_model(crate::upscale::MODEL_FILE).is_none() {
                    ui.label(RichText::new(crate::i18n::tr("⚠ Modelo de IA não encontrado: coloque o realesr-general-x4v3.onnx na pasta modelos.")).color(p.accent));
                }
            }
            ImgTool::Adjust => {
                for (label, v) in [("Brilho:", &mut self.brightness), ("Contraste:", &mut self.contrast), ("Saturação:", &mut self.saturation)] {
                    row(ui, p, label, |ui| {
                        ui.add(egui::Slider::new(v, -100..=100));
                        if *v != 0 && widgets::secondary(ui, p, "Zerar", true) {
                            *v = 0;
                        }
                    });
                }
                row(ui, p, "Filtro:", |ui| {
                    let opts: Vec<(Filter, &str)> = Filter::ALL.iter().map(|f| (*f, f.label())).collect();
                    segmented(ui, p, &mut self.filter, &opts);
                });
            }
            ImgTool::Pdf => {
                row(ui, p, "Tamanho da página:", |ui| {
                    let opts: Vec<(PageSize, &str)> = PageSize::ALL.iter().map(|s| (*s, s.label())).collect();
                    segmented(ui, p, &mut self.page, &opts);
                });
                row(ui, p, "Nome do PDF:", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.pdf_name).margin(crate::widgets::FIELD_MARGIN).desired_width(240.0));
                    ui.label(RichText::new(crate::i18n::tr(".pdf")).color(p.muted));
                });
                hint(ui, p, "Use ↑ e ↓ na lista para mudar a ordem das páginas.");
            }
        }
    }

    fn preview_ui(&mut self, ui: &mut Ui, p: &Palette, thumbs: &Thumbs, height: f32) {
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Prévia")).family(bold()).color(p.text));
                if self.tool != ImgTool::Pdf {
                    ui.with_layout(crate::widgets::back(Align::Center), |ui| {
                        segmented(ui, p, &mut self.show_result, &[(false, "Original"), (true, "Resultado")]);
                    });
                }
            });
            ui.add_space(4.0);
            let Some(path) = self.queue.selected_path().cloned() else {
                ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Adicione uma imagem para ver a prévia")).color(p.muted));
                    });
                });
                return;
            };
            let original = thumbs.get(&path);
            let result = if self.show_result { self.preview.current() } else { None };
            let area = vec2(ui.available_width(), height - 44.0);
            let (rect, _) = ui.allocate_exact_size(area, egui::Sense::hover());
            ui.painter().rect_filled(rect, 8, p.input);
            let checker = self.tool == ImgTool::RemoveBg;
            let draw = |ui: &mut Ui, tex: &TextureHandle| {
                let s = tex.size_vec2();
                let size = widgets::fit((s.x, s.y), area - vec2(16.0, 16.0));
                if checker {
                    widgets::checkerboard(ui.painter(), egui::Rect::from_center_size(rect.center(), size));
                }
                ui.painter().image(
                    tex.id(),
                    egui::Rect::from_center_size(rect.center(), size),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            };
            let center_text = |ui: &mut Ui, t: &str, c| {
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, crate::i18n::tr(t), egui::FontId::proportional(14.0), c);
            };
            let orig_size = tools::human_size(tools::file_size(&path));
            let caption = if self.show_result {
                match &result {
                    Some(Ok(d)) => {
                        draw(ui, &d.tex);
                        format!("Resultado: {}×{}  •  {}   ({})", d.dims.0, d.dims.1, tools::human_size(d.size), d.info)
                    }
                    Some(Err(e)) => {
                        center_text(ui, e, p.accent);
                        String::new()
                    }
                    None => {
                        if let Thumb::Ready { tex, .. } = &original {
                            draw(ui, tex);
                            ui.painter().rect_filled(rect, 8, p.input.gamma_multiply(0.6));
                        }
                        center_text(ui, "Gerando prévia…", p.text);
                        String::new()
                    }
                }
            } else {
                match &original {
                    Thumb::Ready { tex, dims } => {
                        draw(ui, tex);
                        let page = if self.tool == ImgTool::Pdf {
                            format!("Página {}  •  ", self.queue.selected + 1)
                        } else {
                            String::new()
                        };
                        format!("{page}Original: {}×{}  •  {orig_size}", dims.0, dims.1)
                    }
                    Thumb::Loading => {
                        center_text(ui, "Carregando…", p.muted);
                        String::new()
                    }
                    Thumb::Failed => {
                        center_text(ui, "Não foi possível abrir esta imagem.", p.accent);
                        String::new()
                    }
                }
            };
            ui.add_space(2.0);
            ui.add(egui::Label::new(RichText::new(crate::i18n::tr(caption)).size(13.0).color(p.muted)).truncate());
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, thumbs: &Thumbs) {
        let (title, subtitle, action) = self.texts();
        widgets::header(ui, p, title, subtitle);

        let want = self.queue.selected_path().cloned().zip(self.op().ok());
        self.preview.update(ui.ctx(), if self.show_result { want } else { None });

        let height = 300.0;
        ui.horizontal_top(|ui| {
            let total = ui.available_width();
            let left = (total * 0.42).max(300.0);
            ui.allocate_ui_with_layout(vec2(left, height + 80.0), crate::widgets::down(), |ui| {
                widgets::file_list(ui, p, &mut self.queue, Some(thumbs), None, height);
            });
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height + 80.0), crate::widgets::down(), |ui| {
                self.preview_ui(ui, p, thumbs, height);
            });
        });
        ui.add_space(4.0);
        card(ui, p, |ui| self.options_ui(ui, p));
        ui.add_space(4.0);
        self.out.ui(ui, p);

        match widgets::action_bar(ui, p, &self.job, action, !self.queue.files.is_empty()) {
            Act::Start if self.tool == ImgTool::Pdf => self.start_pdf(ui.ctx(), title),
            Act::Start => match self.op() {
                Ok(op) => {
                    let files = self.queue.files.clone();
                    let out = self.out.path.clone();
                    self.job.start(ui.ctx(), move |ctl| {
                        run_batch(ctl, title, &files, |src, progress| {
                            crate::upscale::with_progress(progress, &|| ctl.cancelled(), || imaging::process_and_save(src, &op, out.as_deref()))
                        });
                    });
                }
                Err(e) => alert(e),
            },
            Act::Cancel => self.job.cancel(),
            _ => {}
        }
    }

    fn start_pdf(&mut self, ctx: &egui::Context, title: &'static str) {
        let name: String = self.pdf_name.trim().chars().filter(|c| !"\\/:*?\"<>|".contains(*c)).collect();
        let name = if name.is_empty() { "imagens".to_string() } else { name };
        let files = self.queue.files.clone();
        let folder = self
            .out
            .path
            .clone()
            .or_else(|| files.first().and_then(|f| f.parent()).map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let page = self.page;
        self.job.start(ctx, move |ctl| {
            let input = format!("{} imagens", files.len());
            run_single(ctl, title, &input, || {
                let dst = tools::unique_path(folder.join(format!("{name}.pdf")));
                let pages = imaging::images_to_pdf(&files, &dst, page, &|f| ctl.progress(f), &|| ctl.cancelled())?;
                Ok((format!("{pages} página(s) • {}", tools::human_size(tools::file_size(&dst))), dst))
            });
        });
    }
}

// ============================================================== download

const BROWSERS: &[(&str, Option<&str>)] = &[
    ("Não usar", None),
    ("Firefox", Some("firefox")),
    ("Edge", Some("edge")),
    ("Chrome", Some("chrome")),
    ("Brave", Some("brave")),
    ("Opera", Some("opera")),
];

pub struct DownloadPage {
    url: String,
    subtitles: bool,
    cover: bool,
    kind: Kind,
    quality: usize,
    playlist: bool,
    browser: usize,
    pub out: OutDir,
    job: Job,
}

impl DownloadPage {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            url: String::new(),
            subtitles: false,
            cover: false,
            kind: Kind::Mp4,
            quality: 0,
            playlist: false,
            browser: 0,
            out: OutDir::fixed(folder),
            job: Job::default(),
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        widgets::header(
            ui,
            p,
            "Baixar vídeos e músicas",
            "YouTube, YouTube Music, TikTok, Instagram, X/Twitter, Facebook, Vimeo, SoundCloud, Twitch e centenas de outros sites.",
        );
        card(ui, p, |ui| {
            let n = download::parse_links(&self.url).len();
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Links")).family(bold()).color(p.text));
                if n > 1 {
                    ui.label(RichText::new(crate::i18n::tr(format!("{n} links — serão baixados em fila"))).size(12.5).color(p.muted));
                }
            });
            ui.horizontal_top(|ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.url)
                        .hint_text(crate::i18n::tr("Cole aqui o link do vídeo, música ou playlist (um ou vários, um por linha)"))
                        .desired_width(ui.available_width() - 90.0)
                        .desired_rows(3)
                        .font(egui::FontId::proportional(14.5))
                        .margin(egui::Margin::symmetric(10, 8)),
                );
                if widgets::secondary(ui, p, "Colar", true) {
                    if let Ok(text) = arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                        let text = text.trim();
                        if self.url.trim().is_empty() {
                            self.url = text.to_string();
                        } else {
                            // já tem links: acrescenta numa nova linha
                            self.url = format!("{}\n{text}", self.url.trim_end());
                        }
                    }
                }
            });
            ui.add_space(6.0);
            row(ui, p, "Formato:", |ui| {
                if segmented(ui, p, &mut self.kind, &[(Kind::Mp4, "MP4 (vídeo)"), (Kind::Mp3, "MP3 (áudio)")]) {
                    self.quality = 0;
                }
            });
            row(ui, p, "Qualidade:", |ui| {
                let list = if self.kind == Kind::Mp3 { download::MP3_QUALITIES } else { download::MP4_QUALITIES };
                let opts: Vec<(usize, &str)> = list.iter().enumerate().map(|(i, s)| (i, *s)).collect();
                segmented(ui, p, &mut self.quality, &opts);
            });
            row(ui, p, "Login do navegador:", |ui| {
                egui::ComboBox::from_id_salt("cookies").selected_text(crate::i18n::tr(BROWSERS[self.browser].0)).width(140.0).show_ui(
                    ui,
                    |ui| {
                        for (i, (name, _)) in BROWSERS.iter().enumerate() {
                            ui.selectable_value(&mut self.browser, i, crate::i18n::tr(*name));
                        }
                    },
                );
                hint(ui, p, "para sites que pedem conta (Instagram, Facebook). Feche o navegador antes de baixar.");
            });
            row(ui, p, "Baixar junto:", |ui| {
                crate::widgets::check(ui, &mut self.subtitles, crate::i18n::tr("Legendas (.srt, português e inglês)"));
                ui.add_space(8.0);
                crate::widgets::check(ui, &mut self.cover, crate::i18n::tr("Imagem de capa (.jpg)"));
            });
            crate::widgets::check(ui, &mut self.playlist, crate::i18n::tr("Baixar a playlist inteira (se o link for de uma playlist)"));
        });
        ui.add_space(4.0);
        self.out.ui(ui, p);
        hint(ui, p, "Baixe apenas conteúdos que você tem direito de baixar.");

        let links = download::parse_links(&self.url);
        match widgets::action_bar(ui, p, &self.job, "Baixar", !links.is_empty()) {
            Act::Start => {
                let Some(folder) = self.out.path.clone().filter(|d| d.is_dir()) else {
                    alert("Escolha uma pasta de saída válida.");
                    return;
                };
                let (kind, quality, playlist) = (self.kind, self.quality, self.playlist);
                let cookies = BROWSERS[self.browser].1;
                let extras = download::Extras { subtitles: self.subtitles, cover: self.cover };
                self.job.start(ui.ctx(), move |ctl| {
                    let n = links.len();
                    let (mut total, mut failed) = (0, 0);
                    for (i, url) in links.iter().enumerate() {
                        if ctl.cancelled() {
                            ctl.log(LogKind::Info, "Cancelado pelo usuário.");
                            break;
                        }
                        let prefix = if n > 1 { format!("[link {}/{n}] ", i + 1) } else { String::new() };
                        ctl.status(format!("{prefix}Buscando informações…"));
                        ctl.progress(0.0);
                        ctl.log(LogKind::Info, format!("{prefix}{url}"));
                        match download::download(url, &folder, kind, quality, playlist, cookies, extras, ctl) {
                            Ok(files) => {
                                for f in &files {
                                    history::record("Download", url, f, &tools::human_size(tools::file_size(f)));
                                }
                                total += files.len();
                            }
                            Err(e) => {
                                failed += 1;
                                ctl.log(LogKind::Err, format!("✖ {e}"));
                            }
                        }
                    }
                    ctl.progress(1.0);
                    let errors = if failed > 0 { format!(" ({failed} link(s) com erro)") } else { String::new() };
                    ctl.status(format!("Concluído: {total} arquivo(s) baixado(s){errors}."));
                    ctl.done_folder(folder.clone());
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}
