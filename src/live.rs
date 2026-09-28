//! The front page's Live card: one frame per poll, captured through the camera trait and written
//! to disk.

use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use loco_rs::{app::AppContext, config::Config, prelude::*};
use serde::{Deserialize, Serialize};

use crate::{
    camera_lock::CameraLock,
    hardware::{Camera, Hardware, HardwareError},
};

/// Where live frames land when `settings.live.dir` is absent.
pub const DEFAULT_DIR: &str = "/tmp/sagan/live";

/// The newest frame younger than this (the server's side of the cadence) is reused instead of
/// captured again, so two tabs polling at once frame once.
///
/// It is *shorter* than the card's poll interval (`dashboard/_live.html`'s `every 15s`), so a poll
/// with the frame already at the age of the interval still captures a new one.
pub const REFRESH_MS: i64 = 10_000;

/// How many frames the live directory keeps. The directory is scratch in `/tmp` (possibly tmpfs,
/// i.e. RAM): a poll every 15 s would otherwise write ~5 800 JPEGs a day.
pub const KEEP: usize = 10;

/// The card's caption when there is no frame at all yet.
pub const NOTE_WAITING: &str = "Waiting for the first frame from the camera.";

/// The `settings.live` block — `dir` only, defaulted like `settings.capture.dir`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LiveConfig {
    pub dir: PathBuf,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            dir: DEFAULT_DIR.into(),
        }
    }
}

impl LiveConfig {
    /// # Errors
    /// When the `settings:` block exists but does not match this schema (a typo in `live:`) — a
    /// typo must fail the boot, not silently fall back to `/tmp/sagan/live`.
    pub fn from_context(config: &Config) -> Result<Self> {
        /// The `settings:` block, as far as this module reads it.
        ///
        /// `Config::settings` deserializes the *whole* block, so the `live:` key has to be named
        /// here; `deny_unknown_fields` on [`LiveConfig`] then rejects a typo inside `live:`
        /// rather than defaulting it.
        #[derive(Debug, Default, Deserialize)]
        #[serde(default)]
        struct Settings {
            live: LiveConfig,
        }

        Ok(config.settings::<Settings>()?.live)
    }
}

/// One frame: the file, the instant its name carries, and the two strings the card shows.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    /// The frame on disk. Not serialized: the card names a frame by its instant, and the image
    /// route serves the newest file itself.
    #[serde(skip_serializing)]
    pub path: PathBuf,
    /// Unix milliseconds, from the file name `<taken_at_ms>.jpg`.
    pub taken_at_ms: i64,
    /// RFC 3339 UTC — the `datetime` of the `<time>` element `static/js/local-time.js` renders.
    pub taken_at_utc: String,
    /// The same instant as UTC text — the fallback a reader without JavaScript keeps.
    pub taken_at: String,
}

/// The Live card's payload.
#[derive(Debug, Clone, Serialize)]
pub struct Live {
    pub frame: Option<Frame>,
    /// Why there is no newer frame: the camera failed, or none exists yet. `None` when the frame
    /// is a fresh capture.
    pub note: Option<String>,
}

impl Frame {
    /// `None` when chrono cannot represent the instant (only a hand-placed absurd file name can).
    fn at(path: PathBuf, taken_at_ms: i64) -> Option<Self> {
        let at = DateTime::from_timestamp_millis(taken_at_ms)?;
        Some(Self {
            path,
            taken_at_ms,
            taken_at_utc: at.to_rfc3339_opts(SecondsFormat::Secs, true),
            taken_at: at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        })
    }
}

/// The Unix-millisecond instant a frame's name carries, or `None` for every other file in the
/// directory: a `<ms>.partial.jpg` still being written, the lock file, a foreign file, a `.jpg`
/// whose stem is not a number.
fn frame_ms(path: &Path) -> Option<i64> {
    let stem = path.file_name()?.to_str()?.strip_suffix(".jpg")?;
    let taken_at_ms = stem.parse::<i64>().ok()?;
    DateTime::from_timestamp_millis(taken_at_ms).map(|_| taken_at_ms)
}

/// The newest frame in `dir`. `None` when the directory does not exist yet or holds no frame.
#[must_use]
pub fn newest(dir: &Path) -> Option<Frame> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            Frame::at(path.clone(), frame_ms(&path)?)
        })
        .max_by_key(|frame| frame.taken_at_ms)
}

/// Keeps the newest `keep` frames and deletes the rest. A frame that cannot be deleted is logged
/// and stepped over: the next capture tries again.
fn prune(dir: &Path, keep: usize) -> std::io::Result<()> {
    let mut frames: Vec<(i64, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            Some((frame_ms(&path)?, path))
        })
        .collect();
    frames.sort_unstable_by_key(|(taken_at_ms, _)| std::cmp::Reverse(*taken_at_ms));
    for (taken_at_ms, path) in frames.into_iter().skip(keep) {
        if let Err(err) = std::fs::remove_file(&path) {
            tracing::warn!(path = %path.display(), taken_at_ms, error = %err, "could not prune a live frame");
        }
    }
    Ok(())
}

/// The newest frame when it is younger than [`REFRESH_MS`], else `None` (a capture is due).
fn fresh(dir: &Path, now_ms: i64) -> Option<Frame> {
    newest(dir).filter(|frame| now_ms - frame.taken_at_ms < REFRESH_MS)
}

