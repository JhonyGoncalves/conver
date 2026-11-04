//! Integração com o Windows e com o GitHub: menu do botão direito, instância única,
//! verificação de atualizações e atualização do yt-dlp.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::{download, imaging, media, tools};

/// Repositório do Conver no GitHub (dono/nome).
pub const REPO: &str = match option_env!("CONVER_REPO") {
    Some(r) => r,
    None => "JhonyGoncalves/conver",
};

pub fn repo_url() -> String {
    format!("https://github.com/{REPO}")
}

// --------------------------------------------------------- menu de contexto

const VERB: &str = "Conver";

fn menu_exts() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    for e in imaging::IMAGE_EXTS.iter().chain(media::MEDIA_EXTS).chain(["gif", "pdf"].iter()) {
        if !v.contains(e) {
            v.push(e);
        }
    }
    v
}

fn key_path(ext: &str) -> String {
    format!(r"Software\Classes\SystemFileAssociations\.{ext}\shell\{VERB}")
}

/// Adiciona "Abrir com Conver" ao menu do botão direito (só para o usuário atual, sem precisar de administrador).
pub fn register_menu() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.display().to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in menu_exts() {
        let (key, _) = hkcu.create_subkey(key_path(ext)).map_err(|e| e.to_string())?;
        key.set_value("", &"Abrir com Conver").map_err(|e| e.to_string())?;
        key.set_value("Icon", &format!("\"{exe}\",0")).map_err(|e| e.to_string())?;
        // vários arquivos selecionados chegam juntos (cada um abre um processo, que repassa à janela aberta)
        key.set_value("MultiSelectModel", &"Player").map_err(|e| e.to_string())?;
        let (cmd, _) = key.create_subkey("command").map_err(|e| e.to_string())?;
        cmd.set_value("", &format!("\"{exe}\" \"%1\"")).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn unregister_menu() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for ext in menu_exts() {
        let _ = hkcu.delete_subkey_all(key_path(ext));
    }
}

pub fn menu_registered() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(cmd) = hkcu.open_subkey(format!(r"{}\command", key_path("png"))) else { return false };
    let Ok(value) = cmd.get_value::<String, _>("") else { return false };
    std::env::current_exe().is_ok_and(|exe| value.contains(&exe.display().to_string()))
}

// ------------------------------------------------------------ instância única

const PORT: u16 = 47_815;

/// Se o Conver já estiver aberto, entrega os arquivos a ele e devolve `true` (este processo deve sair).
/// Senão, passa a escutar e devolve `false`; os arquivos recebidos depois aparecem em `inbox`.
pub fn single_instance(args: &[PathBuf], inbox: Arc<Mutex<Vec<PathBuf>>>) -> bool {
    match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => {
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let mut lines = Vec::new();
                    let mut reader = BufReader::new(stream);
                    let mut first = String::new();
                    // só aceita mensagens do próprio Conver
                    if reader.read_line(&mut first).is_err() || first.trim() != "CONVER/1" {
                        continue;
                    }
                    for line in reader.lines().map_while(Result::ok) {
                        if !line.is_empty() {
                            lines.push(PathBuf::from(line));
                        }
                    }
                    let mut inbox = inbox.lock().unwrap();
                    inbox.push(PathBuf::new()); // só traz a janela para a frente
                    inbox.extend(lines);
                }
            });
            false
        }
        Err(_) => {
            let Ok(mut s) = TcpStream::connect(("127.0.0.1", PORT)) else { return false };
            let mut msg = String::from("CONVER/1\n");
            for a in args {
                msg.push_str(&a.display().to_string());
                msg.push('\n');
            }
            s.write_all(msg.as_bytes()).is_ok()
        }
    }
}

// ------------------------------------------------------------- atualizações

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(&format!("Conver/{}", env!("CARGO_PKG_VERSION")))
        .build()
}

fn parse_version(v: &str) -> Vec<u32> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map_while(|p| p.parse().ok()).collect()
}

/// `true` se `latest` for maior que `current` ("v1.2.0" > "1.1.9").
pub fn is_newer(latest: &str, current: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

pub struct Release {
    pub version: String,
    pub url: String,
}

/// Última versão publicada no GitHub, se for mais nova que esta.
pub fn check_update() -> Result<Option<Release>, String> {
    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = agent()
        .get(&api)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("não foi possível consultar o GitHub: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or_default().to_string();
    let url = json["html_url"].as_str().map(str::to_string).unwrap_or_else(|| format!("{}/releases/latest", repo_url()));
    Ok(is_newer(&tag, env!("CARGO_PKG_VERSION")).then(|| Release { version: tag.trim_start_matches('v').to_string(), url }))
}

/// SHA-256 (sem dependência extra).
pub fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

const YTDLP: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";

/// Baixa o yt-dlp.exe oficial mais recente para `dst_dir`, conferindo o SHA-256 publicado.
/// Devolve o caminho do arquivo baixado (ainda não colocado no lugar).
pub fn download_ytdlp(dst_dir: &Path, on_progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    let a = agent();
    let sums = a
        .get(&format!("{YTDLP}/SHA2-256SUMS"))
        .call()
        .map_err(|e| format!("não foi possível acessar o GitHub: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let expected = sums
        .lines()
        .find_map(|l| l.strip_suffix(" yt-dlp.exe").or_else(|| l.strip_suffix("  yt-dlp.exe")).map(|h| h.trim().to_lowercase()))
        .ok_or("lista de verificação do yt-dlp sem o yt-dlp.exe.")?;
    let resp = a.get(&format!("{YTDLP}/yt-dlp.exe")).call().map_err(|e| format!("falha no download: {e}"))?;
    let total: usize = resp.header("Content-Length").and_then(|v| v.parse().ok()).unwrap_or(20_000_000);
    let mut reader = resp.into_reader();
    let mut data = Vec::with_capacity(total);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        on_progress((data.len() as f32 / total as f32).min(0.99));
    }
    if sha256(&data) != expected {
        return Err("o arquivo baixado não confere com a assinatura publicada; nada foi alterado.".into());
    }
    let tmp = dst_dir.join("yt-dlp.exe.novo");
    std::fs::write(&tmp, &data).map_err(|e| format!("não foi possível salvar: {e}"))?;
    Ok(tmp)
}

/// Atualiza bin\yt-dlp.exe e devolve (versão anterior, versão nova).
pub fn update_ytdlp(on_progress: &dyn Fn(f32)) -> Result<(String, String), String> {
    let current = tools::find_tool("yt-dlp").filter(|p| p.starts_with(tools::exe_dir()));
    let bin = tools::exe_dir().join("bin");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let before = download::version().unwrap_or_else(|| "nenhuma".into());
    let new = download_ytdlp(&bin, on_progress)?;
    let target = current.unwrap_or_else(|| bin.join("yt-dlp.exe"));
    let backup = bin.join("yt-dlp.exe.antigo");
    let _ = std::fs::remove_file(&backup);
    if target.exists() {
        std::fs::rename(&target, &backup).map_err(|e| format!("não foi possível substituir o yt-dlp (está em uso?): {e}"))?;
    }
    if let Err(e) = std::fs::rename(&new, &target) {
        let _ = std::fs::rename(&backup, &target);
        return Err(e.to_string());
    }
    let after = download::version().unwrap_or_default();
    if after.is_empty() {
        // o novo não roda: volta o antigo
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::rename(&backup, &target);
        return Err("a versão baixada não funcionou; a anterior foi mantida.".into());
    }
    let _ = std::fs::remove_file(&backup);
    Ok((before, after))
}
