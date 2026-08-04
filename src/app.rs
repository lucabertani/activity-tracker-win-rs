#![allow(unsafe_op_in_unsafe_fn)]

use std::mem;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, SetBkMode, SetTextColor, TRANSPARENT, HBRUSH, HDC,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{LVIF_TEXT, LVN_GETDISPINFOW, NMLVDISPINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CREATESTRUCTW, DefWindowProcW, DestroyWindow, DispatchMessageW, EN_CHANGE,
    GWLP_USERDATA, GetCursorPos, GetDlgCtrlID, GetMessageW, KillTimer, MB_ICONERROR, MB_OK,
    MSG, MessageBoxW, PostQuitMessage, SetTimer, SetWindowLongPtrW, TranslateMessage, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_CTLCOLORSTATIC, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NOTIFY,
    WM_RBUTTONUP, WM_SIZE, WM_TIMER,
};

use crate::error::{AppError, AppResult};
use crate::model::{parse_refresh_interval_seconds, AppSettings, ProcessInfo, SortColumn, SortState};
use crate::monitor::cpu::CpuThrottleMonitor;
use crate::monitor::memory::total_memory_status;
use crate::monitor::processes::{apply_filter_and_sort, snapshot_processes, terminate_process};
use crate::settings::autostart::set_autostart;
use crate::settings::store::{load_settings, save_settings};
use crate::ui::menu::{CMD_EXIT_APP, CMD_OPEN_WINDOW, show_tray_context_menu};
use crate::ui::tray::{
    WM_TRAYICON, add_tray_icon, build_percentage_icon, destroy_icon, remove_tray_icon,
    update_tray_icon, update_tray_tooltip,
};
use crate::ui::window::{
    IDC_APPLY_SETTINGS, IDC_HEADER_CPU_THROTTLE, IDC_HEADER_RAM, IDC_INTERVAL_EDIT, IDC_SEARCH,
    IDC_SORT_MEMORY, IDC_PROCESS_LIST, IDC_SORT_NAME, IDC_START_IN_BACKGROUND,
    IDC_START_ON_WINDOWS, IDC_TERMINATE_PROCESS, IDC_TRAY_THERMAL_RED,
    create_controls,
    create_main_window, get_control_text, hide_main_window, is_checkbox_checked,
    register_main_window_class, selected_process_row, set_checkbox, set_label_text,
    set_virtual_item_count, show_main_window,
    wide,
};

const TIMER_REFRESH_ID: usize = 1;

struct AppState {
    settings: AppSettings,
    sort: SortState,
    search_query: String,
    all_processes: Vec<ProcessInfo>,
    visible_processes: Vec<ProcessInfo>,
    current_ram_percent: u32,
    cpu_monitor: CpuThrottleMonitor,
    window_bg_brush: HBRUSH,
    tray_icon: windows_sys::Win32::UI::WindowsAndMessaging::HICON,
    notify_text_buf: Vec<u16>,
}

impl AppState {
    fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            sort: SortState::default(),
            search_query: String::new(),
            all_processes: Vec::new(),
            visible_processes: Vec::new(),
            current_ram_percent: 0,
            cpu_monitor: CpuThrottleMonitor::new(),
            // Same color as the window background: used as the background
            // brush when painting the red throttling label.
            window_bg_brush: unsafe { CreateSolidBrush(crate::ui::window::APP_WINDOW_BG_COLOR) },
            tray_icon: std::ptr::null_mut(),
            notify_text_buf: Vec::new(),
        }
    }
}

