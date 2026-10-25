//! Startup steps shared by every frontend.

use crate::model::AppSettings;
use crate::settings::autostart::set_autostart;
use crate::settings::store::load_settings;

/// Started by the autostart task: begin hidden in the tray.
pub const ARG_BACKGROUND: &str = "--background";
/// Never ask for elevation (development / smoke tests): no temperatures.
pub const ARG_NO_ELEVATE: &str = "--no-elevate";

pub struct Startup {
    pub settings: AppSettings,
    /// Hide the window at startup: started by Windows autostart
    /// (`--background`) or the "window_start_hidden" setting. A manual
    /// double-click always shows the window, even if "Start in background"
    /// is enabled.
    pub start_hidden: bool,
}

/// Loads the settings and re-syncs the autostart registration. Call after
/// [`elevate_if_driver_denies_access`](crate::elevation::elevate_if_driver_denies_access).
pub fn prepare() -> Startup {
    let settings = load_settings()
        .unwrap_or_else(|_| AppSettings::default())
        .normalized();

    // Best-effort autostart re-sync: keeps the registered exe path fresh and
    // migrates the legacy Run-key entry to the elevated logon task. A
    // non-elevated start just keeps the existing registration.
    if settings.start_on_windows
        && let Ok(exe) = std::env::current_exe()
    {
        let _ = set_autostart(true, settings.start_in_background, &exe);
    }

    let from_cli = has_arg(ARG_BACKGROUND);
    Startup {
        start_hidden: settings.window_start_hidden || from_cli,
        settings,
    }
}

/// Case-insensitive command line flag check.
pub fn has_arg(flag: &str) -> bool {
    std::env::args().any(|a| a.eq_ignore_ascii_case(flag))
}
