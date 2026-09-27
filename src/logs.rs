//! The `/logs` page's model: reads the application's own log records straight from the
//! files Loco's file appender writes.
//!
//! Records are listed newest first, filterable by minimum level and by a UTC time range.
//! There is no database table and no ingestion step behind this page — the appender
//! (`logger.file_appender` in `config/*.yaml`) is the writer, these files are the store, and
//! `src/controllers/logs.rs` is the only caller. The
//! reader therefore has to be forgiving: a request must never fail because a log directory
//! is missing, a file rotated mid-read, or a line was flushed half-way. Every one of those
//! degrades to "this record is not listed", never to an error response.
//!
//! The work is bounded per request, because a deployment can hold days of debug logs:
//! the newest [`MAX_FILES`] files are scanned, only the last [`TAIL_BYTES_PER_FILE`] of each,
//! and at most [`MAX_ENTRIES`] matching records are collected. When that last bound bites,
//! the page says so through [`LogPage::truncated`] instead of silently dropping history.
//!
//! Deliberately does not glob-import `loco_rs::prelude`: its `DateTime` is
//! `chrono::NaiveDateTime`.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use loco_rs::config::Config;
use serde::Deserialize;
use serde_json::Value;

/// Log records listed per page.
pub const PAGE_SIZE: u64 = 50;

/// Bytes read from the end of each file: 1 MiB, because a request must not read a whole
/// day of debug logs to render the newest fifty records.
const TAIL_BYTES_PER_FILE: u64 = 1_048_576;

/// Newest files by name, so one request does not walk a month of rotated files. The
/// appender prunes to `max_log_files` anyway; this is the same bound applied to reads.
const MAX_FILES: usize = 20;

/// Parse/sort/memory bound per request. Reached only on a very busy deployment.
const MAX_ENTRIES: usize = 10_000;

/// A log record's severity. The variant order *is* the severity order, which is what makes
/// "minimum level" a plain `<` comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// The level string as it appears in a JSON log line; unknown or absent means "skip".
    fn from_json(value: &str) -> Option<Self> {
        [
            Self::Trace,
            Self::Debug,
            Self::Info,
            Self::Warn,
            Self::Error,
        ]
        .into_iter()
        .find(|level| level.label().eq_ignore_ascii_case(value))
    }

    /// The query-string spelling, also the `<select>` value.
    #[must_use]
    pub const fn param(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    /// The `level` field's value in a JSON log line.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    /// The level badge's Tailwind classes.
    #[must_use]
    pub const fn css(self) -> &'static str {
        match self {
            Self::Trace => "bg-slate-100 text-slate-500",
            Self::Debug => "bg-slate-200 text-slate-600",
            Self::Info => "bg-blue-100 text-blue-800",
            Self::Warn => "bg-amber-100 text-amber-800",
            Self::Error => "bg-red-100 text-red-800",
        }
    }

    /// Parses the `level` query parameter. `None` for an absent, empty, `all` or
    /// unrecognised value — an unreadable filter is ignored, never a 400.
    #[must_use]
    pub fn parse_param(value: &str) -> Option<Self> {
        [
            Self::Trace,
            Self::Debug,
            Self::Info,
            Self::Warn,
            Self::Error,
        ]
        .into_iter()
        .find(|level| level.param() == value)
    }
}

/// The query-string contract of `GET /logs`.
#[derive(Debug, Clone, Deserialize)]
pub struct LogsParams {
    pub page: Option<u64>,
    pub level: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// [`LogsParams`] after parsing: the filter and the page the reader actually uses.
#[derive(Debug, Clone, Default)]
pub struct LogsQuery {
    /// 1-based; 0 becomes 1.
    pub page: u64,
    /// Inclusive minimum severity; `None` means every level.
    pub min_level: Option<LogLevel>,
    /// Inclusive lower bound.
    pub from: Option<DateTime<Utc>>,
    /// Inclusive of the whole selected second.
    pub to: Option<DateTime<Utc>>,
}

impl LogsQuery {
    /// Everything a hand-typed URL can put here that is not a recognised value is dropped,
    /// so a partial or stale link still renders the logs.
    #[must_use]
    pub fn from_params(params: &LogsParams) -> Self {
        Self {
            page: params.page.unwrap_or(1).max(1),
            min_level: params.level.as_deref().and_then(LogLevel::parse_param),
            from: params.from.as_deref().and_then(parse_datetime_local),
            to: params.to.as_deref().and_then(parse_datetime_local),
        }
    }
}

/// One parsed log record.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    /// The remaining structured fields, as compact JSON; empty when there are none.
    pub extra: String,
}

