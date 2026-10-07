//! Modeless settings window with a native tab control:
//! - "General" tab: autostart, background start, thermal-red icons, refresh
//!   interval
//! - "Tray icons" tab: enable/disable the RAM and CPU tray icons, live icon
//!   previews (SS_ICON statics) and background/text color pickers (clickable
//!   SS_NOTIFY swatches that open the native ChooseColor dialog)
//!
//! Everything is plain Win32: no third-party GUI libraries.

#![allow(unsafe_op_in_unsafe_fn)]

use std::mem;

use windows_sys::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, HBRUSH, HDC, OPAQUE, SetBkMode, SetTextColor, UpdateWindow,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::Dialogs::{
    CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW, ChooseColorW,
};
use windows_sys::Win32::UI::Controls::{
    ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, NMHDR, TCIF_TEXT, TCITEMW,
    TCM_GETCURSEL, TCM_INSERTITEMW, TCN_SELCHANGE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BN_CLICKED, BS_AUTOCHECKBOX, CB_ADDSTRING, CB_GETCURSEL,
    CB_SETCURSEL, CBN_SELCHANGE, CBS_DROPDOWNLIST, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, GWLP_HWNDPARENT,
    GWLP_USERDATA, GetDlgCtrlID, GetDlgItem, GetWindowLongPtrW, HMENU, IDC_ARROW, LoadCursorW,
    RegisterClassW, STN_CLICKED, SW_HIDE, SW_RESTORE, SW_SHOW, SendMessageW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowTextW, ShowWindow, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_CTLCOLORSTATIC, WM_DESTROY, WM_NOTIFY, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE,
    WS_EX_CONTROLPARENT, WS_EX_DLGMODALFRAME, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use crate::app::{AppState, refresh_tray_icons, restart_refresh_timer, show_error};
use crate::model::{
    AppSettings, DEFAULT_TRAY_CPU_BG, DEFAULT_TRAY_CPU_FG, DEFAULT_TRAY_ICON_FONT_SIZE,
    DEFAULT_TRAY_RAM_BG, DEFAULT_TRAY_RAM_FG, parse_refresh_interval_seconds,
};
use crate::settings::autostart::set_autostart;
use crate::settings::store::save_settings;
use crate::ui::tray::{build_cpu_icon, build_ram_icon, destroy_icon};
use crate::ui::window::{get_control_text, wide};

pub const SETTINGS_CLASS: &str = "ActivityTrackerSettingsWindow";

// Constants not exposed by windows-sys 0.61 (stable Win32 values).
const STM_SETIMAGE: u32 = 0x0172;
const SS_ICON: u32 = 0x0003;
const SS_NOTIFY: u32 = 0x0100;
const SYS_TAB_CONTROL_CLASS: &str = "SysTabControl32";

// Control ids of the settings window.
const IDC_TAB: i32 = 3000;
const IDC_CHK_AUTOSTART: i32 = 3001;
const IDC_CHK_BACKGROUND: i32 = 3002;
const IDC_CHK_THERMAL_RED: i32 = 3003;
const IDC_EDIT_INTERVAL: i32 = 3004;
const IDC_BTN_APPLY: i32 = 3005;
const IDC_BTN_CLOSE: i32 = 3006;
const IDC_CHK_RAM_ENABLED: i32 = 3101;
const IDC_PREVIEW_RAM: i32 = 3102;
const IDC_SWATCH_RAM_BG: i32 = 3103;
const IDC_SWATCH_RAM_FG: i32 = 3104;
const IDC_CHK_CPU_ENABLED: i32 = 3105;
const IDC_PREVIEW_CPU: i32 = 3106;
const IDC_SWATCH_CPU_BG: i32 = 3107;
const IDC_SWATCH_CPU_FG: i32 = 3108;
const IDC_BTN_RESTORE: i32 = 3109;
const IDC_COMBO_FONT_SIZE: i32 = 3110;

/// Font size options for the tray icons' text (pixels); 0 = automatic.
const FONT_SIZE_OPTIONS: [u32; 9] = [0, 14, 16, 18, 20, 22, 24, 28, 32];

#[allow(clippy::upper_case_acronyms)]
type HICON = windows_sys::Win32::UI::WindowsAndMessaging::HICON;

/// Per-window state, stored in GWLP_USERDATA.
struct SettingsData {
    state: *mut AppState,
    /// Brushes for the four color swatches: ram_bg, ram_fg, cpu_bg, cpu_fg.
    swatch_brushes: [HBRUSH; 4],
    general_controls: Vec<HWND>,
    icons_controls: Vec<HWND>,
    preview_ram: HWND,
    preview_cpu: HWND,
    last_preview_ram: HICON,
    last_preview_cpu: HICON,
}

impl SettingsData {
    fn new(state: *mut AppState) -> Self {
        let settings = unsafe { &(*state).settings };
        Self {
            state,
            swatch_brushes: [
                make_brush(settings.tray_icon_ram_bg),
                make_brush(settings.tray_icon_ram_fg),
                make_brush(settings.tray_icon_cpu_bg),
                make_brush(settings.tray_icon_cpu_fg),
            ],
            general_controls: Vec::new(),
            icons_controls: Vec::new(),
            preview_ram: std::ptr::null_mut(),
            preview_cpu: std::ptr::null_mut(),
            last_preview_ram: std::ptr::null_mut(),
            last_preview_cpu: std::ptr::null_mut(),
        }
    }
}

fn make_brush(color: u32) -> HBRUSH {
    unsafe { CreateSolidBrush(color) }
}

pub fn register_settings_window_class(hinstance: HINSTANCE) -> bool {
    let class_name = wide(SETTINGS_CLASS);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { CreateSolidBrush(0x00F0F0F0) as HBRUSH },
        lpszMenuName: std::ptr::null_mut(),
        lpszClassName: class_name.as_ptr(),
    };

    unsafe { RegisterClassW(&wc) != 0 }
}

