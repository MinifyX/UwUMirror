//! Sending this computer's screen (Windows only).
//!
//! [`Broadcast::start`] sets up everything that can fail on this computer
//! first — the screen recording, the encoder — then connects. Two threads do
//! the work: one records, converts and encodes the picture at a steady rate,
//! one records the sound; both put what they make into the session's
//! [`Outbox`](crate::sender::Outbox). A task waits for the end: stopped here,
//! closed by the receiver, or failed.

mod capture;
mod convert;
mod encode;
mod sound;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use tokio::sync::oneshot;
use windows::Win32::Media::MediaFoundation::{
    MFShutdown, MFStartup, MFSTARTUP_NOSOCKET, MF_VERSION,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::protocol::{Hello, Message, VERSION};
use crate::sender::{self, Ending, Outbox, SendError};

pub use convert::fit;

/// What to send, and how.
#[derive(Debug, Clone)]
pub struct SendOptions {
    /// This computer's name, as the receiver shows it.
    pub name: String,
    pub audio: bool,
    /// The largest picture sent; the screen is scaled down into it.
    pub max_width: u32,
    pub max_height: u32,
    /// Frames a second with the graphics card's encoder; Windows' own, on
    /// the processor, does at most [`SOFTWARE_FPS`].
    pub fps: u32,
    pub bit_rate: u32,
    /// Try the graphics card's encoder first. Off, Windows' own is used.
    pub hardware: bool,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            audio: true,
            max_width: 1920,
            max_height: 1080,
            fps: 60,
            bit_rate: 10_000_000,
            hardware: true,
        }
    }
}

pub const SOFTWARE_FPS: u32 = 30;

/// What runs while sending; facts for the interface and the log.
#[derive(Debug, Clone)]
pub struct Sending {
    pub receiver: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub encoder: String,
    pub hardware: bool,
    pub audio: bool,
}

/// A running broadcast. Dropping it stops it, as [`Broadcast::stop`] does.
pub struct Broadcast {
    stop: Arc<AtomicBool>,
    pub info: Sending,
}

impl Broadcast {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Broadcast {
    fn drop(&mut self) {
        self.stop();
    }
}

/// "Windows 11" or "Windows 10", by build number.
pub fn system_name() -> String {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buffer = [0u16; 32];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let read = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"),
            w!("CurrentBuildNumber"),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let build: u32 = if read.is_ok() {
        String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)])
            .trim()
            .parse()
            .unwrap_or(0)
    } else {
        0
    };
    if build >= 22_000 {
        "Windows 11".into()
    } else {
        "Windows 10".into()
    }
}

/// The encoded size for a screen: fitted into the largest allowed, even.
pub fn encoded_size(screen: (u32, u32), max: (u32, u32)) -> (u32, u32) {
    // Never larger than the screen itself.
    let max = (max.0.min(screen.0), max.1.min(screen.1));
    let rect = fit(screen, max);
    (
        ((rect.right - rect.left) as u32).max(2),
        ((rect.bottom - rect.top) as u32).max(2),
    )
}

/// Ready to encode: what the picture thread reports before it waits for the
/// connection.
struct Ready {
    width: u32,
    height: u32,
    fps: u32,
    encoder: String,
    hardware: bool,
}

/// Starts sending to the receiver at `address`. `on_end` is called once when
/// it is over, however that came.
pub async fn start(
    address: SocketAddr,
    options: SendOptions,
    on_end: impl FnOnce(Ending) + Send + 'static,
) -> Result<Broadcast, SendError> {
    let stop = Arc::new(AtomicBool::new(false));
    let clock = Instant::now();
    let (ready_tx, ready_rx) = oneshot::channel::<Result<Ready, String>>();
    let (outbox_tx, outbox_rx) = mpsc::channel::<Outbox>();
    let (ended_tx, ended_rx) = oneshot::channel::<Option<String>>();
    let video = {
        let stop = stop.clone();
        let options = options.clone();
        std::thread::Builder::new()
            .name("uwumirror-send".into())
            .spawn(move || {
                let result = picture(&options, &stop, clock, ready_tx, outbox_rx);
                let _ = ended_tx.send(result.err());
            })
            .map_err(|error| SendError::Capture(error.to_string()))?
    };
    let ready = match ready_rx.await {
        Ok(Ok(ready)) => ready,
        Ok(Err(error)) => return Err(SendError::Capture(error)),
        Err(_) => return Err(SendError::Capture("the picture thread ended".into())),
    };

    let hello = Hello {
        version: VERSION,
        name: options.name.clone(),
        model: system_name(),
        audio: options.audio,
    };
    let session = match sender::connect(address, &hello).await {
        Ok(session) => session,
        Err(error) => {
            stop.store(true, Ordering::Relaxed);
            drop(outbox_tx);
            return Err(error);
        }
    };
    tracing::info!(%address, receiver = %session.receiver, "sending the screen");
    session
        .outbox
        .send(Message::VideoSize {
            width: ready.width,
            height: ready.height,
        })
        .await;
    let _ = outbox_tx.send(session.outbox.clone());

    // Sound that won't record leaves the picture going.
    let loopback = if options.audio {
        match sound::Loopback::start(session.outbox.clone(), clock) {
            Ok(loopback) => Some(loopback),
            Err(error) => {
                tracing::warn!(%error, "no sound to send");
                None
            }
        }
    } else {
        None
    };

    let info = Sending {
        receiver: session.receiver.clone(),
        width: ready.width,
        height: ready.height,
        fps: ready.fps,
        encoder: ready.encoder,
        hardware: ready.hardware,
        audio: loopback.is_some(),
    };
    let watch_stop = stop.clone();
    let mut done = session.done;
    drop(session.outbox);
    tokio::spawn(async move {
        let ending = tokio::select! {
            ending = &mut done => ending.unwrap_or(Ending::Failed("the connection's task ended".into())),
            failed = ended_rx => match failed {
                // The picture thread ended on its own: it failed, or it was
                // stopped and its end message is on the way.
                Ok(Some(error)) => Ending::Failed(error),
                _ => match tokio::time::timeout(Duration::from_secs(2), &mut done).await {
                    Ok(Ok(ending)) => ending,
                    _ => Ending::Stopped,
                },
            },
        };
        watch_stop.store(true, Ordering::Relaxed);
        done.abort();
        drop(loopback);
        let _ = tokio::task::spawn_blocking(move || video.join()).await;
        tracing::info!(?ending, "sending ended");
        on_end(ending);
    });
    Ok(Broadcast { stop, info })
}

