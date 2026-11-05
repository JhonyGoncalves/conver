//! Gatilhos dos fluxos: rodar sozinho quando chegam arquivos numa pasta (com o Conver aberto) ou num horário
//! (pelo Agendador de Tarefas do Windows, que chama o Conver mesmo com ele fechado).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::flow::{self, Inputs, RunState};
use crate::{ops, tools};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Trigger {
    /// Pasta vigiada (vazia = sem gatilho de pasta).
    pub folder: String,
    /// Horário "HH:MM" (vazio = sem agendamento).
    pub time: String,
    /// Dias da semana (0 = domingo … 6 = sábado); vazio = todos os dias.
    pub days: Vec<u8>,
    /// Notificação do Windows ao terminar.
    pub notify: bool,
}

impl Trigger {
    pub fn is_empty(&self) -> bool {
        self.folder.trim().is_empty() && self.time.trim().is_empty()
    }
}

/// "HH:MM" válido (aceita "8:5", "08h30", "8").
pub fn parse_time(t: &str) -> Option<(u32, u32)> {
    let t = t.trim().replace(['h', 'H', '.'], ":");
    let mut it = t.split(':');
    let h: u32 = it.next()?.trim().parse().ok()?;
    let m: u32 = it.next().map(|m| m.trim()).filter(|m| !m.is_empty()).map_or(Some(0), |m| m.parse().ok())?;
    (h < 24 && m < 60).then_some((h, m))
}

// i18n: fora
const DAYS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// Nome da tarefa no Agendador (pasta "Conver").
fn task_name(flow_name: &str) -> String {
    let clean: String = flow_name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    format!("Conver\\Fluxo - {}", clean.trim())
}

fn schtasks() -> std::process::Command {
    tools::command(Path::new("schtasks.exe"))
}

// i18n: dentro

/// O Conver.exe a chamar: o próprio programa ou, quando quem chama é outro executável (os testes, em
/// `target\…\deps`), o conver.exe ao lado ou uma pasta acima.
fn conver_exe() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    if me.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case("conver")) {
        return Ok(me);
    }
    let dir = me.parent().map(Path::to_path_buf).unwrap_or_default();
    [dir.join("conver.exe"), dir.parent().map(|p| p.join("conver.exe")).unwrap_or_default()]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| "não foi possível achar o Conver.exe.".to_string())
}

/// Cria, troca ou remove a tarefa agendada do fluxo conforme o gatilho. Devolve uma frase para a tela.
pub fn sync_schedule(flow_name: &str, t: &Trigger) -> Result<String, String> {
    let name = task_name(flow_name);
    if t.time.trim().is_empty() {
        let _ = schtasks().args(["/Delete", "/TN", &name, "/F"]).output();
        return Ok("Sem agendamento.".into());
    }
    let (h, m) = parse_time(&t.time).ok_or("horário inválido; use HH:MM, como 08:30.")?;
    let exe = conver_exe()?;
    // o Conver.exe (de janela) com --cli não abre console; o nome vai entre aspas
    // i18n: fora
    let mut run = format!("\"{}\" --cli flow_run --name \"{}\"", exe.display(), flow_name.replace('"', "'"));
    if t.notify {
        run.push_str(" --notify true");
    }
    // i18n: dentro
    let mut cmd = schtasks();
    cmd.args(["/Create", "/F", "/TN", &name, "/TR", &run, "/ST", &format!("{h:02}:{m:02}")]);
    let mut days: Vec<u8> = t.days.iter().copied().filter(|d| *d < 7).collect();
    days.sort();
    days.dedup();
    if days.is_empty() || days.len() == 7 {
        cmd.args(["/SC", "DAILY"]);
    } else {
        let list: Vec<&str> = days.iter().map(|d| DAYS[*d as usize]).collect();
        cmd.args(["/SC", "WEEKLY", "/D", &list.join(",")]);
    }
    let out = cmd.output().map_err(|e| format!("não foi possível usar o Agendador de Tarefas: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("o Agendador de Tarefas recusou: {msg}"));
    }
    Ok(format!("Agendado para {h:02}:{m:02}."))
}

/// Se o fluxo tem uma tarefa no Agendador.
#[cfg(test)]
pub fn is_scheduled(flow_name: &str) -> bool {
    schtasks().args(["/Query", "/TN", &task_name(flow_name)]).output().is_ok_and(|o| o.status.success())
}

pub fn unschedule(flow_name: &str) {
    let _ = sync_schedule(flow_name, &Trigger::default());
}

// ------------------------------------------------------------------ pasta vigiada

/// O que a tela mostra de cada fluxo com gatilho de pasta.
fn status_map() -> &'static Mutex<HashMap<String, String>> {
    static S: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn folder_status(flow_name: &str) -> Option<String> {
    status_map().lock().unwrap().get(flow_name).cloned()
}

fn set_status(flow_name: &str, s: String) {
    status_map().lock().unwrap().insert(flow_name.to_string(), s);
}

/// Arquivos soltos na pasta (sem subpastas, temporários ou ocultos).
fn files(folder: &Path) -> Vec<PathBuf> {
    let skip = ["crdownload", "part", "tmp", "partial", "download"];
    let mut v: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| !skip.contains(&tools::ext_lower(p).as_str()))
        .filter(|p| p.file_name().is_some_and(|n| !n.to_string_lossy().starts_with(['.', '~'])))
        .collect();
    v.sort();
    v
}

