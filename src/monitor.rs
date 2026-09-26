//! The `System` page's model: one [`SystemMonitor`] per process, built at boot in
//! `Hooks::after_context` and read by `src/controllers/system.rs`.
//!
//! `System` is an application-wide resource, not a per-request one: sysinfo computes CPU
//! usage as the difference between two `/proc/stat` readings, and the first reading of a
//! fresh `System` has nothing to subtract from (it reports `0.0` for every core). One
//! instance is refreshed by every sample instead.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use sysinfo::{Components, CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};

/// A cached sample is served for this long before the OS is read again.
///
/// Two properties depend on it, both of them why it exceeds
/// `sysinfo::MINIMUM_CPU_UPDATE_INTERVAL` (200 ms): CPU usage is only meaningful when that
/// much wall time separates two refreshes, and a second browser tab (or a reload) must not
/// add a second `/proc/stat` read.
const CACHE_TTL: Duration = Duration::from_secs(1);

/// Host facts that do not change while the process runs.
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub hostname: String,
    pub os: String,
    pub arch: String,
}

/// One CPU's usage at the moment of the sample.
#[derive(Debug, Clone)]
pub struct CoreSample {
    /// The core's OS name, e.g. `cpu0`.
    pub name: String,
    /// 0.0..=100.0.
    pub usage: f32,
}

#[derive(Debug, Clone)]
pub struct MemSample {
    pub total_bytes: u64,
    /// `total - available`: what `free -h` calls "used".
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct TempSample {
    pub label: String,
    pub celsius: f32,
}

/// Everything one poll of the page shows.
#[derive(Debug, Clone)]
pub struct SystemSample {
    /// Unix milliseconds from the server clock — the charts' x values.
    pub taken_at_ms: i64,
    /// Whole-machine usage, 0.0..=100.0.
    pub cpu_total: f32,
    pub cores: Vec<CoreSample>,
    pub memory: MemSample,
    /// Empty when the host reports no hwmon/thermal sensor at all.
    pub temps: Vec<TempSample>,
}

/// Reads host metrics from one long-lived [`System`]/[`Components`] pair.
pub struct SystemMonitor {
    info: SystemInfo,
    inner: Mutex<Inner>,
}

struct Inner {
    sys: System,
    components: Components,
    last: Option<(Instant, SystemSample)>,
}

impl SystemMonitor {
    /// Loads the static host facts and takes the first, discarded CPU reading; the first
    /// [`Self::sample`] call is the one that yields a real CPU percentage.
    #[must_use]
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                .with_memory(MemoryRefreshKind::nothing().with_ram().with_swap()),
        );
        let components = Components::new_with_refreshed_list();
        let arch = System::cpu_arch();

        Self {
            info: SystemInfo {
                hostname: System::host_name().unwrap_or_else(|| "unknown".to_string()),
                os: os_name(),
                arch: if arch.is_empty() {
                    "unknown".to_string()
                } else {
                    arch
                },
            },
            inner: Mutex::new(Inner {
                sys,
                components,
                last: None,
            }),
        }
    }

    pub const fn info(&self) -> &SystemInfo {
        &self.info
    }

    /// Current metrics, reusing the previous refresh while it is younger than [`CACHE_TTL`].
    pub fn sample(&self) -> SystemSample {
        let mut inner = self.lock();
        if let Some((at, cached)) = &inner.last {
            if at.elapsed() < CACHE_TTL {
                return cached.clone();
            }
        }

        inner
            .sys
            .refresh_cpu_specifics(CpuRefreshKind::nothing().with_cpu_usage());
        inner.sys.refresh_memory();
        inner.components.refresh(true);

        let total = inner.sys.total_memory();
        let available = inner.sys.available_memory();
        let sample = SystemSample {
            taken_at_ms: chrono::Utc::now().timestamp_millis(),
            cpu_total: inner.sys.global_cpu_usage(),
            cores: inner
                .sys
                .cpus()
                .iter()
                .map(|cpu| CoreSample {
                    name: cpu.name().to_string(),
                    usage: cpu.cpu_usage(),
                })
                .collect(),
            memory: MemSample {
                total_bytes: total,
                // `free -h`'s "used": whatever is not available to applications, page cache
                // excluded. sysinfo's `used_memory()` is `total - free` and counts the page
                // cache as used, which overstates pressure on every Linux host.
                used_bytes: total.saturating_sub(available),
                available_bytes: available,
                swap_total_bytes: inner.sys.total_swap(),
                swap_used_bytes: inner.sys.used_swap(),
            },
            temps: inner
                .components
                .iter()
                .enumerate()
                .filter_map(|(index, component)| {
                    Some(TempSample {
                        // A sensor whose reading cannot be parsed is skipped, not shown as 0.
                        celsius: component.temperature()?,
                        label: sensor_label(component.label(), component.id(), index),
                    })
                })
                .collect(),
        };

        inner.last = Some((Instant::now(), sample.clone()));
        sample
    }

    /// A poisoned lock means an earlier sample panicked mid-refresh; the monitor holds no
    /// invariant a panic can break, so take the data back instead of propagating.
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for SystemMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// The sensor text to show: sysinfo's hwmon label when there is one, else the sensor id
/// (`thermal_zone0` — the fallback path it uses on Raspberry Pi hosts leaves the label
/// empty), else a positional placeholder.
fn sensor_label(label: &str, id: Option<&str>, index: usize) -> String {
    if !label.is_empty() {
        label.to_string()
    } else if let Some(id) = id.filter(|id| !id.is_empty()) {
        id.to_string()
    } else {
        format!("Sensor {index}")
    }
}

/// `Debian GNU/Linux 13`, `Raspbian GNU/Linux 13` — sysinfo reads `NAME=` and `VERSION_ID=`
/// from `/etc/os-release`; either half may be missing.
fn os_name() -> String {
    match (System::name(), System::os_version()) {
        (Some(name), Some(version)) => format!("{name} {version}"),
        (Some(name), None) => name,
        (None, Some(version)) => version,
        (None, None) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensor_label_prefers_the_hwmon_label() {
        assert_eq!(
            sensor_label("coretemp Package id 0", Some("hwmon0_1"), 0),
            "coretemp Package id 0"
        );
    }

    #[test]
    fn sensor_label_falls_back_to_the_sensor_id() {
        // The `/sys/class/thermal` path sysinfo uses on Raspberry Pi hosts.
        assert_eq!(sensor_label("", Some("thermal_zone0"), 0), "thermal_zone0");
    }

    #[test]
    fn sensor_label_falls_back_to_the_index() {
        assert_eq!(sensor_label("", None, 3), "Sensor 3");
        assert_eq!(sensor_label("", Some(""), 3), "Sensor 3");
    }

    #[test]
    fn sample_reads_the_host_metrics() {
        let monitor = SystemMonitor::new();
        let sample = monitor.sample();

        assert!(sample.memory.total_bytes > 0, "memory was not refreshed");
        assert!(!sample.cores.is_empty(), "the CPU list was not refreshed");
        assert!(
            (0.0..=100.0).contains(&sample.cpu_total),
            "cpu total out of range: {}",
            sample.cpu_total
        );
        for core in &sample.cores {
            assert!(
                (0.0..=100.0).contains(&core.usage),
                "{} out of range: {}",
                core.name,
                core.usage
            );
        }
    }

    #[test]
    fn samples_inside_the_cache_window_are_the_same_sample() {
        let monitor = SystemMonitor::new();
        let first = monitor.sample();
        let second = monitor.sample();

        assert_eq!(
            first.taken_at_ms, second.taken_at_ms,
            "a second sample inside the cache window must be served from the cache"
        );
    }
}
