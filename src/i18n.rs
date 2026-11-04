//! Idiomas da interface. O texto-fonte é o português escrito no código; `tr()` traduz o que vai para a tela.
//!
//! As traduções ficam em `i18n/<código>.txt`: a linha N de cada arquivo traduz a linha N de `i18n/keys.txt`.
//! Textos com valores (`"{n} arquivos"`) são reconhecidos pelo molde e os valores são mantidos.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

pub struct Lang {
    pub code: &'static str,
    /// Nome no próprio idioma.
    pub name: &'static str,
    /// Nome em português.
    pub pt_name: &'static str,
    pub rtl: bool,
    data: &'static str,
}

macro_rules! lang {
    ($code:literal, $name:literal, $pt:literal, $rtl:literal) => {
        Lang { code: $code, name: $name, pt_name: $pt, rtl: $rtl, data: include_str!(concat!("../i18n/", $code, ".txt")) }
    };
}

/// Os 20 idiomas: os mais falados do mundo cuja escrita a interface mostra corretamente.
pub static LANGS: LazyLock<Vec<Lang>> = LazyLock::new(|| {
    vec![
        Lang { code: "pt", name: "Português", pt_name: "Português", rtl: false, data: "" },
        lang!("en", "English", "Inglês", false),
        lang!("zh", "中文（简体）", "Chinês", false),
        lang!("es", "Español", "Espanhol", false),
        lang!("ar", "العربية", "Árabe", true),
        lang!("fr", "Français", "Francês", false),
        lang!("ru", "Русский", "Russo", false),
        lang!("id", "Bahasa Indonesia", "Indonésio", false),
        lang!("ur", "اردو", "Urdu", true),
        lang!("de", "Deutsch", "Alemão", false),
        lang!("ja", "日本語", "Japonês", false),
        lang!("vi", "Tiếng Việt", "Vietnamita", false),
        lang!("tr", "Türkçe", "Turco", false),
        lang!("ha", "Hausa", "Hauçá", false),
        lang!("ko", "한국어", "Coreano", false),
        lang!("it", "Italiano", "Italiano", false),
        lang!("fa", "فارسی", "Persa", true),
        lang!("sw", "Kiswahili", "Suaíli", false),
        lang!("pl", "Polski", "Polonês", false),
        lang!("uk", "Українська", "Ucraniano", false),
    ]
});

const KEYS: &str = include_str!("../i18n/keys.txt");

static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\t", "\t")
}

/// Uma parte de um molde: texto fixo ou lugar de um valor (`{n}`, `{}`, `{pct:.0}`).
#[derive(Debug, PartialEq)]
enum Part {
    Text(String),
    Slot(String),
}

fn parse(template: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(a) = rest.find('{') {
        let Some(b) = rest[a..].find('}').map(|b| a + b) else { break };
        if a > 0 {
            parts.push(Part::Text(rest[..a].to_string()));
        }
        parts.push(Part::Slot(rest[a..=b].to_string()));
        rest = &rest[b + 1..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest.to_string()));
    }
    parts
}

struct Template {
    parts: Vec<Part>,
    translated: String,
    /// Tamanho do texto fixo: moldes mais específicos são testados primeiro.
    weight: usize,
}

impl Template {
    /// Confere se `s` segue o molde; devolve os valores de cada lugar.
    fn capture(&self, s: &str) -> Option<Vec<(String, String)>> {
        let mut pos = 0;
        let mut out = Vec::new();
        let mut pending: Option<&str> = None;
        let n = self.parts.len();
        for (i, part) in self.parts.iter().enumerate() {
            match part {
                Part::Slot(name) => pending = Some(name),
                Part::Text(t) => {
                    let found = if i == n - 1 {
                        // último pedaço fixo: precisa terminar a frase
                        s.len().checked_sub(t.len()).filter(|&at| at >= pos && s.is_char_boundary(at) && s[at..] == **t)
                    } else if pending.is_none() {
                        s[pos..].starts_with(t.as_str()).then_some(pos)
                    } else {
                        s[pos..].find(t.as_str()).map(|i| pos + i)
                    }?;
                    if let Some(name) = pending.take() {
                        if found == pos {
                            return None; // valor vazio
                        }
                        // valor no começo (nome de arquivo, número) não engole uma mensagem composta ("a.mp4: …")
                        if out.is_empty() && i == 1 && SEPARATORS.iter().any(|sep| s[pos..found].contains(sep)) {
                            return None;
                        }
                        out.push((name.to_string(), s[pos..found].to_string()));
                    } else if found != pos {
                        return None;
                    }
                    pos = found + t.len();
                }
            }
        }
        match pending {
            Some(name) if pos < s.len() => out.push((name.to_string(), s[pos..].to_string())),
            Some(_) => return None,
            None if pos != s.len() => return None,
            None => {}
        }
        Some(out)
    }
}