/// Shows the settings window (creates it on first use), owned by the main window.
pub(crate) fn open_settings(hwnd_main: HWND, state: &mut AppState) {
    if !state.settings_window.is_null() {
        unsafe {
            ShowWindow(state.settings_window, SW_RESTORE);
            ShowWindow(state.settings_window, SW_SHOW);
            SetForegroundWindow(state.settings_window);
        }
        return;
    }

    let class_name = wide(SETTINGS_CLASS);
    let title = wide("Settings");
    let data = Box::new(SettingsData::new(state as *mut AppState));
    let ptr = Box::into_raw(data);

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT | WS_EX_DLGMODALFRAME,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            500,
            448,
            hwnd_main,
            std::ptr::null_mut(),
            GetModuleHandleW(std::ptr::null()) as HINSTANCE,
            ptr as *const _,
        )
    };

    if hwnd.is_null() {
        unsafe {
            drop(Box::from_raw(ptr));
        }
    } else {
        state.settings_window = hwnd;
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            if cs.is_null() {
                return 0;
            }
            let data = (*cs).lpCreateParams as *mut SettingsData;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize);

            let data = &mut *data;
            create_controls(hwnd, data);
            sync_controls(hwnd, data);
            show_page(hwnd, data, 0);
            0
        }
        WM_COMMAND => {
            let Some(data) = get_data_mut(hwnd) else {
                return 0;
            };
            let id = (wparam & 0xFFFF) as i32;
            let notify = (wparam >> 16) as u32;

            match id {
                IDC_CHK_AUTOSTART | IDC_CHK_BACKGROUND | IDC_CHK_THERMAL_RED
                    if notify == BN_CLICKED =>
                {
                    on_checkbox(hwnd, data, id);
                }
                IDC_CHK_RAM_ENABLED | IDC_CHK_CPU_ENABLED if notify == BN_CLICKED => {
                    on_icon_enabled(hwnd, data, id);
                }
                IDC_SWATCH_RAM_BG | IDC_SWATCH_RAM_FG | IDC_SWATCH_CPU_BG | IDC_SWATCH_CPU_FG
                    if notify == STN_CLICKED =>
                {
                    on_color(hwnd, data, id);
                }
                IDC_BTN_APPLY if notify == BN_CLICKED => on_apply(hwnd, data),
                IDC_BTN_RESTORE if notify == BN_CLICKED => on_restore_defaults(hwnd, data),
                IDC_COMBO_FONT_SIZE if notify == CBN_SELCHANGE => on_font_size(hwnd, data),
                IDC_BTN_CLOSE if notify == BN_CLICKED => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_NOTIFY => {
            let Some(data) = get_data_mut(hwnd) else {
                return 0;
            };
            let nm = lparam as *const NMHDR;
            if !nm.is_null() && (*nm).idFrom == IDC_TAB as usize && (*nm).code == TCN_SELCHANGE {
                let tab = GetDlgItem(hwnd, IDC_TAB);
                let page = SendMessageW(tab, TCM_GETCURSEL, 0, 0) as usize;
                show_page(hwnd, data, page);
            }
            0
        }
        WM_CTLCOLORSTATIC => {
            let Some(data) = get_data_mut(hwnd) else {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            };
            let control = lparam as HWND;
            if control.is_null() {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let index = match GetDlgCtrlID(control) {
                IDC_SWATCH_RAM_BG => Some(0),
                IDC_SWATCH_RAM_FG => Some(1),
                IDC_SWATCH_CPU_BG => Some(2),
                IDC_SWATCH_CPU_FG => Some(3),
                _ => None,
            };
            if let Some(index) = index {
                // Opaque background painted with the swatch brush: the color
                // the user picked fills the whole control.
                SetBkMode(wparam as HDC, OPAQUE as i32);
                SetTextColor(wparam as HDC, 0);
                return data.swatch_brushes[index] as LRESULT;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            let data_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsData;
            if !data_ptr.is_null() {
                let data = &mut *data_ptr;
                // Unlink from the main app state before freeing ourselves.
                (*data.state).settings_window = std::ptr::null_mut();
                for brush in data.swatch_brushes.iter() {
                    if !brush.is_null() {
                        let _ = DeleteObject(*brush as _);
                    }
                }
                if !data.last_preview_ram.is_null() {
                    destroy_icon(data.last_preview_ram);
                }
                if !data.last_preview_cpu.is_null() {
                    destroy_icon(data.last_preview_cpu);
                }
                drop(Box::from_raw(data_ptr));
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn get_data_mut(hwnd: HWND) -> Option<&'static mut SettingsData> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut SettingsData;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

fn main_hwnd(hwnd: HWND) -> HWND {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_HWNDPARENT) as HWND }
}

fn create_controls(hwnd: HWND, data: &mut SettingsData) {
    let class_checkbox = wide("BUTTON");
    let class_label = wide("STATIC");
    let class_edit = wide("EDIT");

    let hinstance = unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null()) as HINSTANCE;

        let icc = INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);

        let tab = CreateWindowExW(
            0,
            wide(SYS_TAB_CONTROL_CLASS).as_ptr(),
            std::ptr::null_mut(),
            WS_CHILD | WS_VISIBLE,
            12,
            12,
            476,
            310,
            hwnd,
            IDC_TAB as HMENU,
            hinstance,
            std::ptr::null_mut(),
        );

        for index in 0..2 {
            let mut text = wide(if index == 0 {
                "General"
            } else {
                "Tray icons & colors"
            });
            let mut item = TCITEMW {
                mask: TCIF_TEXT,
                pszText: text.as_mut_ptr(),
                ..mem::zeroed()
            };
            let _ = SendMessageW(tab, TCM_INSERTITEMW, index, &mut item as *mut _ as isize);
        }

        hinstance
    };

    // --- General page ---
    data.general_controls = vec![
        create_checkbox(
            hwnd,
            hinstance,
            &class_checkbox,
            "Start automatically with Windows",
            32,
            44,
            300,
            IDC_CHK_AUTOSTART,
        ),
        create_checkbox(
            hwnd,
            hinstance,
            &class_checkbox,
            "Start in background (tray only)",
            32,
            74,
            300,
            IDC_CHK_BACKGROUND,
        ),
        create_checkbox(
            hwnd,
            hinstance,
            &class_checkbox,
            "Red tray icons while thermal throttling",
            32,
            104,
            320,
            IDC_CHK_THERMAL_RED,
        ),
        create_label(
            hwnd,
            hinstance,
            &class_label,
            "Refresh interval (s):",
            32,
            148,
            140,
        ),
        create_edit(
            hwnd,
            hinstance,
            &class_edit,
            IDC_EDIT_INTERVAL,
            180,
            145,
            60,
        ),
        create_button(
            hwnd,
            hinstance,
            &class_checkbox,
            "Apply",
            250,
            144,
            80,
            26,
            IDC_BTN_APPLY,
        ),
    ];

    // --- Tray icons page ---
    let preview_ram = create_icon_preview(hwnd, IDC_PREVIEW_RAM, 32, 76);
    let preview_cpu = create_icon_preview(hwnd, IDC_PREVIEW_CPU, 32, 148);
    data.preview_ram = preview_ram;
    data.preview_cpu = preview_cpu;

    data.icons_controls = vec![
        create_checkbox(
            hwnd,
            hinstance,
            &class_checkbox,
            "Show RAM icon in the tray",
            32,
            44,
            280,
            IDC_CHK_RAM_ENABLED,
        ),
        preview_ram,
        create_label(hwnd, hinstance, &class_label, "Background", 76, 80, 90),
        create_swatch(hwnd, IDC_SWATCH_RAM_BG, 172, 78, &class_label),
        create_label(hwnd, hinstance, &class_label, "Text", 228, 80, 40),
        create_swatch(hwnd, IDC_SWATCH_RAM_FG, 274, 78, &class_label),
        create_checkbox(
            hwnd,
            hinstance,
            &class_checkbox,
            "Show CPU icon in the tray",
            32,
            116,
            280,
            IDC_CHK_CPU_ENABLED,
        ),
        preview_cpu,
        create_label(hwnd, hinstance, &class_label, "Background", 76, 152, 90),
        create_swatch(hwnd, IDC_SWATCH_CPU_BG, 172, 150, &class_label),
        create_label(hwnd, hinstance, &class_label, "Text", 228, 152, 40),
        create_swatch(hwnd, IDC_SWATCH_CPU_FG, 274, 150, &class_label),
        create_label(
            hwnd,
            hinstance,
            &class_label,
            "Font size (both icons):",
            32,
            196,
            150,
        ),
        create_font_size_combo(hwnd, hinstance, 190, 194),
        create_button(
            hwnd,
            hinstance,
            &class_checkbox,
            "Restore defaults",
            32,
            230,
            130,
            26,
            IDC_BTN_RESTORE,
        ),
        create_label(
            hwnd,
            hinstance,
            &class_label,
            "Tip: click a colored square to change its color.",
            76,
            268,
            340,
        ),
    ];

    // Always visible.
    create_button(
        hwnd,
        hinstance,
        &class_checkbox,
        "Close",
        398,
        328,
        90,
        26,
        IDC_BTN_CLOSE,
    );
}
#[allow(clippy::too_many_arguments)]
fn create_checkbox(
    hwnd: HWND,
    hinstance: HINSTANCE,
    class: &[u16],
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    id: i32,
) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | BS_AUTOCHECKBOX as u32,
            x,
            y,
            w,
            22,
            hwnd,
            id as HMENU,
            hinstance,
            std::ptr::null_mut(),
        )
    }
}

