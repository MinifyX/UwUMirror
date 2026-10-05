//! Sending this computer's screen (Windows only).
//!
//! [`Broadcast::start`] sets up everything that can fail on this computer
//! first — the screen recording, the encoder — then connects. The picture
//! thread waits for Windows to hand over a new frame and converts and encodes
//! it at once, so a frame never waits for a fixed tick; at most
//! [`SendOptions::fps`] a second, and on a still screen the last one again
//! every [`REPEAT`]. A hardware encoder's finished frames go straight from its
//! event thread to the network, the sound from the loopback thread; both
//! through the session's [`Outbox`](crate::sender::Outbox). A task waits for
//! the end: stopped here, closed by the receiver, or failed.
//!
//! Latency, stage by stage, is in `docs/architecture.md`.

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
    /// Bits a second; `None` picks one for the picture's size and rate
    /// ([`bit_rate_for`]).
    pub bit_rate: Option<u32>,
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
            bit_rate: None,
            hardware: true,
        }
    }
}

pub use encode::{bit_rate_for, KEY_FRAME_SECONDS};

/// A still screen sends no new frames; the last one is encoded again this
/// often, which sharpens it after a key frame the rate control kept small
/// and tells the receiver the sender is still there.
const REPEAT: Duration = Duration::from_millis(100);
/// How long the picture thread waits at most before it looks at the stop
/// flag and the receiver's requests for a key frame again.
const WAIT_SLICE: Duration = Duration::from_millis(10);

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

/// The sending side's clock: [`capture::counter`], which frames are stamped
/// with when they come, tied to the wall clock once at the start.
///
/// The encoder sees microseconds since the start, small numbers as it
/// expects; the wire gets wall-clock time ([`Clock::wall`]), so the receiver
/// can tell how old a frame is. Over a long session the two clocks may drift
/// apart by a few parts per million, far below anything one could see.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    /// The counter at the start.
    start: i64,
    /// The wall clock at the start, µs since 1970.
    epoch_us: u64,
}

impl Clock {
    pub fn new() -> Self {
        Self {
            start: capture::counter(),
            epoch_us: crate::latency::wall_clock_us(),
        }
    }

    /// Microseconds since the start.
    pub fn now_us(&self) -> u64 {
        self.since_start_us(capture::counter())
    }

    /// A [`capture::counter`] time as microseconds since the start.
    pub fn since_start_us(&self, counter: i64) -> u64 {
        (counter - self.start).max(0) as u64 / 10
    }

    /// Microseconds since the start as wall-clock time, for the wire.
    pub fn wall(&self, since_start_us: u64) -> u64 {
        self.epoch_us + since_start_us
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
    let clock = Clock::new();
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
    clock: Clock,
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
    clock: Clock,
    ready: oneshot::Sender<Result<Ready, String>>,
    outbox: mpsc::Receiver<Outbox>,
) -> Result<(), String> {
    let setup = || -> windows::core::Result<_> {
        let gpu = capture::Gpu::new()?;
        let mut screen = capture::Capture::primary(&gpu)?;
        let (width, height) = encoded_size(screen.size(), (options.max_width, options.max_height));
        let fps = options.fps.clamp(10, 60);
        let encoder = encode::Encoder::open(
            &gpu,
            encode::Settings {
                width,
                height,
                fps,
                bit_rate: options
                    .bit_rate
                    .unwrap_or_else(|| bit_rate_for(width, height, fps)),
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

    // Finished frames go out from whichever thread has them, stamped with
    // wall-clock time for the receiver.
    encoder.set_sink({
        let outbox = outbox.clone();
        Box::new(move |frame: encode::Encoded| {
            outbox.video(frame.key, clock.wall(frame.pts_us), frame.data);
        })
    });

    // A little under a frame apart: a 60 Hz screen's frames come every
    // 16.7 ms give or take, and shouldn't wait for the take.
    let spacing = Duration::from_secs(1) * 4 / 5 / settings.fps;
    let mut last: Option<Instant> = None;
    let mut first = true;
    let mut last_pts = 0;
    let failed = loop {
        if stop.load(Ordering::Relaxed) {
            break None;
        }
        if outbox.is_closed() {
            return Ok(());
        }
        // A new frame from Windows, a still screen due for a repeat, or a
        // receiver that needs a key frame now.
        let fresh = screen.wait(WAIT_SLICE);
        let since = last.map(|at| at.elapsed());
        if !fresh && since.is_some_and(|since| since < REPEAT) && !outbox.key_wanted() {
            continue;
        }
        // Not much more than `fps` a second (a 144 Hz screen would otherwise
        // send 144): early, the frame waits its turn, and whatever is newest
        // then is taken.
        if let Some(since) = since.filter(|since| *since < spacing) {
            std::thread::sleep(spacing - since);
        }
        last = Some(Instant::now());

        let mut step = || -> windows::core::Result<()> {
            if screen.update(&gpu)? {
                converter.set_source(&gpu, &screen.texture)?;
            }
            let frame = converter.convert()?;
            // On its way to the card now, not when the encoder next submits.
            unsafe { gpu.context.Flush() };
            let key = std::mem::take(&mut first) | outbox.take_key_request();
            // When Windows handed the frame over; a repeat is stamped now.
            let pts_us = screen
                .captured
                .take()
                .map_or_else(|| clock.now_us(), |at| clock.since_start_us(at))
                .max(last_pts + 1);
            last_pts = pts_us;
            encoder.encode(&gpu, &frame, pts_us, key)
        };
        if let Err(error) = step() {
            break Some(error.message());
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
