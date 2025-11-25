//! GPU widget rendering.
//!
//! Renders the GPU widget with a power draw chart and legend table, similar to the CPU widget.
//!
//! Public objects:
//! - `Painter::draw_gpu`: Main entry point for drawing the GPU widget.
//!
//! External dependencies: tui.

use tui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
};

use crate::{
    app::{App, data::StoredData, layout_manager::WidgetDirection},
    canvas::{
        Painter,
        components::{
            data_table::{DrawInfo, SelectionState},
            time_graph::{GraphData, variants::percent::PercentTimeGraph},
        },
        drawing_utils::should_hide_x_label,
    },
    widgets::GpuWidgetState,
};

const ALL_POSITION: usize = 0;

impl Painter {
    /// Draws the GPU widget.
    pub fn draw_gpu(&self, f: &mut Frame<'_>, app_state: &mut App, draw_loc: Rect, widget_id: u64) {
        let legend_width = (draw_loc.width as f64 * 0.15) as u16;

        if legend_width < 6 {
            // Skip drawing legend.
            if app_state.current_widget.widget_id == (widget_id + 1) {
                if app_state.app_config_fields.cpu_left_legend {
                    app_state.move_widget_selection(&WidgetDirection::Right);
                } else {
                    app_state.move_widget_selection(&WidgetDirection::Left);
                }
            }
            self.draw_gpu_graph(f, app_state, draw_loc, widget_id);
            if let Some(gpu_widget_state) = app_state
                .states
                .gpu_state
                .widget_states
                .get_mut(&widget_id)
            {
                gpu_widget_state.is_legend_hidden = true;
            }

            // Update draw loc in widget map.
            if app_state.should_get_widget_bounds() {
                if let Some(bottom_widget) = app_state.widget_map.get_mut(&widget_id) {
                    bottom_widget.top_left_corner = Some((draw_loc.x, draw_loc.y));
                    bottom_widget.bottom_right_corner =
                        Some((draw_loc.x + draw_loc.width, draw_loc.y + draw_loc.height));
                }
            }
        } else {
            let graph_width = draw_loc.width - legend_width;
            let (graph_index, legend_index, constraints) =
                if app_state.app_config_fields.cpu_left_legend {
                    (
                        1,
                        0,
                        [
                            Constraint::Length(legend_width),
                            Constraint::Length(graph_width),
                        ],
                    )
                } else {
                    (
                        0,
                        1,
                        [
                            Constraint::Length(graph_width),
                            Constraint::Length(legend_width),
                        ],
                    )
                };

            let partitioned_draw_loc = Layout::default()
                .margin(0)
                .direction(Direction::Horizontal)
                .constraints(constraints)
                .split(draw_loc);

            self.draw_gpu_graph(f, app_state, partitioned_draw_loc[graph_index], widget_id);
            self.draw_gpu_legend(
                f,
                app_state,
                partitioned_draw_loc[legend_index],
                widget_id + 1,
            );

            if app_state.should_get_widget_bounds() {
                // Update draw loc in widget map.
                if let Some(gpu_widget) = app_state.widget_map.get_mut(&widget_id) {
                    gpu_widget.top_left_corner = Some((
                        partitioned_draw_loc[graph_index].x,
                        partitioned_draw_loc[graph_index].y,
                    ));
                    gpu_widget.bottom_right_corner = Some((
                        partitioned_draw_loc[graph_index].x + partitioned_draw_loc[graph_index].width,
                        partitioned_draw_loc[graph_index].y + partitioned_draw_loc[graph_index].height,
                    ));
                }

                if let Some(legend_widget) = app_state.widget_map.get_mut(&(widget_id + 1)) {
                    legend_widget.top_left_corner = Some((
                        partitioned_draw_loc[legend_index].x,
                        partitioned_draw_loc[legend_index].y,
                    ));
                    legend_widget.bottom_right_corner = Some((
                        partitioned_draw_loc[legend_index].x + partitioned_draw_loc[legend_index].width,
                        partitioned_draw_loc[legend_index].y + partitioned_draw_loc[legend_index].height,
                    ));
                }
            }
        }
    }

