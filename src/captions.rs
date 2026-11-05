//! Legendas queimadas no vídeo, com ou sem animação (palavra por palavra, no estilo Reels/TikTok).
//! O Whisper dá o tempo de cada palavra; as frases podem ser corrigidas à mão (modo manual) ou vir de um .srt;
//! o Conver monta uma legenda ASS com o estilo escolhido e o ffmpeg a grava no vídeo.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::media_extra::{self, SubSize};
use crate::transcribe::{self, WModel, Word};
use crate::{tools, video};

/// Como a palavra falada aparece quando a legenda é animada.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Effect {
    /// A frase aparece inteira e a palavra falada muda de cor.
    Cor,
    /// A palavra falada ganha uma "caixa" colorida atrás.
    Caixa,
    /// As palavras vão aparecendo conforme são faladas.
    Revelar,
}

/// Aparência da legenda. Tamanhos são relativos ao vídeo, para o mesmo estilo servir em qualquer resolução.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// Destacar palavra por palavra (senão, frases paradas, como uma legenda comum).
    pub animated: bool,
    pub effect: Effect,
    /// "Pulinho" na palavra que começa a ser falada.
    pub pop: bool,
    pub font: String,
    /// Tamanho da letra, em % da altura de referência do vídeo.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub uppercase: bool,
    pub words_per_line: usize,
    pub text: [u8; 3],
    pub highlight: [u8; 3],
    pub outline_color: [u8; 3],
    /// Espessura do contorno, em % do tamanho da letra.
    pub outline: f32,
    /// Sombra, em % do tamanho da letra.
    pub shadow: f32,
    /// Faixa atrás do texto.
    pub background: bool,
    pub back_color: [u8; 3],
    /// Opacidade da faixa, 0 a 100.
    pub back_opacity: u8,
    /// Centro da legenda, em % da largura e da altura do vídeo.
    pub x: f32,
    pub y: f32,
}

impl Default for Style {
    fn default() -> Self {
        presets()[0].1.clone()
    }
}

pub const YELLOW: [u8; 3] = [0xFF, 0xE1, 0x00];
pub const GREEN: [u8; 3] = [0x6A, 0xE0, 0x47];
pub const RED: [u8; 3] = [0xF1, 0x30, 0x30];
pub const BLUE: [u8; 3] = [0x38, 0xB2, 0xFF];
pub const WHITE: [u8; 3] = [0xFF, 0xFF, 0xFF];
pub const DARK: [u8; 3] = [0x10, 0x10, 0x10];

// i18n: fora
/// Fontes oferecidas na tela (só as que existem em C:\Windows\Fonts aparecem).
pub const FONTS: &[(&str, &str)] = &[
    ("Segoe UI Black", "seguibl.ttf"),
    ("Segoe UI", "segoeui.ttf"),
    ("Segoe UI Semibold", "seguisb.ttf"),
    ("Arial", "arial.ttf"),
    ("Arial Black", "ariblk.ttf"),
    ("Impact", "impact.ttf"),
    ("Bahnschrift", "bahnschrift.ttf"),
    ("Verdana", "verdana.ttf"),
    ("Tahoma", "tahoma.ttf"),
    ("Trebuchet MS", "trebuc.ttf"),
    ("Calibri", "calibri.ttf"),
    ("Georgia", "georgia.ttf"),
    ("Comic Sans MS", "comic.ttf"),
    ("Consolas", "consola.ttf"),
];
const BASE_FONT: &str = "Segoe UI Black";
const PLAIN_FONT: &str = "Segoe UI Semibold";

pub fn installed_fonts() -> Vec<&'static str> {
    let dir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("Fonts");
    FONTS.iter().filter(|(_, f)| dir.join(f).is_file()).map(|(n, _)| *n).collect()
}
// i18n: dentro

