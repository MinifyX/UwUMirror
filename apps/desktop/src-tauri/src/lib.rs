//! UwUMirror's shell: starts the AirPlay receiver and the Android side, hands
//! their streams to the page (see `hub.rs`), and offers the page its commands.

mod hub;
mod log;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use hub::{Hub, StreamState};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{Manager, RunEvent, State};
use tauri_plugin_opener::OpenerExt;
use tokio::task::AbortHandle;
use uwumirror_airplay::{Receiver, ReceiverConfig};
use uwumirror_android::{AdbStatus, Android, Device, MirrorOptions, QrPairing};
use uwumirror_core::{decode, EventSink};

/// What the page asks of the AirPlay receiver.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AirplaySettings {
    enabled: bool,
    name: String,
    width: u32,
    height: u32,
    fps: u32,
    audio: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AirplayStatus {
    running: bool,
    port: Option<u16>,
    name: String,
    error: Option<String>,
}

struct AirplayState {
    receiver: Option<Receiver>,
    settings: Option<AirplaySettings>,
    error: Option<String>,
}

struct AppState {
    hub: Arc<Hub>,
    sink: EventSink,
    identity_path: PathBuf,
    airplay: tokio::sync::Mutex<AirplayState>,
    android: Arc<Android>,
    pairing: Mutex<Option<AbortHandle>>,
}

type Result<T> = std::result::Result<T, String>;

fn text(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// The computer's name, as the default for what iPhones show in their list.
#[tauri::command]
fn computer_name() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("scutil")
        .args(["--get", "ComputerName"])
        .output()
    {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !name.is_empty() {
            return name;
        }
    }
    for var in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(name) = std::env::var(var) {
            if !name.trim().is_empty() {
                return name.trim().to_owned();
            }
        }
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    /// libavcodec's major version, when FFmpeg is on the system.
    ffmpeg: Option<u32>,
    scrcpy: &'static str,
}

#[tauri::command]
async fn app_info() -> AppInfo {
    let ffmpeg = tokio::task::spawn_blocking(decode::ffmpeg_version)
        .await
        .ok()
        .flatten();
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        ffmpeg,
        scrcpy: uwumirror_android::scrcpy::SERVER_VERSION,
    }
}

#[tauri::command]
async fn ffmpeg_recheck() -> Option<u32> {
    tokio::task::spawn_blocking(decode::reload_ffmpeg)
        .await
        .ok()
        .flatten()
}

fn airplay_status(state: &AirplayState) -> AirplayStatus {
    AirplayStatus {
        running: state.receiver.is_some(),
        port: state.receiver.as_ref().map(Receiver::port),
        name: state
            .settings
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default(),
        error: state.error.clone(),
    }
}

/// Starts, restarts or stops the receiver to match `settings`.
#[tauri::command]
async fn airplay_apply(
    state: State<'_, AppState>,
    settings: AirplaySettings,
) -> Result<AirplayStatus> {
    let mut airplay = state.airplay.lock().await;
    if airplay.settings.as_ref() == Some(&settings)
        && (airplay.receiver.is_some() || !settings.enabled)
    {
        return Ok(airplay_status(&airplay));
    }
    // Only sound changed: no need to take the receiver off the network.
    if let (Some(receiver), Some(old)) = (&airplay.receiver, &airplay.settings) {
        if settings.enabled
            && (AirplaySettings {
                audio: settings.audio,
                ..old.clone()
            }) == settings
        {
            receiver.set_audio(settings.audio);
            airplay.settings = Some(settings);
            return Ok(airplay_status(&airplay));
        }
    }
    if let Some(old) = airplay.receiver.take() {
        // Saying goodbye on mDNS waits a moment; not on the async runtime.
        let _ = tokio::task::spawn_blocking(move || drop(old)).await;
    }
    airplay.error = None;
    if settings.enabled {
        let name = settings.name.trim();
        let config = ReceiverConfig {
            name: if name.is_empty() {
                "UwUMirror".into()
            } else {
                name.chars().take(60).collect()
            },
            width: settings.width.clamp(640, 3840),
            height: settings.height.clamp(360, 2160),
            fps: settings.fps.clamp(15, 60),
            audio: settings.audio,
            identity_path: state.identity_path.clone(),
        };
        match Receiver::start(config, state.sink.clone()).await {
            Ok(receiver) => airplay.receiver = Some(receiver),
            Err(error) => {
                tracing::warn!(%error, "AirPlay receiver");
                airplay.error = Some(error.to_string());
            }
        }
    }
    airplay.settings = Some(settings);
    Ok(airplay_status(&airplay))
}

#[tauri::command]
fn streams(state: State<'_, AppState>) -> Vec<StreamState> {
    state.hub.streams()
}

#[tauri::command]
fn subscribe_video(state: State<'_, AppState>, channel: Channel<InvokeResponseBody>) {
    state.hub.subscribe(channel);
}

/// Ends a stream at whichever source it came from.
async fn stop_stream(state: &AppState, id: u64) -> bool {
    if let Some(receiver) = &state.airplay.lock().await.receiver {
        if receiver.end_stream(id) {
            return true;
        }
    }
    state.android.stop(id)
}

#[tauri::command]
async fn stream_stop(state: State<'_, AppState>, id: u64) -> Result<bool> {
    Ok(stop_stream(&state, id).await)
}

#[tauri::command]
async fn android_status(state: State<'_, AppState>) -> Result<AdbStatus> {
    Ok(state.android.status().await)
}

