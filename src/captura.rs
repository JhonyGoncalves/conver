//! Só na compilação de depuração: abre telas, tira capturas da própria janela e fecha, para revisar o visual sem
//! clicar em nada. Não salva a configuração do usuário nem passa pela instância única.
//!
//! ```text
//! CONVER_CAPTURA=<pasta de saída>
//! CONVER_CENAS=img_resize:claro:pt,settings:claro:pt:900    (tela:tema:idioma[:rolagem]; tela = id de main::page_id,
//!                                                          "settings" ou "history")
//! CONVER_ARQUIVOS=a.png;b.mp4                              (opcional: entram como se fossem arrastados)
//! CONVER_BUSCA=cortar                                      (opcional: texto na busca do menu)
//! CONVER_ESPERA=300                                        (opcional: quadros de espera por cena; padrão 45)
//! CONVER_DEMO_AUDIO=1                                      (opcional: o gravador de áudio mostra um som gerado)
//! ```

use std::path::PathBuf;

/// Som de teste para o medidor do gravador: fala com pausas e um pico que estoura a cada 4 s.
pub const DEMO_AUDIO: &str = "aevalsrc='if(gt(mod(t,4),3.3),1.25,0.55)*sin(2*PI*220*t)*(0.25+0.75*abs(sin(2*PI*1.7*t)))*gt(mod(t,2.5),0.4)':s=16000";

pub struct Cena {
    pub page: String,
    pub dark: bool,
    pub lang: String,
    /// Rolagem da página, em pixels (4º campo da cena).
    pub scroll: Option<f32>,
}

pub struct Captura {
    pub dir: PathBuf,
    pub cenas: Vec<Cena>,
    pub atual: usize,
    pub quadros: u32,
    pub pedida: bool,
}

/// Quadros de espera antes de cada captura (miniaturas e durações carregam em segundo plano); `CONVER_ESPERA` muda.
pub fn espera() -> u32 {
    std::env::var("CONVER_ESPERA").ok().and_then(|v| v.parse().ok()).unwrap_or(45)
}

pub fn from_env() -> Option<Captura> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let dir = PathBuf::from(std::env::var_os("CONVER_CAPTURA")?);
    let cenas = std::env::var("CONVER_CENAS")
        .unwrap_or_else(|_| "img_resize:claro:pt".into())
        .split(',')
        .filter_map(|c| {
            let mut it = c.trim().split(':');
            Some(Cena {
                page: it.next()?.to_string(),
                dark: it.next().unwrap_or("claro") == "escuro",
                lang: it.next().unwrap_or("pt").to_string(),
                scroll: it.next().and_then(|v| v.parse().ok()),
            })
        })
        .collect::<Vec<_>>();
    if cenas.is_empty() {
        return None;
    }
    let _ = std::fs::create_dir_all(&dir);
    Some(Captura { dir, cenas, atual: 0, quadros: 0, pedida: false })
}

pub fn active() -> bool {
    cfg!(debug_assertions) && std::env::var_os("CONVER_CAPTURA").is_some()
}

pub fn arquivos() -> Vec<PathBuf> {
    std::env::var("CONVER_ARQUIVOS")
        .map(|s| s.split(';').filter(|p| !p.trim().is_empty()).map(|p| PathBuf::from(p.trim())).collect())
        .unwrap_or_default()
}

pub fn salvar(dir: &std::path::Path, cena: &Cena, n: usize, img: &eframe::egui::ColorImage) {
    let [w, h] = img.size;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
        let tema = if cena.dark { "escuro" } else { "claro" };
        let _ = buf.save(dir.join(format!("{n:02}_{}_{tema}_{}.png", cena.page, cena.lang)));
    }
}