/// One rendered page of records, with the totals the pager needs.
#[derive(Debug, Clone)]
pub struct LogPage {
    pub entries: Vec<LogEntry>,
    pub page: u64,
    pub total_pages: u64,
    pub total_items: u64,
    /// The [`MAX_ENTRIES`] bound was reached, so older matching records exist but are not
    /// listed.
    pub truncated: bool,
}

/// Where the records are, taken from the same config block Loco's file appender writes
/// with — so the writer and this reader cannot disagree about the directory or the naming.
#[derive(Debug, Clone)]
pub struct LogSource {
    pub dir: PathBuf,
    pub prefix: String,
    pub suffix: String,
    pub enabled: bool,
}

impl LogSource {
    /// `logger.file_appender` is the single source of truth for where the logs are; an
    /// absent block means `./logs`, no name filter, and not enabled.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let appender = config.logger.file_appender.as_ref();

        Self {
            dir: appender
                .and_then(|appender| appender.dir.clone())
                .map_or_else(|| PathBuf::from("./logs"), PathBuf::from),
            prefix: appender
                .and_then(|appender| appender.filename_prefix.clone())
                .unwrap_or_default(),
            suffix: appender
                .and_then(|appender| appender.filename_suffix.clone())
                .unwrap_or_default(),
            enabled: appender.is_some_and(|appender| appender.enable),
        }
    }
}

/// Reads the newest matching records, newest first, and slices out the requested page.
///
/// The line format on stdout is irrelevant here: only the appender's `json` lines are read,
/// and a line that is not one is skipped.
#[must_use]
pub fn read_page(source: &LogSource, query: &LogsQuery) -> LogPage {
    // "To" names a second, not an instant: `10:00:01` has to include everything up to
    // `10:00:01.999…`.
    let to_exclusive = query.to.map(|to| to + Duration::seconds(1));
    let mut entries = Vec::new();
    let mut truncated = false;

    for path in newest_files(source, MAX_FILES) {
        let text = read_tail(&path, TAIL_BYTES_PER_FILE);
        // Within one file the appender writes oldest -> newest, so walk backwards to reach
        // the newest records before the entry budget can be spent.
        for line in text.lines().rev() {
            let Some(entry) = parse_line(line) else {
                continue;
            };
            if query.min_level.is_some_and(|min| entry.level < min) {
                continue;
            }
            if query.from.is_some_and(|from| entry.timestamp < from) {
                continue;
            }
            if to_exclusive.is_some_and(|to| entry.timestamp >= to) {
                continue;
            }

            entries.push(entry);
            if entries.len() == MAX_ENTRIES {
                truncated = true;
                break;
            }
        }
        if truncated {
            break;
        }
    }

    // Sorted by timestamp rather than by file position, so a record written just before
    // midnight still lands on the right side of its neighbour in the next file.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));

    let total_items = entries.len() as u64;
    let total_pages = total_items.div_ceil(PAGE_SIZE).max(1);
    let page = query.page.clamp(1, total_pages);
    let page_offset = usize::try_from((page - 1) * PAGE_SIZE).unwrap_or(usize::MAX);
    let mut page_entries = entries.split_off(page_offset.min(entries.len()));
    page_entries.truncate(usize::try_from(PAGE_SIZE).unwrap_or(usize::MAX));

    LogPage {
        entries: page_entries,
        page,
        total_pages,
        total_items,
        truncated,
    }
}

/// The log files to read, newest name first.
///
/// A missing or unreadable directory is an empty list, never an error: a log viewer must
/// not answer 500 just because this deployment has logging switched off.
fn newest_files(source: &LogSource, limit: usize) -> Vec<PathBuf> {
    let Ok(dir) = std::fs::read_dir(&source.dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && matches_file_name(path, &source.prefix, &source.suffix))
        .collect();

    // Rotation names carry an ISO date (`sagan.2026-09-26.log`), so the name orders
    // chronologically and no metadata read is needed.
    files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    files.truncate(limit);
    files
}

/// Whether a directory entry is one of the appender's files.
///
/// With no prefix and no suffix configured (the `dir: .` case) a name only qualifies when
/// it parses as a date — the same guard `tracing_appender` applies when pruning, and the
/// reason a database file in the same directory is never read as a log.
fn matches_file_name(path: &Path, prefix: &str, suffix: &str) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if prefix.is_empty() && suffix.is_empty() {
        return NaiveDate::parse_from_str(name, "%Y-%m-%d").is_ok();
    }

    name.starts_with(prefix) && name.ends_with(suffix)
}