/// Estilos prontos: (nome na tela, estilo).
pub fn presets() -> Vec<(&'static str, Style)> {
    let reels = Style {
        animated: true,
        effect: Effect::Cor,
        pop: true,
        font: BASE_FONT.into(),
        size: 6.2,
        bold: true,
        italic: false,
        uppercase: true,
        words_per_line: 3,
        text: WHITE,
        highlight: YELLOW,
        outline_color: DARK,
        outline: 10.0,
        shadow: 6.0,
        background: false,
        back_color: [0, 0, 0],
        back_opacity: 60,
        x: 50.0,
        y: 80.0,
    };
    vec![
        ("Reels", reels.clone()),
        ("Caixa colorida", Style { effect: Effect::Caixa, highlight: GREEN, ..reels.clone() }),
        ("Uma palavra por vez", Style { words_per_line: 1, size: 9.5, y: 50.0, ..reels.clone() }),
        ("Aparecendo", Style { effect: Effect::Revelar, words_per_line: 4, highlight: WHITE, ..reels.clone() }),
        (
            "Clássica",
            Style { animated: false, font: PLAIN_FONT.into(), size: 4.2, uppercase: false, words_per_line: 7, outline: 8.0, shadow: 4.0, y: 88.0, ..reels.clone() },
        ),
        (
            "Faixa escura",
            Style {
                animated: false,
                font: PLAIN_FONT.into(),
                size: 4.0,
                uppercase: false,
                words_per_line: 7,
                background: true,
                back_opacity: 65,
                y: 88.0,
                ..reels
            },
        ),
    ]
}

/// Uma frase da legenda: o texto que aparece entre `start` e `end` e, quando veio do Whisper, o tempo de cada palavra.
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub words: Vec<Word>,
}

