//! Painel de "Cortar trecho" e "Cortar áudio": vários trechos numa linha do tempo. Trechos para manter e para tirar
//! podem ficar um dentro do outro; quem resolve o que sobra é `media::plan_cuts`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use crate::cut_preset::{self, CutPreset};
use crate::i18n::tr;
use crate::media::{self, Cut, CutMode};
use crate::theme::{Palette, bold};
use crate::video;
use crate::widgets::{self, Durations, hint, secondary, segmented};

/// Um trecho na tela: os tempos ficam como texto enquanto a pessoa digita.
struct Row {
    keep: bool,
    start: String,
    end: String,
}

type Frame = (String, egui::ColorImage);

pub struct TrimPanel {
    rows: Vec<Row>,
    /// Trecho que os botões e as alças da linha do tempo mexem.
    current: usize,
    /// Várias partes num arquivo só (`true`) ou um arquivo para cada trecho.
    pub together: bool,
    /// Ponto da linha do tempo, em segundos.
    head: f64,
    /// Alça sendo arrastada: `true` = início, `false` = fim.
    edge: Option<bool>,
    file: Option<PathBuf>,
    frame: Option<(String, egui::TextureHandle)>,
    arrived: Arc<Mutex<Option<Frame>>>,
    busy: Arc<AtomicBool>,
    asked: String,
    /// Resultado da última leitura de um arquivo de cortes: nome e erro, se houve.
    preset: Option<(String, Option<String>)>,
    /// Modo vindo do arquivo lido, para a tela aplicar.
    pub wanted_mode: Option<CutMode>,
    /// Quadros-chave do arquivo aberto, lidos em segundo plano.
    keys: Vec<f64>,
    keys_in: Arc<Mutex<Option<(PathBuf, Vec<f64>)>>>,
}

impl TrimPanel {
    pub fn new() -> Self {
        // modo de captura: CONVER_CORTES = "m:0:10-0:40;t:0:20-0:30"
        let scene: Vec<Row> = std::env::var("CONVER_CORTES")
            .iter()
            .flat_map(|s| s.split(';'))
            .filter_map(|c| {
                let (kind, range) = c.split_once(':')?;
                let (start, end) = range.split_once('-')?;
                Some(Row { keep: kind == "m", start: start.into(), end: end.into() })
            })
            .collect();
        Self {
            rows: if scene.is_empty() { vec![Row { keep: true, start: "00:00:00".into(), end: String::new() }] } else { scene },
            current: 0,
            together: true,
            head: 0.0,
            edge: None,
            file: None,
            frame: None,
            arrived: Arc::default(),
            busy: Arc::default(),
            asked: String::new(),
            preset: None,
            wanted_mode: None,
            keys: Vec::new(),
            keys_in: Arc::default(),
        }
    }

    /// Lê um arquivo de cortes: os trechos da tela são trocados pelos dele. Com erro, a tela fica como estava.
    pub fn load(&mut self, path: &Path) {
        let name = crate::tools::file_name(path);
        match cut_preset::read(path) {
            Ok(p) => {
                self.rows = p
                    .cuts
                    .iter()
                    .map(|c| Row { keep: c.keep, start: video::fmt_time_exact(c.start), end: c.end.map(video::fmt_time_exact).unwrap_or_default() })
                    .collect();
                self.current = 0;
                self.together = p.together;
                self.wanted_mode = p.mode;
                self.preset = Some((name, None));
            }
            Err(e) => self.preset = Some((name, Some(e))),
        }
    }

    fn save(&mut self, mode: CutMode) {
        let Ok(cuts) = self.cuts() else { return };
        let name = cut_preset::file_name_for(self.file.as_deref());
        let mut dialog = rfd::FileDialog::new().add_filter(tr("Arquivo de cortes"), &["json"]).set_file_name(name);
        if let Some(dir) = self.file.as_deref().and_then(Path::parent) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(dst) = dialog.save_file() {
            let text = cut_preset::to_json(&CutPreset { cuts, together: self.together, mode: Some(mode) });
            let shown = crate::tools::file_name(&dst);
            self.preset = Some((shown, std::fs::write(&dst, text).err().map(|e| e.to_string())));
        }
    }

    /// Os trechos prontos para o `media::cut`, ou o que há de errado com eles.
    pub fn cuts(&self) -> Result<Vec<Cut>, String> {
        self.rows
            .iter()
            .map(|r| {
                let start = if r.start.trim().is_empty() {
                    0.0
                } else {
                    video::parse_timecode(&r.start).ok_or("Início inválido. Use o formato 00:01:30 (ou só segundos).")?
                };
                let end = if r.end.trim().is_empty() {
                    None
                } else {
                    Some(video::parse_timecode(&r.end).ok_or("Fim inválido. Use o formato 00:02:45 (ou só segundos).")?)
                };
                if end.is_some_and(|e| e <= start) {
                    return Err("O fim precisa ser depois do início.".to_string());
                }
                Ok(Cut { keep: r.keep, start, end })
            })
            .collect()
    }