/// The last `max_bytes` of a file, as lossy UTF-8.
///
/// Any failure is an empty string, and a read that began mid-file drops its first, partial
/// line. A line damaged in any other way is left for [`parse_line`] to reject.
fn read_tail(path: &Path, max_bytes: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };

    let len = file.metadata().map_or(0, |metadata| metadata.len());
    let truncated = len > max_bytes;
    if truncated && file.seek(SeekFrom::Start(len - max_bytes)).is_err() {
        return String::new();
    }

    let mut buf = Vec::new();
    // Whatever was read before an I/O error is still usable.
    let _ = file.read_to_end(&mut buf);

    if truncated {
        if let Some(newline) = buf.iter().position(|byte| *byte == b'\n') {
            buf.drain(..=newline);
        }
    }

    String::from_utf8_lossy(&buf).into_owned()
}

/// One JSON log line, or `None` when it is anything else: a foreign file, a mid-flush
/// partial line, or a record written before the appender switched to `format: json`.
fn parse_line(line: &str) -> Option<LogEntry> {
    let value: Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;

    let timestamp = DateTime::parse_from_rfc3339(object.get("timestamp")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    let level = LogLevel::from_json(object.get("level")?.as_str()?)?;

    let mut fields = object
        .get("fields")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let message = fields
        .remove("message")
        .and_then(|message| message.as_str().map(ToString::to_string))
        .unwrap_or_default();
    // A field-only event (`{"shaved":true}`) has no message but still shows its fields.
    let extra = if fields.is_empty() {
        String::new()
    } else {
        serde_json::to_string(&Value::Object(fields)).unwrap_or_default()
    };

    Some(LogEntry {
        timestamp,
        level,
        target: object
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        message,
        extra,
    })
}

/// `2026-09-26T10:00:00` (a `datetime-local` with `step="1"`) or `2026-09-26T10:00`
/// (minute granularity). Treated as UTC.
fn parse_datetime_local(value: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"))
        .ok()
        .map(|value| value.and_utc())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory no other test shares; each `write_log` call fills it.
    fn fixture_dir() -> PathBuf {
        std::env::temp_dir().join(format!("sagan-logs-{}", uuid::Uuid::new_v4()))
    }

    fn write_log(dir: &Path, name: &str, lines: &[String]) {
        std::fs::create_dir_all(dir).expect("the fixture dir should be creatable");
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(dir.join(name), body).expect("the fixture file should be writable");
    }

    fn line(ts: &str, level: &str, message: &str) -> String {
        format!(
            r#"{{"timestamp":"{ts}","level":"{level}","fields":{{"message":"{message}"}},"target":"sagan::test"}}"#
        )
    }

    fn source(dir: &Path) -> LogSource {
        LogSource {
            dir: dir.to_path_buf(),
            prefix: "sagan".to_string(),
            suffix: "log".to_string(),
            enabled: true,
        }
    }

    fn query(page: u64) -> LogsQuery {
        LogsQuery {
            page,
            ..LogsQuery::default()
        }
    }

    const FILE: &str = "sagan.2026-09-26.log";

    #[test]
    fn logs_parses_json_lines_newest_first() {
        let dir = fixture_dir();
        write_log(
            &dir,
            FILE,
            &[
                line("2026-09-26T10:00:00.000000Z", "INFO", "first"),
                line("2026-09-26T10:01:00.000000Z", "WARN", "second"),
                r#"{"timestamp":"2026-09-26T10:02:00.000000Z","level":"ERROR","fields":{"message":"third","k":"v"},"target":"sagan::test"}"#.to_string(),
            ],
        );

        let page = read_page(&source(&dir), &query(1));

        assert_eq!(page.entries.len(), 3);
        assert_eq!(
            page.entries[0].timestamp.to_rfc3339(),
            "2026-09-26T10:02:00+00:00"
        );
        assert_eq!(page.entries[0].message, "third");
        assert_eq!(page.entries[0].level, LogLevel::Error);
        assert_eq!(page.entries[0].target, "sagan::test");
        assert_eq!(page.entries[0].extra, r#"{"k":"v"}"#);
        assert_eq!(page.entries[2].message, "first");
        assert_eq!(page.entries[2].level, LogLevel::Info);
        assert_eq!(page.entries[2].extra, "");
    }

    #[test]
    fn logs_filters_by_minimum_level() {
        let dir = fixture_dir();
        write_log(
            &dir,
            FILE,
            &[
                line("2026-09-26T10:00:00.000000Z", "TRACE", "t"),
                line("2026-09-26T10:01:00.000000Z", "DEBUG", "d"),
                line("2026-09-26T10:02:00.000000Z", "INFO", "i"),
                line("2026-09-26T10:03:00.000000Z", "WARN", "w"),
                line("2026-09-26T10:04:00.000000Z", "ERROR", "e"),
            ],
        );

        let page = read_page(
            &source(&dir),
            &LogsQuery {
                min_level: Some(LogLevel::Warn),
                ..query(1)
            },
        );

        let messages: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["e", "w"]);
    }

    #[test]
    fn logs_filters_by_time_range_to_the_second() {
        let dir = fixture_dir();
        write_log(
            &dir,
            FILE,
            &[
                line("2026-09-26T09:59:59.900000Z", "INFO", "before"),
                line("2026-09-26T10:00:00.500000Z", "INFO", "inside"),
                line("2026-09-26T10:00:01.500000Z", "INFO", "after"),
            ],
        );

        let page = read_page(
            &source(&dir),
            &LogsQuery {
                from: parse_datetime_local("2026-09-26T10:00:00"),
                to: parse_datetime_local("2026-09-26T10:00:01"),
                ..query(1)
            },
        );

        let messages: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["after", "inside"]);
    }

    #[test]
    fn logs_paginates_and_reports_totals() {
        let dir = fixture_dir();
        let lines: Vec<String> = (0..120)
            .map(|index| {
                line(
                    &format!("2026-09-26T10:00:{:02}.000000Z", index % 60),
                    "INFO",
                    &format!("entry {index}"),
                )
            })
            .collect();
        write_log(&dir, FILE, &lines);
        let source = source(&dir);

        let first = read_page(&source, &query(1));
        assert_eq!(first.entries.len(), 50);
        assert_eq!(first.page, 1);
        assert_eq!(first.total_pages, 3);
        assert_eq!(first.total_items, 120);

        let third = read_page(&source, &query(3));
        assert_eq!(third.entries.len(), 20);
        assert_eq!(third.page, 3);

        // A page past the end shows the last page rather than an empty table or a 404.
        let clamped = read_page(&source, &query(99));
        assert_eq!(clamped.page, 3);
        assert_eq!(clamped.entries.len(), 20);
    }

    #[test]
    fn logs_ignores_foreign_files_and_broken_lines() {
        let dir = fixture_dir();
        write_log(
            &dir,
            FILE,
            &[
                line("2026-09-26T10:00:00.000000Z", "INFO", "kept"),
                r#"{"timestamp":"2026-09-26T10:01:00.000000Z","level":"INFO","fie"#.to_string(),
                line("2026-09-26T10:02:00.000000Z", "INFO", "also kept"),
            ],
        );
        write_log(
            &dir,
            "other.2026-09-26.log",
            &[line("2026-09-26T11:00:00.000000Z", "INFO", "wrong prefix")],
        );
        write_log(
            &dir,
            "sagan.notes.txt",
            &[line("2026-09-26T12:00:00.000000Z", "INFO", "wrong suffix")],
        );

        let page = read_page(&source(&dir), &query(1));

        let messages: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["also kept", "kept"]);
    }

    #[test]
    fn logs_reads_the_newest_file_first() {
        let dir = fixture_dir();
        write_log(
            &dir,
            "sagan.2026-09-25.log",
            &[line("2026-09-25T23:59:00.000000Z", "INFO", "yesterday")],
        );
        write_log(
            &dir,
            FILE,
            &[line("2026-09-26T00:01:00.000000Z", "INFO", "today")],
        );

        let page = read_page(&source(&dir), &query(1));

        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries[0].message, "today");
        assert_eq!(page.entries[1].message, "yesterday");
    }

    #[test]
    fn logs_ignores_bad_query_params() {
        let params = LogsParams {
            page: Some(0),
            level: Some("bogus".to_string()),
            from: Some("yesterday".to_string()),
            to: None,
        };
        let query = LogsQuery::from_params(&params);
        assert_eq!(query.page, 1);
        assert_eq!(query.min_level, None);
        assert_eq!(query.from, None);
        assert_eq!(query.to, None);

        let params = LogsParams {
            page: None,
            level: Some("warn".to_string()),
            from: Some("2026-09-26T10:00".to_string()),
            to: Some("2026-09-26T10:01:00".to_string()),
        };
        let query = LogsQuery::from_params(&params);
        assert_eq!(query.min_level, Some(LogLevel::Warn));
        assert_eq!(
            query.from.map(|from| from.to_rfc3339()),
            Some("2026-09-26T10:00:00+00:00".to_string())
        );
        assert_eq!(
            query.to.map(|to| to.to_rfc3339()),
            Some("2026-09-26T10:01:00+00:00".to_string())
        );

        // `all` is the select's "no filter" option, not a level.
        let params = LogsParams {
            page: Some(2),
            level: Some("all".to_string()),
            from: Some(String::new()),
            to: Some(String::new()),
        };
        let query = LogsQuery::from_params(&params);
        assert_eq!(query.page, 2);
        assert_eq!(query.min_level, None);
        assert_eq!(query.from, None);
        assert_eq!(query.to, None);
    }
}
