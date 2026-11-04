//! Atualizar o Conver de dentro do próprio app.
//!
//! 1. `prepare` baixa da release do GitHub o arquivo certo para esta cópia (o instalador, se o Conver foi
//!    instalado; o .zip, se for a versão portátil) e confere o SHA-256 publicado na própria release.
//! 2. `apply` inicia quem vai trocar os arquivos e o app se fecha:
//!    - instalado: o instalador novo roda em modo silencioso (mesma pasta, mesmas opções) e reabre o Conver;
//!    - portátil: o Conver.exe novo, ainda na pasta temporária, espera este fechar, copia os arquivos por cima
//!      (sem mexer em `dados\`) e abre o Conver atualizado.

use std::path::{Path, PathBuf};

use crate::integration::Release;
use crate::tools;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Instalado pelo instalador (tem o desinstalador ao lado).
    Instalado,
    /// Versão portátil (tem o portatil.txt ao lado).
    Portatil,
    /// Outro jeito (compilado do código, pasta copiada…): só dá para baixar pelo navegador.
    Manual,
}

pub fn mode() -> Mode {
    mode_of(&tools::exe_dir())
}

fn mode_of(dir: &Path) -> Mode {
    if dir.join("portatil.txt").is_file() {
        Mode::Portatil
    } else if dir.join("unins000.exe").is_file() {
        Mode::Instalado
    } else {
        Mode::Manual
    }
}

/// Atualização baixada, conferida e pronta para instalar.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub mode: Mode,
    /// O instalador (.exe) ou a pasta `Conver` extraída do .zip.
    pub path: PathBuf,
}

/// Soma SHA-256 de `name` no arquivo de somas da release ("<hash>  <nome>").
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

fn work_dir(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("conver-atualizacao-{version}"))
}

/// Baixa e confere a atualização (0..1 em `on_progress`).
pub fn prepare(rel: &Release, mode: Mode, on_progress: &dyn Fn(f32), cancelled: &dyn Fn() -> bool) -> Result<Prepared, String> {
    let suffix = match mode {
        Mode::Instalado => "-instalador.exe",
        Mode::Portatil => "-portatil.zip",
        Mode::Manual => return Err("esta cópia do Conver não foi instalada pelo instalador nem é a portátil; baixe pelo site.".into()),
    };
    let asset = rel.asset(suffix).ok_or("a versão nova não tem o arquivo para esta cópia do Conver; baixe pelo site.")?;
    let sums_asset = rel.asset("-SHA256.txt").ok_or("a versão nova não publicou as somas de verificação; baixe pelo site.")?;
    let sums = crate::integration::agent()
        .get(&sums_asset.url)
        .call()
        .map_err(|e| format!("não foi possível acessar o GitHub: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let expected = expected_sum(&sums, &asset.name).ok_or("o arquivo da atualização não está na lista de verificação; baixe pelo site.")?;

    let dir = work_dir(&rel.version);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(&asset.name);
    let label = format!("Conver {}", rel.version);
    let share = if mode == Mode::Portatil { 0.9 } else { 1.0 };
    crate::ai::download_verified(&asset.url, &file, &expected, asset.size, &label, &|p| on_progress(p * share), cancelled)?;

    let path = match mode {
        Mode::Portatil => {
            // o tar do Windows (10 1803 ou mais novo) abre .zip
            let out = dir.join("extraido");
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let tar = PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\tar.exe");
            let st = tools::command(&tar).arg("-xf").arg(&file).arg("-C").arg(&out).status().map_err(|e| format!("não foi possível extrair a atualização: {e}"))?;
            let folder = out.join("Conver");
            if !st.success() || !folder.join("Conver.exe").is_file() {
                return Err("não foi possível extrair a atualização.".into());
            }
            let _ = std::fs::remove_file(&file);
            folder
        }
        _ => file,
    };
    on_progress(1.0);
    Ok(Prepared { mode, path })
}

/// Inicia a troca dos arquivos. Depois de `Ok`, o app deve fechar logo.
pub fn apply(p: &Prepared) -> Result<(), String> {
    let pid = std::process::id().to_string();
    match p.mode {
        Mode::Instalado => {
            // /RELANCAR=1: o instalador abre o Conver de novo no fim (veja installer\conver.iss)
            std::process::Command::new(&p.path)
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELANCAR=1"])
                .spawn()
                .map_err(|e| format!("não foi possível iniciar o instalador: {e}"))?;
        }
        Mode::Portatil => {
            std::process::Command::new(p.path.join("Conver.exe"))
                .arg("--aplicar-atualizacao")
                .arg(&p.path)
                .arg(tools::exe_dir())
                .arg(pid)
                .spawn()
                .map_err(|e| format!("não foi possível iniciar a atualização: {e}"))?;
        }
        Mode::Manual => return Err("atualização automática indisponível nesta cópia.".into()),
    }
    Ok(())
}

/// Espera o processo `pid` terminar (no máximo `secs` segundos).
fn wait_pid(pid: u32, secs: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
    // SAFETY: só abre o processo para esperar por ele e fecha a alça em seguida
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(h, secs * 1000);
            let _ = CloseHandle(h);
        }
    }
}

