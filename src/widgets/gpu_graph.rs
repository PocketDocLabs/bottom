//! GPU widget state and data structures.
//!
//! Provides the GPU widget which displays power or utilization per GPU as a chart.
//!
//! Public objects:
//! - `GpuWidgetState`: State for the GPU widget.
//! - `GpuWidgetColumn`: Column types for the GPU legend table.
//! - `GpuWidgetTableData`: Data for the GPU legend table.
//!
//! External dependencies: tui, concat_string.
//!
//! Usage:
//! ```ignore
//! let state = GpuWidgetState::new(&config, display_time, autohide_timer, &styles);
//! ```

use std::{borrow::Cow, num::NonZeroU16, time::Instant};

use concat_string::concat_string;
use tui::widgets::Row;

use crate::{
    app::AppConfigFields,
    canvas::{
        Painter,
        components::data_table::{
            Column, ColumnHeader, DataTable, DataTableColumn, DataTableProps, DataTableStyling,
            DataToCell,
        },
    },
    collection::nvidia::{GpuData, GpuMetric},
    options::config::style::Styles,
};

/// Column types for the GPU legend table.
pub enum GpuWidgetColumn {
    Gpu,
    /// Shows either power (W) or utilization (%) depending on available data.
    Metric,
}

impl ColumnHeader for GpuWidgetColumn {
    fn text(&self) -> Cow<'static, str> {
        match self {
            GpuWidgetColumn::Gpu => "GPU".into(),
            // Header is generic; actual display adapts based on data type.
            GpuWidgetColumn::Metric => "Metric".into(),
        }
    }
}

/// Data for the GPU legend table.
pub enum GpuWidgetTableData {
    All,
    Entry {
        index: usize,
        name: String,
        metric: GpuMetric,
    },
}

impl GpuWidgetTableData {
    /// Creates table data from GPU data.
    pub fn from_gpu_data(index: usize, data: &GpuData) -> GpuWidgetTableData {
        GpuWidgetTableData::Entry {
            index,
            name: data.name.clone(),
            metric: data.metric.clone(),
        }
    }
}

impl DataToCell<GpuWidgetColumn> for GpuWidgetTableData {
    fn to_cell_text(
        &self, column: &GpuWidgetColumn, calculated_width: NonZeroU16,
    ) -> Option<Cow<'static, str>> {
        const GPU_TRUNCATE_BREAKPOINT: u16 = 5;

        let calculated_width = calculated_width.get();

        match &self {
            GpuWidgetTableData::All => match column {
                GpuWidgetColumn::Gpu => Some("All".into()),
                GpuWidgetColumn::Metric => None,
            },
            GpuWidgetTableData::Entry {
                index,
                name: _,
                metric,
            } => {
                if calculated_width == 0 {
                    None
                } else {
                    match column {
                        GpuWidgetColumn::Gpu => {
                            let index_str = index.to_string();
                            let text = if calculated_width < GPU_TRUNCATE_BREAKPOINT {
                                index_str.into()
                            } else {
                                concat_string!("GPU", index_str).into()
                            };
                            Some(text)
                        }
                        GpuWidgetColumn::Metric => {
                            let text = match metric {
                                GpuMetric::Power { draw_mw, limit_mw } => {
                                    let draw_w = *draw_mw as f32 / 1000.0;
                                    if let Some(limit) = limit_mw {
                                        let limit_w = *limit as f32 / 1000.0;
                                        format!("{:.0}/{:.0}W", draw_w, limit_w)
                                    } else {
                                        format!("{:.0}W", draw_w)
                                    }
                                }
                                GpuMetric::Utilization(pct) => {
                                    format!("{:.1}%", pct)
                                }
                            };
                            Some(text.into())
                        }
                    }
                }
            }
        }
    }

    #[inline(always)]
    fn style_row<'a>(&self, row: Row<'a>, painter: &Painter) -> Row<'a> {
        let style = match self {
            GpuWidgetTableData::All => painter.styles.all_cpu_colour,
            GpuWidgetTableData::Entry { index, .. } => {
                // Reuse CPU colour styles for GPUs.
                painter.styles.cpu_colour_styles[index % painter.styles.cpu_colour_styles.len()]
            }
        };

        row.style(style)
    }

    fn column_widths<C: DataTableColumn<GpuWidgetColumn>>(
        _data: &[Self], _columns: &[C],
    ) -> Vec<u16>
    where
        Self: Sized,
    {
        vec![1, 8]
    }
}

/// State for the GPU widget.
pub struct GpuWidgetState {
    /// Current display time range in milliseconds.
    pub current_display_time: u64,
    /// Whether the legend is hidden.
    pub is_legend_hidden: bool,
    /// Timer for autohiding the time label.
    pub autohide_timer: Option<Instant>,
    /// The legend table.
    pub table: DataTable<GpuWidgetTableData, GpuWidgetColumn>,
    /// Whether to force a data update.
    pub force_update_data: bool,
}

impl GpuWidgetState {
    /// Creates a new GPU widget state.
    ///
    /// Args:
    ///     config: The app config fields.
    ///     currentDisplayTime: The current display time range in milliseconds.
    ///     autohideTimer: Optional timer for autohiding the time label.
    ///     colours: The style configuration.
    ///
    /// Returns:
    ///     GpuWidgetState: The new GPU widget state.
    pub(crate) fn new(
        config: &AppConfigFields, current_display_time: u64, autohide_timer: Option<Instant>,
        colours: &Styles,
    ) -> Self {
        const COLUMNS: [Column<GpuWidgetColumn>; 2] = [
            Column::soft(GpuWidgetColumn::Gpu, Some(0.4)),
            Column::soft(GpuWidgetColumn::Metric, Some(0.6)),
        ];

        let props = DataTableProps {
            title: None,
            table_gap: config.table_gap,
            left_to_right: false,
            is_basic: false,
            show_table_scroll_position: false,
            show_current_entry_when_unfocused: true,
        };

        let styling = DataTableStyling::from_palette(colours);
        let table = DataTable::new(COLUMNS, props, styling);

        GpuWidgetState {
            current_display_time,
            is_legend_hidden: false,
            autohide_timer,
            table,
            force_update_data: false,
        }
    }

    /// Forces an update of the data stored.
    #[inline]
    pub fn force_data_update(&mut self) {
        self.force_update_data = true;
    }

    /// Sets the legend data from GPU data.
    pub fn set_legend_data(&mut self, data: &[GpuData]) {
        self.table.set_data(
            std::iter::once(GpuWidgetTableData::All)
                .chain(
                    data.iter()
                        .enumerate()
                        .map(|(i, d)| GpuWidgetTableData::from_gpu_data(i, d)),
                )
                .collect(),
        );
        self.force_update_data = false;
    }
}
