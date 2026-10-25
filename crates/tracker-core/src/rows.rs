//! Process list rows: grouping by name, search filter, sorting and the
//! closing "System & shared" row. Pure logic, shared by every GUI.

use std::collections::HashSet;

use crate::model::{DisplayRow, ProcessInfo, SortColumn, SortDirection, SortState};

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
