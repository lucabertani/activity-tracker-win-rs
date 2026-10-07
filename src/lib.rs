// Win32 FFI codebase: window/icon handles (HWND, HICON, ...) are obtained from
// Win32 APIs and forwarded to other Win32 calls; they are valid by
// construction. Marking every wrapper `unsafe` would only move the `unsafe`
// blocks to the callers without adding real safety.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

#[cfg(target_os = "windows")]
pub mod app;
#[cfg(target_os = "windows")]
pub mod error;
#[cfg(target_os = "windows")]
pub mod model;
#[cfg(target_os = "windows")]
pub mod monitor;
#[cfg(target_os = "windows")]
pub mod settings;
#[cfg(target_os = "windows")]
pub mod ui;

#[cfg(target_os = "windows")]
pub fn run() -> error::AppResult<()> {
    app::run()
}
