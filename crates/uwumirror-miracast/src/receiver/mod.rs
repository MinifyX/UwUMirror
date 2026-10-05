//! Windows' Miracast receiver, borrowed.
//!
//! One thread owns it all — the `MiracastReceiver`, its session, the
//! connection and its player — inside the multithreaded COM apartment, where
//! these objects want to be called. The receiver's events arrive on Windows'
//! own worker threads; they only pass what they got on to the owner thread,
//! so nothing is set up or torn down twice at the same time, and no player is
//! ever closed from inside its own event.
//!
//! **One connection at a time.** The session allows a takeover: a second phone
//! that connects replaces the first, as on an Apple TV, and as UwUMirror does
//! between sources.
//!
//! **The name** senders list is Windows' own receiver name, the computer's
//! name. The receiver could be given another one
//! (`DisconnectAllAndApplySettings`), but that setting belongs to Windows'
//! receiver as a whole — the one behind "Projecting to this PC" and the
//! Wireless Display app, too — and would outlive UwUMirror. So UwUMirror
//! leaves it alone and shows the name it has instead.

mod playback;

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

use parking_lot::Mutex;
use uwumirror_core::{next_stream_id, AudioStatus, EventSink, StreamEvent, StreamInfo, StreamKind};
use windows::core::{IInspectable, IUnknown, Interface, HSTRING};
use windows::ApplicationModel::Core::CoreApplicationView;
use windows::Foundation::{Deferral, TypedEventHandler, Uri};
use windows::Media::Core::MediaSource;
use windows::Media::Miracast::{
    MiracastReceiver, MiracastReceiverConnection, MiracastReceiverConnectionCreatedEventArgs,
    MiracastReceiverDisconnectReason, MiracastReceiverDisconnectedEventArgs,
    MiracastReceiverMediaSourceCreatedEventArgs, MiracastReceiverSession,
};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

pub use playback::PlaybackStats;
use playback::{Counters, Playback};

use crate::status::{self, MiracastState, MiracastStatus};
use crate::{Error, ReceiverConfig, StatusSink};

fn windows_error(error: windows::core::Error) -> Error {
    Error::Windows(error.message())
}

/// Runs `body` on a thread of its own inside the multithreaded apartment.
fn mta_thread<T: Send + 'static>(
    name: &str,
    body: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<JoinHandle<T>> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            // SAFETY: a fresh thread, initialized once and uninitialized once.
            let entered = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
            let result = body();
            if entered {
                unsafe { RoUninitialize() };
            }
            result
        })
}

/// What phones would see, without starting anything: Windows' receiver name
/// and whether Wi-Fi Direct is there. For when the receiver is switched off.
pub fn probe() -> MiracastStatus {
    let probe = mta_thread("miracast-probe", || -> windows::core::Result<_> {
        let receiver = MiracastReceiver::new()?;
        let name = receiver.GetCurrentSettings()?.FriendlyName()?.to_string();
        let wifi = receiver.GetStatus()?.WiFiStatus()?.0;
        Ok((name, wifi))
    });
    match probe.map(|thread| thread.join()) {
        Ok(Ok(Ok((name, wifi)))) => {
            let state = match wifi {
                status::WIFI_NOT_SUPPORTED => MiracastState::NoWifiDirect,
                _ => MiracastState::Off,
            };
            MiracastStatus::new(state, name)
        }
        Ok(Ok(Err(error))) => MiracastStatus {
            error: Some(error.message()),
            ..MiracastStatus::new(MiracastState::Failed, String::new())
        },
        _ => MiracastStatus::new(MiracastState::Failed, String::new()),
    }
}

/// What the receiver's events (and the app) ask of the owner thread.
enum Command {
    Connected {
        connection: MiracastReceiverConnection,
        pin: Option<String>,
    },
    Media {
        connection: MiracastReceiverConnection,
        source: MediaSource,
        deferral: Option<Deferral>,
    },
    Disconnected(Option<MiracastReceiverConnection>),
    /// The player of stream `id` gave up (with why) or ran out.
    PlaybackEnded {
        id: u64,
        reason: Option<String>,
    },
    End(u64),
    Audio(bool),
    StatusChanged,
    Stop,
}

