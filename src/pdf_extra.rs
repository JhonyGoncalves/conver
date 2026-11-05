//! Mais ferramentas de PDF: comprimir, proteger com senha, tirar a senha, salvar páginas como imagens
//! (com o leitor de PDF do próprio Windows) e PDF pesquisável (OCR do Windows numa camada de texto invisível).

use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, imageops::FilterType};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Storage::StorageFile;
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
use windows::core::HSTRING;

use crate::imaging::Level;
use crate::tools;

fn werr(e: windows::core::Error) -> String {
    e.message().to_string()
}

fn load(path: &Path) -> Result<Document, String> {
    Document::load(path).map_err(|e| format!("{}: não foi possível abrir o PDF ({e})", tools::file_name(path)))
}

fn save(doc: &mut Document, dst: &Path) -> Result<(), String> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(dst).map_err(|e| format!("não foi possível salvar: {e}"))?);
    doc.save_modern(&mut f).map_err(|e| format!("não foi possível salvar: {e}"))
}

// ================================================================ comprimir

fn name_of(d: &Dictionary, key: &[u8]) -> Option<Vec<u8>> {
    d.get(key).ok().and_then(|o| o.as_name().ok()).map(<[u8]>::to_vec)
}

/// Só filtros simples (um nome, ou uma lista de um nome).
fn single_filter(d: &Dictionary) -> Option<Vec<u8>> {
    match d.get(b"Filter").ok()? {
        Object::Name(n) => Some(n.clone()),
        Object::Array(a) if a.len() == 1 => a[0].as_name().ok().map(<[u8]>::to_vec),
        _ => None,
    }
}

/// Recomprime as imagens de dentro do PDF (e regrava tudo com compressão de objetos).
pub fn compress(src: &Path, out_dir: Option<&Path>, level: Level, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(String, PathBuf), String> {
    let mut doc = load(src)?;
    if doc.is_encrypted() {
        return Err(format!("{} está protegido por senha; tire a senha antes.", tools::file_name(src)));
    }
    let (max_side, quality): (u32, u8) = match level {
        Level::Leve => (2400, 80),
        Level::Media => (1800, 65),
        Level::Forte => (1200, 50),
    };
    let ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| matches!(o, Object::Stream(s) if name_of(&s.dict, b"Subtype").as_deref() == Some(b"Image")))
        .map(|(id, _)| *id)
        .collect();
    let mut changed = 0;
    for (i, id) in ids.iter().enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        on_progress(i as f32 / ids.len().max(1) as f32 * 0.9);
        // espaço de cor da imagem (resolve referências)
        let Some(Object::Stream(stream)) = doc.objects.get(id) else { continue };
        let cs = match stream.dict.get(b"ColorSpace") {
            Ok(Object::Reference(r)) => doc.get_object(*r).ok().cloned(),
            Ok(o) => Some(o.clone()),
            Err(_) => None,
        };
        let comps = match &cs {
            Some(Object::Name(n)) if n == b"DeviceRGB" => Some(3),
            Some(Object::Name(n)) if n == b"DeviceGray" => Some(1),
            Some(Object::Array(a)) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => a
                .get(1)
                .and_then(|r| r.as_reference().ok())
                .and_then(|r| doc.get_object(r).ok())
                .and_then(|o| o.as_stream().ok())
                .and_then(|s| s.dict.get(b"N").ok()?.as_i64().ok())
                .filter(|n| *n == 1 || *n == 3)
                .map(|n| n as usize),
            _ => None,
        };
        let Some(Object::Stream(stream)) = doc.objects.get_mut(id) else { continue };
        let Some(comps) = comps else { continue };
        if stream.dict.has(b"ImageMask") || stream.dict.has(b"Decode") || stream.dict.has(b"Mask") && !stream.dict.get(b"Mask").map(|m| m.as_reference().is_ok()).unwrap_or(false) {
            continue;
        }
        let bpc = stream.dict.get(b"BitsPerComponent").and_then(|o| o.as_i64()).unwrap_or(8);
        let w = stream.dict.get(b"Width").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        let h = stream.dict.get(b"Height").and_then(|o| o.as_i64()).unwrap_or(0) as u32;
        if bpc != 8 || w < 16 || h < 16 {
            continue;
        }
        let before = stream.content.len();
        let img: Option<DynamicImage> = match single_filter(&stream.dict).as_deref() {
            Some(b"DCTDecode") => image::load_from_memory_with_format(&stream.content, ImageFormat::Jpeg).ok(),
            // imagens sem perda grandes viram JPEG só nos níveis médio e forte (prints e desenhos ficariam borrados no leve)
            Some(b"FlateDecode") if level != Level::Leve && w * h >= 400 * 400 => stream.decompressed_content().ok().and_then(|raw| {
                if raw.len() < (w * h) as usize * comps {
                    return None;
                }
                let raw = raw[..(w * h) as usize * comps].to_vec();
                if comps == 3 {
                    image::RgbImage::from_raw(w, h, raw).map(DynamicImage::ImageRgb8)
                } else {
                    image::GrayImage::from_raw(w, h, raw).map(DynamicImage::ImageLuma8)
                }
            }),
            _ => None,
        };
        let Some(img) = img else { continue };
        let img = if img.width().max(img.height()) > max_side { img.resize(max_side, max_side, FilterType::Lanczos3) } else { img };
        let img = if comps == 1 { DynamicImage::ImageLuma8(img.to_luma8()) } else { DynamicImage::ImageRgb8(img.to_rgb8()) };
        let mut bytes = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
        if img.write_with_encoder(enc).is_err() || bytes.len() >= before {
            continue;
        }
        stream.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        stream.dict.remove(b"DecodeParms");
        stream.dict.set("Width", img.width() as i64);
        stream.dict.set("Height", img.height() as i64);
        stream.set_content(bytes);
        stream.allows_compression = false;
        changed += 1;
    }
    doc.prune_objects();
    doc.delete_zero_length_streams();
    doc.compress();
    let dst = tools::out_path(src, out_dir, "_comprimido", "pdf");
    save(&mut doc, &dst)?;
    let (a, b) = (tools::file_size(src), tools::file_size(&dst));
    if b >= a {
        // não conseguiu reduzir: entrega uma cópia do original
        std::fs::copy(src, &dst).map_err(|e| e.to_string())?;
        on_progress(1.0);
        return Ok(("o PDF já estava otimizado (sem redução)".into(), dst));
    }
    on_progress(1.0);
    Ok((format!("{changed} imagem(ns) recomprimida(s) • {}", tools::savings(a, b)), dst))
}