/// One capture: `<taken_at_ms>.partial.jpg` while the driver writes, renamed to
/// `<taken_at_ms>.jpg` when it is whole.
///
/// The intermediate name keeps `.jpg` as the extension — the Pi's `libcamera-*` tools infer the
/// format from it — and its stem is not a number, so `newest`/`frame_ms` never pick up a frame
/// that is still being written while a concurrent `/live/image` reads the directory.
async fn capture(
    camera: &dyn Camera,
    dir: &Path,
    taken_at_ms: i64,
) -> Result<Frame, HardwareError> {
    let partial = format!("{taken_at_ms}.partial.jpg");
    camera.capture(dir, &partial).await?;
    let path = dir.join(format!("{taken_at_ms}.jpg"));
    std::fs::rename(dir.join(&partial), &path)?;
    prune(dir, KEEP)?;
    tracing::info!(path = %path.display(), taken_at_ms, "live frame written");
    Frame::at(path, taken_at_ms).ok_or_else(|| {
        HardwareError::Io(format!(
            "capture instant {taken_at_ms} is not a representable timestamp"
        ))
    })
}

/// What the card shows when the page is rendered: the newest frame on disk, without touching the
/// camera — a page load must not wait for a capture.
///
/// # Errors
/// A `settings.live` that does not parse.
pub fn snapshot(ctx: &AppContext) -> Result<Live> {
    let dir = LiveConfig::from_context(&ctx.config)?.dir;
    let frame = newest(&dir);
    Ok(Live {
        note: frame.is_none().then(|| NOTE_WAITING.to_string()),
        frame,
    })
}

/// What the card shows on a poll: the newest frame, capturing a fresh one first when what is
/// there is older than [`REFRESH_MS`].
///
/// # Errors
/// A `settings.live` that does not parse, and a lock file that cannot be opened. A camera that
/// cannot frame is **not** an error — the card keeps the newest frame it has and carries the
/// reason as `note`.
pub async fn refresh(ctx: &AppContext) -> Result<Live> {
    let dir = LiveConfig::from_context(&ctx.config)?.dir;
    let now_ms = Utc::now().timestamp_millis();
    if let Some(frame) = fresh(&dir, now_ms) {
        return Ok(Live {
            frame: Some(frame),
            note: None,
        });
    }

    // Waiting here is the point: the scheduled capture job is another process and holds the same
    // lock, so a poll frames only when the camera is free.
    let _lock = CameraLock::acquire().await?;

    // Another tab may have captured while this poll waited for the lock.
    let now_ms = Utc::now().timestamp_millis();
    if let Some(frame) = fresh(&dir, now_ms) {
        return Ok(Live {
            frame: Some(frame),
            note: None,
        });
    }

    match capture(Hardware::of(ctx)?.camera.as_ref(), &dir, now_ms).await {
        Ok(frame) => Ok(Live {
            frame: Some(frame),
            note: None,
        }),
        Err(err) => Ok(Live {
            frame: newest(&dir),
            note: Some(err.to_string()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory under the system temp dir that no other test shares: these tests write real
    /// files.
    fn scratch(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past the epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("sagan-{tag}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn frame_ms_reads_only_a_numeric_jpg_stem() {
        assert_eq!(
            frame_ms(Path::new("/tmp/live/1790547901111.jpg")),
            Some(1_790_547_901_111)
        );
        for rejected in [
            "/tmp/live/1790547901111.partial.jpg",
            "/tmp/live/abc.jpg",
            "/tmp/live/notes.txt",
            "/tmp/live/.camera.lock",
        ] {
            assert_eq!(frame_ms(Path::new(rejected)), None, "{rejected}");
        }
    }

    #[test]
    fn newest_picks_the_highest_instant_or_nothing() {
        let dir = scratch("live-newest");
        std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
        for name in ["100.jpg", "300.jpg", "200.partial.jpg", "abc.jpg"] {
            std::fs::write(dir.join(name), b"x").expect("the scratch file is writable");
        }

        assert_eq!(
            newest(&dir).expect("a frame is there").taken_at_ms,
            300,
            "the highest numeric stem wins, and the other files are not frames"
        );

        let _ = std::fs::remove_dir_all(&dir);
        assert!(newest(&dir).is_none(), "a missing directory holds no frame");

        let empty = scratch("live-empty");
        std::fs::create_dir_all(&empty).expect("the scratch directory is creatable");
        assert!(
            newest(&empty).is_none(),
            "an empty directory holds no frame"
        );
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn prune_keeps_the_newest_frames() {
        let dir = scratch("live-prune");
        std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
        for ms in 1_000..1_012 {
            std::fs::write(dir.join(format!("{ms}.jpg")), b"x")
                .expect("the scratch file is writable");
        }

        prune(&dir, 10).expect("pruning a directory of frames succeeds");

        let mut remaining: Vec<i64> = std::fs::read_dir(&dir)
            .expect("the directory reads")
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| frame_ms(&entry.path()))
            .collect();
        remaining.sort_unstable();
        assert_eq!(remaining, (1_002..1_012).collect::<Vec<i64>>());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn at_formats_the_instant_the_card_shows() {
        let frame = Frame::at(
            PathBuf::from("/tmp/live/1790510400000.jpg"),
            1_790_510_400_000,
        )
        .expect("a real instant");

        assert_eq!(frame.taken_at_utc, "2026-09-27T12:00:00Z");
        assert_eq!(frame.taken_at, "2026-09-27 12:00:00 UTC");
    }
}
