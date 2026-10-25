//! Settings panel shown on the right of the main window. Every change is
//! applied and saved right away (no Apply button).

use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{InputEvent, InputState, NumberInput};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, IconName, IndexPath, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, EventEmitter, Hsla, IntoElement, ParentElement,
    Render, RenderImage, SharedString, Styled, Subscription, Window, div, img, px, rgb,
};
use tracker_core::model::{
    AppSettings, DEFAULT_TRAY_CPU_BG, DEFAULT_TRAY_CPU_FG, DEFAULT_TRAY_FONT_WEIGHT,
    DEFAULT_TRAY_ICON_FONT_SIZE, DEFAULT_TRAY_RAM_BG, DEFAULT_TRAY_RAM_FG,
};
use tracker_platform::restart::switch_prompt;
use tracker_platform::tray::{IconPixels, cpu_icon_pixels, ram_icon_pixels};

use crate::model::AppModel;

/// Font sizes offered for the tray text (0 = automatic).
const FONT_SIZES: [u32; 8] = [0, 14, 15, 16, 17, 18, 19, 20];
/// Segoe UI weights offered for the tray text.
const FONT_WEIGHTS: [u32; 10] = [100, 150, 200, 250, 300, 350, 400, 450, 500, 550];

#[derive(Debug, Clone)]
pub enum SettingsPanelEvent {
    Close,
    /// Something for a toast (e.g. saving failed).
    Message(String),
}

#[derive(Clone, Copy)]
enum Swatch {
    RamBg,
    RamFg,
    CpuBg,
    CpuFg,
}

pub struct SettingsPanel {
    model: Entity<AppModel>,
    interval: Entity<InputState>,
    colors: Vec<(Swatch, Entity<ColorPickerState>)>,
    font_size: Entity<SelectState<Vec<SharedString>>>,
    font_weight: Entity<SelectState<Vec<SharedString>>>,
    previews: Option<(Arc<RenderImage>, Arc<RenderImage>)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsPanelEvent> for SettingsPanel {}

impl SettingsPanel {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = model.read(cx).settings.clone();
        let mut subscriptions = Vec::new();

