//! The receiving side: other UwUMirrors send their screen here.
//!
//! Like the AirPlay receiver, it is open to the local network while it is
//! switched on; each connection is one stream, and ending the stream closes
//! the connection, which the sender sees and stops.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{AbortHandle, JoinHandle};
use uwumirror_core::audio::AudioPlayer;
use uwumirror_core::{
    next_stream_id, AudioStatus, EventSink, StreamEvent, StreamInfo, StreamKind, VideoPacket,
};

use crate::discovery::Announcement;
use crate::latency;
use crate::protocol::{
    Hello, Message, Welcome, WelcomeStatus, AUDIO_CHANNELS, AUDIO_RATE, VERSION,
};

/// Next to AirPlay's 7000. Taken, the receiver takes any port: mDNS tells
/// senders which.
pub const PREFERRED_PORT: u16 = 7100;
/// A sender that says nothing for this long is gone (it sends a picture
/// many times a second, even of a still screen).
const IDLE: Duration = Duration::from_secs(15);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a receiver waiting for a key frame asks again.
const KEY_FRAME_ASK_EVERY: Duration = Duration::from_secs(1);
/// How often the latency goes into the log.
const LATENCY_EVERY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// The name senders show in their list.
    pub name: String,
    /// Play the sound that comes with a stream.
    pub audio: bool,
    /// Announce on mDNS. Off in tests, which connect by address.
    pub announce: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiverError {
    #[error("no network port for receiving: {0}")]
    Listen(std::io::Error),
    #[error("announcing the receiver on the network: {0}")]
    Announce(#[from] mdns_sd::Error),
}

struct Shared {
    name: String,
    sink: EventSink,
    audio: AtomicBool,
    /// Open connections, by connection number: their task and stream id.
    connections: Mutex<HashMap<u64, (AbortHandle, Arc<AtomicU64>)>>,
}

/// The running receiver. Dropping it ends every stream and takes it off the
/// network.
pub struct CastReceiver {
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
    port: u16,
    _announcement: Option<Announcement>,
}

async fn listen() -> std::io::Result<TcpListener> {
    if let Ok(listener) = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], PREFERRED_PORT))).await
    {
        return Ok(listener);
    }
    TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await
}

