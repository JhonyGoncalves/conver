//! Tela de automação: editor de fluxos por nodes. À esquerda os tipos de node, no meio o quadro (arrastar para
//! mover, puxar da saída ● de um node até outro para ligar, rolar para aproximar), à direita os ajustes do node
//! escolhido e embaixo a execução. Tudo é salvo sozinho em `fluxos/`, o mesmo lugar que o MCP usa.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Frame, Margin, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
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
    /// Fluxo de demonstração aberto (só nas capturas): não existe no disco, então não é recarregado nem gravado.
    showcase: bool,
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
    /// Configuração da IA lida de `ia.json` (relida a cada 2 s, não a cada quadro: abrir a chave usa o DPAPI).
    ai_cache: Option<(Instant, Result<ai_api::Config, String>)>,
    /// Últimas execuções do fluxo aberto (relidas do disco a cada poucos segundos).
    history: Option<(String, Instant, Vec<Value>)>,
    /// Gatilho de horário já enviado ao Agendador do Windows, por fluxo (para só sincronizar quando muda).
    sched_synced: std::collections::HashMap<String, (String, Vec<u8>, bool)>,
    sched_msg: Arc<Mutex<Option<(bool, String)>>>,
    /// Miniaturas dos resultados de cada node (criado no primeiro uso).
    thumbs: Option<widgets::Thumbs>,
}

impl FlowsPage {
    pub fn new() -> Self {
        Self {
            kinds: Vec::new(),
            names: Vec::new(),
            flow: None,
            showcase: false,
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
            ai_cache: None,
            history: None,
            sched_synced: Default::default(),
            sched_msg: Arc::default(),
            thumbs: None,
        }
    }

