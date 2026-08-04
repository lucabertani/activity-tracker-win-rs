#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This application supports Windows only.");
}

#[cfg(target_os = "windows")]
fn main() {
    // GUI app without console: surface panics and startup errors in a
    // message box instead of failing silently on double-click.
    std::panic::set_hook(Box::new(|info| {
        let message = format!("Errore imprevisto:\n{info}\n\nRiavvia l'applicazione. Se il problema persiste, riporta questo testo.");
        show_fatal_error("Activity Tracker - Errore", &message);
    }));

    if let Err(err) = activity_tracker_win_rs::run() {
        show_fatal_error("Activity Tracker - Errore di avvio", &format!("{err}"));
    }
}

#[cfg(target_os = "windows")]
fn show_fatal_error(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};

    let title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
