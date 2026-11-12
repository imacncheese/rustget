//! Tray, autostart, clipboard and power integration
#![forbid(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PowerAction {
    Shutdown,
    Sleep,
    Hibernate,
    Lock,
    Logoff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl PowerCommand {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
        }
    }
}

#[derive(Debug, Error)]
pub enum PowerError {
    #[error("power action is not supported on this platform")]
    Unsupported,
    #[error("power command failed with status {0}")]
    Failed(i32),
    #[error("cannot run power command: {0}")]
    Io(#[from] std::io::Error),
}

/// Build the OS command without executing it. This is what tests and dry runs
/// use, so CI never suspends or shuts down the host.
pub fn power_command(action: PowerAction) -> Result<PowerCommand, PowerError> {
    let command = if cfg!(windows) {
        match action {
            PowerAction::Shutdown => PowerCommand::new("shutdown.exe", &["/s", "/t", "5"]),
            PowerAction::Sleep => {
                PowerCommand::new("rundll32.exe", &["powrprof.dll,SetSuspendState", "0,1,0"])
            }
            PowerAction::Hibernate => PowerCommand::new("shutdown.exe", &["/h"]),
            PowerAction::Lock => PowerCommand::new("rundll32.exe", &["user32.dll,LockWorkStation"]),
            PowerAction::Logoff => PowerCommand::new("shutdown.exe", &["/l"]),
        }
    } else if cfg!(target_os = "macos") {
        match action {
            PowerAction::Shutdown => PowerCommand::new(
                "osascript",
                &["-e", "tell application \"System Events\" to shut down"],
            ),
            PowerAction::Sleep => PowerCommand::new("pmset", &["sleepnow"]),
            PowerAction::Hibernate => return Err(PowerError::Unsupported),
            PowerAction::Lock => PowerCommand::new("pmset", &["displaysleepnow"]),
            PowerAction::Logoff => PowerCommand::new(
                "osascript",
                &["-e", "tell application \"System Events\" to log out"],
            ),
        }
    } else {
        match action {
            PowerAction::Shutdown => PowerCommand::new("systemctl", &["poweroff"]),
            PowerAction::Sleep => PowerCommand::new("systemctl", &["suspend"]),
            PowerAction::Hibernate => PowerCommand::new("systemctl", &["hibernate"]),
            PowerAction::Lock => PowerCommand::new("loginctl", &["lock-session"]),
            PowerAction::Logoff => return Err(PowerError::Unsupported),
        }
    };
    Ok(command)
}

/// Execute a power action. Call only from a daemon that was explicitly started
/// with power actions enabled; the default deployment never calls this.
pub fn execute_power(action: PowerAction) -> Result<PowerCommand, PowerError> {
    let command = power_command(action)?;
    let status = std::process::Command::new(&command.program)
        .args(&command.args)
        .status()?;
    if status.success() {
        Ok(command)
    } else {
        Err(PowerError::Failed(status.code().unwrap_or(-1)))
    }
}

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
    fn power_commands_are_constructed_without_execution() {
        for action in [PowerAction::Shutdown, PowerAction::Sleep, PowerAction::Lock] {
            let command = power_command(action).unwrap();
            assert!(!command.program.is_empty());
        }
        if cfg!(windows) {
            assert_eq!(power_command(PowerAction::Hibernate).unwrap().args, ["/h"]);
        }
    }

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
