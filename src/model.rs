use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_bytes: u64,
    pub memory_mib: f64,
    pub memory_percent: f64,
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
    /// Turn the tray icon red while the CPU is thermal-throttling.
    #[serde(default)]
    pub tray_icon_thermal_red: bool,
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
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.refresh_interval_seconds = self.refresh_interval_seconds.clamp(1, 60);
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
    use super::{parse_refresh_interval_seconds, AppSettings};

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
