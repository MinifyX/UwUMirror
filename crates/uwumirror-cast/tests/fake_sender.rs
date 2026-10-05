//! The receiver against pretend senders over loopback: one that speaks
//! UwUCast byte by byte, one that uses the real sending half with made-up
//! frames, and some that misbehave.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uwumirror_cast::protocol::{Hello, Message, Welcome, WelcomeStatus, VERSION};
use uwumirror_cast::{sender, CastReceiver, Ending, ReceiverConfig};
use uwumirror_core::{AudioStatus, EventSink, StreamEvent, StreamKind};

const SPS_PPS_IDR: [u8; 16] = [
    0, 0, 0, 1, 0x67, 0x4d, 0, 0x28, 0, 0, 0, 1, 0x68, 0xee, 0, 0x65,
];

type Events = Arc<Mutex<Vec<StreamEvent>>>;

async fn receiver() -> (CastReceiver, Events) {
    let events: Events = Arc::default();
    let sink: EventSink = {
        let events = events.clone();
        Arc::new(move |event| events.lock().push(event))
    };
    let receiver = CastReceiver::start(
        ReceiverConfig {
            name: "UwUMirror (Test)".into(),
            // No sound device needed: the sound is said to be off.
            audio: false,
            announce: false,
        },
        sink,
    )
    .await
    .expect("receiver starts");
    (receiver, events)
}

