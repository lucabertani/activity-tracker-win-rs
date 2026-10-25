use std::mem;

use windows_sys::Win32::Foundation::{HWND, RECT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, FF_DONTCARE, FillRect, GetDC, GetDIBits,
    GetTextExtentPoint32W, HBRUSH, HGDIOBJ, OUT_DEFAULT_PRECIS, ReleaseDC, SelectObject, SetBkMode,
    SetDIBits, SetTextColor, TRANSPARENT, TextOutW,
};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, GetSystemMetrics, ICONINFO, IDI_APPLICATION, LoadIconW,
    SM_CXSMICON, SM_CYSMICON,
};

use crate::model::{AppSettings, DEFAULT_TRAY_FONT_WEIGHT, ThermalStatus};

#[allow(clippy::upper_case_acronyms)]
type HICON = windows_sys::Win32::UI::WindowsAndMessaging::HICON;
pub use crate::model::{TRAY_BG_COLOR, TRAY_TEXT_COLOR};

pub const TRAY_UID_RAM: u32 = 1;
pub const TRAY_UID_CPU: u32 = 2;
/// Shown only while both the RAM and CPU icons are disabled.
pub const TRAY_UID_FALLBACK: u32 = 3;
pub const WM_TRAYICON: u32 = 0x0400 + 1;

// COLORREF format is 0x00BBGGRR.
pub const TRAY_BG_RED: u32 = 0x0000FF; // #ff0000, used while thermal-throttling

// Font settings.
//pub const TRAY_FONT_NAME: &str = "Tahoma";
//pub const TRAY_FONT_NAME: &str = "Segoe Fluent Icons";
// Plain "Segoe UI" is a static font: it only has a handful of fixed
// weights, so the weight selector would collapse to 2-3 looks. The
// Windows 11 variable font (segoeuiz.ttf) has a continuous weight axis,
// which is what makes the 100..550 slider smooth.
pub const TRAY_FONT_NAME: &str = "Segoe UI Variable";
//pub const TRAY_FONT_NAME: &str = "Segoe UI";
//pub const TRAY_FONT_NAME: &str = "Verdana";

// Font height = (icon_height - offset).max(min_px), per digit count
// (automatic mode only; an explicit size from the settings is used as-is).
pub const TRAY_FONT_OFFSET_1: i32 = 2;
pub const TRAY_FONT_MIN_1: i32 = 16;
pub const TRAY_FONT_OFFSET_2: i32 = 3;
pub const TRAY_FONT_MIN_2: i32 = 18;
pub const TRAY_FONT_OFFSET_3: i32 = 5;
pub const TRAY_FONT_MIN_3: i32 = 10;

// The icon is drawn at this multiple of its size and box-filtered back
// down: GDI grid-fits small fonts, so without supersampling 15 px and
// 16 px would rasterize identically. It also gives smoother edges.
const ICON_SUPERSAMPLE: i32 = 4;

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
    build_ram_icon(
        percent,
        red,
        TRAY_BG_COLOR,
        TRAY_TEXT_COLOR,
        0,
        DEFAULT_TRAY_FONT_WEIGHT,
    )
}

/// RAM tray icon with custom colors. `font_size` 0 = automatic.
pub fn build_ram_icon(
    percent: u32,
    red: bool,
    bg: u32,
    fg: u32,
    font_size: u32,
    weight: u32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let (text, bg) = ram_icon_face(percent, red, bg);
    build_text_icon(&text, bg, fg, font_size, weight)
}

/// CPU tray icon: average temperature (e.g. `62`), or `--` when no
/// temperature source is available (PawnIO driver missing).
/// `font_size` 0 = automatic.
pub fn build_cpu_icon(
    temp_celsius: Option<u32>,
    red: bool,
    bg: u32,
    fg: u32,
    font_size: u32,
    weight: u32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let (text, bg) = cpu_icon_face(temp_celsius, red, bg);
    build_text_icon(&text, bg, fg, font_size, weight)
}

/// Fallback tray icon shown when both the RAM and CPU icons are disabled:
/// a plain "AT" (Activity Tracker) badge, so the app is still visibly
/// running in the background.
pub fn build_fallback_icon(weight: u32) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    build_text_icon("AT", TRAY_BG_COLOR, TRAY_TEXT_COLOR, 0, weight)
}

