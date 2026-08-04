#![cfg(target_os = "windows")]

use std::sync::Mutex;

use activity_tracker_win_rs::model::{parse_refresh_interval_seconds, AppSettings};
use activity_tracker_win_rs::settings::store::{load_settings, save_settings, settings_path};

static APPDATA_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn settings_roundtrip_uses_appdata_location() {
    let _guard = APPDATA_LOCK.lock().expect("lock APPDATA");
    let temp = tempfile::tempdir().expect("tempdir");

    unsafe {
        std::env::set_var("APPDATA", temp.path());
    }

    let source = AppSettings {
        start_on_windows: true,
        start_in_background: true,
        refresh_interval_seconds: 7,
        window_start_hidden: true,
        show_tray_tooltip: true,
    };

    save_settings(&source).expect("save settings");

    let path = settings_path().expect("settings path");
    assert!(path.exists(), "settings file should exist");

    let loaded = load_settings().expect("load settings");

    assert!(loaded.start_on_windows);
    assert!(loaded.start_in_background);
    assert_eq!(loaded.refresh_interval_seconds, 7);
    assert!(loaded.window_start_hidden);
    assert!(loaded.show_tray_tooltip);
}

#[test]
fn parse_interval_for_ui_input_behaves_as_expected() {
    assert_eq!(parse_refresh_interval_seconds("10", 3), 10);
    assert_eq!(parse_refresh_interval_seconds("0", 3), 1);
    assert_eq!(parse_refresh_interval_seconds("999", 3), 60);
    assert_eq!(parse_refresh_interval_seconds("not-number", 9), 9);
}