/// Copia a pasta nova por cima da atual, sem mexer nos dados do usuário. Tenta de novo por alguns segundos
/// se um arquivo ainda estiver em uso.
fn copy_over(src: &Path, dst: &Path) -> Result<usize, String> {
    let mut n = 0;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        let s = entry.path();
        let d = dst.join(&name);
        let lower = name.to_string_lossy().to_lowercase();
        if lower == "dados" || (lower == "portatil.txt" && d.is_file()) {
            continue;
        }
        if s.is_dir() {
            std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
            n += copy_over(&s, &d)?;
        } else {
            let mut tries = 0;
            loop {
                match std::fs::copy(&s, &d) {
                    Ok(_) => break,
                    Err(_) if tries < 40 => {
                        tries += 1;
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                    Err(e) => return Err(format!("{}: {e}", d.display())),
                }
            }
            n += 1;
        }
    }
    Ok(n)
}

/// `Conver.exe --aplicar-atualizacao <pasta nova> <pasta do Conver> <pid>` (versão portátil).
pub fn run_apply(args: &[String]) -> i32 {
    let [src, dst, pid] = args else { return 2 };
    let (src, dst) = (PathBuf::from(src), PathBuf::from(dst));
    wait_pid(pid.parse().unwrap_or(0), 60);
    let result = copy_over(&src, &dst);
    // nos testes o Conver não é reaberto (para não abrir janelas)
    let reopen = std::env::var_os("CONVER_NAO_REABRIR").is_none();
    match result {
        Ok(_) => {
            if reopen {
                let _ = std::process::Command::new(dst.join("Conver.exe")).current_dir(&dst).spawn();
            }
            0
        }
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title(crate::i18n::tr("Conver"))
                .set_level(rfd::MessageLevel::Error)
                .set_description(crate::i18n::tr(format!("Não foi possível terminar a atualização: {e}")))
                .show();
            if reopen {
                let _ = std::process::Command::new(dst.join("Conver.exe")).current_dir(&dst).spawn();
            }
            1
        }
    }
}

/// Apaga sobras de atualizações anteriores na pasta temporária.
pub fn cleanup_old() {
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("conver-atualizacao-") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn test_mode_of(dir: &Path) -> Mode {
    mode_of(dir)
}

#[cfg(test)]
pub(crate) fn test_expected_sum(sums: &str, name: &str) -> Option<String> {
    expected_sum(sums, name)
}

#[cfg(test)]
pub(crate) fn test_copy_over(src: &Path, dst: &Path) -> Result<usize, String> {
    copy_over(src, dst)
}
