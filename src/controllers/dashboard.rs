use crate::{models::users, views::user::UserView};
use loco_rs::prelude::*;

/// `GET /` — the signed-in home page.
///
/// HTML, so a missing/expired/forged cookie must land on the login form, not on
/// a 401 JSON body: Loco's optional JWT extraction returns `None` for every
/// token failure instead of rejecting, which is exactly this behaviour.
#[debug_handler]
async fn index(
    ViewEngine(v): ViewEngine<TeraView>,
    auth: Option<auth::JWT>,
    State(ctx): State<AppContext>,
) -> Result<Response> {
    let Some(auth) = auth else {
        return format::redirect("/login");
    };

    let user = match users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await {
        Ok(user) => user,
        // Live token, deleted row: treat as signed out.
        Err(ModelError::EntityNotFound) => return format::redirect("/login"),
        Err(err) => return Err(err.into()),
    };

    format::render().view(
        &v,
        "dashboard/index.html",
        data!({"user": UserView::from(&user), "active": "dashboard"}),
    )
}

pub fn routes() -> Routes {
    Routes::new().add("/", get(index))
}
