use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PreallocateMode {
    /// Leave the file length at zero until data arrives.
    None,
    /// Set the final length immediately. Most filesystems create a sparse file.
    #[default]
    Sparse,
    /// Set the final length and write zeros to every block. Slow but guarantees
    /// that ENOSPC is discovered before the transfer starts.
    Full,
}

/// Positional writer for one `.part` file.
#[derive(Debug)]
pub struct PartFile {
    file: File,
    path: PathBuf,
    len: u64,
}

impl PartFile {
    pub fn open(
        path: impl AsRef<Path>,
        total_size: u64,
        mode: PreallocateMode,
    ) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        let current = file.metadata()?.len();
        if current < total_size && mode != PreallocateMode::None {
            file.set_len(total_size)?;
        }
        if mode == PreallocateMode::Full {
            preallocate_zeros(&file, total_size.min(current))?;
        }
        Ok(Self {
            file,
            path,
            len: total_size,
        })
    }

    /// Open a part file whose final size is not yet known (chunked response).
    /// The logical bound is disabled until `set_len` is called.
    pub fn open_unbounded(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            file,
            path,
            len: u64::MAX,
        })
    }

    /// Wipe a corrupt/incompatible part file before a fresh transfer.
    pub fn reset(
        path: impl AsRef<Path>,
        total_size: u64,
        mode: PreallocateMode,
    ) -> io::Result<Self> {
        let path = path.as_ref();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Self::open(path, total_size, mode)
    }

    pub fn write_at(&self, offset: u64, mut data: &[u8]) -> io::Result<()> {
        if offset.saturating_add(data.len() as u64) > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "write [{}, {}) exceeds file length {}",
                    offset,
                    offset as usize + data.len(),
                    self.len
                ),
            ));
        }
        let mut cursor = offset;
        while !data.is_empty() {
            let written = write_at_platform(&self.file, cursor, data)?;
            if written == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "positional write returned zero",
                ));
            }
            cursor += written as u64;
            data = &data[written..];
        }
        Ok(())
    }

    pub fn sync_data(&self) -> io::Result<()> {
        self.file.sync_data()
    }

    pub fn sync_all(&self) -> io::Result<()> {
        self.file.sync_all()
    }

    pub fn set_len(&self, len: u64) -> io::Result<()> {
        self.file.set_len(len)
    }

    #[must_use]
    pub fn len(&self) -> u64 {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn preallocate_zeros(file: &File, from: u64) -> io::Result<()> {
    // `set_len` already allocated metadata; for Full mode we explicitly touch
    // every block so delayed allocation cannot fail mid-transfer.
    let len = file.metadata()?.len();
    if len <= from {
        return Ok(());
    }
    let zeros = [0_u8; 1024 * 1024];
    let mut offset = from;
    while offset < len {
        let chunk = usize::try_from((len - offset).min(zeros.len() as u64)).unwrap_or(zeros.len());
        write_at_platform(file, offset, &zeros[..chunk])?;
        offset += chunk as u64;
    }
    file.sync_data()
}

#[cfg(unix)]
fn write_at_platform(file: &File, offset: u64, data: &[u8]) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(data, offset)
}

#[cfg(windows)]
fn write_at_platform(file: &File, offset: u64, data: &[u8]) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(data, offset)
}

#[cfg(not(any(unix, windows)))]
fn write_at_platform(_file: &File, _offset: u64, _data: &[u8]) -> io::Result<usize> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "positional writes are unavailable on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn out_of_order_positional_writes_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.part");
        let file = PartFile::open(&path, 16, PreallocateMode::Sparse).unwrap();
        file.write_at(8, b"ijklmnop").unwrap();
        file.write_at(0, b"abcdefgh").unwrap();
        drop(file);
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes, b"abcdefghijklmnop");
    }

    #[test]
    fn write_beyond_length_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let file = PartFile::open(dir.path().join("x.part"), 4, PreallocateMode::Sparse).unwrap();
        assert!(file.write_at(2, b"three").is_err());
    }

    #[test]
    fn full_preallocation_writes_real_zeros() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full.part");
        let file = PartFile::open(&path, 64 * 1024, PreallocateMode::Full).unwrap();
        assert_eq!(file.len(), 64 * 1024);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 64 * 1024);
    }
}
