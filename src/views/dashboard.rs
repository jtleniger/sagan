use serde::Serialize;

use crate::{
    monitor::{DiskSample, SystemSample},
    views::system::{bytes, percent},
};

/// A disk at or above these limits is what the `Disk` tile calls out. The percentages are the
/// ones the System page's bars already use, so "nearly full" means the same thing on both pages;
/// the byte floors catch a small volume that is at a modest percentage while having almost no
/// room left for captures.
const DISK_PROBLEM_PERCENT: f32 = 90.0;
const DISK_OK_PERCENT: f32 = 75.0;
const DISK_PROBLEM_FREE_BYTES: u64 = 1024 * 1024 * 1024;
const DISK_OK_FREE_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// A dashboard status tile's severity — the colour of its badge.
#[derive(Debug, Clone, Copy)]
enum StatusLevel {
    /// Everything is as it should be. Green badge.
    Good,
    /// Worth watching, but not failing. Yellow badge.
    Ok,
    /// Needs attention. Red badge.
    Problem,
    /// No reading was taken, so no state can be claimed. Slate badge.
    Unknown,
}

impl StatusLevel {
    /// The badge's Tailwind classes: green, yellow or red, in that order.
    const fn css(self) -> &'static str {
        match self {
            Self::Good => "bg-emerald-100 text-emerald-800",
            Self::Ok => "bg-amber-100 text-amber-800",
            Self::Problem => "bg-red-100 text-red-800",
            Self::Unknown => "bg-slate-100 text-slate-600",
        }
    }

    /// The badge's text.
    const fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Ok => "OK",
            Self::Problem => "Problem",
            Self::Unknown => "Unknown",
        }
    }
}

/// One tile in the dashboard's `Status` section.
#[derive(Debug, Serialize)]
pub struct StatusItemView {
    /// Display name, e.g. `Captures`.
    pub name: String,
    /// The badge's text, e.g. `Good`.
    pub status: String,
    /// The badge's Tailwind classes: green, yellow or red.
    pub status_css: String,
    /// The line under the name, e.g. `14 captures today`.
    pub text: String,
}

impl StatusItemView {
    fn new(name: &str, level: StatusLevel, text: &str) -> Self {
        Self {
            name: name.to_string(),
            status: level.label().to_string(),
            status_css: level.css().to_string(),
            text: text.to_string(),
        }
    }
}

/// The `Disk` tile: the room left on the volume the binary runs from.
fn disk_status(disk: Option<&DiskSample>) -> StatusItemView {
    let Some(disk) = disk else {
        return StatusItemView::new(
            "Disk",
            StatusLevel::Unknown,
            "No disk reported by this host",
        );
    };

    let used_percent = percent(disk.used_bytes, disk.total_bytes);
    let level =
        if used_percent >= DISK_PROBLEM_PERCENT || disk.available_bytes < DISK_PROBLEM_FREE_BYTES {
            StatusLevel::Problem
        } else if used_percent >= DISK_OK_PERCENT || disk.available_bytes < DISK_OK_FREE_BYTES {
            StatusLevel::Ok
        } else {
            StatusLevel::Good
        };

    StatusItemView::new(
        "Disk",
        level,
        &format!(
            "{} free of {}",
            bytes(disk.available_bytes),
            bytes(disk.total_bytes)
        ),
    )
}

/// The dashboard's `Live` section: the most recent camera capture.
#[derive(Debug, Serialize)]
pub struct LiveView {
    /// The capture's instant as RFC 3339 UTC — the `datetime` of the `<time>` element
    /// `static/js/local-time.js` renders in the reader's own zone.
    pub captured_at_utc: String,
    /// The same instant as UTC text — the fallback a reader without JavaScript keeps.
    pub captured_at: String,
}

/// Everything `assets/views/dashboard/index.html` reads beyond the shell's
/// `user`/`active`.
#[derive(Debug, Serialize)]
pub struct DashboardView {
    pub live: LiveView,
    pub status: Vec<StatusItemView>,
}

