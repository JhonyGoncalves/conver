//! Gravar a tela (inteira, uma janela ou uma área) em MP4 com o ffmpeg (gdigrab), com microfone opcional.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::tools;

#[derive(Clone, PartialEq, Debug)]
pub enum Area {
    TelaInteira,
    /// Título exato da janela.
    Janela(String),
    /// x, y, largura, altura (em pixels da tela).
    Regiao { x: i32, y: i32, w: u32, h: u32 },
}

#[derive(Clone, PartialEq, Debug)]
pub struct Options {
    pub area: Area,
    pub fps: u32,
    /// Microfone (nome do recorder::devices) ou nenhum.
    pub mic: Option<String>,
    pub cursor: bool,
}

/// Títulos das janelas visíveis (para gravar só uma delas).
pub fn windows() -> Vec<String> {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible};
    use windows::core::BOOL;
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        // SAFETY: lp aponta para o Vec criado abaixo, que vive durante toda a enumeração
        let list = unsafe { &mut *(lp.0 as *mut Vec<String>) };
        unsafe {
            if IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
                let n = GetWindowTextLengthW(hwnd);
                if n > 0 {
                    let mut buf = vec![0u16; n as usize + 1];
                    let got = GetWindowTextW(hwnd, &mut buf);
                    let t = String::from_utf16_lossy(&buf[..got as usize]);
                    if !t.trim().is_empty() && t != "Program Manager" && !list.contains(&t) {
                        list.push(t);
                    }
                }
            }
        }
        BOOL(1)
    }
    let mut list: Vec<String> = Vec::new();
    // SAFETY: o callback só usa o ponteiro durante a chamada
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

fn input_args(o: &Options) -> Vec<String> {
    let mut a: Vec<String> = vec!["-f".into(), "gdigrab".into(), "-framerate".into(), o.fps.clamp(5, 60).to_string()];
    a.extend(["-draw_mouse".into(), if o.cursor { "1" } else { "0" }.into()]);
    match &o.area {
        Area::TelaInteira => a.extend(["-i".into(), "desktop".into()]),
        Area::Janela(t) => a.extend(["-i".into(), format!("title={t}")]),
        Area::Regiao { x, y, w, h } => {
            // o H.264 precisa de largura e altura pares
            let (w, h) = ((*w).max(16) & !1, (*h).max(16) & !1);
            a.extend([
                "-offset_x".into(), x.to_string(), "-offset_y".into(), y.to_string(),
                "-video_size".into(), format!("{w}x{h}"), "-i".into(), "desktop".into(),
            ]);
        }
    }
    if let Some(m) = &o.mic {
        a.extend(["-f".into(), "dshow".into(), "-i".into(), format!("audio={m}")]);
    }
    a
}

/// Uma gravação de tela em andamento.
pub struct ScreenRecording {
    child: std::process::Child,
    pub dst: PathBuf,
    elapsed: Arc<AtomicU64>,
    errors: Arc<Mutex<String>>,
}

impl ScreenRecording {
    pub fn start(o: &Options, dst: &Path, on_tick: impl Fn() + Send + 'static) -> Result<Self, String> {
        let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
        let mut args = vec!["-hide_banner".to_string(), "-y".into(), "-nostats".into(), "-loglevel".into(), "error".into(),
                            "-progress".into(), "pipe:1".into(), "-thread_queue_size".into(), "1024".into()];
        args.extend(input_args(o));
        args.extend(
            ["-c:v", "libx264", "-preset", "ultrafast", "-crf", "23", "-pix_fmt", "yuv420p",
             "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"]
                .map(String::from),
        );
        if o.mic.is_some() {
            args.extend(["-c:a", "aac", "-b:a", "160k"].map(String::from));
        }
        args.extend(["-movflags", "+faststart"].map(String::from));
        let mut child = tools::command(&ff)
            .args(&args)
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
        Ok(Self { child, dst: dst.to_path_buf(), elapsed, errors })
    }

    pub fn seconds(&self) -> f64 {
        self.elapsed.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Ainda gravando? Se o ffmpeg parou sozinho (janela fechada…), devolve o erro.
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

    /// Para com calma ("q") para o MP4 ficar completo.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = stdin.write_all(b"q\n");
            let _ = stdin.flush();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
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
            return Err(if e.is_empty() { "nada foi gravado.".into() } else { e.lines().last().unwrap_or_default().to_string() });
        }
        Ok(self.dst.clone())
    }
}

impl Drop for ScreenRecording {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            if let Some(stdin) = self.child.stdin.as_mut() {
                let _ = stdin.write_all(b"q\n");
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = self.child.kill();
        }
    }
}

/// Grava por um tempo fixo (usado pela linha de comando e pelo MCP).
pub fn record_for(o: &Options, dst: &Path, seconds: f64, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let mut rec = ScreenRecording::start(o, dst, || {})?;
    let started = std::time::Instant::now();
    while started.elapsed().as_secs_f64() < seconds && !cancelled() {
        if !rec.check()? {
            break;
        }
        on_progress((started.elapsed().as_secs_f64() / seconds).min(1.0) as f32);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let secs = rec.seconds();
    let path = rec.stop()?;
    Ok((format!("{} • {}", crate::video::fmt_time(secs.max(started.elapsed().as_secs_f64().min(seconds))), tools::human_size(tools::file_size(&path))), path))
}
