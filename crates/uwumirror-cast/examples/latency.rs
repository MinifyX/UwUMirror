//! How late UwUCast's picture is, on this computer: sends this screen to a
//! receiver in the same process (no mDNS, any free port) and measures capture
//! to received for every frame.
//!
//! ```text
//! cargo run --release -p uwumirror-cast --example latency -- [seconds] [frames.bin]
//! ```
//!
//! `UWUMIRROR_SOFTWARE=1` tries Windows' own encoder instead of the card's.
//!
//! The screen should change all the time while it runs — a video, a moving
//! window — or only the few frames Windows sends for a still screen count.
//! With a file name, every frame is also written there (a 4-byte length, a
//! key frame byte, the PTS and the receive time in µs, 8 bytes each, little
//! endian, then the Annex B data) for a look at decoding in a browser.

#[cfg(windows)]
#[tokio::main]
async fn main() {
    use std::io::Write;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use parking_lot::Mutex;
    use uwumirror_cast::latency::{self, Stats};
    use uwumirror_cast::screen::{self, SendOptions};
    use uwumirror_cast::{CastReceiver, ReceiverConfig};
    use uwumirror_core::{EventSink, StreamEvent};

    // The encoder's settings and the receiver's own latency lines;
    // RUST_LOG overrides.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "uwumirror_cast=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    let dump = args.next();
    // UWUMIRROR_SOFTWARE=1: Windows' own encoder instead of the card's.
    let software = std::env::var_os("UWUMIRROR_SOFTWARE").is_some();

    #[derive(Default)]
    struct Seen {
        stats: Stats,
        frames: usize,
        keys: usize,
        largest: usize,
        dump: Vec<u8>,
    }
    let seen: Arc<Mutex<Seen>> = Arc::default();
    // The first second is setup: the encoder warming up, the first key frame.
    let counting_from = Instant::now() + Duration::from_secs(1);
    let sink: EventSink = {
        let seen = seen.clone();
        let keep = dump.is_some();
        Arc::new(move |event| {
            if let StreamEvent::Video { packet, .. } = event {
                let now = latency::wall_clock_us();
                let mut seen = seen.lock();
                if keep {
                    seen.dump
                        .extend_from_slice(&(packet.data.len() as u32).to_le_bytes());
                    seen.dump.push(u8::from(packet.key));
                    seen.dump.extend_from_slice(&packet.pts_us.to_le_bytes());
                    seen.dump.extend_from_slice(&now.to_le_bytes());
                    seen.dump.extend_from_slice(&packet.data);
                }
                if Instant::now() < counting_from {
                    return;
                }
                seen.stats.frame(packet.pts_us, packet.data.len());
                seen.frames += 1;
                seen.keys += usize::from(packet.key);
                seen.largest = seen.largest.max(packet.data.len());
            }
        })
    };
    let receiver = CastReceiver::start(
        ReceiverConfig {
            name: "Latency".into(),
            audio: false,
            announce: false,
        },
        sink,
    )
    .await
    .expect("receiver starts");

    let (ended_tx, ended_rx) = tokio::sync::oneshot::channel();
    let broadcast = screen::start(
        ([127, 0, 0, 1], receiver.port()).into(),
        SendOptions {
            name: "Latency".into(),
            audio: false,
            hardware: !software,
            ..SendOptions::default()
        },
        move |ending| {
            let _ = ended_tx.send(ending);
        },
    )
    .await
    .unwrap_or_else(|error| {
        eprintln!("Can't send: {error}");
        std::process::exit(1);
    });
    let info = broadcast.info.clone();
    println!(
        "{} × {} at {} fps with {} ({})",
        info.width,
        info.height,
        info.fps,
        info.encoder,
        if info.hardware {
            "graphics card"
        } else {
            "processor"
        }
    );
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    broadcast.stop();
    let ending = tokio::time::timeout(Duration::from_secs(5), ended_rx).await;
    let seen = seen.lock();
    let counted = seconds.saturating_sub(1).max(1) as f64;
    match seen.stats.summary() {
        Some(summary) => println!("capture → received: {summary}"),
        None => println!("no frames counted: did the screen change?"),
    }
    println!(
        "{:.1} frames/s, {} key frames, {:.1} Mbit/s, {:.0} kB a frame on average, {:.0} kB the largest",
        seen.frames as f64 / counted,
        seen.keys,
        seen.stats.bytes as f64 * 8.0 / counted / 1e6,
        seen.stats.bytes as f64 / seen.frames.max(1) as f64 / 1024.0,
        seen.largest as f64 / 1024.0
    );
    if let Some(path) = dump {
        std::fs::File::create(&path)
            .and_then(|mut file| file.write_all(&seen.dump))
            .expect("writing the frames");
        println!("frames written to {path}");
    }
    println!("ended: {ending:?}");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Sending the screen works on Windows only.");
}
