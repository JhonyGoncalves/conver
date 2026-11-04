//! conver-cli.exe: porta de entrada de linha de comando do Conver (scripts, .bat e agentes de IA via MCP).
//!
//! O Conver.exe é um programa de janela; o Windows não espera por ele num terminal e não mostra o que ele
//! escreve. Este executável de console só repassa os argumentos para o Conver.exe (modo `--cli`), com a
//! entrada e a saída do terminal, espera terminar e devolve o mesmo código de saída.

#![windows_subsystem = "console"]

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn conver_exe() -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let dir = me.parent()?;
    ["Conver.exe", "conver.exe"].iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

fn main() {
    let Some(exe) = conver_exe() else {
        eprintln!("Conver.exe não encontrado ao lado do conver-cli.exe.");
        std::process::exit(2);
    };
    let status = Command::new(exe)
        .arg("--cli")
        .args(std::env::args_os().skip(1))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(s) => std::process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("não foi possível iniciar o Conver.exe: {e}");
            std::process::exit(2);
        }
    }
}
