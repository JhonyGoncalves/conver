#![windows_subsystem = "windows"]
//! Conver — conversor universal.

mod download;
mod extras;
mod extras2;
mod extras3;
mod history;
mod imaging;
mod info;
mod integration;
mod job;
mod media;
mod media_extra;
mod media_page;
mod notify;
mod ocr;
mod pages;
mod pdf;
mod qr;
mod recorder;
mod rename;
mod theme;
mod tools;
mod video;
mod watch;
mod widgets;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, CursorIcon, FontId, Frame, Layout, Margin, RichText, Sense, Stroke,
    TextureHandle, TextureOptions, Ui, vec2,
};
use serde::{Deserialize, Serialize};

use extras::{HistoryPage, QrPage, RenamePage};
use extras2::{PdfPage, PdfTool, WatchPage};
use extras3::{InfoPage, OcrPage, RecorderPage};
use job::Job;
use media_page::{MTool, MediaPage};
use pages::{DownloadPage, ImagePage, ImgTool};
use theme::{Palette, bold};
use widgets::{Durations, FileQueue, Thumbs, card, row};

const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

// ------------------------------------------------------------ configurações

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Config {
    dark: bool,
    download_dir: Option<PathBuf>,
    notify: bool,
    sound: bool,
    check_updates: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { dark: false, download_dir: None, notify: true, sound: true, check_updates: true }
    }
}

fn config_path() -> PathBuf {
    tools::app_dir().join("config.json")
}

impl Config {
    fn load() -> Self {
        std::fs::read_to_string(config_path())
            .ok()
            .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        let _ = std::fs::create_dir_all(tools::app_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_path(), s);
        }
    }

    fn download_dir(&self) -> PathBuf {
        self.download_dir.clone().filter(|d| d.is_dir()).unwrap_or_else(|| {
            std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads")).unwrap_or_else(tools::exe_dir)
        })
    }
}

// ------------------------------------------------------------------- app

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Img(ImgTool),
    Media(MTool),
    Download,
    Pdf(PdfTool),
    Watch,
    Recorder,
    Ocr,
    Info,
    Rename,
    Qr,
    History,
    Settings,
}

const IMG_TOOLS: [ImgTool; 8] = [
    ImgTool::Resize,
    ImgTool::Compress,
    ImgTool::Quality,
    ImgTool::Convert,
    ImgTool::Transform,
    ImgTool::Strip,
    ImgTool::Pdf,
    ImgTool::Adjust,
];

const MEDIA_TOOLS: [MTool; 15] = [
    MTool::VConvert,
    MTool::VCompress,
    MTool::VTrim,
    MTool::Merge,
    MTool::Speed,
    MTool::AudioFrames,
    MTool::AConvert,
    MTool::ATrim,
    MTool::Normalize,
    MTool::ACompress,
    MTool::Gif,
    MTool::VTransform,
    MTool::Subtitles,
    MTool::Music,
    MTool::AMerge,
];

const NAV: &[(&str, &[(Page, &str)])] = &[
    (
        "IMAGEM",
        &[
            (Page::Img(ImgTool::Resize), "Redimensionar"),
            (Page::Img(ImgTool::Compress), "Comprimir"),
            (Page::Img(ImgTool::Quality), "Diminuir qualidade"),
            (Page::Img(ImgTool::Convert), "Converter formato"),
            (Page::Img(ImgTool::Transform), "Girar / recortar"),
            (Page::Img(ImgTool::Adjust), "Ajustes de cor"),
            (Page::Img(ImgTool::Strip), "Remover metadados"),
            (Page::Img(ImgTool::Pdf), "Imagens → PDF"),
            (Page::Ocr, "Texto de imagem (OCR)"),
        ],
    ),
    (
        "VÍDEO",
        &[
            (Page::Media(MTool::VConvert), "Converter formato"),
            (Page::Media(MTool::VCompress), "Comprimir"),
            (Page::Media(MTool::VTrim), "Cortar trecho"),
            (Page::Media(MTool::Merge), "Juntar vídeos"),
            (Page::Media(MTool::Speed), "Mudar velocidade"),
            (Page::Media(MTool::VTransform), "Girar / recortar"),
            (Page::Media(MTool::Gif), "Vídeo → GIF"),
            (Page::Media(MTool::Subtitles), "Queimar legenda"),
            (Page::Media(MTool::Music), "Música de fundo"),
            (Page::Media(MTool::AudioFrames), "Remover áudio / frames"),
        ],
    ),
    (
        "ÁUDIO",
        &[
            (Page::Recorder, "Gravador de áudio"),
            (Page::Media(MTool::AConvert), "Converter formato"),
            (Page::Media(MTool::ATrim), "Cortar áudio"),
            (Page::Media(MTool::AMerge), "Juntar áudios"),
            (Page::Media(MTool::Normalize), "Normalizar volume"),
            (Page::Media(MTool::ACompress), "Comprimir"),
        ],
    ),
    ("PDF", &[(Page::Pdf(PdfTool::Merge), "Juntar PDFs"), (Page::Pdf(PdfTool::Split), "Dividir PDF")]),
    ("DOWNLOAD", &[(Page::Download, "Vídeos e músicas")]),
    (
        "FERRAMENTAS",
        &[
            (Page::Info, "Informações do arquivo"),
            (Page::Watch, "Pasta vigiada"),
            (Page::Rename, "Renomear em lote"),
            (Page::Qr, "QR Code"),
        ],
    ),
];

enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(integration::Release),
    Failed(String),
}

struct About {
    ffmpeg: Option<PathBuf>,
    ytdlp: Option<String>,
}

struct ConverApp {
    cfg: Config,
    pal: Palette,
    page: Page,
    logo: TextureHandle,
    thumbs: Thumbs,
    durs: Durations,
    images: HashMap<ImgTool, ImagePage>,
    medias: HashMap<MTool, MediaPage>,
    download: DownloadPage,
    pdfs: HashMap<PdfTool, PdfPage>,
    watch: WatchPage,
    rename: RenamePage,
    qr: QrPage,
    history: HistoryPage,
    recorder: RecorderPage,
    ocr: OcrPage,
    info: InfoPage,
    about: Arc<Mutex<Option<About>>>,
    inbox: Arc<Mutex<Vec<PathBuf>>>,
    update: Arc<Mutex<UpdateState>>,
    update_dismissed: bool,
    menu_on: bool,
    ytdlp_job: Job,
}

impl ConverApp {
    fn new(cc: &eframe::CreationContext, inbox: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        let cfg = Config::load();
        notify::configure(cfg.notify, cfg.sound);
        let pal = Palette::new(cfg.dark);
        theme::apply(ctx, &pal);

        let logo_img = image::load_from_memory(LOGO_PNG).expect("logo").to_rgba8();
        let logo = ctx.load_texture(
            "logo",
            ColorImage::from_rgba_unmultiplied([logo_img.width() as usize, logo_img.height() as usize], logo_img.as_raw()),
            TextureOptions::LINEAR,
        );

        // informações para a tela de configurações, lidas em segundo plano
        let about: Arc<Mutex<Option<About>>> = Arc::default();
        {
            let (about, ctx) = (about.clone(), ctx.clone());
            std::thread::spawn(move || {
                let info = About { ffmpeg: tools::find_tool("ffmpeg"), ytdlp: download::version() };
                *about.lock().unwrap() = Some(info);
                ctx.request_repaint();
            });
        }

        let mut app = Self {
            download: DownloadPage::new(cfg.download_dir()),
            cfg,
            pal,
            page: Page::Img(ImgTool::Resize),
            logo,
            thumbs: Thumbs::new(ctx),
            durs: Durations::new(ctx),
            images: IMG_TOOLS.iter().map(|t| (*t, ImagePage::new(*t))).collect(),
            medias: MEDIA_TOOLS.iter().map(|t| (*t, MediaPage::new(*t))).collect(),
            pdfs: [PdfTool::Merge, PdfTool::Split].iter().map(|t| (*t, PdfPage::new(*t))).collect(),
            watch: WatchPage::new(),
            rename: RenamePage::new(),
            qr: QrPage::new(),
            history: HistoryPage::new(),
            recorder: RecorderPage::new(),
            ocr: OcrPage::new(),
            info: InfoPage::new(),
            about,
            inbox,
            update: Arc::new(Mutex::new(UpdateState::Idle)),
            update_dismissed: false,
            menu_on: integration::menu_registered(),
            ytdlp_job: Job::default(),
        };
        if app.cfg.check_updates {
            app.check_updates(ctx);
        }
        // arquivos arrastados para o ícone do Conver.exe / "Abrir com"
        app.add_paths(std::env::args_os().skip(1).map(PathBuf::from).collect());
        app
    }

