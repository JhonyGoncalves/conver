//! Gravar a tela (inteira, um monitor, uma janela ou uma área) em MP4 com o ffmpeg (gdigrab), com microfone
//! opcional, qualidade e resolução máxima. A pausa grava em trechos que são juntados no fim (sem recomprimir).

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

/// Qualidade do vídeo: quanto mais alta, maior o arquivo.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quality {
    Alta,
    #[default]
    Media,
    Leve,
}

impl Quality {
    /// (CRF do H.264, preset do x264, kbps do áudio)
    fn params(self) -> (u32, &'static str, u32) {
        match self {
            Quality::Alta => (18, "superfast", 192),
            Quality::Media => (23, "ultrafast", 160),
            Quality::Leve => (30, "ultrafast", 96),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Options {
    pub area: Area,
    pub fps: u32,
    /// Microfone (nome do recorder::devices) ou nenhum.
    pub mic: Option<String>,
    pub cursor: bool,
    pub quality: Quality,
    /// Altura máxima do vídeo (1080, 720…); `None` = a resolução da tela.
    pub max_height: Option<u32>,
}

impl Options {
    #[cfg(test)]
    pub fn new(area: Area) -> Self {
        Options { area, fps: 30, mic: None, cursor: true, quality: Quality::Media, max_height: None }
    }
}

/// Um monitor do Windows, em pixels da tela (o principal começa em 0, 0).
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub primary: bool,
}

impl Monitor {
    pub fn area(&self) -> Area {
        Area::Regiao { x: self.x, y: self.y, w: self.w, h: self.h }
    }
}

/// Monitores ligados: o principal primeiro, depois da esquerda para a direita.
pub fn monitors() -> Vec<Monitor> {
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
    use windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware;
    use windows::core::BOOL;
    unsafe extern "system" fn cb(hmon: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
        // SAFETY: lp aponta para o Vec criado abaixo, que vive durante toda a enumeração
        let list = unsafe { &mut *(lp.0 as *mut Vec<Monitor>) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if unsafe { GetMonitorInfoW(hmon, &mut info) }.as_bool() {
            let r = info.rcMonitor;
            list.push(Monitor { x: r.left, y: r.top, w: (r.right - r.left) as u32, h: (r.bottom - r.top) as u32, primary: info.dwFlags & 1 != 0 });
        }
        BOOL(1)
    }
    let mut list: Vec<Monitor> = Vec::new();
    // SAFETY: o callback só usa o ponteiro durante a chamada. Pixels reais também na linha de comando
    // (a janela do Conver já é DPI-aware; o processo da CLI não era).
    unsafe {
        let _ = SetProcessDPIAware();
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut list as *mut _ as isize));
    }
    list.sort_by_key(|m| (!m.primary, m.x, m.y));
    list
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

// ------------------------------------------------------------------ prévia

/// Uma imagem reduzida do que vai ser gravado.
pub struct Grab {
    pub width: u32,
    pub height: u32,
    /// RGBA, linha por linha de cima para baixo.
    pub rgba: Vec<u8>,
    /// Tamanho real (em pixels) do que vai ser gravado.
    pub source: (u32, u32),
}

/// Captura a área (tela inteira, a parte de dentro de uma janela ou uma região), reduzida para no máximo `max_w`
/// de largura. A janela é capturada mesmo se estiver coberta por outras (PrintWindow); minimizada ou fechada, `None`.
pub fn grab(area: &Area, max_w: u32) -> Option<Grab> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GdiFlush, GetDC, HALFTONE, ReleaseDC, SRCCOPY, SelectObject, SetBrushOrgEx, SetStretchBltMode,
        StretchBlt,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetClientRect, GetSystemMetrics, IsIconic, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };
    use windows::core::PCWSTR;

