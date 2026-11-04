//! Tarefas em segundo plano com progresso, status, log e cancelamento.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Ok,
    Err,
    Info,
}

pub struct JobState {
    pub running: bool,
    pub progress: f32,
    pub status: String,
    pub log: Vec<(LogKind, String)>,
    pub done_folder: Option<PathBuf>,
}

impl Default for JobState {
    fn default() -> Self {
        Self { running: false, progress: 0.0, status: "Pronto.".into(), log: Vec::new(), done_folder: None }
    }
}

#[derive(Clone, Default)]
pub struct Job {
    state: Arc<Mutex<JobState>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn state(&self) -> MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.state().status = "Cancelando…".into();
    }

    /// Inicia `work` numa thread. Não faz nada se já houver uma tarefa rodando.
    pub fn start(&self, ctx: &eframe::egui::Context, work: impl FnOnce(&Ctl) + Send + 'static) {
        {
            let mut s = self.state();
            if s.running {
                return;
            }
            *s = JobState { running: true, status: "Iniciando…".into(), ..Default::default() };
        }
        self.cancel.store(false, Ordering::Relaxed);
        let ctl = Ctl { job: self.clone(), ctx: ctx.clone() };
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&ctl)));
            if result.is_err() {
                ctl.log(LogKind::Err, "Erro inesperado.");
                ctl.status("Erro.");
            }
            let (status, failed) = {
                let mut s = ctl.job.state();
                s.running = false;
                let failed = s.log.iter().any(|(k, _)| *k == LogKind::Err);
                (s.status.clone(), failed)
            };
            // avisa só tarefas demoradas (e não quando o usuário cancelou)
            if started.elapsed().as_secs() >= 4 && !ctl.cancelled() {
                crate::notify::finished(&status, !failed);
            }
            ctl.ctx.request_repaint();
        });
    }
}

/// Controle entregue à thread de trabalho.
pub struct Ctl {
    job: Job,
    ctx: eframe::egui::Context,
}

impl Ctl {
    fn update(&self, f: impl FnOnce(&mut JobState)) {
        f(&mut self.job.state());
        self.ctx.request_repaint();
    }

    pub fn progress(&self, p: f32) {
        self.update(|s| s.progress = p.clamp(0.0, 1.0));
    }

    pub fn status(&self, text: impl Into<String>) {
        let text = text.into();
        self.update(|s| s.status = text);
    }

    pub fn log(&self, kind: LogKind, text: impl Into<String>) {
        let text = text.into();
        self.update(|s| s.log.push((kind, text)));
    }

    pub fn done_folder(&self, p: PathBuf) {
        self.update(|s| s.done_folder = Some(p));
    }

    pub fn cancelled(&self) -> bool {
        self.job.cancel.load(Ordering::Relaxed)
    }
}
