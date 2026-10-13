#![allow(unsafe_op_in_unsafe_fn)]

use std::collections::HashSet;
use std::mem;

use windows_sys::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, HBRUSH, HDC, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    LVIF_TEXT, LVN_GETDISPINFOW, LVN_ITEMACTIVATE, NMLISTVIEW, NMLVDISPINFOW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, CREATESTRUCTW, DefWindowProcW, DestroyWindow, DispatchMessageW, EN_CHANGE,
    GWLP_USERDATA, GetCursorPos, GetDlgCtrlID, GetMessageW, KillTimer, MB_ICONERROR, MB_OK, MSG,
    MessageBoxW, PostQuitMessage, SW_SHOWNORMAL, SetTimer, SetWindowLongPtrW, TranslateMessage,
    WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORSTATIC, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NOTIFY,
    WM_RBUTTONUP, WM_SIZE, WM_TIMER,
};

use crate::error::{AppError, AppResult};
use crate::model::{AppSettings, DisplayRow, ProcessInfo, SortColumn, SortState};
use crate::monitor::cpu::CpuThrottleMonitor;
use crate::monitor::memory::total_memory_status;
use crate::monitor::msr::{MsrThermalMonitor, OpenError};
use crate::monitor::processes::{build_display_rows, snapshot_processes, terminate_process};
use crate::settings::store::load_settings;
use crate::ui::menu::{CMD_EXIT_APP, CMD_OPEN_WINDOW, show_tray_context_menu};
use crate::ui::settings_window::{open_settings, register_settings_window_class};
use crate::ui::tray::{
    TRAY_UID_CPU, TRAY_UID_FALLBACK, TRAY_UID_RAM, WM_TRAYICON, add_tray_icon, build_cpu_icon,
    build_fallback_icon, build_ram_icon, destroy_icon, remove_tray_icon, update_tray_icon,
    update_tray_tooltip,
};
use crate::ui::window::{
    IDC_HEADER_CPU_TEMP, IDC_HEADER_CPU_THROTTLE, IDC_HEADER_RAM, IDC_PROCESS_LIST, IDC_SEARCH,
    IDC_SETTINGS_BUTTON, IDC_SORT_MEMORY, IDC_SORT_NAME, IDC_TERMINATE_PROCESS, create_controls,
    create_main_window, get_control_text, hide_main_window, register_main_window_class,
    selected_process_row, set_label_text, set_virtual_item_count, show_main_window, wide,
};

const TIMER_REFRESH_ID: usize = 1;

#[allow(clippy::upper_case_acronyms)]
type HICON = windows_sys::Win32::UI::WindowsAndMessaging::HICON;

pub(crate) struct AppState {
    pub(crate) settings: AppSettings,
    sort: SortState,
    search_query: String,
    all_processes: Vec<ProcessInfo>,
    visible_rows: Vec<DisplayRow>,
    // Lower-cased process names whose group the user has expanded.
    expanded_groups: HashSet<String>,
    total_ram_bytes: u64,
    pub(crate) current_ram_percent: u32,
    pub(crate) cpu_monitor: CpuThrottleMonitor,
    cpu_throttling: bool,
    window_bg_brush: HBRUSH,
    tray_icon_ram: HICON,
    tray_icon_cpu: HICON,
    tray_icon_fallback: HICON,
    pub(crate) settings_window: HWND,
    notify_text_buf: Vec<u16>,
}

impl AppState {
    fn new(settings: AppSettings) -> Self {
        Self {
            settings,
            sort: SortState::default(),
            search_query: String::new(),
            all_processes: Vec::new(),
            visible_rows: Vec::new(),
            expanded_groups: HashSet::new(),
            total_ram_bytes: 0,
            current_ram_percent: 0,
            cpu_monitor: CpuThrottleMonitor::new(),
            cpu_throttling: false,
            // Same color as the window background: used as the background
            // brush when painting the red throttling label.
            window_bg_brush: unsafe { CreateSolidBrush(crate::ui::window::APP_WINDOW_BG_COLOR) },
            tray_icon_ram: std::ptr::null_mut(),
            tray_icon_cpu: std::ptr::null_mut(),
            tray_icon_fallback: std::ptr::null_mut(),
            settings_window: std::ptr::null_mut(),
            notify_text_buf: Vec::new(),
        }
    }
}

