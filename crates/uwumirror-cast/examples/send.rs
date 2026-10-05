//! Sends this screen to a UwUMirror on the network for a while, without the
//! app: for trying a receiver out.
//!
//! ```text
//! cargo run -p uwumirror-cast --example send -- [part of the receiver's name] [seconds]
//! ```
//!
//! Lists the receivers it finds, sends to the first whose name contains the
//! text (or the only one), and says how it ended.

#[cfg(windows)]
#[tokio::main]
async fn main() {
    use std::time::Duration;

    use uwumirror_cast::screen::{self, SendOptions};
    use uwumirror_cast::Browser;

    let mut args = std::env::args().skip(1);
    let wanted = args.next().unwrap_or_default().to_lowercase();
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);

    let browser = Browser::start().expect("mDNS");
    println!("Looking for receivers…");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let receivers = browser.receivers();
    for receiver in &receivers {
        println!(
            "  {} — UwUMirror {} at {}{}",
            receiver.name,
            receiver.version,
            receiver.address,
            if receiver.compatible {
                ""
            } else {
                " (another protocol version)"
            }
        );
    }
    let Some(receiver) = receivers
        .iter()
        .find(|r| r.name.to_lowercase().contains(&wanted))
    else {
        eprintln!("No receiver matches {wanted:?}.");
        std::process::exit(1);
    };

    let (ended_tx, ended_rx) = tokio::sync::oneshot::channel();
    let broadcast = screen::start(
        receiver.address,
        SendOptions {
            name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "UwUCast example".into()),
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
    println!("Sending to {}: {:?}", receiver.name, broadcast.info);
    let ending = tokio::select! {
        ending = ended_rx => ending.ok(),
        () = tokio::time::sleep(Duration::from_secs(seconds)) => {
            broadcast.stop();
            None
        }
    };
    let ending = match ending {
        Some(ending) => ending,
        None => {
            // Give the goodbye a moment to go out.
            tokio::time::sleep(Duration::from_millis(500)).await;
            uwumirror_cast::Ending::Stopped
        }
    };
    println!("Ended: {ending:?}");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Sending the screen works on Windows only.");
}
