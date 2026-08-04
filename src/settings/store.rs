use std::fs;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::model::AppSettings;

const APP_DIR: &str = "ActivityTracker";
const SETTINGS_FILE: &str = "settings.json";

pub fn settings_path() -> AppResult<PathBuf> {
    let appdata = std::env::var("APPDATA")
        .map_err(|_| AppError::InvalidSetting("APPDATA environment variable is not set".to_string()))?;
    Ok(PathBuf::from(appdata).join(APP_DIR).join(SETTINGS_FILE))
}

pub fn load_settings() -> AppResult<AppSettings> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let raw = fs::read_to_string(path)?;
    // Tolerate a UTF-8 BOM: some editors / PowerShell write one, and
    // serde_json refuses to parse it (which would silently reset all
    // settings to their defaults).
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
    let parsed = serde_json::from_str::<AppSettings>(raw)?;
    Ok(parsed.normalized())
}

pub fn save_settings(settings: &AppSettings) -> AppResult<()> {
    let normalized = settings.clone().normalized();
    let path = settings_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let json = serde_json::to_string_pretty(&normalized)?;
    fs::write(path, json)?;
    Ok(())
}
