use activity_tracker_win_rs::monitor::cpu::CpuThrottleMonitor;
use std::time::Duration;

fn main() {
    let mut monitor = CpuThrottleMonitor::new();
    for i in 0..10 {
        std::thread::sleep(Duration::from_millis(500));
        let throttling = monitor.refresh();
        let sys = monitor.system();
        let max_freq = sys.cpus().iter().map(|c| c.frequency()).max().unwrap_or(0);
        let busy: Vec<_> = sys
            .cpus()
            .iter()
            .filter(|c| c.cpu_usage() >= 50.0)
            .map(|c| (c.frequency(), c.cpu_usage() as u32))
            .collect();
        println!(
            "[{i}] throttling={throttling} max_freq={max_freq} MHz busy_cores={busy:?}"
        );
    }
}
