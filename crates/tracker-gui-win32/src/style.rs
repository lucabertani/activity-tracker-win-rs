//! Shared palette, native control themes and fonts for the classic windows.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use tracker_core::model::ThemePreference;
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::UI::Controls::{
    LVM_SETBKCOLOR, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, SetWindowTheme,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT};
use windows_sys::Win32::Graphics::Gdi::{
    CLEARTYPE_QUALITY, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, FW_NORMAL, FW_SEMIBOLD,
    HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE,
    RedrawWindow, SetBkColor, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumChildWindows, SendMessageW, WM_SETFONT};

use crate::window::wide;

/// Window background, COLORREF (0x00BBGGRR): #f3f3f3, the Windows 11
/// dialog gray.
const BG_COLOR: u32 = 0x00F3F3F3;
/// Body text: #1b1b1b.
const TEXT_COLOR: u32 = 0x001B1B1B;
/// Throttling warning: #c42b1c (Windows 11 "critical" red).
const WARNING_COLOR: u32 = 0x001C2BC4;

static DARK: AtomicBool = AtomicBool::new(false);
static DARK_BRUSH: OnceLock<usize> = OnceLock::new();

pub fn update(preference: ThemePreference) -> bool {
    let dark = match preference {
        ThemePreference::Light => false,
        ThemePreference::Dark => true,
        ThemePreference::Auto => RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
            .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
            .map(|light| light == 0)
            .unwrap_or(false),
    };
    DARK.swap(dark, Ordering::Relaxed) != dark
}

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

pub fn bg_color() -> u32 {
    match is_dark() {
        true => 0x00202020,
        false => BG_COLOR,
    }
}

pub fn text_color() -> u32 {
    match is_dark() {
        true => 0x00F3F3F3,
        false => TEXT_COLOR,
    }
}

pub fn warning_color() -> u32 {
    match is_dark() {
        true => 0x009999FF,
        false => WARNING_COLOR,
    }
}

/// Retheme existing controls as well as the title bar; no restart needed.
pub fn apply_window(hwnd: HWND) {
    unsafe extern "system" fn apply_child(child: HWND, _: LPARAM) -> i32 {
        let mut class = [0u16; 64];
        let len = unsafe { GetClassNameW(child, class.as_mut_ptr(), class.len() as i32) };
        let class = String::from_utf16_lossy(&class[..len as usize]);
        let theme = if is_dark() {
            match class.as_str() {
                "ComboBox" => "DarkMode_CFD",
                _ => "DarkMode_Explorer",
            }
        } else {
            "Explorer"
        };
        unsafe {
            SetWindowTheme(child, wide(theme).as_ptr(), std::ptr::null());
            if class == "SysHeader32" || class == "SysTabControl32" {
                windows_sys::Win32::UI::Shell::SetWindowSubclass(
                    child,
                    Some(strip_proc),
                    1,
                    usize::from(class == "SysTabControl32"),
                );
            }
            if class == "SysListView32" {
                SendMessageW(child, LVM_SETBKCOLOR, 0, bg_color() as isize);
                SendMessageW(child, LVM_SETTEXTBKCOLOR, 0, bg_color() as isize);
                SendMessageW(child, LVM_SETTEXTCOLOR, 0, text_color() as isize);
            }
        }
        1
    }
    if hwnd.is_null() {
        return;
    }
    unsafe {
        let dark = i32::from(is_dark());
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &dark as *const _ as _,
            std::mem::size_of_val(&dark) as u32,
        );
        EnumChildWindows(hwnd, Some(apply_child), 0);
        RedrawWindow(
            hwnd,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        );
    }
}

