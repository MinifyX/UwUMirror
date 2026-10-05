//! What every UwUMirror source shares: the stream model the app listens to,
//! the sound output, and the audio decoder for AirPlay.
//!
//! A *source* (the AirPlay receiver, an Android phone) turns whatever arrives
//! on the network into [`StreamEvent`]s: a stream starts, its video comes in
//! as H.264 access units in Annex B form, it ends. Video is never decoded in
//! Rust — the page decodes it with the system's own decoder (WebCodecs, or
//! Media Source as a fallback), which is fast, hardware-backed and carries no
//! patent baggage for us. Miracast is the exception that proves it: Windows
//! decodes that one itself and hands over pictures, which travel as
//! [`RawFrame`]s. Sound is decoded here and played with [`audio`].

pub mod audio;
pub mod decode;
pub mod stream;

pub use stream::{
    next_stream_id, AudioStatus, EventSink, RawFrame, StreamEvent, StreamInfo, StreamKind,
    VideoPacket,
};
