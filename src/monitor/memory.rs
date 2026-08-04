use std::mem::size_of;

use crate::error::{AppError, AppResult};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

pub fn total_memory_status() -> AppResult<(u32, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };

    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return Err(AppError::WinApi("GlobalMemoryStatusEx failed"));
    }

    Ok((status.dwMemoryLoad, status.ullTotalPhys))
}