// ================================================================ senha

/// Protege o PDF com senha (AES-128). `owner` vazio = mesma senha do usuário.
pub fn protect(src: &Path, out_dir: Option<&Path>, password: &str, owner: &str, allow_print: bool, allow_copy: bool) -> Result<(String, PathBuf), String> {
    use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};
    if password.is_empty() {
        return Err("informe a senha.".into());
    }
    let mut doc = load(src)?;
    if doc.is_encrypted() {
        return Err(format!("{} já está protegido por senha.", tools::file_name(src)));
    }
    let mut perms = Permissions::COPYABLE_FOR_ACCESSIBILITY;
    if allow_print {
        perms |= Permissions::PRINTABLE | Permissions::PRINTABLE_IN_HIGH_QUALITY;
    }
    if allow_copy {
        perms |= Permissions::COPYABLE;
    }
    // a cifra usa o ID do documento: cria um se o PDF não tiver
    if !doc.trailer.has(b"ID") {
        let mut id = [0u8; 16];
        getrandom::fill(&mut id).map_err(|e| e.to_string())?;
        let s = Object::String(id.to_vec(), lopdf::StringFormat::Hexadecimal);
        doc.trailer.set("ID", Object::Array(vec![s.clone(), s]));
    }
    let owner = if owner.is_empty() { password } else { owner };
    // AES-128: abre em todos os leitores (o do Windows não aceita o AES-256 gerado aqui)
    let filter: std::sync::Arc<dyn CryptFilter> = std::sync::Arc::new(Aes128CryptFilter);
    let version = EncryptionVersion::V4 {
        document: &doc,
        encrypt_metadata: true,
        crypt_filters: std::collections::BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: owner,
        user_password: password,
        permissions: perms,
    };
    let state = EncryptionState::try_from(version).map_err(|e| e.to_string())?;
    doc.encrypt(&state).map_err(|e| format!("não foi possível proteger: {e}"))?;
    let dst = tools::out_path(src, out_dir, "_protegido", "pdf");
    doc.save(&dst).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok(("protegido com senha (AES-128)".into(), dst))
}

