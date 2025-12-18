//! macOS GPU data collection via IOKit.
//!
//! Provides GPU utilization and memory data for macOS systems by querying
//! IOAccelerator services through IOKit. Works on both Intel Macs with
//! discrete GPUs and Apple Silicon.
//!
//! Public objects:
//! - `AppleGpusData`: Collection of all GPU data from Apple systems.
//! - `get_apple_gpu_vecs`: Main entry point for GPU data collection.
//!
//! External dependencies: core-foundation, core-foundation-sys, libc.
//!
//! Usage:
//! ```ignore
//! if let Some(data) = get_apple_gpu_vecs(&widgets_to_harvest) {
//!     // Process GPU data
//! }
//! ```

use std::{ffi::CStr, ptr};

use core_foundation::{
    base::{CFAllocatorRef, CFType, TCFType, kCFAllocatorDefault},
    dictionary::{CFDictionary, CFDictionaryRef, CFMutableDictionaryRef},
    number::CFNumber,
    string::CFString,
};
use mach2::kern_return::kern_return_t;

use crate::app::layout_manager::UsedWidgets;

// Re-export common GPU types from the gpu module.
pub use super::gpu::{GpuData, GpuMetric};

// IOKit type aliases.
#[allow(non_camel_case_types)]
type io_object_t = u32;
#[allow(non_camel_case_types)]
type io_iterator_t = io_object_t;
#[allow(non_camel_case_types)]
type io_registry_entry_t = io_object_t;

const KERN_SUCCESS: kern_return_t = 0;
const IO_OBJECT_NULL: io_object_t = 0;

// IOKit FFI bindings for GPU enumeration.
// NOTE: These duplicate some bindings from disks/unix/macos but that module is private.
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    #[link_name = "kIOMasterPortDefault"]
    static kIOMasterPortDefault: u32;

    fn IOServiceMatching(name: *const libc::c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        mainPort: u32, matching: CFMutableDictionaryRef, existing: *mut io_iterator_t,
    ) -> kern_return_t;
    fn IOIteratorNext(iterator: io_iterator_t) -> io_object_t;
    fn IORegistryEntryCreateCFProperties(
        entry: io_registry_entry_t, properties: *mut CFMutableDictionaryRef,
        allocator: CFAllocatorRef, options: u32,
    ) -> kern_return_t;
    fn IORegistryEntryGetName(entry: io_registry_entry_t, name: *mut libc::c_char)
    -> kern_return_t;
    fn IOObjectRelease(object: io_object_t) -> kern_return_t;
}

/// GPU data collected from Apple systems.
pub struct AppleGpusData {
    /// GPU utilization/power data for the GPU widget.
    pub gpu_data: Option<Vec<GpuData>>,
}

/// Retrieves GPU data from macOS IOAccelerator services.
///
/// Queries IOKit for GPU performance statistics including utilization percentage.
/// On Apple Silicon, VRAM metrics are not meaningful due to unified memory.
///
/// Args:
///     widgets_to_harvest: Configuration for which widgets need data.
///
/// Returns:
///     Option<AppleGpusData>: GPU data if available, None if no GPUs found or on error.
pub fn get_apple_gpu_vecs(widgets_to_harvest: &UsedWidgets) -> Option<AppleGpusData> {
    if !widgets_to_harvest.use_gpu {
        return None;
    }

    let gpu_data_vec = collect_gpu_data()?;

    if gpu_data_vec.is_empty() {
        return None;
    }

    Some(AppleGpusData {
        gpu_data: Some(gpu_data_vec),
    })
}

/// Collects GPU data from all IOAccelerator services.
fn collect_gpu_data() -> Option<Vec<GpuData>> {
    let mut gpu_data_vec = Vec::new();

    // SAFETY: IOServiceMatching takes a C string and returns a CFDictionary.
    // The dictionary is consumed by IOServiceGetMatchingServices.
    let matching_dict = unsafe {
        let service_name = b"IOAccelerator\0";
        IOServiceMatching(service_name.as_ptr() as *const libc::c_char)
    };

    if matching_dict.is_null() {
        return None;
    }

    let mut iterator: io_iterator_t = 0;

    // SAFETY: IOServiceGetMatchingServices consumes matching_dict and populates iterator.
    let result =
        unsafe { IOServiceGetMatchingServices(kIOMasterPortDefault, matching_dict, &mut iterator) };

    if result != KERN_SUCCESS {
        return None;
    }

    // Iterate through all GPU services.
    loop {
        // SAFETY: IOIteratorNext returns the next object or IO_OBJECT_NULL.
        let service = unsafe { IOIteratorNext(iterator) };
        if service == IO_OBJECT_NULL {
            break;
        }

        if let Some(gpu_data) = extract_gpu_data_from_service(service) {
            gpu_data_vec.push(gpu_data);
        }

        // SAFETY: Release the service object after use.
        unsafe {
            IOObjectRelease(service);
        }
    }

    // SAFETY: Release the iterator.
    unsafe {
        IOObjectRelease(iterator);
    }

    Some(gpu_data_vec)
}

