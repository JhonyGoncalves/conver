//! Fluxos de automação por nodes: cada node é uma ferramenta do Conver (`ops::TOOLS`), uma etapa de IA, um filtro,
//! a entrada ou a saída. As ligações levam o resultado de um node (arquivos e textos) para o próximo.
//!
//! Os fluxos ficam em `fluxos/<nome>.json`, na pasta de dados, e são os mesmos para a tela de automação, a linha de
//! comando e o MCP. Cada execução roda numa pasta própria (`fluxos/execucoes/…`): cada node de ferramenta grava ali
//! e tudo o que ele criou passa para os nodes seguintes, o que funciona até com ferramentas que geram vários
//! arquivos (PDF → imagens, separar voz…).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::ai_api::{self, Part};
use crate::{imaging, ops, tools};

// ================================================================== formato

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Flow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
    /// Rodar sozinho: pasta vigiada e/ou horário.
    #[serde(default, skip_serializing_if = "crate::triggers::Trigger::is_empty")]
    pub trigger: crate::triggers::Trigger,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: u32,
    /// "input", "text", "tool:<nome da ferramenta>", "ai_prompt", "ai_gate", "filter" ou "output".
    pub kind: String,
    /// Nome mostrado no node (vazio = o nome do tipo).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub args: Map<String, Value>,
    /// Posição no editor.
    #[serde(default)]
    pub pos: [f32; 2],
}

fn out_port() -> String {
    "out".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Link {
    pub from: u32,
    /// Saída usada: "out" ou, nos nodes que separam, "yes"/"no".
    #[serde(default = "out_port")]
    pub port: String,
    pub to: u32,
}

impl Flow {
    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn next_id(&self) -> u32 {
        self.nodes.iter().map(|n| n.id).max().unwrap_or(0) + 1
    }

    pub fn remove_node(&mut self, id: u32) {
        self.nodes.retain(|n| n.id != id);
        self.links.retain(|l| l.from != id && l.to != id);
    }

    /// A ligação `from → to` criaria um ciclo?
    pub fn would_cycle(&self, from: u32, to: u32) -> bool {
        if from == to {
            return true;
        }
        // existe caminho de `to` até `from`?
        let mut stack = vec![to];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == from {
                return true;
            }
            if seen.insert(n) {
                stack.extend(self.links.iter().filter(|l| l.from == n).map(|l| l.to));
            }
        }
        false
    }

    /// Ordem de execução (cada node depois dos que ligam nele).
    pub fn order(&self) -> Result<Vec<u32>, String> {
        let mut indeg: HashMap<u32, usize> = self.nodes.iter().map(|n| (n.id, 0)).collect();
        for l in &self.links {
            *indeg.entry(l.to).or_default() += 1;
        }
        // na ordem em que aparecem, para a execução ser previsível
        let mut ready: Vec<u32> = self.nodes.iter().map(|n| n.id).filter(|id| indeg[id] == 0).collect();
        let mut out = Vec::new();
        while !ready.is_empty() {
            let id = ready.remove(0);
            out.push(id);
            for l in self.links.iter().filter(|l| l.from == id) {
                let d = indeg.get_mut(&l.to).unwrap();
                *d -= 1;
                if *d == 0 {
                    ready.push(l.to);
                }
            }
        }
        if out.len() != self.nodes.len() {
            return Err("o fluxo tem um ciclo (um node volta para um anterior).".into());
        }
        Ok(out)
    }

    /// Confere tipos, ferramentas, ligações e ciclos. Devolve todos os problemas encontrados.
    pub fn validate(&self) -> Result<(), String> {
        let mut errs = Vec::new();
        if self.name.trim().is_empty() {
            errs.push("o fluxo precisa de um nome.".to_string());
        }
        let mut ids = HashSet::new();
        for n in &self.nodes {
            if !ids.insert(n.id) {
                errs.push(format!("há dois nodes com o id {}.", n.id));
            }
            if kind_info(&n.kind).is_none() {
                errs.push(format!("node {}: tipo desconhecido \"{}\" (veja flow_node_types).", n.id, n.kind));
            }
        }
        for l in &self.links {
            match (self.node(l.from), self.node(l.to)) {
                (Some(a), Some(b)) => {
                    if kind_info(&a.kind).is_some() {
                        let outs = node_outputs(a);
                        if !outs.contains(&l.port) {
                            errs.push(format!("ligação {} → {}: o node {} não tem a saída \"{}\" (tem: {}).", l.from, l.to, l.from, l.port, outs.join(", ")));
                        }
                    }
                    if let Some(k) = kind_info(&b.kind) {
                        if !k.has_input {
                            errs.push(format!("ligação {} → {}: o node {} não recebe entradas.", l.from, l.to, l.to));
                        }
                    }
                }
                _ => errs.push(format!("ligação {} → {}: node inexistente.", l.from, l.to)),
            }
        }
        if let Err(e) = self.order() {
            errs.push(e);
        }
        if errs.is_empty() { Ok(()) } else { Err(errs.join(" ")) }
    }
}

// ============================================================ tipos de node

#[derive(Clone)]
pub struct KindInfo {
    pub kind: String,
    /// Nome em português.
    pub title: String,
    /// Grupo na lista do editor.
    pub category: &'static str,
    /// Em inglês, para agentes.
    pub description: String,
    pub outputs: &'static [&'static str],
    pub has_input: bool,
    /// Argumentos (JSON Schema), sem "files" e "out_dir", que o fluxo preenche.
    pub schema: Value,
}

/// Ferramentas que não fazem sentido dentro de um fluxo.
const HIDDEN_TOOLS: &[&str] = &["list_devices", "preset_list", "preset_save"];

fn tool_category(name: &str) -> &'static str {
    if name.starts_with("image") || name == "qr_code" {
        "IMAGEM"
    } else if name.starts_with("video") || name.starts_with("media") || name == "screen_record" {
        "VÍDEO"
    } else if name.starts_with("audio") || name == "transcribe" || name == "text_to_speech" {
        "ÁUDIO"
    } else if name.starts_with("pdf") {
        "PDF"
    } else if name == "download" {
        "DOWNLOAD"
    } else {
        "FERRAMENTAS"
    }
}

fn strip_schema(mut s: Value) -> Value {
    if let Some(p) = s["properties"].as_object_mut() {
        p.remove("files");
        p.remove("file");
        p.remove("out_dir");
    }
    if let Some(r) = s["required"].as_array_mut() {
        r.retain(|v| v != "files" && v != "out_dir");
    }
    s
}