/// Arquivo que ainda está sendo copiado/baixado não abre para escrita no Windows.
fn ready(p: &Path) -> bool {
    std::fs::OpenOptions::new().read(true).write(true).open(p).is_ok()
}

pub(crate) struct Watch {
    folder: PathBuf,
    seen: HashSet<PathBuf>,
    sizes: HashMap<PathBuf, u64>,
}

/// Uma passada: arquivos novos e prontos de cada fluxo com pasta vigiada viram uma execução.
fn tick(watches: &mut HashMap<String, Watch>) {
    let flows: Vec<flow::Flow> = flow::list().into_iter().filter(|f| !f.trigger.folder.trim().is_empty()).collect();
    watches.retain(|name, _| flows.iter().any(|f| &f.name == name));
    for f in flows {
        let folder = PathBuf::from(f.trigger.folder.trim());
        if !folder.is_dir() {
            set_status(&f.name, "A pasta vigiada não existe.".into());
            watches.remove(&f.name);
            continue;
        }
        // pasta nova (ou trocada): o que já está lá não conta
        let w = watches.entry(f.name.clone()).or_insert_with(|| Watch { folder: folder.clone(), seen: HashSet::new(), sizes: HashMap::new() });
        if w.folder != folder || (w.seen.is_empty() && w.sizes.is_empty()) {
            *w = Watch { folder: folder.clone(), seen: files(&folder).into_iter().collect(), sizes: HashMap::new() };
            // marca de "já inicializado" mesmo com a pasta vazia
            w.sizes.insert(PathBuf::new(), 0);
            set_status(&f.name, "Vigiando a pasta.".into());
            continue;
        }
        let mut batch = Vec::new();
        for p in files(&folder) {
            if w.seen.contains(&p) {
                continue;
            }
            let size = tools::file_size(&p);
            let stable = w.sizes.get(&p) == Some(&size) && size > 0;
            w.sizes.insert(p.clone(), size);
            if stable && ready(&p) {
                batch.push(p);
            }
        }
        if batch.is_empty() {
            continue;
        }
        for p in &batch {
            w.seen.insert(p.clone());
            w.sizes.remove(p);
        }
        let n = batch.len();
        set_status(&f.name, format!("Rodando com {n} arquivo(s) novo(s)…"));
        let state = Mutex::new(RunState::default());
        let ctx = ops::Ctx { progress: &|_| {}, cancelled: &|| false, log: &|_| {} };
        let ai = crate::ai_api::Settings::load().config();
        let r = flow::run(&f, &Inputs { files: batch, texts: Vec::new() }, None, &state, &ctx, &ai);
        let (y, mo, d, h, mi, _) = tools::local_parts(std::time::SystemTime::now());
        let when = format!("{d:02}/{mo:02}/{y} {h:02}:{mi:02}");
        let (msg, ok) = match &r {
            Ok(v) => {
                // resultados salvos dentro da própria pasta vigiada não disparam de novo
                for res in v["results"].as_array().into_iter().flatten() {
                    if let Some(file) = res["file"].as_str() {
                        w.seen.insert(PathBuf::from(file));
                    }
                }
                let outs = v["results"].as_array().map(Vec::len).unwrap_or(0);
                let fails = v["failed_inputs"].as_array().map(Vec::len).unwrap_or(0) + v["failed_nodes"].as_u64().unwrap_or(0) as usize;
                (format!("{when}: {n} arquivo(s), {outs} resultado(s){}", if fails > 0 { format!(", {fails} erro(s)") } else { String::new() }), fails == 0)
            }
            Err(e) => (format!("{when}: erro — {e}"), false),
        };
        // arquivos criados na pasta durante a execução (saídas do próprio fluxo) também ficam de fora
        for p in files(&folder) {
            if tools::file_size(&p) > 0 && !w.sizes.contains_key(&p) && std::fs::metadata(&p).and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() < Duration::from_secs(120)) {
                w.seen.insert(p);
            }
        }
        set_status(&f.name, msg.clone());
        if f.trigger.notify {
            crate::notify::toast(if ok { "Conver — fluxo concluído" } else { "Conver — fluxo com erro" }, &format!("{}: {msg}", f.name));
        }
    }
}

/// Liga o vigia de pastas (uma thread que confere a cada 2 s). Só roda com o Conver aberto.
pub fn start_watcher() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    std::thread::spawn(|| {
        let mut watches = HashMap::new();
        loop {
            tick(&mut watches);
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(test)]
pub(crate) fn tick_for_test(watches: &mut HashMap<String, Watch>) {
    tick(watches)
}

#[cfg(test)]
pub(crate) fn new_watches() -> HashMap<String, Watch> {
    HashMap::new()
}