/// Tira a senha de um PDF (é preciso saber a senha).
pub fn unlock(src: &Path, out_dir: Option<&Path>, password: &str) -> Result<(String, PathBuf), String> {
    let mut doc = Document::load_with_password(src, password).or_else(|_| load(src))?;
    if doc.is_encrypted() {
        doc.decrypt(password).map_err(|_| "senha incorreta.".to_string())?;
    }
    if doc.trailer.has(b"Encrypt") {
        doc.trailer.remove(b"Encrypt");
    }
    doc.encryption_state = None;
    let dst = tools::out_path(src, out_dir, "_sem_senha", "pdf");
    save(&mut doc, &dst)?;
    Ok(("senha removida".into(), dst))
}

// ================================================================ páginas como imagens (Windows.Data.Pdf)

fn open_winrt(path: &Path, password: &str) -> Result<PdfDocument, String> {
    crate::ocr::init();
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str())).and_then(|f| f.join()).map_err(werr)?;
    let r = if password.is_empty() {
        PdfDocument::LoadFromFileAsync(&file).and_then(|a| a.join())
    } else {
        PdfDocument::LoadFromFileWithPasswordAsync(&file, &HSTRING::from(password)).and_then(|a| a.join())
    };
    r.map_err(|e| {
        if e.code().0 as u32 == 0x8007052B || e.message().to_lowercase().contains("password") {
            "PDF protegido: informe a senha.".to_string()
        } else {
            format!("não foi possível abrir o PDF ({})", e.message())
        }
    })
}

/// Desenha a página (começando em 0) com `dpi` e devolve a imagem em PNG (bytes) e o tamanho da página em pontos.
fn render_page(doc: &PdfDocument, index: u32, dpi: f32) -> Result<(Vec<u8>, (f32, f32)), String> {
    (|| -> windows::core::Result<(Vec<u8>, (f32, f32))> {
        let page = doc.GetPage(index)?;
        let size = page.Size()?; // em DIPs (1/96 de polegada)
        let opts = PdfPageRenderOptions::new()?;
        opts.SetDestinationWidth((size.Width * dpi / 96.0).round().max(1.0) as u32)?;
        opts.SetDestinationHeight((size.Height * dpi / 96.0).round().max(1.0) as u32)?;
        let stream = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&stream, &opts)?.join()?;
        let len = stream.Size()? as u32;
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0)?)?;
        reader.LoadAsync(len)?.join()?;
        let mut buf = vec![0u8; len as usize];
        reader.ReadBytes(&mut buf)?;
        Ok((buf, (size.Width * 72.0 / 96.0, size.Height * 72.0 / 96.0)))
    })()
    .map_err(werr)
}

/// Miniaturas das páginas (para organizar): chama `each(página, PNG)` para cada uma, da primeira à última.
pub fn thumbnails(src: &Path, dpi: f32, mut each: impl FnMut(u32, Vec<u8>) -> bool) -> Result<u32, String> {
    let doc = open_winrt(src, "")?;
    let total = doc.PageCount().map_err(werr)?;
    for i in 0..total {
        let (png, _) = render_page(&doc, i, dpi)?;
        if !each(i + 1, png) {
            break;
        }
    }
    Ok(total)
}

