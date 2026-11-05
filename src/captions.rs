//! Legendas animadas no estilo Reels/TikTok: o Whisper dá o tempo de cada palavra, o Conver monta uma legenda ASS
//! em que a palavra falada fica destacada (com um leve "pulo") e o ffmpeg a queima no vídeo.

use std::path::{Path, PathBuf};

use crate::media_extra::{self, SubSize};
use crate::transcribe::{self, WModel, Word};
use crate::{tools, video};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// A frase aparece inteira e a palavra falada muda de cor.
    Destaque,
    /// A palavra falada ganha uma "caixa" colorida atrás.
    Caixa,
    /// Só uma palavra por vez, grande, no centro.
    UmaPalavra,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Baixo,
    Meio,
    Cima,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tint {
    Amarelo,
    Verde,
    Cinabrio,
    Azul,
}

impl Tint {
    /// Cor no formato do ASS (&HBBGGRR).
    fn ass(self) -> &'static str {
        match self {
            Tint::Amarelo => "&H0000E1FF&",
            Tint::Verde => "&H0047E06A&",
            Tint::Cinabrio => "&H003030F1&",
            Tint::Azul => "&H00FFB238&",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    pub look: Look,
    pub place: Place,
    pub tint: Tint,
    pub words_per_line: usize,
    pub uppercase: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style { look: Look::Destaque, place: Place::Baixo, tint: Tint::Amarelo, words_per_line: 3, uppercase: true }
    }
}

fn ts(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

fn clean(w: &str, upper: bool) -> String {
    let w = w.trim().replace(['{', '}', '\\'], "");
    if upper { w.to_uppercase() } else { w }
}

/// Junta as palavras em grupos (frases curtas): no máximo `n` palavras, quebrando em pausas e pontuação.
fn groups(words: &[Word], n: usize) -> Vec<Vec<&Word>> {
    let mut out: Vec<Vec<&Word>> = Vec::new();
    let mut cur: Vec<&Word> = Vec::new();
    for w in words.iter().filter(|w| !w.text.trim().is_empty()) {
        let pause = cur.last().is_some_and(|l| w.start - l.end > 0.6);
        if !cur.is_empty() && (cur.len() >= n.max(1) || pause) {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(w);
        if w.text.trim_end().ends_with(['.', '!', '?', ';']) {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Monta o arquivo .ass para um vídeo de `w`×`h`.
// i18n: fora
pub fn build_ass(words: &[Word], w: u32, h: u32, st: Style) -> String {
    let base = h.min(w * 16 / 9) as f32; // em vídeos verticais, o tamanho acompanha a largura
    let (size, outline) = match st.look {
        Look::UmaPalavra => (base * 0.095, base * 0.008),
        _ => (base * 0.062, base * 0.006),
    };
    let (align, margin) = match st.place {
        Place::Baixo => (2, (h as f32 * 0.12) as u32),
        Place::Meio => (5, 0),
        Place::Cima => (8, (h as f32 * 0.10) as u32),
    };
    let hl = st.tint.ass();
    let mut s = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,Segoe UI Black,{:.0},&H00FFFFFF,&H00FFFFFF,&H00101010,&H80000000,-1,0,0,0,100,100,0,0,1,{:.1},{:.1},{align},{},{},{margin},1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        size,
        outline,
        outline * 0.6,
        (w as f32 * 0.06) as u32,
        (w as f32 * 0.06) as u32
    );
    let pop = "\\t(0,80,\\fscx112\\fscy112)\\t(80,160,\\fscx100\\fscy100)";
    let gs = groups(words, if st.look == Look::UmaPalavra { 1 } else { st.words_per_line });
    for (gi, g) in gs.iter().enumerate() {
        let next_start = gs.get(gi + 1).and_then(|n| n.first()).map(|w| w.start);
        for (k, word) in g.iter().enumerate() {
            let start = word.start;
            // a palavra fica destacada até a próxima começar (a última segura um pouco, sem invadir a próxima frase)
            let end = match g.get(k + 1) {
                Some(n) => n.start,
                None => {
                    let hold = word.end + 0.35;
                    next_start.map_or(hold, |ns| hold.min(ns))
                }
            }
            .max(start + 0.05);
            let text: Vec<String> = g
                .iter()
                .enumerate()
                .map(|(j, x)| {
                    let t = clean(&x.text, st.uppercase);
                    if j != k {
                        return t;
                    }
                    match st.look {
                        Look::Destaque => format!("{{\\c{hl}{pop}}}{t}{{\\r}}"),
                        Look::Caixa => format!("{{\\c&H00101010&\\3c{hl}\\bord{:.0}{pop}}}{t}{{\\r}}", size * 0.22),
                        Look::UmaPalavra => format!("{{\\c{hl}{pop}}}{t}"),
                    }
                })
                .collect();
            s.push_str(&format!("Dialogue: 0,{},{},Default,,0,0,0,,{}\n", ts(start), ts(end), text.join(" ")));
        }
    }
    s
}

// i18n: dentro
/// Transcreve o vídeo palavra por palavra e queima as legendas animadas. Também salva o .ass ao lado.
#[allow(clippy::too_many_arguments)]
pub fn animated(
    src: &Path,
    out_dir: Option<&Path>,
    model: WModel,
    lang: &str,
    st: Style,
    keep_ass: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = video::probe(src);
    let (w, h) = info.size.ok_or("o arquivo não tem vídeo.")?;
    // 0–50%: palavras com o tempo de cada uma; 50–100%: queimar no vídeo
    let words = transcribe::word_timestamps(src, model, lang, &|p| on_progress(p * 0.5), cancelled)?;
    if words.is_empty() {
        return Err("nenhuma fala encontrada no vídeo.".into());
    }
    let ass = build_ass(&words, w, h, st);
    let ass_path = tools::out_path(src, out_dir, "_legendas_animadas", "ass");
    std::fs::write(&ass_path, &ass).map_err(|e| format!("não foi possível salvar a legenda: {e}"))?;
    let r = media_extra::burn_subtitles(src, out_dir, &ass_path, SubSize::Media, false, &|p| on_progress(0.5 + p * 0.5), cancelled);
    if !keep_ass {
        let _ = std::fs::remove_file(&ass_path);
    }
    let (_, dst) = r?;
    // o queimar salva como "_legendado": renomeia para deixar claro que são as animadas
    let final_dst = tools::out_path(src, out_dir, "_legendas_animadas", &tools::ext_lower(&dst));
    let final_dst = if std::fs::rename(&dst, &final_dst).is_ok() { final_dst } else { dst };
    Ok((format!("{} palavras • {}", words.len(), tools::human_size(tools::file_size(&final_dst))), final_dst))
}