async fn wait_for(events: &Events, done: impl Fn(&[StreamEvent]) -> bool) -> Vec<StreamEvent> {
    for _ in 0..300 {
        if done(&events.lock()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    events.lock().clone()
}

fn ended(events: &[StreamEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, StreamEvent::Ended { .. }))
}

fn hello() -> Hello {
    Hello {
        version: VERSION,
        name: "Büro-PC".into(),
        model: "Windows 11".into(),
        audio: true,
    }
}

async fn connect(receiver: &CastReceiver) -> TcpStream {
    TcpStream::connect(("127.0.0.1", receiver.port()))
        .await
        .expect("receiver listens")
}

#[tokio::test(flavor = "multi_thread")]
async fn receives_a_pretend_sender() {
    let (receiver, events) = receiver().await;
    let mut socket = connect(&receiver).await;
    socket.write_all(&hello().encode()).await.unwrap();
    let welcome = Welcome::read(&mut socket).await.unwrap();
    assert_eq!(welcome.status, WelcomeStatus::Accepted);
    assert_eq!(welcome.name, "UwUMirror (Test)");

    // A frame from the middle of a stream can't start a decoder: the
    // receiver drops it and asks for a key frame.
    Message::Video {
        key: false,
        pts_us: 1,
        data: vec![0, 0, 0, 1, 0x41, 1],
    }
    .write(&mut socket)
    .await
    .unwrap();
    let asked = tokio::time::timeout(Duration::from_secs(2), Message::read(&mut socket))
        .await
        .expect("asked in time")
        .unwrap();
    assert_eq!(asked, Message::KeyFrameRequest);

    for message in [
        Message::VideoSize {
            width: 1920,
            height: 1080,
        },
        Message::Video {
            key: true,
            pts_us: 16_666,
            data: SPS_PPS_IDR.to_vec(),
        },
        Message::Video {
            key: false,
            pts_us: 33_333,
            data: vec![0, 0, 0, 1, 0x41, 2],
        },
        Message::Audio {
            pts_us: 33_333,
            samples: vec![0; 960],
        },
        Message::End,
    ] {
        message.write(&mut socket).await.unwrap();
    }

    let seen = wait_for(&events, ended).await;
    let StreamEvent::Started(info) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert_eq!(info.kind, StreamKind::Cast);
    assert_eq!(info.name, "Büro-PC");
    assert_eq!(info.model.as_deref(), Some("Windows 11"));
    assert_eq!(info.address, "127.0.0.1");
    assert!(seen.iter().any(|e| matches!(
        e,
        StreamEvent::Audio {
            status: AudioStatus::Off,
            ..
        }
    )));
    assert!(seen.iter().any(|e| matches!(
        e,
        StreamEvent::VideoSize {
            width: 1920,
            height: 1080,
            ..
        }
    )));
    let frames: Vec<_> = seen
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Video { packet, .. } => Some(packet.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 2, "the frame before the key frame is gone");
    assert!(frames[0].key);
    assert_eq!(frames[0].data, SPS_PPS_IDR);
    assert_eq!((frames[1].key, frames[1].pts_us), (false, 33_333));
    assert!(
        matches!(seen.last(), Some(StreamEvent::Ended { reason: None, .. })),
        "{seen:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sending_half_and_ending_from_the_receiver() {
    let (receiver, events) = receiver().await;
    let address = ([127, 0, 0, 1], receiver.port()).into();
    let session = sender::connect(address, &hello()).await.expect("welcomed");
    assert_eq!(session.receiver, "UwUMirror (Test)");
    let outbox = session.outbox.clone();
    assert!(
        outbox
            .send(Message::VideoSize {
                width: 1280,
                height: 720
            })
            .await
    );
    assert!(outbox.video(true, 0, SPS_PPS_IDR.to_vec()));
    assert!(outbox.audio(0, vec![1, -1, 2, -2]));

    let seen = wait_for(&events, |e| {
        e.iter().any(|e| matches!(e, StreamEvent::Video { .. }))
    })
    .await;
    let id = seen[0].id();

    // Ended over there (the "Stop" button, or another device taking over):
    // the sender learns of it.
    assert!(receiver.end_stream(id));
    assert!(!receiver.end_stream(id + 1000));
    let ending = tokio::time::timeout(Duration::from_secs(5), session.done)
        .await
        .expect("the sender notices")
        .unwrap();
    assert_eq!(ending, Ending::ByReceiver);
    let seen = wait_for(&events, ended).await;
    assert!(matches!(
        seen.last(),
        Some(StreamEvent::Ended { reason: None, .. })
    ));
    for _ in 0..100 {
        if outbox.is_closed() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(outbox.is_closed(), "capture threads would stop now");
}

#[tokio::test(flavor = "multi_thread")]
async fn another_version_is_told_and_turned_away() {
    let (receiver, events) = receiver().await;
    let mut socket = connect(&receiver).await;
    let hello = Hello {
        version: VERSION + 1,
        ..hello()
    };
    socket.write_all(&hello.encode()).await.unwrap();
    let welcome = Welcome::read(&mut socket).await.unwrap();
    assert_eq!(welcome.status, WelcomeStatus::OtherVersion);
    assert_eq!(welcome.version, VERSION);
    let mut rest = Vec::new();
    socket.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty(), "closed after the welcome");
    assert!(events.lock().is_empty(), "no stream");

    let mut other = connect(&receiver).await;
    // Something that isn't UwUCast at all gets no answer.
    other.write_all(b"GET / HTTP/1.1\r\n\r\n").await.unwrap();
    let mut rest = Vec::new();
    let _ = other.read_to_end(&mut rest).await;
    assert!(rest.is_empty(), "not even a welcome for garbage");
}

#[tokio::test(flavor = "multi_thread")]
async fn garbage_mid_stream_ends_it_with_a_reason() {
    let (receiver, events) = receiver().await;
    let mut socket = connect(&receiver).await;
    socket.write_all(&hello().encode()).await.unwrap();
    Welcome::read(&mut socket).await.unwrap();
    // Type 1 with a length far beyond any frame: refused before allocating.
    socket
        .write_all(&[1, 0x7f, 0xff, 0xff, 0xff])
        .await
        .unwrap();
    let seen = wait_for(&events, ended).await;
    assert!(
        matches!(
            seen.last(),
            Some(StreamEvent::Ended {
                reason: Some(_),
                ..
            })
        ),
        "{seen:?}"
    );
    drop(receiver);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_receiver_ends_its_streams() {
    let (receiver, events) = receiver().await;
    let mut socket = connect(&receiver).await;
    socket.write_all(&hello().encode()).await.unwrap();
    Welcome::read(&mut socket).await.unwrap();
    wait_for(&events, |e| !e.is_empty()).await;
    drop(receiver);
    let seen = wait_for(&events, ended).await;
    assert!(ended(&seen), "{seen:?}");
    let mut rest = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), socket.read_to_end(&mut rest)).await;
}
