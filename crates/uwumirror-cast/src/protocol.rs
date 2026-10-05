//! UwUCast on the wire.
//!
//! One TCP connection per stream, sender to receiver. Everything is big
//! endian, like scrcpy's protocol.
//!
//! **Hello** (sender → receiver), once:
//!
//! | bytes | field                                         |
//! | ----- | --------------------------------------------- |
//! | 8     | magic `UwUCast\0`                             |
//! | 2     | protocol version ([`VERSION`])                |
//! | 1     | flags: bit 0 = sound comes along              |
//! | 1 + n | the sender's name, UTF-8, length first        |
//! | 1 + n | what it runs on ("Windows 11"), the same way  |
//!
//! **Welcome** (receiver → sender), once:
//!
//! | bytes | field                                              |
//! | ----- | -------------------------------------------------- |
//! | 8     | magic `UwUCast\0`                                  |
//! | 2     | the protocol version the receiver speaks           |
//! | 1     | 0 = go ahead, 1 = other version, 2 = not receiving |
//! | 1 + n | the receiver's name                                |
//!
//! Then **messages**, each a type byte and a 4-byte payload length:
//!
//! | type | direction | payload                                                     |
//! | ---- | --------- | ----------------------------------------------------------- |
//! | 1    | →         | video: flags (bit 0 = key frame), PTS in µs (8), Annex B   |
//! | 2    | →         | picture size: width (4), height (4)                         |
//! | 3    | →         | sound: PTS in µs (8), s16le PCM, 48 kHz, stereo interleaved |
//! | 4    | →         | end: empty, the sender stops on purpose                     |
//! | 0x81 | ←         | key frame, please: empty                                    |
//!
//! A PTS is wall-clock time, microseconds since 1970 on the sender's clock:
//! for video the moment the screen showed the picture, for sound the moment
//! it was recorded. Players only use the differences; the receiver also
//! measures how late a frame is ([`crate::latency`]). Older senders counted
//! from their start instead, which still plays, and doesn't count as latency.
//!
//! Every length is checked before anything is allocated, and anything that
//! doesn't fit — an unknown type, a frame without a start code, odd sound —
//! ends the connection: a receiver open to the whole network takes nothing
//! on trust.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAGIC: [u8; 8] = *b"UwUCast\0";
/// Bumped whenever the wire changes in a way the other side would misread.
pub const VERSION: u16 = 1;
/// What receivers announce themselves as on mDNS.
pub const SERVICE_TYPE: &str = "_uwumirror._tcp.local.";

/// The sound UwUCast carries, always: what the receiver's player expects.
pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: usize = 2;

/// Names longer than this are cut (in whole characters) when sent.
pub const MAX_NAME: usize = 120;
/// One access unit. A 4K key frame at a high bit rate stays well below.
pub const MAX_VIDEO: usize = 16 * 1024 * 1024;
/// One piece of sound: a second of it is far more than a sender ever sends.
pub const MAX_AUDIO: usize = 8 + 48_000 * 4;
/// Either side of the picture.
pub const MAX_DIMENSION: u32 = 16_384;

const TYPE_VIDEO: u8 = 1;
const TYPE_VIDEO_SIZE: u8 = 2;
const TYPE_AUDIO: u8 = 3;
const TYPE_END: u8 = 4;
const TYPE_KEY_FRAME_REQUEST: u8 = 0x81;

const FLAG_AUDIO: u8 = 1;
const FLAG_KEY: u8 = 1;

/// What a sender says first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub version: u16,
    pub name: String,
    /// What the sender runs on, e.g. "Windows 11".
    pub model: String,
    pub audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WelcomeStatus {
    Accepted,
    /// The receiver speaks another version; the welcome says which.
    OtherVersion,
    /// The receiver isn't taking streams right now.
    Refused,
}

/// The receiver's answer to a hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Welcome {
    pub version: u16,
    pub status: WelcomeStatus,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Video {
        key: bool,
        pts_us: u64,
        /// One access unit, Annex B (start codes).
        data: Vec<u8>,
    },
    VideoSize {
        width: u32,
        height: u32,
    },
    Audio {
        pts_us: u64,
        /// Interleaved stereo at 48 kHz.
        samples: Vec<i16>,
    },
    End,
    KeyFrameRequest,
}

fn invalid(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.into())
}