fn special(kind: &str, title: &str, category: &'static str, desc: &str, outputs: &'static [&'static str], has_input: bool, schema: Value) -> KindInfo {
    KindInfo { kind: kind.into(), title: title.into(), category, description: desc.into(), outputs, has_input, schema }
}

// nomes dos tipos especiais (em português: são as chaves de tradução)
const T_INPUT: &str = "Arquivos de entrada";
const T_TEXT: &str = "Texto";
const T_OUTPUT: &str = "Salvar em pasta";
const T_AI_PROMPT: &str = "IA: instrução";
const T_AI_GATE: &str = "IA: avaliar";
const T_FILTER: &str = "Filtrar arquivos";
const T_CLASSIFY: &str = "IA local: classificar";
const T_RENAME: &str = "Renomear com padrão";
const T_CONDITION: &str = "Condição";
const T_WEBHOOK: &str = "Chamar webhook";
const T_CLOUD: &str = "Enviar para a nuvem";

/// Saída do classificador para o que ficou abaixo da confiança mínima (ou não é imagem nem vídeo).
pub const UNCERTAIN: &str = "uncertain";

/// As saídas de um node: as fixas do tipo ou, no classificador, uma por opção mais "uncertain".
pub fn node_outputs(n: &Node) -> Vec<String> {
    if n.kind == "classify" {
        let mut v = crate::classify::parse_options(n.args.get("options").and_then(Value::as_str).unwrap_or_default());
        v.push(UNCERTAIN.into());
        return v;
    }
    kind_info(&n.kind).map(|k| k.outputs.iter().map(|o| o.to_string()).collect()).unwrap_or_else(|| vec!["out".into()])
}

/// Largura e altura de uma imagem ou vídeo.
fn dims_of(p: &Path) -> Option<(u32, u32)> {
    if crate::imaging::IMAGE_EXTS.contains(&tools::ext_lower(p).as_str()) {
        if let Ok(d) = image::image_dimensions(p) {
            return Some(d);
        }
    }
    crate::video::probe(p).size
}

#[cfg(test)]
pub(crate) fn condition_passes_for_test(p: &Path, check: &str, more: bool, value: f64) -> bool {
    condition_passes(p, check, more, value, "vertical")
}

/// O node "Condição": o arquivo tem a propriedade pedida?
fn condition_passes(p: &Path, check: &str, more: bool, value: f64, orientation: &str) -> bool {
    let cmp = |x: f64| if more { x > value } else { x < value };
    match check {
        "orientation" => dims_of(p).is_some_and(|(w, h)| match orientation {
            "horizontal" => w > h,
            "square" => w == h,
            _ => h > w,
        }),
        "width" => dims_of(p).is_some_and(|(w, _)| cmp(w as f64)),
        "height" => dims_of(p).is_some_and(|(_, h)| cmp(h as f64)),
        "duration" => crate::video::probe(p).duration.is_some_and(cmp),
        "size_mb" => cmp(tools::file_size(p) as f64 / 1_048_576.0),
        "pages" => tools::ext_lower(p) == "pdf" && crate::pdf::open(p).is_ok_and(|d| cmp(d.get_pages().len() as f64)),
        "has_audio" => crate::video::probe(p).has_audio,
        _ => false,
    }
}

/// Todos os tipos de node: entrada, texto, IA, filtro, saída e uma para cada ferramenta.
// i18n: fora
pub fn kinds() -> Vec<KindInfo> {
    let s = |props: Value, req: &[&str]| json!({ "type": "object", "properties": props, "required": req });
    let mut v = vec![
        special("input", T_INPUT, "ENTRADA E SAÍDA",
            "Start of the flow: the files to process. When the flow runs from the CLI/MCP with \"files\", those replace the ones saved here.",
            &["out"], false,
            s(json!({ "files": { "type": "array", "items": { "type": "string" }, "description": "Files or folders (folders are expanded)." } }), &[])),
        special("text", T_TEXT, "ENTRADA E SAÍDA",
            "A fixed text passed on to the next nodes (e.g. links for download, one per line, or a text for the QR code).",
            &["out"], false,
            s(json!({ "text": { "type": "string", "description": "The text." } }), &["text"])),
        special("cloud", T_CLOUD, "ENTRADA E SAÍDA",
            "Copy (or move) the files that arrive to the synced folder of OneDrive, Google Drive or Dropbox on this PC; the service's own app uploads them. The copies go on to the next nodes.",
            &["out"], true,
            s(json!({ "service": { "type": "string", "enum": ["auto", "onedrive", "google_drive", "dropbox"], "default": "auto", "description": "Which cloud (auto: the first one found)." },
                "subfolder": { "type": "string", "default": "Conver", "description": "Folder inside the cloud, e.g. \"Conver/Reels\"." },
                "move": { "type": "boolean", "default": false, "description": "Move instead of copy." } }), &[])),
        special("webhook", T_WEBHOOK, "ENTRADA E SAÍDA",
            "Call a webhook URL (n8n, Zapier, Make, Discord…) with what arrives: as JSON (name, path, size, text of each item) or sending each file (multipart form with \"file\" and the message in \"content\", the Discord format). Items pass on unchanged.",
            &["out"], true,
            s(json!({ "url": { "type": "string", "description": "Webhook URL (https://…)." },
                "send": { "type": "string", "enum": ["json", "file"], "default": "json", "description": "json: one call with the details of all items; file: one call per file, with the file attached." },
                "message": { "type": "string", "description": "Optional message (\"content\" field; for Discord, the text of the post)." } }), &["url"])),
        special("output", T_OUTPUT, "ENTRADA E SAÍDA",
            "End of the flow: copies (or moves) the files that arrive to a folder; texts are saved as .txt.",
            &["out"], true,
            s(json!({ "folder": { "type": "string", "description": "Destination folder (default: Documents\\Conver\\<flow name>)." },
                "move": { "type": "boolean", "default": false, "description": "Move instead of copy." },
                "save_texts": { "type": "boolean", "default": true, "description": "Save the texts that arrive as .txt files." } }), &[])),
        special("ai_prompt", T_AI_PROMPT, "IA",
            "Ask the AI (cloud API set in Settings) to do something with what arrives: describe, summarize, translate, write captions… Images are sent as pictures, text files as text, other files as their details. The answer becomes a text for the next nodes (and optionally a file).",
            &["out"], true,
            s(json!({ "prompt": { "type": "string", "description": "What the AI should do. Use {text} for the incoming text and {file} for the file name." },
                "per_item": { "type": "boolean", "default": true, "description": "One answer per file/text (false: one answer for everything together)." },
                "keep_files": { "type": "boolean", "default": true, "description": "Pass the incoming files on together with the answer." },
                "save_as": { "type": "string", "enum": ["none", "txt", "md", "srt", "json", "csv"], "default": "none", "description": "Also save each answer as a file with this extension." } }), &["prompt"])),
        special("ai_gate", T_AI_GATE, "IA",
            "The AI checks each file/text against a criterion and sends it to the \"yes\" or \"no\" output (e.g. approve the background removal, keep only photos with people, detect bad audio).",
            &["yes", "no"], true,
            s(json!({ "criteria": { "type": "string", "description": "What makes an item pass, e.g. \"the background was removed without cutting the person\"." } }), &["criteria"])),
        special("filter", T_FILTER, "FERRAMENTAS",
            "Send items to \"yes\" or \"no\" by type, extension or size, without AI.",
            &["yes", "no"], true,
            s(json!({ "types": { "type": "array", "items": { "type": "string", "enum": ["image", "video", "audio", "pdf", "text"] }, "description": "Accepted kinds (empty = any)." },
                "extensions": { "type": "string", "description": "Accepted extensions, e.g. \"png, jpg\" (empty = any)." },
                "min_mb": { "type": "number", "description": "Minimum size in MB." }, "max_mb": { "type": "number", "description": "Maximum size in MB." } }), &[])),
        special("condition", T_CONDITION, "FERRAMENTAS",
            "Send items to \"yes\" or \"no\" by a property of the file: orientation, duration, width, height, size, number of PDF pages or whether it has sound. Items without that property (texts, or a duration of an image) go to \"no\".",
            &["yes", "no"], true,
            s(json!({ "check": { "type": "string", "enum": ["orientation", "duration", "width", "height", "size_mb", "pages", "has_audio"], "default": "orientation", "description": "What to check." },
                "orientation": { "type": "string", "enum": ["vertical", "horizontal", "square"], "default": "vertical", "description": "For check = orientation: the orientation that goes to \"yes\"." },
                "compare": { "type": "string", "enum": ["more", "less"], "default": "more", "description": "For numeric checks: \"yes\" when the value is more (or less) than \"value\"." },
                "value": { "type": "number", "description": "Number to compare with (seconds, pixels, MB or pages)." } }), &[])),
        special("rename", T_RENAME, "FERRAMENTAS",
            "Copy each file that arrives with a new name from a pattern (the originals are not touched). Tokens: {name} original name, {n} counter, {option} option chosen by the local classifier, {date} today (YYYY-MM-DD), {time} (HH-MM), {file_date} date the file was modified, {folder} name of its folder. Portuguese aliases also work: {nome}, {opcao}, {data}, {hora}, {data_arquivo}, {pasta}. The extension is kept.",
            &["out"], true,
            s(json!({ "pattern": { "type": "string", "default": "{name}_{n}", "description": "Name pattern without extension, e.g. \"{option}_{date}_{n}\"." },
                "start": { "type": "integer", "default": 1, "description": "First number of {n}." },
                "digits": { "type": "integer", "default": 3, "description": "Digits of {n} (zeros on the left)." },
                "count_per_option": { "type": "boolean", "default": true, "description": "{n} restarts for each option of the classifier (cat_001, cat_002, dog_001…)." } }), &[])),
        special("classify", T_CLASSIFY, "IA",
            "Local AI, offline: compares each item with the options and sends it to the output named after the most likely option. Images and video frames use SigLIP 2 (380 MB), sounds use CLAP (246 MB) and texts (text files, PDFs, texts from previous nodes) use a local language model (Qwen 3.5 2B, 1.3 GB); each is downloaded on first use. Items below min_confidence, or that the chosen look_at does not cover, go to \"uncertain\". The outputs are one per option (link with \"port\": \"<option>\") plus \"uncertain\". Options can be written in Portuguese, English and 100+ other languages. Short, concrete options work best (\"cat\", \"receipt\", \"beach\"); an option like \"person or animal\" scores lower than separate options linked to the same next node.",
            &[UNCERTAIN], true,
            s(json!({ "options": { "type": "string", "description": "The options, one per line or separated by commas, e.g. \"documento, selfie, paisagem, comida\"." },
                "min_confidence": { "type": "number", "default": 50, "description": "Minimum confidence in % (0-100) for the best option; below it the item goes to \"uncertain\"." },
                "look_at": { "type": "string", "enum": ["auto", "image", "sound", "text"], "default": "auto", "description": "What to judge: auto (by file type: image, audio → sound, video → a frame, text/PDF → text), image, sound (audio of audios and videos) or text." } }), &["options"])),
    ];
    for t in ops::TOOLS.iter().filter(|t| !HIDDEN_TOOLS.contains(&t.name) && !t.name.starts_with("flow_")) {
        let schema = (t.schema)();
        let has_input = schema["properties"].get("files").is_some() || schema["properties"].get("urls").is_some() || schema["properties"].get("text").is_some();
        v.push(KindInfo {
            kind: format!("tool:{}", t.name),
            title: t.title.to_string(),
            category: tool_category(t.name),
            description: t.description.to_string(),
            outputs: &["out"],
            has_input,
            schema: strip_schema(schema),
        });
    }
    v
}

// i18n: dentro
pub fn kind_info(kind: &str) -> Option<KindInfo> {
    kinds().into_iter().find(|k| k.kind == kind)
}

// ================================================================ armazenamento

pub fn dir() -> PathBuf {
    tools::app_dir().join("fluxos")
}

fn slug(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() { "fluxo".into() } else { s.chars().take(80).collect() }
}

pub fn path_of(name: &str) -> PathBuf {
    dir().join(format!("{}.json", slug(name)))
}

pub fn list() -> Vec<Flow> {
    let mut out: Vec<Flow> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| serde_json::from_str::<Flow>(&std::fs::read_to_string(p).ok()?).ok())
        .collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

pub fn find(name: &str) -> Option<Flow> {
    let n = name.trim().to_lowercase();
    list().into_iter().find(|f| f.name.to_lowercase() == n)
}

pub fn save(flow: &Flow) -> Result<(), String> {
    flow.validate()?;
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(flow).map_err(|e| e.to_string())?;
    std::fs::write(path_of(&flow.name), text).map_err(|e| format!("não foi possível salvar o fluxo: {e}"))
}

/// Salva sem validar (o editor guarda o trabalho em andamento, mesmo incompleto).
pub fn save_draft(flow: &Flow) -> Result<(), String> {
    std::fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(flow).map_err(|e| e.to_string())?;
    std::fs::write(path_of(&flow.name), text).map_err(|e| format!("não foi possível salvar o fluxo: {e}"))
}

pub fn remove(name: &str) -> Result<(), String> {
    let p = path_of(name);
    if !p.is_file() {
        return Err(format!("fluxo não encontrado: {name}"));
    }
    // a tarefa agendada (se houver) vai junto
    if find(name).is_some_and(|f| !f.trigger.time.trim().is_empty()) {
        crate::triggers::unschedule(name);
    }
    // as pastas das execuções (histórico) do fluxo também
    for d in run_dirs(name) {
        let _ = std::fs::remove_dir_all(d);
    }
    std::fs::remove_file(p).map_err(|e| e.to_string())
}

/// Fluxos de exemplo, criados na primeira vez que a tela de automação abre.
// i18n: fora
pub fn examples() -> Vec<Flow> {
    let n = |id: u32, kind: &str, title: &str, args: Value, x: f32, y: f32| Node {
        id,
        kind: kind.into(),
        title: title.into(),
        args: args.as_object().cloned().unwrap_or_default(),
        pos: [x, y],
    };
    let l = |from: u32, port: &str, to: u32| Link { from, port: port.into(), to };
    vec![
        Flow {
            name: "Exemplo - fotos sem fundo revisadas pela IA".into(),
            description: "Tira o fundo das fotos e pede para a IA conferir cada uma: as aprovadas vão para uma pasta, as outras para revisão.".into(),
            nodes: vec![
                n(1, "input", "", json!({}), 0.0, 100.0),
                n(2, "tool:image_remove_background", "", json!({ "background": "transparent" }), 250.0, 100.0),
                n(3, "ai_gate", "A IA confere o recorte", json!({ "criteria": "O fundo foi removido por completo, sem cortar partes da pessoa ou do produto?" }), 500.0, 100.0),
                n(4, "output", "Aprovadas", json!({}), 750.0, 20.0),
                n(5, "output", "Para revisar", json!({}), 750.0, 190.0),
            ],
            links: vec![l(1, "out", 2), l(2, "out", 3), l(3, "yes", 4), l(3, "no", 5)],
            ..Default::default()
        },
        Flow {
            name: "Exemplo - transcrever e resumir com IA".into(),
            description: "Transcreve vídeos ou áudios e pede para a IA um resumo com título e tópicos, salvo em Markdown ao lado da transcrição.".into(),
            nodes: vec![
                n(1, "input", "", json!({}), 0.0, 100.0),
                n(2, "tool:transcribe", "", json!({ "formats": ["txt"] }), 250.0, 100.0),
                n(3, "ai_prompt", "Resumo", json!({ "prompt": "Escreva um título, um resumo de 3 frases e os 5 pontos principais desta transcrição, em português.", "save_as": "md" }), 500.0, 100.0),
                n(4, "output", "", json!({}), 750.0, 100.0),
            ],
            links: vec![l(1, "out", 2), l(2, "out", 3), l(3, "out", 4)],
            ..Default::default()
        },
    ]
}

/// Fluxo grande de demonstração, só para capturas (`CONVER_FLUXO=vitrine`): nunca é gravado no disco.
pub fn showcase() -> Flow {
    let n = |id: u32, kind: &str, title: &str, args: Value, x: f32, y: f32| Node {
        id,
        kind: kind.into(),
        title: title.into(),
        args: args.as_object().cloned().unwrap_or_default(),
        pos: [x, y],
    };
    let l = |from: u32, port: &str, to: u32| Link { from, port: port.into(), to };
    Flow {
        name: "Podcast para redes sociais".into(),
        description: "Corta os episódios, limpa a voz, deixa na vertical com legendas animadas e logo, gera a versão dublada em inglês e escreve título e descrição.".into(),
        nodes: vec![
            n(1, "input", "Episódios", json!({}), 0.0, 180.0),
            n(2, "tool:media_trim", "Cortes do episódio", json!({ "cuts_file": "auto", "cut_mode": "smart" }), 250.0, 180.0),
            n(3, "tool:audio_enhance_voice", "Melhorar voz", json!({}), 500.0, 180.0),
            n(4, "condition", "Já é vertical?", json!({ "check": "orientation", "orientation": "vertical" }), 750.0, 180.0),
            n(5, "tool:video_transform", "Deixar em 9:16", json!({ "aspect": "9:16" }), 1000.0, 300.0),
            n(6, "tool:video_captions", "Legendas animadas", json!({ "preset": "reels" }), 1250.0, 180.0),
            n(7, "tool:watermark", "Logo do canal", json!({ "text": "@martelinho", "position": "bottom_right" }), 1500.0, 180.0),
            n(8, "tool:video_compress", "Até 50 MB", json!({ "target_mb": 50 }), 1750.0, 180.0),
            n(9, "output", "Reels e Shorts", json!({}), 2000.0, 180.0),
            n(10, "tool:transcribe", "Transcrever", json!({ "formats": ["srt"] }), 750.0, 470.0),
            n(11, "tool:subtitle_translate", "Dublar em inglês", json!({ "to": "en", "dub": true }), 1000.0, 560.0),
            n(12, "output", "Versão em inglês", json!({}), 1250.0, 560.0),
            n(13, "ai_prompt", "Título e descrição", json!({ "prompt": "Escreva um título curto e uma descrição com 3 hashtags para este corte.", "save_as": "md" }), 1000.0, 420.0),
            n(14, "output", "Textos para postar", json!({}), 1250.0, 420.0),
        ],
        links: vec![
            l(1, "out", 2), l(2, "out", 3), l(3, "out", 4), l(4, "yes", 6), l(4, "no", 5), l(5, "out", 6), l(6, "out", 7),
            l(7, "out", 8), l(8, "out", 9), l(3, "out", 10), l(10, "out", 13), l(13, "out", 14), l(10, "out", 11), l(11, "out", 12),
        ],
        ..Default::default()
    }
}

// i18n: dentro
/// Cria os exemplos se ainda não há nenhum fluxo.
pub fn ensure_examples() {
    if list().is_empty() {
        for f in examples() {
            let _ = save(&f);
        }
    }
}

// ================================================================== execução

/// O que passa de um node para outro: um arquivo ou um texto (ou os dois: um texto ligado a um arquivo).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Item {
    pub path: Option<PathBuf>,
    pub text: Option<String>,
    /// A opção escolhida pelo classificador (segue com o arquivo pelos nodes seguintes; usada pelo renomear).
    pub tag: Option<String>,
    /// O arquivo de entrada de onde este item veio (para "repetir só o que falhou").
    pub origin: Option<PathBuf>,
}