/// The picture thread: sets up, says it's ready, waits for the connection,
/// then records and encodes until stopped or disconnected.
fn picture(
    options: &SendOptions,
    stop: &AtomicBool,
    clock: Instant,
    ready: oneshot::Sender<Result<Ready, String>>,
    outbox: mpsc::Receiver<Outbox>,
) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) {
            let _ = ready.send(Err(error.to_string()));
            return Ok(());
        }
    }
    let result = run(options, stop, clock, ready, outbox);
    unsafe {
        let _ = MFShutdown();
        CoUninitialize();
    }
    result
}

fn run(
    options: &SendOptions,
    stop: &AtomicBool,
    clock: Instant,
    ready: oneshot::Sender<Result<Ready, String>>,
    outbox: mpsc::Receiver<Outbox>,
) -> Result<(), String> {
    let setup = || -> windows::core::Result<_> {
        let gpu = capture::Gpu::new()?;
        let mut screen = capture::Capture::primary(&gpu)?;
        let (width, height) = encoded_size(screen.size(), (options.max_width, options.max_height));
        let encoder = encode::Encoder::open(
            &gpu,
            encode::Settings {
                width,
                height,
                fps: options.fps.clamp(10, 60),
                bit_rate: options.bit_rate,
            },
            SOFTWARE_FPS,
            options.hardware,
        )?;
        let fps = encoder.settings().fps;
        let mut converter = convert::Converter::new(&gpu, width, height, fps, encoder.hardware)?;
        // The first frame, if it's there already: no black start.
        screen.update(&gpu)?;
        converter.set_source(&gpu, &screen.texture)?;
        Ok((gpu, screen, encoder, converter))
    };
    let (gpu, mut screen, mut encoder, mut converter) = match setup() {
        Ok(parts) => parts,
        Err(error) => {
            let _ = ready.send(Err(error.message()));
            return Ok(());
        }
    };
    let settings = encoder.settings();
    let _ = ready.send(Ok(Ready {
        width: settings.width,
        height: settings.height,
        fps: settings.fps,
        encoder: encoder.name.clone(),
        hardware: encoder.hardware,
    }));
    // Not connected (or stopped meanwhile): the sender is gone.
    let Ok(outbox) = outbox.recv() else {
        return Ok(());
    };

    let interval = Duration::from_secs(1) / settings.fps;
    let mut next = Instant::now();
    let mut first = true;
    let failed = loop {
        if stop.load(Ordering::Relaxed) {
            break None;
        }
        if outbox.is_closed() {
            return Ok(());
        }
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        }
        // Behind by more than a frame (a slow encoder, a busy machine): skip
        // ahead instead of rushing frames out to catch up.
        next = (next + interval).max(Instant::now());

        let mut step = || -> windows::core::Result<Vec<encode::Encoded>> {
            if screen.update(&gpu)? {
                converter.set_source(&gpu, &screen.texture)?;
            }
            let frame = converter.convert()?;
            let key = std::mem::take(&mut first) | outbox.take_key_request();
            let pts_us = clock.elapsed().as_micros() as u64;
            encoder.encode(&gpu, &frame, pts_us, key)
        };
        match step() {
            Ok(frames) => {
                for frame in frames {
                    if !outbox.video(frame.key, frame.pts_us, frame.data) {
                        return Ok(());
                    }
                }
            }
            Err(error) => break Some(error.message()),
        }
    };
    // Say goodbye, but don't wait long on a network that is stuck.
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if outbox.try_end() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    match failed {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_size_fits_and_stays_even() {
        assert_eq!(encoded_size((2560, 1440), (1920, 1080)), (1920, 1080));
        assert_eq!(encoded_size((3840, 2160), (1920, 1080)), (1920, 1080));
        assert_eq!(encoded_size((1920, 1200), (1920, 1080)), (1728, 1080));
        assert_eq!(encoded_size((1366, 768), (1920, 1080)), (1366, 768));
        assert_eq!(encoded_size((1365, 767), (1920, 1080)), (1364, 766));
    }
}
