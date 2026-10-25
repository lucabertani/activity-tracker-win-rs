//! Main window: custom title bar, header cards (RAM, CPU, throttling),
//! toolbar (search, terminate, settings) and the process table, with the
//! settings panel sliding in from the right.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, Hsla, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, SharedString, Styled, Subscription, Window,
    WindowControlArea, actions, div, px,
};
use tracker_core::model::{DisplayRow, ThrottleState};
use tracker_core::process_list::{RowKey, mib};

use crate::model::AppModel;
use crate::process_table::ProcessTable;
use crate::settings_panel::{SettingsPanel, SettingsPanelEvent};
use crate::shell::APP_TITLE;

actions!(
    activity_tracker,
    [
        FocusSearch,
        TerminateSelected,
        ToggleSelectedGroup,
        CloseToTray
    ]
);

const CONTEXT: &str = "MainView";
const TITLE_BAR_HEIGHT: f32 = 36.;

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-f", FocusSearch, Some(CONTEXT)),
        KeyBinding::new("delete", TerminateSelected, Some(CONTEXT)),
        KeyBinding::new("enter", ToggleSelectedGroup, Some(CONTEXT)),
        KeyBinding::new("ctrl-w", CloseToTray, Some(CONTEXT)),
    ]);
}

pub struct MainView {
    model: Entity<AppModel>,
    table: Entity<TableState<ProcessTable>>,
    search: Entity<InputState>,
    settings: Option<Entity<SettingsPanel>>,
    /// Selected row by identity: survives refreshes and re-sorting.
    selected: Option<RowKey>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl MainView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let table = cx.new(|cx| {
            TableState::new(ProcessTable::new(model.clone()), window, cx)
                .col_selectable(false)
                .col_movable(false)
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search processes"));

        let subscriptions = vec![
            cx.observe(&model, |this, _, cx| this.on_model_changed(cx)),
            cx.subscribe(&table, |this, _, event: &TableEvent, cx| match event {
                TableEvent::SelectRow(row) => {
                    this.selected = this.model.read(cx).processes.row_key(*row);
                    cx.notify();
                }
                TableEvent::DoubleClickedRow(row) => this.toggle_group(*row, cx),
                TableEvent::ClearSelection => {
                    this.selected = None;
                    cx.notify();
                }
                _ => {}
            }),
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let query = input.read(cx).value().to_string();
                    this.model.update(cx, |model, cx| {
                        model.processes.set_search(&query);
                        cx.notify();
                    });
                }
            }),
        ];

        Self {
            model,
            table,
            search,
            settings: None,
            selected: None,
            focus: cx.focus_handle(),
            _subscriptions: {
                let mut subscriptions = subscriptions;
                subscriptions.push(crate::theme::follow_system_appearance(window));
                subscriptions
            },
        }
    }

    /// Rows changed (refresh, search, sort, expand): keep the selection on
    /// the same process and repaint the table.
    fn on_model_changed(&mut self, cx: &mut Context<Self>) {
        let index = self
            .selected
            .as_ref()
            .and_then(|key| self.model.read(cx).processes.find_row(key));
        if self.selected.is_some() && index.is_none() {
            self.selected = None;
        }
        self.table.update(cx, |table, cx| {
            match index {
                Some(row) if table.selected_row() != Some(row) => {
                    table.set_selection(gpui_kit::component::table::TableSelection::Row(row), cx);
                }
                None if table.selected_row().is_some() => table.clear_selection(cx),
                _ => {}
            }
            cx.notify();
        });
        cx.notify();
    }

    fn toggle_group(&mut self, row: usize, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            if model.processes.toggle_group(row) {
                cx.notify();
            }
        });
    }

    fn selected_row(&self, cx: &App) -> Option<usize> {
        self.selected
            .as_ref()
            .and_then(|key| self.model.read(cx).processes.find_row(key))
    }

    fn terminate_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row(cx) else {
            window.push_notification("Select a process from the list first.", cx);
            return;
        };
        let result = self.model.update(cx, |model, _| model.terminate_row(row));
        match result {
            Ok(count) if count > 1 => {
                window.push_notification(format!("Terminated {count} processes."), cx)
            }
            Ok(_) => window.push_notification("Process terminated.", cx),
            Err(err) => window.push_notification(err, cx),
        }
    }

    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.take().is_none() {
            let panel = cx.new(|cx| SettingsPanel::new(self.model.clone(), window, cx));
            self._subscriptions.push(cx.subscribe_in(
                &panel,
                window,
                |this, _, event, window, cx| {
                    match event {
                        SettingsPanelEvent::Close => this.settings = None,
                        SettingsPanelEvent::Message(text) => {
                            window.push_notification(text.clone(), cx)
                        }
                    }
                    cx.notify();
                },
            ));
            self.settings = Some(panel);
        }
        cx.notify();
    }

    fn close_to_tray(&mut self, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }

    fn render_title_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let hover_bg = theme.secondary_hover;
        let button = move |id: &'static str, icon: IconName, close: bool| {
            div()
                .id(id)
                .h_full()
                .w(px(46.))
                .flex()
                .items_center()
                .justify_center()
                .hover(move |this| {
                    if close {
                        this.bg(gpui_kit::rgb(0xc42b1c))
                            .text_color(gpui_kit::white())
                    } else {
                        this.bg(hover_bg)
                    }
                })
                .child(Icon::new(icon).small())
        };
        h_flex()
            .h(px(TITLE_BAR_HEIGHT))
            .w_full()
            .flex_shrink_0()
            .bg(theme.title_bar)
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .id("title-drag")
                    .flex_1()
                    .h_full()
                    .pl_3()
                    .gap_2()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        Icon::new(IconName::LayoutDashboard)
                            .small()
                            .text_color(theme.primary),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(APP_TITLE),
                    ),
            )
            // Minimize and close both send the app to the tray, as before.
            .child(
                button("title-minimize", IconName::WindowMinimize, false)
                    .on_click(cx.listener(|this, _, window, cx| this.close_to_tray(window, cx))),
            )
            .child(
                button(
                    "title-maximize",
                    if window.is_maximized() {
                        IconName::WindowRestore
                    } else {
                        IconName::WindowMaximize
                    },
                    false,
                )
                .window_control_area(WindowControlArea::Max),
            )
            .child(
                button("title-close", IconName::WindowClose, true)
                    .on_click(cx.listener(|this, _, window, cx| this.close_to_tray(window, cx))),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let theme = cx.theme();
        let list = &model.processes;

        let ram_detail = if list.total_ram_bytes() > 0 {
            format!(
                "{:.1} / {:.1} GB",
                gib(list.ram_in_use_bytes()),
                gib(list.total_ram_bytes())
            )
        } else {
            String::new()
        };
        let ram = card(
            "RAM",
            format!("{}%", model.ram_percent),
            ram_detail,
            Some((
                model.ram_percent as f32 / 100.0,
                level_color(model.ram_percent as f32, 75.0, 90.0, cx),
            )),
            None,
            cx,
        );

        let thermal = &model.thermal;
        let (cpu_value, cpu_color) = match thermal.avg_temp_c {
            Some(t) => (
                format!("{t:.0} °C"),
                Some(level_color(t as f32, 75.0, 90.0, cx)),
            ),
            None => ("n/a".to_string(), None),
        };
        let mut cpu_detail = Vec::new();
        if let Some(package) = thermal.package_temp_c {
            cpu_detail.push(format!("package {package} °C"));
        }
        if let Some(mhz) = thermal.avg_effective_mhz {
            cpu_detail.push(format!("{:.2} GHz", mhz as f64 / 1000.0));
        }
        let cpu = card(
            "CPU average",
            cpu_value,
            cpu_detail.join(" · "),
            None,
            cpu_color,
            cx,
        );

        let (badge, badge_color, detail) = match thermal.state() {
            ThrottleState::Unknown => (
                "n/a",
                theme.muted_foreground,
                "needs PawnIO + elevation".to_string(),
            ),
            ThrottleState::Ok if thermal.reasons.power_limit => {
                ("Power limited", theme.warning, last_minute(thermal))
            }
            ThrottleState::Ok => ("OK", theme.success, last_minute(thermal)),
            ThrottleState::Throttling => ("Throttling", theme.danger, thermal.reasons.describe()),
        };
        let throttle = card(
            "Thermal throttling",
            badge.to_string(),
            detail,
            None,
            Some(badge_color),
            cx,
        );

        h_flex()
            .gap_3()
            .px_4()
            .pt_4()
            .child(ram)
            .child(cpu)
            .child(throttle)
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let can_terminate = matches!(
            self.selected_row(cx)
                .and_then(|row| self.model.read(cx).processes.rows().get(row)),
            Some(DisplayRow::Group { .. } | DisplayRow::Proc(_))
        );
        h_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(
                div().flex_1().child(
                    Input::new(&self.search)
                        .prefix(Icon::new(IconName::Search).small())
                        .cleanable(true),
                ),
            )
            .child(
                Button::new("terminate")
                    .danger()
                    .icon(IconName::Delete)
                    .label("Terminate")
                    .disabled(!can_terminate)
                    .tooltip("End the selected process (or the whole group) — Del")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.terminate_selected(window, cx)),
                    ),
            )
            .child(
                Button::new("settings")
                    .ghost()
                    .icon(IconName::Settings)
                    .tooltip("Settings")
                    .selected(self.settings.is_some())
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_settings(window, cx))),
            )
    }
}

