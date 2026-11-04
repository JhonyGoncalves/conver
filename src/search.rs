//! Busca de ferramentas no menu lateral: nome (no idioma da tela), seção e palavras que alguém usaria para
//! procurar aquela ferramenta ("cortar", "aparar", "trim"…). Sem diferenciar maiúsculas e acentos, e
//! tolerando um erro de digitação em palavras maiores.
//!
//! Este arquivo fica fora da extração de textos (i18n/extrair.py): as palavras não aparecem na tela.

/// Junta o que pode ser procurado de uma ferramenta, já normalizado.
pub fn haystack(label: &str, label_tr: &str, section: &str, section_tr: &str, keywords: &str) -> String {
    normalize(&format!("{label} {label_tr} {section} {section_tr} {keywords}"))
}

/// Minúsculas, sem acentos e com o resto da pontuação virando espaço.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            '→' | '/' | '-' | '_' | '(' | ')' | ',' | '.' | '●' | '•' => ' ',
            c => c,
        };
        out.push(c);
    }
    out
}

/// Distância de edição ≤ 1 (uma letra trocada, sobrando ou faltando).
fn near(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let (mut i, mut j, mut edits) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
            continue;
        }
        edits += 1;
        if edits > 1 {
            return false;
        }
        match a.len().cmp(&b.len()) {
            std::cmp::Ordering::Greater => i += 1,
            std::cmp::Ordering::Less => j += 1,
            std::cmp::Ordering::Equal => {
                i += 1;
                j += 1;
            }
        }
    }
    edits + (a.len() - i) + (b.len() - j) <= 1
}

/// Todas as palavras da busca precisam aparecer (como começo de palavra ou dentro dela; com 5+ letras,
/// aceita um erro de digitação).
pub fn matches(query_norm: &str, haystack: &str) -> bool {
    let words: Vec<&str> = haystack.split_whitespace().collect();
    query_norm.split_whitespace().all(|q| {
        haystack.contains(q)
            || (q.chars().count() >= 5
                && words.iter().any(|w| near(q, w) || (w.chars().count() > q.chars().count() && near(q, &w.chars().take(q.chars().count()).collect::<String>()))))
    })
}

