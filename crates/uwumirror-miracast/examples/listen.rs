//! Starts Windows' Miracast receiver the way UwUMirror does and tells what
//! happens: the receiver's state, senders coming and going, pictures.
//!
//! ```text
//! cargo run -p uwumirror-miracast --example listen -- 60
//! ```
//!
//! Then cast to this computer from a phone ("Smart View", "Cast") or another
//! PC (Win+K). The number is how many seconds to listen (default 10).

#[cfg(windows)]
fn main() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use uwumirror_core::StreamEvent;
    use uwumirror_miracast::{Receiver, ReceiverConfig};

    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    println!("before starting: {:?}", uwumirror_miracast::probe());
    let frames = Arc::new(AtomicU64::new(0));
    let sink = {
        let frames = frames.clone();
        Arc::new(move |event: StreamEvent| match event {
            StreamEvent::Frame { frame, .. } => {
                if frames.fetch_add(1, Ordering::Relaxed).is_multiple_of(60) {
                    println!("picture {} × {}", frame.width, frame.height);
                }
            }
            other => println!("{other:?}"),
        })
    };
    let receiver = match Receiver::start(
        ReceiverConfig { audio: true },
        sink,
        Arc::new(|status| println!("status: {status:?}")),
    ) {
        Ok(receiver) => receiver,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!("listening for {seconds} s: {:?}", receiver.status());
    std::thread::sleep(Duration::from_secs(seconds));
    drop(receiver);
    println!("{} pictures", frames.load(Ordering::Relaxed));
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Miracast needs Windows.");
}
