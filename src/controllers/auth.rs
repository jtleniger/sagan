use crate::models::users::{self, LoginParams};
use axum::http::StatusCode;
use loco_rs::prelude::*;

const AUTH_COOKIE: &str = "auth_token";

/// `GET /login` — the sign-in form.
#[debug_handler]
async fn login_form(ViewEngine(v): ViewEngine<TeraView>) -> Result<Response> {
    render_login(&v, "", None)
}

/// `POST /login` — verify credentials, set the JWT cookie, continue to the dashboard.
#[debug_handler]
async fn login(
    ViewEngine(v): ViewEngine<TeraView>,
    State(ctx): State<AppContext>,
    Form(params): Form<LoginParams>,
) -> Result<Response> {
    let email = params.email.trim();

    let user = match users::Model::find_by_email(&ctx.db, email).await {
        Ok(user) => user,
        Err(ModelError::EntityNotFound) => {
            return render_login(&v, email, Some("Invalid email or password"));
        }
        Err(err) => return Err(err.into()),
    };

    if !user.verify_password(&params.password) {
        return render_login(&v, email, Some("Invalid email or password"));
    }

    let jwt = ctx.config.get_jwt_config()?;
    let token = user.generate_jwt(&jwt.secret, jwt.expiration)?;
    let cookie = cookie::Cookie::build((AUTH_COOKIE, token))
        .path("/")
        .http_only(true)
        .same_site(cookie::SameSite::Lax)
        .build();

    format::render().cookies(&[cookie])?.redirect("/")
}

/// `POST /logout` — drop the cookie, back to the form.
///
/// `State` is bound even though it is unused: a handler whose extractors give
/// Axum no `AppContext` reference fails to infer the router's state type.
#[debug_handler]
async fn logout(State(_ctx): State<AppContext>) -> Result<Response> {
    let mut cookie = cookie::Cookie::build((AUTH_COOKIE, "")).path("/").build();
    cookie.make_removal();
    format::render().cookies(&[cookie])?.redirect("/login")
}

fn render_login(v: &TeraView, email: &str, error: Option<&str>) -> Result<Response> {
    let status = if error.is_some() {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::OK
    };
    format::render().status(status).view(
        v,
        "auth/login.html",
        data!({"email": email, "error": error}),
    )
}

pub fn routes() -> Routes {
    Routes::new()
        .add("/login", get(login_form))
        .add("/login", post(login))
        .add("/logout", post(logout))
}
