//! The Raspberry Pi subsystems — a PWM case fan, an I2C BME280, the camera — behind traits the
//! rest of the app holds without a Pi under it.
//!
//! Every host the app runs on gets a [`Hardware`] bundle: on a Pi (once `src/hardware/pi.rs`
//! exists) the real drivers, everywhere else the mock in [`mock`]. `Hooks::after_context` builds
//! it once and puts it in `shared_store`; [`Hardware::of`] is the only reader.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use loco_rs::{app::AppContext, config::Config, Error, Result};
use serde::Deserialize;

pub mod mock;

/// The full-scale duty a [`Fan`] accepts: `set_speed` rejects anything above it.
pub const MAX_SPEED_PERCENT: u8 = 100;

/// A hardware call that did not do what the caller asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareError {
    /// This host has no such hardware (a `driver: pi` build started on a machine with no Pi
    /// peripheral behind the syscall).
    Unavailable,
    /// The OS or the device refused the call; the string is the underlying message.
    Io(String),
    /// A caller-supplied duty was above [`MAX_SPEED_PERCENT`].
    InvalidSpeed(u8),
}

impl std::fmt::Display for HardwareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(f, "hardware is not available on this host"),
            Self::Io(message) => write!(f, "hardware I/O failed: {message}"),
            Self::InvalidSpeed(percent) => {
                write!(f, "fan speed {percent}% is outside 0..={MAX_SPEED_PERCENT}")
            }
        }
    }
}

impl std::error::Error for HardwareError {}

impl From<std::io::Error> for HardwareError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

/// One ambient reading, in the units the field names carry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Readings {
    /// Air temperature, °C.
    pub celsius: f32,
    /// Relative humidity, 0.0..=100.0 %.
    pub humidity_percent: f32,
    /// Barometric pressure, hPa — the BME280's compensated output unit.
    pub pressure_hpa: f32,
    /// Unix milliseconds from the server clock.
    pub taken_at_ms: i64,
}

#[async_trait]
pub trait Fan: Send + Sync {
    /// Set the duty as a whole percent of full speed, `0..=100`; `0` stops the fan.
    ///
    /// # Errors
    /// [`HardwareError::InvalidSpeed`] when `percent` is above [`MAX_SPEED_PERCENT`];
    /// [`HardwareError::Io`] when the PWM device rejects the write.
    async fn set_speed(&self, percent: u8) -> Result<(), HardwareError>;

    /// The duty last set, as a whole percent — the *commanded* speed: a two-wire fan on a 5 V
    /// rail plus a PWM GPIO has no tachometer line to measure RPM from.
    ///
    /// # Errors
    /// [`HardwareError::Io`] when the PWM device cannot be read back.
    async fn speed(&self) -> Result<u8, HardwareError>;
}

#[async_trait]
pub trait EnvironmentSensor: Send + Sync {
    /// # Errors
    /// [`HardwareError::Unavailable`] when no chip answers on the configured I2C bus;
    /// [`HardwareError::Io`] when the chip answers but the transfer fails.
    async fn readings(&self) -> Result<Readings, HardwareError>;
}

#[async_trait]
pub trait Camera: Send + Sync {
    /// Take one still and write it to `dir`/`filename`, creating `dir` if it is missing.
    ///
    /// The Pi's camera tools are command-line programs whose only output is a file on disk, so
    /// the image is never handed back in memory: the caller names the destination, and the
    /// driver's success is the file's existence.
    ///
    /// # Errors
    /// [`HardwareError::Unavailable`] when no camera is attached; [`HardwareError::Io`] when the
    /// capture command runs and fails, or the destination cannot be written.
    async fn capture(&self, dir: &Path, filename: &str) -> Result<(), HardwareError>;
}

/// The `settings.hardware` block, deserialized once at boot.
///
/// Every field defaults, so an absent `settings:` block (or an absent `hardware:` key) boots as
/// `driver: auto` rather than failing.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct HardwareConfig {
    /// Which implementation to build.
    pub driver: Driver,
}