fn create_label(
    hwnd: HWND,
    hinstance: HINSTANCE,
    class: &[u16],
    text: &str,
    x: i32,
    y: i32,
    w: i32,
) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            20,
            hwnd,
            0 as HMENU,
            hinstance,
            std::ptr::null_mut(),
        )
    }
}

fn create_edit(
    hwnd: HWND,
    hinstance: HINSTANCE,
    class: &[u16],
    id: i32,
    x: i32,
    y: i32,
    w: i32,
) -> HWND {
    unsafe {
        CreateWindowExW(
            WS_EX_CLIENTEDGE,
            class.as_ptr(),
            std::ptr::null_mut(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            x,
            y,
            w,
            24,
            hwnd,
            id as HMENU,
            hinstance,
            std::ptr::null_mut(),
        )
    }
}
#[allow(clippy::too_many_arguments)]
fn create_button(
    hwnd: HWND,
    hinstance: HINSTANCE,
    class: &[u16],
    text: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: i32,
) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            hwnd,
            id as HMENU,
            hinstance,
            std::ptr::null_mut(),
        )
    }
}

/// Clickable color swatch: a STATIC with SS_NOTIFY (sends STN_CLICKED).
fn create_swatch(hwnd: HWND, id: i32, x: i32, y: i32, class: &[u16]) -> HWND {
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null_mut(),
            WS_CHILD | WS_VISIBLE | SS_NOTIFY,
            x,
            y,
            40,
            22,
            hwnd,
            id as HMENU,
            GetModuleHandleW(std::ptr::null()) as HINSTANCE,
            std::ptr::null_mut(),
        )
    }
}

