use std::collections::{HashMap, HashSet};
use std::ffi::{OsString, c_void};
use std::os::windows::ffi::OsStringExt;

use crate::model::{DisplayRow, ProcessInfo, SortColumn, SortDirection, SortState};
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

/// Builds the process list rows: one group row per process name (totals
/// over all its instances, with the expand/collapse state), followed by
/// one child row per instance of the expanded groups, and a final summary
/// row with the memory that belongs to no process.
/// `expanded` holds the lower-cased names of the groups the user opened.
/// `ram_in_use_bytes` is the system-wide resident memory (kernel, drivers and
/// shared pages included), used for that closing row.
///
/// ponytail: O(n^2) group lookup; fine for the few hundred processes a
/// Windows box runs. Switch to a HashMap if this ever shows up in a profile.
pub fn build_display_rows(
    source: &[ProcessInfo],
    filter: &str,
    sort: SortState,
    expanded: &HashSet<String>,
    ram_in_use_bytes: u64,
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

    // Closing row: everything resident that no process owns (kernel, drivers,
    // pages shared between processes). Without it the rows visibly stop short
    // of the header total, which is measured system-wide. Summed over `source`
    // and not over the filtered groups: the row is a system total, not a
    // process, so the search box does not narrow it.
    if ram_in_use_bytes > 0 {
        let listed: u64 = source.iter().map(|process| process.memory_bytes).sum();
        rows.push(DisplayRow::System {
            bytes: ram_in_use_bytes.saturating_sub(listed),
        });
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
        // 0 = no system summary row, this test is about the grouping.
        let rows = build_display_rows(&source, "", memory_desc(), &expanded, 0);

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
        let rows = build_display_rows(&source, "CHROME", memory_desc(), &HashSet::new(), 0);
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            DisplayRow::Group { name, .. } => assert_eq!(name, "chrome.exe"),
            _ => panic!("expected a group row"),
        }
    }

    #[test]
    fn system_row_closes_the_gap_to_the_header_total() {
        let source = vec![p(1, "firefox.exe", 100), p(2, "notepad.exe", 50)];
        // 350 MB resident, 150 MB listed -> 200 MB owned by kernel/drivers.
        let rows = build_display_rows(&source, "", memory_desc(), &HashSet::new(), 350);
        assert_eq!(rows.len(), 3);
        match rows.last().unwrap() {
            DisplayRow::System { bytes } => assert_eq!(*bytes, 200),
            other => panic!("expected the system row last, got {other:?}"),
        }
    }

    #[test]
    fn system_row_never_underflows() {
        // The memory status is sampled separately from the process list, so the
        // listed total can come out slightly larger than the system one.
        let source = vec![p(1, "firefox.exe", 500)];
        let rows = build_display_rows(&source, "", memory_desc(), &HashSet::new(), 100);
        match rows.last().unwrap() {
            DisplayRow::System { bytes } => assert_eq!(*bytes, 0),
            other => panic!("expected the system row last, got {other:?}"),
        }
    }
}
