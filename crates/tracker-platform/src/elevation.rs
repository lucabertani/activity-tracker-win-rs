//! Self-elevation: the PawnIO temperature device allows administrators only.

use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::monitor::msr::{MsrThermalMonitor, OpenError};

/// When the PawnIO device exists but denies this process, relaunches the
/// exe elevated (UAC prompt) and exits: the elevated copy takes over.
/// Returns when no elevation is needed, or when it is not possible (UAC
/// declined): the app then runs without CPU temperatures.
pub fn elevate_if_driver_denies_access() {
    if let Err(OpenError::AccessDenied) = MsrThermalMonitor::open() {
        relaunch_elevated();
    }
}

fn relaunch_elevated() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let file: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let params = quote_args(std::env::args().skip(1));
    let params = wide(&params);

    let launched = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("runas").as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if launched as isize > 32 {
        // The elevated copy owns the window and tray icons from here on.
        std::process::exit(0);
    }
}

/// Command line for the elevated copy: same arguments, quoted when needed.
fn quote_args(args: impl Iterator<Item = String>) -> String {
    args.map(|arg| {
        if arg.contains([' ', '"']) {
            format!("\"{}\"", arg.replace('"', "\\\""))
        } else {
            arg
        }
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::quote_args;

    #[test]
    fn arguments_are_forwarded_quoted() {
        let args = ["--background", "a b", "say \"hi\""].map(String::from);
        assert_eq!(
            quote_args(args.into_iter()),
            "--background \"a b\" \"say \\\"hi\\\"\""
        );
    }
}
