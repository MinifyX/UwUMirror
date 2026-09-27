//! An AirPlay receiver: screen mirroring and sound from iPhones, iPads and Macs.
//!
//! [`Receiver::start`] announces UwUMirror on the network, listens for
//! senders and turns each one into a stream of [`StreamEvent`]s. It speaks
//! the AirPlay dialect the open-source receivers before it worked out (UxPlay,
//! RPiPlay, shairplay): legacy pairing, FairPlay (see `playfair/`), H.264
//! mirroring and RAOP audio. There is no PIN: while the receiver is switched
//! on, anyone on the local network can mirror to it — the app shows who is
//! connected and can end a stream with one click.
//!
//! [`StreamEvent`]: uwumirror_core::StreamEvent

mod advertise;
mod fairplay;
mod mirror;
mod pairing;
mod rtsp;
mod session;
mod sound;
mod timing;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::net::TcpListener;
use tokio::task::{AbortHandle, JoinHandle};
use uwumirror_core::{EventSink, StreamInfo};

pub use advertise::Offer;
pub use pairing::Identity;

/// The port Apple TVs use. Taken (by macOS's own AirPlay receiver, say),
/// UwUMirror moves on: Bonjour tells senders the port anyway.
const PREFERRED_PORT: u16 = 7000;

#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// The name senders show in their AirPlay list.
    pub name: String,
    /// The picture size and frame rate senders are asked for.
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Play the sound that comes with a stream.
    pub audio: bool,
    /// Where the receiver's lasting key lives.
    pub identity_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiverError {
    #[error("no network port for AirPlay: {0}")]
    Listen(std::io::Error),
    #[error("the receiver's key: {0}")]
    Identity(std::io::Error),
    #[error("announcing the receiver on the network: {0}")]
    Announce(#[from] mdns_sd::Error),
}

/// What every connection shares.
pub(crate) struct Shared {
    identity: Identity,
    offer: Offer,
    sink: EventSink,
    audio: AtomicBool,
    /// Running streams, by id, for the app to list.
    streams: Mutex<HashMap<u64, StreamInfo>>,
    /// Open connections, by connection number.
    connections: Mutex<HashMap<u64, (AbortHandle, Arc<AtomicU64>)>>,
}

impl Shared {
    fn audio_enabled(&self) -> bool {
        self.audio.load(Ordering::Relaxed)
    }
}

/// The running receiver. Dropping it ends every stream and takes the receiver
/// off the network.
pub struct Receiver {
    shared: Arc<Shared>,
    accept: JoinHandle<()>,
    port: u16,
    _announcement: advertise::Announcement,
}

async fn listen() -> Result<TcpListener, std::io::Error> {
    for port in PREFERRED_PORT..PREFERRED_PORT + 10 {
        if let Ok(listener) = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
            return Ok(listener);
        }
    }
    TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await
}

impl Receiver {
    pub async fn start(config: ReceiverConfig, sink: EventSink) -> Result<Self, ReceiverError> {
        let identity =
            Identity::load_or_create(&config.identity_path).map_err(ReceiverError::Identity)?;
        let listener = listen().await.map_err(ReceiverError::Listen)?;
        let port = listener.local_addr().map_err(ReceiverError::Listen)?.port();
        let announcement = advertise::Announcement::start(&identity, &config.name, port)?;
        let shared = Arc::new(Shared {
            identity,
            offer: Offer {
                name: config.name.clone(),
                width: config.width,
                height: config.height,
                fps: config.fps,
            },
            sink,
            audio: AtomicBool::new(config.audio),
            streams: Mutex::new(HashMap::new()),
            connections: Mutex::new(HashMap::new()),
        });
        let accept = tokio::spawn(accept_loop(listener, shared.clone()));
        tracing::info!(port, name = %config.name, "AirPlay receiver on");
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

    /// The streams running right now.
    pub fn streams(&self) -> Vec<StreamInfo> {
        self.shared.streams.lock().values().cloned().collect()
    }

    /// Ends one stream by closing its sender's connection. The sender sees
    /// the receiver go away and stops mirroring.
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

impl Drop for Receiver {
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
                tracing::warn!(%error, "AirPlay accept");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
        };
        let _ = socket.set_nodelay(true);
        next += 1;
        let number = next;
        let stream_id = Arc::new(AtomicU64::new(0));
        let connection = session::Connection::new(shared.clone(), peer, stream_id.clone());
        // Held while spawning, so a connection that ends at once can't remove
        // itself before it was added.
        let mut connections = shared.connections.lock();
        let task = tokio::spawn({
            let shared = shared.clone();
            async move {
                tracing::info!(%peer, "AirPlay sender connected");
                connection.run(socket).await;
                shared.connections.lock().remove(&number);
            }
        });
        connections.insert(number, (task.abort_handle(), stream_id));
    }
}

#[cfg(test)]
mod tests;