        let interval = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(settings.refresh_interval_seconds.to_string())
                .step(1.0)
                .min(1.0)
                .max(60.0)
        });
        subscriptions.push(cx.subscribe_in(
            &interval,
            window,
            |this, input, event: &InputEvent, window, cx| {
                if let InputEvent::Change = event
                    && let Ok(seconds) = input.read(cx).value().trim().parse::<u64>()
                {
                    this.change(window, cx, |s| s.refresh_interval_seconds = seconds);
                }
            },
        ));

        let mut colors = Vec::new();
        for swatch in [Swatch::RamBg, Swatch::RamFg, Swatch::CpuBg, Swatch::CpuFg] {
            let initial = colorref_to_hsla(swatch_value(&settings, swatch));
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(initial));
            subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this, _, event: &ColorPickerEvent, window, cx| {
                    let ColorPickerEvent::Change(Some(color)) = event else {
                        return;
                    };
                    let color = hsla_to_colorref(*color);
                    this.change(window, cx, |s| set_swatch(s, swatch, color));
                },
            ));
            colors.push((swatch, picker));
        }

        let font_size = cx.new(|cx| {
            let items = FONT_SIZES
                .iter()
                .map(|&size| font_size_label(size))
                .collect::<Vec<_>>();
            let index = FONT_SIZES
                .iter()
                .position(|&size| size == settings.tray_icon_font_size);
            SelectState::new(items, index.map(IndexPath::new), window, cx)
        });
        subscriptions.push(cx.subscribe_in(
            &font_size,
            window,
            |this, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                if let Some(&size) = FONT_SIZES.iter().find(|&&s| font_size_label(s) == *label) {
                    this.change(window, cx, |s| s.tray_icon_font_size = size);
                }
            },
        ));

        let font_weight = cx.new(|cx| {
            let items = FONT_WEIGHTS
                .iter()
                .map(|w| SharedString::from(w.to_string()))
                .collect::<Vec<_>>();
            let index = FONT_WEIGHTS
                .iter()
                .position(|&w| w == settings.tray_icon_font_weight);
            SelectState::new(items, index.map(IndexPath::new), window, cx)
        });
        subscriptions.push(cx.subscribe_in(
            &font_weight,
            window,
            |this, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                if let Ok(weight) = label.parse::<u32>() {
                    this.change(window, cx, |s| s.tray_icon_font_weight = weight);
                }
            },
        ));

        // Previews follow the live readings (tray text) as well.
        subscriptions.push(cx.observe_in(&model, window, |this, _, window, cx| {
            this.refresh_previews(window, cx);
        }));

        let mut panel = Self {
            model,
            interval,
            colors,
            font_size,
            font_weight,
            previews: None,
            _subscriptions: subscriptions,
        };
        panel.refresh_previews(window, cx);
        panel
    }

    fn settings<'a>(&self, cx: &'a App) -> &'a AppSettings {
        &self.model.read(cx).settings
    }

    /// Applies one edit to the settings, saves them, reports failures.
    fn change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut AppSettings),
    ) {
        let mut settings = self.settings(cx).clone();
        edit(&mut settings);
        let result = self
            .model
            .update(cx, |model, cx| model.apply_settings(settings, cx));
        if let Err(message) = result {
            cx.emit(SettingsPanelEvent::Message(message));
        }
        self.refresh_previews(window, cx);
        cx.notify();
    }

    /// Asks to restart into the classic interface; the switch keeps showing
    /// "on" until the restart happens.
    fn confirm_switch_to_classic(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let (title, text) = switch_prompt(false);
        window.open_alert_dialog(cx, move |alert, _, _| {
            let model = model.clone();
            alert
                .title(title)
                .description(text)
                .confirm()
                .ok_text("Restart")
                .on_ok(move |_, window, cx| {
                    if let Err(message) = model.update(cx, |model, cx| model.switch_to_classic(cx))
                    {
                        window.push_notification(message, cx);
                    }
                    true
                })
        });
    }

    fn restore_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let defaults = AppSettings::default();
        self.change(window, cx, |s| {
            s.tray_icon_ram_enabled = defaults.tray_icon_ram_enabled;
            s.tray_icon_cpu_enabled = defaults.tray_icon_cpu_enabled;
            s.tray_icon_ram_bg = DEFAULT_TRAY_RAM_BG;
            s.tray_icon_ram_fg = DEFAULT_TRAY_RAM_FG;
            s.tray_icon_cpu_bg = DEFAULT_TRAY_CPU_BG;
            s.tray_icon_cpu_fg = DEFAULT_TRAY_CPU_FG;
            s.tray_icon_font_size = DEFAULT_TRAY_ICON_FONT_SIZE;
            s.tray_icon_font_weight = DEFAULT_TRAY_FONT_WEIGHT;
            s.tray_icon_thermal_red = defaults.tray_icon_thermal_red;
        });
        let settings = self.settings(cx).clone();
        for (swatch, picker) in self.colors.clone() {
            let color = colorref_to_hsla(swatch_value(&settings, swatch));
            picker.update(cx, |picker, cx| picker.set_value(color, window, cx));
        }
        self.font_size.update(cx, |select, cx| {
            let index = FONT_SIZES
                .iter()
                .position(|&size| size == settings.tray_icon_font_size);
            select.set_selected_index(index.map(IndexPath::new), window, cx)
        });
        self.font_weight.update(cx, |select, cx| {
            let index = FONT_WEIGHTS
                .iter()
                .position(|&w| w == settings.tray_icon_font_weight);
            select.set_selected_index(index.map(IndexPath::new), window, cx)
        });
    }

    /// Re-renders the two icon previews with the GDI tray renderer, so they
    /// match the real tray pixel for pixel.
    fn refresh_previews(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        let temp = model.thermal.avg_temp_c.map(|t| t as u32);
        let ram = ram_icon_pixels(model.ram_percent, &model.settings).map(render_image);
        let cpu = cpu_icon_pixels(temp, &model.settings).map(render_image);
        let new = ram.zip(cpu);
        if let Some((old_ram, old_cpu)) = std::mem::replace(&mut self.previews, new) {
            let _ = window.drop_image(old_ram);
            let _ = window.drop_image(old_cpu);
        }
    }

    fn color_picker(&self, swatch: Swatch) -> AnyElement {
        self.colors
            .iter()
            .find(|(s, _)| {
                matches!(
                    (s, swatch),
                    (Swatch::RamBg, Swatch::RamBg)
                        | (Swatch::RamFg, Swatch::RamFg)
                        | (Swatch::CpuBg, Swatch::CpuBg)
                        | (Swatch::CpuFg, Swatch::CpuFg)
                )
            })
            .map(|(_, picker)| ColorPicker::new(picker).small().into_any_element())
            .unwrap_or_else(|| div().into_any_element())
    }
}

fn section(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .pt_2()
        .text_xs()
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.to_uppercase())
}

fn row(label: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    h_flex()
        .justify_between()
        .gap_3()
        .min_h(px(32.))
        .child(div().text_sm().child(label.into()))
        .child(control)
}

