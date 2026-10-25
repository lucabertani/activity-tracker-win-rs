#![allow(unsafe_op_in_unsafe_fn)]

use std::mem;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, HBRUSH, HDC, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    LVIF_TEXT, LVN_GETDISPINFOW, LVN_ITEMACTIVATE, NMLISTVIEW, NMLVDISPINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CREATESTRUCTW, DefWindowProcW, DestroyWindow, DispatchMessageW, EN_CHANGE,
    GWLP_USERDATA, GetCursorPos, GetDlgCtrlID, GetMessageW, KillTimer, MB_ICONERROR, MB_OK, MSG,
    MessageBoxW, PostQuitMessage, SetTimer, SetWindowLongPtrW, TranslateMessage, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_CTLCOLORSTATIC, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NOTIFY,
    WM_RBUTTONUP, WM_SIZE, WM_TIMER,
};

use crate::settings_window::{open_settings, register_settings_window_class};
use crate::window::{
    IDC_HEADER_CPU_TEMP, IDC_HEADER_CPU_THROTTLE, IDC_HEADER_RAM, IDC_PROCESS_LIST, IDC_SEARCH,
    IDC_SETTINGS_BUTTON, IDC_SORT_MEMORY, IDC_SORT_NAME, IDC_TERMINATE_PROCESS, create_controls,
    create_main_window, get_control_text, hide_main_window, register_main_window_class,
    selected_process_row, set_label_text, set_virtual_item_count, show_main_window, wide,
};
use tracker_core::error::{AppError, AppResult};
use tracker_core::model::{AppSettings, DisplayRow, SortColumn};
use tracker_core::process_list::{ProcessList, mib};
use tracker_platform::monitor::cpu::CpuThrottleMonitor;
use tracker_platform::monitor::memory::total_memory_status;
use tracker_platform::monitor::processes::{snapshot_processes, terminate_process};
use tracker_platform::startup::Startup;
use tracker_platform::tray::{TrayIcons, WM_TRAYICON};
use tracker_platform::tray_menu::{CMD_EXIT_APP, CMD_OPEN_WINDOW, show_tray_context_menu};

const TIMER_REFRESH_ID: usize = 1;

pub(crate) struct AppState {
    pub(crate) settings: AppSettings,
    processes: ProcessList,
    pub(crate) current_ram_percent: u32,
    pub(crate) cpu_monitor: CpuThrottleMonitor,
    window_bg_brush: HBRUSH,
    tray: TrayIcons,
    pub(crate) settings_window: HWND,
    notify_text_buf: Vec<u16>,
}

impl AppState {
    fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            processes: ProcessList::new(),
            current_ram_percent: 0,
            cpu_monitor: CpuThrottleMonitor::new(),
            // Same color as the window background: used as the background
            // brush when painting the red throttling label.
            window_bg_brush: unsafe { CreateSolidBrush(crate::window::APP_WINDOW_BG_COLOR) },
            tray: TrayIcons::default(),
            settings_window: std::ptr::null_mut(),
            notify_text_buf: Vec::new(),
        }
    }
}

