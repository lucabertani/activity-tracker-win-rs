use std::path::Path;

use crate::error::AppResult;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WRITE};

const RUN_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_NAME: &str = "ActivityTrackerWinRs";

pub fn set_autostart(enabled: bool, background: bool, exe_path: &Path) -> AppResult<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run_key, _) = hkcu.create_subkey_with_flags(RUN_PATH, KEY_WRITE | KEY_SET_VALUE)?;

    if enabled {
        let value = if background {
            format!("\"{}\" --background", exe_path.display())
        } else {
            format!("\"{}\"", exe_path.display())
        };
        run_key.set_value(APP_NAME, &value)?;
    } else {
        let _ = run_key.delete_value(APP_NAME);
    }

    Ok(())
}