impl Render for SettingsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings(cx).clone();
        let can_switch_ui = self.model.read(cx).can_switch_ui;
        let theme = cx.theme().clone();
        let switch = |id: &'static str, checked: bool, edit: fn(&mut AppSettings, bool)| {
            Switch::new(id).checked(checked).on_click(cx.listener(
                move |this, checked: &bool, window, cx| {
                    let checked = *checked;
                    this.change(window, cx, |s| edit(s, checked));
                },
            ))
        };
        let preview = |image: Option<Arc<RenderImage>>| match image {
            Some(image) => img(image).size(px(32.)).rounded(px(4.)).into_any_element(),
            None => div().size(px(32.)).into_any_element(),
        };
        let (ram_preview, cpu_preview) = match &self.previews {
            Some((ram, cpu)) => (Some(ram.clone()), Some(cpu.clone())),
            None => (None, None),
        };

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Settings"),
                    )
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .tooltip("Close")
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(SettingsPanelEvent::Close)),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("settings-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .px_4()
                    .pb_4()
                    .gap_1()
                    .when(can_switch_ui, |this| {
                        this.child(section("Interface", cx)).child(
                            v_flex()
                                .child(row(
                                    "New interface",
                                    Switch::new("modern-ui").checked(true).on_click(
                                        cx.listener(|this, _: &bool, window, cx| {
                                            this.confirm_switch_to_classic(window, cx)
                                        }),
                                    ),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(
                                            "Turn off for the lighter classic window                                              (less RAM and CPU). Restarts the app.",
                                        ),
                                ),
                        )
                    })
                    .child(section("General", cx))
                    .child(row(
                        "Start with Windows",
                        switch("autostart", settings.start_on_windows, |s, v| {
                            s.start_on_windows = v
                        }),
                    ))
                    .child(row(
                        "Start in background",
                        switch("background", settings.start_in_background, |s, v| {
                            s.start_in_background = v
                        }),
                    ))
                    .child(row(
                        "Refresh every (seconds)",
                        div()
                            .w(px(110.))
                            .child(NumberInput::new(&self.interval).small()),
                    ))
                    .child(section("Tray icons", cx))
                    .child(row(
                        "Red icon while throttling",
                        switch("thermal-red", settings.tray_icon_thermal_red, |s, v| {
                            s.tray_icon_thermal_red = v
                        }),
                    ))
                    .child(
                        h_flex().gap_3().py_2().child(preview(ram_preview)).child(
                            v_flex()
                                .flex_1()
                                .gap_1()
                                .child(row(
                                    "RAM icon",
                                    switch(
                                        "ram-enabled",
                                        settings.tray_icon_ram_enabled,
                                        |s, v| s.tray_icon_ram_enabled = v,
                                    ),
                                ))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child("Background")
                                        .child(self.color_picker(Swatch::RamBg))
                                        .child("Text")
                                        .child(self.color_picker(Swatch::RamFg)),
                                ),
                        ),
                    )
                    .child(
                        h_flex().gap_3().py_2().child(preview(cpu_preview)).child(
                            v_flex()
                                .flex_1()
                                .gap_1()
                                .child(row(
                                    "CPU icon",
                                    switch(
                                        "cpu-enabled",
                                        settings.tray_icon_cpu_enabled,
                                        |s, v| s.tray_icon_cpu_enabled = v,
                                    ),
                                ))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child("Background")
                                        .child(self.color_picker(Swatch::CpuBg))
                                        .child("Text")
                                        .child(self.color_picker(Swatch::CpuFg)),
                                ),
                        ),
                    )
                    .child(row(
                        "Font size",
                        div()
                            .w(px(110.))
                            .child(Select::new(&self.font_size).small()),
                    ))
                    .child(row(
                        "Font weight",
                        div()
                            .w(px(110.))
                            .child(Select::new(&self.font_weight).small()),
                    ))
                    .child(
                        h_flex().pt_3().child(
                            Button::new("restore-defaults")
                                .outline()
                                .small()
                                .label("Restore tray defaults")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.restore_defaults(window, cx)
                                })),
                        ),
                    ),
            )
    }
}

fn font_size_label(size: u32) -> SharedString {
    if size == 0 {
        "Auto".into()
    } else {
        format!("{size} px").into()
    }
}

fn swatch_value(settings: &AppSettings, swatch: Swatch) -> u32 {
    match swatch {
        Swatch::RamBg => settings.tray_icon_ram_bg,
        Swatch::RamFg => settings.tray_icon_ram_fg,
        Swatch::CpuBg => settings.tray_icon_cpu_bg,
        Swatch::CpuFg => settings.tray_icon_cpu_fg,
    }
}

fn set_swatch(settings: &mut AppSettings, swatch: Swatch, color: u32) {
    match swatch {
        Swatch::RamBg => settings.tray_icon_ram_bg = color,
        Swatch::RamFg => settings.tray_icon_ram_fg = color,
        Swatch::CpuBg => settings.tray_icon_cpu_bg = color,
        Swatch::CpuFg => settings.tray_icon_cpu_fg = color,
    }
}

/// COLORREF is 0x00BBGGRR; gpui's `rgb` takes 0xRRGGBB.
fn colorref_to_hsla(colorref: u32) -> Hsla {
    let (r, g, b) = (
        colorref & 0xFF,
        (colorref >> 8) & 0xFF,
        (colorref >> 16) & 0xFF,
    );
    rgb((r << 16) | (g << 8) | b).into()
}

fn hsla_to_colorref(color: Hsla) -> u32 {
    let c = color.to_rgb();
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    channel(c.r) | (channel(c.g) << 8) | (channel(c.b) << 16)
}

/// gpui images are BGRA, the same layout the GDI renderer produces.
fn render_image(pixels: IconPixels) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(pixels.width, pixels.height, pixels.bgra)
        .expect("buffer size matches the icon size");
    Arc::new(RenderImage::new([image::Frame::new(buffer)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_roundtrip() {
        for colorref in [0x003e3e3e, 0x00FFFFFF, 0x000000FF, 0x00123456] {
            assert_eq!(hsla_to_colorref(colorref_to_hsla(colorref)), colorref);
        }
    }
}
