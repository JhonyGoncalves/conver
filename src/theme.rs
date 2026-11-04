//! Paleta do Conver: soft linen #e8ebe4, charcoal #494947, cinnabar #f13030.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

pub const LINEN: Color32 = Color32::from_rgb(0xE8, 0xEB, 0xE4);
pub const CHARCOAL: Color32 = Color32::from_rgb(0x49, 0x49, 0x47);
pub const CINNABAR: Color32 = Color32::from_rgb(0xF1, 0x30, 0x30);

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub input: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                bg: Color32::from_rgb(0x2C, 0x2C, 0x2B),
                sidebar: Color32::from_rgb(0x24, 0x24, 0x23),
                card: Color32::from_rgb(0x38, 0x38, 0x36),
                input: CHARCOAL,
                border: Color32::from_rgb(0x52, 0x52, 0x4F),
                text: LINEN,
                muted: Color32::from_rgb(0xA8, 0xAB, 0xA3),
                accent: CINNABAR,
                accent_soft: Color32::from_rgb(0x5A, 0x2E, 0x2C),
                on_accent: Color32::WHITE,
                ok: Color32::from_rgb(0x8F, 0xD1, 0x8A),
            }
        } else {
            Self {
                dark,
                bg: LINEN,
                sidebar: Color32::from_rgb(0xDC, 0xE0, 0xD7),
                card: Color32::from_rgb(0xF5, 0xF6, 0xF2),
                input: Color32::WHITE,
                border: Color32::from_rgb(0xCC, 0xD0, 0xC6),
                text: CHARCOAL,
                muted: Color32::from_rgb(0x7C, 0x7D, 0x78),
                accent: CINNABAR,
                accent_soft: Color32::from_rgb(0xF6, 0xD3, 0xCF),
                on_accent: Color32::WHITE,
                ok: Color32::from_rgb(0x2E, 0x8B, 0x3E),
            }
        }
    }
}

/// Usa a Segoe UI do Windows (com variação semibold para títulos), se existir.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut bold = proportional.clone();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    for (name, file, target) in [("segoe", "segoeui.ttf", 0), ("segoe-sb", "seguisb.ttf", 1)] {
        if let Ok(bytes) = std::fs::read(format!("{windir}\\Fonts\\{file}")) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            if target == 0 {
                proportional.insert(0, name.into());
            } else {
                bold.insert(0, name.into());
            }
        }
    }
    if !bold.first().is_some_and(|f| f == "segoe-sb") && proportional.first().is_some_and(|f| f == "segoe") {
        bold.insert(0, "segoe".into());
    }
    // escritas de outros idiomas: chinês e japonês (YaHei), coreano (Malgun) e formas árabes (Arial)
    for (name, file) in [("yahei", "msyh.ttc"), ("yugothic", "YuGothM.ttc"), ("malgun", "malgun.ttf"), ("arial", "arial.ttf")] {
        if let Ok(bytes) = std::fs::read(format!("{windir}\\Fonts\\{file}")) {
            fonts.font_data.insert(name.into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
            let at = proportional.len().min(2);
            proportional.insert(at, name.into());
            let at = bold.len().min(2);
            bold.insert(at, name.into());
        }
    }
    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    ctx.set_fonts(fonts);
}

pub fn bold() -> FontFamily {
    FontFamily::Name("bold".into())
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    ctx.set_theme(if p.dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut v = if p.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0f32, p.border);
    v.extreme_bg_color = p.input;
    v.faint_bg_color = p.card;
    v.code_bg_color = p.input;
    v.override_text_color = None; // o texto vem do fg_stroke; assim o texto de dica (placeholder) fica apagado
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0f32, p.on_accent);
    v.slider_trailing_fill = true;
    let r = CornerRadius::same(5);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, p.card),
        (&mut v.widgets.inactive, p.input),
        (&mut v.widgets.hovered, p.input),
        (&mut v.widgets.active, p.accent_soft),
        (&mut v.widgets.open, p.input),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.corner_radius = r;
        w.fg_stroke = Stroke::new(1.0f32, p.text);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, p.border);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0f32, p.border);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0f32, p.accent);
    v.widgets.active.bg_stroke = Stroke::new(1.0f32, p.accent);
    v.widgets.open.bg_stroke = Stroke::new(1.0f32, p.accent);

    ctx.style_mut(|s| {
        s.visuals = v;
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.slider_width = 260.0;
        s.text_styles = [
            (TextStyle::Heading, FontId::new(26.0, bold())),
            (TextStyle::Body, FontId::proportional(14.5)),
            (TextStyle::Button, FontId::proportional(14.5)),
            (TextStyle::Small, FontId::proportional(12.0)),
            (TextStyle::Monospace, FontId::monospace(13.0)),
        ]
        .into();
    });
}
