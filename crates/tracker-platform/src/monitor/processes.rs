use std::collections::HashMap;
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;

use crate::model::ProcessInfo;
use windows_sys::Wdk::System::SystemInformation::{
    NtQuerySystemInformation, SystemProcessInformation,
};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{
    GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    PROCESS_MEMORY_COUNTERS_EX2,
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
    // Processes we cannot open are resolved from one system-wide snapshot,
    // taken only if at least one open fails.
    let mut unopenable: Option<HashMap<u32, u64>> = None;
    while has_entry {
        let pid = pe.th32ProcessID;
        let name = utf16_to_string(&pe.szExeFile);

        let memory_bytes = process_private_ws_bytes(pid).or_else(|| {
            unopenable
                .get_or_insert_with(unopenable_private_ws_map)
                .get(&pid)
                .copied()
        });

        if let Some(memory_bytes) = memory_bytes {
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

/// Private working set in bytes: the resident-RAM value shown in the Task
/// Manager's "Memory" column. The plain `WorkingSetSize` would also count
/// file-backed pages — e.g. a llama.cpp server with an mmap'ed model would
/// report the whole model size instead of its real RAM usage.
fn process_private_ws_bytes(pid: u32) -> Option<u64> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };

    if handle.is_null() {
        return None;
    }

    let bytes = query_private_ws_bytes(handle);

    unsafe {
        CloseHandle(handle);
    }

    bytes
}

fn query_private_ws_bytes(handle: HANDLE) -> Option<u64> {
    let mut ex2: PROCESS_MEMORY_COUNTERS_EX2 = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        GetProcessMemoryInfo(
            handle,
            // The API takes any of the counters structs; the cb size selects
            // which one gets filled (classic Win32 pattern).
            &mut ex2 as *mut PROCESS_MEMORY_COUNTERS_EX2 as *mut PROCESS_MEMORY_COUNTERS,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        )
    };

    if ok != 0 && ex2.PrivateWorkingSetSize != 0 {
        return Some(ex2.PrivateWorkingSetSize as u64);
    }

    // EX2 only exists since the September 2023 cumulative updates, and an older
    // OS may report success while leaving the extra fields zeroed.
    let mut ex: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        GetProcessMemoryInfo(
            handle,
            &mut ex as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        )
    };

    if ok == 0 {
        return None;
    }

    // Fallback: commit charge. An over-estimate, but the best older systems
    // expose (the private working set is not available before EX2).
    Some(ex.PrivateUsage as u64)
}

/// `SYSTEM_PROCESS_INFORMATION` (x64) byte offsets. The struct is undocumented
/// and its tail varies between builds, so only these fixed prefix fields are
/// read. Verified against `GetProcessMemoryInfo` on the same process:
/// +8 private working set, +144 working set, +184 commit.
const OFF_NEXT_ENTRY: usize = 0;
const OFF_PRIVATE_WORKING_SET: usize = 8;
const OFF_PROCESS_ID: usize = 80;
const OFF_WORKING_SET: usize = 144;
const ENTRY_MIN_LEN: usize = OFF_WORKING_SET + 8;

const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

/// pid -> private working set for the processes we are not allowed to open.
/// Protected processes (the WSL2 `vmmemWSL` worker is a VBS one) deny even
/// `PROCESS_QUERY_LIMITED_INFORMATION` to an elevated but unsigned process, so
/// no amount of rights helps: the system-wide snapshot needs no handle.
fn unopenable_private_ws_map() -> HashMap<u32, u64> {
    let Some(buf) = system_process_snapshot() else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    let mut offset = 0usize;
    // The entries are a singly linked list, terminated by NextEntryOffset 0.
    while offset + ENTRY_MIN_LEN <= buf.len() {
        let private_ws = u64_at(&buf, offset + OFF_PRIVATE_WORKING_SET);
        let bytes = if private_ws == 0 {
            // Builds before Windows 10 1703 leave the private size at zero.
            u64_at(&buf, offset + OFF_WORKING_SET)
        } else {
            private_ws
        };
        map.insert(u32_at(&buf, offset + OFF_PROCESS_ID), bytes);

        // Offsets come from the kernel: validate before stepping.
        match u32_at(&buf, offset + OFF_NEXT_ENTRY) as usize {
            0 => break,
            next => match offset.checked_add(next) {
                Some(next_offset) => offset = next_offset,
                None => break,
            },
        }
    }

    map
}

fn system_process_snapshot() -> Option<Vec<u8>> {
    let mut len = 256 * 1024u32;
    loop {
        let mut buf = vec![0u8; len as usize];
        let mut returned = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessInformation,
                buf.as_mut_ptr() as *mut c_void,
                len,
                &mut returned,
            )
        };
        if status == 0 {
            buf.truncate(returned as usize);
            return Some(buf);
        }
        if status != STATUS_INFO_LENGTH_MISMATCH {
            return None;
        }
        // The kernel reports the size it needs; keep slack for the processes
        // spawning between the two calls.
        len = returned.saturating_add(64 * 1024);
    }
}

// Bounds are checked by the caller before every read; a short read degrades to
// 0 instead of panicking, since the data comes from the kernel.
fn u32_at(buf: &[u8], offset: usize) -> u32 {
    buf.get(offset..offset + 4)
        .and_then(|bytes| bytes.first_chunk::<4>())
        .map_or(0, |bytes| u32::from_ne_bytes(*bytes))
}

fn u64_at(buf: &[u8], offset: usize) -> u64 {
    buf.get(offset..offset + 8)
        .and_then(|bytes| bytes.first_chunk::<8>())
        .map_or(0, |bytes| u64::from_ne_bytes(*bytes))
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
