use crate::{
    controllers::current_user,
    logs::{self, LogSource, LogsQuery},
    views::logs::LogsPageView,
};
use loco_rs::prelude::*;

/// `GET /logs` — the application's log records, newest first.
///
/// The records come from the files Loco's file appender writes, not from anywhere in the
/// database, so there is no model to call: the handler resolves the user, parses the
/// filters, and hands both to `logs::read_page` + `LogsPageView`.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
    Query(params): Query<logs::LogsParams>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    let source = LogSource::from_config(&ctx.config);
    let query = LogsQuery::from_params(&params);
    let page = LogsPageView::new(&source, logs::read_page(&source, &query), &query);

    format::render().view(
        &v,
        "logs/index.html",
        data!({"user": user, "active": "logs", "logs": page}),
    )
}

pub fn routes() -> Routes {
    Routes::new().prefix("/logs").add("/", get(index))
}