/// The implementations a build can be asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Driver {
    /// The Pi drivers when this build has them, the mock everywhere else. The default.
    #[default]
    Auto,
    /// The mock, on any host — including a Pi whose hardware is being serviced.
    Mock,
    /// The Pi drivers, or a boot error in a build that has none.
    Pi,
}

impl HardwareConfig {
    /// # Errors
    /// When the `settings:` block exists but does not match this schema (for example an unknown
    /// `driver` value) — a typo must fail the boot, not silently fall back to `auto`.
    pub fn from_context(config: &Config) -> Result<Self> {
        config.settings()
    }
}

/// The three subsystems, built once per process by `Hooks::after_context`.
pub struct Hardware {
    pub fan: Arc<dyn Fan>,
    pub environment: Arc<dyn EnvironmentSensor>,
    pub camera: Arc<dyn Camera>,
}

impl Hardware {
    /// The mock bundle: the three subsystems with nothing behind them.
    #[must_use]
    pub fn mock() -> Self {
        Self {
            fan: Arc::new(mock::MockFan::new()),
            environment: Arc::new(mock::MockEnvironmentSensor::new()),
            camera: Arc::new(mock::MockCamera),
        }
    }

    /// Builds the bundle named by [`HardwareConfig::driver`].
    ///
    /// # Errors
    /// A boot error when `driver: pi` names drivers this build does not contain; the app refuses
    /// to start rather than silently driving nothing.
    pub fn from_config(config: &HardwareConfig) -> Result<Self> {
        match config.driver {
            // When `src/hardware/pi.rs` lands, this arm becomes
            // `#[cfg(all(target_os = "linux", target_arch = "aarch64"))] Driver::Pi | Driver::Auto
            // => Self::pi(config)`, with the mock arm cfg'd to `not(...)`.
            Driver::Mock | Driver::Auto => {
                tracing::info!(driver = "mock", "hardware");
                Ok(Self::mock())
            }
            Driver::Pi => Err(Error::string(
                "hardware.driver: pi requires a build with the Raspberry Pi drivers \
                 (target_os = linux, target_arch = aarch64); use `auto` or `mock`",
            )),
        }
    }

    /// The bundle `Hooks::after_context` registered — the same accessor shape as
    /// `crate::controllers::monitor`.
    ///
    /// # Errors
    /// [`Error::InternalServerError`] when the app did not boot through `Hooks::after_context`
    /// (every boot path, including `cargo loco task`, does).
    pub fn of(ctx: &AppContext) -> Result<Arc<Self>> {
        ctx.shared_store
            .get::<Arc<Self>>()
            .ok_or(Error::InternalServerError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory under the system temp dir that no other test shares. The mock camera writes
    /// real files, so each test gets its own.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past the epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("sagan-{tag}-{}-{nanos}", std::process::id()))
    }

    #[tokio::test]
    async fn auto_and_mock_build_driving_bundles() {
        for driver in [Driver::Auto, Driver::Mock] {
            let config = HardwareConfig { driver };
            let hardware = Hardware::from_config(&config).expect("both drivers build here");

            hardware.fan.set_speed(30).await.expect("30% is in range");
            assert_eq!(hardware.fan.speed().await.expect("read back"), 30);
            hardware
                .environment
                .readings()
                .await
                .expect("the mock sensor always reads");

            let dir = scratch("bundle-camera");
            hardware
                .camera
                .capture(&dir, "frame.jpg")
                .await
                .expect("the mock camera always writes");
            let bytes = std::fs::read(dir.join("frame.jpg")).expect("the frame is on disk");
            assert!(bytes.starts_with(&[0xFF, 0xD8]));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn pi_driver_is_a_boot_error_in_this_build() {
        let config = HardwareConfig { driver: Driver::Pi };
        assert!(Hardware::from_config(&config).is_err());
    }

    #[test]
    fn an_unknown_driver_is_rejected() {
        assert!(
            serde_json::from_value::<HardwareConfig>(serde_json::json!({"driver": "gpio"}))
                .is_err()
        );
    }
}
