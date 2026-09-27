use crate::{controllers::current_user, views::dashboard::DashboardView};
use loco_rs::prelude::*;

/// `GET /` — the signed-in home page.
///
/// HTML, so a missing/expired/forged cookie must land on the login form, not on
/// a 401 JSON body: the resolution in `controllers::current_user` returns `None`
/// for every token failure instead of rejecting, which is exactly this behaviour.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let Some(user) = current_user(&ctx, auth).await? else {
        return format::redirect("/login");
    };

    format::render().view(
        &v,
        "dashboard/index.html",
        data!({
            "user": user,
            "active": "dashboard",
            "dashboard": DashboardView::placeholder(),
        }),
    )
}

pub fn routes() -> Routes {
    Routes::new().add("/", get(index))
}