impl Focusable for MainView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();

        v_flex()
            .id("main-view")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                this.search.update(cx, |input, cx| input.focus(window, cx));
            }))
            .on_action(cx.listener(|this, _: &TerminateSelected, window, cx| {
                this.terminate_selected(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSelectedGroup, _, cx| {
                if let Some(row) = this.selected_row(cx) {
                    this.toggle_group(row, cx);
                }
            }))
            .on_action(
                cx.listener(|this, _: &CloseToTray, window, cx| this.close_to_tray(window, cx)),
            )
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(self.render_title_bar(window, cx))
            .child(
                // The settings panel floats over the right side, so the table
                // keeps its full width underneath.
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.render_header(cx))
                            .child(self.render_toolbar(cx))
                            .child(
                                div().flex_1().min_h_0().px_4().pb_4().child(
                                    DataTable::new(&self.table)
                                        .stripe(true)
                                        .bordered(true)
                                        .small(),
                                ),
                            ),
                    )
                    .children(self.settings.clone().map(|panel| {
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom_0()
                            .w(px(360.))
                            .shadow_xl()
                            .border_l_1()
                            .border_color(theme.border)
                            .bg(theme.sidebar)
                            .child(panel)
                    })),
            )
    }
}

fn gib(bytes: u64) -> f64 {
    mib(bytes) / 1024.0
}

