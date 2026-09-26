use serde::Serialize;

use crate::models::_entities::users;

#[derive(Debug, Serialize)]
pub struct UserView {
    pub pid: String,
    pub name: String,
    pub email: String,
}

impl From<&users::Model> for UserView {
    fn from(user: &users::Model) -> Self {
        Self {
            pid: user.pid.to_string(),
            name: user.name.clone(),
            email: user.email.clone(),
        }
    }
}
