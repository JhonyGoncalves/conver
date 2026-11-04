//! Separar voz e instrumental com IA (UVR MDX-Net "Inst HQ 3"), rodando no próprio PC.
//!
//! O modelo trabalha no espectrograma: o áudio (44,1 kHz estéreo) é cortado em blocos, cada bloco vira uma
//! STFT (n_fft 6144, salto 1024, 3072 faixas de frequência × 256 quadros), o modelo devolve o espectrograma
//! do instrumental e a STFT inversa volta para o som. A voz é o original menos o instrumental.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ort::value::Tensor;
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

use crate::ai::{self, ModelSpec};
use crate::tools;
use crate::video;

pub const MODEL: ModelSpec = ModelSpec {
    file: "UVR-MDX-NET-Inst_HQ_3.onnx",
    url: "https://github.com/TRvlvr/model_repo/releases/download/all_public_uvr_models/UVR-MDX-NET-Inst_HQ_3.onnx",
    sha256: "317554b07fe1ea5279a77f2b1520a41ea4b93432560c4ffd08792c30fddf9adc",
    size: 66_759_214,
    label: "UVR MDX-Net (67 MB)",
};

const RATE: u32 = 44_100;
const N_FFT: usize = 6144;
const HOP: usize = 1024;
const DIM_F: usize = 3072;
const DIM_T: usize = 256;
const COMPENSATE: f32 = 1.022;

/// O que salvar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stems {
    Ambos,
    Voz,
    Instrumental,
}

impl Stems {
    pub const ALL: [Stems; 3] = [Stems::Ambos, Stems::Voz, Stems::Instrumental];
    pub fn label(self) -> &'static str {
        match self {
            Stems::Ambos => "Voz e instrumental",
            Stems::Voz => "Só a voz",
            Stems::Instrumental => "Só o instrumental (karaokê)",
        }
    }
}

fn hann() -> Vec<f32> {
    // janela de Hann "periódica", como a do torch.hann_window
    (0..N_FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos()).collect()
}

/// Lê o áudio como PCM float estéreo 44,1 kHz (dois canais separados).
fn read_pcm(src: &Path) -> Result<[Vec<f32>; 2], String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let out = tools::command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(src)
        .args(["-vn", "-f", "f32le", "-acodec", "pcm_f32le", "-ac", "2", "-ar", &RATE.to_string(), "pipe:1"])
        .output()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    if !out.status.success() || out.stdout.len() < 8 {
        return Err("o arquivo não tem áudio.".into());
    }
    let mut l = Vec::with_capacity(out.stdout.len() / 8);
    let mut r = Vec::with_capacity(out.stdout.len() / 8);
    for f in out.stdout.chunks_exact(8) {
        l.push(f32::from_le_bytes([f[0], f[1], f[2], f[3]]));
        r.push(f32::from_le_bytes([f[4], f[5], f[6], f[7]]));
    }
    Ok([l, r])
}

