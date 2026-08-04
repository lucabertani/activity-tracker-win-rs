use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, MF_STRING, SetForegroundWindow, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
};

pub const CMD_OPEN_WINDOW: u16 = 1001;
pub const CMD_EXIT_APP: u16 = 1002;

pub fn show_tray_context_menu(hwnd: HWND, x: i32, y: i32) -> u16 {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return 0;
        }

        let open = wide("Apri finestra");
        let exit = wide("Esci");

        let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN_WINDOW as usize, open.as_ptr());
        let _ = AppendMenuW(menu, MF_STRING, CMD_EXIT_APP as usize, exit.as_ptr());

        let _ = SetForegroundWindow(hwnd);
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            x,
            y,
            0,
            hwnd,
            std::ptr::null(),
        );

        let _ = DestroyMenu(menu);
        selected as u16
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
