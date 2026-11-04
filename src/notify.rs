//! Aviso ao terminar: som e notificação do Windows (esta só quando o Conver não está em primeiro plano).

use std::sync::atomic::{AtomicBool, Ordering};

static NOTIFY: AtomicBool = AtomicBool::new(true);
static SOUND: AtomicBool = AtomicBool::new(true);
static FOCUSED: AtomicBool = AtomicBool::new(true);

pub fn configure(notify: bool, sound: bool) {
    NOTIFY.store(notify, Ordering::Relaxed);
    SOUND.store(sound, Ordering::Relaxed);
}

pub fn set_focused(focused: bool) {
    FOCUSED.store(focused, Ordering::Relaxed);
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(kind: u32) -> i32;
}

fn beep(ok: bool) {
    #[cfg(windows)]
    // SAFETY: MessageBeep só toca o som do sistema.
    unsafe {
        MessageBeep(if ok { 0x40 } else { 0x10 }); // informação / erro
    }
}

/// Chamado quando uma tarefa termina.
pub fn finished(text: &str, ok: bool) {
    if SOUND.load(Ordering::Relaxed) {
        beep(ok);
    }
    if NOTIFY.load(Ordering::Relaxed) && !FOCUSED.load(Ordering::Relaxed) {
        toast(if ok { "Conver — pronto" } else { "Conver — erro" }, text);
    }
}

/// Notificação do Windows (aparece mesmo com o Conver minimizado).
pub fn toast(title: &str, text: &str) {
    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;
        let (title, text) = (title.to_string(), text.to_string());
        std::thread::spawn(move || {
            let _ = Toast::new(Toast::POWERSHELL_APP_ID).title(&title).text1(&text).sound(None).show();
        });
    }
}