pub fn run() -> AppResult<()> {
    let mut settings = load_settings().unwrap_or_else(|_| AppSettings::default());
    settings = settings.normalized();

    let start_in_background_from_cli = std::env::args().any(|a| a.eq_ignore_ascii_case("--background"));

    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) } as HINSTANCE;
    if hinstance.is_null() {
        return Err(AppError::WinApi("GetModuleHandleW failed"));
    }

    if !register_main_window_class(hinstance, wnd_proc) {
        return Err(AppError::WinApi("RegisterClassW failed"));
    }

    let boxed_state = Box::new(AppState::new(settings));
    let state_ptr = Box::into_raw(boxed_state);

    let hwnd = create_main_window(hinstance, state_ptr as isize);
    if hwnd.is_null() {
        unsafe {
            let _ = Box::from_raw(state_ptr);
        }
        return Err(AppError::WinApi("CreateWindowExW failed"));
    }

    // Hide the window only when explicitly requested: started by Windows
    // autostart (--background) or the "window_start_hidden" setting.
    // A manual double-click always shows the window, even if
    // "Avvio in background" is enabled.
    let start_hidden = unsafe {
        let state = &*state_ptr;
        state.settings.window_start_hidden || start_in_background_from_cli
    };

    if start_hidden {
        hide_main_window(hwnd);
    }

    let mut msg: MSG = unsafe { mem::zeroed() };
    while unsafe { GetMessageW(&mut msg, 0 as HWND, 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            if cs.is_null() {
                return 0;
            }

            let state_ptr = (*cs).lpCreateParams as *mut AppState;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            let hinstance = (*cs).hInstance as HINSTANCE;
            create_controls(hwnd, hinstance);

            if let Some(state) = get_state_mut(hwnd) {
                set_checkbox(hwnd, IDC_START_ON_WINDOWS, state.settings.start_on_windows);
                set_checkbox(hwnd, IDC_START_IN_BACKGROUND, state.settings.start_in_background);
                set_checkbox(hwnd, IDC_TRAY_THERMAL_RED, state.settings.tray_icon_thermal_red);
                set_label_text(
                    hwnd,
                    IDC_INTERVAL_EDIT,
                    &state.settings.refresh_interval_seconds.to_string(),
                );

                let initial_tip = format!("RAM {}", state.current_ram_percent);
                state.tray_icon = build_percentage_icon(state.current_ram_percent, false);
                let _ = add_tray_icon(hwnd, &initial_tip, state.tray_icon);

                let _ = SetTimer(
                    hwnd,
                    TIMER_REFRESH_ID,
                    (state.settings.refresh_interval_seconds * 1000) as u32,
                    None,
                );

                refresh_ui_data(hwnd, state);
            }

            0
        }
        WM_NOTIFY => {
            let nmhdr = lparam as *const windows_sys::Win32::UI::Controls::NMHDR;
            if !nmhdr.is_null() {
                if (*nmhdr).idFrom as i32 == IDC_PROCESS_LIST && (*nmhdr).code == LVN_GETDISPINFOW {
                    if let Some(state) = get_state_mut(hwnd) {
                        let info = lparam as *mut NMLVDISPINFOW;
                        if !info.is_null() {
                            let item = &mut (*info).item;
                            if (item.mask & LVIF_TEXT as u32) != 0 {
                                let text = process_cell_text(state, item.iItem, item.iSubItem);
                                state.notify_text_buf = wide(&text);
                                item.pszText = state.notify_text_buf.as_mut_ptr();
                            }
                        }
                    }
                }
            }
            0
        }
        WM_COMMAND => {
            let id = loword(wparam as u32) as i32;
            let notify = hiword(wparam as u32) as u16;

            match id {
                IDC_SEARCH if notify == EN_CHANGE as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        state.search_query = get_control_text(hwnd, IDC_SEARCH);
                        rebuild_visible_and_render(hwnd, state);
                    }
                }
                IDC_SORT_NAME if notify == BN_CLICKED as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        state.sort.toggle_for(SortColumn::Name);
                        rebuild_visible_and_render(hwnd, state);
                    }
                }
                IDC_SORT_MEMORY if notify == BN_CLICKED as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        state.sort.toggle_for(SortColumn::Memory);
                        rebuild_visible_and_render(hwnd, state);
                    }
                }
                IDC_APPLY_SETTINGS if notify == BN_CLICKED as u16 => {
                    apply_settings_from_ui(hwnd);
                }
                // Settings checkboxes save immediately on click (no need to
                // press "Applica"): the user expects the flag to persist
                // as soon as it is toggled.
                (IDC_START_ON_WINDOWS | IDC_START_IN_BACKGROUND | IDC_TRAY_THERMAL_RED)
                    if notify == BN_CLICKED as u16 =>
                {
                    apply_settings_from_ui(hwnd);
                }
                IDC_TERMINATE_PROCESS if notify == BN_CLICKED as u16 => {
                    terminate_selected_process(hwnd);
                }
                _ => {}
            }

            0
        }
        WM_TIMER => {
            if wparam == TIMER_REFRESH_ID {
                if let Some(state) = get_state_mut(hwnd) {
                    refresh_ui_data(hwnd, state);
                }
            }
            0
        }
        WM_SIZE => {
            // Hide-to-tray on minimize.
            if wparam as u32 == windows_sys::Win32::UI::WindowsAndMessaging::SIZE_MINIMIZED {
                hide_main_window(hwnd);
            }
            0
        }
        WM_CLOSE => {
            // Keep process alive and only hide window.
            hide_main_window(hwnd);
            0
        }
        WM_TRAYICON => {
            let tray_event = lparam as u32;
            match tray_event {
                WM_LBUTTONDBLCLK => show_main_window(hwnd),
                WM_RBUTTONUP => {
                    let mut pt = POINT { x: 0, y: 0 };
                    let _ = GetCursorPos(&mut pt);
                    let cmd = show_tray_context_menu(hwnd, pt.x, pt.y);
                    if cmd == CMD_OPEN_WINDOW {
                        show_main_window(hwnd);
                    } else if cmd == CMD_EXIT_APP {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                _ => {}
            }
            0
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, TIMER_REFRESH_ID);
            remove_tray_icon(hwnd);

            let state_ptr = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState;
            if !state_ptr.is_null() {
                if !(*state_ptr).tray_icon.is_null() {
                    destroy_icon((*state_ptr).tray_icon);
                    (*state_ptr).tray_icon = std::ptr::null_mut();
                }
                if !(*state_ptr).window_bg_brush.is_null() {
                    let _ = DeleteObject((*state_ptr).window_bg_brush);
                    (*state_ptr).window_bg_brush = std::ptr::null_mut();
                }
                let _ = Box::from_raw(state_ptr);
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }

            PostQuitMessage(0);
            0
        }
        WM_CTLCOLORSTATIC => {
            // Color the throttling warning label red; keep everything else default.
            let control = lparam as HWND;
            if !control.is_null()
                && unsafe { GetDlgCtrlID(control) } == IDC_HEADER_CPU_THROTTLE
            {
                if let Some(state) = get_state_mut(hwnd) {
                    unsafe {
                        // COLORREF format is 0x00BBGGRR: red.
                        SetTextColor(wparam as HDC, 0x0000FF);
                        SetBkMode(wparam as HDC, TRANSPARENT as i32);
                    }
                    return state.window_bg_brush as LRESULT;
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn loword(value: u32) -> u16 {
    (value & 0xFFFF) as u16
}

fn hiword(value: u32) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

fn get_state_mut(hwnd: HWND) -> Option<&'static mut AppState> {
    let ptr = unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
            as *mut AppState
    };
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

fn refresh_ui_data(hwnd: HWND, state: &mut AppState) {
    // Thermal throttling check: same refresh interval as the RAM percentage.
    // A single throttled core is enough to light up the warning.
    let cpu_throttling = state.cpu_monitor.refresh();
    set_label_text(
        hwnd,
        IDC_HEADER_CPU_THROTTLE,
        if cpu_throttling { "CPU thermal throttling" } else { "" },
    );

    // Tray icon turns red while throttling when the option is enabled.
    let tray_red = state.settings.tray_icon_thermal_red && cpu_throttling;

    if let Ok((ram_percent, total_ram_bytes)) = total_memory_status() {
        state.current_ram_percent = ram_percent;

        set_label_text(
            hwnd,
            IDC_HEADER_RAM,
            &format!("RAM totale usata: {}", state.current_ram_percent),
        );
        update_tray_tooltip(hwnd, &format!("RAM {}", state.current_ram_percent));

        let new_icon = build_percentage_icon(state.current_ram_percent, tray_red);
        if !new_icon.is_null() {
            update_tray_icon(hwnd, new_icon);
            if !state.tray_icon.is_null() {
                destroy_icon(state.tray_icon);
            }
            state.tray_icon = new_icon;
        }

        state.all_processes = snapshot_processes(total_ram_bytes);
        rebuild_visible_and_render(hwnd, state);
    }
}

fn rebuild_visible_and_render(hwnd: HWND, state: &mut AppState) {
    state.visible_processes = apply_filter_and_sort(&state.all_processes, &state.search_query, state.sort);
    set_virtual_item_count(hwnd, state.visible_processes.len());
}

fn apply_settings_from_ui(hwnd: HWND) {
    let Some(state) = get_state_mut(hwnd) else {
        return;
    };

    let mut new_settings = state.settings.clone();
    new_settings.start_on_windows = is_checkbox_checked(hwnd, IDC_START_ON_WINDOWS);
    new_settings.start_in_background = is_checkbox_checked(hwnd, IDC_START_IN_BACKGROUND);
    new_settings.tray_icon_thermal_red = is_checkbox_checked(hwnd, IDC_TRAY_THERMAL_RED);

    let interval_text = get_control_text(hwnd, IDC_INTERVAL_EDIT);
    new_settings.refresh_interval_seconds =
        parse_refresh_interval_seconds(&interval_text, state.settings.refresh_interval_seconds);

    if new_settings.start_on_windows != state.settings.start_on_windows
        || new_settings.start_in_background != state.settings.start_in_background
    {
        match std::env::current_exe() {
            Ok(exe) => {
                if let Err(err) = set_autostart(
                    new_settings.start_on_windows,
                    new_settings.start_in_background,
                    &exe,
                ) {
                    show_error(hwnd, &format!("Errore aggiornamento autostart: {err}"));
                }
            }
            Err(err) => show_error(hwnd, &format!("Impossibile leggere path eseguibile: {err}")),
        }
    }

    if let Err(err) = save_settings(&new_settings) {
        show_error(hwnd, &format!("Errore salvataggio impostazioni: {err}"));
        return;
    }

    state.settings = new_settings.normalized();

    unsafe {
        let _ = KillTimer(hwnd, TIMER_REFRESH_ID);
        let _ = SetTimer(
            hwnd,
            TIMER_REFRESH_ID,
            (state.settings.refresh_interval_seconds * 1000) as u32,
            None,
        );
    }

    set_label_text(
        hwnd,
        IDC_INTERVAL_EDIT,
        &state.settings.refresh_interval_seconds.to_string(),
    );
}

fn show_error(hwnd: HWND, text: &str) {
    let title = wide("Activity Tracker");
    let message = wide(text);

    unsafe {
        let _ = MessageBoxW(hwnd, message.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

fn process_cell_text(state: &AppState, row: i32, column: i32) -> String {
    if row < 0 {
        return String::new();
    }

    let Some(process) = state.visible_processes.get(row as usize) else {
        return String::new();
    };

    match column {
        0 => process.name.clone(),
        1 => process.pid.to_string(),
        2 => format!("{:.2}", process.memory_mib),
        3 => format!("{:.2}", process.memory_percent),
        _ => String::new(),
    }
}

fn terminate_selected_process(hwnd: HWND) {
    let Some(state) = get_state_mut(hwnd) else {
        return;
    };

    let Some(row) = selected_process_row(hwnd) else {
        show_error(hwnd, "Seleziona un processo dalla lista prima di terminarlo.");
        return;
    };

    let Some(proc_info) = state.visible_processes.get(row as usize).cloned() else {
        show_error(hwnd, "Processo selezionato non valido.");
        return;
    };

    if proc_info.pid == std::process::id() {
        show_error(hwnd, "Non puoi terminare l'applicazione corrente da questa azione.");
        return;
    }

    if let Err(err) = terminate_process(proc_info.pid) {
        show_error(
            hwnd,
            &format!(
                "Impossibile terminare il processo {} (PID {}): {}",
                proc_info.name, proc_info.pid, err
            ),
        );
        return;
    }

    refresh_ui_data(hwnd, state);
}
