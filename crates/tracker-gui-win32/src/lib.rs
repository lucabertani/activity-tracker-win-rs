//! Legacy GUI on the standard Win32 controls (ListView, buttons, tab
//! control). Kept as a fallback for the GPUI frontend.
#![cfg(target_os = "windows")]
// Win32 FFI codebase: window/icon handles (HWND, HICON, ...) are obtained from
// Win32 APIs and forwarded to other Win32 calls; they are valid by
// construction. Marking every wrapper `unsafe` would only move the `unsafe`
// blocks to the callers without adding real safety.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

mod app;
mod menu;
mod settings_window;
mod window;

pub use app::run;
