//! Gravador de áudio (microfone) com o ffmpeg (DirectShow), com medidor de nível ao vivo.
//!
//! Além do arquivo, o ffmpeg manda uma cópia do som (mono, 16 kHz, float) pela saída padrão; daqui sai o tempo
//! gravado, o medidor, a forma de onda e a detecção de estouro (clipping).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

// ------------------------------------------------------------------ medidor

/// Amostras por segundo da cópia usada no medidor.
pub const RATE: u32 = 16_000;
/// Cada bloco do medidor tem 50 ms.
const BLOCK: usize = (RATE / 20) as usize;
/// Blocos guardados para a forma de onda (60 s).
const KEEP: usize = 1200;
/// A partir daqui (−0,1 dBFS) conta como estouro.
pub const CLIP: f32 = 0.989;

#[derive(Clone, Copy)]
pub struct Block {
    pub peak: f32,
    pub clipped: bool,
}

/// Níveis do som que está chegando.
pub struct Levels {
    /// Blocos de 50 ms, do mais antigo para o mais novo.
    pub blocks: VecDeque<Block>,
    /// Amostras recebidas desde o começo.
    pub samples: u64,
    /// Blocos que estouraram desde o começo.
    pub clips: u32,
    pub last_clip: Option<Instant>,
    hold: (f32, Instant),
}

impl Default for Levels {
    fn default() -> Self {
        Self { blocks: VecDeque::new(), samples: 0, clips: 0, last_clip: None, hold: (0.0, Instant::now()) }
    }
}

impl Levels {
    /// Pico mais alto dos últimos `ms` milissegundos (0..1).
    pub fn recent_peak(&self, ms: u32) -> f32 {
        let n = (ms / 50).max(1) as usize;
        self.blocks.iter().rev().take(n).map(|b| b.peak).fold(0.0, f32::max)
    }

    /// Pico que fica parado por 1,5 s antes de cair (a marquinha do medidor).
    pub fn peak_hold(&self) -> f32 {
        if self.hold.1.elapsed() < Duration::from_millis(1500) { self.hold.0 } else { self.recent_peak(300) }
    }

    /// Estourou nos últimos `ms` milissegundos?
    pub fn clipped_recently(&self, ms: u64) -> bool {
        self.last_clip.is_some_and(|t| t.elapsed() < Duration::from_millis(ms))
    }

    fn push(&mut self, b: Block) {
        if b.clipped {
            self.clips += 1;
            self.last_clip = Some(Instant::now());
        }
        if b.peak >= self.hold.0 || self.hold.1.elapsed() >= Duration::from_millis(1500) {
            self.hold = (b.peak, Instant::now());
        }
        self.blocks.push_back(b);
        while self.blocks.len() > KEEP {
            self.blocks.pop_front();
        }
    }
}

pub type Meter = Arc<Mutex<Levels>>;

/// Decibéis (dBFS) de um nível 0..1; −90 para silêncio.
pub fn db(v: f32) -> f32 {
    if v <= 1e-5 { -90.0 } else { 20.0 * v.log10() }
}

/// Lê o som em float de 32 bits da saída do ffmpeg e preenche o medidor.
fn read_levels(mut out: impl Read + Send + 'static, meter: Meter, on_tick: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::with_capacity(4);
        let (mut peak, mut n) = (0.0f32, 0usize);
        loop {
            let got = match out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(k) => k,
            };
            carry.extend_from_slice(&buf[..got]);
            let whole = carry.len() / 4 * 4;
            let mut blocks = Vec::new();
            for c in carry[..whole].chunks_exact(4) {
                let s = f32::from_le_bytes([c[0], c[1], c[2], c[3]]).abs();
                peak = peak.max(s);
                n += 1;
                if n == BLOCK {
                    blocks.push(Block { peak, clipped: peak >= CLIP });
                    (peak, n) = (0.0, 0);
                }
            }
            carry.drain(..whole);
            if let Ok(mut m) = meter.lock() {
                m.samples += (whole / 4) as u64;
                for b in blocks.iter() {
                    m.push(*b);
                }
            }
            if !blocks.is_empty() {
                on_tick();
            }
        }
    });
}