    fn check_updates(&self, ctx: &egui::Context) {
        let (state, ctx) = (self.update.clone(), ctx.clone());
        *state.lock().unwrap() = UpdateState::Checking;
        std::thread::spawn(move || {
            let r = match integration::check_update() {
                Ok(Some(rel)) => UpdateState::Available(rel),
                Ok(None) => UpdateState::UpToDate,
                Err(e) => UpdateState::Failed(e),
            };
            *state.lock().unwrap() = r;
            ctx.request_repaint();
        });
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.cfg.dark = dark;
        self.cfg.save();
        self.pal = Palette::new(dark);
        theme::apply(ctx, &self.pal);
    }

    fn handle_drop(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        self.add_paths(dropped);
    }

    fn queue_of(&mut self, page: Page) -> Option<&mut FileQueue> {
        match page {
            Page::Img(t) => self.images.get_mut(&t).map(|p| &mut p.queue),
            Page::Media(t) => self.medias.get_mut(&t).map(|p| &mut p.queue),
            Page::Pdf(t) => self.pdfs.get_mut(&t).map(|p| &mut p.queue),
            Page::Rename => Some(&mut self.rename.queue),
            Page::Ocr => Some(&mut self.ocr.queue),
            Page::Info => Some(&mut self.info.queue),
            _ => None,
        }
    }

