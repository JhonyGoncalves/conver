//! Informações de um arquivo: dados gerais, EXIF de fotos (câmera, GPS…), faixas de áudio/vídeo e páginas de PDF.

use std::path::Path;

use crate::{imaging, pdf, tools, video};

/// Uma seção de informações: título e pares (rótulo, valor).
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<(String, String)>,
}

pub struct Info {
    pub sections: Vec<Section>,
    /// Latitude e longitude, se a foto tiver GPS.
    pub gps: Option<(f64, f64)>,
}

impl Info {
    /// Tudo em texto, para copiar.
    pub fn as_text(&self) -> String {
        let mut s = String::new();
        for sec in &self.sections {
            s.push_str(&format!("[{}]\n", sec.title));
            for (k, v) in &sec.rows {
                s.push_str(&format!("{k}: {v}\n"));
            }
            s.push('\n');
        }
        s.trim_end().to_string()
    }
}

fn row(rows: &mut Vec<(String, String)>, k: &str, v: impl Into<String>) {
    let v = v.into();
    if !v.trim().is_empty() {
        rows.push((k.to_string(), v));
    }
}

fn fmt_systime(t: std::io::Result<std::time::SystemTime>) -> String {
    // sem dependência de data: mostra só quanto tempo atrás, que é o que interessa na prática
    let Ok(t) = t else { return String::new() };
    let Ok(age) = std::time::SystemTime::now().duration_since(t) else { return String::new() };
    let d = age.as_secs() / 86_400;
    match d {
        0 => "hoje".into(),
        1 => "ontem".into(),
        d if d < 60 => format!("há {d} dias"),
        d if d < 730 => format!("há {} meses", d / 30),
        d => format!("há {} anos", d / 365),
    }
}

fn gps_coord(f: &exif::Field, reference: Option<&exif::Field>) -> Option<f64> {
    let exif::Value::Rational(v) = &f.value else { return None };
    if v.len() < 3 {
        return None;
    }
    let deg = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
    let negative = reference.map(|r| r.display_value().to_string()).is_some_and(|r| r.starts_with('S') || r.starts_with('W'));
    Some(if negative { -deg } else { deg })
}

fn exif_section(path: &Path) -> (Option<Section>, Option<(f64, f64)>) {
    let Ok(file) = std::fs::File::open(path) else { return (None, None) };
    let Ok(data) = exif::Reader::new().read_from_container(&mut std::io::BufReader::new(file)) else { return (None, None) };
    let mut rows = Vec::new();
    let get = |tag| data.get_field(tag, exif::In::PRIMARY);
    let show = |tag| get(tag).map(|f| f.display_value().with_unit(&data).to_string().trim_matches('"').to_string());
    for (label, tag) in [
        ("Câmera (marca)", exif::Tag::Make),
        ("Câmera (modelo)", exif::Tag::Model),
        ("Lente", exif::Tag::LensModel),
        ("Data da foto", exif::Tag::DateTimeOriginal),
        ("Exposição", exif::Tag::ExposureTime),
        ("Abertura", exif::Tag::FNumber),
        ("ISO", exif::Tag::PhotographicSensitivity),
        ("Distância focal", exif::Tag::FocalLength),
        ("Flash", exif::Tag::Flash),
        ("Programa", exif::Tag::Software),
        ("Autor", exif::Tag::Artist),
        ("Direitos autorais", exif::Tag::Copyright),
    ] {
        if let Some(v) = show(tag) {
            row(&mut rows, label, v);
        }
    }
    let gps = match (get(exif::Tag::GPSLatitude), get(exif::Tag::GPSLongitude)) {
        (Some(lat), Some(lon)) => gps_coord(lat, get(exif::Tag::GPSLatitudeRef)).zip(gps_coord(lon, get(exif::Tag::GPSLongitudeRef))),
        _ => None,
    };
    if let Some((lat, lon)) = gps {
        row(&mut rows, "Localização (GPS)", format!("{lat:.6}, {lon:.6}"));
    }
    let n = data.fields().count();
    row(&mut rows, "Total de campos EXIF", n.to_string());
    let sec = (!rows.is_empty()).then_some(Section { title: "Foto (EXIF)", rows });
    (sec, gps)
}

