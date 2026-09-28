#![allow(elided_lifetimes_in_paths)]
#![allow(clippy::wildcard_imports)]
pub use sea_orm_migration::prelude::*;
mod m20220101_000001_users;
mod m20260926_000002_seed_default_user;

mod m20260927_202919_app_settings;

mod m20260927_231500_job_runs;
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_users::Migration),
            Box::new(m20260926_000002_seed_default_user::Migration),
            Box::new(m20260927_202919_app_settings::Migration),
            Box::new(m20260927_231500_job_runs::Migration),
            // inject-above (do not remove this comment)
        ]
    }
}
