//! A whole Android mirror against a pretend phone: a shell script stands in
//! for `adb`, and this test plays scrcpy's server on the forwarded port.

#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use uwumirror_android::{scrcpy, Adb, MirrorOptions};
use uwumirror_core::{AudioStatus, EventSink, StreamEvent, StreamKind};

fn header(first: u64, size: u32) -> Vec<u8> {
    let mut out = first.to_be_bytes().to_vec();
    out.extend_from_slice(&size.to_be_bytes());
    out
}

async fn wait_for(events: &Arc<Mutex<Vec<StreamEvent>>>, count: usize) -> Vec<StreamEvent> {
    for _ in 0..300 {
        if events.lock().len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    events.lock().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn mirrors_a_pretend_phone() {
    let phone = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = phone.local_addr().unwrap().port();
    let dir = std::env::temp_dir().join(format!("uwumirror-fake-adb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("calls");
    let script = dir.join("adb");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{log}'\ncase \"$3\" in\n  forward) [ \"$4\" = tcp:0 ] && echo {port} ;;\n  shell) sleep 30 ;;\nesac\nexit 0\n",
            log = log.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let server = dir.join("scrcpy-server");
    std::fs::write(&server, b"jar").unwrap();

    // The pretend server: video socket with its preamble and three packets,
    // then an audio socket saying "no sound on this phone".
    let pretend = tokio::spawn(async move {
        // As scrcpy's DesktopConnection: the dummy byte right after the first
        // accept, the device name once every socket is open.
        let (mut video, _) = phone.accept().await.unwrap();
        video.write_all(&[0]).await.unwrap();
        let (mut audio, _) = phone.accept().await.unwrap();
        let mut name = [0u8; 64];
        name[..7].copy_from_slice(b"Pixel 8");
        video.write_all(&name).await.unwrap();
        video
            .write_all(&0x6832_3634u32.to_be_bytes())
            .await
            .unwrap();
        audio.write_all(&0u32.to_be_bytes()).await.unwrap();
        video
            .write_all(&header((1 << 63) | 1080, 2400))
            .await
            .unwrap();
        video.write_all(&header(1 << 62, 8)).await.unwrap();
        video.write_all(&[0, 0, 0, 1, 0x67, 0, 0, 0]).await.unwrap();
        video.write_all(&header((1 << 61) | 1000, 5)).await.unwrap();
        video.write_all(&[0, 0, 0, 1, 0x65]).await.unwrap();
        video.write_all(&header(2000, 5)).await.unwrap();
        video.write_all(&[0, 0, 0, 1, 0x41]).await.unwrap();
        // Keep the sockets open until the test stops the mirror.
        tokio::time::sleep(Duration::from_secs(10)).await;
        drop((video, audio));
    });

    let events = Arc::new(Mutex::new(Vec::new()));
    let sink: EventSink = {
        let events = events.clone();
        Arc::new(move |event| events.lock().push(event))
    };
    let adb = Adb { path: script };
    let options = MirrorOptions {
        max_size: 1280,
        bit_rate: 4_000_000,
        max_fps: 30,
        audio: true,
    };
    let mut handle = scrcpy::start(
        &adb,
        "R58N123",
        Some("SM G973F".into()),
        &server,
        &options,
        sink,
    )
    .await
    .expect("mirror starts");

    let seen = wait_for(&events, 5).await;
    let StreamEvent::Started(info) = &seen[0] else {
        panic!("{seen:?}")
    };
    assert_eq!(
        (info.kind, info.name.as_str()),
        (StreamKind::Android, "Pixel 8")
    );
    assert_eq!(info.address, "R58N123");
    assert!(seen.iter().any(|e| matches!(
        e,
        StreamEvent::Audio {
            status: AudioStatus::Unavailable,
            ..
        }
    )));
    assert!(seen.iter().any(|e| matches!(
        e,
        StreamEvent::VideoSize {
            width: 1080,
            height: 2400,
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
    assert_eq!(frames.len(), 2);
    assert!(frames[0].key);
    assert_eq!(
        frames[0].data,
        vec![0, 0, 0, 1, 0x67, 0, 0, 0, 0, 0, 0, 1, 0x65],
        "SPS in front"
    );
    assert_eq!((frames[1].key, frames[1].pts_us), (false, 2000));

    handle.stop();
    let seen = wait_for(&events, 6).await;
    assert!(
        matches!(seen.last(), Some(StreamEvent::Ended { reason: None, .. })),
        "{seen:?}"
    );
    pretend.abort();

    // What adb was asked to do, in order.
    let calls = std::fs::read_to_string(&log).unwrap();
    let calls: Vec<&str> = calls.lines().collect();
    assert!(calls[0].starts_with("-s R58N123 push "));
    assert!(calls[0].ends_with(" /data/local/tmp/uwumirror-scrcpy-server.jar"));
    assert!(calls[1].starts_with("-s R58N123 forward tcp:0 localabstract:scrcpy_"));
    let shell = calls.iter().find(|c| c.contains(" shell ")).unwrap();
    for part in [
        "app_process / com.genymobile.scrcpy.Server 4.1",
        "tunnel_forward=true",
        "control=false",
        "video_codec=h264",
        "max_size=1280",
        "audio_codec=raw",
    ] {
        assert!(shell.contains(part), "{part} missing in {shell}");
    }
    for _ in 0..100 {
        if std::fs::read_to_string(&log)
            .unwrap()
            .contains(&format!("forward --remove tcp:{port}"))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains(&format!("forward --remove tcp:{port}")));
    std::fs::remove_dir_all(&dir).ok();
}
