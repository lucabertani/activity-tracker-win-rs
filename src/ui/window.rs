use std::mem;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows_sys::Win32::UI::Controls::{
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_FMT, LVCF_TEXT,
    LVCF_WIDTH, LVCFMT_LEFT, LVCOLUMNW, LVS_EX_FULLROWSELECT, LVS_EX_GRIDLINES,
    LVS_OWNERDATA, LVS_REPORT, LVS_SHOWSELALWAYS, WC_LISTVIEWW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_PUSHBUTTON, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, CreateWindowExW, ES_AUTOHSCROLL, GetWindowTextLengthW, GetWindowTextW, HMENU,
    HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, LoadCursorW, RegisterClassW, SW_HIDE, SW_RESTORE,
    SW_SHOW, SWP_NOMOVE, SWP_NOSIZE, SendMessageW, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, WNDCLASSW, WS_BORDER, WS_CHILD, WS_CLIPSIBLINGS,
    WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
};

pub const IDC_HEADER_RAM: i32 = 2001;
pub const IDC_HEADER_CPU_THROTTLE: i32 = 2011;
pub const IDC_SEARCH: i32 = 2002;
pub const IDC_SORT_NAME: i32 = 2003;
pub const IDC_SORT_MEMORY: i32 = 2004;
pub const IDC_PROCESS_LIST: i32 = 2005;
pub const IDC_START_ON_WINDOWS: i32 = 2006;
pub const IDC_START_IN_BACKGROUND: i32 = 2007;
pub const IDC_INTERVAL_EDIT: i32 = 2008;
pub const IDC_APPLY_SETTINGS: i32 = 2009;
pub const IDC_TERMINATE_PROCESS: i32 = 2010;
// COLORREF format is 0x00BBGGRR.
pub const APP_WINDOW_BG_COLOR: u32 = 0x00F0F0F0; // #f0f0f0

pub const MAIN_CLASS: &str = "ActivityTrackerMainWindow";

pub fn register_main_window_class(hinstance: HINSTANCE, wndproc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) -> bool {
    let class_name = wide(MAIN_CLASS);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { CreateSolidBrush(APP_WINDOW_BG_COLOR) as HBRUSH },
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
    };

    unsafe { RegisterClassW(&wc) != 0 }
}

pub fn create_main_window(hinstance: HINSTANCE, lp_param: isize) -> HWND {
    let class_name = wide(MAIN_CLASS);
    let title = wide("Activity Tracker (Win32 + Rust)");

    unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            980,
            700,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            lp_param as _,
        )
    }
}

pub fn create_controls(hwnd: HWND, hinstance: HINSTANCE) {
    unsafe {
        let mut icc = INITCOMMONCONTROLSEX {
            dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        let _ = InitCommonControlsEx(&mut icc);

        create_label(hwnd, hinstance, "RAM totale usata: 0", 20, 20, 350, 20, IDC_HEADER_RAM);
        // Shown (in red) only while the CPU is thermal-throttling.
        create_label(hwnd, hinstance, "", 20, 40, 360, 16, IDC_HEADER_CPU_THROTTLE);
        create_label(hwnd, hinstance, "Ricerca processi:", 20, 60, 140, 22, 0);

        let _ = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | (ES_AUTOHSCROLL as u32),
            160,
            58,
            260,
            24,
            hwnd,
            menu_id(IDC_SEARCH),
            hinstance,
            std::ptr::null(),
        );

        create_button(hwnd, hinstance, "Sort Nome", 440, 58, 100, 24, IDC_SORT_NAME);
        create_button(hwnd, hinstance, "Sort RAM", 550, 58, 100, 24, IDC_SORT_MEMORY);
        create_button(
            hwnd,
            hinstance,
            "Termina processo",
            665,
            58,
            140,
            24,
            IDC_TERMINATE_PROCESS,
        );

        let list = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            WC_LISTVIEWW,
            std::ptr::null(),
            WS_CHILD
                | WS_VISIBLE
                | WS_BORDER
                | WS_CLIPSIBLINGS
                | (LVS_REPORT as u32)
                | (LVS_SHOWSELALWAYS as u32)
                | (LVS_OWNERDATA as u32),
            20,
            95,
            940,
            420,
            hwnd,
            menu_id(IDC_PROCESS_LIST),
            hinstance,
            std::ptr::null(),
        );

        let _ = SendMessageW(
            list,
            windows_sys::Win32::UI::Controls::LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_GRIDLINES) as isize,
        );

        add_listview_columns(list);

        create_label(hwnd, hinstance, "Impostazioni", 20, 535, 130, 24, 0);

        let _ = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("Avvio automatico con Windows").as_ptr(),
            WS_CHILD | WS_VISIBLE | (BS_AUTOCHECKBOX as u32),
            20,
            565,
            260,
            24,
            hwnd,
            menu_id(IDC_START_ON_WINDOWS),
            hinstance,
            std::ptr::null(),
        );

        let _ = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide("Avvio in background").as_ptr(),
            WS_CHILD | WS_VISIBLE | (BS_AUTOCHECKBOX as u32),
            290,
            565,
            180,
            24,
            hwnd,
            menu_id(IDC_START_IN_BACKGROUND),
            hinstance,
            std::ptr::null(),
        );

        create_label(hwnd, hinstance, "Refresh RAM (s):", 490, 567, 110, 20, 0);

        let _ = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            wide("EDIT").as_ptr(),
            wide("3").as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | (ES_AUTOHSCROLL as u32),
            605,
            564,
            80,
            24,
            hwnd,
            menu_id(IDC_INTERVAL_EDIT),
            hinstance,
            std::ptr::null(),
        );

        create_button(hwnd, hinstance, "Applica", 700, 563, 100, 26, IDC_APPLY_SETTINGS);
    }
}