    /// Modo de captura: abre o primeiro exemplo, escolhe o node de IA e mostra uma execução de mentira.
// i18n: fora
    pub fn demo(&mut self) {
        self.kinds = flow::kinds();
        flow::ensure_examples();
        self.load_list();
        // CONVER_FLUXO=vitrine: o fluxo grande de demonstração, no meio da execução (não grava nada)
        if std::env::var("CONVER_FLUXO").is_ok_and(|v| v == "vitrine") {
            self.flow = Some(flow::showcase());
            self.showcase = true;
            self.sel = Some(Sel::Node(6));
            self.fit_pending = true;
            let mut r = self.run.lock().unwrap();
            for (id, note) in [(1, "3 arquivo(s)"), (2, "7 corte(s)"), (3, "7 item(ns)"), (4, "5 sim, 2 não"), (5, "2 item(ns)"), (10, "7 legenda(s)"), (13, "7 texto(s)")] {
                r.status.insert(id, Status::Done(note.into()));
            }
            r.status.insert(6, Status::Running(0.57));
            r.status.insert(11, Status::Running(0.3));
            for id in [7, 8, 9, 12, 14] {
                r.status.insert(id, Status::Waiting);
            }
            r.status.insert(14, Status::Done("7 arquivo(s)".into()));
            drop(r);
            // a barra de baixo como no meio de uma execução de verdade
            let mut st = self.job.state();
            st.running = true;
            st.progress = 0.62;
            st.status = "Legendas animadas: corte 4 de 7…".into();
            return;
        }
        // CONVER_FLUXO=<nome>: abre esse fluxo com o node 2 escolhido
        if let Some(name) = std::env::var("CONVER_FLUXO").ok().filter(|n| self.names.contains(n)) {
            self.open(&name);
            // CONVER_FLUXO_PAINEL: sem node escolhido, para mostrar o painel do fluxo (gatilhos e histórico)
            // CONVER_FLUXO_NODE=<id>: escolhe outro node (padrão: 2)
            let id = std::env::var("CONVER_FLUXO_NODE").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
            self.sel = if std::env::var_os("CONVER_FLUXO_PAINEL").is_some() { None } else { Some(Sel::Node(id)) };
            return;
        }
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
    fn ai(&mut self) -> Result<ai_api::Config, String> {
        if self.ai_cache.as_ref().is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(2)) {
            self.ai_cache = Some((Instant::now(), ai_api::Settings::load().config()));
        }
        self.ai_cache.as_ref().unwrap().1.clone()
    }

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
            self.sched_synced.insert(f.name.clone(), (f.trigger.time.clone(), f.trigger.days.clone(), f.trigger.notify));
            *self.sched_msg.lock().unwrap() = None;
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
            let now = (f.trigger.time.clone(), f.trigger.days.clone(), f.trigger.notify);
            let before = self.sched_synced.get(&f.name).cloned().unwrap_or_default();
            let valid = f.trigger.time.trim().is_empty() || crate::triggers::parse_time(&f.trigger.time).is_some();
            if now != before && valid {
                self.sched_synced.insert(f.name.clone(), now);
                let (name, trigger, msg) = (f.name.clone(), f.trigger.clone(), self.sched_msg.clone());
                std::thread::spawn(move || {
                    let r = crate::triggers::sync_schedule(&name, &trigger);
                    *msg.lock().unwrap() = Some(match r {
                        Ok(m) => (true, m),
                        Err(e) => (false, e),
                    });
                });
            }
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
        if self.dirty.is_none() && !self.showcase {
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
        let f = Flow { name: name.clone(), description: String::new(), nodes: vec![mk(1, "input", 40.0), mk(2, "output", 560.0)], links: Vec::new(), ..Default::default() };
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
        ui.allocate_ui_with_layout(vec2(w, h), crate::widgets::fwd(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let insp = 300.0;
            let cw = (w - insp - 8.0).max(260.0);
            self.canvas(ui, p, vec2(cw, h));
            ui.allocate_ui_with_layout(vec2(insp, h), crate::widgets::down(), |ui| self.inspector(ui, p, h));
        });
        self.run_bar(ui, p);
        if self.job.state().running {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &Palette) {
        // a linha quebra quando não cabe (janela estreita, textos longos em outros idiomas)
        ui.horizontal_wrapped(|ui| {
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
            egui::ComboBox::from_id_salt("flow_pick").selected_text(crate::i18n::tr(&current)).width((ui.available_width() - 620.0).clamp(180.0, 300.0)).show_ui(ui, |ui| {
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
            // exportar: um .json para compartilhar, sem os arquivos de entrada nem o gatilho (são do seu PC)
            if widgets::secondary(ui, p, "Exportar", has) {
                self.save_now();
                if let Some(mut f) = self.flow.clone() {
                    for n in f.nodes.iter_mut().filter(|n| n.kind == "input") {
                        n.args.remove("files");
                    }
                    f.trigger = Default::default();
                    if let Some(dst) = rfd::FileDialog::new().add_filter(crate::i18n::tr("Fluxo do Conver"), &["json"]).set_file_name(format!("{}.json", f.name)).save_file() {
                        let r = serde_json::to_string_pretty(&f).map_err(|e| e.to_string()).and_then(|t| std::fs::write(&dst, t).map_err(|e| e.to_string()));
                        self.notice = Some(match r {
                            Ok(()) => (true, format!("✔ Exportado: {}", tools::file_name(&dst))),
                            Err(e) => (false, format!("✖ {e}")),
                        });
                    }
                }
            }
            if widgets::secondary(ui, p, "Importar", true) {
                if let Some(src) = rfd::FileDialog::new().add_filter(crate::i18n::tr("Fluxo do Conver"), &["json"]).pick_file() {
                    match import_flow(&src) {
                        Ok(mut f) => {
                            self.save_now();
                            self.load_list();
                            f.name = self.unique_name(f.name.trim());
                            match flow::save_draft(&f) {
                                Ok(()) => {
                                    let name = f.name.clone();
                                    self.load_list();
                                    self.open(&name);
                                    self.notice = Some((true, format!("✔ Importado: {name}")));
                                }
                                Err(e) => self.notice = Some((false, format!("✖ {e}"))),
                            }
                        }
                        Err(e) => self.notice = Some((false, format!("✖ {e}"))),
                    }
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
                        let resp = resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(crate::i18n::tr(node_help(&k.kind)));
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
            "rename" => vec![s("pattern").unwrap_or_else(|| "{name}_{n}".into())],
            "classify" => {
                let n = crate::classify::parse_options(&s("options").unwrap_or_default()).len();
                let min = a.get("min_confidence").and_then(Value::as_f64).unwrap_or(50.0);
                if n < 2 {
                    vec![crate::i18n::tr("escreva as opções")]
                } else {
                    vec![crate::i18n::tr(format!("{n} opções")), crate::i18n::tr(format!("confiança mínima {min:.0}%"))]
                }
            }
            "output" => vec![s("folder").map(|f| tools::file_name(std::path::Path::new(&f))).unwrap_or_else(|| crate::i18n::tr("Documentos\\Conver"))],
            "cloud" => vec![format!("{}: {}", enum_text(&s("service").unwrap_or_else(|| "auto".into())), s("subfolder").unwrap_or_else(|| "Conver".into()))],
            "webhook" => vec![s("url").map(|u| u.split('/').nth(2).unwrap_or(&u).to_string()).unwrap_or_else(|| crate::i18n::tr("cole o endereço"))],
            "condition" => {
                let check = s("check").unwrap_or_else(|| "orientation".into());
                let what = crate::i18n::tr(enum_label(&check));
                if check == "orientation" {
                    vec![format!("{what}: {}", enum_text(&s("orientation").unwrap_or_else(|| "vertical".into())))]
                } else if check == "has_audio" {
                    vec![what]
                } else {
                    let op = if s("compare").as_deref() == Some("less") { "<" } else { ">" };
                    let v = a.get("value").and_then(Value::as_f64).map(|v| format!("{v}")).unwrap_or_else(|| "?".into());
                    vec![format!("{what} {op} {v}")]
                }
            }
            "filter" => {
                let t: Vec<String> = a.get("types").and_then(Value::as_array).map(|x| x.iter().filter_map(|v| v.as_str()).map(|v| enum_text(v)).collect()).unwrap_or_default();
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
        // com muitas saídas (classificador), o node cresce para caber uma por linha
        let outs = flow::node_outputs(n).len() as f32;
        let h = (HEADER + 8.0 + lines * 17.0 + 26.0).max(HEADER + 14.0 + outs * 20.0);
        // e fica mais largo, para o nome de cada opção aparecer inteiro ao lado da saída
        vec2(if outs > 2.0 { NODE_W + 70.0 } else { NODE_W }, h)
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
        let outs_of: std::collections::HashMap<u32, Vec<String>> = flow.nodes.iter().map(|n| (n.id, flow::node_outputs(n))).collect();
        let outputs_of = |id: u32| -> Vec<String> { outs_of.get(&id).cloned().unwrap_or_else(|| vec!["out".into()]) };
        let out_pos = |r: Rect, id: u32, port: &str| -> Pos2 {
            let outs = outputs_of(id);
            let i = outs.iter().position(|o| o == port).unwrap_or(0) as f32;
            let y = match outs.len() {
                1 => r.top() + HEADER * z / 2.0 + r.height() / 2.0 - HEADER * z / 4.0,
                2 => r.top() + r.height() * (0.45 + 0.3 * i),
                // várias saídas: igualmente espaçadas abaixo do cabeçalho
                n => r.top() + HEADER * z + (r.height() - HEADER * z) * (i + 0.5) / n as f32,
            };
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
            let (s, e) = (out_pos(a, l.from, &l.port), in_pos(b));
            let pts = bezier(s, e, z);
            let selected = self.sel == Some(Sel::Link(i));
            let color = if selected { p.accent } else { p.muted };
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(if selected { 3.0f32 } else { 2.0 }, color)));
            link_paths.push(pts);
        }
        // ligação sendo puxada
        if let (Some((from, port)), Some(ptr)) = (&self.dragging, ui.input(|i| i.pointer.hover_pos())) {
            if let Some(a) = rect_of(*from) {
                painter.add(egui::Shape::line(bezier(out_pos(a, *from, port), ptr, z), Stroke::new(2.0f32, p.accent)));
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
                let reserve = match outputs_of(n.id).len() {
                    1 => 0.0,
                    2 => 30.0,
                    _ => 130.0,
                };
                let g = fit_text(&painter, &line, FontId::proportional(12.0 * z), p.muted, r.width() - 2.0 * pad - reserve * z);
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
            let outs = outputs_of(n.id);
            if outs.len() > 1 {
                for o in &outs {
                    let pp = out_pos(r, n.id, o);
                    let (label, color) = port_label(o, p);
                    let g = fit_text(&painter, &label, FontId::new(11.0 * z, bold()), color, 126.0 * z);
                    painter.galley(pp - vec2(12.0 * z + g.size().x, g.size().y / 2.0), g, color);
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
            for o in &outputs_of(n.id) {
                let pp = out_pos(r, n.id, o);
                let hit = Rect::from_center_size(pp, vec2(18.0, 18.0) * z.max(0.8));
                let resp = ui.interact(hit, ui.id().with(("flow_port", n.id, o.as_str())), Sense::drag()).on_hover_cursor(CursorIcon::Crosshair);
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
        // (filhos soltos: não empurram o cursor do layout, que em árabe, urdu e persa corre da direita para a esquerda)
        let add_bar = crate::widgets::mirror(Rect::from_min_size(pos2(rect.left() + 10.0, rect.top() + 10.0), vec2(220.0, 30.0)), rect);
        {
            let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(add_bar).layout(crate::widgets::fwd(Align::Center)));
            let b = egui::Button::new(RichText::new(crate::i18n::tr("+  Adicionar node")).family(bold()).color(p.text))
                .fill(p.card)
                .stroke(Stroke::new(1.0f32, p.border))
                .corner_radius(8)
                .min_size(vec2(0.0, 30.0));
            egui::menu::menu_custom_button(ui, b, |ui| self.palette(ui, p));
        }
        let bar = crate::widgets::mirror(Rect::from_min_size(pos2(rect.right() - 136.0, rect.top() + 10.0), vec2(126.0, 30.0)), rect);
        {
            let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(crate::widgets::back(Align::Center)));
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
        }
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
                        flow::UNCERTAIN => format!(" ({})", crate::i18n::tr("incerto")),
                        "out" => String::new(),
                        other => format!(" ({other})"),
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
        let ai = self.ai();
        let Some(f) = self.flow.as_mut() else { return };
        ui.label(RichText::new(crate::i18n::tr("Fluxo")).family(bold()).size(16.0).color(p.text));
        ui.label(RichText::new(crate::i18n::tr(&f.name)).color(p.muted));
        ui.add_space(8.0);
        ui.label(RichText::new(crate::i18n::tr("Descrição")).color(p.text));
        if ui.add(egui::TextEdit::multiline(&mut f.description).desired_rows(3).desired_width(f32::INFINITY)).changed() {
            self.dirty = Some(Instant::now());
        }
        // rodar sozinho: pasta vigiada, horário e aviso
        ui.add_space(10.0);
        ui.label(RichText::new(crate::i18n::tr("Rodar sozinho")).family(bold()).color(p.text));
        let mut changed = false;
        let t = &mut f.trigger;
        let mut on_folder = !t.folder.trim().is_empty();
        if crate::widgets::check(ui, &mut on_folder, crate::i18n::tr("Quando chegar arquivo novo numa pasta")).changed() {
            if on_folder {
                if let Some(d) = rfd::FileDialog::new().pick_folder() {
                    t.folder = d.display().to_string();
                }
            } else {
                t.folder.clear();
            }
            changed = true;
        }
        if !t.folder.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                let folder = std::path::Path::new(&t.folder);
                ui.label(RichText::new(crate::tools::file_name(folder)).color(p.text)).on_hover_text(&t.folder);
                if ui.small_button(crate::i18n::tr("Trocar")).clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_directory(folder).pick_folder() {
                        t.folder = d.display().to_string();
                        changed = true;
                    }
                }
            });
            if let Some(st) = crate::triggers::folder_status(&f.name) {
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.label(RichText::new(crate::i18n::tr(&st)).color(p.muted).size(12.0));
                });
            }
            widgets::hint(ui, p, "Funciona com o Conver aberto (pode ficar minimizado). O que já estava na pasta não conta.");
        }
        let mut on_time = !t.time.trim().is_empty();
        if crate::widgets::check(ui, &mut on_time, crate::i18n::tr("Num horário")).changed() {
            t.time = if on_time { "08:00".into() } else { String::new() };
            changed = true;
        }
        if on_time {
            ui.horizontal(|ui| {
                ui.add_space(22.0);
                let ok = crate::triggers::parse_time(&t.time).is_some();
                let edit = egui::TextEdit::singleline(&mut t.time).desired_width(56.0).margin(widgets::FIELD_MARGIN).text_color(if ok { p.text } else { p.accent });
                if ui.add(edit).on_hover_text(crate::i18n::tr("HH:MM, por exemplo 08:30")).changed() {
                    changed = true;
                }
                // dias da semana: nenhum marcado = todos os dias
                for (i, d) in ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"].iter().enumerate() {
                    let on = t.days.contains(&(i as u8));
                    if ui.selectable_label(on, crate::i18n::tr(d)).clicked() {
                        if on {
                            t.days.retain(|x| *x != i as u8);
                        } else {
                            t.days.push(i as u8);
                        }
                        changed = true;
                    }
                }
            });
            if let Some((good, m)) = self.sched_msg.lock().unwrap().clone() {
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    ui.label(RichText::new(crate::i18n::tr(&m)).color(if good { p.ok } else { p.accent }).size(12.0));
                });
            }
            widgets::hint(ui, p, "Usa o Agendador de Tarefas do Windows: roda mesmo com o Conver fechado. Sem dias marcados, roda todo dia.");
        }
        if crate::widgets::check(ui, &mut t.notify, crate::i18n::tr("Avisar quando terminar (notificação do Windows)")).changed() {
            changed = true;
        }
        if changed {
            self.dirty = Some(Instant::now());
        }
        self.history_ui(ui, p);
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
                    if let Err(e) = self.ai() {
                        ui.label(RichText::new(crate::i18n::tr(format!("⚠ IA: {e}"))).size(12.5).color(p.accent));
                        if widgets::secondary(ui, p, "Configurar a IA", true) {
                            self.go_settings = true;
                        }
                        ui.add_space(4.0);
                    }
                }
                if let Some(props) = info.schema["properties"].as_object() {
                    // na ordem do esquema, mostrando só os ajustes que valem para o que já foi escolhido
                    let order = |k: &str| ["check", "orientation", "compare", "value"].iter().position(|x| *x == k).unwrap_or(99);
                    let mut keys: Vec<&String> = props.keys().collect();
                    if n.kind == "condition" {
                        keys.sort_by_key(|k| order(k));
                    }
                    for key in keys {
                        if arg_hidden(&n.kind, key, &n.args) {
                            continue;
                        }
                        arg_editor(ui, p, key, &props[key], &mut n.args);
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
        if let Some(Status::Failed(e)) = &status {
            ui.add_space(6.0);
            ui.label(RichText::new(crate::i18n::tr(format!("✖ {e}"))).size(12.5).color(p.accent));
        }
        if !self.job.state().running && !matches!(status, Some(Status::Failed(_))) {
            self.node_results(ui, p, id);
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
            Act::Start => self.start(ui.ctx(), flow::Inputs::default()),
            Act::Cancel => self.job.cancel(),
            Act::None => {}
        }
    }

    /// Executa o fluxo aberto (com `inputs` no lugar das entradas salvas, se houver).
    fn start(&mut self, ctx: &egui::Context, inputs: flow::Inputs) {
        self.save_now();
        let Some(f) = self.flow.clone() else { return };
        if let Err(e) = f.validate() {
            self.notice = Some((false, format!("✖ {e}")));
            return;
        }
        self.notice = None;
        self.history = None;
        let run = self.run.clone();
        // na hora de executar, lê de novo (a chave pode ter acabado de ser salva)
        self.ai_cache = None;
        let ai = self.ai();
        self.job.start(ctx, move |ctl| {
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
            match flow::run(&f, &inputs, None, &run, &ctx, &ai) {
                Ok(report) => {
                    let failed = report["failed_nodes"].as_u64().unwrap_or(0);
                    let n = report["results"].as_array().map(Vec::len).unwrap_or(0);
                    ctl.status(if failed > 0 { format!("Concluído com {failed} node(s) com erro.") } else { format!("Concluído: {n} resultado(s).") });
                    // "Mostrar arquivo": o primeiro resultado final; sem arquivo, a pasta de trabalho
                    let folder = report["results"]
                        .as_array()
                        .and_then(|r| r.iter().find_map(|i| i["file"].as_str()))
                        .map(PathBuf::from)
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

    /// Execuções do fluxo aberto, da mais nova para a mais velha (relidas do disco a cada 3 s).
    fn runs(&mut self) -> Vec<Value> {
        let Some(name) = self.flow.as_ref().map(|f| f.name.clone()) else { return Vec::new() };
        let stale = self.history.as_ref().is_none_or(|(n, t, _)| *n != name || t.elapsed() > Duration::from_secs(3));
        if stale {
            self.history = Some((name.clone(), Instant::now(), flow::history(&name)));
        }
        self.history.as_ref().map(|(_, _, r)| r.clone()).unwrap_or_default()
    }

    /// O que o node produziu na última execução: miniaturas e nomes dos arquivos (um clique abre), textos e a pasta.
    fn node_results(&mut self, ui: &mut Ui, p: &Palette, id: u32) {
        let runs = self.runs();
        let Some(last) = runs.first() else { return };
        ui.add_space(12.0);
        ui.label(RichText::new(crate::i18n::tr("Resultado da última execução")).family(bold()).color(p.text));
        if let Some(when) = last["started"].as_str() {
            ui.label(RichText::new(when).size(12.0).color(p.muted));
        }
        let Some(rep) = last["nodes"].as_array().and_then(|a| a.iter().find(|r| r["id"].as_u64() == Some(id as u64))).cloned() else {
            widgets::hint(ui, p, "Este node não rodou na última execução.");
            return;
        };
        match rep["status"].as_str() {
            Some("failed") => {
                ui.label(RichText::new(crate::i18n::tr(format!("✖ {}", rep["error"].as_str().unwrap_or_default()))).size(12.5).color(p.accent));
                return;
            }
            Some("skipped") => {
                widgets::hint(ui, p, "Nada chegou neste node.");
                return;
            }
            _ => {}
        }
        let thumbs = self.thumbs.get_or_insert_with(|| widgets::Thumbs::new(ui.ctx()));
        let outputs = rep["outputs"].as_object().cloned().unwrap_or_default();
        let several = outputs.len() > 1;
        let mut first_file: Option<std::path::PathBuf> = None;
        let mut any = false;
        for (port, items) in &outputs {
            let items = items.as_array().cloned().unwrap_or_default();
            if several {
                let name = match port.as_str() {
                    "yes" => crate::i18n::tr("sim"),
                    "no" => crate::i18n::tr("não"),
                    flow::UNCERTAIN => crate::i18n::tr("incerto"),
                    "out" => String::new(),
                    other => other.to_string(),
                };
                ui.label(RichText::new(crate::i18n::tr(format!("{name} ({})", items.len()))).size(12.0).color(p.muted));
            }
            for it in items.iter().take(12) {
                any = true;
                if let Some(f) = it["file"].as_str() {
                    let path = std::path::PathBuf::from(f);
                    first_file.get_or_insert_with(|| path.clone());
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 6, p.input);
                    }
                    let slot = widgets::mirror(Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(32.0, 32.0)), rect);
                    let is_image = crate::imaging::IMAGE_EXTS.contains(&tools::ext_lower(&path).as_str());
                    match is_image.then(|| thumbs.get(&path)) {
                        Some(widgets::Thumb::Ready { tex, .. }) => {
                            let s = tex.size_vec2();
                            let r = Rect::from_center_size(slot.center(), widgets::fit((s.x, s.y), slot.size()));
                            ui.painter().image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        _ => {
                            ui.painter().rect_filled(slot, 5, p.action);
                            let ext: String = tools::ext_lower(&path).to_uppercase().chars().take(4).collect();
                            ui.painter().text(slot.center(), Align2::CENTER_CENTER, ext, FontId::new(9.5, bold()), p.on_accent);
                        }
                    }
                    let text_rect = widgets::mirror(Rect::from_min_max(pos2(slot.right() + 8.0, rect.top()), rect.max), rect);
                    let (x, anchor) = if widgets::rtl() { (text_rect.right(), Align2::RIGHT_CENTER) } else { (text_rect.left(), Align2::LEFT_CENTER) };
                    let painter = ui.painter().with_clip_rect(text_rect);
                    painter.text(pos2(x, rect.center().y - 7.0), anchor, tools::file_name(&path), FontId::proportional(13.0), p.text);
                    painter.text(pos2(x, rect.center().y + 8.0), anchor, tools::human_size(tools::file_size(&path)), FontId::proportional(11.5), p.muted);
                    if resp.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(crate::i18n::tr("Abrir")).clicked() {
                        tools::open_file(&path);
                    }
                } else if let Some(t) = it["text"].as_str() {
                    let short: String = t.chars().take(220).collect();
                    let more = if t.chars().count() > 220 { "…" } else { "" };
                    Frame::default().fill(p.input).corner_radius(6).inner_margin(Margin::same(6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(format!("{short}{more}")).size(12.5).color(p.text));
                        if ui.small_button(crate::i18n::tr("Copiar")).clicked() {
                            ui.ctx().copy_text(t.to_string());
                        }
                    });
                }
            }
            if items.len() > 12 {
                ui.label(RichText::new(crate::i18n::tr(format!("e mais {}", items.len() - 12))).size(12.0).color(p.muted));
            }
        }
        if !any {
            widgets::hint(ui, p, "Nada saiu deste node.");
        }
        if let Some(f) = first_file.filter(|f| f.is_file()) {
            ui.add_space(4.0);
            if widgets::secondary(ui, p, "Mostrar arquivo", true) {
                tools::show_in_folder(&f);
            }
        }
    }

    /// Histórico no painel do fluxo: as últimas execuções, com "Repetir falhas" e a pasta de cada uma.
    fn history_ui(&mut self, ui: &mut Ui, p: &Palette) {
        if self.flow.is_none() {
            return;
        }
        let runs = self.runs();
        ui.add_space(12.0);
        ui.label(RichText::new(crate::i18n::tr("Últimas execuções")).family(bold()).color(p.text));
        if runs.is_empty() {
            widgets::hint(ui, p, "O fluxo ainda não foi executado.");
            return;
        }
        let running = self.job.state().running;
        let mut retry: Option<Vec<PathBuf>> = None;
        for (i, r) in runs.iter().take(8).enumerate() {
            let outs = r["results"].as_array().map(Vec::len).unwrap_or(0);
            let failed: Vec<PathBuf> = r["failed_inputs"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(PathBuf::from)).collect()).unwrap_or_default();
            let bad_nodes = r["failed_nodes"].as_u64().unwrap_or(0);
            let ok = failed.is_empty() && bad_nodes == 0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(if ok { "✔" } else { "✖" }).color(if ok { p.ok } else { p.accent }));
                let when = r["started"].as_str().unwrap_or_default();
                let secs = r["seconds"].as_f64().unwrap_or(0.0);
                ui.label(RichText::new(format!("{when}  •  {secs:.0} s")).color(p.text).size(12.5));
            });
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                let detail = if ok {
                    crate::i18n::tr(format!("{outs} resultado(s)"))
                } else if failed.is_empty() {
                    crate::i18n::tr(format!("{outs} resultado(s), {bad_nodes} node(s) com erro"))
                } else {
                    crate::i18n::tr(format!("{outs} resultado(s), {} arquivo(s) com erro", failed.len()))
                };
                ui.label(RichText::new(detail).color(p.muted).size(12.0));
            });
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                if let Some(d) = r["work_dir"].as_str() {
                    if ui.small_button(crate::i18n::tr("Pasta")).clicked() {
                        crate::tools::open_file(std::path::Path::new(d));
                    }
                }
                let alive: Vec<PathBuf> = failed.iter().filter(|f| f.exists()).cloned().collect();
                if !alive.is_empty() && ui.add_enabled(!running, egui::Button::new(crate::i18n::tr("Repetir falhas")).small()).clicked() {
                    retry = Some(alive);
                }
            });
            if i + 1 < runs.len().min(8) {
                ui.add_space(4.0);
            }
        }
        if let Some(files) = retry {
            self.start(ui.ctx(), flow::Inputs { files, texts: Vec::new() });
        }
    }
}

