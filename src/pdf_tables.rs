//! Tabelas do PDF para Excel: cada página é desenhada e lida pelo OCR do Windows (funciona com PDFs digitais e
//! escaneados); as palavras viram linhas pela altura, células pelo espaço entre elas e colunas pelo alinhamento das
//! células de todas as linhas. Números no formato brasileiro ou americano viram números de verdade na planilha.

use std::path::{Path, PathBuf};

use crate::ocr::Word;
use crate::tools;

/// Uma célula: o texto e onde ela começa e termina (em pixels).
#[derive(Clone, Debug)]
struct Cell {
    text: String,
    x0: f32,
    x1: f32,
}

/// Uma página como grade: linhas de células já nas colunas.
pub struct Grid {
    pub page: u32,
    pub rows: Vec<Vec<String>>,
}

/// Linhas visuais: palavras cujo centro vertical está perto (o OCR às vezes junta ou separa linhas de tabela).
fn rows_of(words: Vec<Word>) -> Vec<Vec<Word>> {
    let mut words: Vec<Word> = words.into_iter().filter(|w| !w.text.trim().is_empty()).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut hs: Vec<f32> = words.iter().map(|w| w.rect.3).collect();
    hs.sort_by(f32::total_cmp);
    let mh = hs[hs.len() / 2].max(1.0);
    words.sort_by(|a, b| (a.rect.1 + a.rect.3 / 2.0).total_cmp(&(b.rect.1 + b.rect.3 / 2.0)));
    let mut rows: Vec<Vec<Word>> = Vec::new();
    for w in words {
        let cy = w.rect.1 + w.rect.3 / 2.0;
        match rows.last_mut() {
            Some(r) if (cy - r.iter().map(|x| x.rect.1 + x.rect.3 / 2.0).sum::<f32>() / r.len() as f32).abs() < mh * 0.55 => r.push(w),
            _ => rows.push(vec![w]),
        }
    }
    for r in &mut rows {
        r.sort_by(|a, b| a.rect.0.total_cmp(&b.rect.0));
    }
    rows
}

/// Células de uma linha: palavras separadas por um espaço maior que o normal ficam em células diferentes.
fn cells_of(row: &[Word], mh: f32) -> Vec<Cell> {
    let mut cells: Vec<Cell> = Vec::new();
    for w in row {
        let (x0, x1) = (w.rect.0, w.rect.0 + w.rect.2);
        match cells.last_mut() {
            Some(c) if x0 - c.x1 < mh * 0.9 => {
                c.text.push(' ');
                c.text.push_str(w.text.trim());
                c.x1 = x1;
            }
            _ => cells.push(Cell { text: w.text.trim().to_string(), x0, x1 }),
        }
    }
    cells
}

