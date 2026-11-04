//! Tela de automação: editor de fluxos por nodes. À esquerda os tipos de node, no meio o quadro (arrastar para
//! mover, puxar da saída ● de um node até outro para ligar, rolar para aproximar), à direita os ajustes do node
//! escolhido e embaixo a execução. Tudo é salvo sozinho em `fluxos/`, o mesmo lugar que o MCP usa.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Frame, Layout, Margin, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use serde_json::{Map, Value, json};

use crate::flow::{self, Flow, KindInfo, Link, Node, RunState, Status};
use crate::job::{Job, LogKind};
use crate::theme::{Palette, bold};
use crate::widgets::{self, Act};
use crate::{ai_api, ops, tools};

const NODE_W: f32 = 210.0;
const HEADER: f32 = 34.0;
const CATEGORIES: &[&str] = &["ENTRADA E SAÍDA", "IA", "IMAGEM", "VÍDEO", "ÁUDIO", "PDF", "DOWNLOAD", "FERRAMENTAS"];

#[derive(Clone, Copy, PartialEq)]
enum Sel {
    Node(u32),
    Link(usize),
}

pub struct FlowsPage {
    kinds: Vec<KindInfo>,
    names: Vec<String>,
    flow: Option<Flow>,
    /// Data do arquivo quando foi lido/salvo (para recarregar se o MCP mudar o fluxo).
    mtime: Option<SystemTime>,
    dirty: Option<Instant>,
    last_check: Instant,
    pan: Vec2,
    zoom: f32,
    fit_pending: bool,
    sel: Option<Sel>,
    /// Ligação sendo puxada: node e saída de origem.
    dragging: Option<(u32, String)>,
    query: String,
    renaming: Option<String>,
    confirm_delete: Option<Instant>,
    notice: Option<(bool, String)>,
    job: Job,
    run: Arc<Mutex<RunState>>,
    go_settings: bool,
}

impl FlowsPage {
    pub fn new() -> Self {
        Self {
            kinds: Vec::new(),
            names: Vec::new(),
            flow: None,
            mtime: None,
            dirty: None,
            last_check: Instant::now() - Duration::from_secs(10),
            pan: vec2(20.0, 20.0),
            zoom: 1.0,
            fit_pending: true,
            sel: None,
            dragging: None,
            query: String::new(),
            renaming: None,
            confirm_delete: None,
            notice: None,
            job: Job::default(),
            run: Arc::default(),
            go_settings: false,
        }
    }

    /// Modo de captura: abre o primeiro exemplo, escolhe o node de IA e mostra uma execução de mentira.
// i18n: fora
    pub fn demo(&mut self) {
        self.kinds = flow::kinds();
        flow::ensure_examples();
        self.load_list();
        let name = flow::examples()[0].name.clone();
        self.open(&name);
        self.sel = Some(Sel::Node(3));
        let mut r = self.run.lock().unwrap();
        r.status.insert(1, Status::Done("3 arquivo(s)".into()));
        r.status.insert(2, Status::Done("3 item(ns)".into()));
        r.status.insert(3, Status::Running(0.6));
        r.status.insert(4, Status::Waiting);
        r.status.insert(5, Status::Waiting);
    }

// i18n: dentro
    /// A tela pediu para abrir as configurações da IA.
    pub fn take_go_settings(&mut self) -> bool {
        std::mem::take(&mut self.go_settings)
    }

    /// Arquivos arrastados para a janela: entram no node de entrada escolhido (ou no primeiro).
    pub fn add_files(&mut self, paths: Vec<PathBuf>) {
        let sel = match self.sel {
            Some(Sel::Node(id)) => Some(id),
            _ => None,
        };
        let Some(f) = self.flow.as_mut() else { return };
        let target = sel.filter(|id| f.node(*id).is_some_and(|n| n.kind == "input")).or_else(|| f.nodes.iter().find(|n| n.kind == "input").map(|n| n.id));
        let Some(id) = target else {
            self.notice = Some((false, "Adicione um node \"Arquivos de entrada\" para receber os arquivos.".into()));
            return;
        };
        let n = f.node_mut(id).unwrap();
        let list = n.args.entry("files").or_insert_with(|| json!([]));
        if let Some(a) = list.as_array_mut() {
            for p in paths {
                let s = p.display().to_string();
                if !a.iter().any(|v| v == &json!(s)) {
                    a.push(json!(s));
                }
            }
        }
        self.sel = Some(Sel::Node(id));
        self.touch();
    }

    fn touch(&mut self) {
        self.dirty = Some(Instant::now());
    }

    fn info(&self, kind: &str) -> Option<&KindInfo> {
        self.kinds.iter().find(|k| k.kind == kind)
    }

    fn title_of(&self, n: &Node) -> String {
        if !n.title.trim().is_empty() {
            return n.title.clone();
        }
        self.info(&n.kind).map(|k| crate::i18n::tr(&k.title)).unwrap_or_else(|| n.kind.clone())
    }

    // ------------------------------------------------------------ arquivos

    fn load_list(&mut self) {
        self.names = flow::list().into_iter().map(|f| f.name).collect();
    }

    fn open(&mut self, name: &str) {
        self.save_now();
        if let Some(f) = flow::find(name) {
            self.mtime = std::fs::metadata(flow::path_of(&f.name)).and_then(|m| m.modified()).ok();
            self.flow = Some(f);
            self.sel = None;
            self.fit_pending = true;
            *self.run.lock().unwrap() = RunState::default();
        }
    }

    fn save_now(&mut self) {
        if self.dirty.take().is_none() {
            return;
        }
        if let Some(f) = &self.flow {
            if let Err(e) = flow::save_draft(f) {
                self.notice = Some((false, format!("✖ {e}")));
            }
            self.mtime = std::fs::metadata(flow::path_of(&f.name)).and_then(|m| m.modified()).ok();
        }
    }

    /// Salva o que mudou (sem atrapalhar quem está arrastando) e recarrega se o fluxo mudou por fora (MCP).
    fn sync(&mut self, ui: &Ui) {
        let pointer_down = ui.input(|i| i.pointer.any_down());
        if self.dirty.is_some_and(|t| t.elapsed() > Duration::from_millis(400)) && !pointer_down {
            self.save_now();
        }
        if self.last_check.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_check = Instant::now();
        self.load_list();
        if self.dirty.is_none() {
            if let Some(name) = self.flow.as_ref().map(|f| f.name.clone()) {
                let m = std::fs::metadata(flow::path_of(&name)).and_then(|m| m.modified()).ok();
                if m.is_none() {
                    // apagado por fora
                    self.flow = None;
                } else if m != self.mtime {
                    if let Some(f) = flow::find(&name) {
                        self.flow = Some(f);
                        self.mtime = m;
                        self.notice = Some((true, "O fluxo foi alterado por fora (MCP ou outro programa) e foi recarregado.".into()));
                    }
                }
            }
        }
        if self.flow.is_none() {
            if let Some(first) = self.names.first().cloned() {
                self.open(&first);
            }
        }
    }

    fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.names.iter().any(|x| x.eq_ignore_ascii_case(n));
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
    }

    fn create(&mut self) {
        self.save_now();
        let name = self.unique_name(&crate::i18n::tr("Novo fluxo"));
        let mk = |id: u32, kind: &str, x: f32| Node { id, kind: kind.into(), title: String::new(), args: Map::new(), pos: [x, 140.0] };
        let f = Flow { name: name.clone(), description: String::new(), nodes: vec![mk(1, "input", 40.0), mk(2, "output", 560.0)], links: Vec::new() };
        self.flow = Some(f);
        self.dirty = Some(Instant::now() - Duration::from_secs(1));
        self.save_now();
        self.load_list();
        self.sel = None;
        self.fit_pending = true;
        *self.run.lock().unwrap() = RunState::default();
    }

    // ------------------------------------------------------------ tela

    pub fn ui(&mut self, ui: &mut Ui, p: &Palette) {
        if self.kinds.is_empty() {
            self.kinds = flow::kinds();
            flow::ensure_examples();
        }
        self.sync(ui);
        self.toolbar(ui, p);
        ui.add_space(6.0);

        let st = self.job.state();
        let bottom = 74.0 + if st.log.is_empty() { 0.0 } else { 140.0 };
        drop(st);
        let h = (ui.available_height() - bottom).max(260.0);
        let w = ui.available_width();
        ui.allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let insp = 300.0;
            let cw = (w - insp - 8.0).max(260.0);
            self.canvas(ui, p, vec2(cw, h));
            ui.allocate_ui_with_layout(vec2(insp, h), Layout::top_down(Align::Min), |ui| self.inspector(ui, p, h));
        });
        self.run_bar(ui, p);
        if self.job.state().running {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &Palette) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Automação")).family(bold()).size(22.0).color(p.text));
            ui.add_space(10.0);
            let current = self.flow.as_ref().map(|f| f.name.clone()).unwrap_or_default();
            if let Some(name) = &mut self.renaming {
                let r = ui.add(egui::TextEdit::singleline(name).margin(widgets::FIELD_MARGIN).desired_width(300.0));
                r.request_focus();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let done = widgets::secondary(ui, p, "OK", true) || enter;
                if done || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    let new = name.trim().to_string();
                    self.renaming = None;
                    if done && !new.is_empty() && new != current {
                        if self.names.iter().any(|n| n.eq_ignore_ascii_case(&new)) {
                            self.notice = Some((false, "Já existe um fluxo com esse nome.".into()));
                        } else if let Some(f) = self.flow.as_mut() {
                            let old = f.name.clone();
                            f.name = new;
                            self.dirty = Some(Instant::now() - Duration::from_secs(1));
                            self.save_now();
                            let _ = flow::remove(&old);
                            self.load_list();
                        }
                    }
                }
                return;
            }
            egui::ComboBox::from_id_salt("flow_pick").selected_text(crate::i18n::tr(&current)).width(300.0).show_ui(ui, |ui| {
                for n in self.names.clone() {
                    if ui.selectable_label(n == current, crate::i18n::tr(&n)).clicked() && n != current {
                        self.open(&n);
                    }
                }
            });
            if widgets::secondary(ui, p, "+ Novo", true) {
                self.create();
            }
            let has = self.flow.is_some();
            if widgets::secondary(ui, p, "Renomear", has) {
                self.renaming = Some(current.clone());
            }
            if widgets::secondary(ui, p, "Duplicar", has) {
                self.save_now();
                if let Some(mut f) = self.flow.clone() {
                    f.name = self.unique_name(&format!("{} ({})", f.name, crate::i18n::tr("cópia")));
                    self.flow = Some(f);
                    self.dirty = Some(Instant::now() - Duration::from_secs(1));
                    self.save_now();
                    self.load_list();
                }
            }
            let confirming = self.confirm_delete.is_some_and(|t| t.elapsed() < Duration::from_secs(4));
            if widgets::secondary(ui, p, if confirming { "Clique de novo para excluir" } else { "Excluir" }, has) {
                if confirming {
                    let _ = flow::remove(&current);
                    self.flow = None;
                    self.dirty = None;
                    self.confirm_delete = None;
                    self.load_list();
                    if let Some(first) = self.names.first().cloned() {
                        self.open(&first);
                    }
                } else {
                    self.confirm_delete = Some(Instant::now());
                }
            }
            if let Some((good, text)) = &self.notice {
                ui.add_space(8.0);
                ui.label(RichText::new(crate::i18n::tr(text)).size(12.5).color(if *good { p.muted } else { p.accent }));
            }
        });
    }

    // ------------------------------------------------------------ lista de nodes

    /// Lista de tipos de node com busca (abre pelo botão "+ Adicionar node" do quadro).
    fn palette(&mut self, ui: &mut Ui, p: &Palette) {
        {
            ui.set_width(260.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .margin(widgets::FIELD_MARGIN)
                    .hint_text(crate::i18n::tr("🔍  Procurar node…"))
                    .desired_width(f32::INFINITY),
            );
            if !r.has_focus() && self.query.is_empty() {
                r.request_focus();
            }
            ui.add_space(4.0);
            let q = crate::search::normalize(self.query.trim());
            let mut add: Option<String> = None;
            egui::ScrollArea::vertical().id_salt("flow_palette").max_height(440.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for cat in CATEGORIES {
                    let items: Vec<&KindInfo> = self
                        .kinds
                        .iter()
                        .filter(|k| k.category == *cat)
                        .filter(|k| {
                            q.trim().is_empty() || {
                                let hay = crate::search::haystack(&k.title, &crate::i18n::tr(&k.title), cat, &crate::i18n::tr(*cat), &format!("{} {}", k.kind, k.description.to_lowercase()));
                                crate::search::matches(&q, &hay)
                            }
                        })
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(crate::i18n::tr(*cat)).family(bold()).size(11.0).color(p.muted));
                    for k in items {
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, 6, p.input);
                        }
                        let galley = fit_text(ui.painter(), &format!("+  {}", crate::i18n::tr(&k.title)), FontId::proportional(13.5), p.text, rect.width() - 12.0);
                        ui.painter().galley(pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
                        let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(&k.description);
                        if resp.clicked() {
                            add = Some(k.kind.clone());
                        }
                    }
                }
            });
            if let Some(kind) = add {
                self.add_node(&kind);
                self.query.clear();
                ui.close_menu();
            }
        }
    }

    fn add_node(&mut self, kind: &str) {
        let (pan, zoom) = (self.pan, self.zoom);
        let Some(f) = self.flow.as_mut() else { return };
        let id = f.next_id();
        // perto do centro do que está visível, um pouco deslocado para não empilhar
        let n = f.nodes.len() as f32;
        let world = (vec2(260.0, 160.0) - pan) / zoom + vec2((n % 5.0) * 26.0, (n % 5.0) * 22.0);
        f.nodes.push(Node { id, kind: kind.into(), title: String::new(), args: Map::new(), pos: [world.x, world.y] });
        self.sel = Some(Sel::Node(id));
        self.touch();
    }

    // ------------------------------------------------------------ quadro

    fn node_lines(&self, n: &Node) -> Vec<String> {
        let a = &n.args;
        let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let lines: Vec<String> = match n.kind.as_str() {
            "input" => {
                let k = a.get("files").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
                vec![if k == 0 { crate::i18n::tr("arraste arquivos para cá") } else { crate::i18n::tr(format!("{k} arquivo(s) ou pasta(s)")) }]
            }
            "text" => vec![s("text").unwrap_or_else(|| crate::i18n::tr("(vazio)"))],
            "ai_prompt" => vec![s("prompt").unwrap_or_else(|| crate::i18n::tr("escreva a instrução"))],
            "ai_gate" => vec![s("criteria").unwrap_or_else(|| crate::i18n::tr("escreva o critério"))],
            "output" => vec![s("folder").map(|f| tools::file_name(std::path::Path::new(&f))).unwrap_or_else(|| crate::i18n::tr("Documentos\\Conver"))],
            "filter" => {
                let t: Vec<String> = a.get("types").and_then(Value::as_array).map(|x| x.iter().filter_map(|v| v.as_str()).map(|v| crate::i18n::tr(enum_label(v))).collect()).unwrap_or_default();
                let mut v = Vec::new();
                if !t.is_empty() {
                    v.push(t.join(", "));
                }
                if let Some(e) = s("extensions") {
                    v.push(e);
                }
                if v.is_empty() {
                    v.push(crate::i18n::tr("tudo passa"));
                }
                v
            }
            _ => {
                let v: Vec<String> = a
                    .iter()
                    .filter(|(_, v)| !v.is_null() && v.as_str().is_none_or(|s| !s.is_empty()))
                    .take(3)
                    .map(|(k, v)| format!("{}: {}", crate::i18n::tr(arg_label(k)), value_text(v)))
                    .collect();
                if v.is_empty() { vec![crate::i18n::tr("ajustes padrão")] } else { v }
            }
        };
        lines.into_iter().take(3).collect()
    }

    fn node_size(&self, n: &Node) -> Vec2 {
        let lines = self.node_lines(n).len().max(1) as f32;
        vec2(NODE_W, HEADER + 8.0 + lines * 17.0 + 26.0)
    }

    fn canvas(&mut self, ui: &mut Ui, p: &Palette, size: Vec2) {
        let (rect, bg) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 12, p.bg);
        painter.rect_stroke(rect, 12, Stroke::new(1.0f32, p.border), StrokeKind::Inside);
        let Some(flow) = self.flow.clone() else {
            painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::tr("Crie um fluxo com \"+ Novo\"."), FontId::proportional(14.0), p.muted);
            return;
        };
        if self.fit_pending {
            self.fit(&flow, rect.size());
            self.fit_pending = false;
        }
        let z = self.zoom;
        let origin = rect.min.to_vec2() + self.pan;
        let to_screen = |w: [f32; 2]| pos2(w[0] * z, w[1] * z) + origin;
        // grade de pontos
        let step = 30.0 * z;
        if step > 14.0 {
            let start = vec2(self.pan.x.rem_euclid(step), self.pan.y.rem_euclid(step));
            let mut y = rect.top() + start.y;
            while y < rect.bottom() {
                let mut x = rect.left() + start.x;
                while x < rect.right() {
                    painter.circle_filled(pos2(x, y), 1.0, p.border);
                    x += step;
                }
                y += step;
            }
        }
        // geometria de cada node
        let geo: Vec<(u32, Rect)> = flow.nodes.iter().map(|n| (n.id, Rect::from_min_size(to_screen(n.pos), self.node_size(n) * z))).collect();
        let rect_of = |id: u32| geo.iter().find(|(i, _)| *i == id).map(|(_, r)| *r);
        let kind_outs: std::collections::HashMap<String, &'static [&'static str]> = self.kinds.iter().map(|k| (k.kind.clone(), k.outputs)).collect();
        let outputs_of = |kind: &str| -> &'static [&'static str] { kind_outs.get(kind).copied().unwrap_or(&["out"]) };
        let out_pos = |r: Rect, kind: &str, port: &str| -> Pos2 {
            let outs = outputs_of(kind);
            let i = outs.iter().position(|o| *o == port).unwrap_or(0) as f32;
            let y = if outs.len() == 1 { r.top() + HEADER * z / 2.0 + r.height() / 2.0 - HEADER * z / 4.0 } else { r.top() + r.height() * (0.45 + 0.3 * i) };
            pos2(r.right(), y)
        };
        let in_pos = |r: Rect| pos2(r.left(), r.top() + HEADER * z / 2.0 + r.height() / 2.0 - HEADER * z / 4.0);

        // ligações
        let mut link_paths: Vec<Vec<Pos2>> = Vec::new();
        for (i, l) in flow.links.iter().enumerate() {
            let (Some(a), Some(b)) = (rect_of(l.from), rect_of(l.to)) else {
                link_paths.push(Vec::new());
                continue;
            };
            let kind = flow.node(l.from).map(|n| n.kind.as_str()).unwrap_or("");
            let (s, e) = (out_pos(a, kind, &l.port), in_pos(b));
            let pts = bezier(s, e, z);
            let selected = self.sel == Some(Sel::Link(i));
            let color = if selected { p.accent } else { p.muted };
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(if selected { 3.0f32 } else { 2.0 }, color)));
            link_paths.push(pts);
        }
        // ligação sendo puxada
        if let (Some((from, port)), Some(ptr)) = (&self.dragging, ui.input(|i| i.pointer.hover_pos())) {
            if let (Some(a), Some(n)) = (rect_of(*from), flow.node(*from)) {
                painter.add(egui::Shape::line(bezier(out_pos(a, &n.kind, port), ptr, z), Stroke::new(2.0f32, p.accent)));
            }
        }

        // nodes
        let run = self.run.lock().unwrap().status.clone();
        let mut moved: Option<(u32, Vec2)> = None;
        let mut clicked_node: Option<u32> = None;
        let mut menu_action: Option<(u32, &'static str)> = None;
        for n in &flow.nodes {
            let r = rect_of(n.id).unwrap();
            if !rect.intersects(r) {
                continue;
            }
            let resp = ui.interact(r, ui.id().with(("flow_node", n.id)), Sense::click_and_drag());
            let selected = self.sel == Some(Sel::Node(n.id));
            let status = run.get(&n.id).cloned().unwrap_or(Status::Waiting);
            let stroke = if selected {
                Stroke::new(2.0f32, p.accent)
            } else if matches!(status, Status::Running(_)) {
                Stroke::new(1.5f32, p.accent)
            } else if resp.hovered() {
                Stroke::new(1.0f32, p.muted)
            } else {
                Stroke::new(1.0f32, p.border)
            };
            let cr = (10.0 * z).round() as u8;
            painter.rect_filled(r, cr, p.card);
            painter.rect_stroke(r, cr, stroke, StrokeKind::Inside);
            let info = self.info(&n.kind);
            let ai = n.kind.starts_with("ai_");
            // cabeçalho: categoria e nome; os nodes de IA ganham uma faixa de cinábrio suave
            let head = Rect::from_min_size(r.min, vec2(r.width(), HEADER * z));
            if ai {
                painter.rect_filled(head.shrink(1.0), egui::CornerRadius { nw: cr, ne: cr, sw: 0, se: 0 }, p.accent_soft);
            }
            let cat = info.map(|k| crate::i18n::tr(k.category)).unwrap_or_default();
            let pad = 10.0 * z;
            painter.text(pos2(r.left() + pad, r.top() + 5.0 * z), Align2::LEFT_TOP, cat, FontId::new(9.5 * z, bold()), p.muted);
            let title = fit_text(&painter, &self.title_of(n), FontId::new(13.5 * z, bold()), p.text, r.width() - 2.0 * pad);
            painter.galley(pos2(r.left() + pad, r.top() + 15.0 * z), title, p.text);
            painter.line_segment([pos2(r.left(), head.bottom()), pos2(r.right(), head.bottom())], Stroke::new(1.0f32, p.border));
            // resumo dos ajustes
            let mut y = head.bottom() + 6.0 * z;
            for line in self.node_lines(n) {
                let g = fit_text(&painter, &line, FontId::proportional(12.0 * z), p.muted, r.width() - 2.0 * pad - if info.is_some_and(|k| k.outputs.len() > 1) { 30.0 * z } else { 0.0 });
                painter.galley(pos2(r.left() + pad, y), g, p.muted);
                y += 17.0 * z;
            }
            // situação na última execução
            let sy = r.bottom() - 20.0 * z;
            let (text, color) = match &status {
                Status::Waiting => (String::new(), p.muted),
                Status::Running(f) => {
                    let bar = Rect::from_min_size(pos2(r.left() + pad, r.bottom() - 9.0 * z), vec2((r.width() - 2.0 * pad) * f.max(0.03), 3.0 * z));
                    painter.rect_filled(bar, 2, p.accent);
                    (crate::i18n::tr("rodando…"), p.accent)
                }
                Status::Done(note) => (format!("✔ {}", crate::i18n::tr(note)), p.ok),
                Status::Skipped(why) => (format!("— {}", crate::i18n::tr(why)), p.muted),
                Status::Failed(e) => (format!("✖ {}", crate::i18n::tr(e)), p.accent),
            };
            if !text.is_empty() && !matches!(status, Status::Running(_)) {
                let g = fit_text(&painter, &text, FontId::proportional(11.5 * z), color, r.width() - 2.0 * pad);
                painter.galley(pos2(r.left() + pad, sy), g, color);
            }
            // portas
            if info.is_some_and(|k| k.has_input) {
                port(&painter, in_pos(r), z, p, false);
            }
            for o in outputs_of(&n.kind) {
                let pp = out_pos(r, &n.kind, o);
                if outputs_of(&n.kind).len() > 1 {
                    let label = crate::i18n::tr(if *o == "yes" { "sim" } else { "não" });
                    painter.text(pp - vec2(12.0 * z, 0.0), Align2::RIGHT_CENTER, label, FontId::new(11.0 * z, bold()), if *o == "yes" { p.ok } else { p.accent });
                }
            }
            let resp = match &status {
                Status::Failed(e) => resp.on_hover_text(crate::i18n::tr(e)),
                _ => resp,
            };
            if resp.dragged() && self.dragging.is_none() {
                moved = Some((n.id, resp.drag_delta() / z));
            }
            if resp.clicked() || resp.drag_started() {
                clicked_node = Some(n.id);
            }
            resp.context_menu(|ui| {
                if ui.button(crate::i18n::tr("Duplicar")).clicked() {
                    menu_action = Some((n.id, "dup"));
                    ui.close_menu();
                }
                if ui.button(crate::i18n::tr("Remover")).clicked() {
                    menu_action = Some((n.id, "del"));
                    ui.close_menu();
                }
            });
        }
        // portas de saída por cima dos nodes (para puxar ligações)
        let mut link_drop: Option<(u32, String, Pos2)> = None;
        for n in &flow.nodes {
            let Some(r) = rect_of(n.id) else { continue };
            for o in outputs_of(&n.kind) {
                let pp = out_pos(r, &n.kind, o);
                let hit = Rect::from_center_size(pp, vec2(18.0, 18.0) * z.max(0.8));
                let resp = ui.interact(hit, ui.id().with(("flow_port", n.id, *o)), Sense::drag()).on_hover_cursor(CursorIcon::Crosshair);
                port(&painter, pp, z, p, resp.hovered() || self.dragging.as_ref().is_some_and(|(f, po)| *f == n.id && po == o));
                if resp.drag_started() {
                    self.dragging = Some((n.id, o.to_string()));
                }
                if resp.drag_stopped() {
                    if let Some(ptr) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos())) {
                        link_drop = Some((n.id, o.to_string(), ptr));
                    }
                    self.dragging = None;
                }
            }
        }
        if let Some((from, port_name, ptr)) = link_drop {
            let target = geo.iter().find(|(id, r)| *id != from && r.expand(10.0).contains(ptr)).map(|(id, _)| *id);
            if let Some(to) = target {
                let accepts = flow.node(to).and_then(|n| self.info(&n.kind)).is_some_and(|k| k.has_input);
                let f = self.flow.as_mut().unwrap();
                if !accepts {
                    self.notice = Some((false, "Esse node não recebe entradas.".into()));
                } else if f.would_cycle(from, to) {
                    self.notice = Some((false, "Essa ligação criaria um ciclo.".into()));
                } else if !f.links.iter().any(|l| l.from == from && l.to == to && l.port == port_name) {
                    f.links.push(Link { from, port: port_name, to });
                    self.notice = None;
                    self.touch();
                }
            }
        }
        if let Some((id, d)) = moved {
            if let Some(n) = self.flow.as_mut().and_then(|f| f.node_mut(id)) {
                n.pos[0] += d.x;
                n.pos[1] += d.y;
                self.touch();
            }
        }
        if let Some(id) = clicked_node {
            self.sel = Some(Sel::Node(id));
        }
        if let Some((id, what)) = menu_action {
            match what {
                "dup" => self.duplicate(id),
                _ => self.remove_node(id),
            }
        }
        // fundo: arrastar move o quadro; clicar escolhe uma ligação ou tira a seleção
        if bg.dragged() && self.dragging.is_none() {
            self.pan += bg.drag_delta();
        }
        if bg.clicked() {
            let ptr = bg.interact_pointer_pos().unwrap_or_default();
            self.sel = link_paths
                .iter()
                .enumerate()
                .find(|(_, pts)| pts.windows(2).any(|w| dist_to_segment(ptr, w[0], w[1]) < 7.0))
                .map(|(i, _)| Sel::Link(i));
        }
        if bg.hovered() || ui.rect_contains_pointer(rect) {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                if let Some(ptr) = ui.input(|i| i.pointer.hover_pos()) {
                    let before = (ptr.to_vec2() - origin) / self.zoom;
                    self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.3, 1.8);
                    self.pan = ptr - rect.min - before * self.zoom;
                }
            }
        }
        // Delete apaga o que está escolhido (se nenhum campo de texto estiver em uso)
        if !ui.ctx().wants_keyboard_input() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            match self.sel {
                Some(Sel::Node(id)) => self.remove_node(id),
                Some(Sel::Link(i)) => self.remove_link(i),
                None => {}
            }
        }
        // botões do quadro
        let add_bar = Rect::from_min_size(pos2(rect.left() + 10.0, rect.top() + 10.0), vec2(220.0, 30.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(add_bar).layout(Layout::left_to_right(Align::Center)), |ui| {
            let b = egui::Button::new(RichText::new(crate::i18n::tr("+  Adicionar node")).family(bold()).color(p.text))
                .fill(p.card)
                .stroke(Stroke::new(1.0f32, p.border))
                .corner_radius(8)
                .min_size(vec2(0.0, 30.0));
            egui::menu::menu_custom_button(ui, b, |ui| self.palette(ui, p));
        });
        let bar = Rect::from_min_size(pos2(rect.right() - 136.0, rect.top() + 10.0), vec2(126.0, 30.0));
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(bar).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if ui.add(egui::Button::new(crate::i18n::tr("Ajustar")).min_size(vec2(0.0, 28.0))).on_hover_text(crate::i18n::tr("Mostrar o fluxo inteiro")).clicked() {
                self.fit_pending = true;
            }
            if ui.add(egui::Button::new("+").min_size(vec2(28.0, 28.0))).clicked() {
                self.zoom_at(rect, 1.15);
            }
            if ui.add(egui::Button::new("−").min_size(vec2(28.0, 28.0))).clicked() {
                self.zoom_at(rect, 1.0 / 1.15);
            }
        });
        if flow.nodes.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, crate::i18n::tr("Adicione nodes pelo botão \"+ Adicionar node\"."), FontId::proportional(14.0), p.muted);
        } else if flow.links.is_empty() {
            painter.text(
                pos2(rect.center().x, rect.bottom() - 22.0),
                Align2::CENTER_CENTER,
                crate::i18n::tr("Puxe da bolinha à direita de um node até outro node para ligar os dois."),
                FontId::proportional(13.0),
                p.muted,
            );
        }
    }

    fn zoom_at(&mut self, rect: Rect, factor: f32) {
        let c = rect.center();
        let before = (c - rect.min - self.pan) / self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.3, 1.8);
        self.pan = c - rect.min - before * self.zoom;
    }

    fn fit(&mut self, flow: &Flow, size: Vec2) {
        if flow.nodes.is_empty() {
            self.pan = vec2(30.0, 30.0);
            self.zoom = 1.0;
            return;
        }
        let mut min = pos2(f32::MAX, f32::MAX);
        let mut max = pos2(f32::MIN, f32::MIN);
        for n in &flow.nodes {
            let s = self.node_size(n);
            min = min.min(pos2(n.pos[0], n.pos[1]));
            max = max.max(pos2(n.pos[0] + s.x, n.pos[1] + s.y));
        }
        let bbox = max - min;
        self.zoom = ((size.x - 60.0) / bbox.x).min((size.y - 110.0) / bbox.y).clamp(0.3, 1.1);
        // um pouco abaixo do centro, para não ficar embaixo dos botões do quadro
        self.pan = (size - bbox * self.zoom) / 2.0 + vec2(0.0, 18.0) - min.to_vec2() * self.zoom;
    }

    fn duplicate(&mut self, id: u32) {
        let Some(f) = self.flow.as_mut() else { return };
        if let Some(n) = f.node(id).cloned() {
            let new = f.next_id();
            f.nodes.push(Node { id: new, pos: [n.pos[0] + 30.0, n.pos[1] + 30.0], ..n });
            self.sel = Some(Sel::Node(new));
            self.touch();
        }
    }

    fn remove_node(&mut self, id: u32) {
        if let Some(f) = self.flow.as_mut() {
            f.remove_node(id);
            self.sel = None;
            self.touch();
        }
    }

    fn remove_link(&mut self, i: usize) {
        if let Some(f) = self.flow.as_mut() {
            if i < f.links.len() {
                f.links.remove(i);
                self.sel = None;
                self.touch();
            }
        }
    }

    // ------------------------------------------------------------ ajustes

    fn inspector(&mut self, ui: &mut Ui, p: &Palette, h: f32) {
        Frame::default().fill(p.card).stroke(Stroke::new(1.0f32, p.border)).corner_radius(12).inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.set_min_height(h - 26.0);
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt("flow_inspector").auto_shrink([false, false]).show(ui, |ui| match self.sel {
                Some(Sel::Node(id)) if self.flow.as_ref().is_some_and(|f| f.node(id).is_some()) => self.node_inspector(ui, p, id),
                Some(Sel::Link(i)) if self.flow.as_ref().is_some_and(|f| i < f.links.len()) => {
                    let f = self.flow.as_ref().unwrap();
                    let l = &f.links[i];
                    let name = |id: u32| f.node(id).map(|n| self.title_of(n)).unwrap_or_default();
                    ui.label(RichText::new(crate::i18n::tr("Ligação")).family(bold()).size(16.0).color(p.text));
                    let port = match l.port.as_str() {
                        "yes" => format!(" ({})", crate::i18n::tr("sim")),
                        "no" => format!(" ({})", crate::i18n::tr("não")),
                        _ => String::new(),
                    };
                    ui.label(RichText::new(format!("{}{port}  →  {}", name(l.from), name(l.to))).color(p.muted));
                    ui.add_space(8.0);
                    if widgets::secondary(ui, p, "Remover ligação", true) {
                        self.remove_link(i);
                    }
                    widgets::hint(ui, p, "Dica: a tecla Delete também remove.");
                }
                _ => self.flow_inspector(ui, p),
            });
        });
    }

    fn flow_inspector(&mut self, ui: &mut Ui, p: &Palette) {
        let Some(f) = self.flow.as_mut() else { return };
        ui.label(RichText::new(crate::i18n::tr("Fluxo")).family(bold()).size(16.0).color(p.text));
        ui.label(RichText::new(crate::i18n::tr(&f.name)).color(p.muted));
        ui.add_space(8.0);
        ui.label(RichText::new(crate::i18n::tr("Descrição")).color(p.text));
        if ui.add(egui::TextEdit::multiline(&mut f.description).desired_rows(3).desired_width(f32::INFINITY)).changed() {
            self.dirty = Some(Instant::now());
        }
        ui.add_space(10.0);
        ui.label(RichText::new(crate::i18n::tr("Como funciona")).family(bold()).color(p.text));
        for t in [
            "1. Adicione nodes pelo botão \"+ Adicionar node\" (ferramentas, IA, filtros).",
            "2. Puxe da bolinha à direita de um node até outro para ligar: o resultado de um vai para o próximo.",
            "3. Clique num node para ajustar; arraste arquivos para a janela para colocá-los na entrada.",
            "4. Clique em Executar fluxo.",
        ] {
            widgets::hint(ui, p, t);
        }
        ui.add_space(6.0);
        widgets::hint(ui, p, "Os nodes de IA usam a API configurada em Configurações. Agentes de IA também criam e rodam fluxos pelo MCP (flow_save, flow_run).");
        let ai = ai_api::Settings::load().config();
        if let Err(e) = &ai {
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr(format!("⚠ IA: {e}"))).size(12.5).color(p.accent));
            if widgets::secondary(ui, p, "Configurar a IA", true) {
                self.go_settings = true;
            }
        }
    }

    fn node_inspector(&mut self, ui: &mut Ui, p: &Palette, id: u32) {
        let node = self.flow.as_ref().unwrap().node(id).unwrap().clone();
        let Some(info) = self.info(&node.kind).cloned() else {
            ui.label(RichText::new(crate::i18n::tr(format!("Tipo desconhecido: {}", node.kind))).color(p.accent));
            return;
        };
        let mut n = node.clone();
        ui.label(RichText::new(crate::i18n::tr(info.category)).family(bold()).size(11.0).color(p.muted));
        ui.label(RichText::new(crate::i18n::tr(&info.title)).family(bold()).size(16.0).color(p.text));
        ui.add_space(4.0);
        ui.label(RichText::new(crate::i18n::tr("Nome no quadro")).color(p.text));
        ui.add(egui::TextEdit::singleline(&mut n.title).margin(widgets::FIELD_MARGIN).hint_text(crate::i18n::tr(&info.title)).desired_width(f32::INFINITY));
        ui.add_space(6.0);
        match n.kind.as_str() {
            "input" => self.input_editor(ui, p, &mut n),
            "output" => {
                ui.label(RichText::new(crate::i18n::tr("Pasta de destino")).color(p.text));
                let mut folder = n.args.get("folder").and_then(Value::as_str).unwrap_or_default().to_string();
                ui.horizontal(|ui| {
                    if ui.add(egui::TextEdit::singleline(&mut folder).margin(widgets::FIELD_MARGIN).hint_text(crate::i18n::tr("Documentos\\Conver\\<fluxo>")).desired_width(ui.available_width() - 90.0)).changed() {
                        n.args.insert("folder".into(), json!(folder));
                    }
                    if widgets::secondary(ui, p, "Escolher…", true) {
                        if let Some(d) = rfd::FileDialog::new().pick_folder() {
                            n.args.insert("folder".into(), json!(d.display().to_string()));
                        }
                    }
                });
                for key in ["move", "save_texts"] {
                    arg_editor(ui, p, key, &info.schema["properties"][key], &mut n.args);
                }
            }
            _ => {
                if n.kind.starts_with("ai_") {
                    if let Err(e) = ai_api::Settings::load().config() {
                        ui.label(RichText::new(crate::i18n::tr(format!("⚠ IA: {e}"))).size(12.5).color(p.accent));
                        if widgets::secondary(ui, p, "Configurar a IA", true) {
                            self.go_settings = true;
                        }
                        ui.add_space(4.0);
                    }
                }
                if let Some(props) = info.schema["properties"].as_object() {
                    for (key, prop) in props {
                        arg_editor(ui, p, key, prop, &mut n.args);
                    }
                }
                if n.kind.starts_with("ai_") || n.kind.starts_with("tool:") {
                    widgets::hint(ui, p, "Nos textos, {text} vira o texto que chega e {file} o nome do arquivo.");
                }
            }
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::secondary(ui, p, "Duplicar", true) {
                self.duplicate(id);
            }
            if widgets::secondary(ui, p, "Remover node", true) {
                self.remove_node(id);
            }
        });
        let status = self.run.lock().unwrap().status.get(&id).cloned();
        if let Some(Status::Failed(e)) = status {
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr(format!("✖ {e}"))).size(12.5).color(p.accent));
        }
        if self.flow.as_ref().and_then(|f| f.node(id)).is_some_and(|cur| *cur != n) {
            if let Some(cur) = self.flow.as_mut().and_then(|f| f.node_mut(id)) {
                *cur = n;
            }
            self.touch();
        }
    }

    fn input_editor(&mut self, ui: &mut Ui, p: &Palette, n: &mut Node) {
        let mut files: Vec<String> = n.args.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
        let before = files.clone();
        ui.label(RichText::new(crate::i18n::tr("Arquivos e pastas")).color(p.text));
        if files.is_empty() {
            widgets::hint(ui, p, "Nenhum ainda: arraste arquivos para a janela ou use os botões abaixo.");
        }
        let mut remove = None;
        for (i, f) in files.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button("×").on_hover_text(crate::i18n::tr("Remover da lista")).clicked() {
                    remove = Some(i);
                }
                let name = tools::file_name(std::path::Path::new(f));
                ui.add(egui::Label::new(RichText::new(name).color(p.text)).truncate()).on_hover_text(f);
            });
        }
        if let Some(i) = remove {
            files.remove(i);
        }
        ui.horizontal(|ui| {
            if widgets::secondary(ui, p, "+ Arquivos…", true) {
                if let Some(list) = rfd::FileDialog::new().pick_files() {
                    files.extend(list.into_iter().map(|p| p.display().to_string()));
                }
            }
            if widgets::secondary(ui, p, "+ Pasta…", true) {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    files.push(d.display().to_string());
                }
            }
            if widgets::secondary(ui, p, "Limpar", !files.is_empty()) {
                files.clear();
            }
        });
        widgets::hint(ui, p, "Pela linha de comando ou pelo MCP, os arquivos passados em flow_run substituem estes.");
        if files != before {
            n.args.insert("files".into(), json!(files));
        }
    }

    // ------------------------------------------------------------ execução

    fn run_bar(&mut self, ui: &mut Ui, p: &Palette) {
        let can = self.flow.as_ref().is_some_and(|f| !f.nodes.is_empty());
        match widgets::action_bar(ui, p, &self.job, "▶  Executar fluxo", can) {
            Act::Start => {
                self.save_now();
                let Some(f) = self.flow.clone() else { return };
                if let Err(e) = f.validate() {
                    self.notice = Some((false, format!("✖ {e}")));
                    return;
                }
                self.notice = None;
                let run = self.run.clone();
                let ai = ai_api::Settings::load().config();
                self.job.start(ui.ctx(), move |ctl| {
                    ctl.status(format!("Executando “{}”…", f.name));
                    let progress = |x: f32| ctl.progress(x);
                    let cancelled = || ctl.cancelled();
                    let log = |s: &str| {
                        let kind = if s.starts_with('✔') || s.trim_start().starts_with('✔') {
                            LogKind::Ok
                        } else if s.trim_start().starts_with('✖') {
                            LogKind::Err
                        } else {
                            LogKind::Info
                        };
                        ctl.log(kind, s);
                    };
                    let ctx = ops::Ctx { progress: &progress, cancelled: &cancelled, log: &log };
                    match flow::run(&f, &flow::Inputs::default(), None, &run, &ctx, &ai) {
                        Ok(report) => {
                            let failed = report["failed_nodes"].as_u64().unwrap_or(0);
                            let n = report["results"].as_array().map(Vec::len).unwrap_or(0);
                            ctl.status(if failed > 0 { format!("Concluído com {failed} node(s) com erro.") } else { format!("Concluído: {n} resultado(s).") });
                            // "Abrir pasta": a pasta do primeiro resultado final, ou a pasta de trabalho
                            let folder = report["results"]
                                .as_array()
                                .and_then(|r| r.iter().find_map(|i| i["file"].as_str()))
                                .and_then(|f| std::path::Path::new(f).parent().map(|d| d.to_path_buf()))
                                .or_else(|| report["work_dir"].as_str().map(PathBuf::from));
                            if let Some(d) = folder {
                                ctl.done_folder(d);
                            }
                        }
                        Err(e) => {
                            ctl.log(LogKind::Err, format!("✖ {e}"));
                            ctl.status(if e == "Cancelado." { "Cancelado.".to_string() } else { "Erro.".to_string() });
                        }
                    }
                    ctl.progress(1.0);
                });
            }
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }
}

