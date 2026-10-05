//! The stream model: one stream per mirrored device, told as events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::Serialize;

/// Where a stream comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    /// Screen mirroring from an iPhone, iPad or Mac.
    Airplay,
    /// AirPlay sound only (Music, Podcasts, …): no picture.
    AirplayAudio,
    /// An Android phone, through scrcpy's server over adb.
    Android,
    /// Another computer running UwUMirror, over UwUCast.
    Cast,
}

/// What the app shows about a stream while it runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    pub id: u64,
    pub kind: StreamKind,
    /// The name the device gives itself ("Lorins iPhone", "Pixel 8").
    pub name: String,
    /// A model identifier when the device tells one ("iPhone15,2").
    pub model: Option<String>,
    /// The address it connected from, or the adb serial.
    pub address: String,
}

/// How sound is doing for a stream, so the page can say why it is silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AudioStatus {
    Playing,
    /// The device sends sound, but no decoder for it was found (AirPlay's
    /// AAC-ELD needs FFmpeg's libavcodec on the system).
    NoDecoder,
    /// No sound output device could be opened.
    NoOutput,
    /// The device doesn't send sound (Android before 11, or turned off).
    Unavailable,
    Off,
}

/// One H.264 access unit in Annex B form (start codes, not length prefixes).
#[derive(Debug, Clone)]
pub struct VideoPacket {
    pub data: Vec<u8>,
    /// An IDR frame: a decoder can start here.
    pub key: bool,
    /// Presentation time in microseconds, on the source's own clock.
    pub pts_us: u64,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Started(StreamInfo),
    /// The picture's size changed (rotation, a new resolution).
    VideoSize {
        id: u64,
        width: u32,
        height: u32,
    },
    Video {
        id: u64,
        packet: VideoPacket,
    },
    /// The device paused its picture (screen locked, app in the background).
    VideoPaused {
        id: u64,
    },
    Audio {
        id: u64,
        status: AudioStatus,
    },
    /// A stream ended; `reason` is set when it didn't end on the device's wish.
    Ended {
        id: u64,
        reason: Option<String>,
    },
}

impl StreamEvent {
    pub fn id(&self) -> u64 {
        match self {
            StreamEvent::Started(info) => info.id,
            StreamEvent::VideoSize { id, .. }
            | StreamEvent::Video { id, .. }
            | StreamEvent::VideoPaused { id }
            | StreamEvent::Audio { id, .. }
            | StreamEvent::Ended { id, .. } => *id,
        }
    }
}

/// Where sources deliver their events. Called from the source's own tasks, so
/// it must be cheap and never block for long.
pub type EventSink = Arc<dyn Fn(StreamEvent) + Send + Sync>;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A stream id no other stream in this run has had.
pub fn next_stream_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
