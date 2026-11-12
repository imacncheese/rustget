//! Tray, autostart, clipboard and power integration
#![forbid(unsafe_code)]

pub mod scheme;

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
pub enum AutostartError {
    #[error("autostart is not supported on this platform")]
    Unsupported,
    #[error("autostart command failed: {0}")]
    Command(String),
    #[error("cannot update autostart: {0}")]
    Io(#[from] std::io::Error),
}

const AUTOSTART_VALUE: &str = "RustGet";
const AUTOSTART_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

fn autostart_command_line(executable: &Path, data_dir: &Path) -> String {
    format!(
        "\"{}\" --data-dir \"{}\" serve",
        executable.display(),
        data_dir.display()
    )
}

#[must_use]
pub fn autostart_preview(executable: &Path, data_dir: &Path) -> String {
    if cfg!(windows) {
        format!(
            "reg.exe add {AUTOSTART_KEY} /v {AUTOSTART_VALUE} /t REG_SZ /d \"{}\" /f",
            autostart_command_line(executable, data_dir)
        )
    } else if cfg!(target_os = "macos") {
        format!(
            "{} (ProgramArguments)",
            macos_autostart_path()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "~/Library/LaunchAgents/com.rustget.daemon.plist".into())
        )
    } else {
        linux_autostart_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "~/.config/autostart/rustget.desktop".into())
    }
}

pub fn autostart_status() -> Result<bool, AutostartError> {
    if cfg!(windows) {
        let status = std::process::Command::new("reg.exe")
            .args(["query", AUTOSTART_KEY, "/v", AUTOSTART_VALUE])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        Ok(status.success())
    } else if cfg!(target_os = "macos") {
        Ok(macos_autostart_path()
            .map(|path| path.is_file())
            .unwrap_or(false))
    } else {
        Ok(linux_autostart_path()
            .map(|path| path.is_file())
            .unwrap_or(false))
    }
}

pub fn autostart_enable(executable: &Path, data_dir: &Path) -> Result<(), AutostartError> {
    if cfg!(windows) {
        let status = std::process::Command::new("reg.exe")
            .args([
                "add",
                AUTOSTART_KEY,
                "/v",
                AUTOSTART_VALUE,
                "/t",
                "REG_SZ",
                "/d",
                &autostart_command_line(executable, data_dir),
                "/f",
            ])
            .status()?;
        if status.success() {
            return Ok(());
        }
        return Err(AutostartError::Command(format!("reg add exited {status}")));
    }
    if cfg!(target_os = "macos") {
        let path = macos_autostart_path().ok_or(AutostartError::Unsupported)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.rustget.daemon</string>
<key>ProgramArguments</key><array>
<string>{}</string><string>--data-dir</string><string>{}</string><string>serve</string>
</array><key>RunAtLoad</key><true/>
</dict></plist>
"#,
            xml_escape(&executable.display().to_string()),
            xml_escape(&data_dir.display().to_string())
        );
        std::fs::write(path, plist)?;
        return Ok(());
    }
    let path = linux_autostart_path().ok_or(AutostartError::Unsupported)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName=RustGet\nComment=RustGet download manager daemon\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_escape(&autostart_command_line(executable, data_dir))
    );
    std::fs::write(path, contents)?;
    Ok(())
}

pub fn autostart_disable() -> Result<(), AutostartError> {
    if cfg!(windows) {
        let _ = std::process::Command::new("reg.exe")
            .args(["delete", AUTOSTART_KEY, "/v", AUTOSTART_VALUE, "/f"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        return Ok(());
    }
    let path = if cfg!(target_os = "macos") {
        macos_autostart_path()
    } else {
        linux_autostart_path()
    };
    if let Some(path) = path {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AutostartError::Io(error)),
        }
    } else {
        Err(AutostartError::Unsupported)
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn linux_autostart_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
        .map(|base| base.join("autostart").join("rustget.desktop"))
}

fn macos_autostart_path() -> Option<PathBuf> {
    home_dir().map(|home| {
        home.join("Library")
            .join("LaunchAgents")
            .join("com.rustget.daemon.plist")
    })
}

fn desktop_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('`', "\\`")
        .replace('$', "\\$")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
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
    fn autostart_preview_contains_the_daemon_command() {
        let preview = autostart_preview(Path::new("rustget.exe"), Path::new("D:/data"));
        assert!(preview.contains("serve"));
        if cfg!(windows) {
            assert!(preview.contains("reg.exe"));
            assert!(preview.contains("RustGet"));
        } else if cfg!(target_os = "macos") {
            assert!(preview.contains("LaunchAgents"));
        } else {
            assert!(preview.contains("autostart"));
        }
    }

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
