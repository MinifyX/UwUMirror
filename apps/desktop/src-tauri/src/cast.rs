//! UwUCast in the shell: receiving from other computers (everywhere), and
//! sending this screen to one of them (Windows).
//!
//! Receiving works like AirPlay: the page hands over the setting and the
//! name, and the receiver follows. Sending has one state at a time — idle,
//! connecting, sending — which the page reads once and then follows through
//! `cast-send` events; the event after an end says how it ended, for a note.

use std::net::SocketAddr;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uwumirror_cast::{Browser, CastReceiver, Ending, Found, ReceiverConfig};

use crate::{text, AppState, Result};

/// What the page asks of the receiver.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CastSettings {
    enabled: bool,
    name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CastStatus {
    running: bool,
    port: Option<u16>,
    error: Option<String>,
}

#[derive(Default)]
struct Receiving {
    receiver: Option<CastReceiver>,
    settings: Option<CastSettings>,
    error: Option<String>,
}

/// How sending ended, told once with the state that follows it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "how", rename_all = "camelCase")]
pub enum SendEnd {
    Stopped,
    /// Ended over there: the stop button, another device, the app quit.
    ByReceiver,
    Failed {
        error: String,
    },
}

impl From<Ending> for SendEnd {
    fn from(ending: Ending) -> Self {
        match ending {
            Ending::Stopped => SendEnd::Stopped,
            Ending::ByReceiver => SendEnd::ByReceiver,
            Ending::Failed(error) => SendEnd::Failed { error },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendStatus {
    /// "idle", "connecting" or "sending".
    state: &'static str,
    /// The receiver's id and name, while connecting or sending.
    id: Option<String>,
    receiver: Option<String>,
    width: u32,
    height: u32,
    fps: u32,
    /// The encoder's name, and whether it is the graphics card's.
    encoder: Option<String>,
    hardware: bool,
    audio: bool,
    ended: Option<SendEnd>,
}

impl SendStatus {
    fn idle(ended: Option<SendEnd>) -> Self {
        Self {
            state: "idle",
            ended,
            ..Self::default()
        }
    }
}

struct Sending {
    status: SendStatus,
    /// Counts the starts, so an old broadcast's end can't reset a newer one.
    generation: u64,
    #[cfg(windows)]
    broadcast: Option<uwumirror_cast::screen::Broadcast>,
    /// The last generation that ended — possibly before its start returned.
    #[cfg(windows)]
    finished: u64,
}

pub struct Cast {
    receiving: tokio::sync::Mutex<Receiving>,
    browser: Mutex<Option<Browser>>,
    sending: Mutex<Sending>,
}

impl Cast {
    pub fn new() -> Self {
        Self {
            receiving: tokio::sync::Mutex::default(),
            browser: Mutex::new(None),
            sending: Mutex::new(Sending {
                status: SendStatus::idle(None),
                generation: 0,
                #[cfg(windows)]
                broadcast: None,
                #[cfg(windows)]
                finished: 0,
            }),
        }
    }

    /// Ends one of the received streams, if it is one.
    pub async fn end_stream(&self, id: u64) -> bool {
        self.receiving
            .lock()
            .await
            .receiver
            .as_ref()
            .is_some_and(|receiver| receiver.end_stream(id))
    }

    /// When the app quits: off the network, and stop sending.
    pub fn shut_down(&self) {
        let receiver = tauri::async_runtime::block_on(self.receiving.lock())
            .receiver
            .take();
        drop(receiver);
        *self.browser.lock() = None;
        #[cfg(windows)]
        drop(self.sending.lock().broadcast.take());
    }
}

fn status(receiving: &Receiving) -> CastStatus {
    CastStatus {
        running: receiving.receiver.is_some(),
        port: receiving.receiver.as_ref().map(CastReceiver::port),
        error: receiving.error.clone(),
    }
}

/// Starts, restarts or stops the receiver to match `settings`.
#[tauri::command]
pub async fn cast_apply(state: State<'_, AppState>, settings: CastSettings) -> Result<CastStatus> {
    let mut receiving = state.cast.receiving.lock().await;
    if receiving.settings.as_ref() == Some(&settings)
        && (receiving.receiver.is_some() || !settings.enabled)
    {
        return Ok(status(&receiving));
    }
    if let Some(old) = receiving.receiver.take() {
        // Saying goodbye on mDNS waits a moment; not on the async runtime.
        let _ = tokio::task::spawn_blocking(move || drop(old)).await;
    }
    receiving.error = None;
    if settings.enabled {
        let name = settings.name.trim();
        let config = ReceiverConfig {
            name: if name.is_empty() {
                "UwUMirror".into()
            } else {
                name.chars().take(60).collect()
            },
            audio: true,
            announce: true,
        };
        match CastReceiver::start(config, state.sink.clone()).await {
            Ok(receiver) => receiving.receiver = Some(receiver),
            Err(error) => {
                tracing::warn!(%error, "UwUCast receiver");
                receiving.error = Some(error.to_string());
            }
        }
    }
    receiving.settings = Some(settings);
    Ok(status(&receiving))
}

/// The receivers on the network, this computer's own left out. The first
/// call starts looking; they show up over the next seconds.
#[tauri::command]
pub fn cast_receivers(state: State<'_, AppState>) -> Result<Vec<Found>> {
    let mut browser = state.cast.browser.lock();
    if browser.is_none() {
        *browser = Some(Browser::start().map_err(text)?);
    }
    Ok(browser.as_ref().map(Browser::receivers).unwrap_or_default())
}

#[tauri::command]
pub fn cast_send_status(state: State<'_, AppState>) -> SendStatus {
    state.cast.sending.lock().status.clone()
}

fn publish(app: &AppHandle, status: SendStatus) {
    if let Err(error) = app.emit("cast-send", &status) {
        tracing::warn!(%error, "sending state to the page");
    }
}

/// Sends this screen to the receiver with `id`, as `name`.
#[tauri::command]
pub async fn cast_send(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<SendStatus> {
    let found = state
        .cast
        .browser
        .lock()
        .as_ref()
        .and_then(|browser| browser.get(&id))
        .ok_or("this receiver isn't on the network any more")?;
    if !found.compatible {
        return Err(format!(
            "{} runs another UwUMirror version ({}): update both",
            found.name, found.version
        ));
    }
    start(app, &state, found.id, found.name, found.address, name).await
}

#[cfg(windows)]
async fn start(
    app: AppHandle,
    state: &AppState,
    id: String,
    receiver: String,
    address: SocketAddr,
    name: String,
) -> Result<SendStatus> {
    use tauri::Manager;
    use uwumirror_cast::screen::{self, SendOptions};

    let generation = {
        let mut sending = state.cast.sending.lock();
        // One at a time: whatever ran stops.
        drop(sending.broadcast.take());
        sending.generation += 1;
        sending.status = SendStatus {
            state: "connecting",
            id: Some(id.clone()),
            receiver: Some(receiver.clone()),
            ..SendStatus::default()
        };
        publish(&app, sending.status.clone());
        sending.generation
    };
    let options = SendOptions {
        name: name.trim().chars().take(60).collect(),
        ..SendOptions::default()
    };
    let on_end = {
        let app = app.clone();
        move |ending: Ending| {
            let state = app.state::<AppState>();
            let mut sending = state.cast.sending.lock();
            if sending.generation != generation {
                return;
            }
            sending.finished = generation;
            sending.broadcast = None;
            sending.status = SendStatus::idle(Some(ending.into()));
            publish(&app, sending.status.clone());
        }
    };
    let result = screen::start(address, options, on_end).await;
    let mut sending = state.cast.sending.lock();
    if sending.generation != generation || sending.finished == generation {
        // Stopped, or another start, while connecting; or already over.
        return Ok(sending.status.clone());
    }
    match result {
        Ok(broadcast) => {
            let info = &broadcast.info;
            sending.status = SendStatus {
                state: "sending",
                id: Some(id),
                receiver: Some(info.receiver.clone()),
                width: info.width,
                height: info.height,
                fps: info.fps,
                encoder: Some(info.encoder.clone()),
                hardware: info.hardware,
                audio: info.audio,
                ended: None,
            };
            sending.broadcast = Some(broadcast);
            publish(&app, sending.status.clone());
            Ok(sending.status.clone())
        }
        Err(error) => {
            tracing::warn!(%error, "sending the screen");
            sending.status = SendStatus::idle(None);
            publish(&app, sending.status.clone());
            Err(error.to_string())
        }
    }
}

#[cfg(not(windows))]
async fn start(
    _app: AppHandle,
    _state: &AppState,
    _id: String,
    _receiver: String,
    _address: SocketAddr,
    _name: String,
) -> Result<SendStatus> {
    Err("sending the screen works on Windows only".into())
}

/// Stops sending. The `cast-send` event with the end follows.
#[tauri::command]
pub fn cast_send_stop(app: AppHandle, state: State<'_, AppState>) {
    let mut sending = state.cast.sending.lock();
    #[cfg(windows)]
    if let Some(broadcast) = &sending.broadcast {
        // Its end comes through `on_end`, once the receiver has the goodbye.
        broadcast.stop();
        return;
    }
    // Nothing running (or still connecting): back to idle at once.
    sending.generation += 1;
    sending.status = SendStatus::idle(Some(SendEnd::Stopped));
    publish(&app, sending.status.clone());
}
