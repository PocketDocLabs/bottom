//! Common GPU data types shared across backends.
//!
//! Provides platform-agnostic GPU data structures used by both the nvidia
//! and apple-gpu backends.
//!
//! Public objects:
//! - `GpuMetric`: Enum representing either power draw or utilization.
//! - `GpuData`: GPU data with name and metric.
//!
//! External dependencies: None.
//!
//! Usage:
//! ```ignore
//! let data = GpuData {
//!     name: "GPU 0".to_string(),
//!     metric: GpuMetric::Utilization(75.0),
//! };
//! ```

/// GPU metric type - either power draw or utilization percentage.
#[derive(Clone, Debug)]
pub enum GpuMetric {
    /// Power draw in milliwatts with optional power limit.
    Power {
        draw_mw: u32,
        limit_mw: Option<u32>,
    },
    /// Utilization as a percentage (0-100).
    Utilization(f32),
}

impl GpuMetric {
    /// Returns the metric as a percentage (0-100).
    pub fn as_percentage(&self) -> f32 {
        match self {
            GpuMetric::Power { draw_mw, limit_mw } => {
                if let Some(limit) = limit_mw {
                    if *limit > 0 {
                        (*draw_mw as f32 / *limit as f32) * 100.0
                    } else {
                        0.0
                    }
                } else {
                    // No limit known, can't compute percentage.
                    0.0
                }
            }
            GpuMetric::Utilization(pct) => *pct,
        }
    }

    /// Returns true if this metric represents power data.
    pub fn is_power(&self) -> bool {
        matches!(self, GpuMetric::Power { .. })
    }
}

impl Default for GpuMetric {
    fn default() -> Self {
        GpuMetric::Utilization(0.0)
    }
}

/// GPU data with either power draw or utilization.
#[derive(Clone, Debug, Default)]
pub struct GpuData {
    /// GPU name.
    pub name: String,
    /// The GPU metric (power or utilization).
    pub metric: GpuMetric,
}