fn create_icon_preview(hwnd: HWND, id: i32, x: i32, y: i32) -> HWND {
    let class = wide("STATIC");
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null_mut(),
            WS_CHILD | WS_VISIBLE | SS_ICON,
            x,
            y,
            28,
            28,
            hwnd,
            id as HMENU,
            GetModuleHandleW(std::ptr::null()) as HINSTANCE,
            std::ptr::null_mut(),
        )
    }
}

/// Shows the controls of `page` (0 = General, 1 = Tray icons), hides the rest.
fn show_page(_hwnd: HWND, data: &SettingsData, page: usize) {
    for control in &data.general_controls {
        unsafe { ShowWindow(*control, if page == 0 { SW_SHOW } else { SW_HIDE }) };
    }
    for control in &data.icons_controls {
        unsafe { ShowWindow(*control, if page == 1 { SW_SHOW } else { SW_HIDE }) };
    }
}

/// Native dropdown with the tray icon font size options (Auto, 14..32 px).
fn create_font_size_combo(hwnd: HWND, hinstance: HINSTANCE, x: i32, y: i32) -> HWND {
    let combo = unsafe {
        CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("COMBOBOX").as_ptr(),
            std::ptr::null_mut(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST as u32,
            x,
            y,
            80,
            200,
            hwnd,
            IDC_COMBO_FONT_SIZE as HMENU,
            hinstance,
            std::ptr::null_mut(),
        )
    };
    for option in FONT_SIZE_OPTIONS.iter() {
        let text = if *option == 0 {
            "Auto".to_string()
        } else {
            option.to_string()
        };
        let label = wide(&text);
        unsafe {
            SendMessageW(combo, CB_ADDSTRING, 0, label.as_ptr() as isize);
        }
    }
    combo
}