/// Junta as linhas da página numa grade. `only_tables`: só as linhas com 2+ células (o texto corrido fica de fora).
pub fn grid(lines: Vec<Vec<Word>>, page: u32, only_tables: bool) -> Grid {
    let words: Vec<Word> = lines.into_iter().flatten().collect();
    let mut hs: Vec<f32> = words.iter().map(|w| w.rect.3).collect();
    hs.sort_by(f32::total_cmp);
    let mh = hs.get(hs.len() / 2).copied().unwrap_or(10.0).max(1.0);
    let rows: Vec<Vec<Cell>> = rows_of(words).iter().map(|r| cells_of(r, mh)).collect();
    // colunas: as faixas horizontais ocupadas pelas células das linhas de tabela, juntando as que se sobrepõem
    let mut spans: Vec<(f32, f32)> = rows.iter().filter(|r| r.len() >= 2).flatten().map(|c| (c.x0, c.x1)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cols: Vec<(f32, f32)> = Vec::new();
    for (a, b) in spans {
        match cols.last_mut() {
            Some(c) if a <= c.1 + mh * 0.3 => c.1 = c.1.max(b),
            _ => cols.push((a, b)),
        }
    }
    let col_of = |c: &Cell| -> usize {
        // a coluna com mais sobreposição; se nenhuma, a mais próxima
        let mut best = (0usize, f32::MIN);
        for (i, (a, b)) in cols.iter().enumerate() {
            let overlap = c.x1.min(*b) - c.x0.max(*a);
            let score = if overlap > 0.0 { overlap } else { -((c.x0 - b).max(a - c.x1)).abs() };
            if score > best.1 {
                best = (i, score);
            }
        }
        best.0
    };
    let n = cols.len().max(1);
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut in_table = false;
    for r in rows {
        let is_table = r.len() >= 2;
        if only_tables && !is_table {
            // uma linha vazia separa uma tabela da próxima
            if in_table {
                out.push(Vec::new());
            }
            in_table = false;
            continue;
        }
        in_table = is_table;
        let mut line = vec![String::new(); n];
        for c in &r {
            let i = if cols.is_empty() { 0 } else { col_of(c) };
            if !line[i].is_empty() {
                line[i].push(' ');
            }
            line[i].push_str(&c.text);
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.iter().all(String::is_empty)) {
        out.pop();
    }
    Grid { page, rows: out }
}

/// Número escrito no formato brasileiro ("1.234,56") ou americano ("1,234.56"), com R$, $, % ou espaços.
pub fn parse_number(s: &str) -> Option<f64> {
    let t: String = s.trim().trim_start_matches("R$").trim_start_matches('$').trim_end_matches('%').chars().filter(|c| !c.is_whitespace()).collect();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')) || !t.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, t),
    };
    let (dots, commas) = (t.matches('.').count(), t.matches(',').count());
    let plain = match (dots, commas) {
        (0, 0) => t.clone(),
        // os dois: o último é o decimal
        (_, _) if dots > 0 && commas > 0 => {
            if t.rfind(',') > t.rfind('.') { t.replace('.', "").replace(',', ".") } else { t.replace(',', "") }
        }
        (0, 1) => {
            // "1,234" seria milhar americano, mas no Brasil é decimal: fica com o decimal
            t.replace(',', ".")
        }
        (0, _) => t.replace(',', ""),
        (1, 0) => {
            // "1.234" (milhar) ou "3.5" (decimal): exatamente 3 dígitos depois = milhar
            let after = t.split('.').nth(1).unwrap_or("");
            if after.len() == 3 { t.replace('.', "") } else { t.clone() }
        }
        _ => t.replace('.', ""),
    };
    let v: f64 = plain.parse().ok()?;
    Some(if neg { -v } else { v })
}

/// Grades de todas as páginas do PDF.
pub fn read_grids(src: &Path, lang: &str, only_tables: bool, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Vec<Grid>, String> {
    let total = crate::pdf::page_count(src).unwrap_or(1).max(1) as f32;
    let mut grids = Vec::new();
    crate::pdf_extra::ocr_words(
        src,
        lang,
        |page, lines, _| {
            on_progress(page as f32 / total);
            grids.push(grid(lines, page, only_tables));
            true
        },
        cancelled,
    )?;
    Ok(grids)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    Xlsx,
    Csv,
}

/// Lê as tabelas do PDF e salva em .xlsx (uma aba por página) ou .csv (separado por ";", páginas em sequência).
pub fn export(src: &Path, out_dir: Option<&Path>, fmt: Out, only_tables: bool, lang: &str, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let grids = read_grids(src, lang, only_tables, &|p| on_progress(p * 0.95), cancelled)?;
    let rows: usize = grids.iter().map(|g| g.rows.iter().filter(|r| !r.iter().all(String::is_empty)).count()).sum();
    if rows == 0 {
        return Err(if only_tables { "nenhuma tabela encontrada no PDF.".into() } else { "nenhum texto encontrado no PDF.".into() });
    }
    let dst = match fmt {
        Out::Xlsx => {
            let dst = tools::out_path(src, out_dir, "_tabelas", "xlsx");
            write_xlsx(&grids, &dst)?;
            dst
        }
        Out::Csv => {
            let dst = tools::out_path(src, out_dir, "_tabelas", "csv");
            let mut text = String::from("\u{feff}");
            for g in &grids {
                for r in &g.rows {
                    let line: Vec<String> = r.iter().map(|c| if c.contains([';', '"', '\n']) { format!("\"{}\"", c.replace('"', "\"\"")) } else { c.clone() }).collect();
                    text.push_str(&line.join(";"));
                    text.push_str("\r\n");
                }
            }
            std::fs::write(&dst, text).map_err(|e| format!("não foi possível salvar: {e}"))?;
            dst
        }
    };
    on_progress(1.0);
    Ok((format!("{rows} linha(s) em {} página(s)", grids.iter().filter(|g| !g.rows.is_empty()).count()), dst))
}

// i18n: fora
fn write_xlsx(grids: &[Grid], dst: &Path) -> Result<(), String> {
    use rust_xlsxwriter::{Format, Workbook};
    let mut wb = Workbook::new();
    let num = Format::new().set_num_format("#,##0.##");
    for g in grids.iter().filter(|g| !g.rows.is_empty()) {
        let ws = wb.add_worksheet();
        ws.set_name(format!("Página {}", g.page)).map_err(|e| e.to_string())?;
        let mut widths: Vec<usize> = Vec::new();
        for (r, row) in g.rows.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                if cell.is_empty() {
                    continue;
                }
                let (r, c16) = (r as u32, c as u16);
                match parse_number(cell) {
                    Some(v) => ws.write_number_with_format(r, c16, v, &num).map(|_| ()),
                    None => ws.write_string(r, c16, cell).map(|_| ()),
                }
                .map_err(|e| e.to_string())?;
                if widths.len() <= c {
                    widths.resize(c + 1, 0);
                }
                widths[c] = widths[c].max(cell.chars().count());
            }
        }
        for (c, w) in widths.iter().enumerate() {
            ws.set_column_width(c as u16, (*w as f64 + 2.0).clamp(6.0, 60.0)).map_err(|e| e.to_string())?;
        }
    }
    wb.save(dst).map_err(|e| format!("não foi possível salvar a planilha: {e}"))
}
// i18n: dentro