    /// Novo trecho a partir do ponto da linha do tempo.
    fn add(&mut self, keep: bool, dur: Option<f64>) {
        let start = self.head;
        let end = (start + 5.0).min(dur.unwrap_or(f64::MAX));
        self.rows.push(Row { keep, start: video::fmt_time_fine(start), end: video::fmt_time_fine(end) });
        self.current = self.rows.len() - 1;
    }

    /// Pede ao ffmpeg o quadro do ponto atual (um pedido por vez; o último vale).
    fn update_frame(&mut self, ctx: &egui::Context, src: &Path) {
        if let Some((key, img)) = self.arrived.lock().unwrap_or_else(|e| e.into_inner()).take() {
            self.frame = Some((key, ctx.load_texture("corte_quadro", img, egui::TextureOptions::LINEAR)));
        }
        let key = format!("{src:?}|{:.1}", self.head);
        if self.asked == key || self.busy.load(Ordering::Relaxed) {
            return;
        }
        self.asked = key.clone();
        self.busy.store(true, Ordering::Relaxed);
        let (arrived, busy, ctx, src, at) = (self.arrived.clone(), self.busy.clone(), ctx.clone(), src.to_path_buf(), self.head);
        std::thread::spawn(move || {
            if let Ok(img) = media::frame_at(&src, at, 270) {
                let size = [img.width() as usize, img.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                *arrived.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, ci));
            }
            busy.store(false, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }

    fn preview(&mut self, ui: &mut Ui, p: &Palette, src: &Path) {
        self.update_frame(ui.ctx(), src);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 220.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8, p.input);
        if let Some((_, tex)) = &self.frame {
            let s = tex.size_vec2();
            let r = Rect::from_center_size(rect.center(), widgets::fit((s.x, s.y), rect.size() - vec2(8.0, 8.0)));
            painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
    }

    /// Linha do tempo: faixa de cima com os trechos, faixa de baixo com o que vai sobrar. Sempre da esquerda para a
    /// direita, também em árabe: é assim que o tempo corre nos editores de vídeo.
    fn timeline(&mut self, ui: &mut Ui, p: &Palette, dur: f64, mode: CutMode) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        let lanes = Rect::from_min_size(rect.min + vec2(0.0, 14.0), vec2(rect.width(), 44.0));
        let x = |t: f64| lanes.left() + (t / dur).clamp(0.0, 1.0) as f32 * lanes.width();
        let at = |px: f32| ((px - lanes.left()) / lanes.width()).clamp(0.0, 1.0) as f64 * dur;
        painter.rect_filled(lanes, 4, p.input);
        let cuts = self.cuts().unwrap_or_default();
        let top = Rect::from_min_max(lanes.min, pos2(lanes.right(), lanes.bottom() - 12.0));
        let span = |c: &Cut| Rect::from_min_max(pos2(x(c.start), top.top()), pos2(x(c.end.unwrap_or(dur)).max(x(c.start) + 2.0), top.bottom()));
        // quem mantém embaixo; quem tira por cima
        for keep in [true, false] {
            for c in cuts.iter().filter(|c| c.keep == keep) {
                // tirar é sempre vermelho, em qualquer tema (o destaque pode ser azul ou roxo)
                let color = if c.keep { p.ok } else { Color32::from_rgb(0xE5, 0x48, 0x4D) };
                painter.rect_filled(span(c), 3, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 110));
            }
        }
        // o que sobra, de verdade
        for clip in media::plan_cuts(&cuts, dur, false).unwrap_or_default() {
            for (a, b) in clip {
                let r = Rect::from_min_max(pos2(x(a), lanes.bottom() - 9.0), pos2(x(b).max(x(a) + 1.0), lanes.bottom() - 2.0));
                painter.rect_filled(r, 2, p.ok);
            }
        }
        // alças do trecho atual
        if let Some(c) = cuts.get(self.current) {
            let r = span(c);
            painter.rect_stroke(r, 3, Stroke::new(1.5f32, p.text), StrokeKind::Inside);
            let near = |px: f32, edge: f32| (px - edge).abs() <= 8.0;
            if let Some(h) = resp.hover_pos() {
                if near(h.x, r.left()) || near(h.x, r.right()) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
            if resp.drag_started() {
                let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or_default();
                self.edge = if near(origin.x, r.left()) {
                    Some(true)
                } else if near(origin.x, r.right()) {
                    Some(false)
                } else {
                    None
                };
            }
        }
        // quadros-chave: onde um corte sem recodificar pode cair
        let tick = Color32::from_rgba_unmultiplied(p.text.r(), p.text.g(), p.text.b(), 110);
        for k in &self.keys {
            painter.line_segment([pos2(x(*k), lanes.top()), pos2(x(*k), lanes.top() + 5.0)], Stroke::new(1.0f32, tick));
        }
        if let Some(q) = resp.interact_pointer_pos() {
            let t = at(q.x);
            // no Rápido a borda arrastada gruda no quadro-chave: é onde o corte vai cair de verdade
            let snapped = if mode == CutMode::Rapido {
                self.keys.iter().copied().min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs())).filter(|k| (x(*k) - q.x).abs() <= 10.0).unwrap_or(t)
            } else {
                t
            };
            match (self.edge, self.rows.get_mut(self.current)) {
                (Some(true), Some(row)) => row.start = video::fmt_time_fine(snapped),
                (Some(false), Some(row)) => row.end = video::fmt_time_fine(snapped),
                _ => {}
            }
            self.head = if self.edge.is_some() { snapped } else { t };
        }
        if resp.clicked() {
            // clicar num trecho o escolhe (o mais curto, para alcançar o que está dentro de outro)
            let t = self.head;
            let hit = cuts.iter().enumerate().filter(|(_, c)| c.start <= t && t <= c.end.unwrap_or(dur));
            if let Some((i, _)) = hit.min_by(|a, b| (a.1.end.unwrap_or(dur) - a.1.start).total_cmp(&(b.1.end.unwrap_or(dur) - b.1.start))) {
                self.current = i;
            }
        }
        if resp.drag_stopped() {
            self.edge = None;
        }
        let h = x(self.head);
        painter.line_segment([pos2(h, lanes.top() - 2.0), pos2(h, lanes.bottom())], Stroke::new(2.0f32, p.text));
        let small = FontId::proportional(11.5);
        // os extremos saem quando o rótulo do ponto chega perto
        let label = pos2(h.clamp(rect.left() + 34.0, rect.right() - 34.0), rect.top());
        if label.x > rect.left() + 90.0 {
            painter.text(rect.left_top(), Align2::LEFT_TOP, video::fmt_time(0.0), small.clone(), p.muted);
        }
        if label.x < rect.right() - 90.0 {
            painter.text(rect.right_top(), Align2::RIGHT_TOP, video::fmt_time(dur), small, p.muted);
        }
        painter.text(label, Align2::CENTER_TOP, video::fmt_time_fine(self.head), FontId::new(11.5, bold()), p.text);
    }

    fn rows_ui(&mut self, ui: &mut Ui, p: &Palette, dur: Option<f64>) {
        let mut remove = None;
        let many = self.rows.len() > 1;
        for (i, row) in self.rows.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                let number = RichText::new((i + 1).to_string()).family(bold()).color(if i == self.current { Color32::WHITE } else { p.muted });
                if ui.selectable_label(i == self.current, number).clicked() {
                    self.current = i;
                }
                if segmented(ui, p, &mut row.keep, &[(true, "Manter"), (false, "Tirar")]) {
                    self.current = i;
                }
                ui.label(RichText::new(tr("Início:")).color(p.text));
                let a = ui.add(egui::TextEdit::singleline(&mut row.start).margin(widgets::FIELD_MARGIN).hint_text(tr("00:00:00")).desired_width(100.0));
                ui.label(RichText::new(tr("Fim:")).color(p.text));
                let b = ui.add(egui::TextEdit::singleline(&mut row.end).margin(widgets::FIELD_MARGIN).hint_text(tr("até o fim")).desired_width(100.0));
                if a.gained_focus() || b.gained_focus() {
                    self.current = i;
                }
                if many && secondary(ui, p, "×", true) {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.rows.remove(i);
            self.current = self.current.min(self.rows.len() - 1);
        }
        ui.horizontal_wrapped(|ui| {
            if secondary(ui, p, "+ Trecho para manter", true) {
                self.add(true, dur);
            }
            if secondary(ui, p, "+ Trecho para tirar", true) {
                self.add(false, dur);
            }
            if dur.is_some() {
                let head = video::fmt_time_fine(self.head);
                if secondary(ui, p, "Início aqui", true) {
                    self.rows[self.current].start = head.clone();
                }
                if secondary(ui, p, "Fim aqui", true) {
                    self.rows[self.current].end = head;
                }
            }
        });
    }

    /// Resumo do que vai sair.
    fn summary(&self, ui: &mut Ui, p: &Palette, dur: Option<f64>) {
        let cuts = match self.cuts() {
            Ok(c) => c,
            Err(e) => {
                ui.label(RichText::new(tr(e)).color(p.accent));
                return;
            }
        };
        let Some(d) = dur else { return };
        match media::plan_cuts(&cuts, d, self.together) {
            Ok(plan) => {
                let left: f64 = plan.iter().flatten().map(|(a, b)| b - a).sum();
                let text = format!("Vai sair: {} arquivo(s) • {} de {}", plan.len(), video::fmt_time(left), video::fmt_time(d));
                ui.label(RichText::new(tr(text)).size(12.5).family(bold()).color(p.text));
            }
            Err(e) => {
                ui.label(RichText::new(tr(e)).color(p.accent));
            }
        }
    }

    /// Quanto o início de algum corte recua no modo Rápido, conforme os quadros-chave.
    fn fast_shift(&self, dur: f64) -> Option<f64> {
        let cuts = self.cuts().ok()?;
        let plan = media::plan_cuts(&cuts, dur, self.together).ok()?;
        let shift = |a: f64| self.keys.iter().rev().find(|k| **k <= a - 0.13).map_or(0.0, |k| a - k);
        plan.iter().flatten().filter(|(a, _)| *a > 0.2).map(|(a, _)| shift(*a)).reduce(f64::max).filter(|s| *s > 0.2)
    }

    fn preset_ui(&mut self, ui: &mut Ui, p: &Palette, mode: CutMode) {
        ui.horizontal_wrapped(|ui| {
            if secondary(ui, p, "Ler arquivo de cortes", true) {
                if let Some(f) = rfd::FileDialog::new().add_filter(tr("Arquivo de cortes"), &["json"]).pick_file() {
                    self.load(&f);
                }
            }
            if secondary(ui, p, "Salvar arquivo de cortes", self.cuts().is_ok()) {
                self.save(mode);
            }
            match &self.preset {
                Some((name, None)) => {
                    ui.label(RichText::new(format!("✔ {name}")).size(12.5).color(p.ok));
                }
                Some((name, Some(e))) => {
                    ui.label(RichText::new(format!("✖ {name}: {}", tr(e))).size(12.5).color(p.accent));
                }
                None => {}
            }
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette, src: Option<&PathBuf>, durs: &Durations, video_tool: bool, mode: CutMode, files: usize) {
        if self.file.as_ref() != src {
            self.file = src.cloned();
            self.head = 0.0;
            self.frame = None;
            self.keys.clear();
            if let (true, Some(f)) = (video_tool, src.cloned()) {
                let (out, ctx) = (self.keys_in.clone(), ui.ctx().clone());
                std::thread::spawn(move || {
                    let keys = crate::smartcut::keyframes_quick(&f);
                    *out.lock().unwrap_or_else(|e| e.into_inner()) = Some((f, keys));
                    ctx.request_repaint();
                });
            }
            // o arquivo de cortes com o nome do vídeo é lido sozinho
            if let Some(f) = src.and_then(|f| cut_preset::beside(f)) {
                self.load(&f);
            }
        }
        if let Some((f, keys)) = self.keys_in.lock().unwrap_or_else(|e| e.into_inner()).take() {
            if Some(&f) == src {
                self.keys = keys;
            }
        }
        let dur = src.and_then(|f| durs.get(f)).filter(|d| *d > 0.0);
        if let (true, Some(f)) = (video_tool, src) {
            self.preview(ui, p, f);
            ui.add_space(4.0);
        }
        if let Some(d) = dur {
            self.timeline(ui, p, d, mode);
            ui.add_space(6.0);
        }
        self.rows_ui(ui, p, dur);
        self.preset_ui(ui, p, mode);
        if self.rows.iter().filter(|r| r.keep).count() > 1 {
            ui.add_space(2.0);
            widgets::row(ui, p, "Salvar:", |ui| {
                segmented(ui, p, &mut self.together, &[(true, "Juntar em um arquivo"), (false, "Um arquivo para cada trecho")]);
            });
        }
        self.summary(ui, p, dur);
        if files > 1 {
            hint(ui, p, "Com vários arquivos, quem tem um .cortes.json com o próprio nome ao lado usa o dele; os outros usam os trechos acima.");
        }
        hint(ui, p, "Um trecho para tirar dentro de um trecho para manter abre um buraco nele. Com um arquivo para cada trecho, um trecho pode ficar dentro de outro e os dois saem completos.");
        if mode == CutMode::Rapido {
            if let Some(s) = dur.and_then(|d| self.fast_shift(d)) {
                let seconds = format!("{s:.1}");
                let text = format!("No modo Rápido, um corte vai começar {} s antes do marcado, no quadro-chave anterior. Para o ponto exato, use Inteligente.", seconds);
                ui.label(RichText::new(tr(text)).size(12.5).color(p.accent));
            } else if self.keys.is_empty() && self.rows.len() > 1 {
                ui.label(RichText::new(tr("No modo Rápido, cada emenda cai no quadro-chave mais próximo e pode sobrar um pouco do que você tirou. Para cortar no ponto exato, use Preciso.")).size(12.5).color(p.accent));
            }
        }
    }
}