/// What the app may look at from any thread.
struct Shared {
    sink: EventSink,
    on_status: StatusSink,
    audio: AtomicBool,
    status: Mutex<MiracastStatus>,
    /// The stream that runs, or 0.
    current: AtomicU64,
}

impl Shared {
    fn set_status(&self, status: MiracastStatus) {
        {
            let mut current = self.status.lock();
            if *current == status {
                return;
            }
            *current = status.clone();
        }
        tracing::info!(state = ?status.state, name = %status.name, "Miracast receiver");
        (self.on_status)(status);
    }
}

/// The connection that runs.
struct Current {
    id: u64,
    connection: MiracastReceiverConnection,
    playback: Option<Playback>,
    pin: Option<String>,
}

/// Whether two references point at the same object (COM identity).
fn same(a: &MiracastReceiverConnection, b: &MiracastReceiverConnection) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

struct Owner {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
    receiver: MiracastReceiver,
    session: MiracastReceiverSession,
    started: bool,
    /// Why the session couldn't start, while it couldn't.
    failure: Option<(MiracastState, Option<String>)>,
    current: Option<Current>,
}

impl Owner {
    fn new(shared: Arc<Shared>, commands: mpsc::Sender<Command>) -> windows::core::Result<Self> {
        let receiver = MiracastReceiver::new()?;
        receiver.StatusChanged(&TypedEventHandler::<MiracastReceiver, IInspectable>::new({
            let commands = commands.clone();
            move |_, _| {
                let _ = commands.send(Command::StatusChanged);
                Ok(())
            }
        }))?;
        let session = receiver.CreateSession(None::<&CoreApplicationView>)?;
        // A second sender takes over from the first, instead of being turned
        // away; one at a time, as everywhere in UwUMirror.
        session.SetAllowConnectionTakeover(true)?;
        if let Err(error) = session.SetMaxSimultaneousConnections(1) {
            tracing::debug!(%error, "Miracast: one connection at a time");
        }
        session.ConnectionCreated(&TypedEventHandler::<
            MiracastReceiverSession,
            MiracastReceiverConnectionCreatedEventArgs,
        >::new({
            let commands = commands.clone();
            move |_, args| {
                if let Some(args) = args.as_ref() {
                    let pin = args
                        .Pin()
                        .ok()
                        .map(|pin| pin.to_string())
                        .filter(|pin| !pin.is_empty());
                    let _ = commands.send(Command::Connected {
                        connection: args.Connection()?,
                        pin,
                    });
                }
                Ok(())
            }
        }))?;
        session.MediaSourceCreated(&TypedEventHandler::<
            MiracastReceiverSession,
            MiracastReceiverMediaSourceCreatedEventArgs,
        >::new({
            let commands = commands.clone();
            move |_, args| {
                if let Some(args) = args.as_ref() {
                    let _ = commands.send(Command::Media {
                        connection: args.Connection()?,
                        source: args.MediaSource()?,
                        // Windows waits for the player until this completes.
                        deferral: args.GetDeferral().ok(),
                    });
                }
                Ok(())
            }
        }))?;
        session.Disconnected(&TypedEventHandler::<
            MiracastReceiverSession,
            MiracastReceiverDisconnectedEventArgs,
        >::new({
            let commands = commands.clone();
            move |_, args| {
                let connection = args.as_ref().and_then(|args| args.Connection().ok());
                let _ = commands.send(Command::Disconnected(connection));
                Ok(())
            }
        }))?;
        Ok(Self {
            shared,
            commands,
            receiver,
            session,
            started: false,
            failure: None,
            current: None,
        })
    }

