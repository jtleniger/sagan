//! `cargo loco task hardware_check` — probe the Pi subsystems without a browser.
//!
//! One sensor reading, a fan duty that is set and read back, and one camera frame written to a
//! directory the operator can look in.

use std::path::PathBuf;

use loco_rs::prelude::*;

use crate::hardware::{Hardware, HardwareError, MAX_SPEED_PERCENT};

/// The duty the fan is put at while probing: audible on a real fan, and the read-back below is
/// asserted to match it.
const PROBE_SPEED_PERCENT: u8 = MAX_SPEED_PERCENT / 2;

pub struct HardwareCheck;

#[async_trait]
impl Task for HardwareCheck {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "hardware_check".to_string(),
            detail: "Probe the Pi subsystems: one sensor reading, the fan set and read back, one \
                     camera frame. Usage: cargo loco task hardware_check dir:target"
                .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let hardware = Hardware::of(ctx)?;
        // `Unavailable` is a fact about this host, not a failure: the mock camera frames, so a
        // laptop writes a file too; `Unavailable` is the Pi driver's answer when no camera is
        // attached, and the probe still exits 0. Anything else means hardware that should have
        // answered did not, and the task fails with the reasons.
        let mut failures: Vec<String> = Vec::new();

        match hardware.environment.readings().await {
            Ok(reading) => tracing::info!(
                celsius = reading.celsius,
                humidity_percent = reading.humidity_percent,
                pressure_hpa = reading.pressure_hpa,
                "bme280 reading"
            ),
            Err(HardwareError::Unavailable) => {
                tracing::warn!("no environment sensor on this host");
            }
            Err(err) => failures.push(format!("sensor: {err}")),
        }

        match hardware.fan.set_speed(PROBE_SPEED_PERCENT).await {
            Ok(()) => match hardware.fan.speed().await {
                Ok(actual) if actual == PROBE_SPEED_PERCENT => {
                    tracing::info!(percent = actual, "fan speed set and read back");
                }
                Ok(actual) => failures.push(format!(
                    "fan: set {PROBE_SPEED_PERCENT}% but read back {actual}%"
                )),
                Err(err) => failures.push(format!("fan: {err}")),
            },
            Err(err) => failures.push(format!("fan: {err}")),
        }

        let dir = PathBuf::from(vars.cli_arg("dir").unwrap_or("target"));
        let filename = format!(
            "hardware-check-{}.jpg",
            chrono::Utc::now().timestamp_millis()
        );
        // The driver writes the file itself, the way the Pi camera tools do; the probe only names
        // a path the operator can open.
        match hardware.camera.capture(&dir, &filename).await {
            Ok(()) => {
                tracing::info!(path = %dir.join(&filename).display(), "capture written");
            }
            Err(HardwareError::Unavailable) => {
                tracing::warn!("no camera on this host");
            }
            Err(err) => failures.push(format!("camera: {err}")),
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(Error::string(&format!(
                "hardware probe failed: {}",
                failures.join("; ")
            )))
        }
    }
}
