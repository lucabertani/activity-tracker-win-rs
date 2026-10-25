//! `TableDelegate` over the shared [`ProcessList`]: rows are read straight
//! from the model on every render, sorting is forwarded to it.

use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div, px,
    relative,
};
use tracker_core::model::{DisplayRow, SortColumn, SortDirection, SortState};
use tracker_core::process_list::{mib, row_bytes};

use crate::model::AppModel;

const COL_NAME: usize = 0;
const COL_PID: usize = 1;
const COL_MIB: usize = 2;
const COL_PERCENT: usize = 3;

pub struct ProcessTable {
    model: Entity<AppModel>,
}

impl ProcessTable {
    pub fn new(model: Entity<AppModel>) -> Self {
        Self { model }
    }
}

fn sort_column(col_ix: usize) -> Option<SortColumn> {
    match col_ix {
        COL_NAME => Some(SortColumn::Name),
        COL_MIB | COL_PERCENT => Some(SortColumn::Memory),
        _ => None,
    }
}

impl TableDelegate for ProcessTable {
    fn columns_count(&self, _: &App) -> usize {
        4
    }

    fn rows_count(&self, cx: &App) -> usize {
        self.model.read(cx).processes.rows().len()
    }

    fn column(&self, col_ix: usize, cx: &App) -> Column {
        let column = match col_ix {
            COL_NAME => Column::new("name", "Process").width(px(300.)),
            COL_PID => Column::new("pid", "PID").width(px(80.)).text_right(),
            COL_MIB => Column::new("mib", "RAM MiB").width(px(110.)).text_right(),
            _ => Column::new("percent", "RAM %").width(px(150.)),
        };
        let Some(this_column) = sort_column(col_ix) else {
            return column;
        };
        // Show the model's current sort, so a column refresh keeps the icon.
        let sort = self.model.read(cx).processes.sort();
        if sort.sort_column != this_column || col_ix == COL_PERCENT {
            return column.sortable();
        }
        match sort.sort_direction {
            SortDirection::Asc => column.ascending(),
            SortDirection::Desc => column.descending(),
        }
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        let Some(column) = sort_column(col_ix) else {
            return;
        };
        let state = match sort {
            // Back to "unsorted": the app default (largest memory first).
            ColumnSort::Default => SortState::default(),
            ColumnSort::Ascending => SortState {
                sort_column: column,
                sort_direction: SortDirection::Asc,
            },
            ColumnSort::Descending => SortState {
                sort_column: column,
                sort_direction: SortDirection::Desc,
            },
        };
        self.model.update(cx, |model, cx| {
            model.processes.set_sort(state);
            cx.notify();
        });
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let model = self.model.read(cx);
        let list = &model.processes;
        let Some(row) = list.rows().get(row_ix) else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let bytes = row_bytes(row);
        let percent = list.percent_of_ram(bytes);
        // Bars are relative to the largest row, so small values stay visible.
        let largest = list
            .rows()
            .iter()
            .filter(|r| !matches!(r, DisplayRow::System { .. }))
            .map(row_bytes)
            .max()
            .unwrap_or(0);
        let bar = if largest == 0 {
            0.0
        } else {
            (bytes as f64 / largest as f64).clamp(0.0, 1.0)
        };

        match col_ix {
            COL_NAME => {
                let (label, detail, indent, chevron): (
                    SharedString,
                    Option<String>,
                    bool,
                    Option<bool>,
                ) = match row {
                    DisplayRow::Group {
                        name,
                        count,
                        expanded,
                        ..
                    } => (
                        name.clone().into(),
                        (*count > 1).then(|| format!("{count}")),
                        false,
                        Some(*expanded),
                    ),
                    DisplayRow::Proc(process) => (process.name.clone().into(), None, true, None),
                    DisplayRow::System { .. } => ("System & shared".into(), None, false, None),
                };
                let muted = theme.muted_foreground;
                let is_system = matches!(row, DisplayRow::System { .. });
                h_flex()
                    .gap_1()
                    .when(indent, |this| this.pl(px(22.)))
                    .child(match chevron {
                        Some(expanded) => Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .text_color(muted)
                        .into_any_element(),
                        None => div().w(px(12.)).into_any_element(),
                    })
                    .child(
                        div()
                            .truncate()
                            .when(is_system, |this| this.italic().text_color(muted))
                            .child(label),
                    )
                    .when_some(detail, |this, count| {
                        this.child(
                            div()
                                .px_1()
                                .rounded(px(4.))
                                .bg(theme.muted)
                                .text_xs()
                                .text_color(muted)
                                .child(count),
                        )
                    })
                    .into_any_element()
            }
            COL_PID => {
                let text = match row {
                    DisplayRow::Proc(process) => process.pid.to_string(),
                    _ => String::new(),
                };
                div()
                    .w_full()
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .child(text)
                    .into_any_element()
            }
            COL_MIB => div()
                .w_full()
                .text_right()
                .child(format!("{:.1}", mib(bytes)))
                .into_any_element(),
            _ => h_flex()
                .w_full()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        // The system remainder is not a process: no bar.
                        .when(matches!(row, DisplayRow::System { .. }), |this| {
                            this.invisible()
                        })
                        .h(px(4.))
                        .rounded(px(2.))
                        .bg(theme.muted)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(2.))
                                .bg(theme.primary)
                                .w(relative(bar as f32)),
                        ),
                )
                .child(
                    div()
                        .w(px(48.))
                        .text_right()
                        .child(format!("{percent:.1}%")),
                )
                .into_any_element(),
        }
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, cx: &App) -> String {
        let list = &self.model.read(cx).processes;
        let Some(row) = list.rows().get(row_ix) else {
            return String::new();
        };
        let bytes = row_bytes(row);
        match (col_ix, row) {
            (COL_NAME, DisplayRow::Group { name, .. }) => name.clone(),
            (COL_NAME, DisplayRow::Proc(process)) => process.name.clone(),
            (COL_NAME, DisplayRow::System { .. }) => "System & shared".to_string(),
            (COL_PID, DisplayRow::Proc(process)) => process.pid.to_string(),
            (COL_MIB, _) => format!("{:.1}", mib(bytes)),
            (COL_PERCENT, _) => format!("{:.1}", list.percent_of_ram(bytes)),
            _ => String::new(),
        }
    }
}