// ================================================================ ajudantes

fn port(painter: &egui::Painter, at: Pos2, z: f32, p: &Palette, hot: bool) {
    let r = 6.0 * z.max(0.7);
    painter.circle_filled(at, r, if hot { p.accent } else { p.card });
    painter.circle_stroke(at, r, Stroke::new(2.0f32, if hot { p.accent } else { p.muted }));
}

fn bezier(a: Pos2, b: Pos2, z: f32) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() * 0.5).max(60.0 * z);
    let (c1, c2) = (a + vec2(dx, 0.0), b - vec2(dx, 0.0));
    (0..=32)
        .map(|i| {
            let t = i as f32 / 32.0;
            let u = 1.0 - t;
            let v = a.to_vec2() * (u * u * u) + c1.to_vec2() * (3.0 * u * u * t) + c2.to_vec2() * (3.0 * u * t * t) + b.to_vec2() * (t * t * t);
            v.to_pos2()
        })
        .collect()
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (a + ab * t).distance(p)
}

/// Texto numa linha só, cortado com "…" se passar da largura.
fn fit_text(painter: &egui::Painter, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.replace('\n', " "), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping { max_width: width.max(10.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => crate::i18n::tr(enum_label(s)),
        Value::Bool(b) => crate::i18n::tr(if *b { "sim" } else { "não" }),
        Value::Array(a) => a.iter().map(value_text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// Editor de um argumento a partir do esquema (lista, sim/não, número, texto, lista de textos, JSON).
fn arg_editor(ui: &mut Ui, p: &Palette, key: &str, prop: &Value, args: &mut Map<String, Value>) {
    let label = crate::i18n::tr(arg_label(key));
    let desc = prop["description"].as_str().unwrap_or_default();
    let ty = prop["type"].as_str().unwrap_or("string");
    let current = args.get(key).cloned().filter(|v| !v.is_null());
    let default = prop.get("default").cloned();
    ui.add_space(4.0);
    if ty == "boolean" {
        let mut b = current.as_ref().and_then(Value::as_bool).or(default.as_ref().and_then(Value::as_bool)).unwrap_or(false);
        if ui.checkbox(&mut b, label).on_hover_text(desc).changed() {
            args.insert(key.into(), json!(b));
        }
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(&label).color(p.text)).on_hover_text(desc);
        if current.is_some() && ui.small_button("↺").on_hover_text(crate::i18n::tr("Voltar ao padrão")).clicked() {
            args.remove(key);
        }
    });
    if let Some(values) = prop["enum"].as_array() {
        let cur = current.as_ref().and_then(Value::as_str).or(default.as_ref().and_then(Value::as_str)).unwrap_or_default().to_string();
        egui::ComboBox::from_id_salt(("arg", key)).selected_text(crate::i18n::tr(enum_label(&cur))).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
            for v in values.iter().filter_map(Value::as_str) {
                if ui.selectable_label(v == cur, crate::i18n::tr(enum_label(v))).clicked() {
                    args.insert(key.into(), json!(v));
                }
            }
        });
        return;
    }
    match ty {
        "integer" | "number" => {
            let min = prop["minimum"].as_f64().unwrap_or(f64::MIN);
            let max = prop["maximum"].as_f64().unwrap_or(f64::MAX);
            let has = current.is_some();
            let mut v = current.as_ref().and_then(Value::as_f64).or(default.as_ref().and_then(Value::as_f64)).unwrap_or(0.0);
            ui.horizontal(|ui| {
                let speed = if max - min < 300.0 { 0.2 } else { 1.0 };
                let r = ui.add(egui::DragValue::new(&mut v).range(min..=max).speed(speed).max_decimals(if ty == "integer" { 0 } else { 2 }));
                if r.changed() {
                    args.insert(key.into(), if ty == "integer" { json!(v.round() as i64) } else { json!(v) });
                }
                if !has {
                    ui.label(RichText::new(crate::i18n::tr(if default.is_some() { "(padrão)" } else { "(automático)" })).size(12.0).color(p.muted));
                }
            });
        }
        "array" => {
            if let Some(items) = prop["items"]["enum"].as_array() {
                let mut chosen: Vec<String> = current.as_ref().and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                let before = chosen.clone();
                ui.horizontal_wrapped(|ui| {
                    for v in items.iter().filter_map(Value::as_str) {
                        let mut on = chosen.iter().any(|c| c == v);
                        if ui.checkbox(&mut on, crate::i18n::tr(enum_label(v))).changed() {
                            if on {
                                chosen.push(v.to_string());
                            } else {
                                chosen.retain(|c| c != v);
                            }
                        }
                    }
                });
                if chosen != before {
                    args.insert(key.into(), json!(chosen));
                }
            } else {
                let mut text: String = current.as_ref().and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap_or_default();
                if ui.add(egui::TextEdit::multiline(&mut text).desired_rows(2).hint_text(crate::i18n::tr("um por linha")).desired_width(f32::INFINITY)).changed() {
                    let list: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
                    args.insert(key.into(), json!(list));
                }
            }
        }
        "object" => {
            let mut text = current.as_ref().map(|v| v.to_string()).unwrap_or_default();
            if ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_rows(2).desired_width(f32::INFINITY)).changed() {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    args.insert(key.into(), v);
                }
            }
        }
        _ => {
            let mut s = current.as_ref().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let long = matches!(key, "prompt" | "criteria" | "text");
            let edit = if long {
                egui::TextEdit::multiline(&mut s).desired_rows(4)
            } else {
                egui::TextEdit::singleline(&mut s).margin(widgets::FIELD_MARGIN)
            };
            if ui.add(edit.desired_width(f32::INFINITY)).on_hover_text(desc).changed() {
                if s.is_empty() {
                    args.remove(key);
                } else {
                    args.insert(key.into(), json!(s));
                }
            }
        }
    }
}