/// Resynchronizes every control with the current settings.
fn sync_controls(hwnd: HWND, data: &mut SettingsData) {
    let state = unsafe { &mut *data.state };
    set_check(hwnd, IDC_CHK_AUTOSTART, state.settings.start_on_windows);
    set_check(hwnd, IDC_CHK_BACKGROUND, state.settings.start_in_background);
    set_check(
        hwnd,
        IDC_CHK_THERMAL_RED,
        state.settings.tray_icon_thermal_red,
    );
    set_check(
        hwnd,
        IDC_CHK_RAM_ENABLED,
        state.settings.tray_icon_ram_enabled,
    );
    set_check(
        hwnd,
        IDC_CHK_CPU_ENABLED,
        state.settings.tray_icon_cpu_enabled,
    );

    // Font size dropdown: select the stored value ("Auto" if unknown).
    let combo = unsafe { GetDlgItem(hwnd, IDC_COMBO_FONT_SIZE) };
    let index = FONT_SIZE_OPTIONS
        .iter()
        .position(|option| *option == state.settings.tray_icon_font_size)
        .unwrap_or(0);
    unsafe {
        SendMessageW(combo, CB_SETCURSEL, index, 0);
    }

    let interval = wide(&state.settings.refresh_interval_seconds.to_string());
    unsafe {
        SetWindowTextW(GetDlgItem(hwnd, IDC_EDIT_INTERVAL), interval.as_ptr());
    }

    refresh_brushes(data, &state.settings);
    update_preview_ram(data);
    update_preview_cpu(data);
}

fn set_check(hwnd: HWND, id: i32, checked: bool) {
    unsafe {
        SendMessageW(
            GetDlgItem(hwnd, id),
            BM_SETCHECK,
            if checked { 1 } else { 0 },
            0,
        );
    }
}

fn is_check(hwnd: HWND, id: i32) -> bool {
    unsafe { SendMessageW(GetDlgItem(hwnd, id), BM_GETCHECK, 0, 0) == 1 }
}

