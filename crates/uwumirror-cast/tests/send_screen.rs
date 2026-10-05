//! The real thing on one Windows computer: record this screen and what it
//! plays, encode with Media Foundation, send it over loopback to a receiver
//! in the same process, and check what arrives.
//!
//! Ignored by default: they need a desktop session that can be recorded (CI
//! runners have none), show Windows' yellow recording frame for a few
//! seconds, and the sound test plays a quiet tone. Run them with
//!
//! ```text
//! cargo test -p uwumirror-cast --test send_screen -- --ignored --nocapture --test-threads 1
//! ```

#![cfg(windows)]

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use uwumirror_cast::h264::nal_units;
use uwumirror_cast::protocol::{Hello, Message, Welcome, WelcomeStatus, VERSION};
use uwumirror_cast::screen::{self, SendOptions, Sending};
use uwumirror_cast::{CastReceiver, Ending, ReceiverConfig};
use uwumirror_core::audio::AudioPlayer;
use uwumirror_core::{EventSink, StreamEvent, StreamKind, VideoPacket};

const SECONDS: u64 = 3;

/// Sends for a few seconds to a receiver in this process; what it saw.
async fn send(options: SendOptions) -> (Sending, Vec<StreamEvent>) {
    let events: Arc<Mutex<Vec<StreamEvent>>> = Arc::default();
    let sink: EventSink = {
        let events = events.clone();
        Arc::new(move |event| events.lock().push(event))
    };
    let receiver = CastReceiver::start(
        ReceiverConfig {
            name: "Loopback".into(),
            // Played here, it would be recorded again.
            audio: false,
            announce: false,
        },
        sink,
    )
    .await
    .expect("receiver starts");

    let (ended_tx, ended_rx) = oneshot::channel();
    let broadcast = screen::start(
        ([127, 0, 0, 1], receiver.port()).into(),
        options,
        move |ending| {
            let _ = ended_tx.send(ending);
        },
    )
    .await
    .expect("sending starts");
    println!("{:?}", broadcast.info);
    assert_eq!(broadcast.info.receiver, "Loopback");

    tokio::time::sleep(Duration::from_secs(SECONDS)).await;
    broadcast.stop();
    let ending = tokio::time::timeout(Duration::from_secs(5), ended_rx)
        .await
        .expect("ends in time")
        .unwrap();
    assert_eq!(ending, Ending::Stopped);
    let seen = events.lock().clone();
    (broadcast.info.clone(), seen)
}

fn check(info: &Sending, seen: &[StreamEvent]) -> Vec<VideoPacket> {
    let StreamEvent::Started(stream) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert_eq!(
        (stream.kind, stream.name.as_str()),
        (StreamKind::Cast, "Test-PC")
    );
    assert!(stream
        .model
        .as_deref()
        .is_some_and(|m| m.starts_with("Windows")));
    let size = seen.iter().find_map(|e| match e {
        StreamEvent::VideoSize { width, height, .. } => Some((*width, *height)),
        _ => None,
    });
    assert_eq!(size, Some((info.width, info.height)));

    let frames: Vec<VideoPacket> = seen
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Video { packet, .. } => Some(packet.clone()),
            _ => None,
        })
        .collect();
    let keys = frames.iter().filter(|f| f.key).count();
    let bytes: usize = frames.iter().map(|f| f.data.len()).sum();
    println!(
        "{} frames in {SECONDS} s ({keys} key frames, {} kB) at {} × {}",
        frames.len(),
        bytes / 1024,
        info.width,
        info.height
    );
    // Frames come as the screen changes; a still one is sent again ten
    // times a second.
    let expected = (10 * SECONDS) as usize;
    assert!(
        frames.len() >= expected / 2,
        "{} frames, expected at least about {expected}",
        frames.len()
    );
    // The first frame starts a decoder: SPS, PPS and an IDR slice.
    let first = &frames[0];
    assert!(first.key);
    let kinds: Vec<u8> = nal_units(&first.data)
        .iter()
        .map(|unit| unit[0] & 0x1f)
        .collect();
    println!("first frame's NAL units: {kinds:?}");
    for wanted in [7, 8, 5] {
        assert!(
            kinds.contains(&wanted),
            "NAL type {wanted} missing: {kinds:?}"
        );
    }
    let sps = nal_units(&first.data)
        .into_iter()
        .find(|unit| unit[0] & 0x1f == 7)
        .unwrap();
    println!("profile_idc {}, level_idc {}", sps[1], sps[3]);
    // Key frames only at the start (and on request, or every few seconds),
    // each with its parameter sets.
    assert!((1..=2).contains(&keys), "{keys} key frames in {SECONDS} s");
    for frame in frames.iter().filter(|f| f.key) {
        let kinds: Vec<u8> = nal_units(&frame.data).iter().map(|u| u[0] & 0x1f).collect();
        assert!(kinds.contains(&7) && kinds.contains(&8), "{kinds:?}");
    }
    assert!(
        frames[1..].iter().any(|f| !f.key),
        "only key frames: no prediction?"
    );
    assert!(frames.windows(2).all(|w| w[0].pts_us < w[1].pts_us));
    // Stamped with the wall clock (latency is the `latency` example's).
    let now = uwumirror_cast::latency::wall_clock_us();
    assert!(
        frames
            .iter()
            .all(|f| f.pts_us <= now && now - f.pts_us < 60_000_000),
        "not wall-clock time"
    );
    // Stopped here: the receiver saw the end, without a reason.
    assert!(
        matches!(seen.last(), Some(StreamEvent::Ended { reason: None, .. })),
        "{:?}",
        seen.last()
    );
    // For a look at the picture: UWUMIRROR_DUMP=folder writes the stream as
    // raw H.264, which `ffmpeg -i x.h264 -frames:v 1 x.png` can show.
    if let Some(folder) = std::env::var_os("UWUMIRROR_DUMP") {
        let path = std::path::Path::new(&folder).join(format!(
            "{}.h264",
            if info.hardware {
                "hardware"
            } else {
                "software"
            }
        ));
        let stream: Vec<u8> = frames.iter().flat_map(|f| f.data.clone()).collect();
        std::fs::write(&path, stream).unwrap();
        println!("written to {}", path.display());
    }
    frames
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "records the screen; needs a desktop session"]
async fn sends_this_screen_with_the_graphics_cards_encoder() {
    let (info, seen) = send(SendOptions {
        name: "Test-PC".into(),
        audio: false,
        ..SendOptions::default()
    })
    .await;
    check(&info, &seen);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "records the screen; needs a desktop session"]
