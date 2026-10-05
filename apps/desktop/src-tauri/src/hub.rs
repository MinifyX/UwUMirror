//! Where every source's events meet the page.
//!
//! Stream events become two kinds of messages: small JSON events for the
//! things the interface shows (a stream started, its size, sound, its end),
//! and binary messages on a Tauri channel for the video itself, which is too
//! much and too frequent for JSON.
//!
//! Video message layout (little endian):
//!
//! | bytes | field                        |
//! | ----- | ---------------------------- |
//! | 0..8  | stream id                    |
//! | 8     | flags: bit 0 = key frame     |
//! | 9..17 | presentation time, µs        |
//! | 17..  | H.264 access unit, Annex B   |
//!
//! The hub also keeps each stream's frames since its last key frame, so a
//! page that (re)subscribes — after a reload, or once the window first shows
//! — can start decoding at once instead of waiting for the sender's next key
//! frame, which an iPhone showing a still screen may not send for minutes.
//!
//! One device mirrors at a time: a stream that starts while another runs
//! takes its place, as on an Apple TV, and the hub asks for the old one to
//! be ended.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter};
use uwumirror_core::{AudioStatus, StreamEvent, StreamInfo, StreamKind, VideoPacket};

/// Frames kept per stream for a late subscriber, at most.
const MAX_CACHED_FRAMES: usize = 600;
const MAX_CACHED_BYTES: usize = 48 * 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamState {
    #[serde(flatten)]
    pub info: StreamInfo,
    pub width: u32,
    pub height: u32,
    pub audio: Option<AudioStatus>,
    pub paused: bool,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message<'a> {
    Started {
        stream: &'a StreamState,
    },
    Updated {
        stream: &'a StreamState,
    },
    Ended {
        id: u64,
        name: String,
        kind: StreamKind,
        reason: Option<String>,
    },
}

#[derive(Default)]
struct Cache {
    frames: Vec<VideoPacket>,
    bytes: usize,
    /// Over the limits: nothing more is kept until the next key frame.
    full: bool,
}

impl Cache {
    fn push(&mut self, packet: &VideoPacket) {
        if packet.key {
            self.frames.clear();
            self.bytes = 0;
            self.full = false;
        } else if self.full || self.frames.is_empty() {
            return;
        }
        if self.frames.len() >= MAX_CACHED_FRAMES
            || self.bytes + packet.data.len() > MAX_CACHED_BYTES
        {
            self.frames.clear();
            self.bytes = 0;
            self.full = true;
            return;
        }
        self.bytes += packet.data.len();
        self.frames.push(packet.clone());
    }
}

pub fn encode_video(id: u64, packet: &VideoPacket) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(17 + packet.data.len());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.push(u8::from(packet.key));
    bytes.extend_from_slice(&packet.pts_us.to_le_bytes());
    bytes.extend_from_slice(&packet.data);
    bytes
}

type EndCallback = Box<dyn Fn(u64) + Send + Sync>;

pub struct Hub {
    app: AppHandle,
    streams: Mutex<HashMap<u64, StreamState>>,
    caches: Mutex<HashMap<u64, Cache>>,
    video: Mutex<Option<Channel<InvokeResponseBody>>>,
    /// Told when a stream ends, e.g. so the Android side forgets its mirror.
    on_end: Mutex<Vec<EndCallback>>,
    /// Told to end a stream that a newer one replaces.
    on_replace: Mutex<Option<EndCallback>>,
}

