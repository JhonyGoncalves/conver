//! Interface 2.0 (o padrão; a clássica fica nas Configurações): começa pelo arquivo, busca central (Ctrl+K), fila
//! com todas as tarefas e visual do Windows 11 (cor de destaque e tema do sistema, ícones Segoe). Os componentes de
//! `widgets` mudam de estilo junto (`widgets::fluent`). `CONVER_INTERFACE=2` ou `=1` força uma das duas.

use std::path::PathBuf;

use eframe::egui::{self, Align, Align2, Color32, CursorIcon, FontFamily, FontId, Frame, Margin, RichText, Sense, Stroke, StrokeKind, Ui, vec2};

use super::{ConverApp, NAV, Page, page_id};
use crate::theme::{Palette, bold};
use crate::{search, tools, widgets};

static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A interface 2.0 está ligada.
pub fn enabled() -> bool {
    ON.load(std::sync::atomic::Ordering::Relaxed)
}

/// Liga ou desliga a interface 2.0 (vale na hora).
pub fn set_enabled(on: bool) {
    ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Decide pela configuração (`interface`: 1 = clássica), a não ser que `CONVER_INTERFACE` force, e já aplica.
pub fn decide(interface: u8) -> bool {
    // i18n: fora
    let on = match std::env::var("CONVER_INTERFACE").as_deref().map(str::trim) {
        Ok("2") => true,
        Ok("1") => false,
        _ => interface != 1,
    };
    // i18n: dentro
    set_enabled(on);
    on
}

/// Ferramenta pelo id (`main::page_id`), para lembrar as recentes entre uma abertura e outra.
pub fn page_by_id(id: &str) -> Option<Page> {
    all_tools().map(|(p, _, _)| p).find(|p| page_id(*p) == id)
}

/// Estado da casca nova.
#[derive(Default)]
pub struct State {
    /// Arquivos soltos ou colados na tela inicial, esperando a escolha da ferramenta.
    pub files: Vec<PathBuf>,
    /// Busca central aberta: texto e item escolhido.
    pub palette: Option<(String, usize)>,
    pub queue_open: bool,
    /// Ferramentas usadas por último (a mais recente primeiro).
    pub recent: Vec<Page>,
}

// ------------------------------------------------------------------ ícones e cores

/// Ícones da fonte de símbolos do Windows (Segoe Fluent Icons no 11, Segoe MDL2 Assets no 10): mesmos códigos.
pub mod icon {
    pub const HOME: char = '\u{E80F}';
    pub const SEARCH: char = '\u{E721}';
    pub const APPS: char = '\u{E71D}';
    pub const QUEUE: char = '\u{E8FD}';
    pub const HISTORY: char = '\u{E81C}';
    pub const SETTINGS: char = '\u{E713}';
    pub const UPLOAD: char = '\u{E898}';
    pub const CLOSE: char = '\u{E711}';
    pub const CHECK: char = '\u{E73E}';
    pub const ERROR: char = '\u{E783}';
    /// Um por seção do menu, na ordem de `NAV`.
    pub const SECTIONS: [char; 7] = ['\u{E895}', '\u{EB9F}', '\u{E714}', '\u{E8D6}', '\u{E8A5}', '\u{E896}', '\u{E90F}'];
}

pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// Põe a fonte de ícones do Windows, se houver, nas fontes que vão ser instaladas (chamado por `theme::install_fonts`).
pub fn add_icons(fonts: &mut egui::FontDefinitions) {
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let Some(bytes) = ["SegoeIcons.ttf", "segmdl2.ttf"].iter().find_map(|f| std::fs::read(format!("{windir}\\Fonts\\{f}")).ok()) else { return };
    fonts.font_data.insert("icons".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    fonts.families.insert(icons(), vec!["icons".into()]);
}

fn reg_dword(path: &str, name: &str) -> Option<u32> {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(path).ok()?.get_value::<u32, _>(name).ok()
}

/// Tema escuro do Windows ("modo dos aplicativos").
pub fn system_dark() -> bool {
    reg_dword(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme") == Some(0)
}

/// Cor de destaque escolhida no Windows (azul padrão se não houver).
fn system_accent() -> Color32 {
    // o registro guarda 0xAABBGGRR
    reg_dword(r"Software\Microsoft\Windows\DWM", "AccentColor")
        .map(|v| Color32::from_rgb(v as u8, (v >> 8) as u8, (v >> 16) as u8))
        .unwrap_or(Color32::from_rgb(0x00, 0x78, 0xD4))
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Cores do Fluent (Windows 11) com o destaque do sistema.
pub fn palette(dark: bool) -> Palette {
    let accent = system_accent();
    if dark {
        let bg = Color32::from_rgb(0x20, 0x20, 0x20);
        Palette {
            dark,
            bg,
            sidebar: Color32::from_rgb(0x1A, 0x1A, 0x1A),
            card: Color32::from_rgb(0x2B, 0x2B, 0x2B),
            input: Color32::from_rgb(0x2D, 0x2D, 0x2D),
            border: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            text: Color32::WHITE,
            muted: Color32::from_rgb(0xC5, 0xC5, 0xC5),
            accent: mix(accent, Color32::WHITE, 0.45),
            accent_soft: mix(bg, accent, 0.3),
            action: accent,
            on_accent: Color32::WHITE,
            ok: Color32::from_rgb(0x6C, 0xCB, 0x5F),
        }
    } else {
        let bg = Color32::from_rgb(0xF3, 0xF3, 0xF3);
        Palette {
            dark,
            bg,
            sidebar: Color32::from_rgb(0xEB, 0xEB, 0xEB),
            card: Color32::from_rgb(0xFB, 0xFB, 0xFB),
            input: Color32::WHITE,
            border: Color32::from_rgb(0xE0, 0xE0, 0xE0),
            text: Color32::from_rgb(0x1B, 0x1B, 0x1B),
            muted: Color32::from_rgb(0x5D, 0x5D, 0x5D),
            accent: mix(accent, Color32::BLACK, 0.2),
            accent_soft: mix(bg, accent, 0.18),
            action: accent,
            on_accent: Color32::WHITE,
            ok: Color32::from_rgb(0x0F, 0x7B, 0x0F),
        }
    }
}

/// Temas da interface 2.0: id (guardado em `Config.theme`) e nome (chave de tradução).
pub const THEMES: [(&str, &str); 9] = [
    ("system", "Igual ao Windows"),
    ("light", "Clara"),
    ("dark", "Escura"),
    ("conver", "Conver"),
    ("midnight", "Meia-noite"),
    ("forest", "Floresta"),
    ("amethyst", "Ametista"),
    ("sand", "Areia"),
    ("contrast", "Alto contraste"),
];

fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Tema de cores fixas: fundo, barra lateral, cartão, campo, borda, texto, texto apagado, destaque, botão principal,
/// texto sobre o botão e verde de sucesso.
#[allow(clippy::too_many_arguments)]
fn fixed(dark: bool, bg: u32, sidebar: u32, card: u32, input: u32, border: u32, text: u32, muted: u32, accent: u32, action: u32, on_accent: u32, ok: u32) -> Palette {
    let (bg, action) = (hex(bg), hex(action));
    Palette {
        dark,
        bg,
        sidebar: hex(sidebar),
        card: hex(card),
        input: hex(input),
        border: hex(border),
        text: hex(text),
        muted: hex(muted),
        accent: hex(accent),
        accent_soft: mix(bg, action, if dark { 0.32 } else { 0.18 }),
        action,
        on_accent: hex(on_accent),
        ok: hex(ok),
    }
}

/// Cores de um tema pelo id (desconhecido = igual ao Windows).
pub fn theme_palette(id: &str) -> Palette {
    match id {
        "light" => palette(false),
        "dark" => palette(true),
        "conver" => fixed(false, 0xE8EBE4, 0xDCE0D7, 0xF5F6F2, 0xFFFFFF, 0xCCD0C6, 0x494947, 0x61625D, 0xC7261F, 0xD42A24, 0xFFFFFF, 0x267A34),
        "midnight" => fixed(true, 0x0F172A, 0x0B1222, 0x1E293B, 0x172033, 0x334155, 0xF1F5F9, 0x94A3B8, 0x60A5FA, 0x2563EB, 0xFFFFFF, 0x4ADE80),
        "forest" => fixed(true, 0x0F1F16, 0x0A1710, 0x17301F, 0x13281A, 0x2A4A33, 0xE8F5EC, 0xA3C2AD, 0x6EE7A0, 0x15803D, 0xFFFFFF, 0x86EFAC),
        "amethyst" => fixed(true, 0x18141F, 0x120F18, 0x241E30, 0x1E1929, 0x3A3350, 0xF3EEFF, 0xB8AFD0, 0xC4B5FD, 0x7C3AED, 0xFFFFFF, 0x86EFAC),
        "sand" => fixed(false, 0xF5EFE6, 0xECE3D6, 0xFBF8F3, 0xFFFFFF, 0xE0D5C4, 0x2B2520, 0x6B5E50, 0xA14A06, 0xB45309, 0xFFFFFF, 0x15803D),
        "contrast" => fixed(true, 0x000000, 0x000000, 0x0A0A0A, 0x000000, 0xFFFFFF, 0xFFFFFF, 0xE6E6E6, 0xFFFF00, 0xFFFF00, 0x000000, 0x3FF23F),
        _ => palette(system_dark()),
    }
}

// ------------------------------------------------------------------ ferramentas

/// Todas as ferramentas do menu: página, nome e seção (índice em `NAV`).
fn all_tools() -> impl Iterator<Item = (Page, &'static str, usize)> {
    NAV.iter().enumerate().flat_map(|(s, (_, items))| items.iter().map(move |(p, l)| (*p, *l, s)))
}

fn label_of(page: Page) -> &'static str {
    match page {
        Page::Home => "Início",
        Page::History => "Histórico",
        Page::Settings => "Configurações",
        _ => all_tools().find(|(p, _, _)| *p == page).map_or("", |(_, l, _)| l),
    }
}

/// Nome da seção de uma ferramenta, para o caminho no cabeçalho.
pub fn crumb(page: Page) -> &'static str {
    section_of(page).map_or("", |s| NAV[s].0)
}

/// Ícone de cada ferramenta (fonte de ícones do Windows); as que não têm um próprio usam o da seção.
pub fn tool_icon(page: Page) -> char {
    match page_id(page) {
        "img_resize" => '\u{E740}',
        "img_compress" => '\u{E8A0}',
        "img_quality" => '\u{E9A6}',
        "img_convert" => '\u{E8AB}',
        "img_transform" => '\u{E7A8}',
        "img_strip" => '\u{E72E}',
        "img_pdf" => '\u{EA90}',
        "img_adjust" => '\u{E790}',
        "img_removebg" => '\u{E8B3}',
        "img_removebg_lote" => '\u{E8B3}',
        "img_upscale" => '\u{E8A3}',
        "img_erase" => '\u{E75C}',
        "img_faces" => '\u{E77B}',
        "img_generate" => '\u{E945}',
        "img_edit" => '\u{E70F}',
        "ocr" => '\u{E8E9}',
        "img_scan" => '\u{E8FE}',
        "v_convert" => '\u{E895}',
        "v_compress" => '\u{E8A0}',
        "v_trim" => '\u{E8C6}',
        "v_merge" => '\u{EA86}',
        "v_speed" => '\u{E916}',
        "v_transform" => '\u{E7AD}',
        "v_adjust" => '\u{E706}',
        "v_gif" => '\u{E8B2}',
        "v_subs" => '\u{E8BC}',
        "v_captions" => '\u{E7F0}',
        "v_translate" => '\u{E8C1}',
        "v_music" => '\u{EC4F}',
        "v_audioframes" => '\u{E74F}',
        "screen" => '\u{E7F4}',
        "recorder" => '\u{E7C8}',
        "transcribe" => '\u{E8E4}',
        "dictation" => '\u{E720}',
        "tts" => '\u{E767}',
        "separate" => '\u{E9E9}',
        "a_convert" => '\u{E895}',
        "a_trim" => '\u{E8C6}',
        "a_merge" => '\u{EA86}',
        "a_normalize" => '\u{E9D9}',
        "a_denoise" => '\u{E7EF}',
        "a_voice" => '\u{E95E}',
        "a_compress" => '\u{E8A0}',
        "pdf_merge" => '\u{EA86}',
        "pdf_split" => '\u{E8C6}',
        "pdf_organize" => '\u{E8F1}',
        "pdf_compress" => '\u{E8A0}',
        "pdf_images" => '\u{E91B}',
        "pdf_protect" => '\u{E72E}',
        "pdf_unlock" => '\u{E8D7}',
        "pdf_ocr" => '\u{E8E9}',
        "pdf_tables" => '\u{E8A1}',
        "download" => '\u{E896}',
        "presets" => '\u{E81E}',
        "info" => '\u{E946}',
        "watch" => '\u{E7B3}',
        "rename" => '\u{E8AC}',
        "watermark" => '\u{E8EC}',
        "qr" => '\u{ED14}',
        "flows" => '\u{E9F5}',
        "v_silence" => '\u{E74F}',
        "img_restore" => '\u{E95E}',
        "img_colorize" => '\u{E790}',
        "v_reframe" => '\u{E77B}',
        "v_highlights" => '\u{E945}',
        "img_icons" => '\u{E8B9}',
        _ => section_of(page).map_or(icon::APPS, |s| icon::SECTIONS[s]),
    }
}

fn section_of(page: Page) -> Option<usize> {
    all_tools().find(|(p, _, _)| *p == page).map(|(_, _, s)| s)
}

/// Resumo dos arquivos: "3 arquivos: 2 imagens, 1 vídeo".
fn summary(files: &[PathBuf]) -> String {
    let (mut img, mut vid, mut aud, mut pdf, mut other) = (0, 0, 0, 0, 0);
    for f in files {
        let e = tools::ext_lower(f);
        if crate::imaging::IMAGE_EXTS.contains(&e.as_str()) {
            img += 1;
        } else if crate::video::VIDEO_EXTS.contains(&e.as_str()) {
            vid += 1;
        } else if crate::media::AUDIO_EXTS.contains(&e.as_str()) {
            aud += 1;
        } else if e == "pdf" {
            pdf += 1;
        } else {
            other += 1;
        }
    }
    let parts: Vec<String> = [(img, "imagem", "imagens"), (vid, "vídeo", "vídeos"), (aud, "áudio", "áudios"), (pdf, "PDF", "PDFs"), (other, "outro", "outros")]
        .iter()
        .filter(|(n, _, _)| *n > 0)
        .map(|(n, one, many)| format!("{n} {}", crate::i18n::tr(if *n == 1 { one } else { many })))
        .collect();
    let n = files.len();
    format!("{n} {}: {}", crate::i18n::tr(if n == 1 { "arquivo" } else { "arquivos" }), parts.join(", "))
}

// ------------------------------------------------------------------ peças

/// Botão da barra de navegação: ícone em cima, nome embaixo, como o NavigationView do Windows.
fn rail_item(ui: &mut Ui, p: &Palette, glyph: char, label: &str, selected: bool, badge: usize) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    let painter = ui.painter();
    if selected || resp.hovered() {
        painter.rect_filled(rect.shrink2(vec2(4.0, 2.0)), 6, if selected { p.card } else { mix(p.sidebar, p.card, 0.6) });
    }
    if selected {
        // a barrinha de destaque fica do lado de dentro, como no Windows
        let x = if widgets::rtl() { rect.right() - 6.0 } else { rect.left() + 4.0 };
        painter.rect_filled(egui::Rect::from_center_size(egui::pos2(x + 1.5, rect.center().y), vec2(3.0, 18.0)), 2, p.action);
    }
    let color = if selected { p.accent } else { p.text };
    painter.text(rect.center() - vec2(0.0, 9.0), Align2::CENTER_CENTER, glyph, FontId::new(19.0, icons()), color);
    painter.text(rect.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, crate::i18n::tr(label), FontId::proportional(11.0), if selected { p.text } else { p.muted });
    if badge > 0 {
        let c = rect.center() + vec2(12.0, -18.0);
        painter.circle_filled(c, 8.0, p.action);
        painter.text(c, Align2::CENTER_CENTER, badge.to_string(), FontId::new(10.5, bold()), p.on_accent);
    }
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// Cartão de ferramenta: ícone da seção e nome.
fn tool_card(ui: &mut Ui, p: &Palette, page: Page, label: &str, width: f32) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 52.0), Sense::click());
    let painter = ui.painter();
    let hover = resp.hovered();
    painter.rect_filled(rect, 6, if hover { mix(p.card, p.accent_soft, 0.5) } else { p.card });
    painter.rect_stroke(rect, 6, Stroke::new(1.0f32, if hover { p.accent } else { p.border }), StrokeKind::Inside);
    let glyph = tool_icon(page);
    let (icon_x, text_x, align) = if widgets::rtl() { (rect.right() - 26.0, rect.right() - 48.0, Align2::RIGHT_CENTER) } else { (rect.left() + 26.0, rect.left() + 48.0, Align2::LEFT_CENTER) };
    painter.text(egui::pos2(icon_x, rect.center().y), Align2::CENTER_CENTER, glyph, FontId::new(18.0, icons()), p.accent);
    let text = crate::i18n::tr(label);
    let mut job = egui::text::LayoutJob::simple_singleline(text, FontId::proportional(14.0), p.text);
    job.wrap = egui::text::TextWrapping { max_width: width - 60.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = align.align_size_within_rect(galley.size(), egui::Rect::from_x_y_ranges(if widgets::rtl() { rect.left() + 10.0..=text_x } else { text_x..=rect.right() - 10.0 }, rect.y_range())).min;
    painter.galley(pos, galley, p.text);
    resp.on_hover_cursor(CursorIcon::PointingHand).clicked()
}

