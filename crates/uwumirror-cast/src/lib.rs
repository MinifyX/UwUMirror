//! UwUCast: one UwUMirror sending its screen to another.
//!
//! A Windows PC can't AirPlay, and Miracast needs Wi-Fi Direct and a
//! different setup on every system. So UwUMirror speaks a small protocol of
//! its own between two of its copies: the sender announces nothing and
//! connects; the receiver announces `_uwumirror._tcp` on mDNS
//! ([`discovery`]), takes the connection ([`receiver`]) and turns it into
//! [`StreamEvent`]s like every other source. The wire format is in
//! [`protocol`].
//!
//! What travels is what the receiving side's page and player already take:
//! H.264 access units in Annex B, as from an iPhone or scrcpy, and raw PCM,
//! as from an Android phone. Nothing is decoded in Rust on either side.
//!
//! Sending ([`sender`] for the network, `screen` for the rest) is Windows
//! only: the screen comes from Windows.Graphics.Capture, the H.264 from Media
//! Foundation's encoder — the graphics card's, or Windows' own — and the
//! sound from WASAPI's loopback. Receiving works everywhere.
//!
//! [`StreamEvent`]: uwumirror_core::StreamEvent

pub mod discovery;
pub mod h264;
pub mod latency;
pub mod protocol;
pub mod receiver;
#[cfg(windows)]
pub mod screen;
pub mod sender;

pub use discovery::{instance_id, Browser, Found};
pub use receiver::{CastReceiver, ReceiverConfig, ReceiverError};
pub use sender::{Ending, SendError};

/// Whether this build can send its screen.
pub const CAN_SEND: bool = cfg!(windows);
