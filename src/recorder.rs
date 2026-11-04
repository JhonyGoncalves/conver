//! Gravador de áudio (microfone) com o ffmpeg (DirectShow).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::media::{AFmt, audio_args};
use crate::tools;
use crate::video::VQuality;

/// Microfones e outras entradas de áudio do Windows.
pub fn devices() -> Vec<String> {
    let Some(ff) = tools::find_tool("ffmpeg") else { return Vec::new() };
    let Ok(out) = tools::command(&ff).args(["-hide_banner", "-list_devices", "true", "-f", "dshow", "-i", "dummy"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        .filter(|l| l.contains("(audio)"))
        .filter_map(|l| {
            let a = l.find('"')? + 1;
            let b = a + l[a..].find('"')?;
            Some(l[a..b].to_string())
        })
        .collect()
}

/// Uma gravação em andamento.
pub struct Recording {
    child: std::process::Child,
    pub dst: PathBuf,
    /// Microssegundos gravados até agora.
    elapsed: Arc<AtomicU64>,
    errors: Arc<Mutex<String>>,
    started: std::time::Instant,
}

impl Recording {
    pub fn start(device: &str, fmt: AFmt, dst: &Path, on_tick: impl Fn() + Send + 'static) -> Result<Self, String> {
        let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
        let mut child = tools::command(&ff)
            .args(["-hide_banner", "-y", "-nostats", "-loglevel", "error", "-progress", "pipe:1", "-f", "dshow", "-i"])
            .arg(format!("audio={device}"))
            .args(audio_args(fmt, VQuality::Alta).into_iter().skip(3)) // sem "-vn -map_metadata 0"
            .arg(dst)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
        let elapsed = Arc::new(AtomicU64::new(0));
        let errors: Arc<Mutex<String>> = Arc::default();
        let (out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let e = elapsed.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<u64>().ok()) {
                    e.store(us, Ordering::Relaxed);
                    on_tick();
                }
            }
        });
        let errs = errors.clone();
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            *errs.lock().unwrap() = s;
        });
        Ok(Self { child, dst: dst.to_path_buf(), elapsed, errors, started: std::time::Instant::now() })
    }

    /// Já se passaram alguns segundos e nenhum som chegou (entrada sem nada conectado).
    pub fn silent(&self) -> bool {
        self.started.elapsed().as_secs() >= 3 && self.elapsed.load(Ordering::Relaxed) == 0
    }

    pub fn seconds(&self) -> f64 {
        self.elapsed.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Ainda gravando? Se o ffmpeg parou sozinho (microfone desconectado…), devolve o erro.
    pub fn check(&mut self) -> Result<bool, String> {
        match self.child.try_wait() {
            Ok(None) => Ok(true),
            Ok(Some(s)) if s.success() => Ok(false),
            Ok(Some(_)) => {
                let e = self.errors.lock().unwrap().trim().to_string();
                Err(if e.is_empty() { "a gravação parou.".into() } else { e.lines().last().unwrap_or_default().to_string() })
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Para a gravação com calma ("q"), para o arquivo ficar completo.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        // sem som nenhum o ffmpeg fica esperando dados e não lê o "q": encerra logo
        let wait = if self.elapsed.load(Ordering::Relaxed) == 0 { 1 } else { 10 };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        if tools::file_size(&self.dst) == 0 {
            let e = self.errors.lock().unwrap().trim().to_string();
            return Err(if e.is_empty() {
                "nenhum som chegou deste microfone. Escolha outra entrada na lista (ex.: \"Microfone (…)\").".into()
            } else {
                e
            });
        }
        Ok(self.dst.clone())
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        // fechar o Conver no meio da gravação não deixa o ffmpeg rodando
        if let Ok(None) = self.child.try_wait() {
            if let Some(stdin) = self.child.stdin.as_mut() {
                let _ = stdin.write_all(b"q\n");
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = self.child.kill();
        }
    }
}