    fn generate_gpu_points<'a>(
        &self, gpu_widget_state: &'a GpuWidgetState, data: &'a StoredData,
    ) -> Vec<GraphData<'a>> {
        let current_scroll_position = gpu_widget_state.table.state.current_index;
        let gpu_data = &data.gpu_data_harvest;
        let gpu_timeseries = &data.timeseries_data.gpu_data;
        let time = &data.timeseries_data.time;

        if current_scroll_position == ALL_POSITION {
            // Show all GPUs. Collect into Vec first to allow reversing.
            let mut points: Vec<GraphData<'a>> = gpu_timeseries
                .iter()
                .enumerate()
                .map(|(itx, (_name, values))| {
                    let style =
                        self.styles.cpu_colour_styles[itx % self.styles.cpu_colour_styles.len()];

                    GraphData::default()
                        .style(style)
                        .time(time)
                        .values(values)
                })
                .collect();
            points.reverse();
            points
        } else if let Some(gpu) = gpu_data.get(current_scroll_position - 1) {
            // Show single GPU.
            if let Some(values) = gpu_timeseries.get(&gpu.name) {
                let style = self.styles.cpu_colour_styles
                    [(current_scroll_position - 1) % self.styles.cpu_colour_styles.len()];

                vec![GraphData::default().style(style).time(time).values(values)]
            } else {
                vec![]
            }
        } else {
            vec![]
        }
    }

    fn draw_gpu_graph(&self, f: &mut Frame<'_>, app_state: &mut App, draw_loc: Rect, widget_id: u64) {
        if let Some(gpu_widget_state) = app_state
            .states
            .gpu_state
            .widget_states
            .get_mut(&widget_id)
        {
            let data = app_state.data_store.get_data();

            let hide_x_labels = should_hide_x_label(
                app_state.app_config_fields.hide_time,
                app_state.app_config_fields.autohide_time,
                &mut gpu_widget_state.autohide_timer,
                draw_loc,
            );

            let graph_data = self.generate_gpu_points(gpu_widget_state, data);

            // Adapt title based on metric type (power vs utilization).
            let title = if data
                .gpu_data_harvest
                .first()
                .map(|g| g.metric.is_power())
                .unwrap_or(false)
            {
                " GPU Power ".into()
            } else {
                " GPU Usage ".into()
            };

            PercentTimeGraph {
                display_range: gpu_widget_state.current_display_time,
                hide_x_labels,
                app_config_fields: &app_state.app_config_fields,
                current_widget: app_state.current_widget.widget_id,
                is_expanded: app_state.is_expanded,
                title,
                styles: &self.styles,
                widget_id,
                legend_position: None,
                legend_constraints: None,
            }
            .build()
            .draw(f, draw_loc, graph_data);
        }
    }

    fn draw_gpu_legend(
        &self, f: &mut Frame<'_>, app_state: &mut App, draw_loc: Rect, widget_id: u64,
    ) {
        let recalculate_column_widths = app_state.should_get_widget_bounds();
        if let Some(gpu_widget_state) = app_state
            .states
            .gpu_state
            .widget_states
            .get_mut(&(widget_id - 1))
        {
            gpu_widget_state.is_legend_hidden = false;

            let is_on_widget = widget_id == app_state.current_widget.widget_id;

            let draw_info = DrawInfo {
                loc: draw_loc,
                force_redraw: app_state.is_force_redraw,
                recalculate_column_widths,
                selection_state: SelectionState::new(app_state.is_expanded, is_on_widget),
            };

            gpu_widget_state.table.draw(
                f,
                &draw_info,
                app_state.widget_map.get_mut(&widget_id),
                self,
            );
        }
    }
}