async fn sends_this_screen_with_windows_own_encoder() {
    let (info, seen) = send(SendOptions {
        name: "Test-PC".into(),
        audio: false,
        hardware: false,
        ..SendOptions::default()
    })
    .await;
    assert!(!info.hardware);
    assert_eq!(info.fps, screen::SOFTWARE_FPS);
    check(&info, &seen);
}

/// The sound, checked on the wire: a pretend receiver counts it while a
/// quiet tone plays.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "records the screen and plays a quiet tone"]
async fn sends_what_this_computer_plays() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let counted = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let hello = Hello::read(&mut socket).await.unwrap();
        assert!(hello.audio);
        socket
            .write_all(
                &Welcome {
                    version: VERSION,
                    status: WelcomeStatus::Accepted,
                    name: "Counter".into(),
                }
                .encode(),
            )
            .await
            .unwrap();
        let (mut samples, mut loudest, mut frames) = (0usize, 0i16, 0usize);
        loop {
            match Message::read(&mut socket).await {
                Ok(Message::Audio { samples: s, .. }) => {
                    samples += s.len();
                    loudest = loudest.max(s.iter().map(|v| v.saturating_abs()).max().unwrap_or(0));
                }
                Ok(Message::Video { .. }) => frames += 1,
                Ok(Message::End) | Err(_) => break,
                Ok(_) => {}
            }
        }
        (samples, loudest, frames)
    });

    // A quiet 440 Hz tone through the speakers while sending.
    let tone = std::thread::spawn(|| {
        let player = AudioPlayer::open(48_000, 2).expect("a sound output");
        player.set_volume(0.05);
        let mut phase = 0f32;
        for _ in 0..(SECONDS * 100 + 50) {
            let mut chunk = Vec::with_capacity(960);
            for _ in 0..480 {
                let v = (phase * std::f32::consts::TAU).sin() * 0.5;
                phase = (phase + 440.0 / 48_000.0) % 1.0;
                chunk.extend([v, v]);
            }
            player.push_f32(&chunk);
            std::thread::sleep(Duration::from_millis(10));
        }
    });

    let (ended_tx, ended_rx) = oneshot::channel();
    let broadcast = screen::start(
        ([127, 0, 0, 1], port).into(),
        SendOptions {
            name: "Test-PC".into(),
            ..SendOptions::default()
        },
        move |ending| {
            let _ = ended_tx.send(ending);
        },
    )
    .await
    .expect("sending starts");
    assert!(broadcast.info.audio, "the loopback opened");
    tokio::time::sleep(Duration::from_secs(SECONDS)).await;
    broadcast.stop();
    assert_eq!(ended_rx.await.unwrap(), Ending::Stopped);
    let (samples, loudest, frames) = counted.await.unwrap();
    tone.join().unwrap();
    let seconds = samples as f64 / 2.0 / 48_000.0;
    println!("{seconds:.2} s of sound, loudest {loudest}, {frames} frames");
    assert!(frames > 0);
    assert!(seconds > SECONDS as f64 / 2.0, "{seconds:.2} s of sound");
    assert!(loudest > 100, "silence: loudest sample {loudest}");
}
