#![windows_subsystem = "windows"]
//! Conver — conversor universal.

mod ai;
mod bgremove;
mod captura;
mod cli;
mod download;
mod extras;
mod extras2;
mod extras3;
mod extras4;
mod history;
mod i18n;
mod imaging;
mod info;
mod integration;
mod job;
mod media;
mod media_extra;
mod media_page;
mod notify;
mod ocr;
mod ops;
mod pages;
mod pdf;
mod pdf_extra;
mod presets;
mod qr;
mod recorder;
mod rename;
mod screenrec;
mod search;
mod separate;
mod theme;
mod tools;
mod transcribe;
mod updater;
mod upscale;
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
use extras4::{PresetsPage, ScreenPage, SeparatePage, TranscribePage};
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
    lang: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { dark: false, download_dir: None, notify: true, sound: true, check_updates: true, lang: i18n::system_default().into() }
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
    Transcribe,
    Separate,
    Screen,
    Presets,
}

const IMG_TOOLS: [ImgTool; 10] = [
    ImgTool::Resize,
    ImgTool::Compress,
    ImgTool::Quality,
    ImgTool::Convert,
    ImgTool::Transform,
    ImgTool::Strip,
    ImgTool::Pdf,
    ImgTool::Adjust,
    ImgTool::RemoveBg,
    ImgTool::Upscale,
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
            (Page::Img(ImgTool::RemoveBg), "Remover fundo"),
            (Page::Img(ImgTool::Upscale), "Ampliar com IA"),
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
            (Page::Screen, "Gravar a tela"),
        ],
    ),
    (
        "ÁUDIO",
        &[
            (Page::Recorder, "Gravador de áudio"),
            (Page::Transcribe, "Transcrever / legendar"),
            (Page::Separate, "Separar voz e instrumental"),
            (Page::Media(MTool::AConvert), "Converter formato"),
            (Page::Media(MTool::ATrim), "Cortar áudio"),
            (Page::Media(MTool::AMerge), "Juntar áudios"),
            (Page::Media(MTool::Normalize), "Normalizar volume"),
            (Page::Media(MTool::ACompress), "Comprimir"),
        ],
    ),
    (
        "PDF",
        &[
            (Page::Pdf(PdfTool::Merge), "Juntar PDFs"),
            (Page::Pdf(PdfTool::Split), "Dividir PDF"),
            (Page::Pdf(PdfTool::Compress), "Comprimir PDF"),
            (Page::Pdf(PdfTool::ToImages), "PDF → imagens"),
            (Page::Pdf(PdfTool::Ocr), "PDF pesquisável (OCR)"),
            (Page::Pdf(PdfTool::Protect), "Proteger com senha"),
            (Page::Pdf(PdfTool::Unlock), "Tirar senha"),
        ],
    ),
    ("DOWNLOAD", &[(Page::Download, "Vídeos e músicas")]),
    (
        "FERRAMENTAS",
        &[
            (Page::Presets, "Predefinições"),
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
    transcribe: TranscribePage,
    separate: SeparatePage,
    screen: ScreenPage,
    presets: PresetsPage,
    about: Arc<Mutex<Option<About>>>,
    inbox: Arc<Mutex<Vec<PathBuf>>>,
    update: Arc<Mutex<UpdateState>>,
    update_dismissed: bool,
    update_job: Job,
    nav_query: String,
    update_ready: Arc<Mutex<Option<updater::Prepared>>>,
    menu_on: bool,
    ytdlp_job: Job,
    captura: Option<captura::Captura>,
}

impl ConverApp {
    fn new(cc: &eframe::CreationContext, inbox: Arc<Mutex<Vec<PathBuf>>>) -> Self {
        let ctx = &cc.egui_ctx;
        theme::install_fonts(ctx);
        let cfg = Config::load();
        i18n::set(&cfg.lang);
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
            pdfs: PdfTool::ALL.iter().map(|t| (*t, PdfPage::new(*t))).collect(),
            watch: WatchPage::new(),
            rename: RenamePage::new(),
            qr: QrPage::new(),
            history: HistoryPage::new(),
            recorder: RecorderPage::new(),
            ocr: OcrPage::new(),
            info: InfoPage::new(),
            transcribe: TranscribePage::new(),
            separate: SeparatePage::new(),
            screen: ScreenPage::new(),
            presets: PresetsPage::new(),
            about,
            inbox,
            update: Arc::new(Mutex::new(UpdateState::Idle)),
            update_dismissed: false,
            update_job: Job::default(),
            nav_query: String::new(),
            update_ready: Arc::default(),
            menu_on: integration::menu_registered(),
            ytdlp_job: Job::default(),
            captura: captura::from_env(),
        };
        if app.captura.is_some() {
            app.add_paths(captura::arquivos());
            app.nav_query = std::env::var("CONVER_BUSCA").unwrap_or_default();
        } else if app.cfg.check_updates {
            app.check_updates(ctx);
        }
        std::thread::spawn(updater::cleanup_old);
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

    /// Modo de captura (só na depuração, veja `captura.rs`): aplica cada cena, espera, captura a janela e fecha no fim.
    fn captura_frame(&mut self, ctx: &egui::Context) {
        let Some(mut cap) = self.captura.take() else { return };
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            captura::salvar(&cap.dir, &cap.cenas[cap.atual], cap.atual, &img);
            cap.atual += 1;
            cap.quadros = 0;
            cap.pedida = false;
            if cap.atual >= cap.cenas.len() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
        }
        if cap.quadros == 0 {
            let cena = &cap.cenas[cap.atual];
            self.cfg.dark = cena.dark; // sem salvar
            self.pal = Palette::new(cena.dark);
            theme::apply(ctx, &self.pal);
            i18n::set(&cena.lang);
            self.page = match cena.page.as_str() {
                "settings" => Page::Settings,
                "history" => Page::History,
                id => NAV.iter().flat_map(|(_, items)| items.iter()).map(|(p, _)| *p).find(|p| page_id(*p) == id).unwrap_or(self.page),
            };
            if self.page == Page::Recorder && std::env::var_os("CONVER_DEMO_AUDIO").is_some() {
                self.recorder.demo(ctx);
            }
            if self.page == Page::Screen {
                self.screen.demo();
            }
        }
        cap.quadros += 1;
        if cap.quadros >= captura::espera() && !cap.pedida {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            cap.pedida = true;
        }
        ctx.request_repaint();
        self.captura = Some(cap);
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
            Page::Transcribe => Some(&mut self.transcribe.queue),
            Page::Separate => Some(&mut self.separate.queue),
            Page::Presets => Some(&mut self.presets.queue),
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
                ui.label(RichText::new(crate::i18n::tr("Conver")).family(bold()).size(24.0).color(p.accent));
                ui.label(RichText::new(crate::i18n::tr("conversor universal")).size(12.0).color(p.muted));
            });
        });
        ui.add_space(8.0);
        // busca: nome, seção e sinônimos ("cortar", "tirar fundo", "youtube"…); Ctrl+F foca, Enter abre a primeira
        let search_id = egui::Id::new("nav_search");
        if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
            ui.memory_mut(|m| m.request_focus(search_id));
        }
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.nav_query).margin(crate::widgets::FIELD_MARGIN)
                .id(search_id)
                .hint_text(crate::i18n::tr("🔍  Buscar ferramenta…"))
                .desired_width(f32::INFINITY),
        )
        .on_hover_text("Ctrl+F");
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.nav_query.clear();
        }
        let query = search::normalize(self.nav_query.trim());
        let searching = !query.trim().is_empty();
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let bottom = 86.0;
        egui::ScrollArea::vertical()
            .id_salt("nav")
            .max_height(ui.available_height() - bottom)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let mut first: Option<Page> = None;
                for (section, items) in NAV {
                    let shown: Vec<&(Page, &str)> = items
                        .iter()
                        .filter(|(page, label)| {
                            !searching || {
                                let hay = search::haystack(label, &crate::i18n::tr(*label), section, &crate::i18n::tr(*section), search::keywords(page_id(*page)));
                                search::matches(&query, &hay)
                            }
                        })
                        .collect();
                    if shown.is_empty() {
                        continue;
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(crate::i18n::tr(*section)).family(bold()).size(11.5).color(p.muted));
                    for (page, label) in shown {
                        first.get_or_insert(*page);
                        let label = match page {
                            Page::Watch if self.watch.running() => "Pasta vigiada  ●",
                            Page::Recorder if self.recorder.recording() => "Gravador de áudio  ●",
                            Page::Screen if self.screen.recording() => "Gravar a tela  ●",
                            _ => label,
                        };
                        if nav_item(ui, &p, label, self.page == *page) {
                            self.page = *page;
                        }
                    }
                }
                if searching {
                    match first {
                        None => {
                            ui.add_space(10.0);
                            ui.label(RichText::new(crate::i18n::tr("Nenhuma ferramenta encontrada.")).color(p.muted));
                        }
                        Some(page) if enter => {
                            self.page = page;
                            self.nav_query.clear();
                        }
                        _ => {}
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
            ui.label(RichText::new(crate::i18n::tr("Idioma")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let current = i18n::current();
            let label = |l: &i18n::Lang| {
                if l.code == "pt" { l.name.to_string() } else { format!("{}  ({})", l.name, crate::i18n::tr(l.pt_name)) }
            };
            let mut chosen = current.code;
            egui::ComboBox::from_id_salt("lang").selected_text(crate::i18n::tr(label(current))).width(300.0).show_ui(ui, |ui| {
                for l in i18n::LANGS.iter() {
                    ui.selectable_value(&mut chosen, l.code, crate::i18n::tr(label(l)));
                }
            });
            if chosen != current.code {
                i18n::set(chosen);
                self.cfg.lang = chosen.to_string();
                self.cfg.save();
            }
            widgets::hint(ui, &p, "Os 20 idiomas mais falados do mundo. Nomes de arquivos e mensagens do FFmpeg e do yt-dlp não são traduzidos.");
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Aparência")).family(bold()).size(16.0).color(p.text));
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
            ui.label(RichText::new(crate::i18n::tr("Avisos")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let (mut n, mut snd) = (self.cfg.notify, self.cfg.sound);
            ui.checkbox(&mut n, crate::i18n::tr("Notificação do Windows quando uma tarefa demorada terminar (com o Conver em segundo plano)"));
            ui.checkbox(&mut snd, crate::i18n::tr("Tocar um som ao terminar"));
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
            ui.label(RichText::new(crate::i18n::tr("Downloads")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            row(ui, &p, "Pasta padrão:", |ui| {
                ui.label(RichText::new(crate::i18n::tr(self.cfg.download_dir().display().to_string())).color(p.text));
                if widgets::secondary(ui, &p, "Alterar…", true) {
                    if let Some(d) = rfd::FileDialog::new().set_title(crate::i18n::tr("Pasta padrão dos downloads")).pick_folder() {
                        self.cfg.download_dir = Some(d.clone());
                        self.cfg.save();
                        self.download.out.path = Some(d);
                    }
                }
            });
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Sobre")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            row(ui, &p, "Versão:", |ui| ui.label(crate::i18n::tr(format!("Conver {}", env!("CARGO_PKG_VERSION")))));
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
            row(ui, &p, "ffmpeg:", |ui| ui.add(egui::Label::new(crate::i18n::tr(ff)).truncate()));
            row(ui, &p, "yt-dlp:", |ui| ui.label(crate::i18n::tr(yt)));
            drop(about);
            row(ui, &p, "Projeto:", |ui| {
                ui.hyperlink_to(crate::i18n::tr(integration::repo_url().replace("https://", "")), integration::repo_url());
            });
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Automação (agentes de IA e linha de comando)")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            widgets::hint(ui, &p, "Agentes de IA (Claude, Cursor, VS Code…) podem usar todas as ferramentas do Conver pelo MCP. Cole a configuração abaixo no arquivo de MCP do seu agente.");
            let cli = tools::exe_dir().join("conver-cli.exe");
            row(ui, &p, "Programa:", |ui| ui.add(egui::Label::new(RichText::new(cli.display().to_string()).color(p.text)).truncate()));
            let snippet = serde_json::to_string_pretty(&serde_json::json!({ "mcpServers": { "conver": { "command": cli.display().to_string(), "args": ["mcp"] } } })).unwrap_or_default();
            ui.add(egui::TextEdit::multiline(&mut snippet.as_str()).code_editor().desired_rows(6).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if widgets::secondary(ui, &p, "Copiar configuração do MCP", true) {
                    let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(snippet.clone()));
                }
                if widgets::secondary(ui, &p, "Copiar exemplo de linha de comando", true) {
                    let ex = format!("\"{}\" image_convert foto.png --format webp", cli.display());
                    let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(ex));
                }
            });
            if !cli.is_file() {
                widgets::hint(ui, &p, "⚠ conver-cli.exe não está ao lado do Conver.exe (ele vem no instalador e na versão portátil).");
            }
            widgets::hint(ui, &p, "Na linha de comando: conver-cli help  •  conver-cli <ferramenta> arquivos… --opção valor");
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Integração com o Windows")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let mut on = self.menu_on;
            ui.checkbox(&mut on, crate::i18n::tr("Mostrar \"Abrir com Conver\" no menu do botão direito (imagens, vídeos, áudios e PDFs)"));
            if on != self.menu_on {
                let r = if on { integration::register_menu() } else { integration::unregister_menu(); Ok(()) };
                match r {
                    Ok(()) => self.menu_on = on,
                    Err(e) => {
                        rfd::MessageDialog::new().set_title(crate::i18n::tr("Conver")).set_description(crate::i18n::tr(format!("Não foi possível alterar o menu: {e}"))).show();
                    }
                }
            }
            widgets::hint(ui, &p, "No Windows 11, a opção fica em \"Mostrar mais opções\". Se você mover a pasta do Conver, desmarque e marque de novo.");
        });
        ui.add_space(4.0);

        card(ui, &p, |ui| {
            ui.label(RichText::new(crate::i18n::tr("Atualizações")).family(bold()).size(16.0).color(p.text));
            ui.add_space(4.0);
            let mut check = self.cfg.check_updates;
            ui.checkbox(&mut check, crate::i18n::tr("Verificar se há uma versão nova do Conver ao abrir (consulta o GitHub)"));
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
                let rel = match &*state {
                    UpdateState::Available(r) => Some(r.clone()),
                    _ => None,
                };
                drop(state);
                if widgets::secondary(ui, &p, "Verificar agora", !checking && !self.update_job.state().running) {
                    self.update_dismissed = false;
                    self.check_updates(ui.ctx());
                }
                ui.label(RichText::new(crate::i18n::tr(text)).color(color));
                if let Some(rel) = rel {
                    self.update_controls(ui, &rel);
                }
            });
            let how = match updater::mode() {
                updater::Mode::Instalado => "\"Atualizar agora\" baixa o instalador novo, confere a assinatura e instala sem perguntas: o Conver fecha e abre de novo sozinho, com as mesmas configurações.",
                updater::Mode::Portatil => "\"Atualizar agora\" baixa a versão portátil nova, confere a assinatura e troca os arquivos desta pasta (a pasta dados é mantida): o Conver fecha e abre de novo sozinho.",
                updater::Mode::Manual => "Esta cópia não foi instalada pelo instalador nem é a versão portátil: baixe a versão nova pelo site.",
            };
            widgets::hint(ui, &p, how);
            ui.add_space(8.0);
            ui.label(RichText::new(crate::i18n::tr("yt-dlp (motor dos downloads)")).family(bold()).color(p.text));
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
    /// Baixa e confere a atualização em segundo plano; quando estiver pronta, `poll_update` instala.
    fn start_update(&mut self, ctx: &egui::Context, rel: integration::Release) {
        let ready = self.update_ready.clone();
        let mode = updater::mode();
        self.update_job.start(ctx, move |ctl| {
            ctl.status(format!("Baixando o Conver {}…", rel.version));
            match updater::prepare(&rel, mode, &|f| ctl.progress(f), &|| ctl.cancelled()) {
                Ok(prep) => {
                    ctl.status("Instalando… o Conver vai fechar e abrir de novo sozinho.");
                    *ready.lock().unwrap() = Some(prep);
                }
                Err(e) => {
                    ctl.log(job::LogKind::Err, format!("✖ {e}"));
                    ctl.status(e);
                }
            }
        });
    }

    /// Atualização pronta: inicia a instalação e fecha o app (ele é reaberto no fim).
    fn poll_update(&mut self, ctx: &egui::Context) {
        let Some(prep) = self.update_ready.lock().unwrap().take() else { return };
        match updater::apply(&prep) {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => {
                rfd::MessageDialog::new().set_title(crate::i18n::tr("Conver")).set_description(crate::i18n::tr(e)).show();
            }
        }
    }

    /// "Atualizar agora" (ou "Baixar", quando esta cópia não se atualiza sozinha) e o andamento.
    fn update_controls(&mut self, ui: &mut Ui, rel: &integration::Release) {
        let p = self.pal;
        let (running, progress, status) = {
            let st = self.update_job.state();
            let err = st.log.iter().any(|(k, _)| *k == job::LogKind::Err);
            (st.running, st.progress, (!st.running && err).then(|| st.status.clone()).or(st.running.then(|| st.status.clone())))
        };
        if running {
            if widgets::secondary(ui, &p, "Cancelar", true) {
                self.update_job.cancel();
            }
            ui.add(egui::ProgressBar::new(progress).desired_width(160.0).show_percentage());
        } else if updater::mode() == updater::Mode::Manual {
            if widgets::primary(ui, &p, "Baixar", true) {
                let _ = std::process::Command::new("explorer").arg(&rel.url).spawn();
            }
        } else if widgets::primary(ui, &p, "Atualizar agora", true) {
            self.start_update(ui.ctx(), rel.clone());
        }
        if let Some(text) = status {
            ui.label(RichText::new(crate::i18n::tr(text)).size(12.5).color(if running { p.muted } else { p.accent }));
        }
        if running {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
    }

    fn update_banner(&mut self, ui: &mut Ui) {
        let p = self.pal;
        self.poll_update(ui.ctx());
        let rel = match &*self.update.lock().unwrap() {
            UpdateState::Available(r) if !self.update_dismissed || self.update_job.state().running => r.clone(),
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
                    ui.label(RichText::new(crate::i18n::tr(format!("Nova versão do Conver disponível: {}", rel.version))).family(bold()).color(p.text));
                    ui.hyperlink_to(crate::i18n::tr("O que mudou"), &rel.url);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.update_job.state().running && widgets::secondary(ui, &p, "Agora não", true) {
                            self.update_dismissed = true;
                        }
                        self.update_controls(ui, &rel);
                    });
                });
            });
        ui.add_space(8.0);
    }
}