fn create_label(hwnd: HWND, hinstance: HINSTANCE, text: &str, x: i32, y: i32, w: i32, h: i32, id: i32) {
    unsafe {
        let _ = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            hwnd,
            menu_id(id),
            hinstance,
            std::ptr::null(),
        );
    }
}

fn create_button(hwnd: HWND, hinstance: HINSTANCE, text: &str, x: i32, y: i32, w: i32, h: i32, id: i32) {
    unsafe {
        let _ = CreateWindowExW(
            0,
            wide("BUTTON").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | (BS_PUSHBUTTON as u32),
            x,
            y,
            w,
            h,
            hwnd,
            menu_id(id),
            hinstance,
            std::ptr::null(),
        );
    }
}

fn add_listview_columns(list: HWND) {
    let columns = [
        ("Processo", 260),
        ("PID", 100),
        ("RAM MiB", 140),
        ("RAM %", 140),
    ];

    for (index, (name, width)) in columns.iter().enumerate() {
        let mut text = wide(name);
        let mut col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
            fmt: LVCFMT_LEFT,
            cx: *width,
            pszText: text.as_mut_ptr(),
            ..unsafe { mem::zeroed() }
        };

        unsafe {
            let _ = SendMessageW(
                list,
                windows_sys::Win32::UI::Controls::LVM_INSERTCOLUMNW,
                index,
                &mut col as *mut _ as isize,
            );
        }
    }
}

pub fn set_label_text(hwnd: HWND, id: i32, text: &str) {
    unsafe {
        let control = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, id);
        if !control.is_null() {
            let _ = SetWindowTextW(control, wide(text).as_ptr());
        }
    }
}

pub fn get_control_text(hwnd: HWND, id: i32) -> String {
    unsafe {
        let control = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, id);
        if control.is_null() {
            return String::new();
        }

        let len = GetWindowTextLengthW(control);
        if len <= 0 {
            return String::new();
        }

        let mut buf = vec![0u16; (len as usize) + 1];
        let copied = GetWindowTextW(control, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..(copied as usize)])
    }
}

pub fn set_checkbox(hwnd: HWND, id: i32, checked: bool) {
    unsafe {
        let control = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, id);
        if !control.is_null() {
            let _ = SendMessageW(
                control,
                BM_SETCHECK,
                if checked { 1 } else { 0 },
                0,
            );
        }
    }
}

pub fn is_checkbox_checked(hwnd: HWND, id: i32) -> bool {
    unsafe {
        let control = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, id);
        if control.is_null() {
            return false;
        }

        SendMessageW(control, BM_GETCHECK, 0, 0) == 1
    }
}

pub fn update_process_list(hwnd: HWND, rows: &[(String, String, String, String)]) {
    unsafe {
        let list = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, IDC_PROCESS_LIST);
        if list.is_null() {
            return;
        }

        let _ = SendMessageW(
            list,
            windows_sys::Win32::UI::Controls::LVM_SETITEMCOUNT,
            rows.len(),
            0,
        );
    }
}

pub fn set_virtual_item_count(hwnd: HWND, count: usize) {
    unsafe {
        let list = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, IDC_PROCESS_LIST);
        if list.is_null() {
            return;
        }

        let _ = SendMessageW(
            list,
            windows_sys::Win32::UI::Controls::LVM_SETITEMCOUNT,
            count,
            0,
        );
    }
}

pub fn selected_process_row(hwnd: HWND) -> Option<i32> {
    unsafe {
        let list = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(hwnd, IDC_PROCESS_LIST);
        if list.is_null() {
            return None;
        }

        let idx = SendMessageW(
            list,
            windows_sys::Win32::UI::Controls::LVM_GETNEXTITEM,
            usize::MAX,
            windows_sys::Win32::UI::Controls::LVNI_SELECTED as isize,
        ) as i32;

        if idx < 0 {
            None
        } else {
            Some(idx)
        }
    }
}

pub fn show_main_window(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        ShowWindow(hwnd, SW_SHOW);
        // Topmost dance helps force Z-order foreground when restoring from tray.
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let _ = SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn hide_main_window(hwnd: HWND) {
    unsafe {
        ShowWindow(hwnd, SW_HIDE);
    }
}

pub fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn menu_id(id: i32) -> HMENU {
    id as usize as HMENU
}
