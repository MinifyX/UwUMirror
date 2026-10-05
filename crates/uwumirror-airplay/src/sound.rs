//! AirPlay's sound: RTP over UDP, AES-CBC encrypted, decoded by FFmpeg.
//!
//! Each RTP packet's payload is encrypted with AES-128-CBC under the session
//! key and SETUP's `eiv`, starting afresh for every packet; a trailing piece
//! shorter than a block is sent in the clear. What comes out is one AAC-ELD,
//! AAC or ALAC frame, decoded and handed to the sound output.
//!
//! Live mirroring has no use for sound that arrives late, so there is no
//! resend request and no reordering beyond dropping what is older than what
//! we already played: a lost packet is a few milliseconds of silence.

use aes::cipher::{BlockDecryptMut, KeyIvInit};
use tokio::net::UdpSocket;
use uwumirror_core::audio::AudioPlayer;
use uwumirror_core::decode::{AudioCodec, AudioDecoder};
use uwumirror_core::{AudioStatus, EventSink, StreamEvent};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

/// Payload of an RTP packet that only keeps the stream alive.
const NO_DATA: [u8; 4] = [0x00, 0x68, 0x34, 0x00];

/// Decrypts one packet's payload in place.
pub fn decrypt(key: &[u8; 16], iv: &[u8; 16], payload: &mut [u8]) {
    let mut cipher = Aes128CbcDec::new(key.into(), iv.into());
    // The tail shorter than a block is left alone: senders leave it clear.
    for block in payload.as_chunks_mut::<16>().0 {
        cipher.decrypt_block_mut(block.as_mut_slice().into());
    }
}

/// AirPlay's volume is in dB, -30 (quietest) to 0, and -144 for mute.
pub fn volume_to_gain(db: f32) -> f32 {
    if db <= -144.0 || db.is_nan() {
        0.0
    } else {
        10f32.powf(db.clamp(-30.0, 0.0) / 20.0)
    }
}

pub struct SoundParams {
    pub id: u64,
    pub codec: Option<AudioCodec>,
    pub key: [u8; 16],
    pub iv: [u8; 16],
    pub sink: EventSink,
    pub volume: std::sync::Arc<std::sync::atomic::AtomicU32>,
    pub enabled: bool,
}

/// What came of the sound so far, for the log: whether packets arrive, decode
/// and are more than silence tells apart the ways a stream can stay quiet.
#[derive(Default)]
struct Stats {
    packets: u64,
    /// Packets the decoder gave nothing for.
    empty: u64,
    samples: u64,
    /// Loudest sample since the last report.
    peak: f32,
    reported: u64,
}

impl Stats {
    /// Logs after the first packet, then every 1000 (about every 10 s).
    fn report(&mut self, id: u64) {
        if self.packets == 1 || self.packets >= self.reported + 1000 {
            tracing::info!(
                id,
                packets = self.packets,
                undecoded = self.empty,
                samples = self.samples,
                peak = self.peak,
                "AirPlay sound"
            );
            self.reported = self.packets;
            self.peak = 0.0;
        }
    }
}

/// `seq` is newer than `last`, allowing for the 16-bit wrap.
fn newer(seq: u16, last: u16) -> bool {
    let diff = seq.wrapping_sub(last);
    diff != 0 && diff < 0x8000
}

