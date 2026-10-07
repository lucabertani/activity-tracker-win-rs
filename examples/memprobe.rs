//! Temporary probe: sample one process with a SINGLE GetProcessMemoryInfo
//! call (consistent snapshot of all fields) over a few seconds, to see how
//! much llama-server's working set oscillates during inference.
//! Run: cargo run --example memprobe [pid]
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

fn main() {
    let pid: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or_else(|| {
            eprintln!("usage: memprobe <pid>");
            std::process::exit(2)
        });

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        eprintln!("OpenProcess failed");
        std::process::exit(1);
    }

    for i in 0..12 {
        let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        unsafe {
            GetProcessMemoryInfo(
                handle,
                &mut c as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            );
        }
        println!(
            "t{:>2}s  WS={:>6.1} MB   PrivateWS={:>6.1} MB",
            i as f64,
            c.WorkingSetSize as f64 / 1024.0 / 1024.0,
            c.PrivateUsage as f64 / 1024.0 / 1024.0,
        );
        thread::sleep(Duration::from_secs(1));
    }

    unsafe {
        CloseHandle(handle);
    }
}
