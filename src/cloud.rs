//! Nodes de integração dos fluxos: enviar para a nuvem (pelas pastas sincronizadas do OneDrive, Google Drive e Dropbox
//! no PC: o próprio aplicativo de cada um sobe os arquivos) e chamar um webhook (n8n, Zapier, Make, Discord…).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::tools;

/// Serviços de nuvem com pasta sincronizada no PC.
// i18n: fora
pub const SERVICES: [(&str, &str); 3] = [("onedrive", "OneDrive"), ("google_drive", "Google Drive"), ("dropbox", "Dropbox")];
// i18n: dentro

/// Pasta sincronizada de um serviço, se o aplicativo dele estiver instalado e configurado.
pub fn folder(service: &str) -> Option<PathBuf> {
    let dir = |p: PathBuf| Some(p).filter(|p| p.is_dir());
    match service {
        "onedrive" => ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
            .iter()
            .find_map(|v| std::env::var_os(v).map(PathBuf::from).and_then(dir))
            .or_else(|| {
                let key = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\OneDrive").ok()?;
                dir(PathBuf::from(key.get_value::<String, _>("UserFolder").ok()?))
            }),
        "dropbox" => {
            // o Dropbox escreve a pasta em info.json
            let bases = [std::env::var_os("LOCALAPPDATA"), std::env::var_os("APPDATA")];
            bases.into_iter().flatten().find_map(|b| {
                let text = std::fs::read_to_string(PathBuf::from(b).join("Dropbox").join("info.json")).ok()?;
                let v: Value = serde_json::from_str(&text).ok()?;
                ["personal", "business"].iter().find_map(|k| v[*k]["path"].as_str().map(PathBuf::from).and_then(dir))
            })
        }
        "google_drive" => {
            // o Google Drive para computador monta uma unidade (G: e outras) com "Meu Drive"
            // i18n: fora
            let names = ["Meu Drive", "My Drive", "Mi unidad", "Mon Drive", "Meine Ablage"];
            // i18n: dentro
            ('D'..='Z')
                .flat_map(|l| names.iter().map(move |n| PathBuf::from(format!("{l}:\\{n}"))))
                .find(|p| p.is_dir())
                .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Google Drive")).and_then(dir))
        }
        _ => None,
    }
}

/// Os serviços achados neste PC.
pub fn available() -> Vec<(&'static str, PathBuf)> {
    SERVICES.iter().filter_map(|(id, _)| folder(id).map(|p| (*id, p))).collect()
}

/// Copia (ou move) os arquivos para a pasta do serviço (`service` = "auto": o primeiro achado). Devolve os novos caminhos
/// e o nome do serviço.
pub fn send(files: &[PathBuf], service: &str, subfolder: &str, mv: bool) -> Result<(Vec<PathBuf>, &'static str), String> {
    let (id, root) = if service == "auto" {
        available().into_iter().next().ok_or("nenhum OneDrive, Google Drive ou Dropbox encontrado neste PC (instale e entre no aplicativo do serviço).")?
    } else {
        let id = SERVICES.iter().find(|(i, _)| *i == service).map(|(i, _)| *i).ok_or("serviço desconhecido.")?;
        let name = SERVICES.iter().find(|(i, _)| *i == id).map_or(id, |(_, n)| *n);
        (id, folder(id).ok_or_else(|| format!("a pasta do {} não foi encontrada neste PC: instale o aplicativo dele e entre na sua conta.", name))?)
    };
    let name = SERVICES.iter().find(|(i, _)| *i == id).map_or("", |(_, n)| *n);
    Ok((copy_into(&root, subfolder, files, mv)?, name))
}

/// Copia (ou move) para `root\subfolder` (subpastas com / ou \; ".." é ignorado, para não sair da pasta da nuvem).
pub(crate) fn copy_into(root: &Path, subfolder: &str, files: &[PathBuf], mv: bool) -> Result<Vec<PathBuf>, String> {
    let dest = subfolder.split(['/', '\\']).filter(|p| !p.trim().is_empty() && p.trim() != "..").fold(root.to_path_buf(), |d, p| d.join(p.trim()));
    std::fs::create_dir_all(&dest).map_err(|e| format!("não foi possível criar {}: {e}", dest.display()))?;
    let mut out = Vec::new();
    for f in files {
        let dst = tools::unique_path(dest.join(tools::file_name(f)));
        let r = if mv { std::fs::rename(f, &dst).or_else(|_| std::fs::copy(f, &dst).and_then(|_| std::fs::remove_file(f))) } else { std::fs::copy(f, &dst).map(|_| ()) };
        r.map_err(|e| format!("{}: {e}", tools::file_name(f)))?;
        out.push(dst);
    }
    Ok(out)
}

/// Um item enviado ao webhook.
pub struct Payload<'a> {
    pub path: Option<&'a Path>,
    pub text: Option<&'a str>,
    pub tag: Option<&'a str>,
}

fn item_json(p: &Payload) -> Value {
    json!({
        "name": p.path.map(tools::file_name),
        "path": p.path.map(|x| x.display().to_string()),
        "size": p.path.map(tools::file_size),
        "text": p.text,
        "option": p.tag,
    })
}

/// Chama o webhook. Em JSON, manda os dados dos itens (nome, caminho, tamanho, texto); em "arquivo", manda cada arquivo
/// como um formulário com o campo `file` e a mensagem em `content` (o formato do Discord, também aceito por n8n e Make).
pub fn webhook(url: &str, flow: &str, items: &[Payload], as_file: bool, message: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("o endereço do webhook precisa começar com https://".into());
    }
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(120)).build();
    let check = |r: Result<ureq::Response, ureq::Error>| -> Result<u16, String> {
        match r {
            Ok(resp) => Ok(resp.status()),
            Err(ureq::Error::Status(code, resp)) => Err(format!("o webhook respondeu erro {}: {}", code, resp.into_string().unwrap_or_default().chars().take(200).collect::<String>())),
            Err(e) => Err(format!("não foi possível chamar o webhook: {}", e)),
        }
    };
    if !as_file {
        let list: Vec<Value> = items.iter().map(item_json).collect();
        let body = json!({ "source": "Conver", "flow": flow, "message": message, "content": message, "items": list });
        let code = check(agent.post(url).set("content-type", "application/json").send_string(&body.to_string()))?;
        return Ok(format!("{} item(ns) enviados (resposta {})", items.len(), code));
    }
    let mut sent = 0;
    for it in items {
        let boundary = format!("----conver{}", std::process::id());
        let mut body: Vec<u8> = Vec::new();
        let field = |body: &mut Vec<u8>, name: &str, value: &str| {
            body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
        };
        let text = if message.is_empty() { it.text.unwrap_or_default().to_string() } else { message.to_string() };
        if !text.is_empty() {
            field(&mut body, "content", &text);
        }
        if let Some(p) = it.path {
            let data = std::fs::read(p).map_err(|e| format!("{}: {e}", tools::file_name(p)))?;
            let name = tools::file_name(p).replace('"', "");
            body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n").as_bytes());
            body.extend(data);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{boundary}--\r\n").as_bytes());
        check(agent.post(url).set("content-type", &format!("multipart/form-data; boundary={boundary}")).send_bytes(&body))?;
        sent += 1;
    }
    Ok(format!("{} arquivo(s) enviados", sent))
}