    // SAFETY: só chamadas GDI com objetos criados aqui, todos liberados antes de sair
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        // de onde copiar: a tela ou uma cópia da janela feita pelo PrintWindow
        let mut window_copy = None;
        let (src, sx, sy, sw, sh) = match area {
            Area::TelaInteira => (
                screen,
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            ),
            Area::Regiao { x, y, w, h } => (screen, *x, *y, (*w).max(16) as i32, (*h).max(16) as i32),
            Area::Janela(title) => {
                let wide: Vec<u16> = title.encode_utf16().chain([0]).collect();
                let found = FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())).ok().filter(|h| !h.is_invalid() && !IsIconic(*h).as_bool());
                let mut rc = RECT::default();
                match found {
                    Some(hwnd) if GetClientRect(hwnd, &mut rc).is_ok() && rc.right > 0 && rc.bottom > 0 => {
                        let mem = CreateCompatibleDC(Some(screen));
                        let bmp = CreateCompatibleBitmap(screen, rc.right, rc.bottom);
                        let old = SelectObject(mem, bmp.into());
                        // 1 = só a parte de dentro (como o gdigrab grava), 2 = também o conteúdo desenhado pela GPU
                        let _ = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(3));
                        window_copy = Some((mem, bmp, old));
                        (mem, 0, 0, rc.right, rc.bottom)
                    }
                    _ => {
                        ReleaseDC(None, screen);
                        return None;
                    }
                }
            }
        };
        let scale = (max_w as f32 / sw.max(1) as f32).min(1.0);
        let (dw, dh) = (((sw as f32 * scale).round() as i32).max(1), ((sh as f32 * scale).round() as i32).max(1));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: dw,
                biHeight: -dh, // de cima para baixo
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut result = None;
        if let Ok(dib) = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            let dst = CreateCompatibleDC(Some(screen));
            let old = SelectObject(dst, dib.into());
            SetStretchBltMode(dst, HALFTONE);
            let _ = SetBrushOrgEx(dst, 0, 0, None);
            if StretchBlt(dst, 0, 0, dw, dh, Some(src), sx, sy, sw, sh, SRCCOPY).as_bool() && !bits.is_null() {
                let _ = GdiFlush();
                let bgra = std::slice::from_raw_parts(bits as *const u8, (dw * dh * 4) as usize);
                let rgba: Vec<u8> = bgra.chunks_exact(4).flat_map(|px| [px[2], px[1], px[0], 255]).collect();
                result = Some(Grab { width: dw as u32, height: dh as u32, rgba, source: (sw as u32, sh as u32) });
            }
            SelectObject(dst, old);
            let _ = DeleteDC(dst);
            let _ = DeleteObject(dib.into());
        }
        if let Some((mem, bmp, old)) = window_copy {
            SelectObject(mem, old);
            let _ = DeleteDC(mem);
            let _ = DeleteObject(bmp.into());
        }
        ReleaseDC(None, screen);
        result
    }
}

/// Prévia ao vivo: uma thread captura a área algumas vezes por segundo enquanto a tela "Gravar a tela" está aberta,
/// e para sozinha quando ninguém mais olha (`touch` deixa de ser chamado).
pub struct Preview {
    area: Arc<Mutex<Option<Area>>>,
    pub frame: Arc<Mutex<Option<Grab>>>,
    /// A última captura falhou (janela minimizada ou fechada).
    pub missing: Arc<std::sync::atomic::AtomicBool>,
    seen: Arc<Mutex<std::time::Instant>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    /// Gravando: captura menos vezes, para não pesar.
    pub slow: Arc<std::sync::atomic::AtomicBool>,
}

impl Preview {
    pub fn new() -> Self {
        Self {
            area: Arc::default(),
            frame: Arc::default(),
            missing: Arc::default(),
            seen: Arc::new(Mutex::new(std::time::Instant::now())),
            running: Arc::default(),
            slow: Arc::default(),
        }
    }