/// Palavras de busca de cada ferramenta (o "id" é o nome da tela em `main.rs`), em português e inglês.
pub fn keywords(id: &str) -> &'static str {
    match id {
        "img_resize" => "redimensionar tamanho dimensao dimensoes diminuir aumentar escala pixels largura altura reduzir resolucao resize scale shrink",
        "img_compress" => "comprimir compactar reduzir tamanho peso diminuir arquivo otimizar leve mb kb compress optimize smaller",
        "img_quality" => "qualidade baixa jpg jpeg comprimir reduzir quality lower",
        "img_convert" => "converter formato transformar mudar extensao png jpg jpeg webp bmp gif tiff ico icone heic convert format",
        "img_removebg" => "remover fundo tirar fundo apagar fundo transparente recortar pessoa produto png sem fundo background remove bg cutout",
        "img_upscale" => "ampliar aumentar melhorar qualidade resolucao nitidez upscale upscaling hd 4k 2x 4x ia inteligencia artificial esrgan enhance super resolution",
        "img_transform" => "girar rotacionar virar espelhar inverter recortar cortar crop proporcao 1 1 quadrado 16 9 rotate flip mirror",
        "img_adjust" => "ajustar cores brilho contraste saturacao filtro preto e branco cinza sepia inverter editar foto adjust brightness contrast filter grayscale",
        "img_strip" => "remover metadados exif gps localizacao privacidade apagar dados camera limpar metadata strip privacy",
        "img_pdf" => "imagens para pdf juntar fotos criar pdf converter imagem em pdf jpg para pdf images to pdf",
        "ocr" => "ocr texto de imagem ler texto extrair texto copiar texto print reconhecer letras escrita digitalizar text recognition extract text",
        "v_convert" => "converter video formato mp4 mkv avi mov webm flv wmv transformar extensao convert video",
        "v_compress" => "comprimir video compactar reduzir tamanho diminuir peso mb whatsapp discord email compress video smaller",
        "v_trim" => "cortar video trecho aparar recortar parte clip pedaco inicio fim trim cut clip",
        "v_merge" => "juntar videos unir mesclar combinar concatenar emendar merge join combine",
        "v_speed" => "velocidade acelerar desacelerar camera lenta rapido devagar timelapse speed slow motion fast",
        "v_transform" => "girar video rotacionar virar espelhar recortar vertical reels tiktok shorts 9 16 quadrado rotate crop",
        "v_gif" => "gif animado video para gif converter gif meme animacao animated",
        "v_subs" => "legenda queimar legenda embutir legenda srt ass vtt colocar legenda subtitulo subtitles burn hardsub caption",
        "v_music" => "musica de fundo trilha sonora adicionar musica audio background music soundtrack",
        "v_audioframes" => "remover audio tirar som mudo silenciar extrair frames quadros fotos do video capturas mute remove audio frames screenshots",
        "screen" => "gravar tela gravador de tela captura de tela video da tela screen recorder record screen capture",
        "recorder" => "gravador de audio gravar audio gravar voz microfone mic voice recorder record audio",
        "transcribe" => "transcrever transcricao legendar legenda automatica fala para texto ditado whisper speech to text subtitles captions transcript",
        "separate" => "separar voz instrumental karaoke remover voz tirar voz isolar voz vocal acapella stem vocals instrumental remover",
        "a_convert" => "converter audio formato mp3 wav flac ogg m4a aac opus musica extrair audio convert audio",
        "a_trim" => "cortar audio trecho aparar recortar musica toque ringtone trim cut",
        "a_merge" => "juntar audios unir mesclar combinar musicas merge join",
        "a_normalize" => "normalizar volume aumentar volume nivelar som alto baixo loudness normalize",
        "a_compress" => "comprimir audio compactar reduzir tamanho mp3 bitrate podcast compress",
        "pdf_merge" => "juntar pdf unir pdfs mesclar combinar merge pdf",
        "pdf_split" => "dividir pdf separar paginas extrair paginas cortar pdf split",
        "pdf_compress" => "comprimir pdf reduzir tamanho diminuir pdf compactar compress pdf",
        "pdf_images" => "pdf para imagens pdf para jpg pdf para png converter pdf em imagem paginas pdf to image",
        "pdf_ocr" => "pdf pesquisavel ocr pdf escaneado digitalizado texto selecionar copiar texto do pdf searchable scanned",
        "pdf_protect" => "proteger pdf senha bloquear criptografar trancar password encrypt lock",
        "pdf_unlock" => "tirar senha desbloquear pdf remover senha destrancar unlock decrypt",
        "download" => "baixar download youtube musica video mp3 mp4 tiktok instagram twitter x facebook link url yt dlp",
        "presets" => "predefinicoes favoritos atalhos configuracoes salvas modelos presets templates",
        "info" => "informacoes do arquivo detalhes propriedades metadados resolucao duracao codec info details",
        "watch" => "pasta vigiada automatico automatizar monitorar converter automaticamente watch folder auto",
        "rename" => "renomear em lote mudar nome nomes arquivos sequencia rename batch",
        "qr" => "qr code qrcode codigo gerador pix wifi link barcode",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(q: &str, id: &str, label: &str) -> bool {
        matches(&normalize(q), &haystack(label, label, "", "", keywords(id)))
    }

    #[test]
    fn finds_by_synonyms() {
        assert!(hit("cortar", "v_trim", "Cortar trecho"));
        assert!(hit("aparar", "a_trim", "Cortar áudio"));
        assert!(hit("tirar fundo", "img_removebg", "Remover fundo"));
        assert!(hit("Legenda", "transcribe", "Transcrever / legendar"));
        assert!(hit("karaoke", "separate", "Separar voz e instrumental"));
        assert!(hit("karaokê", "separate", "Separar voz e instrumental"));
        assert!(hit("youtube", "download", "Vídeos e músicas"));
        assert!(hit("senha", "pdf_protect", "Proteger com senha"));
        // um erro de digitação
        assert!(hit("comprmir", "v_compress", "Comprimir"));
        assert!(hit("redimencionar", "img_resize", "Redimensionar"));
        // não acha o que não tem nada a ver
        assert!(!hit("senha", "v_trim", "Cortar trecho"));
        assert!(!hit("youtube", "pdf_merge", "Juntar PDFs"));
    }

    /// Com o menu de verdade: toda ferramenta tem palavras de busca, e buscas comuns acham o esperado.
    #[test]
    fn real_menu() {
        let find = |q: &str| -> Vec<String> {
            let qn = normalize(q);
            crate::NAV
                .iter()
                .flat_map(|(sec, items)| items.iter().map(move |(page, label)| (sec, page, label)))
                .filter(|(sec, page, label)| matches(&qn, &haystack(label, label, sec, sec, keywords(crate::page_id(**page)))))
                .map(|(sec, _, label)| format!("{sec}/{label}"))
                .collect()
        };
        for (_, items) in crate::NAV {
            for (page, label) in *items {
                assert!(!keywords(crate::page_id(*page)).is_empty(), "sem palavras de busca: {label}");
            }
        }
        for q in ["cortar", "tirar fundo", "juntar", "legenda", "comprimir video", "mp3", "senha", "gravar", "texto", "aumentar"] {
            println!("{q:>16}: {:?}", find(q));
        }
        assert!(find("cortar").iter().any(|r| r.contains("Cortar trecho")) && find("cortar").iter().any(|r| r.contains("Cortar áudio")));
        assert!(find("cortar").iter().any(|r| r.contains("Girar / recortar")));
        assert_eq!(find("comprimir video"), vec!["VÍDEO/Comprimir".to_string()]);
        assert!(find("tirar fundo") == vec!["IMAGEM/Remover fundo".to_string()]);
        assert!(find("xyzqwe").is_empty());
    }
}
