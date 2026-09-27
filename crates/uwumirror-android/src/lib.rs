//! Android phones on UwUMirror: found, paired and mirrored through adb.
//!
//! [`Android`] ties it together for the app: which `adb` to use (or fetching
//! Google's), the phones adb knows, pairing a new one over wireless debugging
//! (QR code or pairing code), and starting and stopping mirrors with scrcpy's
//! server. Streams come out as the same [`StreamEvent`]s AirPlay produces.
//!
//! [`StreamEvent`]: uwumirror_core::StreamEvent

pub mod adb;
pub mod pairing;
pub mod platform_tools;
pub mod scrcpy;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use uwumirror_core::EventSink;

pub use adb::{Adb, AdbError, Device};
pub use pairing::QrPairing;
pub use scrcpy::MirrorOptions;

#[derive(Debug, thiserror::Error)]
pub enum AndroidError {
    #[error("adb was not found")]
    NoAdb,
    #[error(transparent)]
    Adb(#[from] AdbError),
    #[error(transparent)]
    Pairing(#[from] pairing::PairingError),
    #[error(transparent)]
    Mirror(#[from] scrcpy::ScrcpyError),
    #[error(transparent)]
    Download(#[from] platform_tools::DownloadError),
    #[error("{0} is already being mirrored")]
    AlreadyMirrored(String),
}

/// What the settings page shows about adb.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbStatus {
    pub path: Option<String>,
    pub version: Option<String>,
    /// Whether the app can fetch Google's platform-tools for this system.
    pub can_download: bool,
    /// Whether the adb in use is the one the app fetched.
    pub own: bool,
}

pub struct Android {
    /// The app's folder for platform-tools it downloaded.
    own_dir: PathBuf,
    /// scrcpy's server, shipped as a resource of the app.
    server: PathBuf,
    chosen: Mutex<Option<PathBuf>>,
    mirrors: Mutex<HashMap<u64, scrcpy::MirrorHandle>>,
    sink: EventSink,
}

impl Android {
    pub fn new(own_dir: PathBuf, server: PathBuf, sink: EventSink) -> Self {
        Self {
            own_dir,
            server,
            chosen: Mutex::new(None),
            mirrors: Mutex::new(HashMap::new()),
            sink,
        }
    }

    /// Use this `adb` instead of looking for one (`None`: look again).
    pub fn choose_adb(&self, path: Option<PathBuf>) {
        *self.chosen.lock() = path;
    }

    pub fn adb(&self) -> Option<Adb> {
        let chosen = self.chosen.lock().clone();
        adb::find(Some(&self.own_dir), chosen.as_deref()).map(|path| Adb { path })
    }

    fn require(&self) -> Result<Adb, AndroidError> {
        self.adb().ok_or(AndroidError::NoAdb)
    }

    pub async fn status(&self) -> AdbStatus {
        let adb = self.adb();
        let version = match &adb {
            Some(adb) => adb.version().await.ok(),
            None => None,
        };
        AdbStatus {
            own: adb
                .as_ref()
                .is_some_and(|adb| adb.path.starts_with(&self.own_dir)),
            path: adb.map(|adb| adb.path.to_string_lossy().into_owned()),
            version,
            can_download: platform_tools::supported(),
        }
    }

    pub async fn devices(&self) -> Result<Vec<Device>, AndroidError> {
        Ok(self.require()?.devices().await?)
    }

    /// After pairing, the phone's debugging port shows up on mDNS; connect to
    /// it. adb's own mDNS may beat us to it, which is fine.
    async fn connect_after_pairing(&self, adb: &Adb, paired: SocketAddr) {
        match pairing::wait_for_connect(paired.ip(), Duration::from_secs(10)).await {
            Ok(address) => {
                if let Err(error) = adb.connect(&address.to_string()).await {
                    tracing::info!(%error, "connecting after pairing");
                }
            }
            Err(error) => tracing::info!(%error, "no debugging port announced after pairing"),
        }
    }

    /// Pairs with the address and six digits from "Pair device with pairing code".
    pub async fn pair_with_code(&self, address: &str, code: &str) -> Result<(), AndroidError> {
        let adb = self.require()?;
        adb.pair(address, code).await?;
        if let Ok(paired) = address.parse::<SocketAddr>() {
            self.connect_after_pairing(&adb, paired).await;
        }
        Ok(())
    }

    /// Waits for the phone that scans `qr` and pairs with it.
    pub async fn pair_with_qr(
        &self,
        qr: &QrPairing,
        timeout: Duration,
    ) -> Result<(), AndroidError> {
        let adb = self.require()?;
        let address = pairing::wait_for_pairing(&qr.name, timeout).await?;
        adb.pair(&address.to_string(), &qr.password).await?;
        self.connect_after_pairing(&adb, address).await;
        Ok(())
    }

    /// `adb connect host:port`, for phones with `adb tcpip` or a known port.
    pub async fn connect(&self, address: &str) -> Result<(), AndroidError> {
        Ok(self.require()?.connect(address).await?)
    }

    pub async fn disconnect(&self, serial: &str) -> Result<(), AndroidError> {
        Ok(self.require()?.disconnect(serial).await?)
    }

    /// Starts mirroring a phone; returns the stream id.
    pub async fn mirror(&self, serial: &str, options: &MirrorOptions) -> Result<u64, AndroidError> {
        if self.mirrors.lock().values().any(|m| m.serial == serial) {
            return Err(AndroidError::AlreadyMirrored(serial.to_owned()));
        }
        let adb = self.require()?;
        let model = adb
            .devices()
            .await
            .ok()
            .and_then(|devices| devices.into_iter().find(|d| d.serial == serial))
            .and_then(|device| device.model);
        let handle = scrcpy::start(
            &adb,
            serial,
            model,
            &self.server,
            options,
            self.sink.clone(),
        )
        .await?;
        let id = handle.id;
        self.mirrors.lock().insert(id, handle);
        Ok(id)
    }

    /// Ends a mirror; its `Ended` event follows.
    pub fn stop(&self, id: u64) -> bool {
        match self.mirrors.lock().remove(&id) {
            Some(mut handle) => {
                handle.stop();
                true
            }
            None => false,
        }
    }

    /// Forgets a mirror that ended on its own.
    pub fn ended(&self, id: u64) {
        self.mirrors.lock().remove(&id);
    }

    pub fn stop_all(&self) {
        for (_, mut handle) in self.mirrors.lock().drain() {
            handle.stop();
        }
    }

    /// Fetches Google's platform-tools into the app's folder.
    pub async fn download_adb(&self) -> Result<PathBuf, AndroidError> {
        let dir = self.own_dir.clone();
        let path = tokio::task::spawn_blocking(move || platform_tools::download(&dir))
            .await
            .map_err(|e| platform_tools::DownloadError::Http(e.to_string()))??;
        Ok(path)
    }

    pub fn own_dir(&self) -> &Path {
        &self.own_dir
    }
}
