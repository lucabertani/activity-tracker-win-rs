// Probe: per-core thermal status bits, temperature and effective clock via
// the PawnIO driver. Requires PawnIO installed once and an elevated terminal
// (see driver/README.md). Compare the output with HWiNFO's "Core Thermal
// Throttling", "Package/Ring Thermal Throttling" and "IA Limit Reasons".
//
// Usage: msr_probe [seconds] (default 10). Samples every 500 ms like the
// app, prints the full state at start, then one line whenever a status bit
// changes, and a per-core count of the samples with each bit at the end.
use std::time::{Duration, Instant};

use tracker_platform::monitor::msr::{MsrThermalMonitor, MsrThermalSample, OpenError};

/// Bits reported: T = thermal now (0), P = PROCHOT now (2), L = power
/// limit now (10). The log bits are sticky and cannot be cleared, so they
/// say nothing about the present.
const BITS: [(u32, char); 3] = [(0, 'T'), (2, 'P'), (10, 'L')];

fn flags(value: Option<u64>) -> String {
    BITS.iter()
        .map(|&(index, name)| match value {
            Some(v) if v & (1 << index) != 0 => name,
            Some(_) => '.',
            None => '?',
        })
        .collect()
}

/// Every register's flags, package first: the key that detects changes.
fn state(sample: &MsrThermalSample) -> Vec<String> {
    std::iter::once(flags(sample.package_status))
        .chain(sample.cores.iter().map(|core| flags(core.therm_status)))
        .collect()
}

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(10);

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
        "{} physical cores; sampling every 500 ms for {seconds} s. Flags: T thermal, P PROCHOT, L power limit",
        slots.len()
    );

    let start = Instant::now();
    let mut previous: Option<Vec<String>> = None;
    let mut samples = 0u32;
    // [register][bit] = samples with that bit set; register 0 = package.
    let mut counts = vec![[0u32; BITS.len()]; slots.len() + 1];

    while start.elapsed() < Duration::from_secs(seconds) {
        let sample = monitor.sample(samples.is_multiple_of(4));
        samples += 1;
        let registers = std::iter::once(sample.package_status)
            .chain(sample.cores.iter().map(|c| c.therm_status));
        for (register, value) in registers.enumerate() {
            for (bit, &(index, _)) in BITS.iter().enumerate() {
                if value.is_some_and(|v| v & (1 << index) != 0) {
                    counts[register][bit] += 1;
                }
            }
        }

        let current = state(&sample);
        if previous.as_ref() != Some(&current) {
            let at = start.elapsed().as_secs_f32();
            println!(
                "[{at:7.1} s] reasons=[{}] avg={:?} C pkg={:?} C clock={:?} MHz",
                sample.reasons.describe(),
                sample.average_celsius().map(|t| t.round()),
                sample.package_temp_c,
                sample.average_mhz(),
            );
            // The thermal bits fire at TjMax - TCC offset, not at TjMax.
            if let Some(target) = monitor.temperature_target() {
                let tjmax = (target >> 16) & 0xFF;
                let offset = (target >> 24) & 0x3F;
                println!(
                    "    TjMax {tjmax} C, TCC offset {offset} C -> thermal at {} C  raw {target:#010x}",
                    tjmax.saturating_sub(offset)
                );
            }
            println!(
                "    pkg       {}  raw {:#010x}",
                current[0],
                sample.package_status.unwrap_or(0)
            );
            for ((slot, core), flags) in slots.iter().zip(&sample.cores).zip(&current[1..]) {
                println!(
                    "    lp{:2} ec{}  {}  raw {:#010x}  {:>4} C",
                    slot.logical,
                    slot.efficiency_class,
                    flags,
                    core.therm_status.unwrap_or(0),
                    core.temp_c.map_or("-".to_string(), |t| t.to_string()),
                );
            }
            previous = Some(current);
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    println!("\nSamples with each bit set, out of {samples}:");
    println!("              T     P     L");
    let names = std::iter::once("pkg      ".to_string()).chain(
        slots
            .iter()
            .map(|slot| format!("lp{:2} ec{} ", slot.logical, slot.efficiency_class)),
    );
    for (name, count) in names.zip(&counts) {
        println!("    {name} {:5} {:5} {:5}", count[0], count[1], count[2]);
    }
}