impl CastReceiver {
    pub async fn start(config: ReceiverConfig, sink: EventSink) -> Result<Self, ReceiverError> {
        let listener = listen().await.map_err(ReceiverError::Listen)?;
        let port = listener.local_addr().map_err(ReceiverError::Listen)?.port();
        let announcement = if config.announce {
            Some(Announcement::start(&config.name, port)?)
        } else {
            None
        };
        let shared = Arc::new(Shared {
            name: config.name.clone(),
            sink,
            audio: AtomicBool::new(config.audio),
            connections: Mutex::new(HashMap::new()),
        });
        let accept = tokio::spawn(accept_loop(listener, shared.clone()));
        tracing::info!(port, name = %config.name, "UwUCast receiver on");
        Ok(Self {
            shared,
            accept,
            port,
            _announcement: announcement,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Ends one stream by closing its connection; the sender stops.
    pub fn end_stream(&self, id: u64) -> bool {
        let connections = self.shared.connections.lock();
        for (abort, stream) in connections.values() {
            if stream.load(Ordering::Relaxed) == id {
                abort.abort();
                return true;
            }
        }
        false
    }

    /// Sound on or off for streams that start from now on.
    pub fn set_audio(&self, on: bool) {
        self.shared.audio.store(on, Ordering::Relaxed);
    }
}

impl Drop for CastReceiver {
    fn drop(&mut self) {
        self.accept.abort();
        for (abort, _) in self.shared.connections.lock().values() {
            abort.abort();
        }
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut next = 0u64;
    loop {
        let (socket, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "UwUCast accept");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };
        let _ = socket.set_nodelay(true);
        next += 1;
        let number = next;
        let stream_id = Arc::new(AtomicU64::new(0));
        // Held while spawning, so a connection that ends at once can't remove
        // itself before it was added.
        let mut connections = shared.connections.lock();
        let task = tokio::spawn({
            let shared = shared.clone();
            let stream_id = stream_id.clone();
            async move {
                if let Err(error) = serve(socket, peer, &shared, &stream_id).await {
                    tracing::info!(%peer, %error, "UwUCast sender turned away");
                }
                shared.connections.lock().remove(&number);
            }
        });
        connections.insert(number, (task.abort_handle(), stream_id));
    }
}

/// Says the stream ended however the connection's task ends — returned,
/// failed, or aborted by [`CastReceiver::end_stream`].
struct Running {
    id: u64,
    sink: EventSink,
    reason: Option<String>,
}

impl Drop for Running {
    fn drop(&mut self) {
        (self.sink)(StreamEvent::Ended {
            id: self.id,
            reason: self.reason.take(),
        });
    }
}

/// Before a stream starts: the hello and our welcome. An error here means the
/// connection never became a stream.
async fn serve(
    mut socket: TcpStream,
    peer: SocketAddr,
    shared: &Shared,
    stream_id: &AtomicU64,
) -> std::io::Result<()> {
    let hello = tokio::time::timeout(HELLO_TIMEOUT, Hello::read(&mut socket))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "no hello"))??;
    let status = if hello.version == VERSION {
        WelcomeStatus::Accepted
    } else {
        WelcomeStatus::OtherVersion
    };
    let welcome = Welcome {
        version: VERSION,
        status,
        name: shared.name.clone(),
    };
    socket.write_all(&welcome.encode()).await?;
    if status != WelcomeStatus::Accepted {
        return Err(std::io::Error::other(format!(
            "speaks UwUCast {}, we speak {VERSION}",
            hello.version
        )));
    }
    tracing::info!(%peer, name = %hello.name, model = %hello.model, "UwUCast sender connected");

    let id = next_stream_id();
    stream_id.store(id, Ordering::Relaxed);
    (shared.sink)(StreamEvent::Started(StreamInfo {
        id,
        kind: StreamKind::Cast,
        name: if hello.name.trim().is_empty() {
            peer.ip().to_string()
        } else {
            hello.name.clone()
        },
        model: (!hello.model.is_empty()).then(|| hello.model.clone()),
        address: peer.ip().to_string(),
    }));
    let mut running = Running {
        id,
        sink: shared.sink.clone(),
        reason: None,
    };
    let audio = match (hello.audio, shared.audio.load(Ordering::Relaxed)) {
        (false, _) => {
            (shared.sink)(StreamEvent::Audio {
                id,
                status: AudioStatus::Unavailable,
            });
            false
        }
        (true, false) => {
            (shared.sink)(StreamEvent::Audio {
                id,
                status: AudioStatus::Off,
            });
            false
        }
        (true, true) => true,
    };
    running.reason = pump(socket, id, audio, &shared.sink).await;
    Ok(())
}

/// Capture to received, and the bit rate, into the log now and then (the
/// app's detailed log, or `RUST_LOG=uwumirror_cast=debug`).
fn log_latency(id: u64, stats: &latency::Stats, over: Duration) {
    let mbit = stats.bytes as f64 * 8.0 / over.as_secs_f64() / 1e6;
    match stats.summary() {
        Some(summary) => {
            tracing::debug!(id, "UwUCast capture → received: {summary}, {mbit:.1} Mbit/s");
        }
        None if stats.implausible > 0 => tracing::debug!(
            id,
            "UwUCast: {} frames stamped on a clock too far from ours to tell latency, {mbit:.1} Mbit/s",
            stats.implausible
        ),
        None => {}
    }
}

/// The stream's messages until it ends; the reason, if it didn't end well.
async fn pump(socket: TcpStream, id: u64, audio: bool, sink: &EventSink) -> Option<String> {
    let (mut reader, mut writer) = socket.into_split();
    let mut player: Option<AudioPlayer> = None;
    let mut sound_failed = false;
    let mut started = false;
    let mut asked: Option<Instant> = None;
    let mut latency = latency::Stats::default();
    let mut summed = Instant::now();
    loop {
        let message = match tokio::time::timeout(IDLE, Message::read(&mut reader)).await {
            Err(_) => return Some("the sender went silent".into()),
            // Closed between two messages: the sender is done.
            Ok(Err(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => return None,
            Ok(Err(error)) => return Some(error.to_string()),
            Ok(Ok(message)) => message,
        };
        match message {
            Message::Video { key, pts_us, data } => {
                // A decoder can only start at a key frame; until one comes,
                // ask for it instead of passing on what can't be shown.
                started |= key;
                if !started {
                    if asked.is_none_or(|at| at.elapsed() >= KEY_FRAME_ASK_EVERY) {
                        asked = Some(Instant::now());
                        let _ = Message::KeyFrameRequest.write(&mut writer).await;
                    }
                    continue;
                }
                latency.frame(pts_us, data.len());
                if summed.elapsed() >= LATENCY_EVERY {
                    log_latency(id, &latency, summed.elapsed());
                    latency.clear();
                    summed = Instant::now();
                }
                sink(StreamEvent::Video {
                    id,
                    packet: VideoPacket { data, key, pts_us },
                });
            }
            Message::VideoSize { width, height } => {
                sink(StreamEvent::VideoSize { id, width, height });
            }
            Message::Audio { samples, .. } => {
                if !audio || sound_failed {
                    continue;
                }
                if player.is_none() {
                    match AudioPlayer::open(AUDIO_RATE, AUDIO_CHANNELS) {
                        Ok(opened) => {
                            player = Some(opened);
                            sink(StreamEvent::Audio {
                                id,
                                status: AudioStatus::Playing,
                            });
                        }
                        Err(error) => {
                            // Failing sound never ends the picture.
                            tracing::warn!(%error, "sound output");
                            sound_failed = true;
                            sink(StreamEvent::Audio {
                                id,
                                status: AudioStatus::NoOutput,
                            });
                            continue;
                        }
                    }
                }
                if let Some(player) = &player {
                    player.push_i16(&samples);
                }
            }
            Message::End => return None,
            Message::KeyFrameRequest => {
                return Some("the sender spoke out of turn".into());
            }
        }
    }
}