/// `text` cut to [`MAX_NAME`] bytes without splitting a character.
fn short(text: &str) -> &str {
    if text.len() <= MAX_NAME {
        return text;
    }
    let mut end = MAX_NAME;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn put_text(out: &mut Vec<u8>, text: &str) {
    let text = short(text);
    out.push(text.len() as u8);
    out.extend_from_slice(text.as_bytes());
}

async fn read_text<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<String> {
    let len = reader.read_u8().await? as usize;
    if len > MAX_NAME {
        return Err(invalid("name too long"));
    }
    let mut bytes = vec![0u8; len];
    reader.read_exact(&mut bytes).await?;
    // Shown in the interface: no control characters.
    Ok(String::from_utf8_lossy(&bytes)
        .chars()
        .filter(|c| !c.is_control())
        .collect())
}

async fn read_magic<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<()> {
    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic).await?;
    if magic != MAGIC {
        return Err(invalid("not UwUCast"));
    }
    Ok(())
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&self.version.to_be_bytes());
        out.push(if self.audio { FLAG_AUDIO } else { 0 });
        put_text(&mut out, &self.name);
        put_text(&mut out, &self.model);
        out
    }

    pub async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Self> {
        read_magic(reader).await?;
        let version = reader.read_u16().await?;
        let flags = reader.read_u8().await?;
        let name = read_text(reader).await?;
        let model = read_text(reader).await?;
        Ok(Self {
            version,
            name,
            model,
            audio: flags & FLAG_AUDIO != 0,
        })
    }
}

impl Welcome {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&self.version.to_be_bytes());
        out.push(match self.status {
            WelcomeStatus::Accepted => 0,
            WelcomeStatus::OtherVersion => 1,
            WelcomeStatus::Refused => 2,
        });
        put_text(&mut out, &self.name);
        out
    }

    pub async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Self> {
        read_magic(reader).await?;
        let version = reader.read_u16().await?;
        let status = match reader.read_u8().await? {
            0 => WelcomeStatus::Accepted,
            1 => WelcomeStatus::OtherVersion,
            2 => WelcomeStatus::Refused,
            other => return Err(invalid(format!("welcome status {other}"))),
        };
        let name = read_text(reader).await?;
        Ok(Self {
            version,
            status,
            name,
        })
    }
}

/// Whether `data` starts with an Annex B start code.
pub fn is_annex_b(data: &[u8]) -> bool {
    data.starts_with(&[0, 0, 1]) || data.starts_with(&[0, 0, 0, 1])
}

impl Message {
    /// The message as it goes on the wire, header included.
    pub fn encode(&self) -> Vec<u8> {
        let (kind, payload_len) = match self {
            Message::Video { data, .. } => (TYPE_VIDEO, 9 + data.len()),
            Message::VideoSize { .. } => (TYPE_VIDEO_SIZE, 8),
            Message::Audio { samples, .. } => (TYPE_AUDIO, 8 + samples.len() * 2),
            Message::End => (TYPE_END, 0),
            Message::KeyFrameRequest => (TYPE_KEY_FRAME_REQUEST, 0),
        };
        let mut out = Vec::with_capacity(5 + payload_len);
        out.push(kind);
        out.extend_from_slice(&(payload_len as u32).to_be_bytes());
        match self {
            Message::Video { key, pts_us, data } => {
                out.push(if *key { FLAG_KEY } else { 0 });
                out.extend_from_slice(&pts_us.to_be_bytes());
                out.extend_from_slice(data);
            }
            Message::VideoSize { width, height } => {
                out.extend_from_slice(&width.to_be_bytes());
                out.extend_from_slice(&height.to_be_bytes());
            }
            Message::Audio { pts_us, samples } => {
                out.extend_from_slice(&pts_us.to_be_bytes());
                for sample in samples {
                    out.extend_from_slice(&sample.to_le_bytes());
                }
            }
            Message::End | Message::KeyFrameRequest => {}
        }
        out
    }

