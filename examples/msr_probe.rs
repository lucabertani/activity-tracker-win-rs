// Probe: per-core thermal status bits, temperature and effective clock via
// the PawnIO driver. Requires PawnIO installed once and an elevated terminal
// (see driver/README.md). Compare the output with HWiNFO's "Core Thermal
// Throttling", "Package/Ring Thermal Throttling" and "IA Limit Reasons".
use std::time::Duration;

use activity_tracker_win_rs::monitor::msr::{MsrThermalMonitor, OpenError};

fn bit(value: Option<u64>, index: u32) -> char {
    match value {
        Some(v) if v & (1 << index) != 0 => '1',
        Some(_) => '.',
        None => '?',
    }
}

fn main() {
    let mut monitor = match MsrThermalMonitor::open() {
        Ok(monitor) => monitor,
        Err(OpenError::AccessDenied) => {
            eprintln!(
                "access denied: the PawnIO device is admin-only, run this probe as administrator"
            );
            return;
        }
        Err(OpenError::Missing) => {
            eprintln!(
                "PawnIO not available; install it from https://pawnio.eu first (see driver/README.md)"
            );
            return;
        }
    };

    let slots = monitor.cores().to_vec();
    println!(
        "{} physical cores (logical index / efficiency class):",
        slots.len()
    );
    // Bits: T = thermal now (0), t = thermal log (1), P = PROCHOT now (2),
    // L = power limit now (10).
    for round in 0..10 {
        let sample = monitor.sample(true);
        println!(
            "--- sample {round}: reasons=[{}] avg={:?} C pkg={:?} C avg_clock={:?} MHz",
            sample.reasons.describe(),
            sample.average_celsius().map(|t| t.round()),
            sample.package_temp_c,
            sample.average_mhz(),
        );
        println!(
            "pkg  bits T{} t{} P{} L{}",
            bit(sample.package_status, 0),
            bit(sample.package_status, 1),
            bit(sample.package_status, 2),
            bit(sample.package_status, 10),
        );
        for (slot, core) in slots.iter().zip(&sample.cores) {
            println!(
                "core lp{:2} ec{} bits T{} t{} P{} L{}  {:>4} C  {:>5} MHz",
                slot.logical,
                slot.efficiency_class,
                bit(core.therm_status, 0),
                bit(core.therm_status, 1),
                bit(core.therm_status, 2),
                bit(core.therm_status, 10),
                core.temp_c.map_or("-".to_string(), |t| t.to_string()),
                core.effective_mhz
                    .map_or("-".to_string(), |mhz| mhz.to_string()),
            );
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
}
