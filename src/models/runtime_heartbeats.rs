//! The liveness stamps the scheduler and the worker write, and what "alive" means for them.
//!
//! Why a table at all: the web process cannot see another process's worker loop. Loco keeps
//! the loop's `CancellationToken` inside its queue provider, and the `Queue` handle holds
//! that provider in a private field with no accessor, so `ctx.queue_provider.is_some()` says
//! only that a *mailbox* exists — it is `Some` in a web-only process too. A stamp the work
//! itself writes is therefore the only cross-process evidence available, and it costs one
//! row per minute.
//!
//! Two writers, both run by the scheduler's `heartbeat` entry (see `config/<env>.yaml`):
//! the task stamps [`Source::Scheduler`] and enqueues a [`crate::workers::heartbeat`] job
//! whose `perform` stamps [`Source::Worker`]. So a fresh scheduler stamp means the clock
//! ticked, and a fresh worker stamp means some process drained the queue — which is what the
//! `/jobs` page reports, and what the stuck-queue warning is built on.

use chrono::{DateTime, FixedOffset, Utc};
use loco_rs::prelude::*;

pub use super::_entities::runtime_heartbeats::{self, ActiveModel, Entity, Model};

/// The scheduler's cron runs `heartbeat` once a minute, so this is the interval a stamp is
/// expected to arrive at. The page renders the age against it.
pub const EXPECTED_TICK_SECONDS: i64 = 60;

/// How long a source may stay silent before it is [`Liveness::Stale`]: two ticks, so one
/// late or missed tick is not reported as a dead process.
pub const STALE_AFTER_SECONDS: i64 = 2 * EXPECTED_TICK_SECONDS;

/// How much history the table keeps. A day is enough to see a gap after the fact; keeping it
/// bounded is what lets the read and the prune go without an index.
pub const RETENTION_HOURS: i64 = 24;

/// The processes that stamp, and the `source` values they are stored under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Stamped by the `heartbeat` task, which the scheduler runs every minute.
    Scheduler,
    /// Stamped by the [`crate::workers::heartbeat::HeartbeatWorker`] job the task enqueues.
    Worker,
}

impl Source {
    /// Every source, in the order the page lists them.
    pub const ALL: [Self; 2] = [Self::Scheduler, Self::Worker];

    /// The value stored in the row's `source` column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scheduler => "scheduler",
            Self::Worker => "worker",
        }
    }

    /// The name the page shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scheduler => "Scheduler",
            Self::Worker => "Worker",
        }
    }

    /// Who writes the stamp — what a reader has to start to get it moving.
    #[must_use]
    pub const fn stamp_writer(self) -> &'static str {
        match self {
            Self::Scheduler => "the task the scheduler runs once a minute",
            Self::Worker => "any process draining the queue",
        }
    }
}

/// What a source's newest stamp means, as of now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// Seen within the last two ticks.
    Live,
    /// Last seen more than two ticks ago — a process that was there and stopped.
    Stale,
    /// Never stamped at all — the work has not run since this database was created.
    Missing,
}

/// The state a source is in when it last stamped at `seen_at`.
///
/// `now` is a parameter rather than read inside, so the rule can be tested without a clock.
#[must_use]
pub fn liveness(seen_at: Option<DateTime<FixedOffset>>, now: DateTime<Utc>) -> Liveness {
    let Some(seen_at) = seen_at else {
        return Liveness::Missing;
    };

    // A clock that moved backwards must not read as a dead process: only an age that is
    // positive and past the window is stale.
    let age_seconds = (now - seen_at.with_timezone(&Utc)).num_seconds();
    if age_seconds > STALE_AFTER_SECONDS {
        Liveness::Stale
    } else {
        Liveness::Live
    }
}

