//! Miracast, received through Windows' own receiver.
//!
//! Most Android phones can cast their screen over Miracast ("Smart View",
//! "Screen mirroring", "Cast"), and so can Windows PCs (Win+K). Miracast rides
//! Wi-Fi Direct, which no app can drive by itself, but Windows 10 and 11 have
//! a receiver built in — the one behind "Projecting to this PC" — and lend it
//! to apps as `Windows.Media.Miracast.MiracastReceiver`. That works from an
//! ordinary desktop program: no package, no capability, no administrator, not
//! even the optional "Wireless Display" feature.
//!
//! The price: the receiver hands over a `MediaSource`, not the H.264 the phone
//! sent. So for once the picture is decoded outside the page — by Media
//! Foundation, on the graphics card — and `MediaPlayer`'s frame server copies
//! each picture into a Direct3D texture of ours, which is read back as NV12
//! and passed on as [`RawFrame`](uwumirror_core::RawFrame)s. The sound plays
//! through that same `MediaPlayer`, on the default output device.
//!
//! On every other system [`Receiver::start`] reports [`Error::Unsupported`].

mod frame;
#[cfg(windows)]
mod receiver;
mod status;

use std::sync::Arc;

pub use frame::{fit, pack_nv12, MAX_LONG_SIDE, MAX_SHORT_SIDE};
#[cfg(windows)]
pub use receiver::{FilePlayback, Receiver};
pub use status::{MiracastState, MiracastStatus};

/// Told whenever the receiver's state changes (and once when it starts).
pub type StatusSink = Arc<dyn Fn(MiracastStatus) + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Miracast needs Windows")]
    Unsupported,
    #[error("Windows' Miracast receiver: {0}")]
    Windows(String),
}

#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// Play the sender's sound (else the player stays muted).
    pub audio: bool,
}

/// The receiver everywhere but on Windows: it never starts.
#[cfg(not(windows))]
pub struct Receiver;

#[cfg(not(windows))]
impl Receiver {
    pub fn start(
        _config: ReceiverConfig,
        _sink: uwumirror_core::EventSink,
        _on_status: StatusSink,
    ) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }

    pub fn status(&self) -> MiracastStatus {
        MiracastStatus::unsupported()
    }

    pub fn end_stream(&self, _id: u64) -> bool {
        false
    }

    pub fn set_audio(&self, _on: bool) {}
}

/// What phones would see, without starting anything: the name and whether
/// Wi-Fi Direct is there. For when the receiver is switched off.
#[cfg(not(windows))]
pub fn probe() -> MiracastStatus {
    MiracastStatus::unsupported()
}

#[cfg(windows)]
pub use receiver::probe;
