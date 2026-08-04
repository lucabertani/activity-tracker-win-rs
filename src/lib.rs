#[cfg(target_os = "windows")]
pub mod app;
#[cfg(target_os = "windows")]
pub mod error;
#[cfg(target_os = "windows")]
pub mod model;
#[cfg(target_os = "windows")]
pub mod monitor;
#[cfg(target_os = "windows")]
pub mod settings;
#[cfg(target_os = "windows")]
pub mod ui;

#[cfg(target_os = "windows")]
pub fn run() -> error::AppResult<()> {
    app::run()
}