/// The PawnIO temperature device allows administrators only: relaunch this
/// exe elevated (`runas`) and let the copy take over. Returns only when
/// elevation is not possible (UAC declined): the app then runs without
/// CPU temperatures.
fn relaunch_elevated() {
    use std::os::windows::ffi::OsStrExt;

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let file: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let params: String = std::env::args()
        .skip(1)
        .map(|arg| {
            if arg.contains([' ', '"']) {
                format!("\"{}\"", arg.replace('"', "\\\""))
            } else {
                arg
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let params = wide(&params);

    let launched = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("runas").as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if launched as isize > 32 {
        // The elevated copy owns the window and tray icons from here on.
        std::process::exit(0);
    }
}

pub fn run() -> AppResult<()> {
    // The PawnIO device is admin-only: when it exists but denies this
    // process, take over with an elevated copy (UAC prompt).
    if let Err(OpenError::AccessDenied) = MsrThermalMonitor::open() {
        relaunch_elevated();
    }

    let mut settings = load_settings().unwrap_or_else(|_| AppSettings::default());
    settings = settings.normalized();

    let start_in_background_from_cli =
        std::env::args().any(|a| a.eq_ignore_ascii_case("--background"));

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
    // "Start in background" is enabled.
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
                            && let Some(state) = get_state_mut(hwnd)
                        {
                            let key =
                                state
                                    .visible_rows
                                    .get((*item).iItem as usize)
                                    .and_then(|row| match row {
                                        DisplayRow::Group { name, .. } => {
                                            Some(name.to_ascii_lowercase())
                                        }
                                        _ => None,
                                    });
                            if let Some(key) = key {
                                // remove returns true if the group was expanded:
                                // true -> collapse, false -> expand.
                                if !state.expanded_groups.remove(&key) {
                                    state.expanded_groups.insert(key);
                                }
                                rebuild_visible_and_render(hwnd, state);
                            }
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
            remove_tray_icon(hwnd, TRAY_UID_RAM);
            remove_tray_icon(hwnd, TRAY_UID_CPU);
            remove_tray_icon(hwnd, TRAY_UID_FALLBACK);

            let state_ptr =
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, GWLP_USERDATA)
                    as *mut AppState;
            if !state_ptr.is_null() {
                if !(*state_ptr).tray_icon_ram.is_null() {
                    destroy_icon((*state_ptr).tray_icon_ram);
                    (*state_ptr).tray_icon_ram = std::ptr::null_mut();
                }
                if !(*state_ptr).tray_icon_cpu.is_null() {
                    destroy_icon((*state_ptr).tray_icon_cpu);
                    (*state_ptr).tray_icon_cpu = std::ptr::null_mut();
                }
                if !(*state_ptr).tray_icon_fallback.is_null() {
                    destroy_icon((*state_ptr).tray_icon_fallback);
                    (*state_ptr).tray_icon_fallback = std::ptr::null_mut();
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
    // RAM and CPU sampling run in parallel: both are non-blocking Win32 calls,
    // and the CPU path (per-core MSR reads with affinity pinning) is the
    // slower of the two.
    let mut cpu_throttling = false;
    let mut ram_status: Option<(u32, u64)> = None;
    std::thread::scope(|s| {
        s.spawn(|| {
            cpu_throttling = state.cpu_monitor.refresh();
        });
        s.spawn(|| {
            ram_status = total_memory_status().ok();
        });
    });

    // Average core temperature: only available on the MSR (driver) path.
    let cpu_temp = state.cpu_monitor.avg_temperature_celsius();
    let cpu_temp_label = match cpu_temp {
        Some(t) => format!("CPU avg: {:.0} C", t),
        None => "CPU avg: n/a".to_string(),
    };
    set_label_text(hwnd, IDC_HEADER_CPU_TEMP, &cpu_temp_label);

    // Thermal throttling check: same refresh interval as the RAM percentage.
    // A single throttled core is enough to light up the warning.
    set_label_text(
        hwnd,
        IDC_HEADER_CPU_THROTTLE,
        if cpu_throttling {
            "CPU thermal throttling"
        } else {
            ""
        },
    );

    // The tray icons (RAM + CPU) are refreshed on every tick: colors come
    // from the settings, with the red override while thermal throttling.
    state.cpu_throttling = cpu_throttling;

    let total_ram_bytes = if let Some((ram_percent, total)) = ram_status {
        state.current_ram_percent = ram_percent;
        state.total_ram_bytes = total;

        set_label_text(
            hwnd,
            IDC_HEADER_RAM,
            &format!("Total RAM used: {}%", state.current_ram_percent),
        );
        Some(total)
    } else {
        None
    };

    refresh_tray_icons(hwnd, state);

    if let Some(total_ram_bytes) = total_ram_bytes {
        state.all_processes = snapshot_processes(total_ram_bytes);
        rebuild_visible_and_render(hwnd, state);
    }
}

fn rebuild_visible_and_render(hwnd: HWND, state: &mut AppState) {
    state.visible_rows = build_display_rows(
        &state.all_processes,
        &state.search_query,
        state.sort,
        &state.expanded_groups,
    );
    set_virtual_item_count(hwnd, state.visible_rows.len());
}

/// Rebuilds both tray icons from the current settings: each icon is added
/// or removed according to its enable flag and redrawn with the configured
/// colors (red background while thermal throttling).
pub(crate) fn refresh_tray_icons(hwnd: HWND, state: &mut AppState) {
    let red = state.settings.tray_icon_thermal_red && state.cpu_throttling;

    let ram_tip = format!("RAM {}%", state.current_ram_percent);
    apply_tray_icon(
        hwnd,
        TRAY_UID_RAM,
        &mut state.tray_icon_ram,
        state.settings.tray_icon_ram_enabled,
        build_ram_icon(
            state.current_ram_percent,
            red,
            state.settings.tray_icon_ram_bg,
            state.settings.tray_icon_ram_fg,
            state.settings.tray_icon_font_size,
            state.settings.tray_icon_font_weight,
        ),
        &ram_tip,
    );

    let temp = state
        .cpu_monitor
        .avg_temperature_celsius()
        .map(|t| t as u32);
    let cpu_tip = match temp {
        Some(t) => format!("CPU avg {t} C"),
        None => "CPU avg: n/a (PawnIO driver missing)".to_string(),
    };
    apply_tray_icon(
        hwnd,
        TRAY_UID_CPU,
        &mut state.tray_icon_cpu,
        state.settings.tray_icon_cpu_enabled,
        build_cpu_icon(
            temp,
            red,
            state.settings.tray_icon_cpu_bg,
            state.settings.tray_icon_cpu_fg,
            state.settings.tray_icon_font_size,
            state.settings.tray_icon_font_weight,
        ),
        &cpu_tip,
    );

    // With both icons off the tray would go silent: keep a plain "AT" badge
    // so the app is still visibly running in the background.
    let show_fallback =
        !state.settings.tray_icon_ram_enabled && !state.settings.tray_icon_cpu_enabled;
    let fallback_icon = if show_fallback {
        build_fallback_icon(state.settings.tray_icon_font_weight)
    } else {
        std::ptr::null_mut()
    };
    apply_tray_icon(
        hwnd,
        TRAY_UID_FALLBACK,
        &mut state.tray_icon_fallback,
        show_fallback,
        fallback_icon,
        "Activity Tracker",
    );
}

fn apply_tray_icon(
    hwnd: HWND,
    uid: u32,
    current: &mut HICON,
    enabled: bool,
    icon: HICON,
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

    let Some(display) = state.visible_rows.get(row as usize) else {
        return String::new();
    };

    match display {
        DisplayRow::Group {
            name,
            total_bytes,
            count,
            expanded,
        } => {
            let total_mib = *total_bytes as f64 / 1024.0 / 1024.0;
            let total_percent = if state.total_ram_bytes == 0 {
                0.0
            } else {
                (*total_bytes as f64 / state.total_ram_bytes as f64) * 100.0
            };
            match column {
                0 => format!(
                    "{} {} ({}x)",
                    if *expanded { "▼" } else { "►" },
                    name,
                    count
                ),
                1 => "-".to_string(),
                2 => format!("{:.2}", total_mib),
                3 => format!("{:.2}", total_percent),
                _ => String::new(),
            }
        }
        DisplayRow::Proc(process) => match column {
            0 => format!("      {}", process.name),
            1 => process.pid.to_string(),
            2 => format!("{:.2}", process.memory_mib),
            3 => format!("{:.2}", process.memory_percent),
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

    let Some(display) = state.visible_rows.get(row as usize).cloned() else {
        show_error(hwnd, "Invalid process.");
        return;
    };

    // A group row terminates every instance of that process
    // (like Task Manager's "End task group"); a child row just itself.
    let pids: Vec<u32> = match display {
        DisplayRow::Proc(process) => vec![process.pid],
        DisplayRow::Group { name, .. } => {
            let key = name.to_ascii_lowercase();
            state
                .all_processes
                .iter()
                .filter(|p| p.name.to_ascii_lowercase() == key)
                .map(|p| p.pid)
                .collect()
        }
    };
    let pids: Vec<u32> = pids
        .into_iter()
        .filter(|pid| *pid != std::process::id())
        .collect();

    if pids.is_empty() {
        show_error(
            hwnd,
            "You cannot terminate the current application from here.",
        );
        return;
    }

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
