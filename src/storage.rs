//! The app's single file store: one local driver, rooted at `settings.storage.dir`.
//!
//! Loco boots `ctx.storage` as the null driver, which fails every write; the store is
//! therefore wired in `Hooks::after_context`, the one place with the loaded `Config`.

use std::{path::PathBuf, sync::Arc};

use loco_rs::{
    config::Config,
    storage::{drivers::local, Storage},
    Result,
};
use serde::Deserialize;

/// The store's root when `settings.storage.dir` is absent: the directory the capture
/// worker's files land in, and the one `.gitignore` excludes.
pub const DEFAULT_DIR: &str = "captures";

/// The `settings.storage` block.
///
/// Every field defaults, so an absent `settings:` block (or an absent `storage:` key)
/// boots with [`DEFAULT_DIR`] rather than failing.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// The store's root. A relative path resolves against the process working directory.
    pub dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            dir: DEFAULT_DIR.into(),
        }
    }
}

impl StorageConfig {
    /// # Errors
    /// When the `settings.storage` block exists but does not match this schema — a typo must
    /// fail the boot, not silently fall back to `captures`.
    pub fn from_context(config: &Config) -> Result<Self> {
        /// The `settings:` block, as far as this module reads it.
        ///
        /// `Config::settings` deserializes the *whole* block, so the `storage:` key has to be
        /// named here; `deny_unknown_fields` on [`StorageConfig`] then rejects a typo inside
        /// `storage:` rather than defaulting it.
        #[derive(Debug, Default, Deserialize)]
        #[serde(default)]
        struct Settings {
            storage: StorageConfig,
        }

        Ok(config.settings::<Settings>()?.storage)
    }
}

/// The store the app boots with: a single local-driver store whose paths are all relative
/// to `settings.storage.dir`.
///
/// The driver creates the root if it is missing (`opendal`'s `FsBuilder::build` calls
/// `create_dir_all` on a root that does not exist), so no separate `mkdir` is needed.
///
/// # Errors
/// A malformed `settings.storage` block, or a root the filesystem refuses to create.
pub fn store(config: &Config) -> Result<Arc<Storage>> {
    let StorageConfig { dir } = StorageConfig::from_context(config)?;
    Ok(Storage::single(local::new_with_prefix(dir)?).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_store_defaults_to_the_captures_directory() {
        let config: StorageConfig = serde_json::from_value(serde_json::json!({}))
            .expect("an absent settings block is the default");
        assert_eq!(config.dir, PathBuf::from("captures"));
    }

    #[test]
    fn the_directory_comes_from_the_settings_block() {
        let config: StorageConfig =
            serde_json::from_value(serde_json::json!({"dir": "target/test-captures"}))
                .expect("an explicit directory is read");
        assert_eq!(config.dir, PathBuf::from("target/test-captures"));
    }
}
