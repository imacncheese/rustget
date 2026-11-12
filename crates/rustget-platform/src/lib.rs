//! Tray, autostart, clipboard and power integration
#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LockError {
    #[error("another RustGet instance is already using {0}")]
    AlreadyRunning(String),
    #[error("cannot create instance lock: {0}")]
    Io(#[from] std::io::Error),
}

/// Cross-process daemon lock. The lock is released when this value is dropped,
/// including on an abnormal process exit by the OS.
#[derive(Debug)]
pub struct InstanceLock {
    file: File,
    path: PathBuf,
}

impl InstanceLock {
    pub fn acquire(path: impl AsRef<Path>) -> Result<Self, LockError> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        if file.try_lock_exclusive().is_err() {
            return Err(LockError::AlreadyRunning(path.display().to_string()));
        }
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;
        writeln!(file, "{}", std::process::id())?;
        file.sync_all()?;
        Ok(Self { file, path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_is_rejected_until_first_is_dropped() {
        let directory = std::env::temp_dir().join(format!("rustget-lock-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("instance.lock");
        let _ = std::fs::remove_file(&path);
        let first = InstanceLock::acquire(&path).unwrap();
        assert!(matches!(
            InstanceLock::acquire(&path),
            Err(LockError::AlreadyRunning(_))
        ));
        drop(first);
        let third = InstanceLock::acquire(&path).unwrap();
        assert_eq!(third.path(), path);
        drop(third);
        let _ = std::fs::remove_dir_all(&directory);
    }
}