impl Hub {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            streams: Mutex::new(HashMap::new()),
            caches: Mutex::new(HashMap::new()),
            video: Mutex::new(None),
            on_end: Mutex::new(Vec::new()),
            on_replace: Mutex::new(None),
        })
    }

    pub fn on_replace(&self, callback: impl Fn(u64) + Send + Sync + 'static) {
        *self.on_replace.lock() = Some(Box::new(callback));
    }

    pub fn on_end(&self, callback: impl Fn(u64) + Send + Sync + 'static) {
        self.on_end.lock().push(Box::new(callback));
    }

    pub fn streams(&self) -> Vec<StreamState> {
        let mut streams: Vec<StreamState> = self.streams.lock().values().cloned().collect();
        streams.sort_by_key(|s| s.info.id);
        streams
    }

    /// The page's video channel; replays what each stream needs to start.
    pub fn subscribe(&self, channel: Channel<InvokeResponseBody>) {
        for (id, cache) in self.caches.lock().iter() {
            for packet in &cache.frames {
                let _ = channel.send(InvokeResponseBody::Raw(encode_video(*id, packet)));
            }
        }
        *self.video.lock() = Some(channel);
    }

    fn emit(&self, message: Message<'_>) {
        if let Err(error) = self.app.emit("stream", message) {
            tracing::warn!(%error, "stream event to the page");
        }
    }

    fn update(&self, id: u64, change: impl FnOnce(&mut StreamState)) {
        let mut streams = self.streams.lock();
        if let Some(state) = streams.get_mut(&id) {
            change(state);
            let state = state.clone();
            drop(streams);
            self.emit(Message::Updated { stream: &state });
        }
    }

    pub fn handle(&self, event: StreamEvent) {
        match event {
            StreamEvent::Started(info) => {
                let id = info.id;
                let mut streams = self.streams.lock();
                let state = match streams.remove(&id) {
                    // AirPlay sound turning into mirroring keeps what it had.
                    Some(old) => StreamState { info, ..old },
                    None => StreamState {
                        info,
                        width: 0,
                        height: 0,
                        audio: None,
                        paused: false,
                    },
                };
                let replaced: Vec<u64> = streams.keys().copied().collect();
                streams.insert(id, state.clone());
                drop(streams);
                self.emit(Message::Started { stream: &state });
                if !replaced.is_empty() {
                    tracing::info!(new = id, old = ?replaced, "a new device replaces the mirroring one");
                    if let Some(replace) = self.on_replace.lock().as_ref() {
                        for old in replaced {
                            replace(old);
                        }
                    }
                }
            }
            StreamEvent::VideoSize { id, width, height } => self.update(id, |s| {
                s.width = width;
                s.height = height;
                s.paused = false;
            }),
            StreamEvent::VideoPaused { id } => self.update(id, |s| s.paused = true),
            StreamEvent::Audio { id, status } => self.update(id, |s| s.audio = Some(status)),
            StreamEvent::Video { id, packet } => {
                let resumed = {
                    let mut streams = self.streams.lock();
                    streams
                        .get_mut(&id)
                        .is_some_and(|s| std::mem::replace(&mut s.paused, false))
                };
                if resumed {
                    self.update(id, |_| {});
                }
                self.caches.lock().entry(id).or_default().push(&packet);
                if let Some(channel) = self.video.lock().as_ref() {
                    let _ = channel.send(InvokeResponseBody::Raw(encode_video(id, &packet)));
                }
            }
            StreamEvent::Ended { id, reason } => {
                self.caches.lock().remove(&id);
                let state = self.streams.lock().remove(&id);
                if let Some(state) = state {
                    self.emit(Message::Ended {
                        id,
                        name: state.info.name,
                        kind: state.info.kind,
                        reason,
                    });
                }
                for callback in self.on_end.lock().iter() {
                    callback(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(key: bool, len: usize) -> VideoPacket {
        VideoPacket {
            data: vec![0; len],
            key,
            pts_us: 0,
        }
    }

    #[test]
    fn cache_starts_at_a_key_frame_and_resets_on_the_next() {
        let mut cache = Cache::default();
        cache.push(&packet(false, 10));
        assert!(
            cache.frames.is_empty(),
            "nothing before the first key frame"
        );
        cache.push(&packet(true, 10));
        cache.push(&packet(false, 10));
        assert_eq!(cache.frames.len(), 2);
        cache.push(&packet(true, 10));
        assert_eq!(cache.frames.len(), 1);
    }

    #[test]
    fn cache_gives_up_when_too_big_until_the_next_key_frame() {
        let mut cache = Cache::default();
        cache.push(&packet(true, 10));
        cache.push(&packet(false, MAX_CACHED_BYTES));
        assert!(cache.frames.is_empty() && cache.full);
        cache.push(&packet(false, 10));
        assert!(cache.frames.is_empty());
        cache.push(&packet(true, 10));
        assert_eq!(cache.frames.len(), 1);
    }

    #[test]
    fn video_message_layout() {
        let bytes = encode_video(
            7,
            &VideoPacket {
                data: vec![0xaa, 0xbb],
                key: true,
                pts_us: 9,
            },
        );
        assert_eq!(&bytes[..8], &7u64.to_le_bytes());
        assert_eq!(bytes[8], 1);
        assert_eq!(&bytes[9..17], &9u64.to_le_bytes());
        assert_eq!(&bytes[17..], &[0xaa, 0xbb]);
    }
}