/// Junta as palavras do Whisper em frases para revisar: quebra em pausas, em fim de frase e a cada ~10 palavras.
pub fn cues_from_words(words: &[Word]) -> Vec<Cue> {
    let mut groups: Vec<Vec<Word>> = Vec::new();
    let mut cur: Vec<Word> = Vec::new();
    for w in words.iter().filter(|w| !w.text.trim().is_empty()) {
        let pause = cur.last().is_some_and(|l| w.start - l.end > 0.6);
        let long = cur.len() >= 10 || cur.first().is_some_and(|f| w.end - f.start > 5.0);
        if !cur.is_empty() && (pause || long) {
            groups.push(std::mem::take(&mut cur));
        }
        cur.push(w.clone());
        if w.text.trim_end().ends_with(['.', '!', '?']) {
            groups.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    let starts: Vec<f64> = groups.iter().map(|g| g[0].start).collect();
    groups
        .into_iter()
        .enumerate()
        .map(|(i, g)| {
            let last = g.last().map_or(0.0, |w| w.end);
            // a frase fica um pouco na tela depois da última palavra, sem invadir a próxima
            let end = starts.get(i + 1).map_or(last + 0.35, |&n| (last + 0.35).min(n)).max(last);
            Cue { start: g[0].start, end, text: g.iter().map(|w| w.text.trim()).collect::<Vec<_>>().join(" "), words: g }
        })
        .collect()
}

/// O tempo de cada palavra do texto (talvez corrigido) de uma frase. Se o número de palavras não mudou, vale o tempo
/// do Whisper (ajustado ao início e ao fim da frase); senão, o tempo é repartido pelo tamanho de cada palavra.
pub fn cue_words(c: &Cue) -> Vec<Word> {
    let tokens: Vec<&str> = c.text.split_whitespace().collect();
    let (start, end) = (c.start, c.end.max(c.start + 0.05));
    if tokens.is_empty() {
        return Vec::new();
    }
    if tokens.len() == c.words.len() && !c.words.is_empty() {
        let (os, oe) = (c.words[0].start, c.words.last().unwrap().end);
        let span = (oe - os).max(0.01);
        // acompanha o início da frase; se ela ficou mais curta que a fala, as palavras se apertam
        let k = if end - start < span { (end - start) / span } else { 1.0 };
        let scale = |t: f64| start + (t - os) * k;
        let mut out: Vec<Word> = tokens.iter().zip(&c.words).map(|(t, w)| Word { start: scale(w.start), end: scale(w.end), text: t.to_string() }).collect();
        for w in &mut out {
            w.start = w.start.clamp(start, end);
            w.end = w.end.clamp(w.start, end);
        }
        return out;
    }
    let weights: Vec<f64> = tokens.iter().map(|t| t.chars().count() as f64 + 2.0).collect();
    let total: f64 = weights.iter().sum();
    let mut t = start;
    tokens
        .iter()
        .zip(weights)
        .map(|(tok, wgt)| {
            let d = (end - start) * wgt / total;
            let w = Word { start: t, end: t + d, text: tok.to_string() };
            t += d;
            w
        })
        .collect()
}

fn ts(t: f64) -> String {
    let cs = (t.max(0.0) * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

fn clean(w: &str, upper: bool) -> String {
    let w = w.trim().replace(['{', '}', '\\'], "");
    if upper { w.to_uppercase() } else { w }
}

/// Cor no formato do ASS (&HAABBGGRR), com alfa 0 = opaco.
fn ass_color([r, g, b]: [u8; 3], alpha: u8) -> String {
    format!("&H{alpha:02X}{b:02X}{g:02X}{r:02X}&")
}

// i18n: fora
/// Tamanho da letra em pixels do vídeo.
fn font_px(w: u32, h: u32, st: &Style) -> f32 {
    let base = h.min(w * 16 / 9) as f32; // em vídeos verticais, o tamanho acompanha a largura
    (base * st.size / 100.0).max(8.0)
}

fn header(w: u32, h: u32, st: &Style) -> String {
    let size = font_px(w, h, st);
    let (border_style, outline_col, outline, shadow) = if st.background {
        let alpha = 255 - (st.back_opacity.min(100) as u32 * 255 / 100) as u8;
        (3, ass_color(st.back_color, alpha), size * 0.16, 0.0)
    } else {
        (1, ass_color(st.outline_color, 0), size * st.outline / 100.0, size * st.shadow / 100.0)
    };
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n\
         [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
         Style: Default,{},{:.0},{},{},{},&H80000000,{},{},0,0,100,100,0,0,{border_style},{:.1},{:.1},5,{m},{m},0,1\n\n\
         [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        st.font.replace(',', " "),
        size,
        ass_color(st.text, 0),
        ass_color(st.text, 0),
        outline_col,
        if st.bold { -1 } else { 0 },
        if st.italic { -1 } else { 0 },
        outline,
        shadow,
        m = (w as f32 * 0.06) as u32,
    )
}

/// O texto de uma linha da legenda com a palavra `k` destacada (`None` = sem destaque, legenda parada).
fn line(tokens: &[String], k: Option<usize>, w: u32, h: u32, st: &Style) -> String {
    let size = font_px(w, h, st);
    let hl = ass_color(st.highlight, 0);
    let pop = if st.pop { "\\t(0,80,\\fscx112\\fscy112)\\t(80,160,\\fscx100\\fscy100)" } else { "" };
    let pos = format!("{{\\an5\\pos({:.0},{:.0})}}", w as f32 * st.x.clamp(0.0, 100.0) / 100.0, h as f32 * st.y.clamp(0.0, 100.0) / 100.0);
    let words: Vec<String> = tokens
        .iter()
        .enumerate()
        .map(|(j, t)| {
            let Some(k) = k else { return t.clone() };
            match (st.effect, j.cmp(&k)) {
                (Effect::Revelar, std::cmp::Ordering::Greater) => format!("{{\\alpha&HFF&}}{t}{{\\r}}"),
                (_, std::cmp::Ordering::Equal) => match st.effect {
                    Effect::Caixa if !st.background => format!("{{\\c{}\\3c{hl}\\bord{:.0}\\shad0{pop}}}{t}{{\\r}}", ass_color(DARK, 0), size * 0.22),
                    Effect::Caixa => format!("{{\\c{}\\3c{hl}{pop}}}{t}{{\\r}}", ass_color(DARK, 0)),
                    _ => format!("{{\\c{hl}{pop}}}{t}{{\\r}}"),
                },
                _ => t.clone(),
            }
        })
        .collect();
    format!("{pos}{}", words.join(" "))
}

/// Monta o arquivo .ass para um vídeo de `w`×`h`.
pub fn build_ass(cues: &[Cue], w: u32, h: u32, st: &Style) -> String {
    let mut s = header(w, h, st);
    let n = st.words_per_line.max(1);
    for c in cues {
        let words = cue_words(c);
        let chunks: Vec<&[Word]> = words.chunks(n).collect();
        for (gi, g) in chunks.iter().enumerate() {
            let g_end = chunks.get(gi + 1).map_or(c.end, |n| n[0].start).max(g[0].start + 0.05);
            let tokens: Vec<String> = g.iter().map(|x| clean(&x.text, st.uppercase)).collect();
            if !st.animated {
                s.push_str(&format!("Dialogue: 0,{},{},Default,,0,0,0,,{}\n", ts(g[0].start), ts(g_end), line(&tokens, None, w, h, st)));
                continue;
            }
            for k in 0..g.len() {
                // a palavra fica destacada até a próxima começar; a última, até o fim do grupo
                let end = g.get(k + 1).map_or(g_end, |n| n.start).max(g[k].start + 0.05);
                s.push_str(&format!("Dialogue: 0,{},{},Default,,0,0,0,,{}\n", ts(g[k].start), ts(end), line(&tokens, Some(k), w, h, st)));
            }
        }
    }
    s
}

/// A legenda da prévia: uma linha só, na tela o tempo todo.
fn preview_ass(tokens: &[String], k: Option<usize>, w: u32, h: u32, st: &Style) -> String {
    format!("{}Dialogue: 0,0:00:00.00,0:10:00.00,Default,,0,0,0,,{}\n", header(w, h, st), line(tokens, k, w, h, st))
}

/// Lê uma legenda .srt ou .vtt (o texto de cada bloco vira uma frase).
pub fn parse_subtitle(text: &str) -> Vec<Cue> {
    fn t(s: &str) -> Option<f64> {
        let s = s.trim().replace(',', ".");
        let parts: Vec<&str> = s.split(':').collect();
        let nums: Vec<f64> = parts.iter().map(|p| p.trim().parse::<f64>().ok()).collect::<Option<_>>()?;
        Some(match nums.len() {
            3 => nums[0] * 3600.0 + nums[1] * 60.0 + nums[2],
            2 => nums[0] * 60.0 + nums[1],
            _ => return None,
        })
    }
    let strip = |s: &str| {
        let mut out = String::new();
        let mut skip = None;
        for ch in s.chars() {
            match (skip, ch) {
                (None, '<') => skip = Some('>'),
                (None, '{') => skip = Some('}'),
                (Some(e), c) if c == e => skip = None,
                (None, c) => out.push(c),
                _ => {}
            }
        }
        out
    };
    let mut cues = Vec::new();
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(i) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let mut sides = lines[i].split("-->");
        let (Some(a), Some(b)) = (sides.next(), sides.next()) else { continue };
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (t(a), t(b)) else { continue };
        let body = lines[i + 1..].iter().map(|l| strip(l).trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        if !body.is_empty() {
            cues.push(Cue { start, end, text: body, words: Vec::new() });
        }
    }
    cues
}

/// A legenda em .srt (para salvar o que foi corrigido).
pub fn to_srt(cues: &[Cue]) -> String {
    let st = |t: f64| {
        let ms = (t.max(0.0) * 1000.0).round() as u64;
        format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
    };
    cues.iter().enumerate().map(|(i, c)| format!("{}\n{} --> {}\n{}\n\n", i + 1, st(c.start), st(c.end), c.text.trim())).collect()
}
// i18n: dentro

/// De onde vem o texto da legenda.
pub enum Source {
    /// Transcrever com o Whisper.
    Transcribe { model: WModel, lang: String },
    /// Frases já prontas (revisadas na tela ou lidas de um .srt).
    Cues(Vec<Cue>),
}

/// Grava a legenda no vídeo com o estilo escolhido. Também pode salvar o .ass ao lado.
#[allow(clippy::too_many_arguments)]
pub fn burn(
    src: &Path,
    out_dir: Option<&Path>,
    source: Source,
    st: &Style,
    keep_ass: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let info = video::probe(src);
    let (w, h) = info.size.ok_or("o arquivo não tem vídeo.")?;
    let (cues, burn_from) = match source {
        // 0–50%: palavras com o tempo de cada uma; 50–100%: gravar no vídeo
        Source::Transcribe { model, lang } => {
            let words = transcribe::word_timestamps(src, model, &lang, &|p| on_progress(p * 0.5), cancelled)?;
            (cues_from_words(&words), 0.5)
        }
        Source::Cues(c) => (c, 0.0),
    };
    let cues: Vec<Cue> = cues.into_iter().filter(|c| !c.text.trim().is_empty()).collect();
    if cues.is_empty() {
        return Err("nenhuma fala encontrada no vídeo.".into());
    }
    let words: usize = cues.iter().map(|c| c.text.split_whitespace().count()).sum();
    let ass = build_ass(&cues, w, h, st);
    let ass_path = tools::out_path(src, out_dir, "_legenda", "ass");
    std::fs::write(&ass_path, &ass).map_err(|e| format!("não foi possível salvar a legenda: {e}"))?;
    let r = media_extra::burn_subtitles(src, out_dir, &ass_path, SubSize::Media, false, &|p| on_progress(burn_from + p * (1.0 - burn_from)), cancelled);
    if !keep_ass {
        let _ = std::fs::remove_file(&ass_path);
    }
    let (_, dst) = r?;
    Ok((format!("{} palavras • {}", words, tools::human_size(tools::file_size(&dst))), dst))
}

/// Prévia de verdade: um quadro do vídeo (ou um fundo liso) com a legenda desenhada pelo mesmo ffmpeg/libass
/// que vai gravar o vídeo. Devolve o PNG.
pub fn render_preview(src: Option<&Path>, at: f64, size: (u32, u32), text: &str, highlight: Option<usize>, st: &Style, max_h: u32) -> Result<Vec<u8>, String> {
    let ff = tools::find_tool("ffmpeg").ok_or("ffmpeg não encontrado.")?;
    let (w, h) = size;
    let tokens: Vec<String> = text.split_whitespace().take(st.words_per_line.max(1)).map(|t| clean(t, st.uppercase)).collect();
    let k = if st.animated { highlight.map(|k| k.min(tokens.len().saturating_sub(1))) } else { None };
    let ass = preview_ass(&tokens, k, w, h, st);
    let tmp = std::env::temp_dir();
    let name = format!("conver_previa_{}.ass", std::process::id());
    std::fs::write(tmp.join(&name), ass).map_err(|e| e.to_string())?;
    let mut cmd = tools::command(&ff);
    cmd.current_dir(&tmp).args(["-v", "error", "-nostdin"]);
    match src {
        Some(s) => {
            cmd.args(["-ss", &format!("{:.2}", at.max(0.0))]).arg("-i").arg(s);
        }
        None => {
            cmd.args(["-f", "lavfi", "-i", &format!("color=c=0x2A333D:s={w}x{h}:d=1")]);
        }
    }
    let vf = format!("subtitles={name},scale=-2:{}", max_h.min(h).max(64) & !1);
    let out = cmd.args(["-frames:v", "1", "-vf", &vf, "-f", "image2pipe", "-c:v", "png", "-"]).output().map_err(|e| e.to_string());
    let _ = std::fs::remove_file(tmp.join(&name));
    let out = out?;
    if out.stdout.is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("ffmpeg falhou").to_string());
    }
    Ok(out.stdout)
}
