//! Som curto ao soltar arquivos na janela: um "plop" suave, gerado aqui mesmo (nenhum arquivo de som no pacote) e
//! tocado pelo Windows sem travar a interface.

use std::sync::OnceLock;

// i18n: fora

const RATE: u32 = 44_100;

/// WAV mono de 16 bits com o som: duas notas curtas que sobem (dó e sol), com ataque rápido e queda suave.
fn drop_wav() -> &'static [u8] {
    static WAV: OnceLock<Vec<u8>> = OnceLock::new();
    WAV.get_or_init(|| {
        let total = (RATE as f32 * 0.22) as usize;
        let mut samples = vec![0f32; total];
        for (start, freq, len, gain) in [(0.0f32, 523.25f32, 0.16f32, 0.55f32), (0.055, 783.99, 0.16, 0.45)] {
            let s0 = (start * RATE as f32) as usize;
            let n = (len * RATE as f32) as usize;
            for i in 0..n.min(total - s0) {
                let t = i as f32 / RATE as f32;
                // ataque de 4 ms e queda exponencial; um leve deslize de afinação para cima deixa o som "redondo"
                let env = (t / 0.004).min(1.0) * (-t * 28.0).exp();
                let f = freq * (1.0 + 0.06 * (1.0 - (-t * 60.0).exp()));
                let tone = (std::f32::consts::TAU * f * t).sin() + 0.25 * (std::f32::consts::TAU * 2.0 * f * t).sin();
                samples[s0 + i] += tone * env * gain;
            }
        }
        let data: Vec<u8> = samples.iter().flat_map(|s| (((s * 0.5).clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        let mut wav = Vec::with_capacity(44 + data.len());
        wav.extend(b"RIFF");
        wav.extend((36 + data.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes()); // PCM
        wav.extend(1u16.to_le_bytes()); // mono
        wav.extend(RATE.to_le_bytes());
        wav.extend((RATE * 2).to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((data.len() as u32).to_le_bytes());
        wav.extend(data);
        wav
    })
}

/// Toca o som de "arquivo solto" (sem esperar terminar).
pub fn play_drop() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    let wav = drop_wav();
    // SND_MEMORY: o primeiro argumento é o ponteiro para o WAV; ele vive até o fim do programa (OnceLock)
    unsafe {
        let _ = PlaySoundW(windows::core::PCWSTR(wav.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
    }
}

#[cfg(test)]
pub(crate) fn drop_wav_for_test() -> &'static [u8] {
    drop_wav()
}
// i18n: dentro