/// Grava PCM float estéreo no formato do arquivo de destino (pela extensão).
fn write_audio(dst: &Path, ch: &[Vec<f32>; 2]) -> Result<(), String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let mut bytes = Vec::with_capacity(ch[0].len() * 8);
    for i in 0..ch[0].len() {
        bytes.extend_from_slice(&ch[0][i].to_le_bytes());
        bytes.extend_from_slice(&ch[1][i].to_le_bytes());
    }
    let codec: &[&str] = match tools::ext_lower(dst).as_str() {
        "mp3" => &["-c:a", "libmp3lame", "-b:a", "320k"],
        "flac" => &["-c:a", "flac"],
        _ => &["-c:a", "pcm_s16le"],
    };
    let mut child = tools::command(&ff)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "f32le", "-ar", &RATE.to_string(), "-ac", "2", "-i", "pipe:0"])
        .args(codec)
        .arg(dst)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("não foi possível iniciar o ffmpeg: {e}"))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("falha ao salvar: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}

struct Stft {
    window: Vec<f32>,
    fwd: Arc<dyn realfft::RealToComplex<f32>>,
    inv: Arc<dyn realfft::ComplexToReal<f32>>,
}

impl Stft {
    fn new() -> Self {
        let mut p = RealFftPlanner::<f32>::new();
        Self { window: hann(), fwd: p.plan_fft_forward(N_FFT), inv: p.plan_fft_inverse(N_FFT) }
    }

    /// STFT "center" com espelhamento nas pontas (como torch.stft). Devolve [quadro][faixa].
    fn forward(&self, x: &[f32]) -> Vec<Vec<Complex<f32>>> {
        let half = N_FFT / 2;
        let n = x.len();
        let at = |i: isize| -> f32 {
            // espelhamento sem repetir a borda
            let mut i = i;
            if i < 0 {
                i = -i;
            }
            if i >= n as isize {
                i = 2 * (n as isize - 1) - i;
            }
            x[i.clamp(0, n as isize - 1) as usize]
        };
        let frames = 1 + n / HOP;
        let mut buf = self.fwd.make_input_vec();
        let mut spec = self.fwd.make_output_vec();
        let mut out = Vec::with_capacity(frames);
        for t in 0..frames {
            let start = (t * HOP) as isize - half as isize;
            for (k, b) in buf.iter_mut().enumerate() {
                *b = at(start + k as isize) * self.window[k];
            }
            self.fwd.process(&mut buf, &mut spec).expect("fft");
            out.push(spec.clone());
        }
        out
    }

    /// STFT inversa com sobreposição e normalização pela janela; devolve `len` amostras.
    fn inverse(&self, spec: &[Vec<Complex<f32>>], len: usize) -> Vec<f32> {
        let half = N_FFT / 2;
        let total = N_FFT + HOP * (spec.len() - 1);
        let mut y = vec![0f32; total];
        let mut wsum = vec![0f32; total];
        let mut buf = self.inv.make_output_vec();
        for (t, frame) in spec.iter().enumerate() {
            let mut f = frame.clone();
            // o realfft exige parte imaginária zero na faixa 0 e na de Nyquist
            f[0].im = 0.0;
            let last = f.len() - 1;
            f[last].im = 0.0;
            self.inv.process(&mut f, &mut buf).expect("ifft");
            let off = t * HOP;
            for k in 0..N_FFT {
                let w = self.window[k];
                y[off + k] += buf[k] / N_FFT as f32 * w;
                wsum[off + k] += w * w;
            }
        }
        (0..len).map(|i| {
            let j = i + half;
            if wsum[j] > 1e-8 { y[j] / wsum[j] } else { 0.0 }
        }).collect()
    }
}

/// Separa o arquivo e devolve (descrição, primeiro arquivo salvo).
pub fn separate(
    src: &Path,
    out_dir: Option<&Path>,
    stems: Stems,
    fmt_ext: &str,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    // 1. modelo (baixado na primeira vez: 0–10% do progresso)
    let model_path = ai::ensure_model(&MODEL, &|p| on_progress(p * 0.1), cancelled)?;
    let mut session = ai::session(&model_path)?;
    let mix = read_pcm(src)?;
    let n = mix[0].len();
    if n < RATE as usize / 2 {
        return Err("o áudio é curto demais (menos de meio segundo).".into());
    }
    let stft = Stft::new();
    let chunk = HOP * (DIM_T - 1);
    let trim = N_FFT / 2;
    let step_len = chunk - 2 * trim;
    let pad = step_len - n % step_len;
    let padded: [Vec<f32>; 2] = std::array::from_fn(|c| {
        let mut v = vec![0f32; trim];
        v.extend_from_slice(&mix[c]);
        v.extend(std::iter::repeat_n(0.0, pad + trim));
        v
    });
    let mut inst: [Vec<f32>; 2] = [Vec::with_capacity(n + pad), Vec::with_capacity(n + pad)];
    let steps = (n + pad) / step_len;
    for (s, i) in (0..n + pad).step_by(step_len).enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        // espectrograma dos 2 canais: [real e, imag e, real d, imag d] × 3072 faixas × 256 quadros
        let specs: Vec<Vec<Vec<Complex<f32>>>> = (0..2).map(|c| stft.forward(&padded[c][i..i + chunk])).collect();
        let mut data = vec![0f32; 4 * DIM_F * DIM_T];
        for c in 0..2 {
            for t in 0..DIM_T {
                for f in 0..DIM_F {
                    let v = specs[c][t][f];
                    data[((c * 2) * DIM_F + f) * DIM_T + t] = v.re;
                    data[((c * 2 + 1) * DIM_F + f) * DIM_T + t] = v.im;
                }
            }
        }
        let input = Tensor::from_array(([1usize, 4, DIM_F, DIM_T], data)).map_err(|e| e.to_string())?;
        let out = session.run(ort::inputs![input]).map_err(|e| e.to_string())?;
        let (_, v) = out[0].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        if v.len() != 4 * DIM_F * DIM_T {
            return Err("resposta inesperada do modelo.".into());
        }
        for c in 0..2 {
            let frames: Vec<Vec<Complex<f32>>> = (0..DIM_T)
                .map(|t| {
                    let mut fr = vec![Complex::new(0.0, 0.0); N_FFT / 2 + 1];
                    for f in 0..DIM_F {
                        fr[f] = Complex::new(v[((c * 2) * DIM_F + f) * DIM_T + t], v[((c * 2 + 1) * DIM_F + f) * DIM_T + t]);
                    }
                    fr
                })
                .collect();
            let wave = stft.inverse(&frames, chunk);
            inst[c].extend_from_slice(&wave[trim..chunk - trim]);
        }
        on_progress(0.1 + 0.85 * (s + 1) as f32 / steps as f32);
    }
    for ch in inst.iter_mut() {
        ch.truncate(n);
        for x in ch.iter_mut() {
            *x *= COMPENSATE;
        }
    }
    let vocals: [Vec<f32>; 2] = std::array::from_fn(|c| mix[c].iter().zip(&inst[c]).map(|(m, i)| m - i).collect());

    let ext = match fmt_ext {
        "mp3" | "flac" => fmt_ext,
        _ => "wav",
    };
    let mut saved = Vec::new();
    if stems != Stems::Instrumental {
        let dst = tools::out_path(src, out_dir, "_voz", ext);
        write_audio(&dst, &vocals)?;
        saved.push(dst);
    }
    if stems != Stems::Voz {
        let dst = tools::out_path(src, out_dir, "_instrumental", ext);
        write_audio(&dst, &inst)?;
        saved.push(dst);
    }
    on_progress(1.0);
    let names: Vec<String> = saved.iter().map(|p| tools::file_name(p)).collect();
    let dur = video::fmt_time(n as f64 / RATE as f64);
    Ok((format!("{} • {dur}", names.join(" + ")), saved.remove(0)))
}