/// Headers and tab strips have no public dark palette API. Keep the native
/// layout, hit testing and accessibility, replacing only their painting.
unsafe extern "system" fn strip_proc(
    hwnd: HWND,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _: usize,
    is_tab: usize,
) -> LRESULT {
    use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_ERASEBKGND, WM_PAINT, WM_PRINTCLIENT};
    if is_dark() {
        match msg {
            WM_ERASEBKGND => return erase_background(hwnd, wparam as HDC),
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                unsafe {
                    let hdc = BeginPaint(hwnd, &mut ps);
                    paint_strip(hwnd, hdc, is_tab != 0);
                    EndPaint(hwnd, &ps);
                }
                return 0;
            }
            WM_PRINTCLIENT => {
                paint_strip(hwnd, wparam as HDC, is_tab != 0);
                return 0;
            }
            _ => {}
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

fn paint_strip(hwnd: HWND, hdc: HDC, is_tab: bool) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{
        DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, FrameRect,
        SelectObject,
    };
    use windows_sys::Win32::UI::Controls::{
        HDI_TEXT, HDITEMW, HDM_GETITEMCOUNT, HDM_GETITEMRECT, HDM_GETITEMW, TCIF_TEXT, TCITEMW,
        TCM_GETCURSEL, TCM_GETITEMCOUNT, TCM_GETITEMRECT, TCM_GETITEMW,
    };
    erase_background(hwnd, hdc);
    unsafe {
        SetTextColor(hdc, text_color());
        SetBkMode(hdc, TRANSPARENT as i32);
        let old_font = SelectObject(hdc, ui_font() as _);
        let border = CreateSolidBrush(0x00808080);
        let count_message = match is_tab {
            true => TCM_GETITEMCOUNT,
            false => HDM_GETITEMCOUNT,
        };
        let count = SendMessageW(hwnd, count_message, 0, 0);
        let selected = match is_tab {
            true => SendMessageW(hwnd, TCM_GETCURSEL, 0, 0),
            false => -1,
        };
        let accent = CreateSolidBrush(text_color());
        for index in 0..count as usize {
            let mut rect = RECT::default();
            let mut text = [0u16; 256];
            if is_tab {
                SendMessageW(hwnd, TCM_GETITEMRECT, index, &mut rect as *mut _ as isize);
                let mut item = TCITEMW {
                    mask: TCIF_TEXT,
                    pszText: text.as_mut_ptr(),
                    cchTextMax: text.len() as i32,
                    ..Default::default()
                };
                SendMessageW(hwnd, TCM_GETITEMW, index, &mut item as *mut _ as isize);
            } else {
                SendMessageW(hwnd, HDM_GETITEMRECT, index, &mut rect as *mut _ as isize);
                let mut item = HDITEMW {
                    mask: HDI_TEXT,
                    pszText: text.as_mut_ptr(),
                    cchTextMax: text.len() as i32,
                    ..Default::default()
                };
                SendMessageW(hwnd, HDM_GETITEMW, index, &mut item as *mut _ as isize);
            }
            let outline = match is_tab && index == selected as usize {
                true => accent,
                false => border,
            };
            FrameRect(hdc, &rect, outline);
            rect.left += 6;
            rect.right -= 4;
            DrawTextW(
                hdc,
                text.as_ptr(),
                -1,
                &mut rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
        DeleteObject(border as _);
        DeleteObject(accent as _);
        SelectObject(hdc, old_font);
    }
}

/// Handles cast to `usize`, since raw pointers are not `Sync`.
static BG_BRUSH: OnceLock<usize> = OnceLock::new();
static UI_FONT: OnceLock<usize> = OnceLock::new();
static HEADER_FONT: OnceLock<usize> = OnceLock::new();

/// Brush in [`BG_COLOR`], shared by every window (never freed).
pub fn bg_brush() -> HBRUSH {
    if is_dark() {
        *DARK_BRUSH.get_or_init(|| unsafe { CreateSolidBrush(0x00202020) } as usize) as HBRUSH
    } else {
        *BG_BRUSH.get_or_init(|| unsafe { CreateSolidBrush(BG_COLOR) } as usize) as HBRUSH
    }
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

pub fn erase_background(hwnd: HWND, hdc: HDC) -> LRESULT {
    use windows_sys::Win32::Graphics::Gdi::FillRect;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let mut rect = Default::default();
    unsafe {
        GetClientRect(hwnd, &mut rect);
        FillRect(hdc, &rect, bg_brush());
    }
    1
}

/// Native buttons do not consistently support dark text/background colors
/// on Windows 10. Custom draw keeps their native input and accessibility.
pub fn paint_notification(lparam: LPARAM) -> Option<LRESULT> {
    use windows_sys::Win32::Graphics::Gdi::{
        DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawFocusRect, DrawTextW,
        FillRect, FrameRect,
    };
    use windows_sys::Win32::UI::Controls::{
        CDDS_PREPAINT, CDIS_FOCUS, CDIS_SELECTED, CDRF_SKIPDEFAULT, NM_CUSTOMDRAW, NMCUSTOMDRAW,
        NMHDR,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BM_GETCHECK, BS_AUTOCHECKBOX, GWL_STYLE, GetWindowLongPtrW, GetWindowTextW,
    };
    if !is_dark() || lparam == 0 {
        return None;
    }
    unsafe {
        let hdr = &*(lparam as *const NMHDR);
        if hdr.code != NM_CUSTOMDRAW {
            return None;
        }
        let mut class = [0u16; 32];
        let len = GetClassNameW(hdr.hwndFrom, class.as_mut_ptr(), class.len() as i32);
        if String::from_utf16_lossy(&class[..len as usize]) != "Button" {
            return None;
        }
        let draw = &*(lparam as *const NMCUSTOMDRAW);
        if draw.dwDrawStage != CDDS_PREPAINT {
            return None;
        }
        let mut rect = draw.rc;
        FillRect(draw.hdc, &rect, bg_brush());
        let border = CreateSolidBrush(0x00808080);
        let checkbox =
            GetWindowLongPtrW(hdr.hwndFrom, GWL_STYLE) as u32 & 0xF == BS_AUTOCHECKBOX as u32;
        let align = if checkbox {
            let mut check = rect;
            check.top += (rect.bottom - rect.top - 13) / 2;
            check.bottom = check.top + 13;
            check.right = check.left + 13;
            FrameRect(draw.hdc, &check, border);
            if SendMessageW(hdr.hwndFrom, BM_GETCHECK, 0, 0) == 1 {
                check.left += 3;
                check.right -= 3;
                check.top += 3;
                check.bottom -= 3;
                FillRect(draw.hdc, &check, border);
            }
            rect.left += 20;
            DT_LEFT
        } else {
            FrameRect(draw.hdc, &rect, border);
            if draw.uItemState & CDIS_SELECTED != 0 {
                rect.left += 1;
                rect.top += 1;
            }
            DT_CENTER
        };
        DeleteObject(border as _);
        SetTextColor(draw.hdc, text_color());
        SetBkMode(draw.hdc, TRANSPARENT as i32);
        let mut text = [0u16; 256];
        let len = GetWindowTextW(hdr.hwndFrom, text.as_mut_ptr(), text.len() as i32);
        DrawTextW(
            draw.hdc,
            text.as_ptr(),
            len,
            &mut rect,
            align | DT_SINGLELINE | DT_VCENTER,
        );
        if draw.uItemState & CDIS_FOCUS != 0 {
            rect.left += 3;
            rect.top += 3;
            rect.right -= 3;
            rect.bottom -= 3;
            DrawFocusRect(draw.hdc, &rect);
        }
    }
    Some(CDRF_SKIPDEFAULT as LRESULT)
}

/// `WM_CTLCOLORSTATIC` / `WM_CTLCOLORBTN` answer: text in `text_color` on
/// the shared background, so labels and checkboxes blend into the window.
pub fn paint_on_background(hdc: HDC, text_color: u32) -> LRESULT {
    unsafe {
        SetTextColor(hdc, text_color);
        SetBkColor(hdc, bg_color());
        SetBkMode(hdc, TRANSPARENT as i32);
    }
    bg_brush() as LRESULT
}
