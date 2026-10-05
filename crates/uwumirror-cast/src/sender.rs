//! The sending side's network half, the same on every system: connect, say
//! hello, then hand messages from the capture threads to the connection.
//!
//! The capture threads never wait for the network. What they produce goes
//! into a short queue; when the queue is full — the network is slower than
//! the picture — frames are dropped instead of piling up behind, and the
//! encoder is asked for a key frame, because the frames after a dropped one
//! can't be decoded until the next key frame. Live beats complete, as with
//! the receiver's sound.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::protocol::{Hello, Message, Welcome, WelcomeStatus, VERSION};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// About a third of a second of picture and sound.
const QUEUE: usize = 32;

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
    tx: mpsc::Sender<Message>,
    /// The receiver asked for a key frame, or a frame was dropped.
    key_wanted: Arc<AtomicBool>,
    /// A frame was dropped: nothing but a key frame is worth sending now.
    broken: Arc<AtomicBool>,
}

impl Outbox {
    /// Queues one access unit. False once the connection is gone.
    pub fn video(&self, key: bool, pts_us: u64, data: Vec<u8>) -> bool {
        if !key && self.broken.load(Ordering::Relaxed) {
            return !self.tx.is_closed();
        }
        match self.tx.try_send(Message::Video { key, pts_us, data }) {
            Ok(()) => {
                if key {
                    self.broken.store(false, Ordering::Relaxed);
                }
                true
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
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
            self.tx.try_send(Message::Audio { pts_us, samples }),
            Err(mpsc::error::TrySendError::Closed(_))
        )
    }

    /// Queues the end, if there is room. True once it is queued, or there is
    /// no connection left to end.
    pub fn try_end(&self) -> bool {
        !matches!(
            self.tx.try_send(Message::End),
            Err(mpsc::error::TrySendError::Full(_))
        )
    }

    /// Queues something that must not be lost, waiting for room.
    pub async fn send(&self, message: Message) -> bool {
        self.tx.send(message).await.is_ok()
    }

    /// Whether a key frame should come next; asking resets it.
    pub fn take_key_request(&self) -> bool {
        self.key_wanted.swap(false, Ordering::Relaxed)
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
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
    let mut socket = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .map_err(|_| SendError::NoAnswer)?
        .map_err(SendError::Connect)?;
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
    let (tx, mut rx) = mpsc::channel::<Message>(QUEUE);
    let outbox = Outbox {
        tx,
        key_wanted: Arc::new(AtomicBool::new(false)),
        broken: Arc::new(AtomicBool::new(false)),
    };
    let key_wanted = outbox.key_wanted.clone();
    let done = tokio::spawn(async move {
        let write = async {
            while let Some(message) = rx.recv().await {
                if let Err(error) = message.write(&mut writer).await {
                    return Ending::Failed(error.to_string());
                }
                if message == Message::End {
                    let _ = writer.shutdown().await;
                    return Ending::Stopped;
                }
            }
            // Every outbox is gone: stopped without a word.
            Ending::Stopped
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
        rx.close();
        ending
    });
    Ok(Session {
        receiver: welcome.name,
        outbox,
        done,
    })
}