/// Nome de cada tela para as palavras de busca (`search::keywords`).
fn page_id(page: Page) -> &'static str {
    match page {
        Page::Img(t) => match t {
            ImgTool::Resize => "img_resize",
            ImgTool::Compress => "img_compress",
            ImgTool::Quality => "img_quality",
            ImgTool::Convert => "img_convert",
            ImgTool::Transform => "img_transform",
            ImgTool::Strip => "img_strip",
            ImgTool::Pdf => "img_pdf",
            ImgTool::Adjust => "img_adjust",
            ImgTool::RemoveBg => "img_removebg",
            ImgTool::Upscale => "img_upscale",
        },
        Page::Media(t) => match t {
            MTool::VConvert => "v_convert",
            MTool::VCompress => "v_compress",
            MTool::VTrim => "v_trim",
            MTool::Merge => "v_merge",
            MTool::Speed => "v_speed",
            MTool::AudioFrames => "v_audioframes",
            MTool::Gif => "v_gif",
            MTool::VTransform => "v_transform",
            MTool::Subtitles => "v_subs",
            MTool::Music => "v_music",
            MTool::AConvert => "a_convert",
            MTool::ATrim => "a_trim",
            MTool::AMerge => "a_merge",
            MTool::Normalize => "a_normalize",
            MTool::ACompress => "a_compress",
        },
        Page::Pdf(t) => match t {
            PdfTool::Merge => "pdf_merge",
            PdfTool::Split => "pdf_split",
            PdfTool::Compress => "pdf_compress",
            PdfTool::ToImages => "pdf_images",
            PdfTool::Protect => "pdf_protect",
            PdfTool::Unlock => "pdf_unlock",
            PdfTool::Ocr => "pdf_ocr",
        },
        Page::Download => "download",
        Page::Watch => "watch",
        Page::Recorder => "recorder",
        Page::Ocr => "ocr",
        Page::Info => "info",
        Page::Rename => "rename",
        Page::Qr => "qr",
        Page::Transcribe => "transcribe",
        Page::Separate => "separate",
        Page::Screen => "screen",
        Page::Presets => "presets",
        Page::History | Page::Settings => "",
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
        crate::i18n::tr(label),
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
        crate::i18n::tr(name),
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
        painter.text(rect.right_bottom() - vec2(18.0, 16.0), Align2::CENTER_CENTER, crate::i18n::tr("✔"), FontId::proportional(11.0), Color32::WHITE);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

impl eframe::App for ConverApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.captura_frame(ctx);
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
                        Page::Transcribe => self.transcribe.ui(ui, &p, &self.durs),
                        Page::Separate => self.separate.ui(ui, &p, &self.durs),
                        Page::Screen => self.screen.ui(ui, &p),
                        Page::Presets => self.presets.ui(ui, &p),
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
    // linha de comando e MCP (chamados pelo conver-cli.exe, que repassa o terminal)
    if args.first().and_then(|a| a.to_str()) == Some("--cli") {
        if let Ok(cfg) = std::fs::read_to_string(tools::app_dir().join("config.json")) {
            if let Some(lang) = serde_json::from_str::<serde_json::Value>(&cfg).ok().and_then(|v| v["lang"].as_str().map(String::from)) {
                i18n::set(&lang);
            }
        }
        let rest: Vec<String> = args[1..].iter().map(|a| a.to_string_lossy().into_owned()).collect();
        std::process::exit(cli::main(rest));
    }
    // atualização da versão portátil: este é o Conver.exe novo, rodando da pasta temporária
    if args.first().and_then(|a| a.to_str()) == Some("--aplicar-atualizacao") {
        let rest: Vec<String> = args[1..].iter().map(|a| a.to_string_lossy().into_owned()).collect();
        std::process::exit(updater::run_apply(&rest));
    }
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
    if !captura::active() && integration::single_instance(&args, inbox.clone()) {
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
#[cfg(test)]
mod tests_v11;
#[cfg(test)]
mod tests_v12;
#[cfg(test)]
mod tests_v13;
#[cfg(test)]
mod tests_v14;
