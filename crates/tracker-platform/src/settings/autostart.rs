use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::{AppError, AppResult};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE, KEY_WRITE};

const RUN_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const APP_NAME: &str = "ActivityTrackerWinRs";

/// Registers the autostart as a Task Scheduler logon task. With `/RL HIGHEST`
/// the task starts the app elevated at logon *without* a UAC prompt, which
/// the Run key cannot do (it starts processes unelevated, and the app would
/// then ask for elevation at every boot). Requires administrator rights at
/// registration time (the app runs elevated).
pub fn set_autostart(enabled: bool, background: bool, exe_path: &Path) -> AppResult<()> {
    if enabled {
        let action = task_action(exe_path, background);
        let status = Command::new("schtasks.exe")
            .args([
                "/Create", "/F", "/SC", "ONLOGON", "/TN", APP_NAME, "/TR", &action, "/RL",
                "HIGHEST",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(AppError::InvalidSetting(
                "could not register the autostart task (administrator rights are required)"
                    .to_string(),
            ));
        }
    } else if task_exists() {
        let status = Command::new("schtasks.exe")
            .args(["/Delete", "/F", "/TN", APP_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        if !status.success() {
            return Err(AppError::InvalidSetting(
                "could not remove the autostart task (administrator rights are required)"
                    .to_string(),
            ));
        }
    }

    // Drop the legacy Run-key registration (pre-task autostart) so the app is
    // not started twice at logon — and not prompted for UAC by the old entry.
    // Only after the task is in place, so a failed registration keeps the old
    // entry as a fallback.
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((run_key, _)) = hkcu.create_subkey_with_flags(RUN_PATH, KEY_WRITE | KEY_SET_VALUE) {
        let _ = run_key.delete_value(APP_NAME);
    }
    Ok(())
}

fn task_exists() -> bool {
    Command::new("schtasks.exe")
        .args(["/Query", "/TN", APP_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The task action: quoted exe path plus the optional background flag.
fn task_action(exe_path: &Path, background: bool) -> String {
    if background {
        format!("\"{}\" --background", exe_path.display())
    } else {
        format!("\"{}\"", exe_path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::task_action;
    use std::path::Path;

    #[test]
    fn task_action_quotes_the_exe_and_flags_background() {
        assert_eq!(
            task_action(Path::new("C:\\Program Files\\app.exe"), true),
            "\"C:\\Program Files\\app.exe\" --background"
        );
        assert_eq!(
            task_action(Path::new("C:\\app.exe"), false),
            "\"C:\\app.exe\""
        );
    }
}