/// Receives, decodes and plays until the task is aborted.
pub async fn run(data: UdpSocket, control: UdpSocket, params: SoundParams) {
    let status = |status| {
        (params.sink)(StreamEvent::Audio {
            id: params.id,
            status,
        })
    };
    // Sound that can't be played is still received, so the sender's packets
    // don't bounce as unreachable.
    tracing::info!(id = params.id, codec = ?params.codec, enabled = params.enabled, "AirPlay sound starts");
    let mut stats = Stats::default();
    let player = if !params.enabled {
        status(AudioStatus::Off);
        None
    } else {
        match params.codec.map(AudioDecoder::new) {
            None => {
                tracing::warn!("AirPlay sound in a format we don't know");
                status(AudioStatus::NoDecoder);
                None
            }
            Some(Err(error)) => {
                tracing::warn!(%error, "AirPlay sound");
                status(AudioStatus::NoDecoder);
                None
            }
            Some(Ok(decoder)) => match AudioPlayer::open(44_100, 2) {
                Ok(player) => {
                    tracing::info!(
                        id = params.id,
                        ffmpeg = ?uwumirror_core::decode::ffmpeg_version(),
                        "AirPlay sound decoder and output open"
                    );
                    status(AudioStatus::Playing);
                    Some((decoder, player))
                }
                Err(error) => {
                    tracing::warn!(%error, "sound output");
                    status(AudioStatus::NoOutput);
                    None
                }
            },
        }
    };
    let mut player = player;
    let mut last_seq: Option<u16> = None;
    let mut buffer = vec![0u8; 8192];
    let mut control_buffer = vec![0u8; 8192];
    let mut gain = f32::NAN;
    loop {
        let packet: &mut [u8] = tokio::select! {
            received = data.recv(&mut buffer) => match received {
                Ok(len) => &mut buffer[..len],
                Err(_) => continue,
            },
            // Sync packets and resent packets arrive here. A resent packet
            // (type 0x56) wraps a whole audio packet after 4 bytes.
            received = control.recv(&mut control_buffer) => match received {
                Ok(len) if len > 16 && control_buffer[1] & 0x7f == 0x56 => {
                    &mut control_buffer[4..len]
                }
                _ => continue,
            },
        };
        if packet.len() < 12 {
            continue;
        }
        if packet.len() == 12 || (packet.len() == 16 && packet[12..16] == NO_DATA) {
            continue;
        }
        // ALAC's 44-byte packets carry format information only.
        if params.codec == Some(AudioCodec::Alac) && packet.len() == 44 {
            continue;
        }
        let seq = u16::from_be_bytes([packet[2], packet[3]]);
        if let Some(last) = last_seq {
            if !newer(seq, last) {
                continue;
            }
        }
        last_seq = Some(seq);
        let Some((decoder, output)) = player.as_mut() else {
            continue;
        };
        let volume = f32::from_bits(params.volume.load(std::sync::atomic::Ordering::Relaxed));
        if volume.to_bits() != gain.to_bits() {
            gain = volume;
            output.set_volume(volume_to_gain(volume));
            tracing::info!(id = params.id, db = volume, "AirPlay volume");
        }
        let payload = &mut packet[12..];
        decrypt(&params.key, &params.iv, payload);
        let samples = decoder.decode(payload);
        stats.packets += 1;
        if samples.is_empty() {
            stats.empty += 1;
        } else {
            stats.samples += samples.len() as u64;
            stats.peak = samples.iter().fold(stats.peak, |peak, s| peak.max(s.abs()));
            output.push_f32(&samples);
        }
        stats.report(params.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncryptMut;

    #[test]
    fn decrypts_whole_blocks_and_leaves_the_tail() {
        let key = [1u8; 16];
        let iv = [2u8; 16];
        let plain: Vec<u8> = (0..37).collect();
        let mut data = plain.clone();
        let mut encryptor = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &iv.into());
        for block in data[..32].as_chunks_mut::<16>().0 {
            encryptor.encrypt_block_mut(block.as_mut_slice().into());
        }
        assert_ne!(data[..32], plain[..32]);
        decrypt(&key, &iv, &mut data);
        assert_eq!(data, plain);
    }

    #[test]
    fn sequence_numbers_wrap() {
        assert!(newer(1, 0));
        assert!(newer(0, 65535));
        assert!(!newer(65535, 0));
        assert!(!newer(5, 5));
    }

    #[test]
    fn volume_in_db() {
        assert_eq!(volume_to_gain(0.0), 1.0);
        assert_eq!(volume_to_gain(-144.0), 0.0);
        assert!((volume_to_gain(-20.0) - 0.1).abs() < 1e-6);
        assert!((volume_to_gain(-60.0) - volume_to_gain(-30.0)).abs() < 1e-6);
    }
}
