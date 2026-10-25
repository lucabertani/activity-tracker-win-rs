//! Background thread sampling the thermal MSRs much faster than the UI
//! refresh. The status bits are instantaneous and the sticky log bits cannot
//! be cleared through PawnIO, so a single read per UI tick (3 s by default)
//! would miss most throttling bursts. Here every reason seen between two
//! ticks is latched, and kept visible for [`HOLD`] so the indicator does not
//! flicker.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::msr::{MsrThermalMonitor, MsrThermalSample};
use crate::model::{ThermalStatus, ThrottleReasons};

/// Status sampling period. Each sample pins the thread to every physical
/// core in turn, so this also bounds how often idle cores are woken up.
const SAMPLE_PERIOD: Duration = Duration::from_millis(500);
/// Every Nth sample also reads APERF/MPERF (effective clock over ~2 s).
const FREQUENCY_EVERY: u32 = 4;
/// A thermal reason stays reported for this long after it was last seen.
const HOLD: Duration = Duration::from_secs(5);
/// Window of the "share of time throttled" statistic.
const HISTORY: Duration = Duration::from_secs(60);

/// Owns the sampling thread; stops and joins it on drop.
pub struct ThermalSampler {
    shared: Arc<Mutex<Accumulator>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ThermalSampler {
    pub fn start(mut monitor: MsrThermalMonitor) -> Self {
        let shared = Arc::new(Mutex::new(Accumulator::default()));
        let stop = Arc::new(AtomicBool::new(false));

        let thread = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("thermal-sampler".into())
                .spawn(move || {
                    let mut tick = 0u32;
                    while !stop.load(Ordering::Relaxed) {
                        let sample = monitor.sample(tick.is_multiple_of(FREQUENCY_EVERY));
                        tick = tick.wrapping_add(1);
                        if let Ok(mut acc) = shared.lock() {
                            acc.record(&sample, Instant::now());
                        }
                        std::thread::park_timeout(SAMPLE_PERIOD);
                    }
                })
                .ok()
        };

        Self {
            shared,
            stop,
            thread,
        }
    }

    /// Status since the previous call; clears the latched reasons.
    pub fn take(&self) -> ThermalStatus {
        match self.shared.lock() {
            Ok(mut acc) => acc.take(Instant::now()),
            Err(_) => ThermalStatus::default(),
        }
    }
}

impl Drop for ThermalSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// Pure state machine behind the sampler, driven with explicit timestamps
/// so it can be unit-tested.
#[derive(Default)]
struct Accumulator {
    readable: bool,
    /// Union of the reasons seen since the last `take`.
    latched: ThrottleReasons,
    /// Reasons kept visible until `held_until`.
    held: ThrottleReasons,
    held_until: Option<Instant>,
    /// (time, thermal-throttling) per sample, last [`HISTORY`] only.
    history: VecDeque<(Instant, bool)>,
    avg_temp_c: Option<f64>,
    package_temp_c: Option<u16>,
    avg_mhz: Option<u32>,
    max_mhz: Option<u32>,
}

impl Accumulator {
    fn record(&mut self, sample: &MsrThermalSample, now: Instant) {
        self.readable = sample.readable();
        if !self.readable {
            return;
        }

        let reasons = sample.reasons;
        self.latched = self.latched.union(reasons);
        if reasons.any() {
            let still_held = self.held_until.is_some_and(|until| now < until);
            self.held = if still_held {
                self.held.union(reasons)
            } else {
                reasons
            };
            self.held_until = Some(now + HOLD);
        }

        self.history.push_back((now, reasons.thermal()));
        while self
            .history
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > HISTORY)
        {
            self.history.pop_front();
        }

        self.avg_temp_c = sample.average_celsius();
        self.package_temp_c = sample.package_temp_c;
        if let Some(mhz) = sample.average_mhz() {
            self.avg_mhz = Some(mhz);
            self.max_mhz = sample.max_mhz();
        }
    }

    fn take(&mut self, now: Instant) -> ThermalStatus {
        let held = if self.held_until.is_some_and(|until| now < until) {
            self.held
        } else {
            ThrottleReasons::default()
        };
        let reasons = std::mem::take(&mut self.latched).union(held);

        let throttled_share_last_minute = (!self.history.is_empty()).then(|| {
            let hits = self.history.iter().filter(|(_, hit)| *hit).count();
            hits as f32 / self.history.len() as f32
        });

        ThermalStatus {
            available: self.readable,
            reasons,
            avg_temp_c: self.avg_temp_c,
            package_temp_c: self.package_temp_c,
            avg_effective_mhz: self.avg_mhz,
            max_effective_mhz: self.max_mhz,
            throttled_share_last_minute,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitor::msr::CoreSample;

    fn sample(reasons: ThrottleReasons) -> MsrThermalSample {
        MsrThermalSample {
            reasons,
            cores: vec![CoreSample {
                therm_status: Some(0x8000_0000),
                temp_c: Some(60),
                effective_mhz: None,
            }],
            ..Default::default()
        }
    }

    fn thermal() -> ThrottleReasons {
        ThrottleReasons {
            core_thermal: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_burst_between_two_ticks_is_not_lost() {
        let t0 = Instant::now();
        let mut acc = Accumulator::default();
        acc.record(&sample(ThrottleReasons::default()), t0);
        acc.record(&sample(thermal()), t0 + Duration::from_millis(500));
        acc.record(
            &sample(ThrottleReasons::default()),
            t0 + Duration::from_millis(1000),
        );
        // The last sample is clean, but the burst in between is reported.
        assert!(acc.take(t0 + Duration::from_millis(1100)).is_throttling());
    }

    #[test]
    fn the_reason_is_held_then_released() {
        let t0 = Instant::now();
        let mut acc = Accumulator::default();
        acc.record(&sample(thermal()), t0);
        assert!(acc.take(t0).is_throttling());

        acc.record(
            &sample(ThrottleReasons::default()),
            t0 + Duration::from_secs(1),
        );
        assert!(acc.take(t0 + Duration::from_secs(2)).is_throttling());

        acc.record(
            &sample(ThrottleReasons::default()),
            t0 + Duration::from_secs(6),
        );
        assert!(!acc.take(t0 + Duration::from_secs(6)).is_throttling());
    }

    #[test]
    fn unreadable_samples_mean_unknown() {
        let mut acc = Accumulator::default();
        acc.record(&MsrThermalSample::default(), Instant::now());
        let status = acc.take(Instant::now());
        assert!(!status.available);
        assert_eq!(status.state(), crate::model::ThrottleState::Unknown);
    }

    #[test]
    fn share_counts_thermal_samples_of_the_last_minute() {
        let t0 = Instant::now();
        let mut acc = Accumulator::default();
        // An old hit, outside the window by the time of the last sample.
        acc.record(&sample(thermal()), t0);
        for i in 1..=4 {
            let reasons = if i == 4 {
                thermal()
            } else {
                ThrottleReasons::default()
            };
            acc.record(&sample(reasons), t0 + Duration::from_secs(60 + i));
        }
        let status = acc.take(t0 + Duration::from_secs(65));
        assert_eq!(status.throttled_share_last_minute, Some(0.25));
    }

    #[test]
    fn power_limit_alone_is_reported_but_not_throttling() {
        let mut acc = Accumulator::default();
        let reasons = ThrottleReasons {
            power_limit: true,
            ..Default::default()
        };
        acc.record(&sample(reasons), Instant::now());
        let status = acc.take(Instant::now());
        assert!(status.reasons.power_limit);
        assert!(!status.is_throttling());
    }
}
