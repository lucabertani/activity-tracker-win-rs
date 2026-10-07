use std::mem;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DeleteDC, DeleteObject, FF_DONTCARE, FillRect, GetDC, HBRUSH,
    HGDIOBJ, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    TextOutW,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, ICONINFO, IDI_APPLICATION, LoadIconW,
    SM_CXSMICON, SM_CYSMICON,
};

pub const TRAY_UID_RAM: u32 = 1;
pub const TRAY_UID_CPU: u32 = 2;
/// Shown only while both the RAM and CPU icons are disabled.
pub const TRAY_UID_FALLBACK: u32 = 3;
pub const WM_TRAYICON: u32 = 0x0400 + 1;

// COLORREF format is 0x00BBGGRR.
//pub const TRAY_BG_COLOR: u32 = 0x00800040; // #400080
//pub const TRAY_BG_COLOR: u32 = 0x00353138; // #cbc6ce
pub const TRAY_BG_COLOR: u32 = 0x003e3e3e; // #3e3e3e
pub const TRAY_BG_RED: u32 = 0x0000FF; // #ff0000, used while thermal-throttling
pub const TRAY_TEXT_COLOR: u32 = 0x00FFFFFF; // #ffffff

// Font settings.
//pub const TRAY_FONT_NAME: &str = "Tahoma";
//pub const TRAY_FONT_NAME: &str = "Segoe Fluent Icons";
//pub const TRAY_FONT_NAME: &str = "Segoe UI Variable";
pub const TRAY_FONT_NAME: &str = "Segoe UI";
//pub const TRAY_FONT_NAME: &str = "Verdana";
pub const TRAY_FONT_WEIGHT: i32 = 350;

// Font height = (icon_height - offset).max(min_px), per digit count.
pub const TRAY_FONT_OFFSET_1: i32 = 2;
pub const TRAY_FONT_MIN_1: i32 = 16;
pub const TRAY_FONT_OFFSET_2: i32 = 3;
pub const TRAY_FONT_MIN_2: i32 = 18;
pub const TRAY_FONT_OFFSET_3: i32 = 5;
pub const TRAY_FONT_MIN_3: i32 = 10;

// Estimated pixel width per character used for horizontal centering.
pub const TRAY_CHAR_PX_NORMAL: i32 = 7;
pub const TRAY_CHAR_PX_WIDE: i32 = 5;

// Minimum icon size fallback.
pub const TRAY_ICON_MIN_SIZE: i32 = 16;

pub fn add_tray_icon(
    hwnd: HWND,
    uid: u32,
    tip_text: &str,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) -> bool {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };

    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = uid;
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

pub fn update_tray_tooltip(hwnd: HWND, uid: u32, tip_text: &str) {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };

    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = uid;
    nid.uFlags = NIF_TIP;

    write_tip(&mut nid, tip_text);

    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn update_tray_icon(
    hwnd: HWND,
    uid: u32,
    icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
) {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };

    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = uid;
    nid.uFlags = NIF_ICON;
    nid.hIcon = icon;

    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

pub fn remove_tray_icon(hwnd: HWND, uid: u32) {
    let mut nid: NOTIFYICONDATAW = unsafe { mem::zeroed() };

    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = uid;

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

/// RAM tray icon: percentage number, red background while throttling.
pub fn build_percentage_icon(
    percent: u32,
    red: bool,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    build_ram_icon(percent, red, TRAY_BG_COLOR, TRAY_TEXT_COLOR, 0)
}

/// RAM tray icon with custom colors. `font_size` 0 = automatic.
pub fn build_ram_icon(
    percent: u32,
    red: bool,
    bg: u32,
    fg: u32,
    font_size: u32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    build_text_icon(
        &percent.min(100).to_string(),
        if red { TRAY_BG_RED } else { bg },
        fg,
        font_size,
    )
}

/// CPU tray icon: average temperature (e.g. `62`), or `--` when no
/// temperature source is available (WinRing0 driver missing).
/// `font_size` 0 = automatic.
pub fn build_cpu_icon(
    temp_celsius: Option<u32>,
    red: bool,
    bg: u32,
    fg: u32,
    font_size: u32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let text = match temp_celsius {
        Some(t) => t.to_string(),
        None => "--".to_string(),
    };
    build_text_icon(&text, if red { TRAY_BG_RED } else { bg }, fg, font_size)
}

/// Fallback tray icon shown when both the RAM and CPU icons are disabled:
/// a plain "AT" (Activity Tracker) badge, so the app is still visibly
/// running in the background.
pub fn build_fallback_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    build_text_icon("AT", TRAY_BG_COLOR, TRAY_TEXT_COLOR, 0)
}

