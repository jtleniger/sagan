//! The hardware bundle `Hooks::after_context` registers: on this machine every subsystem in it
//! is the mock, which is exactly what `settings.hardware.driver: auto` resolves to here.

use loco_rs::testing::prelude::*;
use sagan::{
    app::App,
    hardware::{Hardware, HardwareError, MAX_SPEED_PERCENT},
};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn boot_registers_a_bundle_the_pages_can_drive() {
    let boot = boot_test::<App>().await.expect("the app should boot");
    let hardware = Hardware::of(&boot.app_context).expect("after_context must register the bundle");

    // The fan round-trips a duty; a real PWM fan would not, which is why this test is only
    // meaningful on a host without one.
    hardware
        .fan
        .set_speed(MAX_SPEED_PERCENT / 2)
        .await
        .expect("the mock fan accepts a duty in range");
    assert_eq!(hardware.fan.speed().await.expect("read back"), 50);

    let reading = hardware
        .environment
        .readings()
        .await
        .expect("the mock sensor always reads");
    assert!((20.0..=24.0).contains(&reading.celsius));
    assert!((35.0..=55.0).contains(&reading.humidity_percent));
    assert!((1008.0..=1018.0).contains(&reading.pressure_hpa));

    // No camera on this host, and the mock says so rather than pretending.
    assert!(matches!(
        hardware.camera.capture().await,
        Err(HardwareError::Unavailable)
    ));
}
