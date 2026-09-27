//! The subsystems with no hardware behind them: what a laptop, CI, and `driver: mock` on a Pi
//! run.
//!
//! Compiled for every host — not a test-only double: on a machine without the peripherals this is
//! the running implementation, so the app boots and pages render.

use std::{
    sync::atomic::{AtomicU8, Ordering},
    time::Instant,
};

use async_trait::async_trait;

use super::{Camera, Capture, EnvironmentSensor, Fan, HardwareError, Readings, MAX_SPEED_PERCENT};

/// A [`Fan`] with no wire behind it: it remembers the duty it was last given, and starts stopped.
pub struct MockFan {
    /// The commanded duty. An atomic, not a `Mutex`: `set_speed(&self)` needs no lock and the
    /// trait object stays `Sync`.
    speed: AtomicU8,
}

impl MockFan {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            speed: AtomicU8::new(0),
        }
    }
}

impl Default for MockFan {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fan for MockFan {
    async fn set_speed(&self, percent: u8) -> Result<(), HardwareError> {
        if percent > MAX_SPEED_PERCENT {
            return Err(HardwareError::InvalidSpeed(percent));
        }
        self.speed.store(percent, Ordering::Relaxed);
        Ok(())
    }

    async fn speed(&self) -> Result<u8, HardwareError> {
        Ok(self.speed.load(Ordering::Relaxed))
    }
}

/// An [`EnvironmentSensor`] with no chip behind it.
///
/// It reports a plausible indoor reading that drifts on a slow sine of the time since the process
/// started, so successive samples differ and a chart drawn from them has a shape.
pub struct MockEnvironmentSensor {
    started: Instant,
}

impl MockEnvironmentSensor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Default for MockEnvironmentSensor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EnvironmentSensor for MockEnvironmentSensor {
    async fn readings(&self) -> Result<Readings, HardwareError> {
        let (celsius, humidity_percent, pressure_hpa) =
            simulated(self.started.elapsed().as_secs_f32());
        Ok(Readings {
            celsius,
            humidity_percent,
            pressure_hpa,
            taken_at_ms: chrono::Utc::now().timestamp_millis(),
        })
    }
}

/// The simulated `(celsius, humidity_percent, pressure_hpa)` at `elapsed_s` seconds after the
/// process started: 20.0..=24.0 °C, 35.0..=55.0 %, 1008.0..=1018.0 hPa. Pure, so its shape is
/// unit-tested without a clock; `simulated(0.0)` is exactly `(22.0, 45.0, 1013.0)`.
fn simulated(elapsed_s: f32) -> (f32, f32, f32) {
    (
        2.0f32.mul_add((elapsed_s / 600.0).sin(), 22.0),
        10.0f32.mul_add((elapsed_s / 900.0).sin(), 45.0),
        5.0f32.mul_add((elapsed_s / 1800.0).sin(), 1013.0),
    )
}

/// A [`Camera`] with no ribbon behind it.
///
/// Every capture reports [`HardwareError::Unavailable`] — the truth on a machine with no camera,
/// and the branch a consumer must handle on the Pi anyway (a Pi with no camera attached behaves
/// identically).
pub struct MockCamera;

#[async_trait]
impl Camera for MockCamera {
    async fn capture(&self) -> Result<Capture, HardwareError> {
        Err(HardwareError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fan_starts_stopped_and_remembers_the_duty_it_was_given() {
        let fan = MockFan::new();
        assert_eq!(fan.speed().await.expect("a mock fan always reads"), 0);

        fan.set_speed(60).await.expect("60% is in range");
        assert_eq!(fan.speed().await.expect("a mock fan always reads"), 60);

        fan.set_speed(0).await.expect("0% is in range");
        assert_eq!(fan.speed().await.expect("a mock fan always reads"), 0);
    }

    #[tokio::test]
    async fn fan_rejects_a_duty_above_full_scale() {
        let fan = MockFan::new();
        fan.set_speed(40).await.expect("40% is in range");

        assert_eq!(
            fan.set_speed(101).await,
            Err(HardwareError::InvalidSpeed(101))
        );
        assert_eq!(
            fan.speed().await.expect("a mock fan always reads"),
            40,
            "a rejected duty must not change the one the fan is running"
        );
    }

    #[test]
    fn simulated_readings_stay_plausible() {
        assert_eq!(simulated(0.0), (22.0, 45.0, 1013.0));

        // An hour of samples, five seconds apart: the sine terms must stay inside the ranges
        // documented on `simulated`.
        for step in 0..=720u16 {
            let (celsius, humidity, pressure) = simulated(f32::from(step) * 5.0);
            assert!((20.0..=24.0).contains(&celsius), "celsius {celsius}");
            assert!((35.0..=55.0).contains(&humidity), "humidity {humidity}");
            assert!((1008.0..=1018.0).contains(&pressure), "pressure {pressure}");
        }
    }

    #[tokio::test]
    async fn readings_carry_a_server_timestamp() {
        let reading = MockEnvironmentSensor::new()
            .readings()
            .await
            .expect("the mock sensor always reads");

        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(
            (reading.taken_at_ms - now_ms).abs() < 1_000,
            "taken_at_ms {} is not within a second of {now_ms}",
            reading.taken_at_ms
        );
    }

    #[tokio::test]
    async fn camera_reports_unavailable() {
        assert!(matches!(
            MockCamera.capture().await,
            Err(HardwareError::Unavailable)
        ));
    }
}