    fn start(&mut self) {
        let wifi = self.wifi();
        match self.session.Start() {
            Ok(result) => {
                let status = result.Status().map(|s| s.0).unwrap_or(-1);
                if status == status::START_SUCCESS {
                    self.started = true;
                    self.failure = None;
                } else {
                    let extended = result.ExtendedError().map(|e| e.0).unwrap_or(0);
                    tracing::warn!(status, extended, wifi, "Miracast session didn't start");
                    self.failure = Some(status::start_failure(status, wifi));
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Miracast session");
                self.failure = Some((MiracastState::Failed, Some(error.message())));
            }
        }
        self.refresh();
    }

    fn wifi(&self) -> i32 {
        self.receiver
            .GetStatus()
            .and_then(|s| s.WiFiStatus())
            .map(|s| s.0)
            .unwrap_or(status::WIFI_UNDETERMINED)
    }

    fn refresh(&self) {
        let name = self
            .receiver
            .GetCurrentSettings()
            .and_then(|s| s.FriendlyName())
            .map(|name| name.to_string())
            .unwrap_or_default();
        let (state, error) = match &self.failure {
            Some((state, error)) => (*state, error.clone()),
            None => match self.receiver.GetStatus() {
                Ok(status) => {
                    let listening = status
                        .ListeningStatus()
                        .map(|s| s.0)
                        .unwrap_or(status::NOT_LISTENING);
                    let wifi = status
                        .WiFiStatus()
                        .map(|s| s.0)
                        .unwrap_or(status::WIFI_UNDETERMINED);
                    (status::state_of(listening, wifi), None)
                }
                Err(error) => (MiracastState::Failed, Some(error.message())),
            },
        };
        self.shared.set_status(MiracastStatus {
            state,
            name,
            pin: self.current.as_ref().and_then(|c| c.pin.clone()),
            error,
        });
    }

    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Connected { connection, pin } => self.connected(connection, pin),
            Command::Media {
                connection,
                source,
                deferral,
            } => {
                self.media(&connection, &source);
                if let Some(deferral) = deferral {
                    let _ = deferral.Complete();
                }
            }
            Command::Disconnected(connection) => {
                let ours = match (&self.current, &connection) {
                    (Some(current), Some(connection)) => same(&current.connection, connection),
                    // Without a connection to compare, it can only be ours.
                    (Some(_), None) => true,
                    (None, _) => false,
                };
                if ours {
                    self.finish(None, None);
                }
            }
            Command::PlaybackEnded { id, reason } => {
                if self.current.as_ref().is_some_and(|c| c.id == id) {
                    self.finish(
                        Some(MiracastReceiverDisconnectReason::MediaDecodingError),
                        Some(reason.unwrap_or_else(|| "the picture ended".into())),
                    );
                }
            }
            Command::End(id) => {
                if self.current.as_ref().is_some_and(|c| c.id == id) {
                    self.finish(
                        Some(MiracastReceiverDisconnectReason::DisconnectedByUser),
                        None,
                    );
                }
            }
            Command::Audio(on) => {
                if let Some(playback) = self.current.as_ref().and_then(|c| c.playback.as_ref()) {
                    playback.set_audio(on);
                }
                if let Some(current) = &self.current {
                    (self.shared.sink)(StreamEvent::Audio {
                        id: current.id,
                        status: if on {
                            AudioStatus::Playing
                        } else {
                            AudioStatus::Off
                        },
                    });
                }
            }
            Command::StatusChanged => {
                // Wi-Fi came on, or a policy went away: try again.
                if !self.started && status::wifi_ready(self.wifi()) {
                    self.start();
                } else {
                    self.refresh();
                }
            }
            Command::Stop => return false,
        }
        true
    }

    fn connected(&mut self, connection: MiracastReceiverConnection, pin: Option<String>) {
        if let Some(current) = &self.current {
            if same(&current.connection, &connection) {
                return;
            }
            // A takeover: the newer sender wins.
            self.finish(None, None);
        }
        let transmitter = connection.Transmitter().ok();
        let name = transmitter
            .as_ref()
            .and_then(|t| t.Name().ok())
            .map(|name| name.to_string())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Miracast".into());
        let address = transmitter
            .as_ref()
            .and_then(|t| t.MacAddress().ok())
            .map(|mac| mac.to_string())
            .unwrap_or_default();
        let id = next_stream_id();
        tracing::info!(id, %name, %address, pin = pin.is_some(), "Miracast: a sender connects");
        self.current = Some(Current {
            id,
            connection,
            playback: None,
            pin,
        });
        self.shared.current.store(id, Ordering::Relaxed);
        (self.shared.sink)(StreamEvent::Started(StreamInfo {
            id,
            kind: StreamKind::Miracast,
            name,
            model: None,
            address,
        }));
        self.refresh();
    }

    fn media(&mut self, connection: &MiracastReceiverConnection, source: &MediaSource) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        if !same(&current.connection, connection) {
            return;
        }
        let id = current.id;
        let audio = self.shared.audio.load(Ordering::Relaxed);
        let on_end = {
            let commands = self.commands.clone();
            Arc::new(move |reason| {
                let _ = commands.send(Command::PlaybackEnded { id, reason });
            })
        };
        // A new source for the same connection replaces the old player.
        current.playback = None;
        match Playback::start(source, id, self.shared.sink.clone(), audio, false, on_end) {
            Ok(playback) => {
                current.playback = Some(playback);
                current.pin = None;
                (self.shared.sink)(StreamEvent::Audio {
                    id,
                    status: if audio {
                        AudioStatus::Playing
                    } else {
                        AudioStatus::Off
                    },
                });
                self.refresh();
            }
            Err(error) => {
                tracing::warn!(%error, "Miracast: the player didn't start");
                self.finish(
                    Some(MiracastReceiverDisconnectReason::FailedToStartStreaming),
                    Some(error.message()),
                );
            }
        }
    }

    /// Ends the connection that runs: hangs up when `hang_up` says why, and
    /// tells the app.
    fn finish(
        &mut self,
        hang_up: Option<MiracastReceiverDisconnectReason>,
        reason: Option<String>,
    ) {
        let Some(current) = self.current.take() else {
            return;
        };
        self.shared.current.store(0, Ordering::Relaxed);
        drop(current.playback);
        if let Some(why) = hang_up {
            if let Err(error) = current.connection.Disconnect(why) {
                tracing::debug!(%error, "Miracast: hanging up");
            }
        }
        tracing::info!(id = current.id, ?reason, "Miracast: the sender is gone");
        (self.shared.sink)(StreamEvent::Ended {
            id: current.id,
            reason,
        });
        self.refresh();
    }

    fn close(mut self) {
        self.finish(Some(MiracastReceiverDisconnectReason::Finished), None);
        if let Err(error) = self.session.Close() {
            tracing::debug!(%error, "Miracast: closing the session");
        }
    }
}

