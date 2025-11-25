use std::{num::NonZeroU64, sync::OnceLock};

use hashbrown::HashMap;
use nvml_wrapper::{
    Nvml, enum_wrappers::device::TemperatureSensor, enums::device::UsedGpuMemory, error::NvmlError,
};

use crate::{
    app::{filter::Filter, layout_manager::UsedWidgets},
    collection::{memory::MemData, temperature::TempSensorData},
};

pub static NVML_DATA: OnceLock<Result<Nvml, NvmlError>> = OnceLock::new();

/// GPU metric type - either power draw or utilization percentage.
#[derive(Clone, Debug)]
pub enum GpuMetric {
    /// Power draw in milliwatts with optional power limit.
    Power { draw_mw: u32, limit_mw: Option<u32> },
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

pub struct GpusData {
    pub memory: Option<Vec<(String, MemData)>>,
    pub temperature: Option<Vec<TempSensorData>>,
    pub procs: Option<(u64, Vec<HashMap<u32, (u64, u32)>>)>,
    pub gpu_data: Option<Vec<GpuData>>,
}

/// Wrapper around Nvml::init
///
/// On Linux, if `Nvml::init()` fails, this function attempts to explicitly load
/// the library from `libnvidia-ml.so.1`. On other platforms, it simply calls `Nvml::init`.
///
/// This is a workaround until https://github.com/Cldfire/nvml-wrapper/pull/63 is accepted.
/// Then, we can go back to calling `Nvml::init` directly on all platforms.
fn init_nvml() -> Result<Nvml, NvmlError> {
    #[cfg(not(target_os = "linux"))]
    {
        Nvml::init()
    }
    #[cfg(target_os = "linux")]
    {
        match Nvml::init() {
            Ok(nvml) => Ok(nvml),
            Err(_) => Nvml::builder()
                .lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1"))
                .init(),
        }
    }
}

/// Returns the GPU data from NVIDIA cards.
#[inline]
pub fn get_nvidia_vecs(
    filter: &Option<Filter>, widgets_to_harvest: &UsedWidgets,
) -> Option<GpusData> {
    if let Ok(nvml) = NVML_DATA.get_or_init(init_nvml) {
        if let Ok(num_gpu) = nvml.device_count() {
            let mut temp_vec = Vec::with_capacity(num_gpu as usize);
            let mut mem_vec = Vec::with_capacity(num_gpu as usize);
            let mut proc_vec = Vec::with_capacity(num_gpu as usize);
            let mut gpu_data_vec = Vec::with_capacity(num_gpu as usize);
            let mut total_mem = 0;

            for i in 0..num_gpu {
                if let Ok(device) = nvml.device_by_index(i) {
                    if let Ok(name) = device.name() {
                        if widgets_to_harvest.use_mem {
                            if let Ok(mem) = device.memory_info() {
                                if let Some(total_bytes) = NonZeroU64::new(mem.total) {
                                    mem_vec.push((
                                        name.clone(),
                                        MemData {
                                            total_bytes,
                                            used_bytes: mem.used,
                                        },
                                    ));
                                }
                            }
                        }

                        if widgets_to_harvest.use_temp
                            && Filter::optional_should_keep(filter, &name)
                        {
                            if let Ok(temperature) = device.temperature(TemperatureSensor::Gpu) {
                                temp_vec.push(TempSensorData {
                                    name,
                                    temperature: Some(temperature as f32),
                                });
                            } else {
                                temp_vec.push(TempSensorData {
                                    name,
                                    temperature: None,
                                });
                            }
                        }
                    }

                    if widgets_to_harvest.use_proc {
                        let mut procs = HashMap::new();

                        if let Ok(gpu_procs) = device.process_utilization_stats(None) {
                            for proc in gpu_procs {
                                let pid = proc.pid;
                                let gpu_util = proc.sm_util + proc.enc_util + proc.dec_util;
                                procs.insert(pid, (0, gpu_util));
                            }
                        }

                        if let Ok(compute_procs) = device.running_compute_processes() {
                            for proc in compute_procs {
                                let pid = proc.pid;
                                let gpu_mem = match proc.used_gpu_memory {
                                    UsedGpuMemory::Used(val) => val,
                                    UsedGpuMemory::Unavailable => 0,
                                };
                                if let Some(prev) = procs.get(&pid) {
                                    procs.insert(pid, (gpu_mem, prev.1));
                                } else {
                                    procs.insert(pid, (gpu_mem, 0));
                                }
                            }
                        }

                        // Use the legacy API too but prefer newer API results
                        if let Ok(graphics_procs) = device.running_graphics_processes_v2() {
                            for proc in graphics_procs {
                                let pid = proc.pid;
                                let gpu_mem = match proc.used_gpu_memory {
                                    UsedGpuMemory::Used(val) => val,
                                    UsedGpuMemory::Unavailable => 0,
                                };
                                if let Some(prev) = procs.get(&pid) {
                                    procs.insert(pid, (gpu_mem, prev.1));
                                } else {
                                    procs.insert(pid, (gpu_mem, 0));
                                }
                            }
                        }

                        if let Ok(graphics_procs) = device.running_graphics_processes() {
                            for proc in graphics_procs {
                                let pid = proc.pid;
                                let gpu_mem = match proc.used_gpu_memory {
                                    UsedGpuMemory::Used(val) => val,
                                    UsedGpuMemory::Unavailable => 0,
                                };
                                if let Some(prev) = procs.get(&pid) {
                                    procs.insert(pid, (gpu_mem, prev.1));
                                } else {
                                    procs.insert(pid, (gpu_mem, 0));
                                }
                            }
                        }

                        if !procs.is_empty() {
                            proc_vec.push(procs);
                        }

                        // running total for proc %
                        if let Ok(mem) = device.memory_info() {
                            total_mem += mem.total;
                        }
                    }

                    // Collect power data for GPU widget.
                    if widgets_to_harvest.use_gpu {
                        if let Ok(name) = device.name() {
                            if let Ok(power_mw) = device.power_usage() {
                                let power_limit_mw = device.power_management_limit().ok();
                                gpu_data_vec.push(GpuData {
                                    name,
                                    metric: GpuMetric::Power {
                                        draw_mw: power_mw,
                                        limit_mw: power_limit_mw,
                                    },
                                });
                            }
                        }
                    }
                }
            }

            Some(GpusData {
                memory: if !mem_vec.is_empty() {
                    Some(mem_vec)
                } else {
                    None
                },
                temperature: if !temp_vec.is_empty() {
                    Some(temp_vec)
                } else {
                    None
                },
                procs: if !proc_vec.is_empty() {
                    Some((total_mem, proc_vec))
                } else {
                    None
                },
                gpu_data: if !gpu_data_vec.is_empty() {
                    Some(gpu_data_vec)
                } else {
                    None
                },
            })
        } else {
            None
        }
    } else {
        None
    }
}