fn last_minute(thermal: &tracker_core::model::ThermalStatus) -> String {
    match thermal.throttled_share_last_minute {
        Some(share) if share > 0.0 => format!("{:.0}% of the last minute", share * 100.0),
        _ => "last minute clean".to_string(),
    }
}

/// Green below `warn`, amber up to `alert`, red above.
fn level_color(value: f32, warn: f32, alert: f32, cx: &App) -> Hsla {
    let theme = cx.theme();
    if value >= alert {
        theme.danger
    } else if value >= warn {
        theme.warning
    } else {
        theme.success
    }
}

/// One header card: label, big value, detail line, optional usage bar.
fn card(
    label: &'static str,
    value: String,
    detail: String,
    bar: Option<(f32, Hsla)>,
    value_color: Option<Hsla>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .p_3()
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .text_2xl()
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .when_some(value_color, |this, color| this.text_color(color))
                .child(value),
        )
        .when_some(bar, |this, (fraction, color)| {
            this.child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded(px(2.))
                    .bg(theme.muted)
                    .child(
                        div()
                            .h_full()
                            .rounded(px(2.))
                            .bg(color)
                            .w(gpui_kit::relative(fraction.clamp(0.0, 1.0))),
                    ),
            )
        })
        .child(
            div()
                .text_xs()
                .text_color(theme.muted_foreground)
                .truncate()
                .child(if detail.is_empty() {
                    " ".to_string()
                } else {
                    detail
                }),
        )
        .into_any_element()
}
