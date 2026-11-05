//! Modelos de IA que o Conver baixa depois de instalado: o catálogo em "pacotes" (o que cada um faz e quais
//! ferramentas usam), o que já está no computador, a fila de downloads em segundo plano e a remoção. Usado pela
//! tela de boas-vindas e por Configurações → Modelos de IA. Cada ferramenta continua baixando o que falta no
//! primeiro uso; isto só deixa a escolha à vista.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText, Ui};

use crate::ai::{self, ModelSpec};
use crate::job::{Job, LogKind};
use crate::theme::Palette;
use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Pack {
    Transcribe,
    Separate,
    Erase,
    Generate,
    ClassifyImage,
    ClassifySound,
    Language,
    Voice,
}

impl Pack {
    pub const ALL: [Pack; 8] =
        [Pack::Transcribe, Pack::Separate, Pack::Voice, Pack::Erase, Pack::Generate, Pack::ClassifyImage, Pack::ClassifySound, Pack::Language];

    /// Nome do pacote (chave de tradução).
    pub fn title(self) -> &'static str {
        match self {
            Pack::Transcribe => "Transcrição (Whisper)",
            Pack::Separate => "Separar voz e instrumental (UVR)",
            Pack::Erase => "Apagar objeto (LaMa)",
            Pack::Generate => "Gerar imagem (DreamShaper 8)",
            Pack::ClassifyImage => "Classificar imagens (SigLIP 2)",
            Pack::ClassifySound => "Classificar sons (CLAP)",
            Pack::Language => "IA de linguagem (Qwen 3.5 2B)",
            Pack::Voice => "Voz natural (Piper)",
        }
    }

    /// Onde ele é usado (chave de tradução).
    pub fn used_by(self) -> &'static str {
        match self {
            Pack::Transcribe => "Legendas, Transcrever, Ditado ao vivo e Traduzir e dublar",
            Pack::Separate => "Separar voz e instrumental",
            Pack::Erase => "Apagar objeto",
            Pack::Generate => "Gerar imagem (precisa de uma placa de vídeo razoável)",
            Pack::ClassifyImage => "Node \"IA local: classificar\" dos fluxos, com imagens e vídeos",
            Pack::ClassifySound => "Node \"IA local: classificar\" dos fluxos, com áudios",
            Pack::Language => "Traduzir legendas e classificar textos nos fluxos",
            Pack::Voice => "Texto para voz e dublagem, na voz do idioma do Conver",
        }
    }

    /// Os arquivos do pacote (a voz é a do idioma atual do Conver).
    pub fn files(self) -> Vec<ModelSpec> {
        match self {
            Pack::Transcribe => vec![crate::transcribe::WModel::Rapido.spec()],
            Pack::Separate => vec![crate::separate::MODEL],
            Pack::Erase => vec![crate::inpaint::MODEL],
            Pack::Generate => vec![crate::imagegen::MODEL],
            Pack::ClassifyImage => crate::classify::MODELS.iter().map(|m| **m).collect(),
            Pack::ClassifySound => crate::audioclass::MODELS.iter().map(|m| **m).collect(),
            Pack::Language => vec![crate::llm::RUNTIME, crate::llm::MODEL],
            Pack::Voice => {
                let v = self.voice();
                vec![v.onnx, v.json]
            }
        }
    }

    pub fn voice(self) -> &'static crate::tts::Voice {
        crate::tts::default_voice(crate::i18n::current().code)
    }

    /// Tamanho do download.
    pub fn size(self) -> u64 {
        self.files().iter().map(|f| f.size).sum()
    }

    pub fn installed(self) -> bool {
        match self {
            Pack::Language => crate::llm::installed(),
            _ => self.files().iter().all(ai::is_installed),
        }
    }

    /// Arquivos que o Conver baixou (nos dados do usuário): só esses podem ser apagados.
    fn downloaded(self) -> Vec<PathBuf> {
        let dir = tools::app_dir().join("modelos");
        let mut out: Vec<PathBuf> = self.files().iter().map(|f| dir.join(f.file)).filter(|p| p.is_file()).collect();
        if self == Pack::Language {
            if let Ok(rd) = std::fs::read_dir(crate::llm::runtime_dir()) {
                out.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_file()));
            }
        }
        out
    }

    /// Espaço que ocupa no disco (o que foi baixado).
    pub fn disk(self) -> u64 {
        self.downloaded().iter().map(|p| tools::file_size(p)).sum()
    }

    pub fn removable(self) -> bool {
        !self.downloaded().is_empty()
    }

    /// Baixa o que falta (de 0 a 1 no progresso, pelo tamanho).
    pub fn ensure(self, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        if self == Pack::Language {
            return crate::llm::ensure(on_progress, cancelled).map(|_| ());
        }
        let files = self.files();
        let total = files.iter().map(|f| f.size).sum::<u64>().max(1) as f32;
        let mut done = 0u64;
        for f in &files {
            let (base, part) = (done as f32 / total, f.size as f32 / total);
            ai::ensure_model(f, &|p| on_progress(base + p * part), cancelled)?;
            done += f.size;
        }
        Ok(())
    }

    /// Apaga o que foi baixado. A ferramenta baixa de novo se for usada.
    pub fn remove(self) -> Result<(), String> {
        for p in self.downloaded() {
            std::fs::remove_file(&p).map_err(|_| format!("não foi possível apagar {} (está em uso?).", tools::file_name(&p)))?;
        }
        if self == Pack::Language {
            let _ = std::fs::remove_dir(crate::llm::runtime_dir());
        }
        Ok(())
    }
}