fn refresh_brushes(data: &mut SettingsData, settings: &AppSettings) {
    let colors = [
        settings.tray_icon_ram_bg,
        settings.tray_icon_ram_fg,
        settings.tray_icon_cpu_bg,
        settings.tray_icon_cpu_fg,
    ];
    for (index, color) in colors.iter().enumerate() {
        let old = data.swatch_brushes[index];
        let new = make_brush(*color);
        if !new.is_null() {
            data.swatch_brushes[index] = new;
        }
        if !old.is_null() {
            unsafe {
                let _ = DeleteObject(old as _);
            }
        }
    }
}

fn update_preview_ram(data: &mut SettingsData) {
    let state = unsafe { &*data.state };
    let icon = build_ram_icon(
        state.current_ram_percent,
        false,
        state.settings.tray_icon_ram_bg,
        state.settings.tray_icon_ram_fg,
        state.settings.tray_icon_font_size,
    );
    if !icon.is_null() {
        set_preview(&mut data.last_preview_ram, data.preview_ram, icon);
    }
}

fn update_preview_cpu(data: &mut SettingsData) {
    let state = unsafe { &*data.state };
    let temp = state
        .cpu_monitor
        .avg_temperature_celsius()
        .map(|t| t as u32);
    let icon = build_cpu_icon(
        temp,
        false,
        state.settings.tray_icon_cpu_bg,
        state.settings.tray_icon_cpu_fg,
        state.settings.tray_icon_font_size,
    );
    if !icon.is_null() {
        set_preview(&mut data.last_preview_cpu, data.preview_cpu, icon);
    }
}

/// Swaps the icon shown by an SS_ICON static (forces a repaint before the
/// previous icon is destroyed, so the static never draws a dead handle).
fn set_preview(slot: &mut HICON, static_hwnd: HWND, icon: HICON) {
    unsafe {
        SendMessageW(static_hwnd, STM_SETIMAGE, 0, icon as isize);
        UpdateWindow(static_hwnd);
    }
    if !slot.is_null() {
        destroy_icon(*slot);
    }
    *slot = icon;
}

/// Opens the native Windows color dialog; returns the chosen COLORREF.
fn choose_color(hwnd: HWND, initial: COLORREF) -> Option<COLORREF> {
    // comdlg32 dereferences lpCustColors unconditionally (CC_FULLOPEN shows
    // the custom palette): leaving it NULL is an access violation.
    let mut custom_colors = [0 as COLORREF; 16];
    let mut cc = CHOOSECOLORW {
        lStructSize: mem::size_of::<CHOOSECOLORW>() as u32,
        hwndOwner: hwnd,
        Flags: CC_FULLOPEN | CC_RGBINIT,
        rgbResult: initial,
        lpCustColors: custom_colors.as_mut_ptr(),
        ..Default::default()
    };

    unsafe { ChooseColorW(&mut cc) != 0 }.then_some(cc.rgbResult)
}

fn on_checkbox(hwnd: HWND, data: &mut SettingsData, id: i32) {
    let state = unsafe { &mut *data.state };
    let checked = is_check(hwnd, id);
    match id {
        IDC_CHK_AUTOSTART => state.settings.start_on_windows = checked,
        IDC_CHK_BACKGROUND => state.settings.start_in_background = checked,
        _ => state.settings.tray_icon_thermal_red = checked,
    }

    if let Err(err) = save_settings(&state.settings) {
        show_error(hwnd, &format!("Could not save settings: {err}"));
        return;
    }

    if id == IDC_CHK_AUTOSTART || id == IDC_CHK_BACKGROUND {
        apply_autostart(hwnd, state);
    } else {
        refresh_tray_icons(main_hwnd(hwnd), state);
    }
}

fn on_icon_enabled(hwnd: HWND, data: &mut SettingsData, id: i32) {
    let state = unsafe { &mut *data.state };
    if id == IDC_CHK_RAM_ENABLED {
        state.settings.tray_icon_ram_enabled = is_check(hwnd, IDC_CHK_RAM_ENABLED);
        update_preview_ram(data);
    } else {
        state.settings.tray_icon_cpu_enabled = is_check(hwnd, IDC_CHK_CPU_ENABLED);
        update_preview_cpu(data);
    }

    if let Err(err) = save_settings(&state.settings) {
        show_error(hwnd, &format!("Could not save settings: {err}"));
        return;
    }

    refresh_tray_icons(main_hwnd(hwnd), state);
}