pub fn run(startup: Startup) -> AppResult<()> {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) } as HINSTANCE;
    if hinstance.is_null() {
        return Err(AppError::WinApi("GetModuleHandleW failed"));
    }

    if !register_main_window_class(hinstance, wnd_proc) {
        return Err(AppError::WinApi("RegisterClassW failed"));
    }

    if !register_settings_window_class(hinstance) {
        return Err(AppError::WinApi("RegisterClassW (settings) failed"));
    }

    let boxed_state = Box::new(AppState::new(startup.settings));
    let state_ptr = Box::into_raw(boxed_state);

    let hwnd = create_main_window(hinstance, state_ptr as isize);
    if hwnd.is_null() {
        unsafe {
            let _ = Box::from_raw(state_ptr);
        }
        return Err(AppError::WinApi("CreateWindowExW failed"));
    }

    if startup.start_hidden {
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

            let state_ptr = (*cs).lpCreateParams as *mut AppState;
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            let hinstance = (*cs).hInstance as HINSTANCE;
            create_controls(hwnd, hinstance);

            if let Some(state) = get_state_mut(hwnd) {
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
            if !nmhdr.is_null() && (*nmhdr).idFrom as i32 == IDC_PROCESS_LIST {
                match (*nmhdr).code {
                    LVN_GETDISPINFOW => {
                        if let Some(state) = get_state_mut(hwnd) {
                            let info = lparam as *mut NMLVDISPINFOW;
                            if !info.is_null() {
                                let item = &mut (*info).item;
                                if item.mask & LVIF_TEXT != 0 {
                                    let text = process_cell_text(state, item.iItem, item.iSubItem);
                                    state.notify_text_buf = wide(&text);
                                    item.pszText = state.notify_text_buf.as_mut_ptr();
                                }
                            }
                        }
                    }
                    // Double-click a group row: expand/collapse its instances.
                    LVN_ITEMACTIVATE => {
                        let item = lparam as *const NMLISTVIEW;
                        if !item.is_null()
                            && (*item).iItem >= 0
                            && let Some(state) = get_state_mut(hwnd)
                            && state.processes.toggle_group((*item).iItem as usize)
                        {
                            render_rows(hwnd, state);
                        }
                    }
                    _ => {}
                }
            }
            0
        }
        WM_COMMAND => {
            let id = loword(wparam as u32) as i32;
            let notify = hiword(wparam as u32);

            match id {
                IDC_SEARCH if notify == EN_CHANGE as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        state
                            .processes
                            .set_search(&get_control_text(hwnd, IDC_SEARCH));
                        render_rows(hwnd, state);
                    }
                }
                IDC_SORT_NAME if notify == BN_CLICKED as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        state.processes.toggle_sort(SortColumn::Name);
                        render_rows(hwnd, state);
                    }
                }
                IDC_SORT_MEMORY if notify == BN_CLICKED as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        state.processes.toggle_sort(SortColumn::Memory);
                        render_rows(hwnd, state);
                    }
                }
                IDC_SETTINGS_BUTTON if notify == BN_CLICKED as u16 => {
                    if let Some(state) = get_state_mut(hwnd) {
                        open_settings(hwnd, state);
                    }
                }
                IDC_TERMINATE_PROCESS if notify == BN_CLICKED as u16 => {
                    terminate_selected_process(hwnd);
                }
                _ => {}
            }

            0
        }
        WM_TIMER => {
            if wparam == TIMER_REFRESH_ID
                && let Some(state) = get_state_mut(hwnd)
            {
                refresh_ui_data(hwnd, state);
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

            let state_ptr =
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
                    as *mut AppState;
            if !state_ptr.is_null() {
                (*state_ptr).tray.remove_all(hwnd);
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
                && let Some(state) = get_state_mut(hwnd)
            {
                unsafe {
                    // COLORREF format is 0x00BBGGRR: red.
                    SetTextColor(wparam as HDC, 0x0000FF);
                    SetBkMode(wparam as HDC, TRANSPARENT as i32);
                }
                return state.window_bg_brush as LRESULT;
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
    // The MSRs are sampled by a background thread: this only collects what
    // it latched since the previous tick.
    let cpu_throttling = state.cpu_monitor.refresh().is_throttling();
    let ram_status = total_memory_status().ok();

    // Average core temperature: only available on the MSR (driver) path.
    let cpu_temp = state.cpu_monitor.status().avg_temp_c;
    let cpu_temp_label = match cpu_temp {
        Some(t) => format!("CPU avg: {:.0} C", t),
        None => "CPU avg: n/a".to_string(),
    };
    set_label_text(hwnd, IDC_HEADER_CPU_TEMP, &cpu_temp_label);

    // A single throttled core (or the package, or PROCHOT) since the
    // previous tick is enough to light up the warning.
    set_label_text(
        hwnd,
        IDC_HEADER_CPU_THROTTLE,
        if cpu_throttling { "T" } else { "" },
    );

    let memory = if let Some((ram_percent, total, available)) = ram_status {
        state.current_ram_percent = ram_percent;

        set_label_text(
            hwnd,
            IDC_HEADER_RAM,
            &format!("Total RAM used: {}%", state.current_ram_percent),
        );
        Some((total, total.saturating_sub(available)))
    } else {
        None
    };

    refresh_tray_icons(hwnd, state);

    if let Some((total, in_use)) = memory {
        state
            .processes
            .set_snapshot(snapshot_processes(total), total, in_use);
        render_rows(hwnd, state);
    }
}

fn render_rows(hwnd: HWND, state: &AppState) {
    set_virtual_item_count(hwnd, state.processes.rows().len());
}

/// Rebuilds the tray icons from the current settings and readings.
pub(crate) fn refresh_tray_icons(hwnd: HWND, state: &mut AppState) {
    state.tray.apply(
        hwnd,
        &state.settings,
        state.current_ram_percent,
        state.cpu_monitor.status(),
    );
}

/// Restarts the main refresh timer with the given interval (seconds).
pub(crate) fn restart_refresh_timer(hwnd: HWND, seconds: u64) {
    unsafe {
        let _ = KillTimer(hwnd, TIMER_REFRESH_ID);
        let _ = SetTimer(hwnd, TIMER_REFRESH_ID, (seconds * 1000) as u32, None);
    }
}

pub(crate) fn show_error(hwnd: HWND, text: &str) {
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

    let list = &state.processes;
    let Some(display) = list.rows().get(row as usize) else {
        return String::new();
    };

    match display {
        DisplayRow::Group {
            name,
            total_bytes,
            count,
            expanded,
        } => match column {
            0 => format!(
                "{} {} ({}x)",
                if *expanded { "▼" } else { "►" },
                name,
                count
            ),
            1 => "-".to_string(),
            2 => format!("{:.2}", mib(*total_bytes)),
            3 => format!("{:.2}", list.percent_of_ram(*total_bytes)),
            _ => String::new(),
        },
        DisplayRow::Proc(process) => match column {
            0 => format!("      {}", process.name),
            1 => process.pid.to_string(),
            2 => format!("{:.2}", process.memory_mib),
            3 => format!("{:.2}", process.memory_percent),
            _ => String::new(),
        },
        DisplayRow::System { bytes } => match column {
            0 => "System & shared".to_string(),
            1 => "-".to_string(),
            2 => format!("{:.2}", mib(*bytes)),
            3 => format!("{:.2}", list.percent_of_ram(*bytes)),
            _ => String::new(),
        },
    }
}

fn terminate_selected_process(hwnd: HWND) {
    let Some(state) = get_state_mut(hwnd) else {
        return;
    };

    let Some(row) = selected_process_row(hwnd) else {
        show_error(hwnd, "Select a process from the list first.");
        return;
    };

    let pids = match state
        .processes
        .pids_to_terminate(row as usize, std::process::id())
    {
        Ok(pids) => pids,
        Err(err) => {
            show_error(hwnd, &err.to_string());
            return;
        }
    };

    let mut first_error: Option<String> = None;
    for pid in &pids {
        if let Err(err) = terminate_process(*pid)
            && first_error.is_none()
        {
            first_error = Some(format!("PID {pid}: {err}"));
        }
    }

    if let Some(err) = first_error {
        show_error(hwnd, &format!("Could not terminate some processes: {err}"));
        return;
    }

    refresh_ui_data(hwnd, state);
}
