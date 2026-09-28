//! One camera, one capture at a time — in this process and in every other one on the host.

use std::path::Path;

use loco_rs::{Error, Result};

/// The lock file every camera user on this host flocks.
///
/// A fixed path on purpose: the HTTP server and the scheduler's child are separate processes, and
/// a lock path configured per environment could differ between them and silently guard nothing.
pub const LOCK_FILE: &str = "/tmp/sagan/camera.lock";

/// An exclusive hold on the camera. What holds the lock is the open descriptor; dropping this
/// closes it, which is what releases the lock.
pub struct CameraLock {
    _file: std::fs::File,
}

impl CameraLock {
    /// Waits for exclusive use of the camera.
    ///
    /// Waiting is not an error: it lasts as long as the capture that holds the lock (a scheduled
    /// capture, a `hardware_check` probe, another tab's poll).
    ///
    /// # Errors
    /// When `/tmp/sagan` or the lock file cannot be created or opened.
    pub async fn acquire() -> Result<Self> {
        // `File::lock` blocks; hold the wait on a blocking thread rather than a runtime worker,
        // and open/lock inside it so the returned guard owns the descriptor that holds the lock.
        tokio::task::spawn_blocking(|| Self::blocking(Path::new(LOCK_FILE)))
            .await
            .map_err(|err| Error::string(&format!("camera lock task failed: {err}")))?
            .map_err(Error::from)
    }

    fn blocking(path: &Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Opened for writing: `File::lock`'s behaviour on a read-only handle is unspecified. And
        // never truncated: the file's only content is the lock, and truncating a descriptor this
        // process may already be holding a lock through is pointless risk.
        let file = std::fs::File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.lock()?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `flock` is per open file description, so a second `open` in this very process contends
    /// with the first exactly as another process would.
    #[tokio::test]
    async fn a_second_holder_cannot_take_the_lock() {
        let path = std::env::temp_dir().join(format!(
            "sagan-camera-lock-test-{}.lock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        let guard = CameraLock::blocking(&path).expect("the first holder takes the lock");

        let other = std::fs::File::options()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .expect("the lock file opens");
        assert!(
            matches!(other.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
            "a second holder must wait"
        );

        drop(guard);
        assert!(
            other.try_lock().is_ok(),
            "dropping the guard releases the lock"
        );

        drop(other);
        let _ = std::fs::remove_file(&path);
    }
}