/// Para que a pessoa vai usar o Conver (tela de boas-vindas): cada finalidade sugere alguns pacotes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Purpose {
    Convert,
    Social,
    Audio,
    Docs,
    Automation,
    Create,
}

impl Purpose {
    pub const ALL: [Purpose; 6] = [Purpose::Convert, Purpose::Social, Purpose::Audio, Purpose::Docs, Purpose::Automation, Purpose::Create];

    pub fn title(self) -> &'static str {
        match self {
            Purpose::Convert => "Converter e comprimir",
            Purpose::Social => "Vídeos para redes sociais",
            Purpose::Audio => "Podcast e áudio",
            Purpose::Docs => "Documentos e PDF",
            Purpose::Automation => "Automação com fluxos",
            Purpose::Create => "Criar com IA",
        }
    }

    pub fn examples(self) -> &'static str {
        match self {
            Purpose::Convert => "Imagens, vídeos e áudios em outro formato ou menores",
            Purpose::Social => "Legendas, cortes, vídeo vertical, tradução",
            Purpose::Audio => "Melhorar voz, transcrever, separar voz e música",
            Purpose::Docs => "Juntar e dividir PDFs, OCR, tabelas para Excel",
            Purpose::Automation => "Separar fotos, sons e textos por assunto sozinho",
            Purpose::Create => "Gerar imagem, apagar objeto, narrar um texto",
        }
    }

    pub fn suggests(self) -> &'static [Pack] {
        match self {
            Purpose::Convert | Purpose::Docs => &[],
            Purpose::Social => &[Pack::Transcribe],
            Purpose::Audio => &[Pack::Transcribe, Pack::Separate],
            Purpose::Automation => &[Pack::ClassifyImage, Pack::ClassifySound, Pack::Language],
            Purpose::Create => &[Pack::Generate, Pack::Erase, Pack::Voice],
        }
    }
}

/// O que já vem no download do Conver (para a tela de boas-vindas dizer que não precisa baixar).
pub const BUNDLED: &str = "Já vêm com o Conver, sem baixar nada: remover fundo, ampliar com IA, remover ruído, desfocar rostos e OCR.";

// ------------------------------------------------------------------ fila de downloads

