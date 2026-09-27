//! The mirrored picture: one TCP connection, H.264 in AES-CTR.
//!
//! Every packet starts with a 128-byte header: payload size (u32, little
//! endian), payload type (0 = a frame, 1 = SPS and PPS, 2 and 5 = heartbeats
//! and reports), an option byte, and an NTP timestamp. Frames are encrypted
//! with AES-128-CTR as one continuous key stream across all packets, and hold
//! NAL units with 4-byte length prefixes; the page's decoder wants Annex B
//! start codes instead, so the lengths are swapped for `00 00 00 01`.
//! Parameter sets arrive in the clear, in an avcC-like layout, and go in
//! front of the next frame, which is always the matching IDR.

use aes::cipher::{KeyIvInit, StreamCipher};
use sha2::{Digest, Sha512};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use uwumirror_core::{EventSink, StreamEvent, VideoPacket};

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

const START: [u8; 4] = [0, 0, 0, 1];
/// Bigger than any frame a sender makes at the resolutions we offer.
const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// The stream's key and IV, from the session key and SETUP's
/// `streamConnectionID`.
pub fn stream_cipher(session_key: &[u8; 16], stream_connection_id: u64) -> Aes128Ctr {
    let derive = |label: &str| {
        let hash = Sha512::new()
            .chain_update(format!("{label}{stream_connection_id}"))
            .chain_update(session_key)
            .finalize();
        let mut out = [0u8; 16];
        out.copy_from_slice(&hash[..16]);
        out
    };
    let key = derive("AirPlayStreamKey");
    let iv = derive("AirPlayStreamIV");
    Aes128Ctr::new(&key.into(), &iv.into())
}

/// SPS and PPS from a type-1 packet, as Annex B.
pub fn parameter_sets(payload: &[u8]) -> Option<Vec<u8>> {
    let be16 = |at: usize| -> Option<usize> {
        Some(u16::from_be_bytes([*payload.get(at)?, *payload.get(at + 1)?]) as usize)
    };
    let sps_len = be16(6)?;
    let sps = payload.get(8..8 + sps_len)?;
    let pps_len = be16(8 + sps_len + 1)?;
    let pps_at = 8 + sps_len + 3;
    let pps = payload.get(pps_at..pps_at + pps_len)?;
    let mut out = Vec::with_capacity(sps.len() + pps.len() + 8);
    out.extend_from_slice(&START);
    out.extend_from_slice(sps);
    out.extend_from_slice(&START);
    out.extend_from_slice(pps);
    Some(out)
}

/// Rewrites 4-byte length prefixes into start codes, in place. Returns whether
/// an IDR slice is among the units, or `None` when the lengths don't add up —
/// which is what a wrongly decrypted frame looks like.
pub fn to_annex_b(data: &mut [u8]) -> Option<bool> {
    let mut at = 0;
    let mut key = false;
    while at < data.len() {
        let length = u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize;
        if length == 0 || length > data.len() - at - 4 {
            return None;
        }
        data[at..at + 4].copy_from_slice(&START);
        let header = data[at + 4];
        if header & 0x80 != 0 {
            return None;
        }
        if header & 0x1f == 5 {
            key = true;
        }
        at += 4 + length;
    }
    Some(key)
}

fn ntp_to_micros(timestamp: u64) -> u64 {
    let seconds = timestamp >> 32;
    let fraction = timestamp & 0xffff_ffff;
    seconds * 1_000_000 + ((fraction * 1_000_000) >> 32)
}

fn le_f32(header: &[u8; 128], at: usize) -> f32 {
    f32::from_le_bytes(header[at..at + 4].try_into().expect("4 bytes"))
}

struct Mirror {
    id: u64,
    sink: EventSink,
    cipher: Aes128Ctr,
    pending_parameters: Option<Vec<u8>>,
    size: (u32, u32),
    paused: bool,
}

impl Mirror {
    fn packet(&mut self, header: &[u8; 128], mut payload: Vec<u8>) {
        let pts_us = ntp_to_micros(u64::from_le_bytes(
            header[8..16].try_into().expect("8 bytes"),
        ));
        match header[4] {
            0 => {
                self.cipher.apply_keystream(&mut payload);
                let Some(mut key) = to_annex_b(&mut payload) else {
                    tracing::debug!(len = payload.len(), "dropping a frame that doesn't parse");
                    return;
                };
                let data = match self.pending_parameters.take() {
                    Some(mut parameters) => {
                        parameters.extend_from_slice(&payload);
                        key = true;
                        parameters
                    }
                    None => payload,
                };
                self.paused = false;
                (self.sink)(StreamEvent::Video {
                    id: self.id,
                    packet: VideoPacket { data, key, pts_us },
                });
            }
            1 => {
                // 0x56 (H.264) and 0x5e (HEVC) mark "the picture stops here":
                // the screen was locked, or the sender went to sleep.
                if matches!(header[6], 0x56 | 0x5e) && !self.paused {
                    self.paused = true;
                    (self.sink)(StreamEvent::VideoPaused { id: self.id });
                }
                let (width, height) = (le_f32(header, 56), le_f32(header, 60));
                let size = (width as u32, height as u32);
                if size.0 > 0 && size.1 > 0 && size != self.size {
                    self.size = size;
                    (self.sink)(StreamEvent::VideoSize {
                        id: self.id,
                        width: size.0,
                        height: size.1,
                    });
                }
                match parameter_sets(&payload) {
                    Some(parameters) => self.pending_parameters = Some(parameters),
                    None => tracing::warn!(len = payload.len(), "unreadable parameter sets"),
                }
            }
            _ => {}
        }
    }
}

