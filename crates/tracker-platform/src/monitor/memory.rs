use std::mem::size_of;

use crate::error::{AppError, AppResult};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// `(load percent, total physical bytes, available bytes)` as reported by
/// Windows. The percent is the same "in use" figure as the Task Manager's
/// performance tab: it covers every resident page, including the kernel,
/// the drivers and the pages shared between processes, so the per-process
/// rows are not expected to sum up to it.
pub fn total_memory_status() -> AppResult<(u32, u64, u64)> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };

    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return Err(AppError::WinApi("GlobalMemoryStatusEx failed"));
    }

    Ok((
        status.dwMemoryLoad,
        status.ullTotalPhys,
        status.ullAvailPhys,
    ))
}