/// Palavras de cada página (com a caixa em pixels da página desenhada), pelo OCR do Windows: serve para PDFs com
/// texto e para os escaneados. Chama `each(página, linhas, (largura, altura) em pixels)`; `false` para.
pub fn ocr_words(
    src: &Path,
    lang: &str,
    mut each: impl FnMut(u32, Vec<Vec<crate::ocr::Word>>, (u32, u32)) -> bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<u32, String> {
    let engine = crate::ocr::engine(lang)?;
    let doc = open_winrt(src, "")?;
    let total = doc.PageCount().map_err(werr)?;
    let max_dim = crate::ocr::max_dimension() as f32;
    for i in 0..total {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        // ~300 dpi deixa números miúdos legíveis; o tamanho fica no limite do OCR
        let size = doc.GetPage(i).and_then(|p| p.Size()).map_err(werr)?;
        let longest = size.Width.max(size.Height) / 96.0;
        let dpi = (300.0f32).min(max_dim / longest.max(0.1)).max(72.0);
        let (png, _) = render_page(&doc, i, dpi)?;
        let (lines, dims) = (|| -> windows::core::Result<_> {
            let stream = InMemoryRandomAccessStream::new()?;
            let writer = windows::Storage::Streams::DataWriter::CreateDataWriter(&stream)?;
            writer.WriteBytes(&png)?;
            writer.StoreAsync()?.join()?;
            writer.FlushAsync()?.join()?;
            writer.DetachStream()?;
            stream.Seek(0)?;
            let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
            let bmp = decoder.GetSoftwareBitmapAsync()?.join()?;
            Ok((bmp, (decoder.PixelWidth()?, decoder.PixelHeight()?)))
        })()
        .map_err(werr)
        .and_then(|(bmp, dims)| Ok((crate::ocr::words(&engine, &bmp)?, dims)))?;
        if !each(i + 1, lines, dims) {
            break;
        }
    }
    Ok(total)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageFmt {
    Png,
    Jpg,
}

/// Salva cada página (ou as páginas de `ranges`) como imagem, numa pasta `<nome>_imagens`.
pub fn to_images(
    src: &Path,
    out_dir: Option<&Path>,
    fmt: PageFmt,
    dpi: u32,
    ranges: Option<&str>,
    password: &str,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let doc = open_winrt(src, password)?;
    let total = doc.PageCount().map_err(werr)?;
    let pages: Vec<u32> = match ranges.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => crate::pdf::parse_ranges(r, total)?,
        None => (1..=total).collect(),
    };
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let parent = out_dir.map(Path::to_path_buf).or_else(|| src.parent().map(Path::to_path_buf)).unwrap_or_default();
    let folder = tools::unique_path(parent.join(format!("{stem}_imagens")));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let dpi = dpi.clamp(36, 600) as f32;
    let digits = total.to_string().len().max(2);
    let mut first = None;
    for (i, p) in pages.iter().enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        let (png, _) = render_page(&doc, p - 1, dpi)?;
        let (ext, bytes) = match fmt {
            PageFmt::Png => ("png", png),
            PageFmt::Jpg => {
                let img = image::load_from_memory_with_format(&png, ImageFormat::Png).map_err(|e| e.to_string())?;
                ("jpg", crate::imaging::encode(&DynamicImage::ImageRgb8(img.to_rgb8()), crate::imaging::Fmt::Jpg, Some(90))?)
            }
        };
        let dst = folder.join(format!("{stem}_pagina_{p:0digits$}.{ext}"));
        std::fs::write(&dst, bytes).map_err(|e| format!("não foi possível salvar: {e}"))?;
        first.get_or_insert(dst);
        on_progress((i + 1) as f32 / pages.len() as f32);
    }
    Ok((format!("{} imagem(ns) na pasta {}", pages.len(), tools::file_name(&folder)), first.unwrap_or(folder)))
}

// ================================================================ PDF pesquisável (OCR)

/// Converte para WinAnsi (a codificação da fonte Helvetica padrão); o que não existir vira "?".
fn winansi(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| match c as u32 {
            0x20..=0x7E | 0xA0..=0xFF => c as u8,
            _ => match c {
                '€' => 0x80, '‚' => 0x82, '„' => 0x84, '…' => 0x85, '‘' => 0x91, '’' => 0x92, '“' => 0x93,
                '”' => 0x94, '•' => 0x95, '–' => 0x96, '—' => 0x97, 'Œ' => 0x8C, 'œ' => 0x9C, 'Š' => 0x8A,
                'š' => 0x9A, 'Ÿ' => 0x9F, 'Ž' => 0x8E, 'ž' => 0x9E,
                _ => b'?',
            },
        })
        .collect()
}

fn pdf_string(bytes: &[u8]) -> String {
    let mut s = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            0x20..=0x7E => s.push(b as char),
            _ => s.push_str(&format!("\\{b:03o}")),
        }
    }
    s.push(')');
    s
}

/// Recursos da página, copiados para a própria página (podem vir herdados ou por referência).
fn page_resources(doc: &Document, page: ObjectId) -> Dictionary {
    let mut cur = Some(page);
    let mut guard = 0;
    while let (Some(id), true) = (cur, guard < 32) {
        guard += 1;
        let Ok(d) = doc.get_dictionary(id) else { break };
        match d.get(b"Resources") {
            Ok(Object::Dictionary(r)) => return r.clone(),
            Ok(Object::Reference(r)) => return doc.get_dictionary(*r).cloned().unwrap_or_default(),
            _ => {}
        }
        cur = d.get(b"Parent").and_then(Object::as_reference).ok();
    }
    Dictionary::new()
}

