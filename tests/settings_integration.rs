#![cfg(target_os = "windows")]

use std::sync::Mutex;

use tracker_core::model::{
    AppSettings, DEFAULT_TRAY_CPU_BG, DEFAULT_TRAY_CPU_FG, DEFAULT_TRAY_FONT_WEIGHT,
    DEFAULT_TRAY_RAM_BG, DEFAULT_TRAY_RAM_FG, parse_refresh_interval_seconds,
};
use tracker_platform::settings::store::{load_settings, save_settings, settings_path};

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
        tray_icon_thermal_red: true,
        tray_icon_ram_enabled: true,
        tray_icon_cpu_enabled: true,
        tray_icon_ram_bg: DEFAULT_TRAY_RAM_BG,
        tray_icon_ram_fg: DEFAULT_TRAY_RAM_FG,
        tray_icon_cpu_bg: DEFAULT_TRAY_CPU_BG,
        tray_icon_cpu_fg: DEFAULT_TRAY_CPU_FG,
        tray_icon_font_size: 0,
        tray_icon_font_weight: DEFAULT_TRAY_FONT_WEIGHT,
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
fn settings_with_utf8_bom_still_load() {
    let _guard = APPDATA_LOCK.lock().expect("lock APPDATA");
    let temp = tempfile::tempdir().expect("tempdir");

    unsafe {
        std::env::set_var("APPDATA", temp.path());
    }

    // A UTF-8 BOM (as written by PowerShell/Notepad) must not reset the
    // settings to defaults.
    let path = settings_path().expect("settings path");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    let json = "{\"start_on_windows\":true,\"start_in_background\":false,\"refresh_interval_seconds\":4,\"window_start_hidden\":false,\"show_tray_tooltip\":true,\"tray_icon_thermal_red\":true}";
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(json.as_bytes());
    std::fs::write(&path, bytes).expect("write settings with BOM");

    let loaded = load_settings().expect("load settings");
    assert!(loaded.start_on_windows);
    assert_eq!(loaded.refresh_interval_seconds, 4);
    assert!(loaded.tray_icon_thermal_red);
}

#[test]
fn parse_interval_for_ui_input_behaves_as_expected() {
    assert_eq!(parse_refresh_interval_seconds("10", 3), 10);
    assert_eq!(parse_refresh_interval_seconds("0", 3), 1);
    assert_eq!(parse_refresh_interval_seconds("999", 3), 60);
    assert_eq!(parse_refresh_interval_seconds("not-number", 9), 9);
}
