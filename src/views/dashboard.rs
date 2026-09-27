use serde::Serialize;

/// A dashboard status tile's severity — the colour of its badge.
#[derive(Debug, Clone, Copy)]
enum StatusLevel {
    /// Everything is as it should be. Green badge.
    Good,
    /// Worth watching, but not failing. Yellow badge.
    Ok,
    /// Needs attention. Red badge.
    Problem,
}

impl StatusLevel {
    /// The badge's Tailwind classes: green, yellow or red, in that order.
    const fn css(self) -> &'static str {
        match self {
            Self::Good => "bg-emerald-100 text-emerald-800",
            Self::Ok => "bg-amber-100 text-amber-800",
            Self::Problem => "bg-red-100 text-red-800",
        }
    }

    /// The badge's text.
    const fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Ok => "OK",
            Self::Problem => "Problem",
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
    /// Placeholder data for every tile.
    ///
    /// Each value stands in for a real reading — a Raspberry Pi capture, the disk's free
    /// space, the last backup, the last reclaim — none of which is wired up yet. Only the
    /// values are temporary; `LiveView` and `StatusItemView` are the shapes the template
    /// renders, and the seam real readings will be built into.
    #[must_use]
    pub fn placeholder() -> Self {
        Self {
            live: LiveView {
                captured_at_utc: "2026-09-27T12:00:00Z".to_string(),
                captured_at: "2026-09-27 12:00:00 UTC".to_string(),
            },
            status: vec![
                StatusItemView::new("Captures", StatusLevel::Good, "14 captures today"),
                StatusItemView::new("Disk", StatusLevel::Ok, "5 GB free"),
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

    #[test]
    fn every_status_level_gets_its_own_badge_colour() {
        let good = StatusLevel::Good.css();
        let ok = StatusLevel::Ok.css();
        let problem = StatusLevel::Problem.css();

        assert!(good.contains("emerald"), "good should be green: {good}");
        assert!(ok.contains("amber"), "ok should be yellow: {ok}");
        assert!(problem.contains("red"), "problem should be red: {problem}");
        // A tile that is good, ok and a problem must not look the same.
        assert_ne!(good, ok);
        assert_ne!(ok, problem);
        assert_ne!(good, problem);
    }
}
