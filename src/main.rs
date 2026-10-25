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
    // The default hook still prints to stderr (visible from a terminal).
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_hook(info);
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

/// Picks the frontend: GPUI when the "new interface" setting is on and
/// `--legacy-ui` is not given (and GPUI is compiled in); falls back to the
/// Win32 GUI when GPUI cannot start (e.g. no usable DirectX 11 device).
#[cfg(target_os = "windows")]
fn run_gui(mut startup: tracker_platform::startup::Startup) -> tracker_core::error::AppResult<()> {
    startup.can_switch_ui = cfg!(all(feature = "gui-gpui", feature = "gui-win32"));

    #[cfg(feature = "gui-gpui")]
    if !cfg!(feature = "gui-win32")
        || (startup.settings.modern_ui && !has_arg(tracker_platform::startup::ARG_LEGACY_UI))
    {
        let settings = startup.settings.clone();
        let start_hidden = startup.start_hidden;
        let can_switch_ui = startup.can_switch_ui;
        match tracker_gui_gpui::run(startup) {
            Ok(()) => return Ok(()),
            #[cfg(feature = "gui-win32")]
            Err(_) => {
                return tracker_gui_win32::run(tracker_platform::startup::Startup {
                    settings,
                    start_hidden,
                    can_switch_ui,
                });
            }
            #[cfg(not(feature = "gui-win32"))]
            Err(err) => {
                let _ = (settings, start_hidden, can_switch_ui);
                return Err(err);
            }
        }
    }

    #[cfg(feature = "gui-win32")]
    {
        tracker_gui_win32::run(startup)
    }
    #[cfg(not(feature = "gui-win32"))]
    {
        let _ = startup;
        Err(tracker_core::error::AppError::WinApi("no GUI compiled in"))
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