fn on_color(hwnd: HWND, data: &mut SettingsData, id: i32) {
    let state = unsafe { &mut *data.state };
    let current = match id {
        IDC_SWATCH_RAM_BG => state.settings.tray_icon_ram_bg,
        IDC_SWATCH_RAM_FG => state.settings.tray_icon_ram_fg,
        IDC_SWATCH_CPU_BG => state.settings.tray_icon_cpu_bg,
        _ => state.settings.tray_icon_cpu_fg,
    };

    let Some(color) = choose_color(hwnd, current) else {
        return; // dialog cancelled
    };

    match id {
        IDC_SWATCH_RAM_BG => state.settings.tray_icon_ram_bg = color,
        IDC_SWATCH_RAM_FG => state.settings.tray_icon_ram_fg = color,
        IDC_SWATCH_CPU_BG => state.settings.tray_icon_cpu_bg = color,
        _ => state.settings.tray_icon_cpu_fg = color,
    }

    if let Err(err) = save_settings(&state.settings) {
        show_error(hwnd, &format!("Could not save settings: {err}"));
        return;
    }

    refresh_brushes(data, &state.settings);
    if id == IDC_SWATCH_RAM_BG || id == IDC_SWATCH_RAM_FG {
        update_preview_ram(data);
    } else {
        update_preview_cpu(data);
    }
    refresh_tray_icons(main_hwnd(hwnd), state);
}

fn on_font_size(hwnd: HWND, data: &mut SettingsData) {
    let state = unsafe { &mut *data.state };
    let combo = unsafe { GetDlgItem(hwnd, IDC_COMBO_FONT_SIZE) };
    let index = unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) } as usize;
    state.settings.tray_icon_font_size = *FONT_SIZE_OPTIONS.get(index).unwrap_or(&0);

    if let Err(err) = save_settings(&state.settings) {
        show_error(hwnd, &format!("Could not save settings: {err}"));
        return;
    }

    update_preview_ram(data);
    update_preview_cpu(data);
    refresh_tray_icons(main_hwnd(hwnd), state);
}

fn on_apply(hwnd: HWND, data: &mut SettingsData) {
    let state = unsafe { &mut *data.state };
    let text = get_control_text(hwnd, IDC_EDIT_INTERVAL);
    state.settings.refresh_interval_seconds =
        parse_refresh_interval_seconds(&text, state.settings.refresh_interval_seconds);

    if let Err(err) = save_settings(&state.settings) {
        show_error(hwnd, &format!("Could not save settings: {err}"));
        return;
    }

    restart_refresh_timer(main_hwnd(hwnd), state.settings.refresh_interval_seconds);
}

fn on_restore_defaults(hwnd: HWND, data: &mut SettingsData) {
    let state = unsafe { &mut *data.state };
    let s = &mut state.settings;
    s.tray_icon_ram_enabled = true;
    s.tray_icon_cpu_enabled = true;
    s.tray_icon_ram_bg = DEFAULT_TRAY_RAM_BG;
    s.tray_icon_ram_fg = DEFAULT_TRAY_RAM_FG;
    s.tray_icon_cpu_bg = DEFAULT_TRAY_CPU_BG;
    s.tray_icon_cpu_fg = DEFAULT_TRAY_CPU_FG;
    s.tray_icon_font_size = DEFAULT_TRAY_ICON_FONT_SIZE;
    s.tray_icon_thermal_red = false;

    if let Err(err) = save_settings(s) {
        show_error(hwnd, &format!("Could not save settings: {err}"));
        return;
    }

    sync_controls(hwnd, data);
    let tab = unsafe { GetDlgItem(hwnd, IDC_TAB) };
    let page = unsafe { SendMessageW(tab, TCM_GETCURSEL, 0, 0) } as usize;
    show_page(hwnd, data, page);

    refresh_tray_icons(main_hwnd(hwnd), state);
}

fn apply_autostart(hwnd: HWND, state: &AppState) {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(err) = set_autostart(
                state.settings.start_on_windows,
                state.settings.start_in_background,
                &exe,
            ) {
                show_error(hwnd, &format!("Autostart update error: {err}"));
            }
        }
        Err(err) => show_error(hwnd, &format!("Could not read executable path: {err}")),
    }
}