struct Table {
    exact: HashMap<String, String>,
    templates: Vec<Template>,
    memo: Mutex<HashMap<String, String>>,
}

fn letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphabetic()).count()
}

/// Linhas "N|tradução" do arquivo do idioma, ligadas à linha N de keys.txt.
fn pairs(lang: &Lang) -> Vec<(&'static str, &'static str)> {
    let keys: Vec<&'static str> = KEYS.lines().collect();
    lang.data
        .trim_start_matches('\u{feff}')
        .lines()
        .filter_map(|l| {
            let (n, text) = l.split_once('|')?;
            let key = keys.get(n.trim().parse::<usize>().ok()?.checked_sub(1)?)?;
            Some((*key, text))
        })
        .collect()
}

fn build(lang: &Lang) -> Table {
    let mut exact = HashMap::new();
    let mut templates = Vec::new();
    for (key, value) in pairs(lang) {
        let (key, value) = (unescape(key), unescape(value));
        if value.trim().is_empty() {
            continue;
        }
        let parts = parse(&key);
        if parts.iter().any(|p| matches!(p, Part::Slot(_))) {
            let fixed: String = parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect();
            // moldes sem palavras ("{a} • {b}") pegariam qualquer coisa
            if letters(&fixed) >= 3 {
                templates.push(Template { parts, translated: value, weight: fixed.len() });
            }
        } else {
            exact.insert(key, value);
        }
    }
    templates.sort_by(|a, b| b.weight.cmp(&a.weight));
    Table { exact, templates, memo: Mutex::new(HashMap::new()) }
}

fn table(i: usize) -> &'static Table {
    static TABLES: LazyLock<Vec<OnceLock<Table>>> = LazyLock::new(|| LANGS.iter().map(|_| OnceLock::new()).collect());
    TABLES[i].get_or_init(|| build(&LANGS[i]))
}

pub fn current() -> &'static Lang {
    &LANGS[CURRENT.load(Ordering::Relaxed)]
}

pub fn set(code: &str) {
    let i = LANGS.iter().position(|l| l.code == code).unwrap_or(0);
    CURRENT.store(i, Ordering::Relaxed);
}

/// Símbolos que abrem mensagens de log ("✔ ", "✖ ", "⚠ "): são mantidos e o resto é traduzido.
const PREFIXES: [&str; 6] = ["✔ ", "✖ ", "⚠ ", "●  ", "■  ", "→ "];
/// Separadores de mensagens compostas ("nome   info", "a • b").
const SEPARATORS: [&str; 4] = ["   ", "  •  ", " • ", ": "];

fn translate(t: &Table, s: &str, depth: u8) -> String {
    if s.trim().is_empty() || letters(s) == 0 {
        return s.to_string();
    }
    if let Some(v) = t.exact.get(s) {
        return v.clone();
    }
    if depth > 3 {
        return s.to_string();
    }
    for p in PREFIXES {
        if let Some(rest) = s.strip_prefix(p) {
            return format!("{p}{}", translate(t, rest, depth + 1));
        }
    }
    for tpl in &t.templates {
        if let Some(values) = tpl.capture(s) {
            let mut out = tpl.translated.clone();
            for (name, value) in values {
                out = out.replacen(&name, &translate(t, &value, depth + 1), 1);
            }
            return out;
        }
    }
    for sep in SEPARATORS {
        if s.contains(sep) {
            return s.split(sep).map(|part| translate(t, part, depth + 1)).collect::<Vec<_>>().join(sep);
        }
    }
    s.to_string()
}

fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => c,
    }
}