impl DashboardView {
    /// The dashboard's tiles. `Disk` is the live reading from the shared monitor; the other
    /// three, and `LiveView`'s capture timestamp, still stand in for readings that are not
    /// wired up yet.
    #[must_use]
    pub fn from_sample(sample: &SystemSample) -> Self {
        Self {
            live: LiveView {
                captured_at_utc: "2026-09-27T12:00:00Z".to_string(),
                captured_at: "2026-09-27 12:00:00 UTC".to_string(),
            },
            status: vec![
                StatusItemView::new("Captures", StatusLevel::Good, "14 captures today"),
                disk_status(sample.disk.as_ref()),
                // A stale backup is what a red tile looks like; the value stands in for a
                // reading that would compare the last backup's age against a threshold.
                StatusItemView::new(
                    "Backup",
                    StatusLevel::Problem,
                    "Last backed up captures at 2026-09-26 03:00:00 UTC",
                ),
                StatusItemView::new(
                    "Reclaim",
                    StatusLevel::Good,
                    "Last reclaimed space at 2026-09-27 04:30:00 UTC; 1.2 GB freed",
                ),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    /// A `DiskSample` with the fields the tile reads.
    fn disk(total: u64, available: u64) -> DiskSample {
        DiskSample {
            mount_point: "/".to_string(),
            file_system: "ext4".to_string(),
            total_bytes: total,
            used_bytes: total - available,
            available_bytes: available,
        }
    }

    #[test]
    fn every_status_level_gets_its_own_badge_colour() {
        let good = StatusLevel::Good.css();
        let ok = StatusLevel::Ok.css();
        let problem = StatusLevel::Problem.css();
        let unknown = StatusLevel::Unknown.css();

        assert!(good.contains("emerald"), "good should be green: {good}");
        assert!(ok.contains("amber"), "ok should be yellow: {ok}");
        assert!(problem.contains("red"), "problem should be red: {problem}");
        assert!(
            unknown.contains("slate"),
            "unknown should be grey: {unknown}"
        );
        // A tile that is good, ok and a problem must not look the same.
        assert_ne!(good, ok);
        assert_ne!(ok, problem);
        assert_ne!(good, problem);
    }

    #[test]
    fn disk_status_shows_the_free_and_total_space() {
        let item = disk_status(Some(&disk(24 * GIB, 10 * GIB)));

        assert_eq!(item.name, "Disk");
        assert_eq!(item.status, "Good");
        assert_eq!(item.text, "10.0 GiB free of 24.0 GiB");
    }

    #[test]
    fn disk_status_warns_at_three_quarters_used() {
        // 750 GiB of 1000 GiB used: the percentage alone decides, the free-space floor cannot.
        let item = disk_status(Some(&disk(1000 * GIB, 250 * GIB)));

        assert_eq!(item.status, "OK");
    }

    #[test]
    fn disk_status_fails_at_ninety_percent_used() {
        let item = disk_status(Some(&disk(1000 * GIB, 100 * GIB)));

        assert_eq!(item.status, "Problem");
    }

    #[test]
    fn disk_status_fails_on_a_large_volume_with_almost_no_room_left() {
        // 0.05% used is not the point: a capture will not fit.
        let item = disk_status(Some(&disk(1024 * GIB, 512 * MIB)));

        assert_eq!(item.status, "Problem");
        assert_eq!(item.text, "512.0 MiB free of 1024.0 GiB");
    }

    #[test]
    fn disk_status_warns_on_a_volume_with_little_room_left() {
        // 70% used, so the percentage alone would say Good: the free-space floor is the
        // only thing that makes this worth watching.
        let item = disk_status(Some(&disk(10 * GIB, 3 * GIB)));

        assert_eq!(item.status, "OK");
    }

    #[test]
    fn disk_status_claims_no_state_without_a_reading() {
        let item = disk_status(None);

        assert_eq!(item.name, "Disk");
        assert_eq!(item.status, "Unknown");
        assert_eq!(item.text, "No disk reported by this host");
    }
}
