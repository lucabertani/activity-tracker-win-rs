#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This application supports Windows only.");
}

#[cfg(target_os = "windows")]
use tracker_platform::startup::{ARG_NO_ELEVATE, has_arg};

#[cfg(target_os = "windows")]
fn main() {
    // GUI app without console: surface panics and startup errors in a
    // message box instead of failing silently on double-click.
    std::panic::set_hook(Box::new(|info| {
        let message = format!(
            "Unexpected error:\n{info}\n\nRestart the application. If the problem persists, report this text."
        );
        show_fatal_error("Activity Tracker - Error", &message);
    }));

    // The PawnIO device is admin-only: when it exists but denies this
    // process, take over with an elevated copy (UAC prompt).
    if !has_arg(ARG_NO_ELEVATE) {
        tracker_platform::elevation::elevate_if_driver_denies_access();
    }
    let startup = tracker_platform::startup::prepare();

    if let Err(err) = run_gui(startup) {
        show_fatal_error("Activity Tracker - Startup error", &format!("{err}"));
    }
}

#[cfg(all(target_os = "windows", feature = "gui-win32"))]
fn run_gui(startup: tracker_platform::startup::Startup) -> tracker_core::error::AppResult<()> {
    tracker_gui_win32::run(startup)
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
