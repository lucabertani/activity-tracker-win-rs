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
/// a single process, shown as a child row of its group, or the closing
/// summary row with the memory owned by no process at all.
#[derive(Debug, Clone)]
pub enum DisplayRow {
    Group {
        name: String,
        total_bytes: u64,
        count: u32,
        expanded: bool,
    },
    Proc(ProcessInfo),
    /// System-wide resident memory that belongs to no process: kernel,
    /// drivers and the pages shared between processes. Not a real entry:
    /// it exists so the listed rows reconcile with the header total.
    System {
        bytes: u64,
    },
}

/// Why the CPU is being slowed down, as reported by the per-core and
/// package thermal status MSRs (the same flags HWiNFO shows).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThrottleReasons {
    /// A core reached its thermal trip point (`IA32_THERM_STATUS` bit 0).
    pub core_thermal: bool,
    /// The package reached its thermal trip point
    /// (`IA32_PACKAGE_THERM_STATUS` bit 0).
    pub package_thermal: bool,
    /// PROCHOT# asserted, usually by the embedded controller or the VRM on
    /// laptops (bit 2 of either MSR).
    pub prochot: bool,
    /// Running at a power limit (bit 10). Normal under sustained load, so it
    /// is reported but does not count as thermal throttling.
    pub power_limit: bool,
}

impl ThrottleReasons {
    /// Thermal throttling in the strict sense: what lights up the red
    /// indicator.
    pub fn thermal(&self) -> bool {
        self.core_thermal || self.package_thermal || self.prochot
    }

    pub fn any(&self) -> bool {
        self.thermal() || self.power_limit
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            core_thermal: self.core_thermal || other.core_thermal,
            package_thermal: self.package_thermal || other.package_thermal,
            prochot: self.prochot || other.prochot,
            power_limit: self.power_limit || other.power_limit,
        }
    }

    /// Short human-readable list, e.g. "core thermal, PROCHOT".
    pub fn describe(&self) -> String {
        let labels = [
            (self.core_thermal, "core thermal"),
            (self.package_thermal, "package thermal"),
            (self.prochot, "PROCHOT"),
            (self.power_limit, "power limit"),
        ];
        labels
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, label)| *label)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleState {
    /// No reliable source: the PawnIO driver is missing, the app is not
    /// elevated, or every MSR read failed.
    Unknown,
    Ok,
    Throttling,
}

/// CPU thermal snapshot handed to the UI on every refresh tick.
#[derive(Debug, Clone, Default)]
pub struct ThermalStatus {
    /// The MSR path delivered at least one valid reading.
    pub available: bool,
    /// Reasons seen since the previous tick (latched by the background
    /// sampler, held for a few seconds so short bursts stay visible).
    pub reasons: ThrottleReasons,
    /// Average temperature over the physical cores.
    pub avg_temp_c: Option<f64>,
    pub package_temp_c: Option<u16>,
    /// Effective clock (APERF/MPERF) averaged over the physical cores.
    pub avg_effective_mhz: Option<u32>,
    pub max_effective_mhz: Option<u32>,
    /// Share of the samples of the last minute with thermal throttling.
    pub throttled_share_last_minute: Option<f32>,
}

impl ThermalStatus {
    pub fn state(&self) -> ThrottleState {
        if !self.available {
            ThrottleState::Unknown
        } else if self.reasons.thermal() {
            ThrottleState::Throttling
        } else {
            ThrottleState::Ok
        }
    }

    pub fn is_throttling(&self) -> bool {
        self.state() == ThrottleState::Throttling
    }

    /// One line for tooltips, e.g. "Throttling: PROCHOT (12% of last min)".
    pub fn summary(&self) -> String {
        if !self.available {
            return "Throttling: n/a (needs PawnIO + elevation)".to_string();
        }
        let mut text = if self.reasons.any() {
            format!("Throttling: {}", self.reasons.describe())
        } else {
            "No throttling".to_string()
        };
        if let Some(share) = self.throttled_share_last_minute
            && share > 0.0
        {
            text.push_str(&format!(" ({:.0}% of last min)", share * 100.0));
        }
        text
    }
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