    pub async fn write<W: AsyncWrite + Unpin>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.encode()).await
    }

    /// Reads one message. A connection that closes between two messages
    /// gives [`io::ErrorKind::UnexpectedEof`], like scrcpy's sockets.
    pub async fn read<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Self> {
        let kind = reader.read_u8().await?;
        let len = reader.read_u32().await? as usize;
        let limit = match kind {
            TYPE_VIDEO => MAX_VIDEO + 9,
            TYPE_VIDEO_SIZE => 8,
            TYPE_AUDIO => MAX_AUDIO,
            TYPE_END | TYPE_KEY_FRAME_REQUEST => 0,
            other => return Err(invalid(format!("unknown message type {other:#x}"))),
        };
        if len > limit {
            return Err(invalid(format!("message type {kind} too long ({len})")));
        }
        let mut payload = vec![0u8; len];
        reader.read_exact(&mut payload).await?;
        match kind {
            TYPE_VIDEO => {
                if len < 9 {
                    return Err(invalid("video message too short"));
                }
                let data = payload.split_off(9);
                if !is_annex_b(&data) {
                    return Err(invalid("video without a start code"));
                }
                Ok(Message::Video {
                    key: payload[0] & FLAG_KEY != 0,
                    pts_us: u64::from_be_bytes(payload[1..9].try_into().expect("8 bytes")),
                    data,
                })
            }
            TYPE_VIDEO_SIZE => {
                if len != 8 {
                    return Err(invalid("picture size message of the wrong size"));
                }
                let width = u32::from_be_bytes(payload[..4].try_into().expect("4 bytes"));
                let height = u32::from_be_bytes(payload[4..].try_into().expect("4 bytes"));
                if !(1..=MAX_DIMENSION).contains(&width) || !(1..=MAX_DIMENSION).contains(&height) {
                    return Err(invalid(format!("picture size {width} × {height}")));
                }
                Ok(Message::VideoSize { width, height })
            }
            TYPE_AUDIO => {
                // Whole stereo frames only, or left and right swap places.
                if len < 8 || !(len - 8).is_multiple_of(2 * AUDIO_CHANNELS) {
                    return Err(invalid("sound message of an odd size"));
                }
                let pts_us = u64::from_be_bytes(payload[..8].try_into().expect("8 bytes"));
                let samples = payload[8..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&b| i16::from_le_bytes(b))
                    .collect();
                Ok(Message::Audio { pts_us, samples })
            }
            TYPE_END => Ok(Message::End),
            _ => Ok(Message::KeyFrameRequest),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hello_and_welcome_round_trip() {
        let hello = Hello {
            version: VERSION,
            name: "Büro-PC".into(),
            model: "Windows 11".into(),
            audio: true,
        };
        assert_eq!(Hello::read(&mut &hello.encode()[..]).await.unwrap(), hello);
        let welcome = Welcome {
            version: VERSION,
            status: WelcomeStatus::OtherVersion,
            name: "UwUMirror (Wohnzimmer)".into(),
        };
        assert_eq!(
            Welcome::read(&mut &welcome.encode()[..]).await.unwrap(),
            welcome
        );
    }

    #[tokio::test]
    async fn long_names_are_cut_between_characters() {
        let hello = Hello {
            version: VERSION,
            name: "ü".repeat(100),
            model: String::new(),
            audio: false,
        };
        let read = Hello::read(&mut &hello.encode()[..]).await.unwrap();
        assert_eq!(read.name, "ü".repeat(MAX_NAME / 2));
    }

    #[tokio::test]
    async fn messages_round_trip() {
        let messages = [
            Message::VideoSize {
                width: 1920,
                height: 1080,
            },
            Message::Video {
                key: true,
                pts_us: 33_333,
                data: vec![0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x65],
            },
            Message::Audio {
                pts_us: 40_000,
                samples: vec![1, -1, i16::MAX, i16::MIN],
            },
            Message::End,
            Message::KeyFrameRequest,
        ];
        let wire: Vec<u8> = messages.iter().flat_map(Message::encode).collect();
        let mut reader = &wire[..];
        for message in &messages {
            assert_eq!(&Message::read(&mut reader).await.unwrap(), message);
        }
        let end = Message::read(&mut reader).await.unwrap_err();
        assert_eq!(end.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn garbage_is_refused() {
        let mut not_magic = Hello {
            version: VERSION,
            name: "x".into(),
            model: String::new(),
            audio: false,
        }
        .encode();
        not_magic[0] = b'X';
        assert!(Hello::read(&mut &not_magic[..]).await.is_err());

        let refused = |wire: Vec<u8>| async move { Message::read(&mut &wire[..]).await.is_err() };
        // Unknown type.
        assert!(refused(vec![9, 0, 0, 0, 0]).await);
        // Longer than any frame may be: refused before anything is allocated.
        assert!(refused(vec![TYPE_VIDEO, 0xff, 0xff, 0xff, 0xff]).await);
        // A frame without a start code.
        let mut frame = vec![TYPE_VIDEO, 0, 0, 0, 11, 1];
        frame.extend_from_slice(&[0; 8]);
        frame.extend_from_slice(&[0x65, 0x88]);
        assert!(refused(frame).await);
        // Half a stereo frame of sound.
        let mut sound = vec![TYPE_AUDIO, 0, 0, 0, 10];
        sound.extend_from_slice(&[0; 10]);
        assert!(refused(sound).await);
        // A picture of no size, and an end that carries something.
        let mut size = vec![TYPE_VIDEO_SIZE, 0, 0, 0, 8];
        size.extend_from_slice(&[0; 8]);
        assert!(refused(size).await);
        assert!(refused(vec![TYPE_END, 0, 0, 0, 1, 0]).await);
    }
}