// ================================================================ ajudantes

/// Nome e cor de uma saída: sim/não, "incerto" ou o nome da opção do classificador.
fn port_label(o: &str, p: &Palette) -> (String, Color32) {
    match o {
        "yes" => (crate::i18n::tr("sim"), p.ok),
        "no" => (crate::i18n::tr("não"), p.accent),
        flow::UNCERTAIN => (crate::i18n::tr("incerto"), p.muted),
        other => (other.to_string(), p.text),
    }
}

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
        Value::String(s) => enum_text(s),
        Value::Bool(b) => crate::i18n::tr(if *b { "sim" } else { "não" }),
        Value::Array(a) => a.iter().map(value_text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// Editor de um argumento a partir do esquema (lista, sim/não, número, texto, lista de textos, JSON).
fn arg_editor(ui: &mut Ui, p: &Palette, key: &str, prop: &Value, args: &mut Map<String, Value>) {
    let label = crate::i18n::tr(arg_label(key));
    let ty = prop["type"].as_str().unwrap_or("string");
    let current = args.get(key).cloned().filter(|v| !v.is_null());
    let default = prop.get("default").cloned();
    ui.add_space(4.0);
    if ty == "boolean" {
        let mut b = current.as_ref().and_then(Value::as_bool).or(default.as_ref().and_then(Value::as_bool)).unwrap_or(false);
        if crate::widgets::check(ui, &mut b, label).changed() {
            args.insert(key.into(), json!(b));
        }
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(&label).color(p.text));
        if current.is_some() && ui.small_button("↺").on_hover_text(crate::i18n::tr("Voltar ao padrão")).clicked() {
            args.remove(key);
        }
    });
    if let Some(values) = prop["enum"].as_array() {
        let cur = current.as_ref().and_then(Value::as_str).or(default.as_ref().and_then(Value::as_str)).unwrap_or_default().to_string();
        let list: Vec<&str> = values.iter().filter_map(Value::as_str).collect();
        if list.len() <= 5 {
            // como nas telas (vira menu suspenso sozinho se não couber)
            let texts: Vec<String> = list.iter().map(|v| enum_text(v)).collect();
            let mut idx = list.iter().position(|v| *v == cur).unwrap_or(usize::MAX);
            let opts: Vec<(usize, &str)> = texts.iter().enumerate().map(|(i, t)| (i, t.as_str())).collect();
            if widgets::segmented(ui, p, &mut idx, &opts) {
                args.insert(key.into(), json!(list[idx]));
            }
            return;
        }
        egui::ComboBox::from_id_salt(("arg", key)).selected_text(enum_text(&cur)).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
            for v in values.iter().filter_map(Value::as_str) {
                if ui.selectable_label(v == cur, enum_text(v)).clicked() {
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
                let slider = min.is_finite() && max.is_finite() && min > f64::MIN && max < f64::MAX && max - min <= 1000.0;
                let r = if slider {
                    ui.spacing_mut().slider_width = (ui.available_width() - 70.0).clamp(80.0, 200.0);
                    {
                        let sl = egui::Slider::new(&mut v, min..=max).max_decimals(if ty == "integer" { 0 } else { 2 });
                        ui.add(if ty == "integer" { sl.integer() } else { sl })
                    }
                } else {
                    ui.add(egui::DragValue::new(&mut v).range(min..=max).speed(speed).max_decimals(if ty == "integer" { 0 } else { 2 }))
                };
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
                        if crate::widgets::check(ui, &mut on, enum_text(v)).changed() {
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
        _ if special_editor(ui, p, key, current.as_ref().and_then(Value::as_str), args) => {}
        _ => {
            let mut s = current.as_ref().map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
            let long = matches!(key, "prompt" | "criteria" | "text" | "options");
            let edit = if long {
                egui::TextEdit::multiline(&mut s).desired_rows(4)
            } else {
                egui::TextEdit::singleline(&mut s).margin(widgets::FIELD_MARGIN)
            };
            if ui.add(edit.desired_width(f32::INFINITY)).changed() {
                if s.is_empty() {
                    args.remove(key);
                } else {
                    args.insert(key.into(), json!(s));
                }
            }
        }
    }
}

// i18n: fora
/// Ajustes que não valem para o que já foi escolhido no node (ficam escondidos no painel).
fn arg_hidden(kind: &str, key: &str, args: &Map<String, Value>) -> bool {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match kind {
        "condition" => {
            let check = args.get("check").and_then(Value::as_str).unwrap_or("orientation");
            match key {
                "orientation" => check != "orientation",
                "compare" | "value" => matches!(check, "orientation" | "has_audio"),
                _ => false,
            }
        }
        "tool:files_rename" => matches!(key, "start" | "digits" | "separator") && !args.get("numbering").and_then(Value::as_bool).unwrap_or(false),
        "tool:subtitle_translate" => matches!(key, "voice" | "original_volume" | "video") && !args.get("dub").and_then(Value::as_bool).unwrap_or(false),
        "tool:document_scan" => key == "name" && s("format") != "pdf",
        _ => false,
    }
}

// i18n: dentro

/// Que tipo de arquivo um argumento de texto recebe (para o botão "Escolher…"): nome do filtro e extensões.
fn file_kind(key: &str) -> Option<(&'static str, &'static [&'static str])> {
    match key {
        "logo" | "mask" => Some(("Imagem", crate::imaging::IMAGE_EXTS)),
        "music" => Some(("Áudio", crate::media::AUDIO_EXTS)),
        "video" => Some(("Vídeo", crate::video::VIDEO_EXTS)),
        "subtitle" | "subtitle_file" => Some(("Legenda", &["srt", "ass", "vtt"])),
        "cuts_file" => Some(("Arquivo de cortes", &["json"])),
        _ => None,
    }
}

fn hex_to_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    (h.len() == 6).then(|| u32::from_str_radix(h, 16).ok()).flatten().map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn color_to_hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Controles especiais, como nas telas: cor, arquivo, pasta e microfone. Devolve `false` para o campo de texto comum.
fn special_editor(ui: &mut Ui, p: &Palette, key: &str, cur: Option<&str>, args: &mut Map<String, Value>) -> bool {
    let is_color = matches!(key, "color" | "text_color" | "background_color" | "highlight_color" | "outline_color");
    if key == "background" {
        // Remover fundo: transparente, branco, preto ou uma cor (como na tela)
        let v = cur.unwrap_or("transparent").to_lowercase();
        let mut kind = match v.as_str() {
            "transparent" => 0usize,
            "#ffffff" | "white" => 1,
            "#000000" | "black" => 2,
            _ => 3,
        };
        let before = kind;
        ui.horizontal(|ui| {
            widgets::segmented(ui, p, &mut kind, &[(0, "Transparente"), (1, "Branco"), (2, "Preto"), (3, "Cor")]);
            if kind == 3 {
                let mut c = hex_to_color(&v).unwrap_or([0x22, 0x88, 0xee]);
                if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() || before != 3 {
                    args.insert(key.into(), json!(color_to_hex(c)));
                }
            }
        });
        if kind != before && kind != 3 {
            args.insert(key.into(), json!(["transparent", "#ffffff", "#000000"][kind]));
        }
        return true;
    }
    if is_color {
        ui.horizontal(|ui| {
            let mut c = cur.and_then(hex_to_color).unwrap_or([0, 0, 0]);
            if egui::color_picker::color_edit_button_srgb(ui, &mut c).changed() {
                args.insert(key.into(), json!(color_to_hex(c)));
            }
            ui.label(RichText::new(cur.map(|c| c.to_uppercase()).unwrap_or_else(|| crate::i18n::tr("(padrão)"))).size(12.0).color(p.muted));
        });
        return true;
    }
    if key == "microphone" {
        let list = crate::recorder::cached_devices(ui.ctx()).unwrap_or_default();
        let shown = cur.map(String::from).unwrap_or_else(|| crate::i18n::tr(list.first().map(|_| "O primeiro da lista").unwrap_or("Sem microfone")));
        egui::ComboBox::from_id_salt(("arg", key)).selected_text(shown).width(ui.available_width() - 8.0).show_ui(ui, |ui| {
            if ui.selectable_label(cur.is_none(), crate::i18n::tr("O primeiro da lista")).clicked() {
                args.remove(key);
            }
            for d in &list {
                if ui.selectable_label(cur == Some(d.as_str()), d).clicked() {
                    args.insert(key.into(), json!(d));
                }
            }
        });
        return true;
    }
    let kind = file_kind(key);
    let folder = key == "folder";
    if kind.is_none() && !folder {
        return false;
    }
    // o botão primeiro, no fim da linha; o campo ocupa o que sobra (o botão é mais largo em alguns idiomas)
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 30.0), widgets::back(egui::Align::Center), |ui| {
        let mut s = cur.unwrap_or_default().to_string();
        let pick = widgets::secondary(ui, p, "Escolher…", true);
        if ui.add(egui::TextEdit::singleline(&mut s).margin(widgets::FIELD_MARGIN).desired_width(ui.available_width())).changed() {
            if s.trim().is_empty() {
                args.remove(key);
            } else {
                args.insert(key.into(), json!(s));
            }
        }
        if pick {
            let picked = if folder {
                rfd::FileDialog::new().pick_folder()
            } else {
                let (name, exts) = kind.unwrap();
                rfd::FileDialog::new().add_filter(crate::i18n::tr(name), exts).pick_file()
            };
            if let Some(f) = picked {
                args.insert(key.into(), json!(f.display().to_string()));
            }
        }
    });
    true
}

/// Lê um fluxo exportado: precisa ter nome e nodes de tipos que este Conver conhece. O gatilho não vem junto.
pub(crate) fn import_flow(src: &std::path::Path) -> Result<Flow, String> {
    let text = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
    let mut f: Flow = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| crate::i18n::tr("o arquivo não é um fluxo do Conver."))?;
    if f.name.trim().is_empty() {
        f.name = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Fluxo importado".into());
    }
    let kinds = flow::kinds();
    if let Some(n) = f.nodes.iter().find(|n| !kinds.iter().any(|k| k.kind == n.kind)) {
        return Err(crate::i18n::tr(format!("o fluxo usa um node que este Conver não tem: {} (atualize o Conver).", n.kind)));
    }
    f.trigger = Default::default();
    Ok(f)
}