impl Item {
    fn file(p: PathBuf) -> Self {
        Item { path: Some(p), ..Default::default() }
    }

    fn text(t: String) -> Self {
        Item { text: Some(t), ..Default::default() }
    }

    pub fn label(&self) -> String {
        match (&self.path, &self.text) {
            (Some(p), _) => tools::file_name(p),
            (None, Some(t)) => {
                let s: String = t.chars().take(40).collect();
                format!("\"{}{}\"", s.replace('\n', " "), if t.chars().count() > 40 { "…" } else { "" })
            }
            _ => "?".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Running(f32),
    Done(String),
    Skipped(String),
    Failed(String),
}

/// Estado de uma execução, lido pela tela enquanto roda.
#[derive(Default)]
pub struct RunState {
    pub status: HashMap<u32, Status>,
    pub work_dir: Option<PathBuf>,
}

/// Entradas vindas de fora (linha de comando, MCP): substituem as salvas nos nodes de entrada.
#[derive(Default, Clone)]
pub struct Inputs {
    pub files: Vec<PathBuf>,
    pub texts: Vec<String>,
}

fn expand(paths: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for s in paths {
        let p = PathBuf::from(s.trim());
        if p.is_dir() {
            let mut inner: Vec<PathBuf> = std::fs::read_dir(&p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
            inner.sort();
            out.extend(inner);
        } else if p.is_file() {
            out.push(p);
        }
    }
    out
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn template(s: &str, item: Option<&Item>, all_text: &str) -> String {
    let text = item.and_then(|i| i.text.clone()).unwrap_or_else(|| all_text.to_string());
    let file = item.and_then(|i| i.path.as_ref()).map(|p| tools::file_name(p)).unwrap_or_default();
    s.replace("{text}", &text).replace("{texto}", &text).replace("{file}", &file).replace("{arquivo}", &file)
}

/// Registro de cada execução, dentro da pasta dela.
pub const RUN_RECORD: &str = "execucao.json";

/// Pastas de execução de um fluxo, da mais nova para a mais antiga.
fn run_dirs(name: &str) -> Vec<PathBuf> {
    let prefix = format!("{}_", slug(name));
    let root = dir().join("execucoes");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix)))
        .collect();
    v.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
    v
}

/// As últimas execuções de um fluxo (o registro de cada uma, com "work_dir").
pub fn history(name: &str) -> Vec<Value> {
    run_dirs(name)
        .into_iter()
        .filter_map(|d| {
            let text = std::fs::read_to_string(d.join(RUN_RECORD)).ok()?;
            let mut v: Value = serde_json::from_str(&text).ok()?;
            v["work_dir"] = json!(d.display().to_string());
            Some(v)
        })
        .collect()
}

/// As entradas que falharam na última execução (e que ainda existem).
pub fn last_failures(name: &str) -> Vec<PathBuf> {
    history(name)
        .first()
        .and_then(|r| r["failed_inputs"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(PathBuf::from))
        .filter(|p| p.exists())
        .collect()
}

/// Pasta da execução: `fluxos/execucoes/<fluxo>_<data>`. Guarda as 15 mais recentes de cada fluxo.
fn new_work_dir(flow: &Flow) -> Result<PathBuf, String> {
    let root = dir().join("execucoes");
    let stamp: String = tools::now_string().chars().map(|c| if c.is_ascii_digit() { c } else { '-' }).collect();
    let d = tools::unique_path(root.join(format!("{}_{stamp}", slug(&flow.name))));
    std::fs::create_dir_all(&d).map_err(|e| format!("não foi possível criar a pasta de trabalho: {e}"))?;
    for old in run_dirs(&flow.name).into_iter().skip(15) {
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(d)
}

/// O que a IA recebe de um item: a imagem, o conteúdo de um arquivo de texto ou os detalhes do arquivo.
// i18n: fora
fn item_parts(item: &Item) -> Vec<Part> {
    let mut parts = Vec::new();
    if let Some(p) = &item.path {
        let ext = tools::ext_lower(p);
        let name = tools::file_name(p);
        if imaging::IMAGE_EXTS.contains(&ext.as_str()) {
            if let Ok(img) = image::open(p) {
                let img = img.thumbnail(1024, 1024).to_rgb8();
                let mut buf = std::io::Cursor::new(Vec::new());
                if image::DynamicImage::ImageRgb8(img).write_to(&mut buf, image::ImageFormat::Jpeg).is_ok() {
                    parts.push(Part::Text(format!("Arquivo: {name}")));
                    parts.push(Part::Image { mime: "image/jpeg", base64: ai_api::base64(buf.get_ref()) });
                }
            }
        } else if ["txt", "md", "srt", "vtt", "csv", "json", "html", "xml", "ass", "log"].contains(&ext.as_str()) {
            let t = std::fs::read_to_string(p).unwrap_or_default();
            let t: String = t.chars().take(60_000).collect();
            parts.push(Part::Text(format!("Arquivo {name}:\n{t}")));
        } else {
            parts.push(Part::Text(format!("Arquivo {name}:\n{}", crate::info::read(p).as_text())));
        }
    }
    if let Some(t) = &item.text {
        parts.push(Part::Text(t.clone()));
    }
    parts
}

// i18n: dentro
fn kind_matches(p: &Path, types: &[String]) -> bool {
    let ext = tools::ext_lower(p);
    types.iter().any(|t| match t.as_str() {
        "image" => imaging::IMAGE_EXTS.contains(&ext.as_str()),
        "video" => crate::video::VIDEO_EXTS.contains(&ext.as_str()),
        "audio" => crate::media::AUDIO_EXTS.contains(&ext.as_str()),
        "pdf" => ext == "pdf",
        "text" => ["txt", "md", "srt", "vtt", "csv", "json"].contains(&ext.as_str()),
        _ => false,
    })
}

/// Executa o fluxo. `ai` é a IA configurada (ou o motivo de não haver), usada pelos nodes de IA.
/// Devolve um relatório em JSON: o que cada node fez e os arquivos/textos finais.
pub fn run(flow: &Flow, inputs: &Inputs, out_dir: Option<&Path>, state: &Mutex<RunState>, ctx: &ops::Ctx, ai: &Result<ai_api::Config, String>) -> Result<Value, String> {
    flow.validate()?;
    let order = flow.order()?;
    if order.is_empty() {
        return Err("o fluxo está vazio.".into());
    }
    let work = new_work_dir(flow)?;
    let started = (tools::now_string(), std::time::Instant::now());
    let mut failed_inputs: Vec<PathBuf> = Vec::new();
    let mut used_inputs: Vec<PathBuf> = Vec::new();
    {
        let mut s = state.lock().unwrap();
        s.status = flow.nodes.iter().map(|n| (n.id, Status::Waiting)).collect();
        s.work_dir = Some(work.clone());
    }
    let set = |id: u32, st: Status| {
        state.lock().unwrap().status.insert(id, st);
    };
    // saídas de cada node, por porta
    let mut outputs: HashMap<(u32, String), Vec<Item>> = HashMap::new();
    let mut report = Vec::new();
    let total = order.len() as f32;
    for (step, id) in order.iter().enumerate() {
        if (ctx.cancelled)() {
            return Err("Cancelado.".into());
        }
        let Some(node) = flow.node(*id) else { continue };
        let info = kind_info(&node.kind).expect("validado");
        let title = if node.title.trim().is_empty() { info.title.clone() } else { node.title.clone() };
        // o que chega: tudo o que sai das ligações para este node
        let mut incoming: Vec<Item> = Vec::new();
        let has_links = flow.links.iter().any(|l| l.to == *id);
        for l in flow.links.iter().filter(|l| l.to == *id) {
            if let Some(items) = outputs.get(&(l.from, l.port.clone())) {
                for it in items {
                    if !incoming.contains(it) {
                        incoming.push(it.clone());
                    }
                }
            }
        }
        if has_links && incoming.is_empty() && info.has_input {
            set(*id, Status::Skipped("nada chegou".into()));
            report.push(json!({ "id": id, "title": title, "status": "skipped", "reason": "nothing arrived" }));
            continue;
        }
        (ctx.log)(&format!("▶ {title}"));
        set(*id, Status::Running(0.0));
        let node_progress = |p: f32| {
            state.lock().unwrap().status.insert(*id, Status::Running(p.clamp(0.0, 1.0)));
            (ctx.progress)((step as f32 + p.clamp(0.0, 1.0)) / total);
        };
        let sub = ops::Ctx { progress: &node_progress, cancelled: ctx.cancelled, log: ctx.log };
        let node_dir = work.join(format!("{:02}_{}", id, slug(&title)));
        let result = run_node(flow, node, &title, &incoming, inputs, out_dir, &node_dir, &sub, ai, &mut failed_inputs);
        match result {
            Ok((ports, note)) => {
                if node.kind == "input" {
                    used_inputs.extend(ports.values().flatten().filter_map(|i| i.origin.clone()));
                }
                let n: usize = ports.values().map(Vec::len).sum();
                let note = if note.is_empty() { format!("{n} item(ns)") } else { note };
                (ctx.log)(&format!("✔ {title}: {note}"));
                set(*id, Status::Done(note.clone()));
                let mut rep = json!({ "id": id, "title": title, "status": "done", "note": note });
                for (port, items) in &ports {
                    rep["outputs"][port] = json!(items.iter().map(item_json).collect::<Vec<_>>());
                }
                report.push(rep);
                for (port, items) in ports {
                    outputs.insert((*id, port), items);
                }
            }
            Err(e) => {
                (ctx.log)(&format!("✖ {title}: {e}"));
                // o node inteiro falhou: tudo o que chegou nele conta como falha
                failed_inputs.extend(incoming.iter().filter_map(|i| i.origin.clone()));
                set(*id, Status::Failed(e.clone()));
                report.push(json!({ "id": id, "title": title, "status": "failed", "error": e }));
            }
        }
        (ctx.progress)((step + 1) as f32 / total);
    }
    // resultado final: o que saiu dos nodes sem ligações de saída
    let mut finals = Vec::new();
    for n in &flow.nodes {
        if !flow.links.iter().any(|l| l.from == n.id) {
            for ((id, _), items) in &outputs {
                if *id == n.id {
                    finals.extend(items.iter().map(item_json));
                }
            }
        }
    }
    let failed = report.iter().filter(|r| r["status"] == "failed").count();
    let mut seen = std::collections::HashSet::new();
    failed_inputs.retain(|p| seen.insert(p.clone()));
    let paths = |v: &[PathBuf]| v.iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
    let result = json!({ "flow": flow.name, "work_dir": work.display().to_string(), "failed_nodes": failed, "failed_inputs": paths(&failed_inputs), "nodes": report, "results": finals });
    // registro da execução, para o histórico (o mesmo JSON, com data, duração e entradas)
    let mut record = result.clone();
    record["started"] = json!(started.0);
    record["seconds"] = json!((started.1.elapsed().as_secs_f64() * 10.0).round() / 10.0);
    record["inputs"] = json!(paths(&used_inputs));
    let _ = std::fs::write(work.join(RUN_RECORD), serde_json::to_string_pretty(&record).unwrap_or_default());
    Ok(result)
}

fn item_json(i: &Item) -> Value {
    let mut v = json!({});
    if let Some(p) = &i.path {
        v["file"] = json!(p.display().to_string());
    }
    if let Some(t) = &i.text {
        v["text"] = json!(t);
    }
    if let Some(t) = &i.tag {
        v["tag"] = json!(t);
    }
    v
}

type Ports = HashMap<String, Vec<Item>>;

fn one(items: Vec<Item>) -> Ports {
    HashMap::from([("out".to_string(), items)])
}

#[allow(clippy::too_many_arguments)]
fn run_node(
    flow: &Flow,
    node: &Node,
    title: &str,
    incoming: &[Item],
    inputs: &Inputs,
    out_dir: Option<&Path>,
    node_dir: &Path,
    ctx: &ops::Ctx,
    ai: &Result<ai_api::Config, String>,
    failed: &mut Vec<PathBuf>,
) -> Result<(Ports, String), String> {
    let args = &node.args;
    let all_text: String = incoming.iter().filter_map(|i| i.text.clone()).collect::<Vec<_>>().join("\n\n");
    match node.kind.as_str() {
        "input" => {
            let files = if inputs.files.is_empty() {
                let saved: Vec<String> = args.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                expand(&saved)
            } else {
                expand(&inputs.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>())
            };
            if files.is_empty() {
                return Err("nenhum arquivo de entrada (adicione arquivos ao node ou passe \"files\").".into());
            }
            let mut items: Vec<Item> = files.into_iter().map(|f| Item { path: Some(f.clone()), origin: Some(f), ..Default::default() }).collect();
            items.extend(inputs.texts.iter().cloned().map(Item::text));
            let n = items.len();
            Ok((one(items), format!("{n} arquivo(s)")))
        }
        "text" => {
            let t = args.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            if t.is_empty() {
                return Err("o texto está vazio.".into());
            }
            Ok((one(vec![Item::text(t)]), String::new()))
        }
        "filter" => {
            let types: Vec<String> = args.get("types").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let exts: Vec<String> = args.get("extensions").and_then(Value::as_str).unwrap_or_default().split([',', ' ', ';']).map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()).collect();
            let min = args.get("min_mb").and_then(Value::as_f64);
            let max = args.get("max_mb").and_then(Value::as_f64);
            let (mut yes, mut no) = (Vec::new(), Vec::new());
            for it in incoming {
                let pass = match &it.path {
                    Some(p) => {
                        let mb = tools::file_size(p) as f64 / 1_048_576.0;
                        (types.is_empty() || kind_matches(p, &types))
                            && (exts.is_empty() || exts.contains(&tools::ext_lower(p)))
                            && min.is_none_or(|m| mb >= m)
                            && max.is_none_or(|m| mb <= m)
                    }
                    // textos só passam se o filtro aceitar "text" (ou não tiver tipos nem extensões)
                    None => (types.is_empty() && exts.is_empty()) || types.iter().any(|t| t == "text"),
                };
                if pass { yes.push(it.clone()) } else { no.push(it.clone()) }
            }
            let note = format!("{} sim, {} não", yes.len(), no.len());
            Ok((HashMap::from([("yes".to_string(), yes), ("no".to_string(), no)]), note))
        }
        "condition" => {
            let check = args.get("check").and_then(Value::as_str).unwrap_or("orientation");
            let more = args.get("compare").and_then(Value::as_str).unwrap_or("more") != "less";
            let value = args.get("value").and_then(Value::as_f64);
            let wanted = args.get("orientation").and_then(Value::as_str).unwrap_or("vertical");
            if !matches!(check, "orientation" | "has_audio") && value.is_none() {
                return Err("informe o valor para comparar.".into());
            }
            let (mut yes, mut no) = (Vec::new(), Vec::new());
            for it in incoming {
                let pass = it.path.as_deref().is_some_and(|p| condition_passes(p, check, more, value.unwrap_or(0.0), wanted));
                if pass { yes.push(it.clone()) } else { no.push(it.clone()) }
            }
            let note = format!("{} sim, {} não", yes.len(), no.len());
            Ok((HashMap::from([("yes".to_string(), yes), ("no".to_string(), no)]), note))
        }
        "rename" => {
            let pattern = args.get("pattern").and_then(Value::as_str).map(str::trim).filter(|p| !p.is_empty()).unwrap_or("{name}_{n}").to_string();
            let start = args.get("start").and_then(Value::as_i64).unwrap_or(1);
            let digits = args.get("digits").and_then(Value::as_u64).unwrap_or(3).clamp(1, 8) as usize;
            let per_option = args.get("count_per_option").and_then(Value::as_bool).unwrap_or(true);
            std::fs::create_dir_all(node_dir).map_err(|e| e.to_string())?;
            let (y, mo, d, h, mi, _) = tools::local_parts(std::time::SystemTime::now());
            let (today, now) = (format!("{y:04}-{mo:02}-{d:02}"), format!("{h:02}-{mi:02}"));
            let mut counters: HashMap<String, i64> = HashMap::new();
            let mut out = Vec::new();
            for it in incoming {
                let Some(p) = &it.path else {
                    out.push(it.clone());
                    continue;
                };
                let key = if per_option { it.tag.clone().unwrap_or_default() } else { String::new() };
                let n = counters.entry(key).or_insert(start);
                let num = format!("{:0digits$}", *n);
                *n += 1;
                let file_date = std::fs::metadata(p)
                    .and_then(|m| m.modified())
                    .map(|t| {
                        let (y, m, d, ..) = tools::local_parts(t);
                        format!("{y:04}-{m:02}-{d:02}")
                    })
                    .unwrap_or_else(|_| today.clone());
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let folder = p.parent().and_then(|f| f.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let option = it.tag.clone().unwrap_or_else(|| "sem-opcao".into());
                let mut name = pattern.clone();
                let tokens: [(&[&str], &String); 7] = [
                    (&["{name}", "{nome}"], &stem),
                    (&["{n}"], &num),
                    (&["{option}", "{opcao}", "{opção}"], &option),
                    (&["{date}", "{data}"], &today),
                    (&["{time}", "{hora}"], &now),
                    (&["{file_date}", "{data_arquivo}"], &file_date),
                    (&["{folder}", "{pasta}"], &folder),
                ];
                for (keys, value) in tokens {
                    for k in keys {
                        name = name.replace(k, value);
                    }
                }
                // sem os caracteres que o Windows não aceita em nomes de arquivo
                let name: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
                let name = name.trim().trim_end_matches('.').to_string();
                let name = if name.is_empty() { stem.clone() } else { name };
                let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
                let dst = tools::unique_path(node_dir.join(format!("{name}{ext}")));
                std::fs::copy(p, &dst).map_err(|e| format!("{}: {e}", tools::file_name(p)))?;
                out.push(Item { path: Some(dst), text: it.text.clone(), tag: it.tag.clone(), origin: it.origin.clone() });
            }
            let n = out.len();
            Ok((one(out), format!("{n} arquivo(s)")))
        }
        "classify" => {
            let options = crate::classify::parse_options(args.get("options").and_then(Value::as_str).unwrap_or_default());
            if options.len() < 2 {
                return Err("escreva pelo menos duas opções.".into());
            }
            let min = (args.get("min_confidence").and_then(Value::as_f64).unwrap_or(50.0) / 100.0).clamp(0.0, 1.0) as f32;
            use crate::classify::Look;
            let look = match args.get("look_at").and_then(Value::as_str).unwrap_or("auto") {
                "image" => Look::Image,
                "sound" => Look::Sound,
                "text" => Look::Text,
                _ => Look::Auto,
            };
            let mut classifier = crate::classify::Classifier::new(options.clone(), look);
            let mut ports: Ports = options.iter().chain(std::iter::once(&UNCERTAIN.to_string())).map(|o| (o.clone(), Vec::new())).collect();
            let n = incoming.len().max(1) as f32;
            for (i, it) in incoming.iter().enumerate() {
                if (ctx.cancelled)() {
                    return Err("Cancelado.".into());
                }
                let progress = |p: f32| (ctx.progress)((i as f32 + p * 0.5) / n);
                let port = match classifier.classify(it.path.as_deref(), it.text.as_deref(), &progress, ctx.cancelled) {
                    Some(Ok(c)) => {
                        let best = crate::classify::decide(&c, min).map(|b| b.label.clone());
                        (ctx.log)(&format!("{} → {} ({:.0}%)", it.label(), best.clone().unwrap_or_else(|| "?".into()), c[0].prob * 100.0));
                        best.unwrap_or_else(|| UNCERTAIN.into())
                    }
                    Some(Err(e)) if e == "Cancelado." => return Err(e),
                    Some(Err(e)) => {
                        (ctx.log)(&format!("✖ {}: {e}", it.label()));
                        UNCERTAIN.into()
                    }
                    None => UNCERTAIN.into(),
                };
                let mut item = it.clone();
                if port != UNCERTAIN {
                    item.tag = Some(port.clone());
                }
                ports.get_mut(&port).unwrap().push(item);
                (ctx.progress)((i + 1) as f32 / n);
            }
            let note = ports
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| format!("{} {}", if k == UNCERTAIN { "?" } else { k.as_str() }, v.len()))
                .collect::<Vec<_>>()
                .join(", ");
            Ok((ports, note))
        }
        "output" => {
            let folder = args.get("folder").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).or_else(|| out_dir.map(Path::to_path_buf)).unwrap_or_else(|| {
                let docs = std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Documents")).unwrap_or_else(std::env::temp_dir);
                let sub = if node.title.trim().is_empty() { slug(&flow.name) } else { format!("{} - {}", slug(&flow.name), slug(&node.title)) };
                docs.join("Conver").join(sub)
            });
            std::fs::create_dir_all(&folder).map_err(|e| format!("não foi possível criar {}: {e}", folder.display()))?;
            let mv = args.get("move").and_then(Value::as_bool).unwrap_or(false);
            let texts = args.get("save_texts").and_then(Value::as_bool).unwrap_or(true);
            let mut out = Vec::new();
            for (i, it) in incoming.iter().enumerate() {
                if (ctx.cancelled)() {
                    return Err("Cancelado.".into());
                }
                if let Some(p) = &it.path {
                    let dst = tools::unique_path(folder.join(tools::file_name(p)));
                    let r = if mv { std::fs::rename(p, &dst).or_else(|_| std::fs::copy(p, &dst).and_then(|_| std::fs::remove_file(p))) } else { std::fs::copy(p, &dst).map(|_| ()) };
                    r.map_err(|e| format!("{}: {e}", tools::file_name(p)))?;
                    out.push(Item { path: Some(dst), tag: it.tag.clone(), origin: it.origin.clone(), ..Default::default() });
                } else if let (true, Some(t)) = (texts, &it.text) {
                    let dst = tools::unique_path(folder.join(format!("texto_{:02}.txt", i + 1)));
                    std::fs::write(&dst, t).map_err(|e| e.to_string())?;
                    out.push(Item { path: Some(dst), text: Some(t.clone()), tag: it.tag.clone(), origin: it.origin.clone() });
                }
                (ctx.progress)((i + 1) as f32 / incoming.len().max(1) as f32);
            }
            let n = out.len();
            (ctx.log)(&format!("   → {}", folder.display()));
            Ok((one(out), format!("{n} salvo(s) em {}", tools::file_name(&folder))))
        }
        "cloud" => {
            let service = args.get("service").and_then(Value::as_str).unwrap_or("auto");
            let sub = args.get("subfolder").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Conver").to_string();
            let mv = args.get("move").and_then(Value::as_bool).unwrap_or(false);
            let files: Vec<PathBuf> = incoming.iter().filter_map(|it| it.path.clone()).collect();
            if files.is_empty() {
                return Ok((one(Vec::new()), "nada para enviar".into()));
            }
            let (copies, name) = crate::cloud::send(&files, service, &sub, mv)?;
            let out: Vec<Item> = copies.into_iter().zip(incoming.iter().filter(|it| it.path.is_some())).map(|(p, it)| Item { path: Some(p), text: it.text.clone(), tag: it.tag.clone(), origin: it.origin.clone() }).collect();
            let n = out.len();
            Ok((one(out), format!("{} arquivo(s) no {}", n, name)))
        }
        "webhook" => {
            let url = args.get("url").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
            if url.is_empty() {
                return Err("informe o endereço do webhook.".into());
            }
            let as_file = args.get("send").and_then(Value::as_str) == Some("file");
            let message = args.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
            let payload: Vec<crate::cloud::Payload> = incoming.iter().map(|it| crate::cloud::Payload { path: it.path.as_deref(), text: it.text.as_deref(), tag: it.tag.as_deref() }).collect();
            let note = crate::cloud::webhook(&url, &flow.name, &payload, as_file, &message)?;
            Ok((one(incoming.to_vec()), note))
        }
        "ai_prompt" => {
            let cfg = ai.as_ref().map_err(|e| e.clone())?;
            let prompt = args.get("prompt").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            if prompt.is_empty() {
                return Err("escreva a instrução para a IA.".into());
            }
            let per_item = args.get("per_item").and_then(Value::as_bool).unwrap_or(true);
            let keep = args.get("keep_files").and_then(Value::as_bool).unwrap_or(true);
            let save_as = args.get("save_as").and_then(Value::as_str).unwrap_or("none").to_string();
// i18n: fora
            let system = "Você é uma etapa de um fluxo de automação do Conver (um conversor de arquivos). Faça exatamente o que a instrução pede e responda só com o resultado, sem comentários antes ou depois.";
// i18n: dentro
            let groups: Vec<Vec<&Item>> = if per_item && !incoming.is_empty() { incoming.iter().map(|i| vec![i]).collect() } else { vec![incoming.iter().collect()] };
            let mut out = Vec::new();
            let n = groups.len();
            for (gi, group) in groups.iter().enumerate() {
                if (ctx.cancelled)() {
                    return Err("Cancelado.".into());
                }
                let first = group.first().copied();
                let mut parts = vec![Part::Text(template(&prompt, if per_item { first } else { None }, &all_text))];
                for it in group {
                    parts.extend(item_parts(it));
                }
                let label = first.map(Item::label).unwrap_or_else(|| title.to_string());
                (ctx.log)(&format!("   IA: {label}"));
                let answer = ai_api::ask(cfg, system, &parts, 4096).map_err(|e| format!("{label}: {e}"))?;
                let mut item = Item::text(answer.clone());
                if save_as != "none" {
                    std::fs::create_dir_all(node_dir).map_err(|e| e.to_string())?;
                    let stem = first.and_then(|i| i.path.as_ref()).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "resposta".into());
                    let dst = tools::unique_path(node_dir.join(format!("{stem}.{save_as}")));
                    std::fs::write(&dst, &answer).map_err(|e| e.to_string())?;
                    item.path = Some(dst);
                }
                out.push(item);
                if keep {
                    out.extend(group.iter().filter(|i| i.path.is_some()).map(|i| Item { path: i.path.clone(), tag: i.tag.clone(), origin: i.origin.clone(), ..Default::default() }));
                }
                (ctx.progress)((gi + 1) as f32 / n as f32);
            }
            Ok((one(out), format!("{n} resposta(s)")))
        }
        "ai_gate" => {
            let cfg = ai.as_ref().map_err(|e| e.clone())?;
            let criteria = args.get("criteria").and_then(Value::as_str).unwrap_or_default().trim().to_string();
            if criteria.is_empty() {
                return Err("escreva o critério da avaliação.".into());
            }
// i18n: fora
            let system = "Você avalia itens de um fluxo de automação. Responda SOMENTE com JSON no formato {\"approve\": true ou false, \"reason\": \"motivo curto em português\"}.";
            let (mut yes, mut no) = (Vec::new(), Vec::new());
            for (i, it) in incoming.iter().enumerate() {
                if (ctx.cancelled)() {
                    return Err("Cancelado.".into());
                }
                let mut parts = vec![Part::Text(format!("Critério: {}\n\nO item abaixo atende ao critério?", template(&criteria, Some(it), &all_text)))];
// i18n: dentro

                parts.extend(item_parts(it));
                let answer = ai_api::ask(cfg, system, &parts, 400).map_err(|e| format!("{}: {e}", it.label()))?;
                let (ok, reason) = ai_api::parse_verdict(&answer);
                (ctx.log)(&format!("   {} {}: {reason}", if ok { "✔" } else { "✖" }, it.label()));
                if ok { yes.push(it.clone()) } else { no.push(it.clone()) }
                (ctx.progress)((i + 1) as f32 / incoming.len().max(1) as f32);
            }
            let note = format!("{} sim, {} não", yes.len(), no.len());
            Ok((HashMap::from([("yes".to_string(), yes), ("no".to_string(), no)]), note))
        }
        k if k.starts_with("tool:") => run_tool(&k[5..], args, incoming, &all_text, node_dir, ctx, failed),
        other => Err(format!("tipo desconhecido: {other}")),
    }
}