/// Pixels of a rendered tray icon: top-down rows, 4 bytes per pixel in
/// B, G, R, A order (the GDI DIB layout), fully opaque.
pub struct IconPixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Draws `text` centered on a solid `bg` with `fg` text color and returns a
/// small system icon (the shared renderer for both tray icons).
/// `font_size` is the text height in pixels; 0 = automatic (fit to the
/// icon size and digit count). Explicit sizes larger than the icon are
/// clipped at the edges (the settings preview shows exactly that).
/// `weight` is the Segoe UI font weight (300 light, 400 regular, 550 heavy).
pub fn build_text_icon(
    text: &str,
    bg: u32,
    fg: u32,
    font_size: u32,
    weight: u32,
) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    match render_text_pixels(text, bg, fg, font_size, weight) {
        Some(pixels) => icon_from_pixels(&pixels),
        None => std::ptr::null_mut(),
    }
}

/// The pixels [`build_text_icon`] turns into an icon, at the system small
/// icon size. Also used for the settings previews.
pub fn render_text_pixels(
    text: &str,
    bg: u32,
    fg: u32,
    font_size: u32,
    weight: u32,
) -> Option<IconPixels> {
    let width = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(TRAY_ICON_MIN_SIZE);

    let height = unsafe { GetSystemMetrics(SM_CYSMICON) }.max(TRAY_ICON_MIN_SIZE);

    let ss = ICON_SUPERSAMPLE;
    let big_w = width * ss;
    let big_h = height * ss;

    let digits = text.chars().count() as i32;

    // Automatic size in final-icon pixels; an explicit size is used as-is
    // (floor at 6 px), so every step of the settings combo is visible.
    let unit_font_height = if font_size != 0 {
        (font_size as i32).max(6)
    } else {
        match digits {
            1 => (height - TRAY_FONT_OFFSET_1).max(TRAY_FONT_MIN_1),
            2 => (height - TRAY_FONT_OFFSET_2).max(TRAY_FONT_MIN_2),
            _ => (height - TRAY_FONT_OFFSET_3).max(TRAY_FONT_MIN_3),
        }
    };
    let font_height = unit_font_height * ss;

    unsafe {
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return None;
        }

        let mem_dc = CreateCompatibleDC(screen_dc);
        if mem_dc.is_null() {
            let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);
            return None;
        }

        let big_bmp = CreateCompatibleBitmap(screen_dc, big_w, big_h);
        if big_bmp.is_null() {
            let _ = DeleteDC(mem_dc);
            let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);
            return None;
        }

        let old_obj = SelectObject(mem_dc, big_bmp as HGDIOBJ);

        // Background (full supersampled area)
        let background = CreateSolidBrush(bg);
        if !background.is_null() {
            let rect = RECT {
                left: 0,
                top: 0,
                right: big_w,
                bottom: big_h,
            };
            let _ = FillRect(mem_dc, &rect, background as HBRUSH);
        }

        // Font (height scaled by the supersample factor)
        let font_name = wide(TRAY_FONT_NAME);
        let font = CreateFontW(
            font_height,
            0,
            0,
            0,
            weight as i32,
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
        let old_font = if font.is_null() {
            std::ptr::null_mut()
        } else {
            SelectObject(mem_dc, font as HGDIOBJ)
        };

        let _ = SetBkMode(mem_dc, TRANSPARENT as i32);
        let _ = SetTextColor(mem_dc, fg);

        // Exact text extent for perfect centering (no per-character guess).
        let text_w: Vec<u16> = text.encode_utf16().collect();
        let mut extent: SIZE = mem::zeroed();
        let _ = GetTextExtentPoint32W(mem_dc, text_w.as_ptr(), text_w.len() as i32, &mut extent);

        let x = ((big_w - extent.cx) / 2).max(0);
        let y = ((big_h - extent.cy) / 2).max(0);
        let _ = TextOutW(mem_dc, x, y, text_w.as_ptr(), text_w.len() as i32);

        // Read back the supersampled buffer (0xAARRGGBB, top-down).
        let mut big_bmi = dib_info(big_w, big_h);
        let mut big_buf = vec![0u8; (big_w * big_h * 4) as usize];
        let read_lines = GetDIBits(
            mem_dc,
            big_bmp,
            0,
            big_h as u32,
            big_buf.as_mut_ptr() as *mut _,
            &mut big_bmi,
            DIB_RGB_COLORS,
        );

        // GDI cleanup: everything below works on the copied pixels.
        if !old_font.is_null() {
            let _ = SelectObject(mem_dc, old_font);
        }
        if !font.is_null() {
            let _ = DeleteObject(font as HGDIOBJ);
        }
        if !background.is_null() {
            let _ = DeleteObject(background as HGDIOBJ);
        }
        let _ = SelectObject(mem_dc, old_obj);
        let _ = DeleteObject(big_bmp as HGDIOBJ);
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);

        if read_lines <= 0 {
            return None;
        }

        Some(IconPixels {
            width: width as u32,
            height: height as u32,
            bgra: downsample(&big_buf, width, height, ss),
        })
    }
}

