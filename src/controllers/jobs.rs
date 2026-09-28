use crate::{
    controllers::current_user,
    models::runtime_heartbeats::{self, Source},
    queue, views,
    views::jobs::{
        stuck_warning, JobRowView, JobsView, ProviderStatus, QueueView, RuntimeRowView,
        ScheduledEntryView, WorkerRowView,
    },
    workers,
};
use loco_rs::prelude::*;

/// `GET /jobs` — the background workers this app configures, the queue they run against,
/// what the scheduler will ask for, and the jobs themselves.
///
/// A provider that fails its ping is a status, not a page error: the failure replaces the
/// healthy line and the page still renders. The same goes for the jobs: a mode that keeps
/// no queue ([`queue::Inspector::of`] is `None`) renders the note instead of a table, and
/// no button, rather than an error.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    let provider = match &ctx.queue_provider {
        Some(queue) => Some(ProviderStatus {
            name: queue.describe(),
            ping: queue.ping().await.map_err(|err| err.to_string()),
        }),
        None => None,
    };

    let inspector = queue::Inspector::of(&ctx);
    let jobs = match &inspector {
        Some(inspector) => inspector.jobs().await?,
        None => Vec::new(),
    };

    // The stamps the scheduler and the worker leave behind, read as of one instant so the
    // two rows cannot disagree about "now".
    let now = chrono::Utc::now();
    let seen = runtime_heartbeats::Model::seen(&ctx.db).await?;
    let runtime: Vec<RuntimeRowView> = seen
        .iter()
        .map(|(source, row)| RuntimeRowView::new(*source, row.as_ref(), now))
        .collect();

    // Waiting work plus no recent worker stamp is the one combination that means a reader
    // has something to fix; see `views::jobs::stuck_warning`.
    let worker = seen
        .iter()
        .find(|(source, _)| *source == Source::Worker)
        .and_then(|(_, row)| row.as_ref());
    let rows: Vec<JobRowView> = jobs.iter().map(JobRowView::from).collect();
    let waiting = rows.iter().filter(|row| row.is_waiting()).count();
    let stuck = stuck_warning(
        waiting,
        worker.map(|row| runtime_heartbeats::liveness(Some(row.created_at), now)),
        worker.map(|row| {
            views::jobs::duration_label(runtime_heartbeats::age_milliseconds(row.created_at, now))
        }),
    );

    let mut scheduled: Vec<ScheduledEntryView> = ctx
        .config
        .scheduler
        .as_ref()
        .map(|scheduler| {
            scheduler
                .jobs
                .iter()
                .map(ScheduledEntryView::from)
                .collect()
        })
        .unwrap_or_default();
    scheduled.sort_by(|left, right| left.name.cmp(&right.name));

    format::render().view(
        &v,
        "jobs/index.html",
        data!({
            "user": user,
            "active": "jobs",
            "jobs": JobsView {
                queue: QueueView::new(&ctx.config.workers.mode, provider),
                workers: workers::configured().iter().map(WorkerRowView::from).collect(),
                scheduled,
                jobs: rows,
                actionable: inspector.is_some(),
                stale_minutes: queue::STALE_PROCESSING_MINUTES,
                runtime,
                stuck,
            },
        }),
    )
}

/// `POST /jobs/{id}/cancel` — cancel a job that has not started.
///
/// The queue's own row is the only thing that moves; a job already being performed cannot
/// be stopped this way (see [`queue::Inspector::cancel_queued`]).
#[debug_handler]
async fn cancel(
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let Some(_user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };
    let inspector = inspector(&ctx)?;

    inspector.cancel_queued(&id).await?;
    format::redirect("/jobs")
}

/// `POST /jobs/{id}/retry` — put a failed job back on the queue.
#[debug_handler]
async fn retry(
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let Some(_user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };
    let inspector = inspector(&ctx)?;

    inspector.retry(&id).await?;
    format::redirect("/jobs")
}

/// `POST /jobs/requeue` — put jobs stranded in `processing` back on the queue.
///
/// This is the recovery button: it exists for the case where a worker process died
/// mid-job and the rows it was holding never moved again.
#[debug_handler]
async fn requeue(auth: Option<auth::JWT>, State(ctx): State<AppContext>) -> Result<Response> {
    let Some(_user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };
    let inspector = inspector(&ctx)?;

    inspector.requeue_stale().await?;
    format::redirect("/jobs")
}

/// The queue inspector, or a client error when this mode keeps no queue — the state a
/// hand-made POST reaches, since the page renders no button without one.
fn inspector(ctx: &AppContext) -> Result<std::sync::Arc<queue::Inspector>> {
    queue::Inspector::of(ctx).ok_or_else(|| {
        Error::BadRequest(format!(
            "no job queue to act on: workers.mode is {:?}",
            ctx.config.workers.mode
        ))
    })
}

pub fn routes() -> Routes {
    Routes::new()
        .prefix("/jobs")
        .add("/", get(index))
        .add("/{id}/cancel", post(cancel))
        .add("/{id}/retry", post(retry))
        .add("/requeue", post(requeue))
}