fn run_tool(
    name: &str,
    args: &Map<String, Value>,
    incoming: &[Item],
    all_text: &str,
    node_dir: &Path,
    ctx: &ops::Ctx,
    failed: &mut Vec<PathBuf>,
) -> Result<(Ports, String), String> {
    let tool = ops::find(name).ok_or_else(|| format!("ferramenta desconhecida: {name}"))?;
    let schema = (tool.schema)();
    let props = schema["properties"].as_object().cloned().unwrap_or_default();
    // {text} e {file} nos textos dos argumentos
    let mut a: Map<String, Value> = args
        .iter()
        .map(|(k, v)| (k.clone(), match v {
            Value::String(s) => Value::String(template(s, None, all_text)),
            other => other.clone(),
        }))
        .collect();
    let empty = |a: &Map<String, Value>, k: &str| a.get(k).is_none_or(|v| v.is_null() || v.as_str().is_some_and(|s| s.trim().is_empty()) || v.as_array().is_some_and(|x| x.is_empty()));
    let mut files: Vec<String> = incoming.iter().filter_map(|i| i.path.as_ref()).map(|p| p.display().to_string()).collect();
    // sem música escolhida, o áudio que chegou faz esse papel (uma narração gerada antes no fluxo, por exemplo)
    if props.contains_key("music") && empty(&a, "music") {
        if let Some(i) = files.iter().position(|f| crate::media::AUDIO_EXTS.contains(&tools::ext_lower(Path::new(f)).as_str())) {
            a.insert("music".into(), json!(files.remove(i)));
        }
    }
    if props.contains_key("files") {
        // ferramentas que aceitam arquivos ou texto (voz, por exemplo) podem rodar só com o texto
        let has_text = props.contains_key("text") && (!empty(&a, "text") || !all_text.is_empty());
        if !files.is_empty() {
            a.insert("files".into(), json!(files));
        } else if !has_text {
            return Err("nenhum arquivo chegou a este node.".into());
        }
    }
    // ferramentas que recebem texto: os links do download, o texto do QR Code
    if props.contains_key("urls") && empty(&a, "urls") && !all_text.is_empty() {
        a.insert("urls".into(), json!(all_text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>()));
    }
    if props.contains_key("text") && empty(&a, "text") && !all_text.is_empty() {
        a.insert("text".into(), json!(all_text));
    }
    let fixed_out = !empty(&a, "out_dir") || !empty(&a, "output");
    if props.contains_key("out_dir") && empty(&a, "out_dir") {
        std::fs::create_dir_all(node_dir).map_err(|e| e.to_string())?;
        a.insert("out_dir".into(), json!(node_dir.display().to_string()));
    }
    let v = ops::call(name, &Value::Object(a), ctx)?;
    // resultado: tudo o que a ferramenta criou na pasta do node, mais os caminhos e textos do JSON
    let mut items: Vec<Item> = if node_dir.is_dir() { files_in(node_dir).into_iter().map(Item::file).collect() } else { Vec::new() };
    let results: Vec<Value> = match v["results"].as_array() {
        Some(r) => r.clone(),
        None => vec![v.clone()],
    };
    let made_files = !items.is_empty();
    for r in &results {
        if let Some(o) = r["output"].as_str() {
            let p = PathBuf::from(o);
            if (fixed_out || !p.starts_with(node_dir)) && p.is_file() && !items.iter().any(|i| i.path.as_ref() == Some(&p)) {
                items.push(Item::file(p));
            }
        }
        // textos só quando a ferramenta não criou arquivos (OCR, informações); a transcrição já gera .txt
        if !made_files {
            if let Some(t) = r["text"].as_str().or(if r["output"].is_null() { r["info"].as_str() } else { None }) {
                if !t.trim().is_empty() {
                    items.push(Item::text(t.to_string()));
                }
            }
        }
    }
    // a opção do classificador segue com o que foi criado a partir de cada arquivo: pelo par entrada → saída do
    // resultado ou, para os arquivos extras (.srt, .txt…), pelo nome que começa com o do arquivo de entrada
    let sources: Vec<&Item> = incoming.iter().filter(|i| i.path.is_some() && (i.tag.is_some() || i.origin.is_some())).collect();
    if !sources.is_empty() {
        let by_output: Vec<(PathBuf, PathBuf)> =
            results.iter().filter_map(|r| Some((PathBuf::from(r["input"].as_str()?), PathBuf::from(r["output"].as_str()?)))).collect();
        let stem = |x: &Path| x.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        for it in items.iter_mut().filter(|i| i.tag.is_none() && i.origin.is_none()) {
            let Some(p) = it.path.clone() else { continue };
            let from = by_output.iter().find(|(_, o)| *o == p).map(|(i, _)| i.clone());
            let src = sources.iter().find(|s| {
                let sp = s.path.as_ref().unwrap();
                from.as_ref() == Some(sp) || (!stem(sp).is_empty() && stem(&p).starts_with(&stem(sp)))
            });
            if let Some(s) = src {
                it.tag = s.tag.clone();
                it.origin = s.origin.clone();
            }
        }
    }
    // arquivos que deram erro: a origem de cada um vai para "repetir só o que falhou"
    for r in results.iter().filter(|r| r["error"].is_string()) {
        if let Some(input) = r["input"].as_str().map(PathBuf::from) {
            let origin = incoming.iter().find(|i| i.path.as_ref() == Some(&input)).and_then(|i| i.origin.clone()).unwrap_or(input);
            failed.push(origin);
        }
    }
    let failed = v["failed"].as_u64().unwrap_or(0);
    let note = if failed > 0 { format!("{} item(ns), {failed} falharam", items.len()) } else { format!("{} item(ns)", items.len()) };
    Ok((one(items), note))
}
