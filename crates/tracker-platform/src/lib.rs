//! Windows system access shared by every frontend.
#![cfg(target_os = "windows")]
// Win32 FFI codebase: window/icon handles (HWND, HICON, ...) are obtained from
// Win32 APIs and forwarded to other Win32 calls; they are valid by
// construction. Marking every wrapper `unsafe` would only move the `unsafe`
// blocks to the callers without adding real safety.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod elevation;
pub mod monitor;
pub mod restart;
pub mod sampler;
pub mod settings;
pub mod startup;
pub mod tray;
pub mod tray_host;
pub mod tray_menu;

// Re-exported so the modules here keep using `crate::model` / `crate::error`.
pub use tracker_core::{error, model};
