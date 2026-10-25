//! App shell: owns the model, opens the main window on demand (tray
//! double-click / "Open window") and quits on "Exit".
//!
//! GPUI has no hide/show on Windows, and an idle GPUI window still holds
//! its DirectX resources (~170 MB working set in the spike), so "to tray"
//! means closing the window; reopening it takes a fraction of a second.

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, Subscription, TitlebarOptions, WindowBounds,
    WindowOptions, px, size,
};

use crate::main_view::MainView;
use crate::model::{AppModel, ModelEvent};

pub const APP_TITLE: &str = "Activity Tracker";

pub struct Shell {
    model: Entity<AppModel>,
    window: Option<AnyWindowHandle>,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(model: Entity<AppModel>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let mut subscriptions = vec![cx.subscribe(&model, |this, _, event, cx| match event {
                ModelEvent::OpenWindow => {
                    let _ = this.open_window(cx);
                }
                ModelEvent::Exit => {
                    this.model.update(cx, |model, _| model.shutdown());
                    cx.quit();
                }
            })];
            // Closing the window (title bar button, Alt+F4, taskbar) keeps the
            // app in the tray: just stop the process enumeration.
            subscriptions.push(cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty()
                    && let Some(shell) = cx.try_global::<ShellGlobal>().map(|g| g.0.clone())
                {
                    shell.update(cx, |this, cx| {
                        this.window = None;
                        this.model
                            .update(cx, |model, _| model.set_window_open(false));
                    });
                }
            }));
            Self {
                model,
                window: None,
                _subscriptions: subscriptions,
            }
        })
    }

    /// Opens the main window, or brings the existing one to the front.
    pub fn open_window(&mut self, cx: &mut Context<Self>) -> gpui_kit::Result<()> {
        if let Some(handle) = self.window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return Ok(());
        }

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(APP_TITLE.into()),
                // Custom title bar drawn by the main view.
                appears_transparent: true,
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::centered(size(px(780.), px(720.)), cx)),
            window_min_size: Some(size(px(560.), px(420.))),
            app_id: Some("ActivityTracker".into()),
            ..Default::default()
        };
        let model = self.model.clone();
        let (handle, _) = gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| MainView::new(model, window, cx))
        })?;
        self.window = Some(handle);
        self.model
            .update(cx, |model, _| model.set_window_open(true));
        Ok(())
    }
}

/// Lets the window-closed hook reach the shell.
pub struct ShellGlobal(pub Entity<Shell>);

impl gpui_kit::Global for ShellGlobal {}
