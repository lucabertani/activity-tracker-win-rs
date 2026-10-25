//! Periodic system sampling on a background thread, so the UI thread never
//! blocks on process enumeration. Frontends receive [`Snapshot`]s through a
//! callback and forward them into their own event loop.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::model::{ProcessInfo, ThermalStatus};
use crate::monitor::cpu::CpuThrottleMonitor;
use crate::monitor::memory::total_memory_status;
use crate::monitor::processes::snapshot_processes;

/// One refresh tick worth of readings.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Load percent as Task Manager shows it.
    pub ram_percent: u32,
    pub total_ram_bytes: u64,
    /// System-wide resident memory (kernel, drivers, shared pages included).
    pub ram_in_use_bytes: u64,
    /// `None` while process sampling is paused (no window to show them).
    pub processes: Option<Vec<ProcessInfo>>,
    pub thermal: ThermalStatus,
}

enum Command {
    SetInterval(Duration),
    IncludeProcesses(bool),
    RefreshNow,
}

/// Owns the sampling thread; stops and joins it on drop.
pub struct Sampler {
    commands: Option<Sender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl Sampler {
    /// Starts sampling right away, then every `interval`.
    pub fn start(
        interval: Duration,
        include_processes: bool,
        mut on_snapshot: impl FnMut(Snapshot) + Send + 'static,
    ) -> Self {
        let (commands, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("sampler".into())
            .spawn(move || {
                let mut cpu = CpuThrottleMonitor::new();
                let mut interval = interval;
                let mut include_processes = include_processes;
                loop {
                    on_snapshot(sample(&mut cpu, include_processes));
                    // Wait for the next tick; commands may arrive meanwhile.
                    loop {
                        match receiver.recv_timeout(interval) {
                            Err(RecvTimeoutError::Timeout) | Ok(Command::RefreshNow) => break,
                            Ok(Command::SetInterval(new)) => interval = new,
                            Ok(Command::IncludeProcesses(on)) => {
                                let resumed = on && !include_processes;
                                include_processes = on;
                                if resumed {
                                    break;
                                }
                            }
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                }
            })
            .ok();
        Self {
            commands: Some(commands),
            thread,
        }
    }

    pub fn set_interval(&self, interval: Duration) {
        self.send(Command::SetInterval(interval));
    }

    /// Process enumeration is the expensive part: pause it while no window
    /// shows the list (the tray only needs RAM and CPU). Resuming samples
    /// immediately.
    pub fn include_processes(&self, include: bool) {
        self.send(Command::IncludeProcesses(include));
    }

    pub fn refresh_now(&self) {
        self.send(Command::RefreshNow);
    }

    fn send(&self, command: Command) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        // Disconnecting the channel ends the thread's wait loop.
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn sample(cpu: &mut CpuThrottleMonitor, include_processes: bool) -> Snapshot {
    let thermal = cpu.refresh().clone();
    let Ok((ram_percent, total, available)) = total_memory_status() else {
        return Snapshot {
            thermal,
            ..Snapshot::default()
        };
    };
    Snapshot {
        ram_percent,
        total_ram_bytes: total,
        ram_in_use_bytes: total.saturating_sub(available),
        processes: include_processes.then(|| snapshot_processes(total)),
        thermal,
    }
}
