use std::mem;

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection,
    CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DeleteDC, DeleteObject,
    FF_DONTCARE, FillRect, GdiFlush, GetDC, HBRUSH, HGDIOBJ, NONANTIALIASED_QUALITY, OPAQUE,
    OUT_DEFAULT_PRECIS, RGBQUAD, ReleaseDC, SelectObject, SetBkColor, SetBkMode, SetTextColor,
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

pub const TRAY_UID: u32 = 1;
pub const WM_TRAYICON: u32 = 0x0400 + 1;

// COLORREF format is 0x00BBGGRR.
pub const TRAY_BG_COLOR: u32 = 0x00800040; // #400080
//pub const TRAY_TEXT_COLOR: u32 = 0x00FEFEFE; // #fefefe
pub const TRAY_TEXT_COLOR: u32 = 0x00FFFFFF; // #ffffff

// Font: name, weight (400 = Normal, 600 = SemiBold, 700 = Bold),
// quality (NONANTIALIASED = no color fringing on colored backgrounds).
pub const TRAY_FONT_NAME: &str = "Segoe UI";
pub const TRAY_FONT_WEIGHT: i32 = 500;
pub const TRAY_FONT_QUALITY: u32 = NONANTIALIASED_QUALITY as u32;

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
        // --- 1. Create a 32bpp DIB section: gives us a raw pixel pointer ---
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // negative = top-down, row 0 is at the top
                biPlanes: 1,
                biBitCount: 32,
                biCompression: 0, // BI_RGB
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
        };

        let mem_dc = CreateCompatibleDC(std::ptr::null_mut());
        if mem_dc.is_null() {
            return std::ptr::null_mut();
        }

        let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let color_bmp = CreateDIBSection(
            mem_dc,
            &bmi,
            0, // DIB_RGB_COLORS
            &mut bits_ptr,
            std::ptr::null_mut(),
            0,
        );
        if color_bmp.is_null() || bits_ptr.is_null() {
            let _ = DeleteDC(mem_dc);
            return std::ptr::null_mut();
        }

        let old_obj = SelectObject(mem_dc, color_bmp as HGDIOBJ);

        // --- 2. Fill background ---
        let background = CreateSolidBrush(TRAY_BG_COLOR);
        if !background.is_null() {
            let rect = RECT { left: 0, top: 0, right: width, bottom: height };
            let _ = FillRect(mem_dc, &rect, background as HBRUSH);
            let _ = DeleteObject(background as HGDIOBJ);
        }

        // --- 3. Draw text with GDI (antialiasing may still occur here) ---
        let font_name = wide(TRAY_FONT_NAME);
        let font = CreateFontW(
            font_height,
            0, 0, 0,
            TRAY_FONT_WEIGHT,
            0, 0, 0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            0,
            TRAY_FONT_QUALITY,
            ((DEFAULT_PITCH as u32) | ((FF_DONTCARE as u32) << 4)) as u32,
            font_name.as_ptr(),
        );
        if !font.is_null() {
            let _ = SelectObject(mem_dc, font as HGDIOBJ);
        }
        let _ = SetBkMode(mem_dc, OPAQUE as i32);
        let _ = SetBkColor(mem_dc, TRAY_BG_COLOR);
        let _ = SetTextColor(mem_dc, TRAY_TEXT_COLOR);

        let text_w: Vec<u16> = text.encode_utf16().collect();
        let per_char = if digits >= 3 { TRAY_CHAR_PX_WIDE } else { TRAY_CHAR_PX_NORMAL };
        let text_px = (digits * per_char).max(6);
        let x = (width - text_px) / 2;
        let y = ((height - font_height) / 2).max(0);
        let _ = TextOutW(mem_dc, x.max(0), y, text_w.as_ptr(), text_w.len() as i32);

        if !font.is_null() {
            let _ = DeleteObject(font as HGDIOBJ);
        }

        // --- 4. Deselect bitmap and flush GDI before accessing pixel data ---
        // GDI batches operations; accessing bits_ptr before flushing yields stale/empty pixels.
        let _ = SelectObject(mem_dc, old_obj);
        let _ = DeleteDC(mem_dc);
        GdiFlush();

        // --- 5. Pixel-snap + set alpha = 0xFF on every pixel ---
        // Forces every pixel to be exactly bg or text color with full opacity.
        // Without alpha=0xFF, DWM treats pixels as transparent and blends them
        // with the taskbar background, producing the "antialiasing" fringing.
        //
        // In a 32bpp DIB: pixel bytes = [B, G, R, A]. As little-endian u32:
        // B | (G<<8) | (R<<16) | (A<<24). COLORREF = R | (G<<8) | (B<<16),
        // so we swap R↔B to convert.
        let colorref_to_dib = |c: u32| -> u32 {
            let r = c & 0xFF;
            let g = (c >> 8) & 0xFF;
            let b = (c >> 16) & 0xFF;
            b | (g << 8) | (r << 16)
        };
        let bg_dib   = colorref_to_dib(TRAY_BG_COLOR);
        let text_dib = colorref_to_dib(TRAY_TEXT_COLOR);
        const ALPHA_OPAQUE: u32 = 0xFF00_0000;

        let pixels = bits_ptr as *mut u32;
        for i in 0..(width * height) as isize {
            let px = *pixels.offset(i) & 0x00FF_FFFF;
            let color = if px != bg_dib { text_dib } else { bg_dib };
            *pixels.offset(i) = color | ALPHA_OPAQUE;
        }

        // --- 6. Build icon from the snapped bitmap ---
        let screen_dc = GetDC(std::ptr::null_mut());
        let mask_bmp = CreateCompatibleBitmap(screen_dc, width, height);
        let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);

        let mut icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&mut icon_info);

        let _ = DeleteObject(mask_bmp as HGDIOBJ);
        let _ = DeleteObject(color_bmp as HGDIOBJ);

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
