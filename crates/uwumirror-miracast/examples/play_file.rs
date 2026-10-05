//! Plays a video file through the same player and frame server a Miracast
//! sender's picture goes through, and measures how the pictures come out:
//! frames a second, their size, and the time copying each out of the
//! graphics card takes. No phone needed.
//!
//! ```text
//! ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=60 -t 20 -c:v libx264 -pix_fmt yuv420p test.mp4
//! cargo run --release -p uwumirror-miracast --example play_file -- C:\full\path\test.mp4 10
//! ```

#[cfg(windows)]
fn main() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use parking_lot::Mutex;
    use uwumirror_core::StreamEvent;
    use uwumirror_miracast::FilePlayback;

    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: play_file <absolute path> [seconds]");
        std::process::exit(2);
    };
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);
    // Where to write the 100th picture as raw NV12, to look at with
    // `ffmpeg -f rawvideo -pix_fmt nv12 -s WxH -i frame.nv12 frame.png`.
    let dump = args.next();
    let frames = Arc::new(AtomicU64::new(0));
    // The mean luma of the last picture: a picture that is all black (16)
    // or all one value would mean the copy brought nothing.
    let luma = Arc::new(Mutex::new(Vec::<f64>::new()));
    let sink = {
        let (frames, luma) = (frames.clone(), luma.clone());
        Arc::new(move |event: StreamEvent| match event {
            StreamEvent::Frame { frame, .. } => {
                let n = frames.fetch_add(1, Ordering::Relaxed);
                if let (100, Some(dump)) = (n, &dump) {
                    let _ = std::fs::write(dump, &frame.data);
                    println!(
                        "picture 100 ({} × {}) written to {dump}",
                        frame.width, frame.height
                    );
                }
                if n.is_multiple_of(30) {
                    let y = &frame.data[..(frame.width * frame.height) as usize];
                    let mean = y.iter().map(|&v| f64::from(v)).sum::<f64>() / y.len() as f64;
                    luma.lock().push(mean);
                }
            }
            other => println!("{other:?}"),
        })
    };
    let playback = match FilePlayback::start(std::path::Path::new(&path), sink, false, true) {
        Ok(playback) => playback,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let started = Instant::now();
    let mut last = 0;
    for _ in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        let now = frames.load(Ordering::Relaxed);
        let stats = playback.stats();
        println!(
            "{:>5.1} s: {} frames/s, copy {:.2} ms/frame, {} failed",
            started.elapsed().as_secs_f64(),
            now - last,
            stats.copy_us as f64 / stats.frames.max(1) as f64 / 1000.0,
            stats.failed,
        );
        last = now;
    }
    drop(playback);
    let luma = luma.lock();
    println!(
        "{} frames in {:.1} s; mean luma of sampled pictures: {:?}",
        frames.load(Ordering::Relaxed),
        started.elapsed().as_secs_f64(),
        luma.iter().map(|v| v.round()).collect::<Vec<_>>()
    );
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Miracast needs Windows.");
}