/// Downloads em segundo plano, um pacote de cada vez. Compartilhado entre as telas.
#[derive(Clone, Default)]
pub struct Downloads {
    pub job: Job,
    queue: Arc<Mutex<Fila>>,
}

#[derive(Default)]
struct Fila {
    waiting: VecDeque<Pack>,
    current: Option<Pack>,
    working: bool,
}

impl Downloads {
    /// Põe pacotes na fila (os que já estão no computador ou na fila ficam de fora).
    pub fn add(&self, ctx: &egui::Context, packs: &[Pack]) {
        let start = {
            let mut q = self.queue.lock().unwrap();
            for p in packs {
                if !p.installed() && q.current != Some(*p) && !q.waiting.contains(p) {
                    q.waiting.push_back(*p);
                }
            }
            let start = !q.working && !q.waiting.is_empty();
            if start {
                q.working = true;
            }
            start
        };
        if !start {
            return;
        }
        let queue = self.queue.clone();
        self.job.start(ctx, move |ctl| {
            let mut ok = 0;
            loop {
                let next = {
                    let mut q = queue.lock().unwrap();
                    q.current = q.waiting.pop_front();
                    if q.current.is_none() {
                        q.working = false;
                    }
                    q.current
                };
                let Some(pack) = next else { break };
                if ctl.cancelled() {
                    queue.lock().unwrap().waiting.clear();
                    continue;
                }
                let name = crate::i18n::tr(pack.title());
                ctl.status(format!("{} {name}…", crate::i18n::tr("Baixando")));
                ctl.progress(0.0);
                match pack.ensure(&|p| ctl.progress(p), &|| ctl.cancelled()) {
                    Ok(()) => {
                        ok += 1;
                        ctl.log(LogKind::Ok, format!("✔ {name}"));
                    }
                    Err(e) => ctl.log(LogKind::Err, format!("✖ {name}: {}", crate::i18n::tr(e))),
                }
            }
            ctl.progress(1.0);
            ctl.status(if ctl.cancelled() { "Cancelado.".to_string() } else { crate::i18n::tr(format!("{ok} pacote(s) baixado(s).")) });
        });
    }

    // i18n: fora
    /// Modo de captura: finge um download em andamento (sem baixar nada) para revisar a interface.
    pub fn demo(&self) {
        let mut q = self.queue.lock().unwrap();
        q.working = true;
        q.current = Some(Pack::Generate);
        q.waiting = [Pack::ClassifyImage, Pack::ClassifySound].into_iter().collect();
        let mut st = self.job.state();
        st.progress = 0.6;
        st.log = vec![(LogKind::Ok, "✔ Apagar objeto (LaMa)".into()), (LogKind::Ok, "✔ Voz natural (Piper)".into())];
    }

    // i18n: dentro

    pub fn current(&self) -> Option<Pack> {
        self.queue.lock().unwrap().current
    }

    pub fn queued(&self, p: Pack) -> bool {
        let q = self.queue.lock().unwrap();
        q.current == Some(p) || q.waiting.contains(&p)
    }

    pub fn busy(&self) -> bool {
        self.queue.lock().unwrap().working
    }

    /// Progresso do pacote baixando agora (0 a 1).
    pub fn progress(&self) -> f32 {
        self.job.state().progress
    }
}

/// Dica sob uma ferramenta: o pacote já está no computador ou será baixado no primeiro uso.
pub fn hint(ui: &mut Ui, p: &Palette, pack: Pack) {
    if pack.installed() {
        ui.label(RichText::new(crate::i18n::tr(format!("✔ {} pronto neste computador.", crate::i18n::tr(pack.title())))).size(12.5).color(p.muted));
    } else {
        crate::widgets::hint(
            ui,
            p,
            &format!(
                "{} ({}) é baixado uma vez, na primeira vez que você usar, ou agora em Configurações → Modelos de IA.",
                crate::i18n::tr(pack.title()),
                tools::human_size(pack.size())
            ),
        );
    }
}