    /// Arquivos soltos na janela (ou abertos com o Conver) vão para a tela atual;
    /// se ela não aceitar, para a ferramenta certa.
    fn add_paths(&mut self, dropped: Vec<PathBuf>) {
        if dropped.is_empty() {
            return;
        }
        // pastas: entram os arquivos de dentro
        let mut paths = Vec::new();
        for p in dropped {
            if p.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&p) {
                    let mut inner: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
                    inner.sort();
                    paths.extend(inner);
                }
            } else {
                paths.push(p);
            }
        }
        let page = self.page;
        let current_accepts = self.queue_of(page).is_some_and(|q| paths.iter().any(|p| q.accepts(p)));
        if !current_accepts {
            let has = |exts: &[&str]| paths.iter().any(|p| exts.contains(&tools::ext_lower(p).as_str()));
            self.page = if has(imaging::IMAGE_EXTS) {
                Page::Img(ImgTool::Convert)
            } else if has(video::VIDEO_EXTS) {
                Page::Media(MTool::VConvert)
            } else if has(media::AUDIO_EXTS) {
                Page::Media(MTool::AConvert)
            } else if has(&["pdf"]) {
                Page::Pdf(PdfTool::Merge)
            } else {
                return;
            };
        }
        let page = self.page;
        if let Some(queue) = self.queue_of(page) {
            let first_new = queue.files.len();
            if queue.add(paths) > 0 {
                queue.selected = first_new;
            }
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let p = self.pal;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((self.logo.id(), vec2(38.0, 38.0))));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Conver").family(bold()).size(24.0).color(p.accent));
                ui.label(RichText::new("conversor universal").size(12.0).color(p.muted));
            });
        });
        ui.add_space(6.0);
        let bottom = 86.0;
        egui::ScrollArea::vertical()
            .id_salt("nav")
            .max_height(ui.available_height() - bottom)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (section, items) in NAV {
                    ui.add_space(8.0);
                    ui.label(RichText::new(*section).family(bold()).size(11.5).color(p.muted));
                    for (page, label) in *items {
                        let label = match page {
                            Page::Watch if self.watch.running() => "Pasta vigiada  ●",
                            Page::Recorder if self.recorder.recording() => "Gravador de áudio  ●",
                            _ => label,
                        };
                        if nav_item(ui, &p, label, self.page == *page) {
                            self.page = *page;
                        }
                    }
                }
                ui.add_space(6.0);
            });
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(4.0);
            if nav_item(ui, &p, "⚙  Configurações", self.page == Page::Settings) {
                self.page = Page::Settings;
            }
            if nav_item(ui, &p, "🕘  Histórico", self.page == Page::History) {
                self.page = Page::History;
            }
            ui.separator();
        });
    }

    fn settings_ui(&mut self, ui: &mut Ui) {
        let p = self.pal;
        widgets::header(ui, &p, "Configurações", "Personalize o Conver.");

        card(ui, &p, |ui| {
            ui.label(RichText::new("Aparência").family(bold()).size(16.0).color(p.text));
            ui.add_space(6.0);
            let mut dark = self.cfg.dark;
            ui.horizontal(|ui| {
                for (value, name) in [(false, "Clara"), (true, "Escura")] {
                    if theme_card(ui, &p, value, name, dark == value) {
                        dark = value;
                    }
                    ui.add_space(8.0);
                }
            });
            if dark != self.cfg.dark {
                self.set_dark(ui.ctx(), dark);
            }
        });
        ui.add_space(4.0);

        let p = self.pal;
        card(ui, &p, |ui| {
            ui.label(RichText::new("Avisos").family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let (mut n, mut snd) = (self.cfg.notify, self.cfg.sound);
            ui.checkbox(&mut n, "Notificação do Windows quando uma tarefa demorada terminar (com o Conver em segundo plano)");
            ui.checkbox(&mut snd, "Tocar um som ao terminar");
            if (n, snd) != (self.cfg.notify, self.cfg.sound) {
                self.cfg.notify = n;
                self.cfg.sound = snd;
                self.cfg.save();
                notify::configure(n, snd);
            }
            ui.horizontal(|ui| {
                if widgets::secondary(ui, &p, "Testar aviso", true) {
                    notify::toast("Conver", "Assim vai aparecer o aviso quando uma tarefa terminar.");
                    notify::finished("teste", true);
                }
                widgets::hint(ui, &p, "a notificação aparece com o nome \"Windows PowerShell\"; é o jeito do Windows para apps sem instalador.");
            });
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new("Downloads").family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            row(ui, &p, "Pasta padrão:", |ui| {
                ui.label(RichText::new(self.cfg.download_dir().display().to_string()).color(p.text));
                if widgets::secondary(ui, &p, "Alterar…", true) {
                    if let Some(d) = rfd::FileDialog::new().set_title("Pasta padrão dos downloads").pick_folder() {
                        self.cfg.download_dir = Some(d.clone());
                        self.cfg.save();
                        self.download.out.path = Some(d);
                    }
                }
            });
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new("Sobre").family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            row(ui, &p, "Versão:", |ui| ui.label(format!("Conver {}", env!("CARGO_PKG_VERSION"))));
            let about = self.about.lock().unwrap();
            let (ff, yt) = match &*about {
                Some(a) => (
                    match &a.ffmpeg {
                        Some(f) if f.starts_with(tools::exe_dir()) => "incluído (bin\\ffmpeg.exe)".into(),
                        Some(f) => f.display().to_string(),
                        None => "não encontrado".into(),
                    },
                    a.ytdlp.clone().unwrap_or_else(|| "não encontrado".into()),
                ),
                None => ("verificando…".into(), "verificando…".into()),
            };
            row(ui, &p, "ffmpeg:", |ui| ui.add(egui::Label::new(ff).truncate()));
            row(ui, &p, "yt-dlp:", |ui| ui.label(yt));
            drop(about);
            row(ui, &p, "Projeto:", |ui| {
                ui.hyperlink_to(integration::repo_url().replace("https://", ""), integration::repo_url());
            });
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new("Integração com o Windows").family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let mut on = self.menu_on;
            ui.checkbox(&mut on, "Mostrar \"Abrir com Conver\" no menu do botão direito (imagens, vídeos, áudios e PDFs)");
            if on != self.menu_on {
                let r = if on { integration::register_menu() } else { integration::unregister_menu(); Ok(()) };
                match r {
                    Ok(()) => self.menu_on = on,
                    Err(e) => {
                        rfd::MessageDialog::new().set_title("Conver").set_description(format!("Não foi possível alterar o menu: {e}")).show();
                    }
                }
            }
            widgets::hint(ui, &p, "No Windows 11, a opção fica em \"Mostrar mais opções\". Se você mover a pasta do Conver, desmarque e marque de novo.");
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new("Atualizações").family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let mut check = self.cfg.check_updates;
            ui.checkbox(&mut check, "Verificar se há uma versão nova do Conver ao abrir (consulta o GitHub)");
            if check != self.cfg.check_updates {
                self.cfg.check_updates = check;
                self.cfg.save();
            }
            ui.horizontal(|ui| {
                let state = self.update.lock().unwrap();
                let checking = matches!(*state, UpdateState::Checking);
                let (text, color) = match &*state {
                    UpdateState::Idle => (String::new(), p.muted),
                    UpdateState::Checking => ("verificando…".into(), p.muted),
                    UpdateState::UpToDate => (format!("✔ você está na versão mais nova ({})", env!("CARGO_PKG_VERSION")), p.ok),
                    UpdateState::Available(r) => (format!("versão {} disponível", r.version), p.accent),
                    UpdateState::Failed(e) => (format!("✖ {e}"), p.accent),
                };
                let url = match &*state {
                    UpdateState::Available(r) => Some(r.url.clone()),
                    _ => None,
                };
                drop(state);
                if widgets::secondary(ui, &p, "Verificar agora", !checking) {
                    self.update_dismissed = false;
                    self.check_updates(ui.ctx());
                }
                if let Some(url) = url {
                    if widgets::primary(ui, &p, "Baixar", true) {
                        let _ = std::process::Command::new("explorer").arg(url).spawn();
                    }
                }
                ui.label(RichText::new(text).color(color));
            });
            ui.add_space(8.0);
            ui.label(RichText::new("yt-dlp (motor dos downloads)").family(bold()).color(p.text));
            widgets::hint(ui, &p, "Os sites mudam com frequência. Se os downloads pararem de funcionar, atualize o yt-dlp (baixa a versão oficial do GitHub e confere a assinatura).");
            match widgets::action_bar(ui, &p, &self.ytdlp_job, "Atualizar yt-dlp", true) {
                widgets::Act::Start => {
                    let about = self.about.clone();
                    self.ytdlp_job.start(ui.ctx(), move |ctl| {
                        ctl.status("Baixando a versão mais nova do yt-dlp…");
                        match integration::update_ytdlp(&|f| ctl.progress(f)) {
                            Ok((before, after)) => {
                                ctl.progress(1.0);
                                let text = if before == after {
                                    format!("Você já tinha a versão mais nova ({after}).")
                                } else {
                                    format!("Atualizado: {before} → {after}.")
                                };
                                ctl.log(job::LogKind::Ok, format!("✔ {text}"));
                                ctl.status(text);
                                if let Some(a) = about.lock().unwrap().as_mut() {
                                    a.ytdlp = Some(after);
                                }
                            }
                            Err(e) => {
                                ctl.log(job::LogKind::Err, format!("✖ {e}"));
                                ctl.status("Erro ao atualizar.");
                            }
                        }
                    });
                }
                widgets::Act::Cancel => self.ytdlp_job.cancel(),
                widgets::Act::None => {}
            }
        });
    }
}