/// Draws `text` centered on a solid `bg` with `fg` text color and returns a
/// small system icon (the shared renderer for both tray icons).
/// `font_size` is the text height in pixels; 0 = automatic (fit to the
/// icon size and digit count).
pub fn build_text_icon(
    text: &str,
    bg: u32,
    fg: u32,
    font_size: u32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let width = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(TRAY_ICON_MIN_SIZE);

    let height = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(TRAY_ICON_MIN_SIZE);

    let digits = text.chars().count() as i32;

    let font_height = if font_size != 0 {
        (font_size as i32).max(6).min(height - 2)
    } else {
        match digits {
            1 => (height - TRAY_FONT_OFFSET_1).max(TRAY_FONT_MIN_1),
            2 => (height - TRAY_FONT_OFFSET_2).max(TRAY_FONT_MIN_2),
            _ => (height - TRAY_FONT_OFFSET_3).max(TRAY_FONT_MIN_3),
        }
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

        // Background
        let background = CreateSolidBrush(bg);

        if !background.is_null() {
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };

            let _ = FillRect(mem_dc, &rect, background as HBRUSH);
        }

        // Font
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
            ANTIALIASED_QUALITY as u32,
            (DEFAULT_PITCH | (FF_DONTCARE << 4)) as u32,
            font_name.as_ptr(),
        );

        if !font.is_null() {
            let _ = SelectObject(mem_dc, font as HGDIOBJ);
        }

        let _ = SetBkMode(mem_dc, TRANSPARENT as i32);
        let _ = SetTextColor(mem_dc, fg);

        // Text rendering
        let text_w: Vec<u16> = text.encode_utf16().collect();

        let per_char = if digits >= 3 {
            TRAY_CHAR_PX_WIDE
        } else {
            TRAY_CHAR_PX_NORMAL
        };

        let text_px = (digits * per_char).max(6);

        let x = (width - text_px) / 2;
        let y = ((height - font_height) / 2).max(0);

        let _ = TextOutW(mem_dc, x.max(0), y, text_w.as_ptr(), text_w.len() as i32);

        // Mask bitmap
        let mask_bmp = CreateCompatibleBitmap(screen_dc, width, height);

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };

        let icon = CreateIconIndirect(&icon_info);

        // Cleanup
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

#[cfg(test)]
mod tests {
    use super::build_percentage_icon;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetPixel,
        HGDIOBJ, ReleaseDC, SelectObject,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx};

    /// Renders the icon into an off-screen DC and returns the COLORREF of
    /// the pixel at (2,2) (a background corner: the percentage text is
    /// centered, so corners are always background).
    fn corner_pixel(icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON) -> u32 {
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, 16, 16);
            let old = SelectObject(mem, bmp as HGDIOBJ);
            let _ = DrawIconEx(mem, 0, 0, icon, 16, 16, 0, std::ptr::null_mut(), DI_NORMAL);
            let px = GetPixel(mem, 2, 2);
            let _ = SelectObject(mem, old);
            let _ = DeleteObject(bmp as HGDIOBJ);
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(std::ptr::null_mut(), screen);
            px
        }
    }

    #[test]
    fn thermal_icon_is_red_normal_icon_is_dark() {
        let red = build_percentage_icon(42, true);
        let normal = build_percentage_icon(42, false);
        assert!(!red.is_null() && !normal.is_null());

        // COLORREF is 0x00BBGGRR.
        let red_px = corner_pixel(red);
        assert!(
            (red_px & 0xFF) > 0x80
                && ((red_px >> 8) & 0xFF) < 0x80
                && ((red_px >> 16) & 0xFF) < 0x80,
            "throttling icon should be red, got 0x{red_px:06X}"
        );

        let normal_px = corner_pixel(normal);
        assert!(
            (normal_px & 0xFF) < 0x80 && ((normal_px >> 8) & 0xFF) < 0x80,
            "normal icon should be the dark background, got 0x{normal_px:06X}"
        );
    }
}
