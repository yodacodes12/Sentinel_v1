//! GPU telemetry from vendor sysfs interfaces, feature-detected per device.
//!
//! - amdgpu: `gpu_busy_percent`, `mem_info_{vram,gtt}_{used,total}`, hwmon
//!   `temp1_input`, `power1_average`/`power1_input` (µW), `freq1_input` (Hz).
//! - i915/xe: current GT frequency only; utilisation needs the perf
//!   interface, which Sentinel does not use.
//! - nouveau: hwmon temperature.
//! - NVIDIA's proprietary driver exposes no utilisation in sysfs; Sentinel
//!   does not run `nvidia-smi` or any other program.
//!
//! Integrated GPUs' "VRAM" is system RAM reserved for the GPU and is
//! reported with memory model `shared`.

use sentinel_hardware::SysRoot;

use super::{RunReport, SampleTime};
use crate::model::{GpuMemoryModel, GpuReport, GpuSample, Reading, ReadingState, round1};

const PCI: &str = "sys/bus/pci/devices";

fn hex(root: &SysRoot, rel: &str) -> Option<u32> {
    sentinel_hardware::parsers::hex_u32(&root.read_trimmed(rel).ok()?)
}

fn num(root: &SysRoot, rel: &str) -> Reading<u64> {
    match root.read_trimmed(rel) {
        Ok(v) => v
            .parse::<u64>()
            .map(Reading::direct)
            .unwrap_or_else(|_| Reading::missing(ReadingState::Error)),
        Err(e) => Reading::missing(super::state_of(&e)),
    }
}

/// Integrated when the GPU shares its PCI slot with AMD platform functions
/// (APU) or is Intel graphics on bus 0; dedicated for NVIDIA or a lone AMD
/// card. Same rule as hardware discovery.
pub fn memory_model(vendor: u32, address: &str, slot_vendors: &[u32]) -> GpuMemoryModel {
    match vendor {
        0x8086 if address.get(5..7) == Some("00") => GpuMemoryModel::Shared,
        0x1002 if slot_vendors.contains(&0x1022) => GpuMemoryModel::Shared,
        0x10de | 0x1002 => GpuMemoryModel::Dedicated,
        _ => GpuMemoryModel::Unknown,
    }
}

pub fn sample(root: &SysRoot, t: SampleTime) -> (Option<GpuReport>, RunReport) {
    let addresses = root.list(PCI).unwrap_or_default();
    let mut gpus = Vec::new();
    for address in &addresses {
        let dir = format!("{PCI}/{address}");
        if hex(root, &format!("{dir}/class")).is_none_or(|c| c >> 16 != 0x03) {
            continue;
        }
        let vendor = hex(root, &format!("{dir}/vendor")).unwrap_or(0);
        let slot = &address[..address.len().saturating_sub(2)];
        let slot_vendors: Vec<u32> = addresses
            .iter()
            .filter(|a| *a != address && a.starts_with(slot))
            .filter_map(|a| hex(root, &format!("{PCI}/{a}/vendor")))
            .collect();
        let driver = root.link_name(&format!("{dir}/driver"));
        let hwmon = root
            .list(&format!("{dir}/hwmon"))
            .ok()
            .and_then(|h| h.into_iter().next())
            .map(|h| format!("{dir}/hwmon/{h}"));
        let temperature_c = hwmon
            .as_ref()
            .and_then(|h| root.read_trimmed(&format!("{h}/temp1_input")).ok())
            .and_then(|v| super::thermal::millidegrees(&v))
            .map(Reading::direct)
            .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported));
        let power_w = hwmon
            .as_ref()
            .and_then(|h| {
                root.read_trimmed(&format!("{h}/power1_average"))
                    .or_else(|_| root.read_trimmed(&format!("{h}/power1_input")))
                    .ok()
            })
            .and_then(|v| v.parse::<u64>().ok())
            .map(|uw| Reading::direct(round1(uw as f64 / 1e6)))
            .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported));
        let core_clock_mhz = match hwmon
            .as_ref()
            .and_then(|h| root.read_trimmed(&format!("{h}/freq1_input")).ok())
        {
            Some(hz) => hz
                .parse::<u64>()
                .map(|v| Reading::direct(v / 1_000_000))
                .unwrap_or_else(|_| Reading::missing(ReadingState::Error)),
            None => {
                // Intel: current GT frequency under the DRM card.
                let card = root
                    .list(&format!("{dir}/drm"))
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.starts_with("card"));
                card.and_then(|c| {
                    root.read_trimmed(&format!("{dir}/drm/{c}/gt_cur_freq_mhz"))
                        .ok()
                })
                .and_then(|v| v.parse::<u64>().ok())
                .map(Reading::direct)
                .unwrap_or_else(|| Reading::missing(ReadingState::Unsupported))
            }
        };
        let busy = match root.read_trimmed(&format!("{dir}/gpu_busy_percent")) {
            Ok(v) => v
                .parse::<f64>()
                .map(Reading::direct)
                .unwrap_or_else(|_| Reading::missing(ReadingState::Error)),
            Err(e) => Reading::missing(super::state_of(&e)),
        };
        let note = match (driver.as_deref(), busy.is_ok()) {
            (_, true) => None,
            (Some("nvidia"), _) => Some(
                "The NVIDIA driver does not expose utilisation through sysfs. Sentinel does not run nvidia-smi.".to_string(),
            ),
            (Some("i915" | "xe"), _) => {
                Some("Intel GPU utilisation needs the perf interface, which Sentinel does not use.".to_string())
            }
            (None, _) => Some("No driver is bound to this GPU.".to_string()),
            _ => Some("This driver does not expose GPU utilisation.".to_string()),
        };
        gpus.push(GpuSample {
            pci_address: address.clone(),
            memory_model: memory_model(vendor, address, &slot_vendors),
            busy_percent: busy,
            vram_used_bytes: num(root, &format!("{dir}/mem_info_vram_used")),
            vram_total_bytes: num(root, &format!("{dir}/mem_info_vram_total")),
            gtt_used_bytes: num(root, &format!("{dir}/mem_info_gtt_used")),
            gtt_total_bytes: num(root, &format!("{dir}/mem_info_gtt_total")),
            temperature_c,
            power_w,
            core_clock_mhz,
            driver,
            note,
        });
    }
    let report = if gpus.is_empty() {
        RunReport::unsupported("no PCI graphics device")
    } else if gpus.iter().all(|g| g.busy_percent.is_ok()) {
        RunReport::ok()
    } else {
        RunReport::partial("some GPU telemetry is not exposed by the driver")
    };
    (
        Some(GpuReport {
            measured_at: t.wall_ms,
            interval_ms: t.interval_ms,
            gpus,
        }),
        report,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_models() {
        assert_eq!(
            memory_model(0x1002, "0000:03:00.0", &[0x1022, 0x1002]),
            GpuMemoryModel::Shared
        );
        assert_eq!(
            memory_model(0x1002, "0000:01:00.0", &[0x1002]),
            GpuMemoryModel::Dedicated
        );
        assert_eq!(
            memory_model(0x8086, "0000:00:02.0", &[]),
            GpuMemoryModel::Shared
        );
        assert_eq!(
            memory_model(0x10de, "0000:01:00.0", &[]),
            GpuMemoryModel::Dedicated
        );
        assert_eq!(
            memory_model(0x1af4, "0000:00:01.0", &[]),
            GpuMemoryModel::Unknown
        );
    }
}
