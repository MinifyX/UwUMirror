//! AirPlay's clock exchange.
//!
//! The receiver asks the sender for its time every few seconds, NTP-style,
//! on the port the sender named in SETUP. UwUMirror shows frames as soon as
//! they arrive and doesn't need the answers for that, but senders expect the
//! questions: a receiver that never asks is one they eventually give up on.

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::net::UdpSocket;

/// Seconds from 1900 (NTP's epoch) to 1970 (Unix's).
const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

fn ntp_now() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() + NTP_UNIX_OFFSET;
    let fraction = (u64::from(now.subsec_nanos()) << 32) / 1_000_000_000;
    (seconds << 32) | fraction
}

/// A timing request: RTP-like header, type 0x52 with the marker bit, and our
/// send time in the last eight bytes.
pub fn request() -> [u8; 32] {
    let mut packet = [0u8; 32];
    packet[..4].copy_from_slice(&[0x80, 0xd2, 0x00, 0x07]);
    packet[24..].copy_from_slice(&ntp_now().to_be_bytes());
    packet
}

/// Asks `sender` for its time every three seconds, until the task is aborted.
pub async fn run(socket: UdpSocket, sender: SocketAddr) {
    let mut buffer = [0u8; 128];
    loop {
        if let Err(error) = socket.send_to(&request(), sender).await {
            tracing::debug!(%error, "timing request");
        }
        // Read (and drop) the answer, so the socket's queue stays empty.
        let _ = tokio::time::timeout(Duration::from_secs(3), socket.recv_from(&mut buffer)).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_carries_the_time_since_1900() {
        let packet = request();
        assert_eq!(&packet[..4], &[0x80, 0xd2, 0x00, 0x07]);
        let seconds = u64::from_be_bytes(packet[24..].try_into().unwrap()) >> 32;
        assert!(seconds > NTP_UNIX_OFFSET + 1_700_000_000);
    }
}
