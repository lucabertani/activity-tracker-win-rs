use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use crate::model::{DisplayRow, ProcessInfo, SortColumn, SortDirection, SortState};
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

/// Builds the process list rows: one group row per process name (totals
/// over all its instances, with the expand/collapse state), followed by
/// one child row per instance of the expanded groups.
/// `expanded` holds the lower-cased names of the groups the user opened.
///
/// ponytail: O(n^2) group lookup; fine for the few hundred processes a
/// Windows box runs. Switch to a HashMap if this ever shows up in a profile.
pub fn build_display_rows(
    source: &[ProcessInfo],
    filter: &str,
    sort: SortState,
    expanded: &HashSet<String>,
) -> Vec<DisplayRow> {
    let needle = filter.to_ascii_lowercase();

    // (lower-cased key, display name, total bytes, instances)
    let mut groups: Vec<(String, String, u64, Vec<ProcessInfo>)> = Vec::new();
    for process in source {
        if !needle.is_empty() && !process.name.to_ascii_lowercase().contains(&needle) {
            continue;
        }
        let key = process.name.to_ascii_lowercase();
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(group) => {
                group.2 += process.memory_bytes;
                group.3.push(process.clone());
            }
            None => groups.push((
                key,
                process.name.clone(),
                process.memory_bytes,
                vec![process.clone()],
            )),
        }
    }

    groups.sort_by(|a, b| {
        let base = match sort.sort_column {
            SortColumn::Name => a.0.cmp(&b.0),
            SortColumn::Memory => a.2.cmp(&b.2).then(a.0.cmp(&b.0)),
        };
        match sort.sort_direction {
            SortDirection::Asc => base,
            SortDirection::Desc => base.reverse(),
        }
    });

    let mut rows = Vec::with_capacity(groups.len() * 2);
    for (key, name, total_bytes, procs) in groups {
        let is_expanded = expanded.contains(&key);
        rows.push(DisplayRow::Group {
            name,
            total_bytes,
            count: procs.len() as u32,
            expanded: is_expanded,
        });
        if is_expanded {
            let mut procs = procs;
            procs.sort_by_key(|a| std::cmp::Reverse(a.memory_bytes));
            rows.extend(procs.into_iter().map(DisplayRow::Proc));
        }
    }

    rows
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

    fn memory_desc() -> SortState {
        SortState {
            sort_column: SortColumn::Memory,
            sort_direction: SortDirection::Desc,
        }
    }

    #[test]
    fn groups_instances_and_sums_memory() {
        let source = vec![
            p(1, "firefox.exe", 100),
            p(2, "firefox.exe", 200),
            p(3, "notepad.exe", 50),
        ];
        let expanded = HashSet::from(["firefox.exe".to_ascii_lowercase()]);
        let rows = build_display_rows(&source, "", memory_desc(), &expanded);

        assert_eq!(rows.len(), 4);
        match &rows[0] {
            // firefox group first (300 > 50), expanded: group + 2 children
            DisplayRow::Group {
                name,
                total_bytes,
                count,
                expanded,
            } => {
                assert_eq!(name, "firefox.exe");
                assert_eq!(*total_bytes, 300);
                assert_eq!(*count, 2);
                assert!(*expanded);
            }
            _ => panic!("expected a group row first"),
        }
        match &rows[1] {
            // children sorted by memory desc
            DisplayRow::Proc(proc) => assert_eq!(proc.pid, 2),
            _ => panic!("expected a child row"),
        }
        match &rows[3] {
            DisplayRow::Group { name, expanded, .. } => {
                assert_eq!(name, "notepad.exe");
                assert!(!*expanded);
            }
            _ => panic!("expected a collapsed group row"),
        }
    }

    #[test]
    fn filter_hides_non_matching_groups() {
        let source = vec![p(1, "chrome.exe", 10), p(2, "firefox.exe", 20)];
        let rows = build_display_rows(&source, "CHROME", memory_desc(), &HashSet::new());
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            DisplayRow::Group { name, .. } => assert_eq!(name, "chrome.exe"),
            _ => panic!("expected a group row"),
        }
    }
}
