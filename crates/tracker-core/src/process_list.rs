//! GUI-independent state of the process list: the latest snapshot plus the
//! user's search, sort and expanded groups, and the rows derived from them.
//! Every frontend renders [`ProcessList::rows`] and forwards user actions
//! here, so the behavior is identical whatever the GUI.

use std::collections::HashSet;

use crate::model::{DisplayRow, ProcessInfo, SortColumn, SortState};
use crate::rows::build_display_rows;

/// Stable identity of a row across refreshes (indices shift whenever a
/// process starts or exits): lets a GUI keep the selection on the same item.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    /// Lower-cased process name.
    Group(String),
    Proc(u32),
    System,
}

/// Why a row cannot be terminated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminateError {
    InvalidRow,
    SystemRow,
    OnlySelf,
}

impl std::fmt::Display for TerminateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TerminateError::InvalidRow => "Invalid process.",
            TerminateError::SystemRow => "That row is a system total, not a process.",
            TerminateError::OnlySelf => "You cannot terminate the current application from here.",
        })
    }
}

#[derive(Debug, Default)]
pub struct ProcessList {
    processes: Vec<ProcessInfo>,
    total_ram_bytes: u64,
    /// System-wide resident memory (kernel, drivers, shared pages included).
    ram_in_use_bytes: u64,
    search: String,
    sort: SortState,
    /// Lower-cased process names whose group the user has expanded.
    expanded: HashSet<String>,
    rows: Vec<DisplayRow>,
}

impl ProcessList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_snapshot(
        &mut self,
        processes: Vec<ProcessInfo>,
        total_ram_bytes: u64,
        ram_in_use_bytes: u64,
    ) {
        self.processes = processes;
        self.total_ram_bytes = total_ram_bytes;
        self.ram_in_use_bytes = ram_in_use_bytes;
        self.rebuild();
    }

    pub fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }

    pub fn processes(&self) -> &[ProcessInfo] {
        &self.processes
    }

    pub fn total_ram_bytes(&self) -> u64 {
        self.total_ram_bytes
    }

    pub fn ram_in_use_bytes(&self) -> u64 {
        self.ram_in_use_bytes
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn set_search(&mut self, query: &str) {
        if self.search != query {
            self.search = query.to_string();
            self.rebuild();
        }
    }

    pub fn sort(&self) -> SortState {
        self.sort
    }

    pub fn set_sort(&mut self, sort: SortState) {
        self.sort = sort;
        self.rebuild();
    }

    /// Same column flips the direction, a new column starts ascending.
    pub fn toggle_sort(&mut self, column: SortColumn) {
        self.sort.toggle_for(column);
        self.rebuild();
    }

    /// Expands or collapses the group at `row`. Returns false when the row
    /// is not a group.
    pub fn toggle_group(&mut self, row: usize) -> bool {
        let Some(DisplayRow::Group { name, .. }) = self.rows.get(row) else {
            return false;
        };
        let key = name.to_ascii_lowercase();
        // remove returns true if the group was expanded: collapse it.
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        self.rebuild();
        true
    }

    pub fn row_key(&self, row: usize) -> Option<RowKey> {
        self.rows.get(row).map(|display| match display {
            DisplayRow::Group { name, .. } => RowKey::Group(name.to_ascii_lowercase()),
            DisplayRow::Proc(process) => RowKey::Proc(process.pid),
            DisplayRow::System { .. } => RowKey::System,
        })
    }

    pub fn find_row(&self, key: &RowKey) -> Option<usize> {
        (0..self.rows.len()).find(|&row| self.row_key(row).as_ref() == Some(key))
    }

    /// PIDs to terminate for `row`: a group row ends every instance of that
    /// process (like Task Manager's "End task group"), a child row only
    /// itself. `own_pid` (this app) is never included.
    pub fn pids_to_terminate(&self, row: usize, own_pid: u32) -> Result<Vec<u32>, TerminateError> {
        let pids: Vec<u32> = match self.rows.get(row) {
            None => return Err(TerminateError::InvalidRow),
            Some(DisplayRow::System { .. }) => return Err(TerminateError::SystemRow),
            Some(DisplayRow::Proc(process)) => vec![process.pid],
            Some(DisplayRow::Group { name, .. }) => {
                let key = name.to_ascii_lowercase();
                self.processes
                    .iter()
                    .filter(|p| p.name.to_ascii_lowercase() == key)
                    .map(|p| p.pid)
                    .collect()
            }
        };
        let pids: Vec<u32> = pids.into_iter().filter(|pid| *pid != own_pid).collect();
        if pids.is_empty() {
            return Err(TerminateError::OnlySelf);
        }
        Ok(pids)
    }

    /// Share of the physical RAM, in percent.
    pub fn percent_of_ram(&self, bytes: u64) -> f64 {
        if self.total_ram_bytes == 0 {
            0.0
        } else {
            bytes as f64 / self.total_ram_bytes as f64 * 100.0
        }
    }

    fn rebuild(&mut self) {
        self.rows = build_display_rows(
            &self.processes,
            &self.search,
            self.sort,
            &self.expanded,
            self.ram_in_use_bytes,
        );
    }
}