/// Título de seção do Fluent: texto em negrito, sem caixa alta.
fn section_title(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(crate::i18n::tr(text)).family(bold()).size(15.0).color(p.text));
    ui.add_space(2.0);
}

/// Cartões em grade, com a largura certa para preencher a linha.
fn grid(ui: &mut Ui, p: &Palette, tools: &[(Page, &'static str)]) -> Option<Page> {
    let cols = ((ui.available_width() + 8.0) / 218.0).floor().max(1.0);
    let width = (ui.available_width() - 8.0 * (cols - 1.0)) / cols;
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (page, label) in tools {
            if tool_card(ui, p, *page, label, width) {
                chosen = Some(*page);
            }
        }
    });
    chosen
}

// ------------------------------------------------------------------ telas

impl ConverApp {
    fn v2(&mut self) -> &mut State {
        self.v2.get_or_insert_with(State::default)
    }

    /// Abre uma ferramenta, levando junto os arquivos esperando na tela inicial.
    pub(super) fn open_tool(&mut self, page: Page) {
        let files = self.v2.as_mut().map(|s| std::mem::take(&mut s.files)).unwrap_or_default();
        self.page = page;
        if !files.is_empty() {
            if page == Page::Flows {
                self.flows.add_files(files);
            } else if let Some(q) = self.queue_of(page) {
                let first = q.files.len();
                if q.add(files) > 0 {
                    q.selected = first;
                }
            }
        }
        if !matches!(page, Page::Home | Page::History | Page::Settings) {
            let recent = &mut self.v2().recent;
            recent.retain(|p| *p != page);
            recent.insert(0, page);
            recent.truncate(8);
            self.cfg.recent = self.v2().recent.iter().map(|p| page_id(*p).to_string()).collect();
            self.cfg.save();
        }
    }