/// Nomes em português dos argumentos das ferramentas (no esquema eles estão em inglês, para os agentes).
fn arg_label(key: &str) -> &str {
    match key {
        "format" => "Formato",
        "quality" => "Qualidade",
        "level" => "Nível",
        "language" => "Idioma",
        "width" => "Largura",
        "height" => "Altura",
        "percent" => "Porcentagem",
        "keep_aspect" => "Manter a proporção",
        "start" => "Início",
        "end" => "Fim",
        "duration" => "Duração (s)",
        "size" => "Tamanho",
        "rotate" => "Girar (graus)",
        "precise" => "Corte preciso",
        "pages" => "Páginas",
        "fps" => "Quadros por segundo",
        "flip_horizontal" => "Espelhar na horizontal",
        "flip_vertical" => "Espelhar na vertical",
        "codec" => "Codificação",
        "window" => "Janela",
        "volume" => "Volume",
        "urls" => "Links",
        "translate_to_english" => "Traduzir para o inglês",
        "text" => "Texto",
        "target_mb" => "Tamanho final (MB)",
        "subtitles" => "Baixar legendas",
        "subtitle" => "Arquivo de legenda",
        "stems" => "Salvar",
        "skip_pages_with_text" => "Pular páginas que já têm texto",
        "show_cursor" => "Mostrar o cursor",
        "seconds" => "Segundos",
        "scale" => "Escala",
        "saturation" => "Saturação",
        "resolution" => "Resolução",
        "replace_audio" => "Substituir o áudio",
        "region" => "Área",
        "position" => "Posição",
        "playlist" => "Playlist inteira",
        "page_size" => "Tamanho da página",
        "owner_password" => "Senha do dono",
        "password" => "Senha",
        "music" => "Música",
        "mono" => "Mono",
        "model" => "Modelo",
        "microphone" => "Microfone",
        "lufs" => "Volume alvo (LUFS)",
        "loop" => "Repetir",
        "kind" => "Tipo",
        "keep_audio" => "Manter o áudio",
        "kbps" => "Kbps",
        "formats" => "Salvar como",
        "filter" => "Filtro",
        "factor" => "Fator",
        "every_seconds" => "A cada (s)",
        "edge" => "Borda",
        "dpi" => "Resolução (DPI)",
        "crop_aspect" => "Recortar",
        "cover" => "Baixar a capa",
        "contrast" => "Contraste",
        "color" => "Cor",
        "browser_cookies" => "Login do navegador",
        "brightness" => "Brilho",
        "background" => "Fundo",
        "audio_kbps" => "Áudio (kbps)",
        "aspect" => "Proporção",
        "allow_print" => "Permitir imprimir",
        "allow_copy" => "Permitir copiar",
        "output" => "Arquivo de saída",
        "name" => "Nome",
        "prompt" => "Instrução para a IA",
        "per_item" => "Uma resposta por arquivo",
        "keep_files" => "Passar os arquivos adiante",
        "save_as" => "Salvar a resposta como",
        "criteria" => "Critério",
        "types" => "Tipos aceitos",
        "extensions" => "Extensões aceitas",
        "min_mb" => "Tamanho mínimo (MB)",
        "max_mb" => "Tamanho máximo (MB)",
        "move" => "Mover em vez de copiar",
        "save_texts" => "Salvar os textos como .txt",
        other => other,
    }
}

/// Nomes em português dos valores mais comuns das listas.
fn enum_label(v: &str) -> &str {
    match v {
        "light" => "Leve",
        "medium" => "Média",
        "strong" => "Forte",
        "low" => "Baixa",
        "high" => "Alta",
        "fast" => "Rápido",
        "balanced" => "Equilibrado",
        "accurate" => "Preciso",
        "both" => "Voz e instrumental",
        "vocals" => "Só a voz",
        "instrumental" => "Só o instrumental",
        "none" => "Nenhum",
        "original" => "Original",
        "transparent" => "Transparente",
        "white" => "Branco",
        "black" => "Preto",
        "grayscale" => "Preto e branco",
        "sepia" => "Sépia",
        "invert" => "Inverter",
        "top" => "Em cima",
        "bottom" => "Embaixo",
        "small" => "Pequeno",
        "large" => "Grande",
        "soft" => "Suave",
        "hard" => "Definida",
        "image" => "Imagens",
        "video" => "Vídeos",
        "audio" => "Áudios",
        "pdf" => "PDFs",
        "text" => "Textos",
        "a4_portrait" => "A4 em pé",
        "a4_landscape" => "A4 deitado",
        other => other,
    }
}