/// Extracts GPU data from a single IOAccelerator service.
fn extract_gpu_data_from_service(service: io_registry_entry_t) -> Option<GpuData> {
    let name = get_service_name(service)?;
    let properties = get_service_properties(service)?;

    // Try to get utilization from PerformanceStatistics dictionary.
    let utilization = get_utilization_from_properties(&properties);

    Some(GpuData {
        name,
        metric: GpuMetric::Utilization(utilization.unwrap_or(0.0)),
    })
}

/// Gets the name of an IOKit service.
fn get_service_name(service: io_registry_entry_t) -> Option<String> {
    let mut name_buffer: [libc::c_char; 128] = [0; 128];

    // SAFETY: IORegistryEntryGetName writes to the buffer.
    let result = unsafe { IORegistryEntryGetName(service, name_buffer.as_mut_ptr()) };

    if result != KERN_SUCCESS {
        return None;
    }

    // SAFETY: The buffer is null-terminated by IORegistryEntryGetName.
    let c_str = unsafe { CStr::from_ptr(name_buffer.as_ptr()) };
    Some(c_str.to_string_lossy().into_owned())
}

/// Gets all properties of an IOKit service as a CFDictionary.
fn get_service_properties(service: io_registry_entry_t) -> Option<CFDictionary<CFString, CFType>> {
    let mut properties_ref: CFMutableDictionaryRef = ptr::null_mut();

    // SAFETY: IORegistryEntryCreateCFProperties allocates a new dictionary.
    let result = unsafe {
        IORegistryEntryCreateCFProperties(
            service,
            &mut properties_ref,
            kCFAllocatorDefault as CFAllocatorRef,
            0,
        )
    };

    if result != KERN_SUCCESS || properties_ref.is_null() {
        return None;
    }

    // SAFETY: We own the dictionary reference and wrap it in a safe type.
    // CFMutableDictionary is a subtype of CFDictionary, so this cast is safe.
    let properties: CFDictionary<CFString, CFType> =
        unsafe { CFDictionary::wrap_under_create_rule(properties_ref as CFDictionaryRef) };

    Some(properties)
}

/// Extracts GPU utilization percentage from service properties.
///
/// Looks for PerformanceStatistics dictionary and tries multiple known keys
/// for utilization data, as Apple changes these between OS versions.
fn get_utilization_from_properties(properties: &CFDictionary<CFString, CFType>) -> Option<f32> {
    // Get the PerformanceStatistics sub-dictionary.
    let perf_stats_key = CFString::new("PerformanceStatistics");
    let perf_stats_value = properties.find(&perf_stats_key)?;

    // SAFETY: We're downcasting the CFType to CFDictionary.
    let perf_stats: CFDictionary<CFString, CFType> = unsafe {
        let dict_ref = perf_stats_value.as_CFTypeRef() as CFDictionaryRef;
        if dict_ref.is_null() {
            return None;
        }
        // Retain since we're creating a new wrapper.
        CFDictionary::wrap_under_get_rule(dict_ref)
    };

    // Try various known keys for GPU utilization.
    // Apple changes these between macOS versions.
    let utilization_keys = [
        "Device Utilization %",
        "GPU Activity(%)",
        "GPU Core Utilization",
        "gpuCoreUtilization",
        "GPU Utilization",
    ];

    for key_str in &utilization_keys {
        let key = CFString::new(key_str);
        if let Some(value) = perf_stats.find(&key) {
            if let Some(num) = extract_number(&value) {
                // Clamp to valid percentage range.
                return Some(num.clamp(0.0, 100.0));
            }
        }
    }

    None
}

/// Extracts a numeric value from a CFType, converting to f32.
fn extract_number(value: &CFType) -> Option<f32> {
    // SAFETY: Downcast to CFNumber if the type matches.
    let type_id = value.type_of();
    if type_id == CFNumber::type_id() {
        let num_ref = value.as_CFTypeRef() as core_foundation::number::CFNumberRef;
        let num: CFNumber = unsafe { CFNumber::wrap_under_get_rule(num_ref) };

        // Try to get as various numeric types.
        if let Some(val) = num.to_f64() {
            return Some(val as f32);
        }
        if let Some(val) = num.to_i64() {
            return Some(val as f32);
        }
        if let Some(val) = num.to_i32() {
            return Some(val as f32);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_gpu_data_does_not_crash() {
        // Basic smoke test - should not panic.
        let _ = collect_gpu_data();
    }
}