/// Escrita da direita para a esquerda: junta as letras árabes e põe na ordem de exibição.
fn visual_rtl(s: &str) -> String {
    use unicode_bidi::{BidiInfo, Level};
    s.split('\n')
        .map(|line| {
            // depois de juntar as letras, as marcas invisíveis (ZWNJ do persa, RLM/LRM) não servem mais
            // e a interface as desenharia como quadradinhos
            let shaped: String =
                ar_reshaper::reshape_line(line).chars().filter(|c| !matches!(c, '\u{200C}' | '\u{200E}' | '\u{200F}')).collect();
            let bidi = BidiInfo::new(&shaped, Some(Level::rtl()));
            let mut out = String::new();
            for p in &bidi.paragraphs {
                let (levels, runs) = bidi.visual_runs(p, p.range.clone());
                for run in runs {
                    let text = &shaped[run.clone()];
                    if levels[run.start].is_rtl() {
                        // trechos da direita para a esquerda: ordem invertida e parênteses espelhados
                        out.extend(text.chars().rev().map(mirror));
                    } else {
                        out.push_str(text);
                    }
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Traduz um texto da interface para o idioma escolhido.
pub fn tr<S: AsRef<str>>(s: S) -> String {
    let s = s.as_ref();
    let i = CURRENT.load(Ordering::Relaxed);
    if i == 0 {
        return s.to_string();
    }
    let t = table(i);
    if let Some(v) = t.memo.lock().unwrap().get(s) {
        return v.clone();
    }
    let mut out = translate(t, s, 0);
    if LANGS[i].rtl && out.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)) {
        out = visual_rtl(&out);
    }
    let mut memo = t.memo.lock().unwrap();
    if memo.len() > 20_000 {
        memo.clear(); // nomes de arquivo não podem crescer a memória para sempre
    }
    memo.insert(s.to_string(), out.clone());
    out
}

/// Idioma do Windows, se for um dos disponíveis.
pub fn system_default() -> &'static str {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        // SAFETY: só lê o idioma da interface do Windows.
        let primary = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
        let code = match primary {
            0x16 => "pt",
            0x09 => "en",
            0x04 => "zh",
            0x0a => "es",
            0x01 => "ar",
            0x0c => "fr",
            0x19 => "ru",
            0x21 => "id",
            0x20 => "ur",
            0x07 => "de",
            0x11 => "ja",
            0x2a => "vi",
            0x1f => "tr",
            0x68 => "ha",
            0x12 => "ko",
            0x10 => "it",
            0x29 => "fa",
            0x41 => "sw",
            0x15 => "pl",
            0x22 => "uk",
            _ => "en",
        };
        return code;
    }
    #[allow(unreachable_code)]
    "pt"
}

/// Confere os arquivos de tradução: todas as linhas traduzidas e com os mesmos lugares de valores.
#[cfg(test)]
pub fn check_files() -> Vec<String> {
    let keys: Vec<&str> = KEYS.lines().collect();
    let mut problems = Vec::new();
    for lang in LANGS.iter().skip(1) {
        let found = pairs(lang);
        let have: std::collections::HashSet<&str> = found.iter().map(|(k, _)| *k).collect();
        // linhas sem palavras ("{w}x{h} → {nw}x{nh}", "320 kbps") podem ficar de fora: valem iguais
        let needs = |k: &str| {
            let fixed: String = parse(k).into_iter().filter_map(|p| if let Part::Text(t) = p { Some(t) } else { None }).collect();
            let words = fixed.replace("kbps", "").replace("px", "").replace("bytes", "").replace("fps", "");
            letters(&words) >= 3
        };
        let missing: Vec<usize> =
            keys.iter().enumerate().filter(|(_, k)| !have.contains(*k) && needs(k)).map(|(i, _)| i + 1).collect();
        if !missing.is_empty() {
            problems.push(format!("{}: faltam as linhas {missing:?}", lang.code));
        }
        for (n, (k, v)) in found.iter().enumerate() {
            let slots = |s: &str| {
                let mut v: Vec<String> =
                    parse(s).into_iter().filter_map(|p| if let Part::Slot(x) = p { Some(x) } else { None }).collect();
                v.sort();
                v
            };
            if slots(k) != slots(v) {
                problems.push(format!("{} linha {}: valores diferentes em {k:?} -> {v:?}", lang.code, n + 1));
            }
            if v.trim().is_empty() {
                problems.push(format!("{} linha {}: vazia ({k:?})", lang.code, n + 1));
            }
        }
    }
    problems
}
