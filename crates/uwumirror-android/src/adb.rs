//! Google's `adb`, found on the system (or fetched on request) and run as a
//! program.
//!
//! adb is the only door into an Android phone that needs no app on it: turn
//! on developer options and (wireless) debugging once, and the phone accepts
//! a paired computer. UwUMirror uses the `adb` the user already has — from
//! the Android SDK, a package manager, or Google's platform-tools that the app
//! downloads into its own folder when asked (see `platform_tools.rs`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum AdbError {
    #[error("adb was not found")]
    NotFound,
    #[error("adb could not be started: {0}")]
    Spawn(std::io::Error),
    #[error("adb took too long")]
    Timeout,
    #[error("{0}")]
    Failed(String),
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// Where an `adb` might be, most specific first. `own` is the folder the app
/// downloads platform-tools into.
pub fn candidates(own: Option<&Path>) -> Vec<PathBuf> {
    let adb = exe("adb");
    let mut paths = Vec::new();
    if let Some(own) = own {
        paths.push(own.join("platform-tools").join(&adb));
    }
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path).map(|dir| dir.join(&adb)));
    }
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(sdk) = std::env::var_os(var) {
            paths.push(PathBuf::from(sdk).join("platform-tools").join(&adb));
        }
    }
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            paths.push(
                PathBuf::from(local)
                    .join("Android/Sdk/platform-tools")
                    .join(&adb),
            );
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = &home {
            paths.push(home.join("Library/Android/sdk/platform-tools/adb"));
        }
        paths.push("/opt/homebrew/bin/adb".into());
        paths.push("/usr/local/bin/adb".into());
    } else {
        if let Some(home) = &home {
            paths.push(home.join("Android/Sdk/platform-tools/adb"));
        }
        paths.push("/usr/bin/adb".into());
        paths.push("/usr/lib/android-sdk/platform-tools/adb".into());
    }
    paths
}

/// The first `adb` that exists, or the one the user picked.
pub fn find(own: Option<&Path>, chosen: Option<&Path>) -> Option<PathBuf> {
    if let Some(chosen) = chosen.filter(|p| p.is_file()) {
        return Some(chosen.to_owned());
    }
    candidates(own).into_iter().find(|path| path.is_file())
}

/// A phone as `adb devices -l` lists it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub serial: String,
    /// "device" when usable; "unauthorized" until the phone allowed this
    /// computer, "offline" while it reconnects.
    pub state: String,
    pub model: Option<String>,
    /// Over the network (wireless debugging or `adb tcpip`), not USB.
    pub wireless: bool,
}

pub fn parse_devices(output: &str) -> Vec<Device> {
    output
        .lines()
        .skip_while(|line| !line.starts_with("List of devices"))
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let serial = fields.next()?.to_owned();
            let state = fields.next()?.to_owned();
            let model = fields
                .find_map(|field| field.strip_prefix("model:"))
                .map(|model| model.replace('_', " "));
            let wireless = serial.contains("._adb-tls-connect.") || serial.contains(':');
            Some(Device {
                serial,
                state,
                model,
                wireless,
            })
        })
        .collect()
}

/// Runs adb programs. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Adb {
    pub path: PathBuf,
}

impl Adb {
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.path);
        command.stdin(Stdio::null()).kill_on_drop(true);
        // Every adb call would flash a console window on Windows.
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        command
    }

    /// Runs adb with `args` and returns what it printed.
    pub async fn run(&self, args: &[&str], timeout: Duration) -> Result<String, AdbError> {
        let output = self
            .command()
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let output = tokio::time::timeout(timeout, output)
            .await
            .map_err(|_| AdbError::Timeout)?
            .map_err(AdbError::Spawn)?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if output.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let message = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            Err(AdbError::Failed(message.to_owned()))
        }
    }

    pub async fn version(&self) -> Result<String, AdbError> {
        let out = self.run(&["version"], Duration::from_secs(10)).await?;
        Ok(out.lines().next().unwrap_or_default().trim().to_owned())
    }

    pub async fn devices(&self) -> Result<Vec<Device>, AdbError> {
        // The first call starts adb's server, which can take a few seconds.
        let out = self
            .run(&["devices", "-l"], Duration::from_secs(20))
            .await?;
        Ok(parse_devices(&out))
    }

    /// `adb pair`: wireless debugging's pairing, with the code from the phone.
    pub async fn pair(&self, address: &str, code: &str) -> Result<(), AdbError> {
        let out = self
            .run(&["pair", address, code], Duration::from_secs(30))
            .await?;
        // adb says so on stdout and exits with 0 either way.
        if out.contains("Successfully paired") {
            Ok(())
        } else {
            Err(AdbError::Failed(out.trim().to_owned()))
        }
    }

    pub async fn connect(&self, address: &str) -> Result<(), AdbError> {
        let out = self
            .run(&["connect", address], Duration::from_secs(20))
            .await?;
        if out.contains("connected to") {
            Ok(())
        } else {
            Err(AdbError::Failed(out.trim().to_owned()))
        }
    }

    pub async fn disconnect(&self, serial: &str) -> Result<(), AdbError> {
        self.run(&["disconnect", serial], Duration::from_secs(10))
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devices_l() {
        let out = "* daemon started successfully\nList of devices attached\n\
            R58N123ABC             device usb:1-1 product:beyond1 model:SM_G973F device:beyond1 transport_id:1\n\
            adb-2B091FDH2004HN-abc._adb-tls-connect._tcp device product:husky model:Pixel_8_Pro device:husky transport_id:2\n\
            192.168.1.23:5555      unauthorized transport_id:3\n\n";
        let devices = parse_devices(out);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].model.as_deref(), Some("SM G973F"));
        assert!(!devices[0].wireless);
        assert_eq!(devices[1].model.as_deref(), Some("Pixel 8 Pro"));
        assert!(devices[1].wireless);
        assert_eq!(devices[2].state, "unauthorized");
        assert!(devices[2].wireless);
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }
}
