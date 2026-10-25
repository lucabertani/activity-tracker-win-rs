//! Switching between the two interfaces: the choice is read at startup, so
//! the app confirms with the user and restarts itself.

use std::process::Command;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW};

use crate::startup::{ARG_BACKGROUND, ARG_LEGACY_UI, ARG_NO_ELEVATE};

/// Title and text of the confirmation shown before switching interface.
pub fn switch_prompt(to_modern: bool) -> (&'static str, &'static str) {
    if to_modern {
        (
            "Switch to the new interface?",
            "The new interface looks better but uses more memory and CPU \
             (about 120 MB of RAM). Activity Tracker will restart now.",
        )
    } else {
        (
            "Switch to the classic interface?",
            "The classic interface is lighter (about 16 MB of RAM). \
             Activity Tracker will restart now.",
        )
    }
}

/// Native Yes/No confirmation for the interface switch, modal to `owner`.
pub fn confirm_switch(owner: HWND, to_modern: bool) -> bool {
    let (title, text) = switch_prompt(to_modern);
    let message = wide(&format!("{title}\n\n{text}"));
    let caption = wide("Activity Tracker");
    let answer = unsafe {
        MessageBoxW(
            owner,
            message.as_ptr(),
            caption.as_ptr(),
            MB_YESNO | MB_ICONQUESTION,
        )
    };
    answer == IDYES
}

/// Starts a new copy of the app; the caller exits right after (removing
/// its tray icons). The copy shows its window, takes the interface from the
/// saved settings and never asks for elevation again: it inherits this
/// process's token, and a declined UAC prompt should not come back.
pub fn relaunch() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    Command::new(exe)
        .args(relaunch_args(std::env::args().skip(1)))
        .spawn()
        .map(drop)
}

fn relaunch_args(args: impl Iterator<Item = String>) -> Vec<String> {
    let mut args: Vec<String> = args
        .filter(|a| {
            !a.eq_ignore_ascii_case(ARG_BACKGROUND) && !a.eq_ignore_ascii_case(ARG_LEGACY_UI)
        })
        .collect();
    if !args.iter().any(|a| a.eq_ignore_ascii_case(ARG_NO_ELEVATE)) {
        args.push(ARG_NO_ELEVATE.to_owned());
    }
    args
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::relaunch_args;

    #[test]
    fn relaunch_shows_the_window_with_the_saved_interface() {
        let args = ["--background", "--LEGACY-UI", "--other"].map(String::from);
        assert_eq!(relaunch_args(args.into_iter()), ["--other", "--no-elevate"]);
        let args = ["--no-elevate"].map(String::from);
        assert_eq!(relaunch_args(args.into_iter()), ["--no-elevate"]);
    }
}