/// How long ago a source last stamped, in milliseconds — negative ages clamp to zero.
#[must_use]
pub fn age_milliseconds(seen_at: DateTime<FixedOffset>, now: DateTime<Utc>) -> i64 {
    (now - seen_at.with_timezone(&Utc))
        .num_milliseconds()
        .max(0)
}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    /// Writes one stamp for `source`, and prunes history past [`RETENTION_HOURS`].
    ///
    /// The host and pid are read here rather than passed in: they are facts about the
    /// process doing the stamping, which is the only thing the caller is.
    ///
    /// # Errors
    /// A database error, or a clock the database refuses.
    pub async fn stamp(db: &DatabaseConnection, source: Source) -> ModelResult<Self> {
        let row = runtime_heartbeats::ActiveModel {
            source: ActiveValue::Set(source.as_str().to_string()),
            host: ActiveValue::Set(host_name()),
            pid: ActiveValue::Set(i64::from(std::process::id())),
            ..Default::default()
        }
        .insert(db)
        .await?;

        // Insert first: a failed prune must not cost the stamp the page depends on.
        prune(db).await?;
        Ok(row)
    }

    /// The newest stamp for `source`, if it has ever stamped.
    ///
    /// # Errors
    /// A database error.
    pub async fn latest(db: &DatabaseConnection, source: Source) -> ModelResult<Option<Self>> {
        Ok(Entity::find()
            .filter(runtime_heartbeats::Column::Source.eq(source.as_str()))
            // `created_at` is whole microseconds, but a stamp written while another was
            // being written could tie; the id (a monotonic rowid) breaks it the right way.
            .order_by_desc(runtime_heartbeats::Column::CreatedAt)
            .order_by_desc(runtime_heartbeats::Column::Id)
            .one(db)
            .await?)
    }

    /// Every source's newest stamp, in [`Source::ALL`] order.
    ///
    /// # Errors
    /// A database error.
    pub async fn seen(db: &DatabaseConnection) -> ModelResult<Vec<(Source, Option<Self>)>> {
        let mut seen = Vec::with_capacity(Source::ALL.len());
        for source in Source::ALL {
            seen.push((source, Self::latest(db, source).await?));
        }
        Ok(seen)
    }
}

/// Drops history past [`RETENTION_HOURS`].
async fn prune(db: &DatabaseConnection) -> ModelResult<()> {
    let cutoff = Utc::now() - chrono::Duration::hours(RETENTION_HOURS);
    Entity::delete_many()
        .filter(runtime_heartbeats::Column::CreatedAt.lt(cutoff))
        .exec(db)
        .await?;
    Ok(())
}

/// The machine the stamp came from — the same field the `/system` page's `hostname` shows.
/// A host whose name cannot be read is still a stamp from somewhere, so the row says so
/// rather than failing.
fn host_name() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seen_seconds_ago: i64) -> (DateTime<FixedOffset>, DateTime<Utc>) {
        let now = DateTime::from_timestamp(1_758_000_000, 0).expect("a valid instant");
        let seen = now - chrono::Duration::seconds(seen_seconds_ago);
        (seen.into(), now)
    }

    #[test]
    fn a_source_that_never_stamped_is_missing() {
        let (_, now) = at(0);
        assert_eq!(liveness(None, now), Liveness::Missing);
    }

    #[test]
    fn one_late_tick_is_not_a_dead_process() {
        let (seen, now) = at(EXPECTED_TICK_SECONDS);
        assert_eq!(liveness(Some(seen), now), Liveness::Live);

        // The boundary is the window itself, inclusive on the live side.
        let (seen, now) = at(STALE_AFTER_SECONDS);
        assert_eq!(liveness(Some(seen), now), Liveness::Live);
    }

    #[test]
    fn two_missed_ticks_read_as_stale() {
        let (seen, now) = at(STALE_AFTER_SECONDS + 1);
        assert_eq!(liveness(Some(seen), now), Liveness::Stale);

        let (seen, now) = at(3_600);
        assert_eq!(liveness(Some(seen), now), Liveness::Stale);
    }

    #[test]
    fn a_clock_that_went_backwards_is_not_stale() {
        // A stamp from the future (an NTP correction, or a stamp from another host whose
        // clock runs ahead) is live, not dead.
        let (_, now) = at(0);
        let future: DateTime<FixedOffset> = (now + chrono::Duration::seconds(30)).into();
        assert_eq!(liveness(Some(future), now), Liveness::Live);
        assert_eq!(
            age_milliseconds(future, now),
            0,
            "a negative age reads as zero"
        );
    }

    #[test]
    fn the_age_is_what_the_page_turns_into_a_label() {
        let (seen, now) = at(90);
        assert_eq!(age_milliseconds(seen, now), 90_000);
    }

    #[test]
    fn every_source_has_its_own_row_key_and_label() {
        let keys: Vec<&str> = Source::ALL.iter().map(|source| source.as_str()).collect();
        assert_eq!(keys, vec!["scheduler", "worker"]);
        for source in Source::ALL {
            assert!(
                !source.label().is_empty() && !source.stamp_writer().is_empty(),
                "{source:?} would render an empty cell"
            );
        }
    }
}
