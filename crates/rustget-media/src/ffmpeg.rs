use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MuxError {
    #[error("FFmpeg was not found; install it or configure RUSTGET_FFMPEG")]
    NotFound,
    #[error("FFmpeg failed with status {status}: {stderr}")]
    Failed { status: i32, stderr: String },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    pub path: PathBuf,
}

impl Ffmpeg {
    /// Mux separate video and audio inputs into a single MP4 without re-encoding
    /// when the codecs are compatible (`-c copy`).
    pub fn mux_copy(
        &self,
        video: &std::path::Path,
        audio: Option<&std::path::Path>,
        output: &std::path::Path,
    ) -> Result<(), MuxError> {
        let mut command = Command::new(&self.path);
        command
            .arg("-hide_banner")
            .arg("-nostdin")
            .arg("-y")
            .arg("-i")
            .arg(video);
        if let Some(audio) = audio {
            command.arg("-i").arg(audio);
            command.args(["-map", "0:v:0", "-map", "1:a:0", "-c", "copy"]);
        } else {
            command.args(["-map", "0", "-c", "copy"]);
        }
        command.arg("-movflags").arg("+faststart").arg(output);
        let result = command.output()?;
        if !result.status.success() {
            return Err(MuxError::Failed {
                status: result.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&result.stderr)
                    .chars()
                    .take(4000)
                    .collect(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn version(&self) -> Option<String> {
        let output = Command::new(&self.path).arg("-version").output().ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(str::to_string)
    }
}

/// Resolve FFmpeg from `RUSTGET_FFMPEG` or `PATH` without invoking a shell.
#[must_use]
pub fn find_ffmpeg() -> Option<Ffmpeg> {
    if let Ok(path) = std::env::var("RUSTGET_FFMPEG") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(Ffmpeg { path });
        }
    }
    let executable = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(executable);
        if candidate.is_file() {
            return Some(Ffmpeg { path: candidate });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_never_panics_without_ffmpeg() {
        let found = find_ffmpeg();
        if let Some(ffmpeg) = found {
            assert!(ffmpeg.path.exists());
        }
    }
}