/// Box-filter downscale: each output pixel is the average of its `ss x ss`
/// block. The icon is fully opaque, so alpha is 0xFF.
fn downsample(big_buf: &[u8], width: i32, height: i32, ss: i32) -> Vec<u8> {
    let big_w = width * ss;
    let mut small_buf = vec![0u8; (width * height * 4) as usize];

    for py in 0..height {
        for px in 0..width {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);

            for dy in 0..ss {
                for dx in 0..ss {
                    let i =
                        (((py * ss + dy) as usize) * big_w as usize + (px * ss + dx) as usize) * 4;
                    r += big_buf[i + 2] as u32;
                    g += big_buf[i + 1] as u32;
                    b += big_buf[i] as u32;
                }
            }

            let n = (ss * ss) as u32;
            let o = ((py as usize) * width as usize + px as usize) * 4;
            small_buf[o] = (b / n) as u8;
            small_buf[o + 1] = (g / n) as u8;
            small_buf[o + 2] = (r / n) as u8;
            small_buf[o + 3] = 0xFF;
        }
    }

    small_buf
}

fn icon_from_pixels(pixels: &IconPixels) -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    let width = pixels.width as i32;
    let height = pixels.height as i32;

    unsafe {
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return std::ptr::null_mut();
        }

        let color_bmp = CreateCompatibleBitmap(screen_dc, width, height);
        let mask_bmp = CreateCompatibleBitmap(screen_dc, width, height);
        let mut icon = std::ptr::null_mut();

        if !color_bmp.is_null() && !mask_bmp.is_null() {
            let small_bmi = dib_info(width, height);
            let _ = SetDIBits(
                screen_dc,
                color_bmp,
                0,
                height as u32,
                pixels.bgra.as_ptr() as *const _,
                &small_bmi,
                DIB_RGB_COLORS,
            );

            let icon_info = ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask_bmp,
                hbmColor: color_bmp,
            };
            // The icon keeps its own copies of both bitmaps.
            icon = CreateIconIndirect(&icon_info);
        }

        if !mask_bmp.is_null() {
            let _ = DeleteObject(mask_bmp as HGDIOBJ);
        }
        if !color_bmp.is_null() {
            let _ = DeleteObject(color_bmp as HGDIOBJ);
        }
        let _ = ReleaseDC(std::ptr::null_mut(), screen_dc);

        icon
    }
}

/// Text and background of the RAM icon (red while throttling).
fn ram_icon_face(percent: u32, red: bool, bg: u32) -> (String, u32) {
    (
        percent.min(100).to_string(),
        if red { TRAY_BG_RED } else { bg },
    )
}

/// Text and background of the CPU icon: the temperature, or `--` without a
/// temperature source.
fn cpu_icon_face(temp_celsius: Option<u32>, red: bool, bg: u32) -> (String, u32) {
    let text = temp_celsius.map_or_else(|| "--".to_string(), |t| t.to_string());
    (text, if red { TRAY_BG_RED } else { bg })
}

/// Pixels of the RAM icon as configured in `settings` (settings preview).
pub fn ram_icon_pixels(percent: u32, settings: &AppSettings) -> Option<IconPixels> {
    let (text, bg) = ram_icon_face(percent, false, settings.tray_icon_ram_bg);
    render_text_pixels(
        &text,
        bg,
        settings.tray_icon_ram_fg,
        settings.tray_icon_font_size,
        settings.tray_icon_font_weight,
    )
}

/// Pixels of the CPU icon as configured in `settings` (settings preview).
pub fn cpu_icon_pixels(temp_celsius: Option<u32>, settings: &AppSettings) -> Option<IconPixels> {
    let (text, bg) = cpu_icon_face(temp_celsius, false, settings.tray_icon_cpu_bg);
    render_text_pixels(
        &text,
        bg,
        settings.tray_icon_cpu_fg,
        settings.tray_icon_font_size,
        settings.tray_icon_font_weight,
    )
}