/// Windows' Miracast receiver, listening for as long as this lives.
pub struct Receiver {
    shared: Arc<Shared>,
    commands: mpsc::Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Receiver {
    /// Starts listening. Blocks while Windows starts its receiver, a moment.
    ///
    /// A receiver that can't listen right now (Wi-Fi off, no Wi-Fi Direct, a
    /// policy) still starts: its status says why, and it starts listening
    /// when Windows reports that things changed.
    pub fn start(
        config: ReceiverConfig,
        sink: EventSink,
        on_status: StatusSink,
    ) -> Result<Self, Error> {
        let shared = Arc::new(Shared {
            sink,
            on_status,
            audio: AtomicBool::new(config.audio),
            status: Mutex::new(MiracastStatus::new(MiracastState::Starting, String::new())),
            current: AtomicU64::new(0),
        });
        let (commands, inbox) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let thread = mta_thread("miracast", {
            let shared = shared.clone();
            let commands = commands.clone();
            move || {
                let mut owner = match Owner::new(shared, commands) {
                    Ok(owner) => owner,
                    Err(error) => {
                        let _ = ready.send(Err(windows_error(error)));
                        return;
                    }
                };
                owner.start();
                let _ = ready.send(Ok(()));
                while let Ok(command) = inbox.recv() {
                    if !owner.handle(command) {
                        break;
                    }
                }
                owner.close();
            }
        })
        .map_err(|error| Error::Windows(error.to_string()))?;
        match started.recv() {
            Ok(Ok(())) => Ok(Self {
                shared,
                commands,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error::Windows("the receiver's thread ended".into()))
            }
        }
    }

    pub fn status(&self) -> MiracastStatus {
        self.shared.status.lock().clone()
    }

    /// Hangs up on the sender of stream `id`, if it is ours.
    pub fn end_stream(&self, id: u64) -> bool {
        if id == 0 || self.shared.current.load(Ordering::Relaxed) != id {
            return false;
        }
        self.commands.send(Command::End(id)).is_ok()
    }

    pub fn set_audio(&self, on: bool) {
        if self.shared.audio.swap(on, Ordering::Relaxed) != on {
            let _ = self.commands.send(Command::Audio(on));
        }
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A video file played as if a Miracast sender showed it: the same player,
/// the same frame server, the same frames. For trying the picture's way into
/// the page without a phone (and for measuring it).
pub struct FilePlayback {
    id: u64,
    counters: Arc<Counters>,
    commands: mpsc::Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl FilePlayback {
    /// Starts playing `path` (an absolute path) as stream of kind Miracast,
    /// over and over if `looping`.
    pub fn start(path: &Path, sink: EventSink, audio: bool, looping: bool) -> Result<Self, Error> {
        let uri = format!("file:///{}", path.to_string_lossy().replace('\\', "/"));
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Video".into());
        let id = next_stream_id();
        let (commands, inbox) = mpsc::channel();
        let (ready, started) = mpsc::channel();
        let thread = mta_thread("miracast-file", {
            let commands = commands.clone();
            move || {
                let on_end = Arc::new(move |reason| {
                    let _ = commands.send(Command::PlaybackEnded { id, reason });
                });
                let playback = Uri::CreateUri(&HSTRING::from(uri))
                    .and_then(|uri| MediaSource::CreateFromUri(&uri))
                    .and_then(|source| {
                        sink(StreamEvent::Started(StreamInfo {
                            id,
                            kind: StreamKind::Miracast,
                            name,
                            model: None,
                            address: "file".into(),
                        }));
                        Playback::start(&source, id, sink.clone(), audio, looping, on_end)
                    });
                let playback = match playback {
                    Ok(playback) => playback,
                    Err(error) => {
                        let _ = ready.send(Err(windows_error(error)));
                        return;
                    }
                };
                let _ = ready.send(Ok(playback.counters()));
                sink(StreamEvent::Audio {
                    id,
                    status: if audio {
                        AudioStatus::Playing
                    } else {
                        AudioStatus::Off
                    },
                });
                let reason = loop {
                    match inbox.recv() {
                        Ok(Command::PlaybackEnded { reason, .. }) => break reason,
                        Ok(Command::End(_) | Command::Stop) | Err(_) => break None,
                        Ok(_) => {}
                    }
                };
                drop(playback);
                sink(StreamEvent::Ended { id, reason });
            }
        })
        .map_err(|error| Error::Windows(error.to_string()))?;
        match started.recv() {
            Ok(Ok(counters)) => Ok(Self {
                id,
                counters,
                commands,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err(Error::Windows("the player's thread ended".into()))
            }
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn stats(&self) -> PlaybackStats {
        self.counters.snapshot()
    }

    pub fn end_stream(&self, id: u64) -> bool {
        id == self.id && self.commands.send(Command::End(id)).is_ok()
    }
}

impl Drop for FilePlayback {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
