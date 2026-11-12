use std::fs::File;
use std::io::Read;
use std::path::Path;

use blake3::Hasher as Blake3Hasher;
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::error::{NetError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Blake3,
}

impl ChecksumAlgorithm {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace('-', "").as_str() {
            "md5" => Some(Self::Md5),
            "sha1" => Some(Self::Sha1),
            "sha256" => Some(Self::Sha256),
            "blake3" => Some(Self::Blake3),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
            Self::Blake3 => "blake3",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChecksumSpec {
    pub algorithm: ChecksumAlgorithm,
    pub expected_hex: String,
}

impl ChecksumSpec {
    pub fn new(algorithm: &str, expected_hex: &str) -> Result<Self> {
        let algorithm = ChecksumAlgorithm::parse(algorithm).ok_or(NetError::UnsupportedChecksum)?;
        Ok(Self {
            algorithm,
            expected_hex: normalize_hex(expected_hex)?,
        })
    }
}

/// Hash a completed file in a blocking worker and compare it.
pub async fn verify_checksum(path: &Path, spec: &ChecksumSpec) -> Result<()> {
    let path = path.to_path_buf();
    let spec = spec.clone();
    let actual = tokio::task::spawn_blocking(move || hash_file(&path, spec.algorithm))
        .await
        .map_err(|error| NetError::Protocol(format!("checksum worker failed: {error}")))??;
    if actual == spec.expected_hex {
        Ok(())
    } else {
        Err(NetError::ChecksumMismatch {
            expected: spec.expected_hex,
            actual,
        })
    }
}

pub fn hash_file(path: &Path, algorithm: ChecksumAlgorithm) -> Result<String> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    match algorithm {
        ChecksumAlgorithm::Md5 => {
            let mut hasher = Md5::new();
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        ChecksumAlgorithm::Sha1 => {
            let mut hasher = Sha1::new();
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        ChecksumAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
        ChecksumAlgorithm::Blake3 => {
            let mut hasher = Blake3Hasher::new();
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(hasher.finalize().to_hex().to_string())
        }
    }
}

fn normalize_hex(value: &str) -> Result<String> {
    let cleaned: String = value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(NetError::Protocol("checksum is not valid hex".into()));
    }
    Ok(cleaned.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_known_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            hash_file(&path, ChecksumAlgorithm::Sha256).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash_file(&path, ChecksumAlgorithm::Md5).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }

    #[test]
    fn normalizes_case_and_whitespace() {
        let spec = ChecksumSpec::new("SHA-256", "AB CD").unwrap();
        assert_eq!(spec.expected_hex, "abcd");
    }
}
