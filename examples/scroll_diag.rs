//! Diagnostic harness: does LVM_SETITEMCOUNT reset the listview scroll, and
//! does LVSICF_NOSCROLL preserve it? Also checks whether a second
//! InitCommonControlsEx call (ICC_TAB_CLASSES after ICC_LISTVIEW_CLASSES)
//! allows creating SysTabControl32 (the settings tabs depend on it).
//!
//! Run: cargo run --example scroll_diag

use std::mem;

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    ICC_LISTVIEW_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_FMT,
    LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT, LVCOLUMNW, LVIF_STATE, LVIS_FOCUSED, LVIS_SELECTED,
    LVITEMW, LVM_ENSUREVISIBLE, LVM_GETITEMCOUNT, LVM_GETNEXTITEM, LVM_GETTOPINDEX,
    LVM_INSERTCOLUMNW, LVM_SETITEMCOUNT, LVM_SETITEMSTATE, LVNI_SELECTED, LVS_OWNERDATA,
    LVS_REPORT, LVSICF_NOSCROLL, TCIF_TEXT, TCITEMW, TCM_GETCURSEL, TCM_GETITEMCOUNT,
    TCM_INSERTITEMW, WC_LISTVIEWW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SendMessageW, WS_CHILD, WS_VISIBLE,
};

#[allow(clippy::upper_case_acronyms)]
type HWND = windows_sys::Win32::Foundation::HWND;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn send(list: HWND, msg: u32, w: usize, l: isize) -> i32 {
    (unsafe { SendMessageW(list, msg, w, l) }) as i32
}

fn item_state() -> LVITEMW {
    LVITEMW {
        mask: LVIF_STATE,
        state: LVIS_SELECTED | LVIS_FOCUSED,
        stateMask: LVIF_STATE,
        ..unsafe { mem::zeroed() }
    }
}

fn main() {
    let hinst = unsafe { GetModuleHandleW(std::ptr::null_mut()) };

    let icc = INITCOMMONCONTROLSEX {
        dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES,
    };
    let r1 = unsafe { InitCommonControlsEx(&icc) };

    // Hidden parent: the scroll state of a listview is kept even when not
    // shown, and nothing here needs a paint-driven message loop.
    let parent: HWND = unsafe {
        CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            300,
            200,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        )
    };

    let list: HWND = unsafe {
        CreateWindowExW(
            0,
            WC_LISTVIEWW,
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE | LVS_REPORT | LVS_OWNERDATA,
            0,
            0,
            280,
            180,
            parent,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        )
    };

    let col_text = wide("Test");
    let col = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
        fmt: LVCFMT_LEFT,
        cx: 250,
        pszText: col_text.as_ptr() as *mut _,
        ..unsafe { mem::zeroed() }
    };
    unsafe {
        SendMessageW(list, LVM_INSERTCOLUMNW, 0, &col as *const _ as isize);
        SendMessageW(list, LVM_SETITEMCOUNT, 1000, 0);
    }

    // Park the view around row 300 so any reset to the top is obvious.
    unsafe {
        SendMessageW(list, LVM_ENSUREVISIBLE, 300, 0);
    }
    let parked = send(list, LVM_GETTOPINDEX, 0, 0);
    println!(
        "parked top index = {parked} (count {})",
        send(list, LVM_GETITEMCOUNT, 0, 0)
    );

    // Is the selection also wiped by SETITEMCOUNT?
    unsafe {
        SendMessageW(
            list,
            LVM_SETITEMSTATE,
            300,
            &mut item_state() as *mut _ as isize,
        );
        SendMessageW(list, LVM_SETITEMCOUNT, 998, 0);
    }
    println!(
        "selection after SETITEMCOUNT:       sel = {}",
        send(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize)
    );

    unsafe {
        SendMessageW(list, LVM_SETITEMCOUNT, 999, 0);
    }
    let after_plain = send(list, LVM_GETTOPINDEX, 0, 0);
    println!("after SETITEMCOUNT(999, 0):          top = {after_plain}");

    unsafe {
        SendMessageW(list, LVM_ENSUREVISIBLE, 300, 0);
        SendMessageW(list, LVM_SETITEMCOUNT, 1001, LVSICF_NOSCROLL as isize);
    }
    let after_noscroll = send(list, LVM_GETTOPINDEX, 0, 0);
    println!("after SETITEMCOUNT(1001, NOSCROLL):  top = {after_noscroll}");

    // Tab class registration after the first InitCommonControlsEx call.
    let icc2 = INITCOMMONCONTROLSEX {
        dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_TAB_CLASSES,
    };
    let r2 = unsafe { InitCommonControlsEx(&icc2) };
    let tab: HWND = unsafe {
        CreateWindowExW(
            0,
            wide("SysTabControl32").as_ptr(),
            std::ptr::null(),
            WS_CHILD,
            0,
            0,
            280,
            180,
            parent,
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        )
    };
    println!(
        "InitCommonControlsEx #1(LISTVIEW) = {}, #2(TAB) = {}; SysTabControl32 null? {}",
        r1,
        r2,
        tab.is_null()
    );

    // Tab items must actually land in the control (they never did while the
    // code used a bogus TCM_INSERTITEMW/TCM_GETCURSEL value).
    for (index, text) in ["General", "Tray icons & colors"].iter().enumerate() {
        let mut text = wide(text);
        let mut item = TCITEMW {
            mask: TCIF_TEXT,
            pszText: text.as_mut_ptr(),
            ..unsafe { mem::zeroed() }
        };
        unsafe {
            SendMessageW(tab, TCM_INSERTITEMW, index, &mut item as *mut _ as isize);
        }
    }
    println!(
        "tabs inserted = {}, selected = {}",
        send(tab, TCM_GETITEMCOUNT, 0, 0),
        send(tab, TCM_GETCURSEL, 0, 0)
    );
}
