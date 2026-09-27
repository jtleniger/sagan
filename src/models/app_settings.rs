//! The database-backed application configuration: one row per configuration *section*.
//!
//! Runtime, application-level configuration — the interval the Captures section runs on — not
//! `config/*.yaml`, which is read once at boot into `ctx.config` and cannot change without a
//! restart. A new section adds a payload struct and a row; this table does not change.

use loco_rs::prelude::*;

use crate::captures::{self, CaptureSettings};

pub use super::_entities::app_settings::{self, ActiveModel, Entity, Model};

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if !insert && self.updated_at.is_unchanged() {
            let mut this = self;
            this.updated_at = sea_orm::ActiveValue::Set(chrono::Utc::now().into());
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

impl Model {
    /// The stored JSON of one section, or `None` when nothing has been saved for it yet.
    ///
    /// Generic over the connection so [`Model::save_section`] can read inside its transaction.
    ///
    /// # Errors
    /// A database error.
    pub async fn find_section<C: ConnectionTrait>(
        db: &C,
        section: &str,
    ) -> ModelResult<Option<Self>> {
        Ok(app_settings::Entity::find()
            .filter(
                model::query::condition()
                    .eq(app_settings::Column::Section, section)
                    .build(),
            )
            .one(db)
            .await?)
    }

    /// Writes a section's JSON, inserting the row the first time.
    ///
    /// A transaction, not a check-then-insert: two savers arriving together would otherwise both
    /// find no row and both insert one.
    ///
    /// # Errors
    /// A database error, or a payload that cannot be serialized.
    pub async fn save_section<T: serde::Serialize + Sync>(
        db: &DatabaseConnection,
        section: &str,
        value: &T,
    ) -> ModelResult<Self> {
        let value = serde_json::to_string(value).map_err(ModelError::to_msg)?;

        let txn = db.begin().await?;
        let row = match Self::find_section(&txn, section).await? {
            Some(existing) => {
                let mut active: app_settings::ActiveModel = existing.into();
                active.value = ActiveValue::Set(value);
                active.update(&txn).await?
            }
            None => {
                app_settings::ActiveModel {
                    section: ActiveValue::Set(section.to_string()),
                    value: ActiveValue::Set(value),
                    ..Default::default()
                }
                .insert(&txn)
                .await?
            }
        };
        txn.commit().await?;
        Ok(row)
    }

    /// The Captures section: what is stored, or [`CaptureSettings::default`] when nothing has been
    /// saved yet (a freshly migrated database has no row).
    ///
    /// # Errors
    /// A database error, or a stored payload this build cannot read (a hand-edited row) — which
    /// fails loudly rather than silently resetting the interval the reader chose.
    pub async fn capture_settings(db: &DatabaseConnection) -> ModelResult<CaptureSettings> {
        let Some(row) = Self::find_section(db, captures::SECTION).await? else {
            return Ok(CaptureSettings::default());
        };
        serde_json::from_str(&row.value).map_err(ModelError::to_msg)
    }

    /// Persists the Captures section.
    ///
    /// # Errors
    /// A database error, or a payload that cannot be serialized.
    pub async fn save_capture_settings(
        db: &DatabaseConnection,
        settings: &CaptureSettings,
    ) -> ModelResult<Self> {
        Self::save_section(db, captures::SECTION, settings).await
    }
}
