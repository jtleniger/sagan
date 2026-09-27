pub mod auth;

pub mod dashboard;

pub mod logs;

pub mod system;

use crate::{models::users, views::user::UserView};
// `auth::JWT` would resolve to the local `pub mod auth` in this module.
use loco_rs::prelude::auth::JWT;
use loco_rs::prelude::*;

/// The signed-in user's view, or `None` for a visitor who must be sent to the login form.
///
/// No token, a forged/expired token and a token whose row is gone are all indistinguishable
/// to Loco's optional JWT extractor, and all three mean "signed out" for a page that renders
/// the app shell.
pub(crate) async fn current_user(ctx: &AppContext, auth: Option<JWT>) -> Result<Option<UserView>> {
    let Some(auth) = auth else {
        return Ok(None);
    };

    match users::Model::find_by_pid(&ctx.db, &auth.claims.pid).await {
        Ok(user) => Ok(Some(UserView::from(&user))),
        Err(ModelError::EntityNotFound) => Ok(None),
        Err(err) => Err(err.into()),
    }
}