    /// Ferramentas que aceitam algum dos arquivos esperando.
    fn tools_for(&mut self, files: &[PathBuf]) -> Vec<(Page, &'static str, usize)> {
        let mut out = Vec::new();
        for (page, label, s) in all_tools() {
            let ok = match page {
                Page::Flows => true,
                Page::Tts => files.iter().any(|f| crate::extras5::TtsPage::accepts(f)),
                _ => self.queue_of(page).is_some_and(|q| files.iter().any(|f| q.accepts(f))),
            };
            if ok {
                out.push((page, label, s));
            }
        }
        out
    }

    /// Barra de navegação à esquerda (à direita em árabe, urdu e persa).
    pub(super) fn rail(&mut self, ui: &mut Ui) {
        let p = self.pal;
        let running = crate::job::all().iter().filter(|(_, j)| j.state().running).count();
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.add(egui::Image::new((self.logo.id(), vec2(28.0, 28.0))));
        });
        ui.add_space(10.0);
        if rail_item(ui, &p, icon::HOME, "Início", self.page == Page::Home, 0) {
            self.open_tool(Page::Home);
        }
        if rail_item(ui, &p, icon::SEARCH, "Buscar", self.v2.as_ref().is_some_and(|s| s.palette.is_some()), 0) {
            self.v2().palette = Some((String::new(), 0));
        }
        let open = self.v2.as_ref().is_some_and(|s| s.queue_open);
        if rail_item(ui, &p, icon::QUEUE, "Fila", open, running) {
            self.v2().queue_open = !open;
        }
        ui.with_layout(egui::Layout::bottom_up(Align::Center), |ui| {
            ui.add_space(6.0);
            if rail_item(ui, &p, icon::SETTINGS, "Ajustes", self.page == Page::Settings, 0) {
                self.open_tool(Page::Settings);
            }
            if rail_item(ui, &p, icon::HISTORY, "Histórico", self.page == Page::History, 0) {
                self.open_tool(Page::History);
            }
        });
    }

    /// Tela inicial: soltar ou colar arquivos e ver só as ferramentas que servem para eles.
    pub(super) fn home_ui(&mut self, ui: &mut Ui) {
        let p = self.pal;
        let files = self.v2().files.clone();
        // busca no topo, que abre a busca central
        let (bar, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(560.0), 38.0), Sense::click());
        let painter = ui.painter();
        painter.rect_filled(bar, 6, p.input);
        painter.rect_stroke(bar, 6, Stroke::new(1.0f32, if resp.hovered() { p.accent } else { p.border }), StrokeKind::Inside);
        painter.line_segment([bar.left_bottom() + vec2(6.0, -1.0), bar.right_bottom() + vec2(-6.0, -1.0)], Stroke::new(1.0f32, p.muted));
        // em árabe, urdu e persa a lupa e o texto vão para a direita
        let (start, end, a_start, a_end, dir) = if widgets::rtl() {
            (bar.right_center(), bar.left_center(), Align2::RIGHT_CENTER, Align2::LEFT_CENTER, -1.0)
        } else {
            (bar.left_center(), bar.right_center(), Align2::LEFT_CENTER, Align2::RIGHT_CENTER, 1.0)
        };
        painter.text(start + vec2(16.0 * dir, 0.0), a_start, icon::SEARCH, FontId::new(14.0, icons()), p.muted);
        painter.text(start + vec2(40.0 * dir, 0.0), a_start, crate::i18n::tr("Buscar ferramenta"), FontId::proportional(14.0), p.muted);
        painter.text(end - vec2(12.0 * dir, 0.0), a_end, "Ctrl+K", FontId::proportional(12.0), p.muted);
        if resp.on_hover_cursor(CursorIcon::Text).clicked() {
            self.v2().palette = Some((String::new(), 0));
        }
        ui.add_space(16.0);

        if files.is_empty() {
            // área para soltar
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::click());
            let hover = resp.hovered();
            let painter = ui.painter();
            painter.rect_filled(rect, 8, if hover { mix(p.card, p.accent_soft, 0.5) } else { p.card });
            painter.rect_stroke(rect, 8, Stroke::new(1.0f32, if hover { p.accent } else { p.border }), StrokeKind::Inside);
            painter.text(rect.center() - vec2(0.0, 34.0), Align2::CENTER_CENTER, icon::UPLOAD, FontId::new(30.0, icons()), p.accent);
            painter.text(rect.center() + vec2(0.0, 8.0), Align2::CENTER_CENTER, crate::i18n::tr("Solte arquivos aqui para começar"), FontId::new(18.0, bold()), p.text);
            painter.text(rect.center() + vec2(0.0, 36.0), Align2::CENTER_CENTER, format!("{}   •   {}", crate::i18n::tr("ou clique para escolher"), crate::i18n::tr("Ctrl+V cola um print")), FontId::proportional(13.0), p.muted);
            if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                if let Some(list) = rfd::FileDialog::new().pick_files() {
                    self.add_paths(list);
                }
            }
            let recent = self.v2().recent.clone();
            if !recent.is_empty() {
                section_title(ui, &p, "Usadas por último");
                let list: Vec<(Page, &str)> = recent.iter().take(4).map(|pg| (*pg, label_of(*pg))).collect();
                if let Some(pg) = grid(ui, &p, &list) {
                    self.open_tool(pg);
                }
            }
            for (s, (section, items)) in NAV.iter().enumerate() {
                let title = crate::i18n::tr(*section);
                let cap = crate::widgets::capitalize(&title);
                section_title(ui, &p, &cap);
                let list: Vec<(Page, &str)> = items.iter().map(|(pg, l)| (*pg, *l)).collect();
                if let Some(pg) = grid(ui, &p, &list) {
                    self.open_tool(pg);
                }
                let _ = s;
            }
            return;
        }

        // arquivos esperando: resumo, a lista e só as ferramentas que servem
        Frame::default().fill(p.card).stroke(Stroke::new(1.0f32, p.border)).corner_radius(8).inner_margin(Margin::same(14)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(summary(&files)).family(bold()).size(15.0).color(p.text));
                ui.with_layout(widgets::back(Align::Center), |ui| {
                    if widgets::secondary(ui, &p, "Limpar", true) {
                        self.v2().files.clear();
                    }
                    if widgets::secondary(ui, &p, "+ Adicionar", true) {
                        if let Some(list) = rfd::FileDialog::new().pick_files() {
                            self.add_paths(list);
                        }
                    }
                });
            });
            ui.add_space(4.0);
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (i, f) in files.iter().enumerate().take(24) {
                    let r = ui.add(egui::Button::new(RichText::new(format!("{}   ×", tools::file_name(f))).size(12.5).color(p.text)).fill(p.input).stroke(Stroke::new(1.0f32, p.border)).corner_radius(12));
                    if r.on_hover_text(crate::i18n::tr("Tirar da lista")).clicked() {
                        remove = Some(i);
                    }
                }
                if files.len() > 24 {
                    ui.label(RichText::new(crate::i18n::tr(format!("e mais {}", files.len() - 24))).size(12.5).color(p.muted));
                }
            });
            if let Some(i) = remove {
                self.v2().files.remove(i);
            }
        });
        let fits = self.tools_for(&files);
        if fits.is_empty() {
            ui.add_space(12.0);
            ui.label(RichText::new(crate::i18n::tr("Nenhuma ferramenta abre esse tipo de arquivo.")).color(p.muted));
            return;
        }
        section_title(ui, &p, "O que fazer com eles");
        // as usadas por último que servem vêm primeiro
        let recent = self.v2().recent.clone();
        let first: Vec<(Page, &str)> = recent.iter().filter_map(|r| fits.iter().find(|(pg, _, _)| pg == r).map(|(pg, l, _)| (*pg, *l))).take(4).collect();
        if !first.is_empty() {
            if let Some(pg) = grid(ui, &p, &first) {
                self.open_tool(pg);
            }
            ui.add_space(4.0);
        }
        for (s, (section, _)) in NAV.iter().enumerate() {
            let list: Vec<(Page, &str)> = fits.iter().filter(|(_, _, i)| *i == s).map(|(pg, l, _)| (*pg, *l)).collect();
            if list.is_empty() {
                continue;
            }
            let title = crate::i18n::tr(*section);
            ui.add_space(6.0);
            ui.label(RichText::new(title).size(12.0).color(p.muted));
            if let Some(pg) = grid(ui, &p, &list) {
                self.open_tool(pg);
            }
        }
    }

    /// Busca central (Ctrl+K): digite parte do nome, setas para escolher, Enter para abrir.
    pub(super) fn palette_ui(&mut self, ctx: &egui::Context) {
        let p = self.pal;
        let typed = ctx.input(|i| i.modifiers.command && (i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::F)));
        if typed && self.v2().palette.is_none() {
            self.v2().palette = Some((String::new(), 0));
        }
        let Some((mut query, mut sel)) = self.v2().palette.clone() else { return };
        let q = search::normalize(query.trim());
        let mut hits: Vec<(Page, &'static str, usize)> = all_tools()
            .filter(|(pg, label, s)| {
                let section = NAV[*s].0;
                q.trim().is_empty() || search::matches(&q, &search::haystack(label, &crate::i18n::tr(*label), section, &crate::i18n::tr(section), search::keywords(page_id(*pg))))
            })
            .collect();
        if q.trim().is_empty() {
            // sem texto: as usadas por último primeiro
            let recent = self.v2().recent.clone();
            hits.sort_by_key(|(pg, _, _)| recent.iter().position(|r| r == pg).unwrap_or(usize::MAX));
        }
        hits.truncate(9);
        let (down, up, enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if down {
            sel = (sel + 1).min(hits.len().saturating_sub(1));
        }
        if up {
            sel = sel.saturating_sub(1);
        }
        let mut open = None;
        let mut close = esc;
        let modal = egui::Modal::new(egui::Id::new("busca_central"))
            .frame(Frame::default().fill(p.card).stroke(Stroke::new(1.0f32, p.border)).corner_radius(8).inner_margin(Margin::same(10)))
            .show(ctx, |ui| {
                ui.set_width(560.0);
                let id = egui::Id::new("busca_central_texto");
                let r = ui.add(egui::TextEdit::singleline(&mut query).id(id).margin(widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("Buscar ferramenta")).desired_width(f32::INFINITY).font(FontId::proportional(16.0)));
                r.request_focus();
                if r.changed() {
                    sel = 0;
                }
                ui.add_space(6.0);
                let files = self.v2.as_ref().map_or(0, |s| s.files.len());
                for (i, (pg, label, s)) in hits.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                    if i == sel || resp.hovered() {
                        ui.painter().rect_filled(rect, 4, if i == sel { p.accent_soft } else { p.input });
                    }
                    ui.painter().text(rect.left_center() + vec2(16.0, 0.0), Align2::CENTER_CENTER, tool_icon(*pg), FontId::new(15.0, icons()), p.accent);
                    ui.painter().text(rect.left_center() + vec2(36.0, 0.0), Align2::LEFT_CENTER, crate::i18n::tr(*label), FontId::proportional(14.0), p.text);
                    ui.painter().text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, crate::i18n::tr(NAV[*s].0), FontId::proportional(11.5), p.muted);
                    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                        open = Some(*pg);
                    }
                }
                if hits.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("Nenhuma ferramenta encontrada.")).color(p.muted));
                }
                ui.add_space(4.0);
                let tip = if files > 0 { format!("↑ ↓ para escolher, Enter para abrir com os {} arquivo(s) da tela inicial, Esc para fechar", files) } else { "↑ ↓ para escolher, Enter para abrir, Esc para fechar".to_string() };
                ui.label(RichText::new(crate::i18n::tr(tip)).size(11.5).color(p.muted));
            });
        if modal.backdrop_response.clicked() {
            close = true;
        }
        if enter {
            open = open.or_else(|| hits.get(sel).map(|(pg, _, _)| *pg));
        }
        if let Some(pg) = open {
            self.v2().palette = None;
            self.open_tool(pg);
        } else if close {
            self.v2().palette = None;
        } else {
            self.v2().palette = Some((query, sel));
        }
    }

    /// Fila: todas as tarefas iniciadas, de qualquer ferramenta, com andamento e ações.
    pub(super) fn queue_ui(&mut self, ui: &mut Ui) {
        let p = self.pal;
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Fila")).family(bold()).size(18.0).color(p.text));
            ui.with_layout(widgets::back(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new(icon::CLOSE).family(icons()).size(12.0)).frame(false)).clicked() {
                    self.v2().queue_open = false;
                }
            });
        });
        ui.label(RichText::new(crate::i18n::tr("As tarefas continuam enquanto você usa outras ferramentas.")).size(12.0).color(p.muted));
        ui.add_space(8.0);
        let jobs = crate::job::all();
        if jobs.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(crate::i18n::tr("Nenhuma tarefa ainda.")).color(p.muted)));
            return;
        }
        let mut go = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (origin, job) in jobs.iter().rev() {
                let page = all_tools().find(|(pg, _, _)| page_id(*pg) == *origin).map(|(pg, _, _)| pg);
                let (running, progress, status, failed, done) = {
                    let s = job.state();
                    (s.running, s.progress, s.status.clone(), s.log.iter().any(|(k, _)| *k == crate::job::LogKind::Err), s.done_folder.clone())
                };
                Frame::default().fill(p.card).stroke(Stroke::new(1.0f32, p.border)).corner_radius(6).inner_margin(Margin::same(10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (glyph, color) = if running {
                            (page.map_or(icon::APPS, tool_icon), p.accent)
                        } else if failed {
                            (icon::ERROR, p.accent)
                        } else {
                            (icon::CHECK, p.ok)
                        };
                        ui.label(RichText::new(glyph).family(icons()).size(14.0).color(color));
                        ui.label(RichText::new(crate::i18n::tr(page.map_or("Tarefa", label_of))).family(bold()).color(p.text));
                    });
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr(&status)).size(12.0).color(p.muted)).truncate());
                    if running {
                        ui.add(egui::ProgressBar::new(progress).desired_height(4.0).fill(p.action));
                    }
                    ui.horizontal(|ui| {
                        if let Some(pg) = page {
                            if ui.add(egui::Button::new(RichText::new(crate::i18n::tr("Abrir")).size(12.5)).small()).clicked() {
                                go = Some(pg);
                            }
                        }
                        if running && ui.add(egui::Button::new(RichText::new(crate::i18n::tr("Cancelar")).size(12.5)).small()).clicked() {
                            job.cancel();
                        }
                        if let (false, Some(d)) = (running, &done) {
                            if ui.add(egui::Button::new(RichText::new(crate::i18n::tr(if d.is_file() { "Mostrar arquivo" } else { "Abrir pasta" })).size(12.5)).small()).clicked() {
                                if d.is_file() { tools::show_in_folder(d) } else { tools::open_folder(d) }
                            }
                        }
                    });
                });
                ui.add_space(6.0);
            }
            if jobs.iter().any(|(_, j)| !j.state().running) && widgets::secondary(ui, &p, "Limpar concluídas", true) {
                crate::job::clear_finished();
            }
        });
        if let Some(pg) = go {
            self.open_tool(pg);
        }
    }

}
