use serde::Serialize;

use crate::monitor::{SystemInfo, SystemSample, TempSample};

/// `/system`'s host tiles.
#[derive(Debug, Serialize)]
pub struct SystemInfoView {
    pub hostname: String,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Serialize)]
pub struct CoreView {
    /// The core's OS name, e.g. `cpu0`.
    pub name: String,
    /// 0.0..=100.0, for the bar width and the chart.
    pub usage: f32,
    /// `12.3%`.
    pub usage_label: String,
}

#[derive(Debug, Serialize)]
pub struct MemView {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    /// 0.0..=100.0, for the chart.
    pub used_percent: f32,
    pub used_percent_label: String,
    pub used_label: String,
    pub total_label: String,
    pub available_label: String,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    /// `0 B / 2.0 GiB`; only rendered when `swap_total_bytes > 0`.
    pub swap_label: String,
}

#[derive(Debug, Serialize)]
pub struct TempView {
    pub label: String,
    pub celsius: f32,
    /// `45.0 °C`.
    pub celsius_label: String,
}

/// Exactly what the polling fragment and the charts' `data-sample` payload carry.
#[derive(Debug, Serialize)]
pub struct SystemSampleView {
    pub taken_at_ms: i64,
    pub cpu_total: f32,
    pub cpu_total_label: String,
    pub cores: Vec<CoreView>,
    pub memory: MemView,
    pub temps: Vec<TempView>,
}

impl From<&SystemInfo> for SystemInfoView {
    fn from(info: &SystemInfo) -> Self {
        Self {
            hostname: info.hostname.clone(),
            os: info.os.clone(),
            arch: info.arch.clone(),
        }
    }
}

impl From<&SystemSample> for SystemSampleView {
    fn from(sample: &SystemSample) -> Self {
        let used_percent = percent(sample.memory.used_bytes, sample.memory.total_bytes);

        Self {
            taken_at_ms: sample.taken_at_ms,
            cpu_total: round1(sample.cpu_total),
            cpu_total_label: format!("{:.1}%", round1(sample.cpu_total)),
            cores: sample
                .cores
                .iter()
                .map(|core| CoreView {
                    name: core.name.clone(),
                    usage: round1(core.usage),
                    usage_label: format!("{:.1}%", round1(core.usage)),
                })
                .collect(),
            memory: MemView {
                total_bytes: sample.memory.total_bytes,
                used_bytes: sample.memory.used_bytes,
                available_bytes: sample.memory.available_bytes,
                used_percent,
                used_percent_label: format!("{used_percent:.1}%"),
                used_label: bytes(sample.memory.used_bytes),
                total_label: bytes(sample.memory.total_bytes),
                available_label: bytes(sample.memory.available_bytes),
                swap_total_bytes: sample.memory.swap_total_bytes,
                swap_used_bytes: sample.memory.swap_used_bytes,
                swap_label: format!(
                    "{} / {}",
                    bytes(sample.memory.swap_used_bytes),
                    bytes(sample.memory.swap_total_bytes)
                ),
            },
            temps: sample.temps.iter().map(TempView::from).collect(),
        }
    }
}

impl From<&TempSample> for TempView {
    fn from(temp: &TempSample) -> Self {
        Self {
            label: temp.label.clone(),
            celsius: round1(temp.celsius),
            celsius_label: format!("{:.1} °C", round1(temp.celsius)),
        }
    }
}

/// One decimal: the chart and the label beside it must not disagree.
fn round1(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

/// Byte counters are far below `2^53`, and the page shows one decimal, so the lossy
/// `u64`-as-float casts these helpers need cannot be observed.
#[allow(clippy::cast_precision_loss)]
fn percent(part: u64, whole: u64) -> f32 {
    if whole == 0 {
        return 0.0;
    }
    round1((part as f32 / whole as f32) * 100.0)
}

/// Binary units, one decimal: `5.9 GiB`, `512.0 KiB`, `900 B`.
#[allow(clippy::cast_precision_loss)]
fn bytes(value: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;

    let value_f = value as f64;
    if value_f >= GIB {
        format!("{:.1} GiB", value_f / GIB)
    } else if value_f >= MIB {
        format!("{:.1} MiB", value_f / MIB)
    } else if value_f >= KIB {
        format!("{:.1} KiB", value_f / KIB)
    } else {
        format!("{value} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_switches_unit_at_the_boundary() {
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1024), "1.0 KiB");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn percent_does_not_divide_by_zero() {
        assert!(percent(0, 0).abs() < f32::EPSILON);
        assert!((percent(3, 8) - 37.5).abs() < 0.05);
    }
}
