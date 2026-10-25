//! Look & feel: follows the Windows light/dark setting, Segoe UI Variable.

use gpui_kit::component::Theme;
use gpui_kit::{App, Subscription, Window, px};

pub fn init(cx: &mut App) {
    // gpui-component starts in light mode: match the system instead.
    Theme::sync_system_appearance(None, cx);
    apply_fonts(cx);
    crate::main_view::bind_keys(cx);
}

/// Keeps the theme in sync when the user switches light/dark while the
/// window is open. Keep the subscription alive with the view.
pub fn follow_system_appearance(window: &mut Window) -> Subscription {
    window.observe_window_appearance(|window, cx| {
        Theme::sync_system_appearance(Some(window), cx);
        apply_fonts(cx);
    })
}

fn apply_fonts(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.font_family = "Segoe UI Variable Text".into();
    theme.font_size = px(14.);
}
