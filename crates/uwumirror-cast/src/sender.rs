//! The sending side's network half, the same on every system: connect, say
//! hello, then hand messages from the capture threads to the connection.
//!
//! The capture threads never wait for the network, and the network never
//! holds more than it must. The socket's send buffer is kept small
//! ([`SEND_BUFFER`]), so frames can't pile up unseen in the kernel, and
//! behind the frame being written at most one more waits ([`VIDEO_QUEUE`]).
//! A frame that finds no room — the network is slower than the picture right
//! now — is dropped instead of queued, and so is every frame after it until
//! the next key frame, which is asked for at once: the frames after a dropped
//! one can't be decoded without it. Live beats complete, as with the
//! receiver's sound. Sound and everything else go in a queue of their own,
//! and before the picture when both wait: they are small, and sound that
//! comes late is heard.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::protocol::{Hello, Message, Welcome, WelcomeStatus, VERSION};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Frames waiting behind the one being written: one. More would only be
/// latency — each one waiting is a frame the picture is behind.
pub const VIDEO_QUEUE: usize = 1;
/// Sound and the rest: a sender sends sound every 10 ms, so this is about
/// a third of a second of it.
const QUEUE: usize = 32;
/// The kernel's send buffer. Left alone, Windows grows it to megabytes on a
/// slow link: seconds of video queued where no one can drop them. 256 kB
/// holds a few frames at the bit rates sent, and keeps a gigabit link (or
/// fast Wi-Fi) busy.
pub const SEND_BUFFER: u32 = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("the receiver can't be reached: {0}")]
    Connect(std::io::Error),
    #[error("the receiver didn't answer")]
    NoAnswer,
    #[error("the receiver speaks UwUCast {0}, this computer {VERSION}: update UwUMirror on both")]
    OtherVersion(u16),
    #[error("the receiver isn't taking streams right now")]
    Refused,
    #[error("{0}")]
    Capture(String),
}

/// How a sending session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// We stopped it.
    Stopped,
    /// The receiver closed the connection: someone ended the stream there,
    /// another device took over, or the app quit.
    ByReceiver,
    Failed(String),
}

/// Where the capture threads put what they made. Cheap to clone.
#[derive(Clone)]
pub struct Outbox {
    video: mpsc::Sender<Message>,
    /// Sound, the picture size and the end, in order.
    other: mpsc::Sender<Message>,
    /// The receiver asked for a key frame, or a frame was dropped.
    key_wanted: Arc<AtomicBool>,
    /// A frame was dropped: nothing but a key frame is worth sending now.
    broken: Arc<AtomicBool>,
}

impl Outbox {
    /// Hands over one access unit, or drops it when the one before is still
    /// waiting. False once the connection is gone.
    pub fn video(&self, key: bool, pts_us: u64, data: Vec<u8>) -> bool {
        if !key && self.broken.load(Ordering::Relaxed) {
            return !self.video.is_closed();
        }
        match self.video.try_send(Message::Video { key, pts_us, data }) {
            Ok(()) => {
                if key {
                    self.broken.store(false, Ordering::Relaxed);
                }
                true
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::debug!(key, "the network is behind: frame dropped");
                self.broken.store(true, Ordering::Relaxed);
                self.key_wanted.store(true, Ordering::Relaxed);
                true
            }
            Err(mpsc::error::TrySendError::Closed(_)) => false,
        }
    }

    /// Queues sound; dropped when the queue is full. False once closed.
    pub fn audio(&self, pts_us: u64, samples: Vec<i16>) -> bool {
        !matches!(
            self.other.try_send(Message::Audio { pts_us, samples }),
            Err(mpsc::error::TrySendError::Closed(_))
        )
    }

    /// Queues the end, if there is room. True once it is queued, or there is
    /// no connection left to end.
    pub fn try_end(&self) -> bool {
        !matches!(
            self.other.try_send(Message::End),
            Err(mpsc::error::TrySendError::Full(_))
        )
    }

    /// Queues something that must not be lost, waiting for room. Not for
    /// video, which never waits.
    pub async fn send(&self, message: Message) -> bool {
        self.other.send(message).await.is_ok()
    }

