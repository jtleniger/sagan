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

/// The simulated frame's size in pixels — the constant's real dimensions.
const FRAME_WIDTH: u32 = 64;
const FRAME_HEIGHT: u32 = 48;

/// The frame every mock capture returns: a 64x48 JPEG (350 bytes), embedded so a host with no
/// camera still exercises the capture path. Regenerate with
/// `magick -size 64x48 gradient:'#334155'-'#94a3b8' -quality 60 jpeg:-` (see the plan's Appendix B
/// for the bytes and their SHA-256).
const FRAME_JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x0D, 0x09, 0x0A, 0x0B, 0x0A, 0x08, 0x0D,
    0x0B, 0x0A, 0x0B, 0x0E, 0x0E, 0x0D, 0x0F, 0x13, 0x20, 0x15, 0x13, 0x12, 0x12, 0x13, 0x27, 0x1C,
    0x1E, 0x17, 0x20, 0x2E, 0x29, 0x31, 0x30, 0x2E, 0x29, 0x2D, 0x2C, 0x33, 0x3A, 0x4A, 0x3E, 0x33,
    0x36, 0x46, 0x37, 0x2C, 0x2D, 0x40, 0x57, 0x41, 0x46, 0x4C, 0x4E, 0x52, 0x53, 0x52, 0x32, 0x3E,
    0x5A, 0x61, 0x5A, 0x50, 0x60, 0x4A, 0x51, 0x52, 0x4F, 0xFF, 0xDB, 0x00, 0x43, 0x01, 0x0E, 0x0E,
    0x0E, 0x13, 0x11, 0x13, 0x26, 0x15, 0x15, 0x26, 0x4F, 0x35, 0x2D, 0x35, 0x4F, 0x4F, 0x4F, 0x4F,
    0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F,
    0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F,
    0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0x4F, 0xFF, 0xC0,
    0x00, 0x11, 0x08, 0x00, 0x30, 0x00, 0x40, 0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11,
    0x01, 0xFF, 0xC4, 0x00, 0x16, 0x00, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x06, 0xFF, 0xC4, 0x00, 0x15, 0x10, 0x01, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x13,
    0xFF, 0xC4, 0x00, 0x15, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0xFF, 0xC4, 0x00, 0x14, 0x11, 0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xDA, 0x00,
    0x0C, 0x03, 0x01, 0x00, 0x02, 0x11, 0x03, 0x11, 0x00, 0x3F, 0x00, 0xCD, 0xCC, 0x9A, 0xB9, 0x93,
    0x5D, 0x24, 0x93, 0x26, 0xAE, 0x64, 0xC1, 0x24, 0xC9, 0xAB, 0x99, 0x30, 0x49, 0x32, 0x6A, 0xE6,
    0x4C, 0x15, 0x4C, 0x9A, 0xA9, 0x93, 0x04, 0xB3, 0x26, 0xAA, 0x64, 0xC1, 0x2C, 0xC9, 0xAA, 0x99,
    0x30, 0x4B, 0x32, 0x6A, 0xA6, 0x4C, 0x15, 0xCC, 0x9A, 0xA9, 0x93, 0x04, 0xB3, 0x26, 0xAA, 0x64,
    0xC1, 0x2C, 0xC9, 0xAA, 0x99, 0x30, 0x4B, 0x32, 0x6A, 0xA6, 0x4C, 0x1F, 0xFF, 0xD9,
];

/// A [`Camera`] with no ribbon behind it.
///
/// It returns the same JPEG frame on every call with a fresh server timestamp: enough for the
/// capture path, the dashboard, and the `hardware_check` task to run on a laptop, in CI, or on a
/// Pi whose camera is being serviced. A host that *has* a camera which cannot be used is the Pi
/// driver's `HardwareError::Unavailable` / `HardwareError::Io`, not this type.
pub struct MockCamera;

#[async_trait]
impl Camera for MockCamera {
    async fn capture(&self) -> Result<Capture, HardwareError> {
        Ok(Capture {
            jpeg: FRAME_JPEG.to_vec(),
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            taken_at_ms: chrono::Utc::now().timestamp_millis(),
        })
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
    async fn camera_returns_a_jpeg_frame() {
        let frame = MockCamera
            .capture()
            .await
            .expect("the mock camera always frames");

        assert_eq!(frame.jpeg.len(), 350);
        assert!(
            frame.jpeg.starts_with(&[0xFF, 0xD8]),
            "a JPEG starts with SOI"
        );
        assert!(frame.jpeg.ends_with(&[0xFF, 0xD9]), "and ends with EOI");
        assert_eq!((frame.width, frame.height), (FRAME_WIDTH, FRAME_HEIGHT));

        let now_ms = chrono::Utc::now().timestamp_millis();
        assert!(
            (frame.taken_at_ms - now_ms).abs() < 1_000,
            "taken_at_ms {} is not within a second of {now_ms}",
            frame.taken_at_ms
        );
    }
}
