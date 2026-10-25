// Probe: starts the tray host alone and reports what happens.
use tracker_platform::model::{AppSettings, ThermalStatus};
use tracker_platform::tray_host::TrayHost;

fn main() {
    match TrayHost::start(|event| println!("event: {event:?}")) {
        Ok(host) => {
            println!("tray host started");
            host.update(&AppSettings::default(), 42, &ThermalStatus::default());
            std::thread::sleep(std::time::Duration::from_secs(8));
        }
        Err(err) => println!("tray host failed: {err}"),
    }
}