impl ConverApp {
    fn update_banner(&mut self, ui: &mut Ui) {
        let p = self.pal;
        let rel = match &*self.update.lock().unwrap() {
            UpdateState::Available(r) if !self.update_dismissed => (r.version.clone(), r.url.clone()),
            _ => return,
        };
        Frame::default()
            .fill(p.accent_soft)
            .stroke(Stroke::new(1.0f32, p.accent))
            .corner_radius(10)
            .inner_margin(Margin::symmetric(14, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("Nova versão do Conver disponível: {}", rel.0)).family(bold()).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::secondary(ui, &p, "Agora não", true) {
                            self.update_dismissed = true;
                        }
                        if widgets::primary(ui, &p, "Baixar", true) {
                            let _ = std::process::Command::new("explorer").arg(&rel.1).spawn();
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }
}

fn nav_item(ui: &mut Ui, p: &Palette, label: &str, selected: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 31.0), Sense::click());
    let fill = if selected {
        p.accent
    } else if resp.hovered() {
        p.card
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8, fill);
    ui.painter().text(
        rect.left_center() + vec2(14.0, 0.0),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.5),
        if selected { p.on_accent } else { p.text },
    );
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// Cartão com uma miniatura do tema.
fn theme_card(ui: &mut Ui, current: &Palette, dark: bool, name: &str, selected: bool) -> bool {
    let t = Palette::new(dark);
    let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 130.0), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 12, t.bg);
    let side = egui::Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(46.0, rect.height() - 40.0));
    painter.rect_filled(side, 6, t.sidebar);
    painter.rect_filled(egui::Rect::from_min_size(side.min + vec2(6.0, 10.0), vec2(34.0, 8.0)), 3, t.accent);
    for i in 0..3 {
        painter.rect_filled(egui::Rect::from_min_size(side.min + vec2(6.0, 26.0 + i as f32 * 12.0), vec2(30.0, 6.0)), 3, t.muted);
    }
    let main = egui::Rect::from_min_max(egui::pos2(side.right() + 8.0, side.top()), egui::pos2(rect.right() - 8.0, side.bottom()));
    painter.rect_filled(main, 6, t.card);
    painter.rect_filled(egui::Rect::from_min_size(main.min + vec2(8.0, 10.0), vec2(70.0, 8.0)), 3, t.text);
    painter.rect_filled(egui::Rect::from_min_size(main.min + vec2(8.0, 26.0), vec2(100.0, 6.0)), 3, t.muted);
    painter.rect_filled(egui::Rect::from_min_size(main.right_bottom() - vec2(50.0, 20.0), vec2(42.0, 12.0)), 4, t.accent);
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 16.0),
        Align2::LEFT_CENTER,
        name,
        FontId::new(14.0, bold()),
        t.text,
    );
    let stroke = if selected {
        Stroke::new(2.5f32, current.accent)
    } else if resp.hovered() {
        Stroke::new(1.5f32, current.muted)
    } else {
        Stroke::new(1.0f32, current.border)
    };
    painter.rect_stroke(rect, 12, stroke, egui::StrokeKind::Inside);
    if selected {
        painter.circle_filled(rect.right_bottom() - vec2(18.0, 16.0), 9.0, current.accent);
        painter.text(rect.right_bottom() - vec2(18.0, 16.0), Align2::CENTER_CENTER, "✔", FontId::proportional(11.0), Color32::WHITE);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

impl eframe::App for ConverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_drop(ctx);
        // arquivos enviados por outra instância (menu do botão direito com o Conver já aberto)
        let received: Vec<PathBuf> = std::mem::take(&mut *self.inbox.lock().unwrap());
        if !received.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.add_paths(received.into_iter().filter(|p| !p.as_os_str().is_empty()).collect());
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        notify::set_focused(ctx.input(|i| i.viewport().focused.unwrap_or(true)));
        let p = self.pal;

        egui::SidePanel::left("nav")
            .exact_width(232.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::default().fill(p.sidebar).inner_margin(Margin::symmetric(14, 16)))
            .show(ctx, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(Frame::default().fill(p.bg).inner_margin(Margin::symmetric(26, 20)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_width(ui.available_width() - 6.0);
                    self.update_banner(ui);
                    match self.page {
                        Page::Img(t) => {
                            if let Some(page) = self.images.get_mut(&t) {
                                page.ui(ui, &p, &self.thumbs);
                            }
                        }
                        Page::Media(t) => {
                            if let Some(page) = self.medias.get_mut(&t) {
                                page.ui(ui, &p, &self.durs);
                            }
                        }
                        Page::Download => self.download.ui(ui, &p),
                        Page::Pdf(t) => {
                            if let Some(page) = self.pdfs.get_mut(&t) {
                                page.ui(ui, &p);
                            }
                        }
                        Page::Watch => self.watch.ui(ui, &p),
                        Page::Rename => self.rename.ui(ui, &p),
                        Page::Qr => self.qr.ui(ui, &p),
                        Page::History => self.history.ui(ui, &p),
                        Page::Recorder => self.recorder.ui(ui, &p),
                        Page::Ocr => self.ocr.ui(ui, &p, &self.thumbs),
                        Page::Info => self.info.ui(ui, &p),
                        Page::Settings => self.settings_ui(ui),
                    }
                });
            });

        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            widgets::paint_drop_overlay(ctx, &self.pal);
        }
    }
}

fn main() -> eframe::Result {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    // usados pelo instalador
    match args.first().and_then(|a| a.to_str()) {
        Some("--registrar-menu") => {
            let _ = integration::register_menu();
            return Ok(());
        }
        Some("--remover-menu") => {
            integration::unregister_menu();
            return Ok(());
        }
        _ => {}
    }
    let inbox: Arc<Mutex<Vec<PathBuf>>> = Arc::default();
    if integration::single_instance(&args, inbox.clone()) {
        return Ok(()); // o Conver já estava aberto e recebeu os arquivos
    }
    let icon = eframe::icon_data::from_png_bytes(LOGO_PNG).unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Conver")
            .with_inner_size([1140.0, 740.0])
            .with_min_inner_size([920.0, 600.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native("Conver", options, Box::new(move |cc| Ok(Box::new(ConverApp::new(cc, inbox)))))
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_new;
#[cfg(test)]
mod tests_new2;
#[cfg(test)]
mod tests_v1;
