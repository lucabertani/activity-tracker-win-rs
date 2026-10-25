//! Application state shared by the GPUI views: settings, the latest
//! readings and the process list, plus the background sampler and the tray
//! host feeding it. Lives for the whole app, with or without a window.

use std::time::Duration;

use gpui_kit::base::async_util::{Sender, unbounded};
use gpui_kit::{App, AppContext, Context, Entity};
use tracker_core::model::{AppSettings, ThermalStatus};
use tracker_core::process_list::ProcessList;
use tracker_platform::monitor::processes::terminate_process;
use tracker_platform::sampler::{Sampler, Snapshot};
use tracker_platform::settings::autostart::set_autostart;
use tracker_platform::settings::store::save_settings;
use tracker_platform::tray_host::{TrayEvent, TrayHost};

/// Messages from the background threads to the UI thread.
enum Incoming {
    Snapshot(Snapshot),
    Tray(TrayEvent),
}

/// Emitted to the app shell (window management lives there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEvent {
    OpenWindow,
    Exit,
}

pub struct AppModel {
    pub settings: AppSettings,
    pub processes: ProcessList,
    pub ram_percent: u32,
    pub thermal: ThermalStatus,
    /// A snapshot arrived at least once.
    pub has_data: bool,
    sampler: Option<Sampler>,
    tray: Option<TrayHost>,
}

impl gpui_kit::EventEmitter<ModelEvent> for AppModel {}

impl AppModel {
    /// Starts the sampler and the tray host and pumps their messages into
    /// the model on the UI thread.
    pub fn new(settings: AppSettings, window_open: bool, cx: &mut App) -> Entity<Self> {
        let (tx, rx) = unbounded::<Incoming>();

        let model = cx.new(|_| Self {
            sampler: Some(start_sampler(&settings, window_open, tx.clone())),
            tray: start_tray(tx),
            settings,
            processes: ProcessList::new(),
            ram_percent: 0,
            thermal: ThermalStatus::default(),
            has_data: false,
        });

        let weak = model.downgrade();
        cx.spawn(async move |cx| {
            while let Ok(message) = rx.recv().await {
                if weak
                    .update(cx, |model, cx| model.handle(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        model
    }

    fn handle(&mut self, message: Incoming, cx: &mut Context<Self>) {
        match message {
            Incoming::Snapshot(snapshot) => {
                self.ram_percent = snapshot.ram_percent;
                self.thermal = snapshot.thermal;
                if let Some(processes) = snapshot.processes {
                    self.processes.set_snapshot(
                        processes,
                        snapshot.total_ram_bytes,
                        snapshot.ram_in_use_bytes,
                    );
                }
                self.has_data = true;
                self.update_tray();
                cx.notify();
            }
            Incoming::Tray(TrayEvent::Open) => cx.emit(ModelEvent::OpenWindow),
            Incoming::Tray(TrayEvent::Exit) => cx.emit(ModelEvent::Exit),
        }
    }

    /// Process enumeration only runs while a window shows the list.
    pub fn set_window_open(&mut self, open: bool) {
        if let Some(sampler) = &self.sampler {
            sampler.include_processes(open);
        }
    }

    pub fn refresh_now(&self) {
        if let Some(sampler) = &self.sampler {
            sampler.refresh_now();
        }
    }

    /// Applies and persists new settings. Autostart is (re)registered only
    /// when its flags changed. Returns a message for the user on failure.
    pub fn apply_settings(
        &mut self,
        new: AppSettings,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let new = new.normalized();
        let autostart_changed = new.start_on_windows != self.settings.start_on_windows
            || new.start_in_background != self.settings.start_in_background;
        let interval_changed =
            new.refresh_interval_seconds != self.settings.refresh_interval_seconds;
        self.settings = new;

        if interval_changed && let Some(sampler) = &self.sampler {
            sampler.set_interval(Duration::from_secs(self.settings.refresh_interval_seconds));
        }
        self.update_tray();
        cx.notify();

        let mut result = save_settings(&self.settings)
            .map_err(|err| format!("Could not save the settings: {err}"));
        if autostart_changed && result.is_ok() {
            result = std::env::current_exe()
                .map_err(|err| err.to_string())
                .and_then(|exe| {
                    set_autostart(
                        self.settings.start_on_windows,
                        self.settings.start_in_background,
                        &exe,
                    )
                    .map_err(|err| err.to_string())
                })
                .map_err(|err| format!("Autostart: {err}"));
        }
        result
    }

    /// Terminates the process(es) of `row`. Returns a message for the user
    /// on failure.
    pub fn terminate_row(&mut self, row: usize) -> Result<usize, String> {
        let pids = self
            .processes
            .pids_to_terminate(row, std::process::id())
            .map_err(|err| err.to_string())?;
        let count = pids.len();
        let first_error = pids.into_iter().find_map(|pid| {
            terminate_process(pid)
                .err()
                .map(|err| format!("PID {pid}: {err}"))
        });
        self.refresh_now();
        match first_error {
            Some(err) => Err(format!("Could not terminate some processes: {err}")),
            None => Ok(count),
        }
    }

    /// Stops the background threads (tray icons disappear).
    pub fn shutdown(&mut self) {
        self.tray = None;
        self.sampler = None;
    }

    fn update_tray(&self) {
        if let Some(tray) = &self.tray {
            tray.update(&self.settings, self.ram_percent, &self.thermal);
        }
    }
}

fn start_sampler(settings: &AppSettings, window_open: bool, tx: Sender<Incoming>) -> Sampler {
    Sampler::start(
        Duration::from_secs(settings.refresh_interval_seconds),
        window_open,
        move |snapshot| {
            let _ = tx.send_blocking(Incoming::Snapshot(snapshot));
        },
    )
}

fn start_tray(tx: Sender<Incoming>) -> Option<TrayHost> {
    TrayHost::start(move |event| {
        let _ = tx.send_blocking(Incoming::Tray(event));
    })
    .ok()
}
