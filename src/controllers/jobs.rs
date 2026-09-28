use chrono::{DateTime, Local, Utc};
use serde::Deserialize;

use crate::{
    controllers::current_user,
    jobs,
    models::job_runs,
    views::jobs::{JobSummaryView, JobsView, RunRowView},
};
use loco_rs::prelude::*;

/// How many `job_runs` rows one page of the run history holds.
const RUNS_PER_PAGE: u64 = 25;

/// The run history's query string.
#[derive(Debug, Deserialize)]
pub struct RunsParams {
    /// 1-based page number; absent or out of range is clamped into the paginator's range.
    pub page: Option<u64>,
}

/// `GET /jobs` — the periodic jobs this app runs and the history of every attempt.
///
/// The summary reads each job's own answers (`jobs::configured()`) so the page cannot claim a
/// cadence or a next-due the dispatcher would not act on; the history is the same `job_runs`
/// table the due rule reads, simply paginated.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
    Query(params): Query<RunsParams>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    let now = Local::now();
    let now_utc: DateTime<Utc> = now.with_timezone(&Utc);

    let mut summaries = Vec::new();
    for job in jobs::configured() {
        let last = job_runs::Model::newest(&ctx.db, job.name()).await?;
        summaries.push(JobSummaryView::new(
            job.name(),
            job.detail(),
            job.interval(&ctx).await?,
            job.latest_slot(&ctx, now).await?,
            job.next_slot(&ctx, now).await?,
            last.as_ref(),
            now_utc,
        ));
    }

    let paginator = job_runs::Entity::find()
        .order_by_desc(job_runs::Column::SlotAt)
        .order_by_desc(job_runs::Column::Id)
        .paginate(&ctx.db, RUNS_PER_PAGE);

    let total_items = paginator.num_items().await?;
    let total_pages = paginator.num_pages().await?.max(1);
    let page = params.page.unwrap_or(1).clamp(1, total_pages);
    let runs = paginator.fetch_page(page - 1).await?;

    format::render().view(
        &v,
        "jobs/index.html",
        data!({
            "user": user,
            "active": "jobs",
            "jobs": JobsView {
                jobs: summaries,
                runs: runs.iter().map(|run| RunRowView::new(run, now_utc)).collect(),
                page,
                total_pages,
                total_items,
                prev_url: (page > 1).then(|| format!("/jobs?page={}", page - 1)),
                next_url: (page < total_pages).then(|| format!("/jobs?page={}", page + 1)),
                first_url: (page > 1).then(|| "/jobs?page=1".to_string()),
                last_url: (page < total_pages).then(|| format!("/jobs?page={total_pages}")),
            },
        }),
    )
}

pub fn routes() -> Routes {
    Routes::new().prefix("/jobs").add("/", get(index))
}