fn page_box(doc: &Document, page: ObjectId) -> [f32; 4] {
    let mut cur = Some(page);
    let mut guard = 0;
    while let (Some(id), true) = (cur, guard < 32) {
        guard += 1;
        let Ok(d) = doc.get_dictionary(id) else { break };
        for key in [b"CropBox".as_slice(), b"MediaBox"] {
            let arr = match d.get(key) {
                Ok(Object::Array(a)) => Some(a.clone()),
                Ok(Object::Reference(r)) => doc.get_object(*r).ok().and_then(|o| o.as_array().ok().cloned()),
                _ => None,
            };
            if let Some(a) = arr.filter(|a| a.len() == 4) {
                let v: Vec<f32> = a.iter().map(|o| o.as_float().or_else(|_| o.as_i64().map(|i| i as f32)).unwrap_or(0.0)).collect();
                return [v[0].min(v[2]), v[1].min(v[3]), v[0].max(v[2]), v[1].max(v[3])];
            }
        }
        cur = d.get(b"Parent").and_then(Object::as_reference).ok();
    }
    [0.0, 0.0, 612.0, 792.0]
}

fn page_rotation(doc: &Document, page: ObjectId) -> i64 {
    let mut cur = Some(page);
    let mut guard = 0;
    while let (Some(id), true) = (cur, guard < 32) {
        guard += 1;
        let Ok(d) = doc.get_dictionary(id) else { break };
        if let Ok(r) = d.get(b"Rotate").and_then(Object::as_i64) {
            return r.rem_euclid(360);
        }
        cur = d.get(b"Parent").and_then(Object::as_reference).ok();
    }
    0
}

