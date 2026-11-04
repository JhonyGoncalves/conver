//! Juntar e dividir PDFs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::tools;

fn open(path: &Path) -> Result<Document, String> {
    let doc = Document::load(path).map_err(|e| format!("{}: não foi possível abrir o PDF ({e})", tools::file_name(path)))?;
    if doc.is_encrypted() {
        return Err(format!("{} está protegido por senha.", tools::file_name(path)));
    }
    Ok(doc)
}

pub fn page_count(path: &Path) -> Option<usize> {
    Document::load(path).ok().map(|d| d.get_pages().len())
}

/// Atributos que a página pode herdar da árvore de páginas; são copiados para a própria página
/// antes de ela mudar de "pai".
const INHERITED: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

fn with_inherited(doc: &Document, page: ObjectId) -> Option<Dictionary> {
    let mut dict = doc.get_dictionary(page).ok()?.clone();
    let mut parent = dict.get(b"Parent").and_then(Object::as_reference).ok();
    let mut guard = 0;
    while let (Some(pid), true) = (parent, guard < 32) {
        guard += 1;
        let Ok(p) = doc.get_dictionary(pid) else { break };
        for key in INHERITED {
            if !dict.has(key) {
                if let Ok(v) = p.get(key) {
                    dict.set(key.to_vec(), v.clone());
                }
            }
        }
        parent = p.get(b"Parent").and_then(Object::as_reference).ok();
    }
    Some(dict)
}

/// Junta os PDFs, na ordem dada, em `dst`. Devolve o número de páginas.
pub fn merge(files: &[PathBuf], dst: &Path, cancelled: &dyn Fn() -> bool) -> Result<usize, String> {
    if files.len() < 2 {
        return Err("adicione pelo menos 2 PDFs.".into());
    }
    let mut max_id = 1;
    let mut pages: Vec<(ObjectId, Dictionary)> = Vec::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    for f in files {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        let mut doc = open(f)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        for id in doc.get_pages().into_values() {
            if let Some(d) = with_inherited(&doc, id) {
                pages.push((id, d));
            }
        }
        objects.extend(doc.objects);
    }

    let mut out = Document::with_version("1.5");
    out.max_id = max_id;
    let pages_id = out.new_object_id();
    for (id, obj) in objects {
        // árvores de páginas, catálogos e marcadores antigos são substituídos pelos novos
        match obj.type_name().unwrap_or(b"") {
            b"Catalog" | b"Pages" | b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                out.objects.insert(id, obj);
            }
        }
    }
    let n = pages.len();
    let mut kids = Vec::with_capacity(n);
    for (id, mut dict) in pages {
        dict.set("Parent", pages_id);
        out.objects.insert(id, Object::Dictionary(dict));
        kids.push(Object::Reference(id));
    }
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", "Pages");
    pages_dict.set("Kids", kids);
    pages_dict.set("Count", n as i64);
    out.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", pages_id);
    let catalog_id = out.add_object(catalog);
    out.trailer.set("Root", catalog_id);
    out.max_id = out.objects.keys().map(|k| k.0).max().unwrap_or(1);
    out.renumber_objects();
    out.compress();
    out.save(dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok(n)
}

/// Lê intervalos como "1-3, 7, 10-" (páginas começam em 1). `total` limita o fim.
pub fn parse_ranges(text: &str, total: u32) -> Result<Vec<u32>, String> {
    let mut pages = Vec::new();
    for part in text.split([',', ';']).map(str::trim).filter(|p| !p.is_empty()) {
        let bad = || format!("intervalo inválido: \"{part}\". Use algo como 1-3, 7, 10-");
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => {
                let a: u32 = if a.trim().is_empty() { 1 } else { a.trim().parse().map_err(|_| bad())? };
                let b: u32 = if b.trim().is_empty() { total } else { b.trim().parse().map_err(|_| bad())? };
                (a, b)
            }
            None => {
                let v: u32 = part.parse().map_err(|_| bad())?;
                (v, v)
            }
        };
        if a == 0 || a > b {
            return Err(bad());
        }
        if a > total {
            return Err(format!("a página {a} não existe (o PDF tem {total})."));
        }
        for p in a..=b.min(total) {
            if !pages.contains(&p) {
                pages.push(p);
            }
        }
    }
    if pages.is_empty() {
        return Err("informe as páginas, por exemplo 1-3, 7".into());
    }
    Ok(pages)
}

fn keep_only(doc: &Document, keep: &[u32]) -> Document {
    let mut d = doc.clone();
    let total = d.get_pages().len() as u32;
    let remove: Vec<u32> = (1..=total).filter(|p| !keep.contains(p)).collect();
    d.delete_pages(&remove);
    d.prune_objects();
    d.compress();
    d
}

/// Salva as páginas pedidas num novo PDF (`ranges`) ou cada página num PDF próprio (`ranges = None`).
/// Devolve (descrição, primeiro arquivo gerado).
pub fn split(
    src: &Path,
    out_dir: Option<&Path>,
    ranges: Option<&str>,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let doc = open(src)?;
    let total = doc.get_pages().len() as u32;
    match ranges {
        Some(text) => {
            let keep = parse_ranges(text, total)?;
            let mut d = keep_only(&doc, &keep);
            let label = text.replace([' ', ','], "_").replace("__", "_");
            let dst = tools::out_path(src, out_dir, &format!("_paginas_{}", label.trim_matches('_')), "pdf");
            d.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
            Ok((format!("{} de {total} páginas", keep.len()), dst))
        }
        None => {
            let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let parent = out_dir.map(Path::to_path_buf).or_else(|| src.parent().map(Path::to_path_buf)).unwrap_or_default();
            let folder = tools::unique_path(parent.join(format!("{stem}_paginas")));
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let digits = total.to_string().len().max(2);
            let mut first = None;
            for p in 1..=total {
                if cancelled() {
                    return Err("Cancelado.".into());
                }
                let mut d = keep_only(&doc, &[p]);
                let dst = folder.join(format!("{stem}_pagina_{p:0digits$}.pdf"));
                d.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
                first.get_or_insert(dst);
                on_progress(p as f32 / total as f32);
            }
            Ok((format!("{total} PDFs na pasta {}", tools::file_name(&folder)), first.unwrap_or(folder)))
        }
    }
}