async fn read_connection(stream: &mut TcpStream, mirror: &mut Mirror) -> std::io::Result<()> {
    let mut header = [0u8; 128];
    loop {
        stream.read_exact(&mut header).await?;
        let size = u32::from_le_bytes(header[0..4].try_into().expect("4 bytes")) as usize;
        if size > MAX_PAYLOAD {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "packet too large",
            ));
        }
        let mut payload = vec![0u8; size];
        stream.read_exact(&mut payload).await?;
        mirror.packet(&header, payload);
    }
}

/// Serves the mirroring connection until the task is aborted. A sender that
/// drops the connection may open it again; the key stream carries on.
pub async fn serve(listener: TcpListener, cipher: Aes128Ctr, id: u64, sink: EventSink) {
    let mut mirror = Mirror {
        id,
        sink,
        cipher,
        pending_parameters: None,
        size: (0, 0),
        paused: false,
    };
    loop {
        let (mut stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "mirroring: accept");
                return;
            }
        };
        let _ = stream.set_nodelay(true);
        tracing::info!(%peer, "mirroring connected");
        if let Err(error) = read_connection(&mut stream, &mut mirror).await {
            tracing::info!(%error, "mirroring connection ended");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_prefixes_become_start_codes() {
        let mut data = vec![0, 0, 0, 2, 0x65, 0xaa, 0, 0, 0, 1, 0x41];
        assert_eq!(to_annex_b(&mut data), Some(true));
        assert_eq!(data, vec![0, 0, 0, 1, 0x65, 0xaa, 0, 0, 0, 1, 0x41]);
        let mut p_frame = vec![0, 0, 0, 1, 0x41];
        assert_eq!(to_annex_b(&mut p_frame), Some(false));
    }

    #[test]
    fn garbage_is_recognised() {
        assert_eq!(to_annex_b(&mut [0, 0, 0, 9, 0x65]), None);
        assert_eq!(
            to_annex_b(&mut [0, 0, 0, 1, 0xe5]),
            None,
            "forbidden bit set"
        );
        assert_eq!(to_annex_b(&mut [0, 0]), None);
    }

    #[test]
    fn parameter_sets_from_the_avcc_layout() {
        // version, profile, compat, level, 0xff, 0xe1, SPS length, SPS, 1, PPS length, PPS
        let payload = [
            1, 0x64, 0, 0x28, 0xff, 0xe1, 0, 3, 0x67, 0x64, 0x28, 1, 0, 2, 0x68, 0xee,
        ];
        assert_eq!(
            parameter_sets(&payload).unwrap(),
            vec![0, 0, 0, 1, 0x67, 0x64, 0x28, 0, 0, 0, 1, 0x68, 0xee]
        );
        assert!(parameter_sets(&payload[..10]).is_none());
    }

    #[test]
    fn frames_decrypt_across_packet_boundaries() {
        let key = [7u8; 16];
        // Encrypt two frames with one continuous key stream, as a sender does.
        let frames = [
            vec![0u8, 0, 0, 5, 0x65, 1, 2, 3, 4],
            vec![0u8, 0, 0, 2, 0x41, 9],
        ];
        let mut sender = stream_cipher(&key, 1234);
        let encrypted: Vec<Vec<u8>> = frames
            .iter()
            .map(|frame| {
                let mut copy = frame.clone();
                sender.apply_keystream(&mut copy);
                copy
            })
            .collect();

        let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let seen_by_sink = seen.clone();
        let sink: EventSink = std::sync::Arc::new(move |event| seen_by_sink.lock().push(event));
        let mut mirror = Mirror {
            id: 1,
            sink,
            cipher: stream_cipher(&key, 1234),
            pending_parameters: None,
            size: (0, 0),
            paused: false,
        };
        let mut codec = [0u8; 128];
        codec[4] = 1;
        codec[56..60].copy_from_slice(&1920f32.to_le_bytes());
        codec[60..64].copy_from_slice(&1080f32.to_le_bytes());
        mirror.packet(
            &codec,
            vec![1, 0x64, 0, 0x28, 0xff, 0xe1, 0, 1, 0x67, 1, 0, 1, 0x68],
        );
        for frame in encrypted {
            mirror.packet(&[0u8; 128], frame);
        }
        let seen = seen.lock();
        assert!(matches!(
            seen[0],
            StreamEvent::VideoSize {
                width: 1920,
                height: 1080,
                ..
            }
        ));
        let StreamEvent::Video { packet, .. } = &seen[1] else {
            panic!("{:?}", seen[1])
        };
        assert!(packet.key);
        assert_eq!(&packet.data[..10], &[0, 0, 0, 1, 0x67, 0, 0, 0, 1, 0x68]);
        assert_eq!(&packet.data[10..], &[0, 0, 0, 1, 0x65, 1, 2, 3, 4]);
        let StreamEvent::Video { packet, .. } = &seen[2] else {
            panic!("{:?}", seen[2])
        };
        assert!(!packet.key);
        assert_eq!(packet.data, vec![0, 0, 0, 1, 0x41, 9]);
    }
}
