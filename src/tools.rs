//! Utilidades: caminhos, programas externos (ffmpeg / yt-dlp) e formatação.

use std::path::{Path, PathBuf};
use std::process::Command;

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn which(file: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(file)).find(|c| c.is_file())
}

/// Procura `name.exe` em `bin\` ao lado do Conver.exe, ao lado dele, e por fim no PATH.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let file = format!("{name}.exe");
    let dir = exe_dir();
    [dir.join("bin").join(&file), dir.join(&file)]
        .into_iter()
        .find(|c| c.is_file())
        .or_else(|| which(&file))
}

/// Arquivo qualquer em `bin\` ao lado do Conver.exe (ou, no desenvolvimento, em `Conver\bin` do projeto).
pub fn find_tool_file(file: &str) -> Option<PathBuf> {
    let dir = exe_dir();
    [dir.join("bin").join(file), dir.join(file), PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Conver").join("bin").join(file)]
        .into_iter()
        .find(|c| c.is_file())
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    which(&format!("{name}.exe"))
}

/// Comando que não abre janela de console.
pub fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

pub fn human_size(n: u64) -> String {
    let mut v = n as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if v < 1024.0 || unit == "GB" {
            return if unit == "B" { format!("{v:.0} {unit}") } else { format!("{v:.1} {unit}") };
        }
        v /= 1024.0;
    }
    unreachable!()
}

pub fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

pub fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn ext_lower(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Evita sobrescrever: foto.png -> foto (1).png.
pub fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|c| !c.exists())
        .unwrap()
}

/// Caminho de saída: `<pasta>/<nome><sufixo>.<ext>`.
pub fn out_path(src: &Path, out_dir: Option<&Path>, suffix: &str, ext: &str) -> PathBuf {
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = out_dir
        .map(Path::to_path_buf)
        .or_else(|| src.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    unique_path(dir.join(format!("{stem}{suffix}.{ext}")))
}

pub fn open_folder(p: &Path) {
    let _ = Command::new("explorer").arg(p).spawn();
}

/// Pasta de dados do Conver (%APPDATA%\Conver).
/// Na versão portátil (arquivo `portatil.txt` ao lado do .exe), fica em `dados\` dentro da pasta do Conver.
pub fn app_dir() -> PathBuf {
    let dir = exe_dir();
    if dir.join("portatil.txt").is_file() {
        return dir.join("dados");
    }
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or(dir).join("Conver")
}

#[cfg(windows)]
#[repr(C)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

#[cfg(windows)]
unsafe extern "system" {
    fn GetLocalTime(st: *mut SystemTime);
}

/// Data e hora locais, "dd/mm/aaaa hh:mm".
pub fn now_string() -> String {
    #[cfg(windows)]
    {
        let mut t = SystemTime { year: 0, month: 0, day_of_week: 0, day: 0, hour: 0, minute: 0, second: 0, millis: 0 };
        // SAFETY: GetLocalTime só preenche a estrutura passada.
        unsafe { GetLocalTime(&mut t) };
        format!("{:02}/{:02}/{} {:02}:{:02}", t.day, t.month, t.year, t.hour, t.minute)
    }
    #[cfg(not(windows))]
    {
        String::new()
    }
}

/// Abre o arquivo com o programa padrão.
pub fn open_file(p: &Path) {
    let _ = Command::new("explorer").arg(p).spawn();
}

/// Abre o Explorer com o arquivo selecionado.
pub fn show_in_folder(p: &Path) {
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(p);
    let _ = Command::new("explorer").arg(arg).spawn();
}

pub fn savings(before: u64, after: u64) -> String {
    let pct = if before > 0 { (1.0 - after as f64 / before as f64) * 100.0 } else { 0.0 };
    let sign = if pct >= 0.0 { "-" } else { "+" };
    format!("{} → {} ({sign}{:.0}%)", human_size(before), human_size(after), pct.abs())
}
