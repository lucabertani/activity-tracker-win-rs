//! Configurable light/dark appearance, with the system theme as default.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Entity, Subscription, Window, px};
use tracker_core::model::ThemePreference;

use crate::model::AppModel;

pub fn init(preference: ThemePreference, cx: &mut App) {
    apply(preference, None, cx);
    crate::main_view::bind_keys(cx);
}

pub fn apply(preference: ThemePreference, window: Option<&mut Window>, cx: &mut App) {
    match preference {
        ThemePreference::Auto => Theme::sync_system_appearance(window, cx),
        ThemePreference::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemePreference::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
    let theme = Theme::global_mut(cx);
    theme.font_family = "Segoe UI Variable Text".into();
    theme.font_size = px(14.);
}

/// Re-evaluate auto on window creation (including reopening from the tray)
/// and on system changes. Explicit choices must not be overwritten.
pub fn follow_system_appearance(
    window: &mut Window,
    model: Entity<AppModel>,
    cx: &mut App,
) -> Subscription {
    apply(model.read(cx).settings.theme, Some(window), cx);
    window.observe_window_appearance(move |window, cx| {
        if model.read(cx).settings.theme == ThemePreference::Auto {
            apply(ThemePreference::Auto, Some(window), cx);
        }
    })
}
