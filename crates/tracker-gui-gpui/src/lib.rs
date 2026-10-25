//! GPU-rendered GUI on GPUI (gpui-kit): custom title bar, header cards,
//! virtualized process table sorted from its column headers, settings
//! panel. The tray stays Win32 (`tracker_platform::tray_host`), since GPUI
//! has no tray support.
#![cfg(target_os = "windows")]

mod main_view;
mod model;
mod process_table;
mod settings_panel;
mod shell;
mod theme;

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::QuitMode;
use tracker_core::error::{AppError, AppResult};
use tracker_platform::startup::Startup;

use crate::model::AppModel;
use crate::shell::{Shell, ShellGlobal};

/// Runs the GPUI frontend until "Exit". Fails (so the caller can fall back
/// to the Win32 GUI) when the first window cannot be opened.
pub fn run(startup: Startup) -> AppResult<()> {
    let failure: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let failure_in_app = Rc::clone(&failure);

    gpui_kit::application()
        // Tray app: keep running with no window open.
        .with_quit_mode(QuitMode::Explicit)
        // Icon SVGs (without them every Icon renders empty).
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);

            let model = AppModel::new(startup.settings.clone(), !startup.start_hidden, cx);
            let shell = Shell::new(model.clone(), cx);
            cx.set_global(ShellGlobal(shell.clone()));

            if !startup.start_hidden
                && let Err(err) = shell.update(cx, |shell, cx| shell.open_window(cx))
            {
                *failure_in_app.borrow_mut() = Some(err.to_string());
                model.update(cx, |model, _| model.shutdown());
                cx.quit();
            }
        });

    match failure.borrow_mut().take() {
        Some(err) => Err(AppError::Gui(err)),
        None => Ok(()),
    }
}