/// The three tray icons (RAM, CPU, fallback "AT") of one window, kept in
/// sync with the settings and the latest readings.
pub struct TrayIcons {
    ram: HICON,
    cpu: HICON,
    fallback: HICON,
}

impl Default for TrayIcons {
    fn default() -> Self {
        Self {
            ram: std::ptr::null_mut(),
            cpu: std::ptr::null_mut(),
            fallback: std::ptr::null_mut(),
        }
    }
}

impl TrayIcons {
    /// Adds, updates or removes each icon according to its enable flag,
    /// redrawn with the configured colors (red background on the CPU icon,
    /// or on the RAM icon when the CPU one is off, while thermal
    /// throttling and the option is on).
    pub fn apply(
        &mut self,
        hwnd: HWND,
        settings: &AppSettings,
        ram_percent: u32,
        thermal: &ThermalStatus,
    ) {
        let red = settings.tray_icon_thermal_red && thermal.is_throttling();
        // Only one icon lights up: the CPU one when shown, otherwise the RAM one.
        let cpu_red = red && settings.tray_icon_cpu_enabled;
        let ram_red = red && !settings.tray_icon_cpu_enabled;

        let ram_tip = format!("RAM {ram_percent}%");
        apply_tray_icon(
            hwnd,
            TRAY_UID_RAM,
            &mut self.ram,
            settings.tray_icon_ram_enabled,
            || {
                build_ram_icon(
                    ram_percent,
                    ram_red,
                    settings.tray_icon_ram_bg,
                    settings.tray_icon_ram_fg,
                    settings.tray_icon_font_size,
                    settings.tray_icon_font_weight,
                )
            },
            &ram_tip,
        );

        let temp = thermal.avg_temp_c.map(|t| t as u32);
        let cpu_tip = match temp {
            Some(t) => format!("CPU avg {t} C\n{}", thermal.summary()),
            None => "CPU avg: n/a (PawnIO driver missing)".to_string(),
        };
        apply_tray_icon(
            hwnd,
            TRAY_UID_CPU,
            &mut self.cpu,
            settings.tray_icon_cpu_enabled,
            || {
                build_cpu_icon(
                    temp,
                    cpu_red,
                    settings.tray_icon_cpu_bg,
                    settings.tray_icon_cpu_fg,
                    settings.tray_icon_font_size,
                    settings.tray_icon_font_weight,
                )
            },
            &cpu_tip,
        );

        // With both icons off the tray would go silent: keep a plain "AT" badge
        // so the app is still visibly running in the background.
        let show_fallback = !settings.tray_icon_ram_enabled && !settings.tray_icon_cpu_enabled;
        apply_tray_icon(
            hwnd,
            TRAY_UID_FALLBACK,
            &mut self.fallback,
            show_fallback,
            || build_fallback_icon(settings.tray_icon_font_weight),
            "Activity Tracker",
        );
    }

    /// Removes every icon from the tray and frees them.
    pub fn remove_all(&mut self, hwnd: HWND) {
        for (uid, icon) in [
            (TRAY_UID_RAM, &mut self.ram),
            (TRAY_UID_CPU, &mut self.cpu),
            (TRAY_UID_FALLBACK, &mut self.fallback),
        ] {
            remove_tray_icon(hwnd, uid);
            destroy_icon(*icon);
            *icon = std::ptr::null_mut();
        }
    }
}

fn apply_tray_icon(
    hwnd: HWND,
    uid: u32,
    current: &mut HICON,
    enabled: bool,
    build: impl FnOnce() -> HICON,
    tip: &str,
) {
    if !enabled {
        if !current.is_null() {
            remove_tray_icon(hwnd, uid);
            destroy_icon(*current);
            *current = std::ptr::null_mut();
        }
        return;
    }

    let icon = build();
    if icon.is_null() {
        return;
    }

    if current.is_null() {
        if add_tray_icon(hwnd, uid, tip, icon) {
            *current = icon;
        } else {
            destroy_icon(icon);
        }
    } else {
        update_tray_icon(hwnd, uid, icon);
        update_tray_tooltip(hwnd, uid, tip);
        destroy_icon(*current);
        *current = icon;
    }
}

/// Top-down 32-bit DIB header for read/write-back of screen-compatible
/// bitmaps (the memory layout is the raw 0xAARRGGBB pixels).
fn dib_info(width: i32, height: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        ..Default::default()
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
