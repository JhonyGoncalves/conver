//! Ditado ao vivo: o microfone vai para o whisper-server (whisper.cpp, com o modelo carregado uma vez só) em trechos
//! curtos. Enquanto a pessoa fala, o texto provisório aparece; numa pausa (ou a cada 15 s) ele é confirmado.

use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::recorder::{Meter, Monitor, RATE};
use crate::transcribe::WModel;
use crate::{ai, tools};

#[derive(Default)]
pub struct State {
    /// Texto confirmado.
    pub text: String,
    /// O que está sendo dito agora (pode mudar).
    pub partial: String,
    pub status: String,
    pub error: Option<String>,
    pub ready: bool,
}

pub struct Dictation {
    pub state: Arc<Mutex<State>>,
    pub meter: Option<Meter>,
    stop: Arc<AtomicBool>,
    /// Começa a valer quando o microfone abre (depois de carregar o modelo).
    meter_slot: Arc<Mutex<Option<Meter>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn rms(s: &[f32]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32).sqrt()
}

/// WAV de 16 bits, mono, 16 kHz, na memória.
// i18n: fora
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&RATE.to_le_bytes());
    v.extend_from_slice(&(RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    v
}

// i18n: dentro
/// O Whisper às vezes "ouve" coisas no silêncio ou marca sons entre colchetes: isso não é fala.
fn clean(text: &str) -> String {
    let t = text.replace('\n', " ");
    let mut out = String::new();
    let mut depth = 0;
    for c in t.chars() {
        match c {
            '[' | '(' | '*' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let low = out.to_lowercase();
    if ["obrigado.", "obrigada.", "legendas pela comunidade amara.org", "thank you.", "you"].contains(&low.trim()) {
        return String::new();
    }
    out
}

fn transcribe(port: u16, samples: &[f32], lang: &str, context: &str) -> Result<String, String> {
    let boundary = "----conver-ditado-7d1";
    let mut body = Vec::new();
    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    };
    field("response_format", "json");
    field("temperature", "0");
    field("language", lang);
    // o fim do que já foi dito dá contexto (nomes, assunto) e evita frases cortadas sem sentido
    if !context.trim().is_empty() {
        let tail: String = context.chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
        field("prompt", tail.trim());
    }
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    body.extend_from_slice(&wav(samples));
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let r = ureq::post(&format!("http://127.0.0.1:{port}/inference"))
        .timeout(Duration::from_secs(60))
        .set("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .send_bytes(&body)
        .map_err(|e| format!("o reconhecimento de fala não respondeu: {e}"))?;
    let v: serde_json::Value = serde_json::from_str(&r.into_string().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(clean(v["text"].as_str().unwrap_or_default()))
}

fn free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Dictation {
    /// Começa a ouvir `device` (o modelo é baixado/carregado antes; o estado conta o que está acontecendo).
    pub fn start(device: &str, model: WModel, lang: &str, ctx: &eframe::egui::Context) -> Self {
        Self::start_input(crate::recorder::dshow_input(device), model, lang, ctx)
    }

    /// Com qualquer entrada do ffmpeg (os testes tocam uma fala gravada como se fosse o microfone).
    pub fn start_input(input: Vec<String>, model: WModel, lang: &str, ctx: &eframe::egui::Context) -> Self {
        let state: Arc<Mutex<State>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let meter_slot: Arc<Mutex<Option<Meter>>> = Arc::default();
        let (st, stp, slot, lang, ctx) = (state.clone(), stop.clone(), meter_slot.clone(), if lang.trim().is_empty() { "auto".to_string() } else { lang.to_string() }, ctx.clone());
        let thread = std::thread::spawn(move || {
            let set = |f: &dyn Fn(&mut State)| {
                f(&mut st.lock().unwrap());
                ctx.request_repaint();
            };
            if let Err(e) = run(&input, model, &lang, &st, &stp, &slot, &ctx) {
                set(&|s| {
                    s.error = Some(e.clone());
                    s.status = String::new();
                });
            }
        });
        Self { state, meter: None, stop, meter_slot, thread: Some(thread) }
    }

    /// O medidor do microfone (depois que ele abre).
    pub fn meter(&mut self) -> Option<Meter> {
        if self.meter.is_none() {
            self.meter = self.meter_slot.lock().unwrap().clone();
        }
        self.meter.clone()
    }

    pub fn running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Para de ouvir; o que ainda não foi confirmado é transcrito antes de sair.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Dictation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(input: &[String], model: WModel, lang: &str, st: &Mutex<State>, stop: &AtomicBool, slot: &Mutex<Option<Meter>>, ctx: &eframe::egui::Context) -> Result<(), String> {
    let set = |f: &dyn Fn(&mut State)| {
        f(&mut st.lock().unwrap());
        ctx.request_repaint();
    };
    let exe: PathBuf = tools::find_tool_file(r"whisper\whisper-server.exe").ok_or("whisper-server.exe não encontrado (pasta bin\\whisper).")?;
    set(&|s| s.status = "Preparando o modelo…".into());
    let spec = model.spec();
    let model_path = ai::ensure_model(&spec, &|p| set(&|s| s.status = format!("Baixando o modelo… {:.0}%", p * 100.0)), &|| stop.load(Ordering::Relaxed))?;
    let port = free_port().ok_or("nenhuma porta livre para o reconhecimento de fala.")?;
    let child = tools::command(&exe)
        .current_dir(model_path.parent().unwrap_or(std::path::Path::new(".")))
        .args(["-m", spec.file, "--host", "127.0.0.1", "--port", &port.to_string(), "-t", &ai::threads().to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o reconhecimento de fala: {e}"))?;
    let mut server = Server(child);
    // espera o servidor carregar o modelo
    let t = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        if ureq::get(&format!("http://127.0.0.1:{port}/")).timeout(Duration::from_millis(500)).call().is_ok() {
            break;
        }
        if let Ok(Some(_)) = server.0.try_wait() {
            return Err("o reconhecimento de fala fechou ao carregar o modelo.".into());
        }
        if t.elapsed() > Duration::from_secs(60) {
            return Err("o reconhecimento de fala demorou demais para começar.".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let ctx2 = ctx.clone();
    let mut mic = Monitor::start_with(input, "", true, move || ctx2.request_repaint())?;
    *slot.lock().unwrap() = Some(mic.meter.clone());
    set(&|s| {
        s.ready = true;
        s.status = "Ouvindo… fale à vontade.".into();
    });
    let rate = RATE as usize;
    let mut last_req = Instant::now();
    let mut stopping = false;
    // a entrada acabou (microfone desconectado, ou o fim de um arquivo): confirma o que já foi dito e para
    let mut ended: Option<String> = None;
    loop {
        std::thread::sleep(Duration::from_millis(150));
        if stop.load(Ordering::Relaxed) {
            stopping = true;
        }
        if ended.is_none() {
            if let Err(e) = mic.check() {
                ended = Some(e);
                stopping = true;
                std::thread::sleep(Duration::from_millis(300)); // o resto do som ainda chega pela saída
            }
        }
        let window: Vec<f32> = mic.meter.lock().unwrap().audio.clone().unwrap_or_default();
        let secs = window.len() as f32 / rate as f32;
        let quiet = rate * 7 / 10;
        let tail_quiet = window.len() > quiet && rms(&window[window.len() - quiet..]) < 0.008;
        // confirma numa pausa depois de pelo menos 1,5 s de fala, a cada 15 s, ou ao parar
        let commit = stopping || secs > 15.0 || (secs > 1.5 && tail_quiet);
        let due = last_req.elapsed() > Duration::from_millis(1200) && secs > 0.8;
        if !(commit || due) {
            continue;
        }
        let taken = window.len();
        // só silêncio: nem manda (o Whisper inventaria frases)
        let context = st.lock().unwrap().text.clone();
        let text = if rms(&window) < 0.004 { String::new() } else { transcribe(port, &window, lang, &context)? };
        last_req = Instant::now();
        if commit {
            if let Some(a) = mic.meter.lock().unwrap().audio.as_mut() {
                a.drain(..taken.min(a.len()));
            }
            set(&|s| {
                if !text.is_empty() {
                    if !s.text.is_empty() && !s.text.ends_with(['\n', ' ']) {
                        s.text.push(' ');
                    }
                    s.text.push_str(&text);
                }
                s.partial.clear();
            });
        } else {
            set(&|s| s.partial = text.clone());
        }
        if stopping {
            break;
        }
    }
    drop(mic);
    drop(server);
    set(&|s| {
        s.status = match &ended {
            Some(e) if !stop.load(Ordering::Relaxed) => format!("Parou: {e}"),
            _ => "Parado.".into(),
        };
        s.ready = false;
    });
    Ok(())
}
