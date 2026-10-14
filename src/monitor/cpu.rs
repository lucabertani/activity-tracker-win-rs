use sysinfo::{CpuRefreshKind, RefreshKind, System};
use winreg::RegKey;
use winreg::enums::HKEY_LOCAL_MACHINE;

use super::msr::MsrThermalMonitor;

/// A core counts as "under load" when its usage reaches this percentage.
const USAGE_THRESHOLD_PERCENT: f32 = 60.0;
/// A core is considered throttled when its current clock drops to this
/// fraction (or below) of its reference clock.
const FREQ_RATIO_THRESHOLD: f64 = 0.70;
/// When seeding the per-core reference at startup, a core running at (or
/// above) this fraction of the OS-rated frequency is trusted to be a
/// high-clock core (e.g. a P-core), so its reference is seeded from the
/// rated frequency instead of the (possibly already throttled) current one.
const RATED_RATIO_TO_TRUST: f64 = 0.70;
/// Consecutive refresh ticks with a throttled core required before the
/// warning lights up (frequency heuristic only; the MSR flag is immediate). Avoids one-tick flicker when a load burst ends and
/// usage lingers while the clock drops.
const MIN_CONSECUTIVE_TICKS: u32 = 2;

/// Monitors CPU cores for thermal throttling.
///
/// Primary signal (HWiNFO-style, when the PawnIO kernel driver is
/// installed): per-core MSR `IA32_THERM_STATUS` bit 0 — the exact flag
/// HWiNFO reads. Falls back to a frequency+load heuristic when the driver
/// is not available (no admin rights / not installed).
///
/// The check runs on the same refresh timer as the RAM percentage. As soon
/// as a single core has the hardware flag set, the whole CPU is reported as
/// throttling (the heuristic fallback needs two consecutive ticks).
///
/// When the driver path is active, `refresh()` also captures the average
/// per-core temperature (bits 22:16 of the same MSR converted with TjMax from
/// `IA32_TEMPERATURE_TARGET`).
pub struct CpuThrottleMonitor {
    system: System,
    core_max_mhz: Vec<u64>,
    consecutive_throttle_ticks: u32,
    msr: Option<MsrThermalMonitor>,
    last_avg_temp_c: Option<f64>,
}

impl Default for CpuThrottleMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuThrottleMonitor {
    pub fn new() -> Self {
        Self {
            // We only need CPU usage + frequency; no process/memory data.
            system: System::new_with_specifics(
                RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
            ),
            core_max_mhz: Vec::new(),
            consecutive_throttle_ticks: 0,
            msr: MsrThermalMonitor::open().ok(),
            last_avg_temp_c: None,
        }
    }

    /// Access to the underlying `sysinfo` system (per-core usage/frequency).
    pub fn system(&self) -> &System {
        &self.system
    }

    /// Refreshes per-core usage and frequency, and returns `true` when at
    /// least one core appears to be thermal-throttling.
    /// Also updates the average core temperature (driver path only).
    pub fn refresh(&mut self) -> bool {
        // HWiNFO-style MSR path: authoritative when the driver is available.
        // Re-probe on every refresh so a driver installed while the app is
        // running gets picked up (opening a missing device is cheap).
        if self.msr.is_none() {
            self.msr = MsrThermalMonitor::open().ok();
        }
        if let Some(msr) = &self.msr {
            // The hardware flag is authoritative: report as soon as any single
            // core has it set, no debounce.
            let sample = msr.refresh();
            self.last_avg_temp_c = sample.average_celsius();
            return sample.throttling;
        }
        self.last_avg_temp_c = None;

        // The heuristic is noisy, so it needs consecutive ticks.
        if self.refresh_frequency_heuristic() {
            self.consecutive_throttle_ticks += 1;
        } else {
            self.consecutive_throttle_ticks = 0;
        }
        self.consecutive_throttle_ticks >= MIN_CONSECUTIVE_TICKS
    }

    /// Average core temperature in °C from the last `refresh()` call.
    /// `None` when the PawnIO driver is not available (the frequency
    /// heuristic has no temperature source).
    pub fn avg_temperature_celsius(&self) -> Option<f64> {
        self.last_avg_temp_c
    }

