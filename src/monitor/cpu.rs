use super::msr::MsrThermalMonitor;
use super::thermal_sampler::ThermalSampler;
use crate::model::ThermalStatus;

/// CPU temperature and thermal throttling, HWiNFO-style.
///
/// The only source is the per-core thermal MSRs read through the PawnIO
/// driver (see `msr.rs`), sampled in the background (`thermal_sampler.rs`).
/// Without the driver, or without elevation, the status is
/// [`ThrottleState::Unknown`](crate::model::ThrottleState::Unknown): there is
/// no reliable fallback on Windows. (The previous frequency heuristic read
/// `sysinfo` clocks, which Windows reports as fixed nominal values, so it
/// could never fire.)
pub struct CpuThrottleMonitor {
    sampler: Option<ThermalSampler>,
    last: ThermalStatus,
}

impl Default for CpuThrottleMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuThrottleMonitor {
    pub fn new() -> Self {
        Self {
            sampler: MsrThermalMonitor::open().ok().map(ThermalSampler::start),
            last: ThermalStatus::default(),
        }
    }

    /// Collects the status accumulated since the previous call.
    pub fn refresh(&mut self) -> &ThermalStatus {
        // Re-probe while missing so a driver installed while the app is
        // running gets picked up (opening a missing device is cheap).
        if self.sampler.is_none() {
            self.sampler = MsrThermalMonitor::open().ok().map(ThermalSampler::start);
        }
        self.last = self
            .sampler
            .as_ref()
            .map(ThermalSampler::take)
            .unwrap_or_default();
        &self.last
    }

    /// Status from the last [`refresh`](Self::refresh).
    pub fn status(&self) -> &ThermalStatus {
        &self.last
    }
}