/// Faixas e duração lidas do ffmpeg.
fn media_sections(path: &Path) -> Vec<Section> {
    let Some(ff) = tools::find_tool("ffmpeg") else { return Vec::new() };
    let Ok(out) = tools::command(&ff).args(["-hide_banner", "-i"]).arg(path).output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stderr).to_string();
    let mut general = Vec::new();
    if let Some(line) = text.lines().find(|l| l.trim_start().starts_with("Duration:")) {
        let mut parts = line.trim().split(", ");
        if let Some(d) = parts.next().and_then(|d| d.strip_prefix("Duration: ")) {
            if let Some(secs) = video::probe(path).duration {
                row(&mut general, "Duração", format!("{} ({d})", video::fmt_time(secs)));
            }
        }
        for p in parts {
            if let Some(b) = p.strip_prefix("bitrate: ") {
                row(&mut general, "Taxa de bits total", b);
            }
        }
    }
    let mut meta = Vec::new();
    let mut in_meta = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("Metadata:") && line.starts_with("  Metadata") {
            in_meta = true;
            continue;
        }
        if in_meta {
            if line.starts_with("    ") && t.contains(':') {
                if let Some((k, v)) = t.split_once(':') {
                    let k = k.trim();
                    if matches!(k, "title" | "artist" | "album" | "date" | "genre" | "comment" | "encoder" | "creation_time" | "album_artist" | "track") {
                        row(&mut meta, k, v.trim());
                    }
                }
            } else {
                in_meta = false;
            }
        }
    }
    let mut streams = Vec::new();
    for line in text.lines().filter(|l| l.trim_start().starts_with("Stream #")) {
        let t = line.trim();
        let Some((head, body)) = t.split_once(": ").and_then(|(_, rest)| rest.split_once(": ")) else { continue };
        let kind = match head {
            "Video" if body.contains("attached pic") => "Capa",
            "Video" => "Vídeo",
            "Audio" => "Áudio",
            "Subtitle" => "Legenda",
            other => other,
        };
        // remove detalhes técnicos que só atrapalham (ids em hexadecimal, "default")
        let clean: Vec<&str> = body.split(", ").filter(|p| !p.contains("(default)") && !p.starts_with("SAR")).collect();
        row(&mut streams, kind, clean.join(", "));
    }
    let mut out = Vec::new();
    if !general.is_empty() {
        out.push(Section { title: "Mídia", rows: general });
    }
    if !streams.is_empty() {
        out.push(Section { title: "Faixas", rows: streams });
    }
    if !meta.is_empty() {
        out.push(Section { title: "Etiquetas", rows: meta });
    }
    out
}

pub fn read(path: &Path) -> Info {
    let mut sections = Vec::new();
    let mut gps = None;
    let ext = tools::ext_lower(path);
    let meta = std::fs::metadata(path);
    let mut general = Vec::new();
    row(&mut general, "Nome", tools::file_name(path));
    row(&mut general, "Pasta", path.parent().map(|p| p.display().to_string()).unwrap_or_default());
    if let Ok(m) = &meta {
        row(&mut general, "Tamanho", format!("{} ({} bytes)", tools::human_size(m.len()), m.len()));
        row(&mut general, "Modificado", fmt_systime(m.modified()));
        row(&mut general, "Criado", fmt_systime(m.created()));
    }
    row(&mut general, "Tipo", ext.to_uppercase());
    sections.push(Section { title: "Arquivo", rows: general });

    if imaging::IMAGE_EXTS.contains(&ext.as_str()) {
        let mut rows = Vec::new();
        if let Ok(reader) = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()) {
            if let Some(f) = reader.format() {
                row(&mut rows, "Formato", format!("{f:?}").to_uppercase());
            }
            if let Ok((w, h)) = reader.into_dimensions() {
                let mp = w as f64 * h as f64 / 1_000_000.0;
                row(&mut rows, "Dimensões", format!("{w} × {h} px ({mp:.1} megapixels)"));
            }
        }
        if let Ok(img) = imaging::load(path) {
            let c = img.color();
            row(&mut rows, "Cores", format!("{c:?}{}", if c.has_alpha() { " (com transparência)" } else { "" }));
        }
        sections.push(Section { title: "Imagem", rows });
        let (exif, g) = exif_section(path);
        sections.extend(exif);
        gps = g;
    } else if ext == "pdf" {
        let mut rows = Vec::new();
        match pdf::page_count(path) {
            Some(n) => row(&mut rows, "Páginas", n.to_string()),
            None => row(&mut rows, "Páginas", "não foi possível ler (danificado ou protegido)"),
        }
        sections.push(Section { title: "PDF", rows });
    } else {
        sections.extend(media_sections(path));
    }
    Info { sections, gps }
}