fn read_errors(mut err: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let errors: Arc<Mutex<String>> = Arc::default();
    let errs = errors.clone();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        *errs.lock().unwrap() = s;
    });
    errors
}

/// Cópia do som para o medidor: mono, 16 kHz, float, pela saída padrão.
const METER_OUT: [&str; 7] = ["-ac", "1", "-ar", "16000", "-f", "f32le", "pipe:1"];

// ---------------------------------------------------------------- gravação

/// Uma gravação em andamento.
pub struct Recording {
    child: std::process::Child,
    pub dst: PathBuf,
    pub meter: Meter,
    errors: Arc<Mutex<String>>,
    started: Instant,
}

impl Recording {
    pub fn start(device: &str, fmt: AFmt, dst: &Path, on_tick: impl Fn() + Send + 'static) -> Result<Self, String> {
        let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
        let mut child = tools::command(&ff)
            .args(["-hide_banner", "-y", "-nostats", "-loglevel", "error", "-f", "dshow", "-i"])
            .arg(format!("audio={device}"))
            .args(audio_args(fmt, VQuality::Alta).into_iter().skip(3)) // sem "-vn -map_metadata 0"
            .arg(dst)
            .args(METER_OUT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
        let meter: Meter = Arc::default();
        read_levels(child.stdout.take().unwrap(), meter.clone(), on_tick);
        let errors = read_errors(child.stderr.take().unwrap());
        Ok(Self { child, dst: dst.to_path_buf(), meter, errors, started: Instant::now() })
    }

    fn samples(&self) -> u64 {
        self.meter.lock().map(|m| m.samples).unwrap_or(0)
    }

    /// Já se passaram alguns segundos e nenhum som chegou (entrada sem nada conectado).
    pub fn silent(&self) -> bool {
        self.started.elapsed().as_secs() >= 3 && self.samples() == 0
    }

    pub fn seconds(&self) -> f64 {
        self.samples() as f64 / RATE as f64
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
        let wait = if self.samples() == 0 { 1 } else { 10 };
        let deadline = Instant::now() + Duration::from_secs(wait);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
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
            std::thread::sleep(Duration::from_millis(300));
            let _ = self.child.kill();
        }
    }
}

// ------------------------------------------------------------ testar o microfone

/// Só escuta o microfone para mostrar o nível, sem gravar arquivo ("Testar microfone").
pub struct Monitor {
    child: std::process::Child,
    pub meter: Meter,
    errors: Arc<Mutex<String>>,
    pub device: String,
}

impl Monitor {
    pub fn start(device: &str, on_tick: impl Fn() + Send + 'static) -> Result<Self, String> {
        Self::start_input(&["-f".into(), "dshow".into(), "-i".into(), format!("audio={device}")], device, on_tick)
    }

    /// Escuta qualquer entrada do ffmpeg (o modo de captura e os testes usam um som gerado).
    pub fn start_input(input: &[String], device: &str, on_tick: impl Fn() + Send + 'static) -> Result<Self, String> {
        let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
        let mut child = tools::command(&ff)
            .args(["-hide_banner", "-nostats", "-loglevel", "error"])
            .args(input)
            .args(METER_OUT)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
        let meter: Meter = Arc::default();
        read_levels(child.stdout.take().unwrap(), meter.clone(), on_tick);
        let errors = read_errors(child.stderr.take().unwrap());
        Ok(Self { child, meter, errors, device: device.to_string() })
    }

    /// Se o ffmpeg parou (microfone em uso, desconectado…), devolve o erro.
    pub fn check(&mut self) -> Result<(), String> {
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(_)) => {
                let e = self.errors.lock().unwrap().trim().to_string();
                Err(if e.is_empty() { "o microfone parou de responder.".into() } else { e.lines().last().unwrap_or_default().to_string() })
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
