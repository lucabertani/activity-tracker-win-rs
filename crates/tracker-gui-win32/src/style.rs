//! Shared look of the classic windows: one soft gray background behind
//! every label and checkbox, and Segoe UI instead of the bitmap system font.

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT};
use windows_sys::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, FW_NORMAL, FW_SEMIBOLD,
    HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumChildWindows, SendMessageW, WM_SETFONT};

use crate::window::wide;

/// Window background, COLORREF (0x00BBGGRR): #f3f3f3, the Windows 11
/// dialog gray.
pub const BG_COLOR: u32 = 0x00F3F3F3;
/// Body text: #1b1b1b.
pub const TEXT_COLOR: u32 = 0x001B1B1B;
/// Throttling warning: #c42b1c (Windows 11 "critical" red).
pub const WARNING_COLOR: u32 = 0x001C2BC4;

/// Handles cast to `usize`, since raw pointers are not `Sync`.
static BG_BRUSH: OnceLock<usize> = OnceLock::new();
static UI_FONT: OnceLock<usize> = OnceLock::new();
static HEADER_FONT: OnceLock<usize> = OnceLock::new();

/// Brush in [`BG_COLOR`], shared by every window (never freed).
pub fn bg_brush() -> HBRUSH {
    *BG_BRUSH.get_or_init(|| unsafe { CreateSolidBrush(BG_COLOR) } as usize) as HBRUSH
}

/// Segoe UI 9 pt: the standard Windows dialog font.
pub fn ui_font() -> HFONT {
    *UI_FONT.get_or_init(|| make_font(-12, FW_NORMAL as i32, "Segoe UI")) as HFONT
}

/// Semibold, larger: the RAM / CPU readings at the top of the main window.
pub fn header_font() -> HFONT {
    *HEADER_FONT.get_or_init(|| make_font(-16, FW_SEMIBOLD as i32, "Segoe UI Semibold")) as HFONT
}

fn make_font(height: i32, weight: i32, face: &str) -> usize {
    let face = wide(face);
    (unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            face.as_ptr(),
        )
    }) as usize
}

/// Sets `font` on every child control of `parent`.
pub fn set_font_on_children(parent: HWND, font: HFONT) {
    unsafe extern "system" fn apply(child: HWND, font: LPARAM) -> i32 {
        unsafe { SendMessageW(child, WM_SETFONT, font as usize, 1) };
        1
    }
    unsafe {
        EnumChildWindows(parent, Some(apply), font as LPARAM);
    }
}

pub fn set_font(control: HWND, font: HFONT) {
    unsafe {
        SendMessageW(control, WM_SETFONT, font as usize, 1);
    }
}

/// `WM_CTLCOLORSTATIC` / `WM_CTLCOLORBTN` answer: text in `text_color` on
/// the shared background, so labels and checkboxes blend into the window.
pub fn paint_on_background(hdc: HDC, text_color: u32) -> LRESULT {
    unsafe {
        SetTextColor(hdc, text_color);
        SetBkColor(hdc, BG_COLOR);
        SetBkMode(hdc, TRANSPARENT as i32);
    }
    bg_brush() as LRESULT
}