    /// Chamado a cada quadro da tela: troca a área, se mudou (devolve `true`), e mantém a thread viva.
    pub fn touch(&self, ctx: &eframe::egui::Context, area: Area) -> bool {
        let changed = {
            let mut a = self.area.lock().unwrap();
            let changed = a.as_ref() != Some(&area);
            if changed {
                *a = Some(area);
                *self.frame.lock().unwrap() = None; // não mostra a área antiga enquanto a nova não chega
                self.missing.store(false, Ordering::Relaxed);
            }
            changed
        };
        *self.seen.lock().unwrap() = std::time::Instant::now();
        if self.running.swap(true, Ordering::SeqCst) {
            return changed;
        }
        let (area, frame, missing, seen, running, slow, ctx) = (
            self.area.clone(),
            self.frame.clone(),
            self.missing.clone(),
            self.seen.clone(),
            self.running.clone(),
            self.slow.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            while seen.lock().unwrap().elapsed() < std::time::Duration::from_millis(1500) {
                let current = area.lock().unwrap().clone();
                if let Some(a) = current {
                    let g = grab(&a, 960);
                    // a área pode ter mudado durante a captura
                    if area.lock().unwrap().as_ref() == Some(&a) {
                        missing.store(g.is_none(), Ordering::Relaxed);
                        if g.is_some() {
                            *frame.lock().unwrap() = g;
                        }
                        ctx.request_repaint();
                    }
                }
                let pause = if slow.load(Ordering::Relaxed) { 500 } else { 200 };
                std::thread::sleep(std::time::Duration::from_millis(pause));
            }
            running.store(false, Ordering::SeqCst);
        });
        changed
    }
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
        let (crf, preset, akbps) = o.quality.params();
        // largura e altura pares (H.264); com altura máxima, reduz mantendo a proporção
        let scale = match o.max_height {
            Some(h) => format!("scale=-2:trunc(min(ih\\,{h})/2)*2"),
            None => "scale=trunc(iw/2)*2:trunc(ih/2)*2".to_string(),
        };
        args.extend(["-c:v".into(), "libx264".into(), "-preset".into(), preset.into(), "-crf".into(), crf.to_string(), "-pix_fmt".into(), "yuv420p".into(), "-vf".into(), scale]);
        if o.mic.is_some() {
            args.extend(["-c:a".into(), "aac".into(), "-b:a".into(), format!("{akbps}k")]);
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

// ---------------------------------------------------------------- com pausa

/// Uma gravação que pode ser pausada: cada trecho é um MP4 numa pasta temporária, juntados no fim.
pub struct Session {
    opts: Options,
    pub dst: PathBuf,
    parts_dir: PathBuf,
    parts: Vec<PathBuf>,
    current: Option<ScreenRecording>,
    /// Segundos dos trechos já fechados.
    done: f64,
    on_tick: std::sync::Arc<dyn Fn() + Send + Sync>,
}

impl Session {
    pub fn start(o: &Options, dst: &Path, on_tick: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        let stamp: String = tools::now_string().chars().filter(|c| c.is_ascii_digit()).collect();
        let parts_dir = std::env::temp_dir().join(format!("conver_gravacao_{stamp}_{}", std::process::id()));
        std::fs::create_dir_all(&parts_dir).map_err(|e| format!("não foi possível criar a pasta temporária: {e}"))?;
        let mut s = Session { opts: o.clone(), dst: dst.to_path_buf(), parts_dir, parts: Vec::new(), current: None, done: 0.0, on_tick: std::sync::Arc::new(on_tick) };
        s.resume()?;
        Ok(s)
    }

    pub fn paused(&self) -> bool {
        self.current.is_none()
    }

    pub fn seconds(&self) -> f64 {
        self.done + self.current.as_ref().map(|r| r.seconds()).unwrap_or(0.0)
    }

    /// Fecha o trecho atual (o MP4 fica completo) e espera o "continuar".
    pub fn pause(&mut self) -> Result<(), String> {
        let Some(rec) = self.current.take() else { return Ok(()) };
        let secs = rec.seconds();
        match rec.stop() {
            Ok(p) => {
                self.done += secs;
                self.parts.push(p);
                Ok(())
            }
            // um trecho curto demais pode sair vazio: não é motivo para perder a gravação
            Err(e) if !self.parts.is_empty() || secs < 1.0 => {
                let _ = e;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if self.current.is_some() {
            return Ok(());
        }
        let part = self.parts_dir.join(format!("parte_{:03}.mp4", self.parts.len() + 1));
        let tick = self.on_tick.clone();
        self.current = Some(ScreenRecording::start(&self.opts, &part, move || tick())?);
        Ok(())
    }

    /// Ainda gravando? Se o ffmpeg parou sozinho (janela fechada…), devolve o erro.
    pub fn check(&mut self) -> Result<bool, String> {
        match self.current.as_mut() {
            Some(r) => r.check(),
            None => Ok(true),
        }
    }

    /// Para e junta os trechos no arquivo final.
    pub fn stop(mut self) -> Result<PathBuf, String> {
        self.pause()?;
        let parts: Vec<PathBuf> = self.parts.iter().filter(|p| tools::file_size(p) > 0).cloned().collect();
        let result = match parts.len() {
            0 => Err("nada foi gravado.".to_string()),
            1 => std::fs::rename(&parts[0], &self.dst).or_else(|_| std::fs::copy(&parts[0], &self.dst).map(|_| ())).map_err(|e| e.to_string()).map(|_| self.dst.clone()),
            _ => concat(&parts, &self.dst),
        };
        let _ = std::fs::remove_dir_all(&self.parts_dir);
        result
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // o Conver fechou no meio: guarda o que já foi gravado em vez de jogar fora
        if self.current.is_some() || !self.parts.is_empty() {
            let _ = self.pause();
            let parts: Vec<PathBuf> = self.parts.iter().filter(|p| tools::file_size(p) > 0).cloned().collect();
            if !parts.is_empty() && !self.dst.exists() {
                let _ = if parts.len() == 1 { std::fs::copy(&parts[0], &self.dst).map(|_| self.dst.clone()).map_err(|e| e.to_string()) } else { concat(&parts, &self.dst) };
            }
            let _ = std::fs::remove_dir_all(&self.parts_dir);
        }
    }
}

/// Junta MP4 com os mesmos codecs, sem recomprimir (concat demuxer do ffmpeg).
fn concat(parts: &[PathBuf], dst: &Path) -> Result<PathBuf, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let list = dst.with_extension("partes.txt");
    let text: String = parts.iter().map(|p| format!("file '{}'\n", p.display().to_string().replace('\'', "'\\''"))).collect();
    std::fs::write(&list, text).map_err(|e| e.to_string())?;
    let out = tools::command(&ff)
        .args(["-hide_banner", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .args(["-c", "copy", "-movflags", "+faststart"])
        .arg(dst)
        .output()
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&list);
    let out = out?;
    if !out.status.success() || tools::file_size(dst) == 0 {
        let e = String::from_utf8_lossy(&out.stderr).trim().lines().last().unwrap_or("falhou").to_string();
        return Err(format!("não foi possível juntar os trechos da gravação: {e}"));
    }
    Ok(dst.to_path_buf())
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
