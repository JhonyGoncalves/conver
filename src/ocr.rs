//! Texto de imagem (OCR) com o reconhecedor que já vem no Windows (Windows.Media.Ocr), sem internet.

use std::path::Path;

use windows::Globalization::Language;
use windows::Graphics::Imaging::BitmapDecoder;
use windows::Media::Ocr::OcrEngine;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::HSTRING;

use crate::{imaging, tools};

fn init() {
    // SAFETY: inicializa o WinRT nesta thread; chamar de novo só devolve um aviso.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

fn err(e: windows::core::Error) -> String {
    e.message().to_string()
}

/// Idiomas de OCR instalados no Windows: (código, nome).
pub fn languages() -> Vec<(String, String)> {
    init();
    let Ok(list) = OcrEngine::AvailableRecognizerLanguages() else { return Vec::new() };
    list.into_iter()
        .filter_map(|l| Some((l.LanguageTag().ok()?.to_string(), l.DisplayName().ok()?.to_string())))
        .collect()
}

/// Lê o texto da imagem. `lang` vazio = idiomas do perfil do usuário.
pub fn recognize(path: &Path, lang: &str) -> Result<String, String> {
    init();
    let engine = if lang.is_empty() {
        OcrEngine::TryCreateFromUserProfileLanguages().map_err(err)?
    } else {
        let l = Language::CreateLanguage(&HSTRING::from(lang)).map_err(err)?;
        OcrEngine::TryCreateFromLanguage(&l).map_err(|_| format!("o idioma {lang} não está instalado para OCR no Windows."))?
    };

    // o OCR do Windows tem limite de tamanho e lê poucos formatos: entrega sempre um PNG do tamanho certo
    let img = imaging::load(path)?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let img = if img.width() > max || img.height() > max { img.resize(max, max, image::imageops::FilterType::Lanczos3) } else { img };
    // imagens muito pequenas (prints de texto miúdo) são ampliadas, o que melhora bastante a leitura
    let img = if img.width().max(img.height()) < 800 {
        let s = (1200 / img.width().max(img.height()).max(1)).clamp(1, 4);
        img.resize(img.width() * s, img.height() * s, image::imageops::FilterType::CatmullRom)
    } else {
        img
    };
    let tmp = std::env::temp_dir().join(format!("conver_ocr_{}_{}.png", std::process::id(), tools::file_size(path)));
    img.save(&tmp).map_err(|e| e.to_string())?;

    let result = (|| -> windows::core::Result<String> {
        let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(tmp.as_os_str()))?.join()?;
        let stream = file.OpenAsync(FileAccessMode::Read)?.join()?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
        let bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
        let result = engine.RecognizeAsync(&bitmap)?.join()?;
        let mut lines = Vec::new();
        for line in result.Lines()? {
            lines.push(line.Text()?.to_string());
        }
        Ok(lines.join("\n"))
    })();
    let _ = std::fs::remove_file(&tmp);
    result.map_err(err)
}