    /// Whether a key frame should come next, without saying it's coming.
    pub fn key_wanted(&self) -> bool {
        self.key_wanted.load(Ordering::Relaxed)
    }

    /// Whether a key frame should come next; asking resets it.
    pub fn take_key_request(&self) -> bool {
        self.key_wanted.swap(false, Ordering::Relaxed)
    }

    pub fn is_closed(&self) -> bool {
        self.video.is_closed()
    }
}

/// A connected, welcomed session.
pub struct Session {
    /// The receiver's name, as it says it.
    pub receiver: String,
    pub outbox: Outbox,
    /// Finishes when the connection does.
    pub done: JoinHandle<Ending>,
}

/// Connects to a receiver and says hello. Then [`Session::outbox`] takes
/// messages until [`Message::End`] or the connection ends.
pub async fn connect(address: SocketAddr, hello: &Hello) -> Result<Session, SendError> {
    let socket = if address.is_ipv4() {
        TcpSocket::new_v4()
    } else {
        TcpSocket::new_v6()
    }
    .map_err(SendError::Connect)?;
    // Before connecting: set later, some systems keep the window they
    // offered for the connection's whole life.
    let _ = socket.set_send_buffer_size(SEND_BUFFER);
    let mut socket = tokio::time::timeout(CONNECT_TIMEOUT, socket.connect(address))
        .await
        .map_err(|_| SendError::NoAnswer)?
        .map_err(SendError::Connect)?;
    // Every frame out the moment it is written, not when the next one fills
    // a packet.
    let _ = socket.set_nodelay(true);
    socket
        .write_all(&hello.encode())
        .await
        .map_err(SendError::Connect)?;
    let welcome = tokio::time::timeout(CONNECT_TIMEOUT, Welcome::read(&mut socket))
        .await
        .map_err(|_| SendError::NoAnswer)?
        .map_err(|_| SendError::NoAnswer)?;
    match welcome.status {
        WelcomeStatus::Accepted => {}
        WelcomeStatus::OtherVersion => return Err(SendError::OtherVersion(welcome.version)),
        WelcomeStatus::Refused => return Err(SendError::Refused),
    }

    let (mut reader, mut writer) = socket.into_split();
    let (video_tx, mut video_rx) = mpsc::channel::<Message>(VIDEO_QUEUE);
    let (other_tx, mut other_rx) = mpsc::channel::<Message>(QUEUE);
    let outbox = Outbox {
        video: video_tx,
        other: other_tx,
        key_wanted: Arc::new(AtomicBool::new(false)),
        broken: Arc::new(AtomicBool::new(false)),
    };
    let key_wanted = outbox.key_wanted.clone();
    let done = tokio::spawn(async move {
        let write = async {
            loop {
                // Sound and the rest first: small, and in order with the
                // picture where it matters (the size before any frame; the
                // end goes after the last one the picture thread queued, or
                // in place of it).
                let message = tokio::select! {
                    biased;
                    message = other_rx.recv() => message,
                    message = video_rx.recv() => message,
                };
                // Every outbox is gone: stopped without a word.
                let Some(message) = message else {
                    return Ending::Stopped;
                };
                // One write a message, header and all.
                if let Err(error) = writer.write_all(&message.encode()).await {
                    return Ending::Failed(error.to_string());
                }
                if message == Message::End {
                    let _ = writer.shutdown().await;
                    return Ending::Stopped;
                }
            }
        };
        let read = async {
            loop {
                match Message::read(&mut reader).await {
                    Ok(Message::KeyFrameRequest) => key_wanted.store(true, Ordering::Relaxed),
                    Ok(_) => return Ending::Failed("the receiver spoke out of turn".into()),
                    Err(error) => {
                        return match error.kind() {
                            std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted => Ending::ByReceiver,
                            _ => Ending::Failed(error.to_string()),
                        };
                    }
                }
            }
        };
        let ending = tokio::select! {
            ending = write => ending,
            ending = read => ending,
        };
        // Whatever the capture threads still queue goes nowhere now.
        video_rx.close();
        other_rx.close();
        ending
    });
    Ok(Session {
        receiver: welcome.name,
        outbox,
        done,
    })
}
