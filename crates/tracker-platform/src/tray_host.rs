//! Tray icons owned by a dedicated thread with its own hidden window and
//! message loop, for frontends whose event loop is not ours (GPUI). The
//! frontend pushes readings with [`TrayHost::update`] and gets clicks back
//! through the event callback, called on the tray thread.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetCursorPos, GetMessageW, GetWindowLongPtrW, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SetWindowLongPtrW, TranslateMessage, WM_APP, WM_CLOSE,
    WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_OVERLAPPED,
};

use crate::error::{AppError, AppResult};
use crate::model::{AppSettings, ThermalStatus};
use crate::tray::{TrayIcons, WM_TRAYICON};
use crate::tray_menu::{CMD_EXIT_APP, CMD_OPEN_WINDOW, show_tray_context_menu};

const CLASS_NAME: &str = "ActivityTrackerTrayHost";
/// Posted by [`TrayHost::update`]: redraw the icons from the shared content.
const WM_APP_REFRESH: u32 = WM_APP + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayEvent {
    /// Double-click on an icon, or "Open window" from its menu.
    Open,
    /// "Exit" from the menu.
    Exit,
}

#[derive(Clone, Default)]
struct TrayContent {
    settings: AppSettings,
    ram_percent: u32,
    thermal: ThermalStatus,
}

struct HostState {
    icons: TrayIcons,
    content: Arc<Mutex<Option<TrayContent>>>,
    on_event: Box<dyn Fn(TrayEvent) + Send>,
    /// Broadcast when Explorer (re)starts: the icons must be added again.
    taskbar_created: u32,
}

/// Owns the tray thread; removes the icons and joins the thread on drop.
pub struct TrayHost {
    /// The hidden window, as an integer so the host is `Send`/`Sync`.
    hwnd: isize,
    content: Arc<Mutex<Option<TrayContent>>>,
    thread: Option<JoinHandle<()>>,
}

impl TrayHost {
    pub fn start(on_event: impl Fn(TrayEvent) + Send + 'static) -> AppResult<Self> {
        let content = Arc::new(Mutex::new(None));
        let (ready_tx, ready_rx) = mpsc::channel::<isize>();

        let shared = Arc::clone(&content);
        let on_event: Box<dyn Fn(TrayEvent) + Send> = Box::new(on_event);

        let thread = std::thread::Builder::new()
            .name("tray".into())
            .spawn(move || {
                // Built on the tray thread: the icon handles never leave it.
                let state = HostState {
                    icons: TrayIcons::default(),
                    content: shared,
                    on_event,
                    taskbar_created: unsafe {
                        RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())
                    },
                };
                let hwnd = create_host_window(state);
                let _ = ready_tx.send(hwnd as isize);
                if hwnd.is_null() {
                    return;
                }
                let mut msg: MSG = unsafe { std::mem::zeroed() };
                while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            })
            .map_err(AppError::Io)?;

        let hwnd = ready_rx.recv().unwrap_or(0);
        if hwnd == 0 {
            let _ = thread.join();
            return Err(AppError::WinApi("tray host window creation failed"));
        }
        Ok(Self {
            hwnd,
            content,
            thread: Some(thread),
        })
    }

    /// Redraws the icons with the given settings and readings.
    pub fn update(&self, settings: &AppSettings, ram_percent: u32, thermal: &ThermalStatus) {
        if let Ok(mut content) = self.content.lock() {
            *content = Some(TrayContent {
                settings: settings.clone(),
                ram_percent,
                thermal: thermal.clone(),
            });
        }
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_APP_REFRESH, 0, 0);
        }
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        unsafe {
            PostMessageW(self.hwnd as HWND, WM_CLOSE, 0, 0);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn create_host_window(state: HostState) -> HWND {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = wide(CLASS_NAME);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(host_wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name.as_ptr(),
            ..std::mem::zeroed()
        };
        // Fails harmlessly when the class is already registered (restart).
        RegisterClassW(&wc);

        let state_ptr = Box::into_raw(Box::new(state));
        // A real (never shown) top-level window rather than a message-only
        // one: those miss the TaskbarCreated broadcast, and the popup menu
        // needs a window that can take the foreground.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            class_name.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            state_ptr as *const _,
        );
        if hwnd.is_null() {
            drop(Box::from_raw(state_ptr));
        }
        hwnd
    }
}

fn host_state(hwnd: HWND) -> Option<&'static mut HostState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut HostState;
    (!ptr.is_null()).then(|| unsafe { &mut *ptr })
}

fn refresh_icons(hwnd: HWND, state: &mut HostState) {
    let content = state.content.lock().ok().and_then(|c| c.clone());
    if let Some(content) = content {
        state.icons.apply(
            hwnd,
            &content.settings,
            content.ram_percent,
            &content.thermal,
        );
    }
}

unsafe extern "system" fn host_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam as *const CREATESTRUCTW;
            if !cs.is_null() {
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize) };
            }
            0
        }
        WM_APP_REFRESH => {
            if let Some(state) = host_state(hwnd) {
                refresh_icons(hwnd, state);
            }
            0
        }
        WM_TRAYICON => {
            let Some(state) = host_state(hwnd) else {
                return 0;
            };
            match lparam as u32 {
                WM_LBUTTONDBLCLK => (state.on_event)(TrayEvent::Open),
                WM_RBUTTONUP => {
                    let mut pt = POINT { x: 0, y: 0 };
                    unsafe { GetCursorPos(&mut pt) };
                    match show_tray_context_menu(hwnd, pt.x, pt.y) {
                        CMD_OPEN_WINDOW => (state.on_event)(TrayEvent::Open),
                        CMD_EXIT_APP => (state.on_event)(TrayEvent::Exit),
                        _ => {}
                    }
                }
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut HostState;
            if !ptr.is_null() {
                let mut state = unsafe { Box::from_raw(ptr) };
                state.icons.remove_all(hwnd);
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            }
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => {
            if let Some(state) = host_state(hwnd)
                && state.taskbar_created != 0
                && msg == state.taskbar_created
            {
                // Explorer restarted: the old icons are gone, add them again.
                state.icons = TrayIcons::default();
                refresh_icons(hwnd, state);
                return 0;
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
