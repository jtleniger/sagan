//! Seed the default login: `admin@example.com` / `admin`.

use loco_rs::hash::hash_password;
use sea_orm_migration::prelude::*;
use uuid::Uuid;

const ADMIN_EMAIL: &str = "admin@example.com";
const ADMIN_NAME: &str = "admin";
const ADMIN_PASSWORD: &str = "admin";

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Users {
    Table,
    Pid,
    Email,
    Password,
    ApiKey,
    Name,
    EmailVerifiedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let password_hash =
            hash_password(ADMIN_PASSWORD).map_err(|err| DbErr::Custom(err.to_string()))?;
        let api_key = format!("lo-{}", Uuid::new_v4());

        // `pid`, `api_key` and `created_at`/`updated_at` cannot come from
        // `ActiveModelBehavior::before_save` here (raw statement), and the
        // timestamps are filled by their column default.
        m.execute(
            Query::insert()
                .into_table(Users::Table)
                .columns([
                    Users::Pid,
                    Users::Email,
                    Users::Password,
                    Users::ApiKey,
                    Users::Name,
                    Users::EmailVerifiedAt,
                ])
                .values_panic([
                    Expr::val(Uuid::new_v4()),
                    Expr::val(ADMIN_EMAIL),
                    Expr::val(password_hash),
                    Expr::val(api_key),
                    Expr::val(ADMIN_NAME),
                    Expr::current_timestamp(),
                ])
                .to_owned(),
        )
        .await?;
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.execute(
            Query::delete()
                .from_table(Users::Table)
                .cond_where(Expr::col(Users::Email).eq(ADMIN_EMAIL))
                .to_owned(),
        )
        .await?;
        Ok(())
    }
}