/// Bytes of one row (group total, process, or system remainder).
pub fn row_bytes(row: &DisplayRow) -> u64 {
    match row {
        DisplayRow::Group { total_bytes, .. } => *total_bytes,
        DisplayRow::Proc(process) => process.memory_bytes,
        DisplayRow::System { bytes } => *bytes,
    }
}

pub fn mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SortDirection;

    fn p(pid: u32, name: &str, memory_bytes: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            name: name.to_string(),
            memory_bytes,
            memory_mib: mib(memory_bytes),
            memory_percent: 0.0,
        }
    }

    fn list() -> ProcessList {
        let mut list = ProcessList::new();
        list.set_snapshot(
            vec![
                p(1, "firefox.exe", 100),
                p(2, "firefox.exe", 200),
                p(3, "notepad.exe", 50),
                p(99, "tracker.exe", 10),
            ],
            1000,
            500,
        );
        list
    }

    #[test]
    fn default_sort_is_memory_descending() {
        let list = list();
        assert_eq!(list.row_key(0), Some(RowKey::Group("firefox.exe".into())));
        assert_eq!(list.row_key(3), Some(RowKey::System));
    }

    #[test]
    fn toggling_a_group_expands_and_collapses_it() {
        let mut list = list();
        assert!(list.toggle_group(0));
        assert_eq!(list.rows().len(), 6); // 3 groups + 2 children + system
        assert_eq!(list.row_key(1), Some(RowKey::Proc(2)));
        assert!(list.toggle_group(0));
        assert_eq!(list.rows().len(), 4);
        // Not a group row.
        assert!(!list.toggle_group(3));
    }

    #[test]
    fn expanded_groups_survive_a_refresh() {
        let mut list = list();
        list.toggle_group(0);
        list.set_snapshot(
            vec![p(1, "Firefox.exe", 100), p(5, "firefox.exe", 1)],
            1000,
            0,
        );
        assert_eq!(list.rows().len(), 3);
        assert!(matches!(
            list.rows()[0],
            DisplayRow::Group { expanded: true, .. }
        ));
    }

    #[test]
    fn rows_are_found_again_by_key_after_resorting() {
        let mut list = list();
        let key = list.row_key(1).unwrap(); // notepad (50) after firefox (300)
        list.set_sort(SortState {
            sort_column: SortColumn::Name,
            sort_direction: SortDirection::Asc,
        });
        assert_eq!(list.find_row(&key), Some(1)); // firefox, notepad, tracker
        list.toggle_sort(SortColumn::Name); // now descending
        assert_eq!(list.find_row(&key), Some(1)); // tracker, notepad, firefox
        assert_eq!(list.row_key(0), Some(RowKey::Group("tracker.exe".into())));
    }

    #[test]
    fn search_filters_groups() {
        let mut list = list();
        list.set_search("NOTE");
        assert_eq!(list.row_key(0), Some(RowKey::Group("notepad.exe".into())));
        assert_eq!(list.rows().len(), 2); // notepad + system
    }

    #[test]
    fn terminate_targets() {
        let list = list();
        assert_eq!(list.pids_to_terminate(0, 99), Ok(vec![1, 2]));
        assert_eq!(list.pids_to_terminate(2, 99), Err(TerminateError::OnlySelf));
        assert_eq!(
            list.pids_to_terminate(3, 99),
            Err(TerminateError::SystemRow)
        );
        assert_eq!(
            list.pids_to_terminate(42, 99),
            Err(TerminateError::InvalidRow)
        );
    }

    #[test]
    fn percent_handles_unknown_total() {
        let mut list = ProcessList::new();
        assert_eq!(list.percent_of_ram(10), 0.0);
        list.set_snapshot(Vec::new(), 200, 0);
        assert_eq!(list.percent_of_ram(50), 25.0);
    }
}