/// Nomes em português dos argumentos das ferramentas (no esquema eles estão em inglês, para os agentes).
pub(crate) fn arg_label(key: &str) -> &str {
    match key {
        "animated" => "Animada (palavra por palavra)",
        "background_color" => "Cor da caixa",
        "background_opacity" => "Opacidade da caixa (%)",
        "bold" => "Negrito",
        "count" => "Quantas",
        "denoise" => "Tirar o ruído também",
        "dub" => "Dublar",
        "effect" => "Efeito",
        "files" => "Arquivos",
        "folder" => "Pasta",
        "font" => "Fonte",
        "font_size" => "Tamanho do texto",
        "highlight_color" => "Cor do destaque",
        "italic" => "Itálico",
        "keep_ass" => "Guardar o .ass",
        "logo" => "Logo (imagem)",
        "loudness" => "Volume final",
        "margin" => "Margem (%)",
        "mask" => "Máscara",
        "monitor" => "Monitor",
        "negative" => "Evitar",
        "only_tables" => "Só as tabelas",
        "opacity" => "Opacidade (%)",
        "order" => "Ordem das páginas",
        "original_volume" => "Som original (%)",
        "outline" => "Contorno",
        "outline_color" => "Cor do contorno",
        "pop" => "Pulinho",
        "preset" => "Estilo pronto",
        "regions" => "Áreas",
        "seed" => "Semente",
        "sensitivity" => "Sensibilidade",
        "shadow" => "Sombra",
        "shape" => "Formato",
        "sort_into_folders" => "Separar em pastas",
        "speed" => "Velocidade",
        "strength" => "Força",
        "style" => "Esconder com",
        "subtitle_file" => "Arquivo de legenda",
        "text_color" => "Cor do texto",
        "to" => "Traduzir para",
        "translate" => "Traduzir o pedido para o inglês",
        "uppercase" => "Em maiúsculas",
        "video" => "Vídeo",
        "voice" => "Voz",
        "words_per_line" => "Palavras por linha",
        "x" => "Posição horizontal (%)",
        "y" => "Posição vertical (%)",
        "options" => "Opções (uma por linha)",
        "check" => "O que conferir",
        "precision" => "Precisão (%)",
        "grow" => "Ajustar a borda (px)",
        "feather" => "Suavizar a borda (px)",
        "antialias" => "Anticerrilhado",
        "detect" => "Achar o objeto",
        "interior" => "Interior do objeto",
        "how" => "Como editar",
        "orientation" => "Orientação",
        "mode" => "Limpeza",
        "crop" => "Achar a folha e endireitar",
        "compare" => "Comparação",
        "value" => "Valor",
        "base" => "Nome base",
        "find" => "Procurar",
        "replace" => "Substituir por",
        "prefix" => "Antes do nome",
        "suffix" => "Depois do nome",
        "numbering" => "Numerar",
        "separator" => "Separador",
        "case" => "Letras",
        "error_correction" => "Correção de erro",
        "pattern" => "Padrão do nome",
        "digits" => "Dígitos do número",
        "count_per_option" => "Contar separado por opção",
        "min_confidence" => "Confiança mínima (%)",
        "look_at" => "O que olhar",
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
        "cuts_file" => "Arquivo de cortes (JSON ou auto)",
        "service" => "Serviço",
        "subfolder" => "Pasta na nuvem",
        "url" => "Endereço do webhook",
        "send" => "Enviar",
        "message" => "Mensagem",
        "threshold_db" => "Limite de volume (dB)",
        "min_pause" => "Pausa mínima (s)",
        "padding" => "Folga (s)",
        "follow" => "Acompanhar",
        "min_seconds" => "Duração mínima (s)",
        "max_seconds" => "Duração máxima (s)",
        "ai" => "IA que escolhe",
        "cut" => "Já cortar",
        "upscale" => "Ampliar a foto 2×",
        "cut_mode" => "Modo do corte",
        "keep" => "Trechos para manter",
        "remove" => "Trechos para tirar",
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
        "blur" => "Desfoque",
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
/// Texto de um valor de lista na tela: idiomas e vozes pelo nome, formatos em maiúsculas, o resto pela tabela.
pub(crate) fn enum_text(v: &str) -> String {
    if let Some((_, _, name)) = crate::translate::LANGS.iter().find(|(code, _, _)| *code == v) {
        return crate::i18n::tr(*name);
    }
    if let Some(voice) = crate::tts::any_voice(v) {
        return crate::i18n::tr(voice.label());
    }
    let label = enum_label(v);
    if label == v && v.len() <= 5 && v.starts_with(|c: char| c.is_ascii_lowercase()) && v.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
        return v.to_uppercase();
    }
    crate::i18n::tr(label)
}

pub(crate) fn enum_label(v: &str) -> &str {
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
        "ai" => "IA (fotos)",
        "remove" => "Tirar",
        "precise" => "Preciso",
        "smart" => "Inteligente",
        "together" => "Juntar em um arquivo",
        "normal" => "Normal",
        "onedrive" => "OneDrive",
        "google_drive" => "Google Drive",
        "dropbox" => "Dropbox",
        "json" => "Dados (JSON)",
        "file" => "Os arquivos",
        "smooth" => "Suave",
        "local" => "Local (sem internet)",
        "api" => "A das Configurações",
        "ico" => "Ícone .ico",
        "web" => "Pacote para sites",
        "fit" => "Caber inteira",
        "crop" => "Cortar o centro",
        "subtitles" => "Só a legenda",
        "thumbnail" => "Só a capa",
        "separate" => "Um arquivo para cada trecho",
        "soft" => "Suave",
        "hard" => "Definida",
        "image" => "Imagens",
        "video" => "Vídeos",
        "audio" => "Áudios",
        "pdf" => "PDFs",
        "text" => "Textos",
        "sound" => "Som",
        "auto" => "Automático",
        "a4_portrait" => "A4 em pé",
        "a4_landscape" => "A4 deitado",
        "better" => "Melhor (mais lenta)",
        "blur" => "Desfoque",
        "pixelate" => "Pixelado",
        "bar" => "Tarja preta",
        "top_left" => "Em cima, à esquerda",
        "top_right" => "Em cima, à direita",
        "bottom_left" => "Embaixo, à esquerda",
        "bottom_right" => "Embaixo, à direita",
        "center" => "No centro",
        "diagonal" => "Diagonal, no meio",
        "left" => "À esquerda",
        "right" => "À direita",
        "middle" => "No meio",
        "square" => "Quadrada",
        "portrait" => "Retrato",
        "landscape" => "Paisagem",
        "podcast" => "Podcast (−16 LUFS)",
        "streaming" => "Reels e YouTube (−14 LUFS)",
        "reels" => "Reels",
        "box" => "Caixa colorida",
        "word" => "Uma palavra por vez",
        "reveal" => "Aparecendo",
        "classic" => "Clássica",
        "band" => "Faixa escura",
        "color" => "Cor",
        "orientation" => "Orientação",
        "duration" => "Duração (s)",
        "width" => "Largura (px)",
        "height" => "Altura (px)",
        "size_mb" => "Tamanho (MB)",
        "pages" => "Páginas do PDF",
        "has_audio" => "Tem som",
        "vertical" => "Vertical",
        "horizontal" => "Horizontal",
        "more" => "Maior que",
        "less" => "Menor que",
        "keep" => "Manter",
        "lower" => "minúsculas",
        "upper" => "MAIÚSCULAS",
        "document" => "Documento (preto e branco)",
        "gray" => "Tons de cinza",
        "sdxl" => "SDXL Lightning",
        "z_image" => "Z-Image Turbo (melhor)",
        "instruction" => "Pedir uma mudança",
        "paint" => "Pintar e pedir",
        "vary" => "Variar a imagem",
        other => other,
    }
}

