//! Google's platform-tools (adb), downloaded on request.
//!
//! Only when the user asks for it, from Google's own address, into the app's
//! data folder — never shipped with UwUMirror, whose license doesn't cover
//! Google's terms. Google builds them for Windows (x64; Windows on ARM runs
//! them emulated), macOS (universal) and Linux x64. On Linux ARM the distro's
//! `adb` / `android-tools` package is the way.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("Google offers no platform-tools for this system")]
    Unsupported,
    #[error("download failed: {0}")]
    Http(String),
    #[error("the download is damaged: {0}")]
    Zip(String),
    #[error("writing the files: {0}")]
    Io(#[from] std::io::Error),
}

pub fn url() -> Option<&'static str> {
    if cfg!(windows) {
        Some("https://dl.google.com/android/repository/platform-tools-latest-windows.zip")
    } else if cfg!(target_os = "macos") {
        Some("https://dl.google.com/android/repository/platform-tools-latest-darwin.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("https://dl.google.com/android/repository/platform-tools-latest-linux.zip")
    } else {
        None
    }
}

/// Whether this system can get platform-tools from Google.
pub fn supported() -> bool {
    url().is_some()
}

/// Unpacks the `platform-tools/` folder of Google's zip into `dir`.
pub fn unpack(zip: &[u8], dir: &Path) -> Result<PathBuf, DownloadError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip)).map_err(|e| DownloadError::Zip(e.to_string()))?;
    // Into a fresh folder next to the old one, then swapped in, so a failed
    // unpack never leaves half an adb behind.
    let fresh = dir.join("platform-tools.new");
    if fresh.exists() {
        std::fs::remove_dir_all(&fresh)?;
    }
    std::fs::create_dir_all(&fresh)?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| DownloadError::Zip(e.to_string()))?;
        // `enclosed_name` refuses paths that climb out ("../") or are absolute.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Ok(relative) = name.strip_prefix("platform-tools") else {
            continue;
        };
        let target = fresh.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        std::fs::write(&target, bytes)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o755))?;
        }
    }
    let adb = fresh.join(if cfg!(windows) { "adb.exe" } else { "adb" });
    if !adb.is_file() {
        std::fs::remove_dir_all(&fresh)?;
        return Err(DownloadError::Zip("no adb inside".into()));
    }
    let target = dir.join("platform-tools");
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::rename(&fresh, &target)?;
    Ok(target.join(if cfg!(windows) { "adb.exe" } else { "adb" }))
}

/// Downloads and unpacks; blocking, so call it off the async runtime.
pub fn download(dir: &Path) -> Result<PathBuf, DownloadError> {
    let url = url().ok_or(DownloadError::Unsupported)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let bytes = response
        .bytes()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    std::fs::create_dir_all(dir)?;
    unpack(&bytes, dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut out));
            let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        out
    }

    #[test]
    fn unpacks_only_platform_tools_and_never_outside() {
        let dir = std::env::temp_dir().join(format!("uwumirror-pt-{}", std::process::id()));
        let adb = if cfg!(windows) {
            "platform-tools/adb.exe"
        } else {
            "platform-tools/adb"
        };
        let zip = zip_with(&[
            (adb, b"#!/bin/sh\n"),
            ("platform-tools/lib64/libc++.so", b"x"),
            ("../evil", b"x"),
            ("other/readme", b"x"),
        ]);
        let path = unpack(&zip, &dir).unwrap();
        assert!(path.is_file());
        assert!(dir.join("platform-tools/lib64/libc++.so").is_file());
        assert!(!dir.join("other").exists());
        assert!(!dir.parent().unwrap().join("evil").exists());
        // Again, over the first one.
        unpack(&zip, &dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_zip_without_adb_is_refused() {
        let dir = std::env::temp_dir().join(format!("uwumirror-pt2-{}", std::process::id()));
        let zip = zip_with(&[("platform-tools/fastboot", b"x")]);
        assert!(unpack(&zip, &dir).is_err());
        assert!(!dir.join("platform-tools").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