    /// Frequency+load fallback used when the kernel driver is not installed.
    fn refresh_frequency_heuristic(&mut self) -> bool {
        self.system.refresh_cpu_all();

        let cpus = self.system.cpus();
        // First refresh: seed the per-core references. Usage is 0 on the
        // first sample anyway, so nothing can be flagged yet.
        if self.core_max_mhz.len() != cpus.len() {
            let rated = rated_frequency_mhz();
            self.core_max_mhz = cpus
                .iter()
                .map(|cpu| seed_reference(cpu.frequency(), rated))
                .collect();
            return false;
        }

        let mut throttling = false;
        for (cpu, core_max) in cpus.iter().zip(self.core_max_mhz.iter_mut()) {
            let frequency = cpu.frequency();
            if frequency > 0 && frequency > *core_max {
                *core_max = frequency;
            }
            if is_core_throttled(cpu.cpu_usage(), frequency, *core_max) {
                throttling = true;
            }
        }
        throttling
    }
}

/// Startup reference for one core: trust the OS-rated frequency when the
/// core is already running near it (it is a high-clock core and the current
/// clock may already be throttled); otherwise use the current clock.
fn seed_reference(current_mhz: u64, rated_mhz: u64) -> u64 {
    if rated_mhz > 0 && current_mhz as f64 >= rated_mhz as f64 * RATED_RATIO_TO_TRUST {
        current_mhz.max(rated_mhz)
    } else {
        current_mhz
    }
}

/// Rated CPU frequency in MHz, from the value the OS writes at boot
/// (`HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\<n>\~MHz`).
fn rated_frequency_mhz() -> u64 {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for index in 0..64u32 {
        let path = format!(r"HARDWARE\DESCRIPTION\System\CentralProcessor\{index}");
        if let Ok(key) = hklm.open_subkey(&path)
            && let Ok(value) = key.get_value::<u32, _>("~MHz")
        {
            return u64::from(value);
        }
    }
    0
}

/// Pure decision rule, kept separate so it can be unit-tested.
fn is_core_throttled(usage_percent: f32, frequency_mhz: u64, reference_mhz: u64) -> bool {
    if frequency_mhz == 0 || reference_mhz == 0 || usage_percent < USAGE_THRESHOLD_PERCENT {
        return false;
    }
    (frequency_mhz as f64) <= (reference_mhz as f64) * FREQ_RATIO_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::{is_core_throttled, seed_reference};

    #[test]
    fn idle_core_is_never_throttled_even_at_low_clock() {
        // Low clock + low usage = normal power management, not throttling.
        assert!(!is_core_throttled(5.0, 800, 4800));
    }

    #[test]
    fn loaded_core_at_normal_clock_is_not_throttled() {
        assert!(!is_core_throttled(100.0, 4600, 4800));
        assert!(!is_core_throttled(100.0, 3400, 3800));
        // Capped-to-base clocks on a power-limited machine stay above the
        // throttle ratio (2300/3072 = 0.75, 1800/1800 = 1.0).
        assert!(!is_core_throttled(100.0, 2300, 3072));
        assert!(!is_core_throttled(100.0, 1800, 1800));
    }

    #[test]
    fn loaded_core_with_capped_clock_is_throttled() {
        // P-core throttled to ~40% of its usual max.
        assert!(is_core_throttled(100.0, 2000, 4800));
        // E-core throttled to ~50% of its usual max.
        assert!(is_core_throttled(80.0, 1900, 3800));
        // P-core dropping below its rated clock while loaded (2300 -> 1500).
        assert!(is_core_throttled(100.0, 1500, 2300));
    }

    #[test]
    fn invalid_data_is_never_throttled() {
        assert!(!is_core_throttled(100.0, 0, 4800));
        assert!(!is_core_throttled(100.0, 2000, 0));
    }

    #[test]
    fn startup_seed_trusts_rated_frequency_for_high_clock_cores() {
        // P-core already at 75% of rated: seed from rated (3072), not from
        // the possibly-throttled current clock.
        assert_eq!(seed_reference(2300, 3072), 3072);
        // E-core running at 59% of rated: seed from its own current clock,
        // otherwise a normal E-core base clock would read as throttled.
        assert_eq!(seed_reference(1800, 3072), 1800);
        // Low-idle core: seed from its own clock.
        assert_eq!(seed_reference(800, 3072), 800);
        // No rated value available: seed from the current clock.
        assert_eq!(seed_reference(2000, 0), 2000);
    }
}
