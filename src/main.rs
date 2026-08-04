#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This application supports Windows only.");
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(err) = activity_tracker_win_rs::run() {
        eprintln!("Application error: {err}");
    }
}