#[tauri::command]
async fn android_devices(state: State<'_, AppState>) -> Result<Vec<Device>> {
    state.android.devices().await.map_err(text)
}

#[tauri::command]
fn android_qr() -> QrPairing {
    uwumirror_android::pairing::new_qr_pairing()
}

/// Waits (up to two minutes) for the phone that scans `qr`, and pairs.
#[tauri::command]
async fn android_pair_qr(state: State<'_, AppState>, qr: QrPairing) -> Result<()> {
    let android = state.android.clone();
    let task =
        tokio::spawn(async move { android.pair_with_qr(&qr, Duration::from_secs(120)).await });
    if let Some(old) = state.pairing.lock().replace(task.abort_handle()) {
        old.abort();
    }
    match task.await {
        Ok(result) => result.map_err(text),
        Err(_) => Err("cancelled".into()),
    }
}

#[tauri::command]
fn android_pair_cancel(state: State<'_, AppState>) {
    if let Some(task) = state.pairing.lock().take() {
        task.abort();
    }
}

#[tauri::command]
async fn android_pair_code(
    state: State<'_, AppState>,
    address: String,
    code: String,
) -> Result<()> {
    state
        .android
        .pair_with_code(address.trim(), code.trim())
        .await
        .map_err(text)
}

#[tauri::command]
async fn android_connect(state: State<'_, AppState>, address: String) -> Result<()> {
    state.android.connect(address.trim()).await.map_err(text)
}

#[tauri::command]
async fn android_disconnect(state: State<'_, AppState>, serial: String) -> Result<()> {
    state.android.disconnect(&serial).await.map_err(text)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AndroidMirrorOptions {
    max_size: u32,
    bit_rate: u32,
    max_fps: u32,
    audio: bool,
}

#[tauri::command]
async fn android_mirror(
    state: State<'_, AppState>,
    serial: String,
    options: AndroidMirrorOptions,
) -> Result<u64> {
    let options = MirrorOptions {
        max_size: options.max_size.min(4096),
        bit_rate: options.bit_rate.clamp(1_000_000, 50_000_000),
        max_fps: options.max_fps.clamp(15, 120),
        audio: options.audio,
    };
    state.android.mirror(&serial, &options).await.map_err(text)
}

#[tauri::command]
async fn android_download_adb(state: State<'_, AppState>) -> Result<String> {
    let path = state.android.download_adb().await.map_err(text)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn android_choose_adb(state: State<'_, AppState>, path: Option<String>) {
    state
        .android
        .choose_adb(path.filter(|p| !p.trim().is_empty()).map(PathBuf::from));
}

/// Opens a web page in the browser: only https, only what the page links to.
#[tauri::command]
fn open_link(app: tauri::AppHandle, url: String) -> Result<()> {
    if !url.starts_with("https://") {
        return Err("only https links".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(text)
}

/// The detailed log on or off (Settings → General).
#[tauri::command]
fn log_detail(on: bool) {
    log::set_detailed(on);
}

/// Shows the log folder in the file manager, for attaching the log to an issue.
#[tauri::command]
fn open_log_folder(app: tauri::AppHandle) -> Result<()> {
    let dir = app.path().app_log_dir().map_err(text)?;
    std::fs::create_dir_all(&dir).map_err(text)?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(text)
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            log::init(&app.path().app_log_dir()?);
            let data = app.path().app_data_dir()?;
            let resources = app.path().resource_dir()?;
            let server = resources.join("scrcpy-server");
            // Windows loads a DLL's neighbours only for a plain path, not a
            // `\\?\` one.
            let resources = resources
                .to_str()
                .and_then(|path| path.strip_prefix(r"\\?\"))
                .map(PathBuf::from)
                .unwrap_or(resources);
            decode::set_bundled_ffmpeg(resources.join("ffmpeg"));
            let hub = Hub::new(app.handle().clone());
            let sink: EventSink = {
                let hub = hub.clone();
                Arc::new(move |event| hub.handle(event))
            };
            let android = Arc::new(Android::new(data.join("android"), server, sink.clone()));
            hub.on_end({
                let android = android.clone();
                move |id| android.ended(id)
            });
            hub.on_replace({
                let app = app.handle().clone();
                move |id| {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        stop_stream(&app.state::<AppState>(), id).await;
                    });
                }
            });
            app.manage(AppState {
                hub,
                sink,
                identity_path: data.join("airplay-identity"),
                airplay: tokio::sync::Mutex::new(AirplayState {
                    receiver: None,
                    settings: None,
                    error: None,
                }),
                android,
                pairing: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            computer_name,
            app_info,
            ffmpeg_recheck,
            airplay_apply,
            streams,
            subscribe_video,
            stream_stop,
            android_status,
            android_devices,
            android_qr,
            android_pair_qr,
            android_pair_cancel,
            android_pair_code,
            android_connect,
            android_disconnect,
            android_mirror,
            android_download_adb,
            android_choose_adb,
            open_link,
            log_detail,
            open_log_folder,
        ])
        .build(tauri::generate_context!())
        .expect("UwUMirror failed to start");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let state = app.state::<AppState>();
            state.android.stop_all();
            // Take the receiver off the network, so iPhones stop offering it.
            let receiver = tauri::async_runtime::block_on(state.airplay.lock())
                .receiver
                .take();
            drop(receiver);
        }
    });
}