/// Adiciona uma camada de texto invisível (reconhecido pelo OCR do Windows) às páginas sem texto,
/// para o PDF ficar pesquisável e o texto poder ser selecionado e copiado.
pub fn make_searchable(
    src: &Path,
    out_dir: Option<&Path>,
    lang: &str,
    skip_text_pages: bool,
    on_progress: &dyn Fn(f32),
    cancelled: &dyn Fn() -> bool,
) -> Result<(String, PathBuf), String> {
    let mut doc = load(src)?;
    if doc.is_encrypted() {
        return Err(format!("{} está protegido por senha; tire a senha antes.", tools::file_name(src)));
    }
    let engine = crate::ocr::engine(lang)?;
    let wdoc = open_winrt(src, "")?;
    let pages: Vec<(u32, ObjectId)> = doc.get_pages().into_iter().collect();
    let max_dim = crate::ocr::max_dimension() as f32;

    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
    let font_id = doc.add_object(font);
    let (mut done, mut skipped, mut words_total) = (0, 0, 0);
    for (i, (num, page_id)) in pages.iter().enumerate() {
        if cancelled() {
            return Err("Cancelado.".into());
        }
        on_progress(i as f32 / pages.len() as f32);
        if skip_text_pages && doc.extract_text(&[*num]).map(|t| t.split_whitespace().count() > 3).unwrap_or(false) {
            skipped += 1;
            continue;
        }
        // desenha a página em ~250 dpi (sem passar do limite do OCR) e reconhece as palavras
        let bx = page_box(&doc, *page_id);
        let rot = page_rotation(&doc, *page_id);
        let (pw, ph) = (bx[2] - bx[0], bx[3] - bx[1]);
        let (dw, dh) = if rot == 90 || rot == 270 { (ph, pw) } else { (pw, ph) };
        let dpi = (250.0f32).min(max_dim / (dw.max(dh) / 72.0)).max(72.0);
        let (png, _) = render_page(&wdoc, num - 1, dpi)?;
        let lines = (|| -> windows::core::Result<_> {
            let stream = InMemoryRandomAccessStream::new()?;
            let writer = windows::Storage::Streams::DataWriter::CreateDataWriter(&stream)?;
            writer.WriteBytes(&png)?;
            writer.StoreAsync()?.join()?;
            writer.FlushAsync()?.join()?;
            writer.DetachStream()?;
            stream.Seek(0)?;
            let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
            let bmp = decoder.GetSoftwareBitmapAsync()?.join()?;
            Ok((bmp, decoder.PixelWidth()?))
        })()
        .map_err(werr)
        .and_then(|(bmp, px_w)| Ok((crate::ocr::words(&engine, &bmp)?, px_w)));
        let (lines, px_w) = lines?;
        // pixels da imagem → pontos da página
        let k = dw / px_w.max(1) as f32;
        let mut content = String::from("q\nBT\n3 Tr\n");
        let mut n_words = 0;
        for line in &lines {
            // uma linha inteira por vez (com espaços), para o texto copiado e pesquisado sair natural
            let words: Vec<&crate::ocr::Word> = line.iter().filter(|w| !w.text.trim().is_empty()).collect();
            if words.is_empty() {
                continue;
            }
            let text = words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ");
            let x0 = words.iter().map(|w| w.rect.0).fold(f32::MAX, f32::min);
            let y0 = words.iter().map(|w| w.rect.1).fold(f32::MAX, f32::min);
            let x1 = words.iter().map(|w| w.rect.0 + w.rect.2).fold(f32::MIN, f32::max);
            let y1 = words.iter().map(|w| w.rect.1 + w.rect.3).fold(f32::MIN, f32::max);
            n_words += words.len() - 1; // o +1 da linha é somado abaixo
            {
                let bytes = winansi(&text);
                let (x, y, ww, hh) = (x0 * k, y0 * k, (x1 - x0) * k, (y1 - y0) * k);
                let size = (hh * 0.92).max(1.0);
                // linha de base um pouco acima da parte de baixo da caixa
                let (dx, dy) = (x, y + hh * 0.8);
                let (ux, uy, m) = match rot {
                    90 => (bx[0] + dy, bx[1] + dx, (0.0, 1.0, -1.0, 0.0)),
                    180 => (bx[2] - dx, bx[1] + dy, (-1.0, 0.0, 0.0, -1.0)),
                    270 => (bx[2] - dy, bx[3] - dx, (0.0, -1.0, 1.0, 0.0)),
                    _ => (bx[0] + dx, bx[3] - dy, (1.0, 0.0, 0.0, 1.0)),
                };
                // largura média da Helvetica ≈ 0,52 do tamanho; Tz estica para ocupar a caixa da palavra
                let natural = size * 0.52 * bytes.len() as f32;
                let tz = (ww / natural.max(0.1) * 100.0).clamp(10.0, 1000.0);
                content.push_str(&format!(
                    "/FConverOcr {size:.2} Tf\n{tz:.1} Tz\n{:.3} {:.3} {:.3} {:.3} {ux:.2} {uy:.2} Tm\n{} Tj\n",
                    m.0, m.1, m.2, m.3, pdf_string(&bytes)
                ));
                n_words += 1;
            }
        }
        content.push_str(std::str::from_utf8(b"ET\nQ\n").unwrap_or_default()); // bytes: não é texto de tela
        if n_words == 0 {
            continue;
        }
        words_total += n_words;
        // recursos: copia para a página e acrescenta a fonte
        let mut res = page_resources(&doc, *page_id);
        let mut fonts = match res.get(b"Font") {
            Ok(Object::Dictionary(f)) => f.clone(),
            Ok(Object::Reference(r)) => doc.get_dictionary(*r).cloned().unwrap_or_default(),
            _ => Dictionary::new(),
        };
        fonts.set("FConverOcr", font_id);
        res.set("Font", fonts);
        // conteúdo: o original entre q/Q (para não herdar transformações) + a camada de texto
        let old = doc.get_page_contents(*page_id);
        let open = doc.add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
        let mut layer = Stream::new(Dictionary::new(), format!("Q\n{content}").into_bytes());
        let _ = layer.compress();
        let layer = doc.add_object(layer);
        let mut contents = vec![Object::Reference(open)];
        contents.extend(old.into_iter().map(Object::Reference));
        contents.push(Object::Reference(layer));
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(*page_id) {
            d.set("Resources", res);
            d.set("Contents", Object::Array(contents));
        }
        done += 1;
    }
    doc.compress();
    let dst = tools::out_path(src, out_dir, "_pesquisavel", "pdf");
    save(&mut doc, &dst)?;
    on_progress(1.0);
    let mut info = format!("{done} página(s) com texto reconhecido • {words_total} palavras");
    if skipped > 0 {
        info.push_str(&format!(" • {skipped} já tinham texto"));
    }
    Ok((info, dst))
}
