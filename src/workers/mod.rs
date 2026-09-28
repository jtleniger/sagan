//! The background workers this app registers, and the list the `/jobs` page renders.
//!
//! Each worker owns its own page entry (`CaptureWorker::entry`), so a worker added here
//! cannot be registered in `Hooks::connect_workers` and stay invisible on the page.

pub mod capture;
pub mod heartbeat;

use crate::workers::{capture::CaptureWorker, heartbeat::HeartbeatWorker};

/// One worker the app registers, as the `/jobs` page shows it.
#[derive(Debug, Clone)]
pub struct WorkerEntry {
    /// The name the queue stores this worker's jobs under — its type name, `CamelCased`.
    pub name: String,
    /// The provider queue the worker's jobs carry; `None` for the provider's default.
    pub queue: Option<String>,
    /// The tags attached to every job this worker enqueues.
    pub tags: Vec<String>,
    /// Why the worker exists, one line.
    pub detail: &'static str,
}

/// Every worker `Hooks::connect_workers` registers, in the order the page lists them.
#[must_use]
pub fn configured() -> Vec<WorkerEntry> {
    vec![CaptureWorker::entry(), HeartbeatWorker::entry()]
}
