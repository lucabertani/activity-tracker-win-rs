// Probe: per-core temperature + thermal throttling via the PawnIO driver.
// Requires PawnIO installed once as administrator (see driver/README.md).
use std::time::Duration;

use activity_tracker_win_rs::monitor::msr::MsrThermalMonitor;

fn main() {
    let Some(monitor) = MsrThermalMonitor::open() else {
        eprintln!(
            "PawnIO not available; install it from https://pawnio.eu first (see driver/README.md)"
        );
        return;
    };

    for sample in 0..6 {
        std::thread::sleep(Duration::from_millis(500));
        let s = monitor.refresh();
        println!(
            "--- sample {sample}: throttling={} avg={:?} C ---",
            s.throttling,
            s.average_celsius()
        );
        for (core, temp) in s.temps_c.iter().enumerate() {
            println!("core {core:2}: {temp:?} C");
        }
    }
}