/// Explicação de cada tipo de node para a tela (as descrições de `flow::kinds` são em inglês, para agentes).
/// As das ferramentas são os subtítulos das telas, que já têm tradução.
pub(crate) fn node_help(kind: &str) -> &'static str {
    match kind.strip_prefix("tool:").unwrap_or(kind) {
        "input" => "Começo do fluxo: os arquivos que vão passar pelos nodes.",
        "text" => "Um texto escrito por você, que segue para os próximos nodes.",
        "output" => "Salva numa pasta tudo o que chega (arquivos e textos).",
        "cloud" => "Copia os arquivos para a pasta do OneDrive, Google Drive ou Dropbox no PC; o aplicativo do serviço sobe para a nuvem.",
        "webhook" => "Avisa outro serviço (n8n, Zapier, Make, Discord) pela URL do webhook, com os dados ou os próprios arquivos.",
        "ai_prompt" => "Pede à IA configurada em Configurações para fazer algo com o que chega: descrever, resumir, traduzir, escrever legendas… A resposta segue como texto.",
        "ai_gate" => "A IA configurada confere cada item pelo critério que você escrever e manda para a saída \"sim\" ou \"não\".",
        "filter" => "Deixa passar só os arquivos do tipo, extensão ou tamanho escolhidos.",
        "document_scan" => "Foto de papel tirada com o celular vira documento reto e limpo: o Conver acha a folha, endireita e tira as sombras. Para PDF ou OCR.",
        "image_edit" => "Mude uma imagem com IA no seu PC: escreva o que mudar, pinte uma área e diga o que colocar, ou crie uma versão parecida.",
        "condition" => "Separa os arquivos por uma propriedade: orientação, duração, largura, altura, tamanho, páginas do PDF ou se tem som.",
        "files_rename" => "Copia os arquivos com nomes novos: nome base, procurar e substituir, antes e depois do nome, letras e numeração.",
        "audio_record" => "Grave o microfone (ou outra entrada de áudio do PC) direto em MP3, WAV ou M4A.",
        "rename" => "Copia cada arquivo com um nome novo, montado com a opção escolhida pela IA, a data, um contador ou o nome original.",
        "classify" => "Uma IA no próprio PC olha cada imagem, ouve cada som ou lê cada texto e manda para a saída da opção que combina mais.",
        "image_convert" => "Converta entre PNG, JPG, WEBP, BMP, GIF, TIFF e ICO.",
        "image_resize" => "Altere as dimensões de uma ou várias imagens, por porcentagem ou em pixels.",
        "image_compress" => "Reduza o tamanho do arquivo mantendo o formato (JPG, PNG, WEBP…).",
        "image_quality" => "Escolha de 1 a 100. JPG e WEBP mantêm o formato; os demais viram JPG (ou WEBP, se tiverem transparência).",
        "image_transform" => "Gire, espelhe ou recorte as imagens numa proporção pronta (o recorte é centralizado).",
        "image_adjust" => "Mude brilho, contraste e saturação ou aplique um filtro pronto. Confira na prévia antes de salvar.",
        "image_strip_metadata" => "Apaga informações escondidas da foto (EXIF, localização GPS, câmera, data) sem perder qualidade.",
        "image_remove_background" => "Tire o fundo de fotos de pessoas, produtos e animais com inteligência artificial, no seu PC e sem internet.",
        "image_upscale" => "Aumente imagens 2× ou 4× sem borrar: a inteligência artificial recria os detalhes. Roda no seu PC, sem internet.",
        "images_to_pdf" => "Junte várias imagens num único PDF, uma por página, na ordem da lista.",
        "image_ocr" => "Leia o texto de prints, fotos de documentos e placas usando o reconhecimento do próprio Windows, sem internet.",
        "qr_code" => "Crie um QR Code de um link, texto, Wi-Fi ou contato e salve como imagem PNG.",
        "video_convert" => "Converta entre MP4, MKV, AVI, MOV, WEBM, FLV, WMV, GIF ou extraia o áudio em MP3.",
        "video_compress" => "Deixe vídeos menores escolhendo um nível de compressão ou o tamanho final em MB.",
        "media_cut_silence" => "Tira as pausas de vídeos e áudios, deixando a fala corrida.",
        "video_auto_vertical" => "Deixa o vídeo vertical (9:16) seguindo o rosto de quem fala.",
        "video_highlights" => "A IA escolhe os melhores trechos de vídeos longos e salva os cortes sugeridos.",
        "image_restore" => "Recupera rostos borrados de fotos antigas com IA.",
        "image_colorize" => "Dá cor a fotos em preto e branco com IA.",
        "image_icons" => "Gera um ícone .ico ou o pacote de ícones para sites a partir de uma imagem.",
        "media_trim" => "Escolha o início e o fim, ou vários trechos para manter e para tirar (0:10-1:00; 2:00-3:30).",
        "video_merge" => "Una vários vídeos num só, na ordem da lista.",
        "video_speed" => "Acelere ou deixe em câmera lenta (0,25× a 4×). O áudio acompanha sem ficar com voz fina ou grossa.",
        "video_remove_audio" | "video_extract_frames" => "Deixe o vídeo mudo ou salve imagens do vídeo a cada intervalo de tempo.",
        "video_to_gif" => "Transforme só um trecho do vídeo num GIF animado.",
        "video_transform" => "Gire vídeos gravados de lado ou recorte para 9:16 (Reels/TikTok), 1:1, 4:5 ou 16:9.",
        "video_burn_subtitles" => "Grave uma legenda (.srt, .ass ou .vtt) dentro do vídeo, para aparecer em qualquer player ou rede social.",
        "video_adjust" => "Deixe vídeos em preto e branco, desfocados ou com outras cores, ajustando brilho, contraste e saturação.",
        "video_add_music" => "Coloque uma música no vídeo, misturada com o som original ou no lugar dele.",
        "video_captions" => "Legende vídeos com o Whisper, no seu PC e sem internet: grave direto ou revise o texto antes, com legendas paradas ou animadas palavra por palavra.",
        "audio_convert" => "Converta entre MP3, WAV, FLAC, OGG, M4A e OPUS. Também tira o áudio de vídeos.",
        "audio_merge" => "Una várias músicas ou gravações num arquivo só, na ordem da lista.",
        "audio_normalize" => "Deixe o volume de vários áudios (ou vídeos) no mesmo nível, sem estourar.",
        "audio_denoise" => "Tire chiado, ventilador, barulho de rua e de fundo de gravações de voz, em áudios e vídeos.",
        "audio_enhance_voice" => "Deixe a voz com som de podcast num clique: menos ruído, mais clara e presente, sem picos e no volume certo.",
        "audio_compress" => "Reduza o tamanho de arquivos de áudio escolhendo a taxa de bits.",
        "audio_separate" => "Tire a voz de uma música (karaokê) ou isole a voz com inteligência artificial, no seu PC.",
        "transcribe" => "Transforme a fala de áudios e vídeos em texto e legendas (SRT/VTT) com o Whisper, no seu PC e sem internet.",
        "text_to_speech" => "Transforme textos e legendas em narração com uma voz natural, no seu PC e sem internet.",
        "subtitle_translate" => "Traduza legendas para outra língua e, se quiser, duble o vídeo com uma voz natural, tudo no seu PC e sem internet.",
        "watermark" => "Coloque seu logo ou um texto em várias imagens, vídeos e PDFs de uma vez, com posição e transparência.",
        "image_hide_faces" => "Ache e esconda os rostos de fotos e vídeos para preservar a privacidade, no seu PC e sem internet.",
        "image_generate" => "Descreva uma imagem e a IA cria no seu PC, usando a placa de vídeo. Sem internet e sem limite.",
        "image_erase" => "Pinte por cima de um objeto, pessoa, texto ou marca d'água e a IA preenche o fundo. No seu PC, sem internet.",
        "pdf_merge" => "Una vários PDFs num só, na ordem da lista.",
        "pdf_split" => "Extraia páginas específicas (ex.: 1-3, 7) ou separe cada página num PDF próprio.",
        "pdf_compress" => "Deixe PDFs menores recomprimindo as imagens de dentro deles (fotos, páginas escaneadas).",
        "pdf_to_images" => "Salve as páginas do PDF como imagens PNG ou JPG.",
        "pdf_protect" => "Coloque uma senha para abrir o PDF e escolha se ele pode ser impresso ou copiado.",
        "pdf_unlock" => "Remova a senha de um PDF seu (é preciso saber a senha atual).",
        "pdf_ocr" => "Reconhece o texto de PDFs escaneados com o OCR do Windows: depois dá para pesquisar, selecionar e copiar.",
        "pdf_organize" => "Mude a ordem, gire e apague páginas de um PDF vendo as miniaturas.",
        "pdf_tables" => "Tire as tabelas de PDFs (inclusive escaneados) para uma planilha do Excel ou um CSV, com os números prontos para somar.",
        "download" => "Baixe vídeos e músicas de centenas de sites pelo link.",
        "file_info" => "Veja os detalhes de imagens, vídeos, áudios e PDFs: resolução, duração, codecs, câmera, localização da foto e mais.",
        "screen_record" => "Grave a tela inteira, um monitor, uma janela ou uma área em MP4, com ou sem o microfone.",
        "preset_run" => "Usa uma predefinição salva: a ferramenta com os ajustes que você guardou.",
        _ => "",
    }
}
