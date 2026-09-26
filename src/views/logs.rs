use chrono::SecondsFormat;
use serde::Serialize;

use crate::logs::{LogEntry, LogPage, LogSource, LogsQuery};

/// One row of the `/logs` table.
#[derive(Debug, Serialize)]
pub struct LogEntryView {
    /// `2026-09-26 10:00:00.123`, UTC — the text a reader without JavaScript keeps.
    pub timestamp: String,
    /// The same instant as `2026-09-26T10:00:00.123Z`, for the `<time datetime>` that
    /// `static/js/logs.js` renders in the reader's own zone.
    pub timestamp_utc: String,
    pub level: String,
    pub level_css: String,
    pub target: String,
    pub message: String,
    /// The record's remaining structured fields, as compact JSON; empty when there are none.
    pub extra: String,
}

/// Everything `assets/views/logs/index.html` reads: the page of records, the pager's URLs
/// and the filter form's echo values.
#[derive(Debug, Serialize)]
pub struct LogsPageView {
    pub entries: Vec<LogEntryView>,
    pub page: u64,
    pub total_pages: u64,
    pub total_items: u64,
    /// Older records exist on disk but are outside the scanned window.
    pub truncated: bool,
    /// The directory being read, so the page can name it — including when it is empty.
    pub dir: String,
    pub enabled: bool,
    /// The `<select>`'s value: `all` or a level keyword.
    pub level_value: String,
    /// The `datetime-local` filter boundaries, `YYYY-MM-DDTHH:MM:SS`, UTC, empty when that
    /// end of the range is unset. `assets/static/js/logs.js` shows them on the reader's own
    /// clock and converts what the reader typed back to UTC before the form is submitted.
    pub from_value: String,
    pub to_value: String,
    pub prev_url: Option<String>,
    pub next_url: Option<String>,
    pub first_url: Option<String>,
    pub last_url: Option<String>,
}

impl LogsPageView {
    /// `page` has already been clamped to the range by [`crate::logs::read_page`], so the
    /// pager's links are derived from the page that was actually rendered, not the one that
    /// was asked for.
    #[must_use]
    pub fn new(source: &LogSource, page: LogPage, query: &LogsQuery) -> Self {
        let LogPage {
            entries,
            page: current,
            total_pages,
            total_items,
            truncated,
        } = page;

        Self {
            entries: entries.iter().map(LogEntryView::from).collect(),
            page: current,
            total_pages,
            total_items,
            truncated,
            dir: source.dir.display().to_string(),
            enabled: source.enabled,
            level_value: query
                .min_level
                .map_or_else(|| "all".to_string(), |level| level.param().to_string()),
            from_value: query
                .from
                .map(|from| from.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default(),
            to_value: query
                .to
                .map(|to| to.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default(),
            prev_url: (current > 1).then(|| page_url(current - 1, query)),
            next_url: (current < total_pages).then(|| page_url(current + 1, query)),
            first_url: (current > 1).then(|| page_url(1, query)),
            last_url: (current < total_pages).then(|| page_url(total_pages, query)),
        }
    }
}

impl From<&LogEntry> for LogEntryView {
    fn from(entry: &LogEntry) -> Self {
        Self {
            timestamp: entry.timestamp.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
            timestamp_utc: entry.timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
            level: entry.level.label().to_string(),
            level_css: entry.level.css().to_string(),
            target: entry.target.clone(),
            message: entry.message.clone(),
            extra: entry.extra.clone(),
        }
    }
}

/// A pager link that keeps the active filters.
///
/// No percent-encoding is needed: every value comes from a fixed alphabet — a level keyword
/// or a canonical `YYYY-MM-DDTHH:MM:SS` datetime — and Tera escapes the attribute anyway.
fn page_url(page: u64, query: &LogsQuery) -> String {
    let mut url = format!("/logs?page={page}");

    if let Some(level) = query.min_level {
        url.push_str("&level=");
        url.push_str(level.param());
    }
    if let Some(from) = query.from {
        url.push_str("&from=");
        url.push_str(&from.format("%Y-%m-%dT%H:%M:%S").to_string());
    }
    if let Some(to) = query.to {
        url.push_str("&to=");
        url.push_str(&to.format("%Y-%m-%dT%H:%M:%S").to_string());
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::LogLevel;
    use chrono::{DateTime, Utc};

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("the fixture timestamp should parse")
            .with_timezone(&Utc)
    }

    fn source() -> LogSource {
        LogSource {
            dir: "logs".into(),
            prefix: "sagan".to_string(),
            suffix: "log".to_string(),
            enabled: true,
        }
    }

    fn page(total_items: u64, page: u64) -> LogPage {
        LogPage {
            entries: Vec::new(),
            page,
            total_pages: total_items.div_ceil(crate::logs::PAGE_SIZE).max(1),
            total_items,
            truncated: false,
        }
    }

    #[test]
    fn logs_page_view_links_keep_the_filters() {
        let query = LogsQuery {
            page: 2,
            min_level: Some(LogLevel::Warn),
            from: Some(at("2026-09-26T10:00:00Z")),
            to: None,
        };

        let view = LogsPageView::new(&source(), page(120, 2), &query);

        assert_eq!(view.level_value, "warn");
        assert_eq!(view.from_value, "2026-09-26T10:00:00");
        assert_eq!(view.to_value, "");
        assert_eq!(
            view.prev_url.as_deref(),
            Some("/logs?page=1&level=warn&from=2026-09-26T10:00:00")
        );
        assert_eq!(
            view.next_url.as_deref(),
            Some("/logs?page=3&level=warn&from=2026-09-26T10:00:00")
        );
        assert_eq!(
            view.first_url.as_deref(),
            Some("/logs?page=1&level=warn&from=2026-09-26T10:00:00")
        );
        assert_eq!(
            view.last_url.as_deref(),
            Some("/logs?page=3&level=warn&from=2026-09-26T10:00:00")
        );
    }

    #[test]
    fn logs_page_view_has_no_pager_links_on_a_single_page() {
        let view = LogsPageView::new(&source(), page(3, 1), &LogsQuery::default());

        assert_eq!(view.level_value, "all");
        assert_eq!(view.from_value, "");
        assert_eq!(view.to_value, "");
        assert_eq!(view.prev_url, None);
        assert_eq!(view.next_url, None);
        assert_eq!(view.first_url, None);
        assert_eq!(view.last_url, None);
    }

    #[test]
    fn logs_page_view_formats_the_row() {
        let entry = LogEntry {
            timestamp: at("2026-09-26T10:00:00.123Z"),
            level: LogLevel::Warn,
            target: "sagan::test".to_string(),
            message: "disk almost full".to_string(),
            extra: r#"{"k":"v"}"#.to_string(),
        };
        let view = LogEntryView::from(&entry);

        assert_eq!(view.timestamp, "2026-09-26 10:00:00.123");
        assert_eq!(view.timestamp_utc, "2026-09-26T10:00:00.123Z");
        assert_eq!(view.level, "WARN");
        assert_eq!(view.level_css, "bg-amber-100 text-amber-800");
        assert_eq!(view.target, "sagan::test");
        assert_eq!(view.message, "disk almost full");
        assert_eq!(view.extra, r#"{"k":"v"}"#);
    }
}
