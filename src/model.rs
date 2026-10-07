use serde::{Deserialize, Serialize};

use crate::ui::tray::{TRAY_BG_COLOR, TRAY_TEXT_COLOR};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_bytes: u64,
    pub memory_mib: f64,
    pub memory_percent: f64,
}

/// One row of the process list: either a group header for one process
/// name (totals over all its instances, with the expand/collapse state),
/// or a single process, shown as a child row of its group.
#[derive(Debug, Clone)]
pub enum DisplayRow {
    Group {
        name: String,
        total_bytes: u64,
        count: u32,
        expanded: bool,
    },
    Proc(ProcessInfo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy)]
pub struct SortState {
    pub sort_column: SortColumn,
    pub sort_direction: SortDirection,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            sort_column: SortColumn::Memory,
            sort_direction: SortDirection::Desc,
        }
    }
}

impl SortState {
    pub fn toggle_for(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_direction = match self.sort_direction {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            };
        } else {
            self.sort_column = column;
            self.sort_direction = SortDirection::Asc;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub start_on_windows: bool,
    pub start_in_background: bool,
    pub refresh_interval_seconds: u64,
    pub window_start_hidden: bool,
    pub show_tray_tooltip: bool,
    /// Turn the tray icons red while the CPU is thermal-throttling.
    #[serde(default)]
    pub tray_icon_thermal_red: bool,
    /// Show the RAM percentage icon in the tray.
    #[serde(default = "default_true")]
    pub tray_icon_ram_enabled: bool,
    /// Show the CPU average-temperature icon in the tray.
    #[serde(default = "default_true")]
    pub tray_icon_cpu_enabled: bool,
    /// COLORREF (0x00BBGGRR) background of the RAM tray icon.
    #[serde(default = "default_ram_bg")]
    pub tray_icon_ram_bg: u32,
    /// COLORREF (0x00BBGGRR) text color of the RAM tray icon.
    #[serde(default = "default_ram_fg")]
    pub tray_icon_ram_fg: u32,
    /// COLORREF (0x00BBGGRR) background of the CPU tray icon.
    #[serde(default = "default_cpu_bg")]
    pub tray_icon_cpu_bg: u32,
    /// COLORREF (0x00BBGGRR) text color of the CPU tray icon.
    #[serde(default = "default_cpu_fg")]
    pub tray_icon_cpu_fg: u32,
    /// Font height in pixels for the tray icons' text. 0 = automatic
    /// (the text is sized to fit the icon).
    #[serde(default)]
    pub tray_icon_font_size: u32,
    /// Font weight for the tray icons' text (Segoe UI scale).
    #[serde(default = "default_font_weight")]
    pub tray_icon_font_weight: u32,
}

fn default_true() -> bool {
    true
}

// Default tray icon colors: dark gray background, white text. The CPU icon
// starts with the same look; both are customizable in the settings window.
pub const DEFAULT_TRAY_RAM_BG: u32 = TRAY_BG_COLOR;
pub const DEFAULT_TRAY_RAM_FG: u32 = TRAY_TEXT_COLOR;
pub const DEFAULT_TRAY_CPU_BG: u32 = TRAY_BG_COLOR;
pub const DEFAULT_TRAY_CPU_FG: u32 = TRAY_TEXT_COLOR;

/// Tray icon text font size: 0 = automatic (fit to the icon).
pub const DEFAULT_TRAY_ICON_FONT_SIZE: u32 = 0;
/// Tray icon text font weight (Segoe UI scale: 300 light, 400 regular,
/// 550 heavy). 350 is slightly lighter than regular.
pub const DEFAULT_TRAY_FONT_WEIGHT: u32 = 350;

fn default_ram_bg() -> u32 {
    DEFAULT_TRAY_RAM_BG
}
fn default_ram_fg() -> u32 {
    DEFAULT_TRAY_RAM_FG
}
fn default_cpu_bg() -> u32 {
    DEFAULT_TRAY_CPU_BG
}
fn default_cpu_fg() -> u32 {
    DEFAULT_TRAY_CPU_FG
}
fn default_font_weight() -> u32 {
    DEFAULT_TRAY_FONT_WEIGHT
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            start_on_windows: false,
            start_in_background: false,
            refresh_interval_seconds: 3,
            window_start_hidden: false,
            show_tray_tooltip: true,
            tray_icon_thermal_red: false,
            tray_icon_ram_enabled: true,
            tray_icon_cpu_enabled: true,
            tray_icon_ram_bg: DEFAULT_TRAY_RAM_BG,
            tray_icon_ram_fg: DEFAULT_TRAY_RAM_FG,
            tray_icon_cpu_bg: DEFAULT_TRAY_CPU_BG,
            tray_icon_cpu_fg: DEFAULT_TRAY_CPU_FG,
            tray_icon_font_size: DEFAULT_TRAY_ICON_FONT_SIZE,
            tray_icon_font_weight: DEFAULT_TRAY_FONT_WEIGHT,
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.refresh_interval_seconds = self.refresh_interval_seconds.clamp(1, 60);
        self.tray_icon_font_size = self.tray_icon_font_size.clamp(0, 32);
        self
    }
}

pub fn parse_refresh_interval_seconds(input: &str, fallback: u64) -> u64 {
    match input.trim().parse::<u64>() {
        Ok(value) => value.clamp(1, 60),
        Err(_) => fallback.clamp(1, 60),
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, parse_refresh_interval_seconds};

    #[test]
    fn normalize_interval_bounds() {
        let low = AppSettings {
            refresh_interval_seconds: 0,
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(low.refresh_interval_seconds, 1);

        let high = AppSettings {
            refresh_interval_seconds: 999,
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(high.refresh_interval_seconds, 60);
    }

    #[test]
    fn parse_interval_input() {
        assert_eq!(parse_refresh_interval_seconds("5", 3), 5);
        assert_eq!(parse_refresh_interval_seconds("0", 3), 1);
        assert_eq!(parse_refresh_interval_seconds("999", 3), 60);
        assert_eq!(parse_refresh_interval_seconds("abc", 3), 3);
    }

    #[test]
    fn old_settings_json_without_new_field_still_loads() {
        // Settings saved by a previous version of the app have no
        // tray_icon_thermal_red field: it must default to false.
        let json = r#"{"start_on_windows":true,"start_in_background":true,"refresh_interval_seconds":5,"window_start_hidden":false,"show_tray_tooltip":true}"#;
        let settings: AppSettings = serde_json::from_str(json).unwrap();
        assert!(!settings.tray_icon_thermal_red);
        assert_eq!(settings.refresh_interval_seconds, 5);
    }
}
