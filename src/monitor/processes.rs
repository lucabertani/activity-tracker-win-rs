use std::cmp::Ordering;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use crate::model::{ProcessInfo, SortColumn, SortDirection, SortState};
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
    TerminateProcess,
};

pub fn snapshot_processes(total_ram_bytes: u64) -> Vec<ProcessInfo> {
    let mut out = Vec::with_capacity(256);

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return out;
    }

    let mut pe: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    pe.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut pe) } != 0;
    while has_entry {
        let pid = pe.th32ProcessID;
        let name = utf16_to_string(&pe.szExeFile);

        if let Some(memory_bytes) = process_private_ws_bytes(pid) {
            let memory_mib = memory_bytes as f64 / 1024.0 / 1024.0;
            let memory_percent = if total_ram_bytes == 0 {
                0.0
            } else {
                (memory_bytes as f64 / total_ram_bytes as f64) * 100.0
            };

            out.push(ProcessInfo {
                pid,
                name,
                memory_bytes,
                memory_mib,
                memory_percent,
            });
        }

        has_entry = unsafe { Process32NextW(snapshot, &mut pe) } != 0;
    }

    unsafe {
        CloseHandle(snapshot);
    }

    out
}

/// Private working set in bytes: the value shown in the Task Manager's
/// "Memory" column. The plain `WorkingSetSize` would also count file-backed
/// pages — e.g. a llama.cpp server with an mmap'ed model would report the
/// whole model size instead of its real RAM usage.
fn process_private_ws_bytes(pid: u32) -> Option<u64> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };

    if handle.is_null() {
        return None;
    }

    let mut counters: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        GetProcessMemoryInfo(
            handle,
            // The API takes the smaller struct and the cb field selects the
            // EX layout (classic Win32 pattern).
            &mut counters as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };

    unsafe {
        CloseHandle(handle);
    }

    if ok == 0 {
        return None;
    }

    Some(counters.PrivateUsage as u64)
}

fn utf16_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned()
}

pub fn terminate_process(pid: u32) -> Result<(), String> {
    let handle = unsafe {
        OpenProcess(
            PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return Err("Could not open the process (insufficient permissions?)".to_string());
    }

    let ok = unsafe { TerminateProcess(handle, 1) };
    unsafe {
        CloseHandle(handle);
    }

    if ok == 0 {
        return Err("Termination failed".to_string());
    }

    Ok(())
}

pub fn apply_filter_and_sort(
    source: &[ProcessInfo],
    filter: &str,
    sort: SortState,
) -> Vec<ProcessInfo> {
    let needle = filter.to_ascii_lowercase();

    let mut data: Vec<ProcessInfo> = source
        .iter()
        .filter(|p| {
            if needle.is_empty() {
                true
            } else {
                p.name.to_ascii_lowercase().contains(&needle)
            }
        })
        .cloned()
        .collect();

    data.sort_by(|a, b| compare_process(a, b, sort));
    data
}

fn compare_process(a: &ProcessInfo, b: &ProcessInfo, sort: SortState) -> Ordering {
    let base = match sort.sort_column {
        SortColumn::Name => a
            .name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
            .then(a.pid.cmp(&b.pid)),
        SortColumn::Memory => a.memory_bytes.cmp(&b.memory_bytes).then(
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase()),
        ),
    };

    match sort.sort_direction {
        SortDirection::Asc => base,
        SortDirection::Desc => base.reverse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str, memory_bytes: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: name.to_string(),
            memory_bytes,
            memory_mib: memory_bytes as f64 / 1024.0 / 1024.0,
            memory_percent: 0.0,
        }
    }

    #[test]
    fn filter_is_case_insensitive() {
        let source = vec![p(1, "Chrome.exe", 10), p(2, "notepad.exe", 20)];
        let out = apply_filter_and_sort(
            &source,
            "CHRO",
            SortState {
                sort_column: SortColumn::Name,
                sort_direction: SortDirection::Asc,
            },
        );

        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Chrome.exe");
    }

    #[test]
    fn sort_memory_desc() {
        let source = vec![p(1, "a", 100), p(2, "b", 300), p(3, "c", 200)];
        let out = apply_filter_and_sort(
            &source,
            "",
            SortState {
                sort_column: SortColumn::Memory,
                sort_direction: SortDirection::Desc,
            },
        );

        assert_eq!(out[0].memory_bytes, 300);
        assert_eq!(out[1].memory_bytes, 200);
        assert_eq!(out[2].memory_bytes, 100);
    }
}
