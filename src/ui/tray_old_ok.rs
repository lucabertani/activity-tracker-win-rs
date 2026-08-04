use std::mem;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_PITCH, DEFAULT_QUALITY, DeleteDC, DeleteObject, FF_DONTCARE, FillRect,
    GetDC, HBRUSH, HGDIOBJ, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT, TextOutW,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, ICONINFO, IDI_APPLICATION, LoadIconW,
    SM_CXSMICON, SM_CYSMICON,
};

pub const TRAY_UID: u32 = 1;
pub const WM_TRAYICON: u32 = 0x0400 + 1;

// COLORREF format is 0x00BBGGRR.
pub const TRAY_BG_COLOR: u32 = 0x00800040; // #400080
//pub const TRAY_TEXT_COLOR: u32 = 0x00FEFEFE; // #fefefe
pub const TRAY_TEXT_COLOR: u32 = 0x00FFFFFF; // #ffffff

// Font: name, weight (400 = Normal, 600 = SemiBold, 700 = Bold).
pub const TRAY_FONT_NAME: &str = "Segoe UI";
pub const TRAY_FONT_WEIGHT: i32 = 500;

// Font height = (icon_height - offset).max(min_px), per digit count.
pub const TRAY_FONT_OFFSET_1: i32 = 2;
pub const TRAY_FONT_MIN_1: i32 = 13;
pub const TRAY_FONT_OFFSET_2: i32 = 3;
pub const TRAY_FONT_MIN_2: i32 = 18;
pub const TRAY_FONT_OFFSET_3: i32 = 6;
pub const TRAY_FONT_MIN_3: i32 = 10;

// Estimated pixel width per character used for horizontal centering.
pub const TRAY_CHAR_PX_NORMAL: i32 = 7; // 1–2 digit numbers
pub const TRAY_CHAR_PX_WIDE: i32 = 5;   // 3 digit numbers

// Minimum icon size in pixels (fallback when GetSystemMetrics returns too small).
pub const TRAY_ICON_MIN_SIZE: i32 = 16;

pub fn add_tray_icon(hwnd: HWND, tip_text: &str, icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) -> bool {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = if icon.is_null() {
        unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
    } else {
        icon
    };
    write_tip(&mut nid, tip_text);

    unsafe { Shell_NotifyIconW(NIM_ADD, &nid) != 0 }
}

pub fn update_tray_tooltip(hwnd: HWND, tip_text: &str) {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_TIP;
    write_tip(&mut nid, tip_text);

    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn update_tray_icon(hwnd: HWND, icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_ICON;
    nid.hIcon = icon;

    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;

    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn destroy_icon(icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) {
    if !icon.is_null() {
        unsafe {
            let _ = DestroyIcon(icon);
        }
    }
}

pub fn build_percentage_icon(percent: u32) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let value = percent.min(100);
    let text = if value == 100 {
        "100".to_string()
    } else {
        value.to_string()
    };

    let width = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(TRAY_ICON_MIN_SIZE);
    let height = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(TRAY_ICON_MIN_SIZE);
    let digits = text.len() as i32;
    let font_height = match digits {
        1 => (height - TRAY_FONT_OFFSET_1).max(TRAY_FONT_MIN_1),
        2 => (height - TRAY_FONT_OFFSET_2).max(TRAY_FONT_MIN_2),
        _ => (height - TRAY_FONT_OFFSET_3).max(TRAY_FONT_MIN_3),
    };

    unsafe {
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return std::ptr::null_mut();
        }

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);
            return std::ptr::null_mut();
        }

        let color_bmp = CreateCompatibleBitmap(screen_dc, width, height);
        if color_bmp.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);
            return std::ptr::null_mut();
        }

        let old_obj = SelectObject(mem_dc, color_bmp as HGDIOBJ);

        let background = CreateSolidBrush(TRAY_BG_COLOR);
        if !background.is_null() {
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let _ = FillRect(mem_dc, &rect, background as HBRUSH);
        }

        let font_name = wide(TRAY_FONT_NAME);
        let font = CreateFontW(
            font_height,
            0,
            0,
            0,
            TRAY_FONT_WEIGHT,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            0,
            DEFAULT_QUALITY as u32,
            ((DEFAULT_PITCH as u32) | ((FF_DONTCARE as u32) << 4)) as u32,
            font_name.as_ptr(),
        );

        if !font.is_null() {
            let _ = SelectObject(mem_dc, font as HGDIOBJ);
        }

        let _ = SetBkMode(mem_dc, TRANSPARENT as i32);
        let _ = SetTextColor(mem_dc, TRAY_TEXT_COLOR);

        let text_w: Vec<u16> = text.encode_utf16().collect();
        let per_char = if digits >= 3 { TRAY_CHAR_PX_WIDE } else { TRAY_CHAR_PX_NORMAL };
        let text_px = (digits * per_char).max(6);
        let x = (width - text_px) / 2;
        let y = ((height - font_height) / 2).max(0);
        let _ = TextOutW(mem_dc, x.max(0), y, text_w.as_ptr(), text_w.len() as i32);

        let mask_bmp = CreateCompatibleBitmap(screen_dc, width, height);

        let mut icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&mut icon_info);

        if !font.is_null() {
            let _ = DeleteObject(font as HGDIOBJ);
        }
        if !background.is_null() {
            let _ = DeleteObject(background as HGDIOBJ);
        }

        let _ = SelectObject(mem_dc, old_obj);
        let _ = DeleteObject(mask_bmp as HGDIOBJ);
        let _ = DeleteObject(color_bmp as HGDIOBJ);
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);

        icon
    }
}

fn write_tip(nid: &mut NOTIFYICONDATAW, text: &str) {
    let mut encoded: Vec<u16> = text.encode_utf16().take(127).collect();
    encoded.push(0);
    for (idx, ch) in encoded.into_iter().enumerate() {
        nid.szTip[idx] = ch;
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
