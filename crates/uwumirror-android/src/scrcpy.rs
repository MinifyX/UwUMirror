//! Android mirroring with scrcpy's server.
//!
//! scrcpy (Genymobile, Apache-2.0) is the best way there is to see an Android
//! screen on a computer: a small Java server, pushed over adb and started with
//! `app_process`, records the screen with the phone's own hardware encoder
//! and streams H.264 back through an adb tunnel. UwUMirror ships that server
//! (see `scripts/fetch-scrcpy-server.mjs`) and speaks its protocol, version
//! 4.1 — server and client must match exactly.
//!
//! The tunnel is a forward (`adb forward tcp:N localabstract:scrcpy_<id>`):
//! we connect, the phone's server accepts. The first socket gets a dummy byte
//! (a forward accepts even when nothing listens yet, so a connection that
//! closes at once means "try again"), then the device name. Each socket
//! starts with a codec id; then come 12-byte headers, each followed by one
//! packet. Sound is asked for as raw 48 kHz PCM, which needs no decoder and
//! is only 1.5 Mbit/s on the network.

use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rand::Rng;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::sync::oneshot;
use uwumirror_core::audio::AudioPlayer;
use uwumirror_core::{
    next_stream_id, AudioStatus, EventSink, StreamEvent, StreamInfo, StreamKind, VideoPacket,
};

use crate::adb::{Adb, AdbError};

/// The scrcpy version whose server `resources/scrcpy-server` is.
pub const SERVER_VERSION: &str = "4.1";
const DEVICE_PATH: &str = "/data/local/tmp/uwumirror-scrcpy-server.jar";

const CODEC_H264: u32 = 0x6832_3634;
const CODEC_RAW: u32 = 0x0072_6177;
const FLAG_SESSION: u64 = 1 << 63;
const FLAG_CONFIG: u64 = 1 << 62;
const FLAG_KEY_FRAME: u64 = 1 << 61;
const PTS_MASK: u64 = FLAG_KEY_FRAME - 1;
const MAX_PACKET: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct MirrorOptions {
    /// The longer side of the picture, in pixels; 0 keeps the phone's own.
    pub max_size: u32,
    pub bit_rate: u32,
    pub max_fps: u32,
    pub audio: bool,
}

impl Default for MirrorOptions {
    fn default() -> Self {
        Self {
            max_size: 1920,
            bit_rate: 8_000_000,
            max_fps: 60,
            audio: true,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScrcpyError {
    #[error(transparent)]
    Adb(#[from] AdbError),
    #[error("the phone's mirroring server didn't answer{0}")]
    NoConnection(String),
    #[error("the phone refused to record its screen{0}")]
    Refused(String),
    #[error("unexpected answer from the phone: {0}")]
    Protocol(String),
}

/// The server's last words, for an error message worth reading.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<VecDeque<String>>>);

impl Log {
    fn push(&self, line: String) {
        let mut lines = self.0.lock();
        if lines.len() == 8 {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    fn errors(&self) -> String {
        let lines = self.0.lock();
        let interesting: Vec<&str> = lines
            .iter()
            .map(String::as_str)
            .filter(|line| line.contains("ERROR") || line.contains("Exception"))
            .collect();
        match interesting.last() {
            Some(line) => format!(": {}", line.trim()),
            None => String::new(),
        }
    }

    fn follow(&self, stream: impl AsyncRead + Unpin + Send + 'static) {
        let log = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "scrcpy", "{line}");
                log.push(line);
            }
        });
    }
}

/// A running mirror. Dropping the handle doesn't end it; [`MirrorHandle::stop`] does.
pub struct MirrorHandle {
    pub id: u64,
    pub serial: String,
    stop: Option<oneshot::Sender<()>>,
}

impl MirrorHandle {
    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

async fn connect_first(port: u16, log: &Log, server: &mut Child) -> Result<TcpStream, ScrcpyError> {
    for _ in 0..100 {
        if let Ok(Some(_)) = server.try_wait() {
            break;
        }
        if let Ok(mut socket) = TcpStream::connect(("127.0.0.1", port)).await {
            let mut dummy = [0u8; 1];
            let read = socket.read_exact(&mut dummy);
            if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(2), read).await {
                return Ok(socket);
            }
            // Closed at once: the server isn't listening yet.
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(ScrcpyError::NoConnection(log.errors()))
}

async fn read_u32(socket: &mut TcpStream) -> std::io::Result<u32> {
    let mut bytes = [0u8; 4];
    socket.read_exact(&mut bytes).await?;
    Ok(u32::from_be_bytes(bytes))
}

/// One step of the video socket, parsed.
#[derive(Debug, PartialEq, Eq)]
pub enum Packet {
    Session {
        width: u32,
        height: u32,
    },
    Config(Vec<u8>),
    Media {
        key: bool,
        pts_us: u64,
        data: Vec<u8>,
    },
}

pub async fn read_packet<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Packet> {
    let mut header = [0u8; 12];
    reader.read_exact(&mut header).await?;
    let first = u64::from_be_bytes(header[..8].try_into().expect("8 bytes"));
    let size = u32::from_be_bytes(header[8..].try_into().expect("4 bytes"));
    if first & FLAG_SESSION != 0 {
        let width = u32::from_be_bytes(header[4..8].try_into().expect("4 bytes"));
        return Ok(Packet::Session {
            width,
            height: size,
        });
    }
    let size = size as usize;
    if size > MAX_PACKET {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "packet too large",
        ));
    }
    let mut data = vec![0u8; size];
    reader.read_exact(&mut data).await?;
    if first & FLAG_CONFIG != 0 {
        return Ok(Packet::Config(data));
    }
    Ok(Packet::Media {
        key: first & FLAG_KEY_FRAME != 0,
        pts_us: first & PTS_MASK,
        data,
    })
}

async fn pump_video(mut socket: TcpStream, id: u64, sink: EventSink) -> std::io::Result<()> {
    let mut config: Option<Vec<u8>> = None;
    loop {
        match read_packet(&mut socket).await? {
            Packet::Session { width, height } => {
                sink(StreamEvent::VideoSize { id, width, height });
            }
            Packet::Config(data) => config = Some(data),
            Packet::Media { key, pts_us, data } => {
                // The decoder gets SPS and PPS right in front of the frame
                // that needs them, as with AirPlay.
                let data = match (key, &config) {
                    (true, Some(config)) => [config.as_slice(), &data].concat(),
                    _ => data,
                };
                sink(StreamEvent::Video {
                    id,
                    packet: VideoPacket { data, key, pts_us },
                });
            }
        }
    }
}

async fn pump_audio(mut socket: TcpStream, player: AudioPlayer) -> std::io::Result<()> {
    loop {
        if let Packet::Media { data, .. } = read_packet(&mut socket).await? {
            let samples: Vec<i16> = data
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            player.push_i16(&samples);
        }
    }
}

/// Starts mirroring `serial`. `server` is the scrcpy-server file shipped with
/// the app; `model` is what `adb devices -l` said, for the stream's info.
pub async fn start(
    adb: &Adb,
    serial: &str,
    model: Option<String>,
    server: &Path,
    options: &MirrorOptions,
    sink: EventSink,
) -> Result<MirrorHandle, ScrcpyError> {
    let server_path = server.to_string_lossy();
    adb.run(
        &["-s", serial, "push", &server_path, DEVICE_PATH],
        Duration::from_secs(60),
    )
    .await?;

    let scid: u32 = rand::thread_rng().gen_range(0..0x8000_0000);
    let socket_name = format!("localabstract:scrcpy_{scid:08x}");
    let forwarded = adb
        .run(
            &["-s", serial, "forward", "tcp:0", &socket_name],
            Duration::from_secs(15),
        )
        .await?;
    let port: u16 = forwarded
        .trim()
        .parse()
        .map_err(|_| ScrcpyError::Protocol(format!("adb forward said {forwarded:?}")))?;

    let remove_forward = {
        let adb = adb.clone();
        let serial = serial.to_owned();
        move || async move {
            let _ = adb
                .run(
                    &["-s", &serial, "forward", "--remove", &format!("tcp:{port}")],
                    Duration::from_secs(10),
                )
                .await;
        }
    };

    let mut args = vec![
        "-s".to_owned(),
        serial.to_owned(),
        "shell".to_owned(),
        format!("CLASSPATH={DEVICE_PATH}"),
        "app_process".to_owned(),
        "/".to_owned(),
        "com.genymobile.scrcpy.Server".to_owned(),
        SERVER_VERSION.to_owned(),
        format!("scid={scid:08x}"),
        "log_level=info".to_owned(),
        "tunnel_forward=true".to_owned(),
        "control=false".to_owned(),
        "video_codec=h264".to_owned(),
        format!("video_bit_rate={}", options.bit_rate),
        format!("max_fps={}", options.max_fps),
        "cleanup=true".to_owned(),
    ];
    if options.max_size > 0 {
        args.push(format!("max_size={}", options.max_size));
    }
    if options.audio {
        args.push("audio_codec=raw".to_owned());
    } else {
        args.push("audio=false".to_owned());
    }
    let mut child = match adb
        .command()
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            remove_forward().await;
            return Err(AdbError::Spawn(error).into());
        }
    };
    let log = Log::default();
    if let Some(stdout) = child.stdout.take() {
        log.follow(stdout);
    }
    if let Some(stderr) = child.stderr.take() {
        log.follow(stderr);
    }

    let setup = async {
        let mut video = connect_first(port, &log, &mut child).await?;
        let audio = if options.audio {
            Some(
                TcpStream::connect(("127.0.0.1", port))
                    .await
                    .map_err(|e| ScrcpyError::NoConnection(format!(" (sound: {e})")))?,
            )
        } else {
            None
        };
        let mut meta = [0u8; 64];
        video
            .read_exact(&mut meta)
            .await
            .map_err(|_| ScrcpyError::Refused(log.errors()))?;
        let end = meta.iter().position(|&b| b == 0).unwrap_or(meta.len());
        let name = String::from_utf8_lossy(&meta[..end]).into_owned();
        match read_u32(&mut video)
            .await
            .map_err(|_| ScrcpyError::Refused(log.errors()))?
        {
            CODEC_H264 => {}
            0 | 1 => return Err(ScrcpyError::Refused(log.errors())),
            other => return Err(ScrcpyError::Protocol(format!("video codec {other:#x}"))),
        }
        Ok::<_, ScrcpyError>((video, audio, name))
    };
    let (video, audio, name) = match setup.await {
        Ok(setup) => setup,
        Err(error) => {
            let _ = child.kill().await;
            remove_forward().await;
            return Err(error);
        }
    };

    let id = next_stream_id();
    let display = if name.is_empty() {
        model.clone().unwrap_or_else(|| serial.to_owned())
    } else {
        name
    };
    sink(StreamEvent::Started(StreamInfo {
        id,
        kind: StreamKind::Android,
        name: display,
        model,
        address: serial.to_owned(),
    }));

    // Sound: its own socket, its own task. Failing sound never ends the picture.
    let audio_task = match audio {
        None => {
            sink(StreamEvent::Audio {
                id,
                status: AudioStatus::Off,
            });
            None
        }
        Some(mut socket) => match read_u32(&mut socket).await {
            Ok(CODEC_RAW) => match AudioPlayer::open(48_000, 2) {
                Ok(player) => {
                    sink(StreamEvent::Audio {
                        id,
                        status: AudioStatus::Playing,
                    });
                    Some(tokio::spawn(pump_audio(socket, player)))
                }
                Err(error) => {
                    tracing::warn!(%error, "sound output");
                    sink(StreamEvent::Audio {
                        id,
                        status: AudioStatus::NoOutput,
                    });
                    None
                }
            },
            // 0: the phone can't record sound (Android 10 and older).
            _ => {
                sink(StreamEvent::Audio {
                    id,
                    status: AudioStatus::Unavailable,
                });
                None
            }
        },
    };

    let (stop_tx, stop_rx) = oneshot::channel();
    tokio::spawn({
        let sink = sink.clone();
        async move {
            let reason = tokio::select! {
                result = pump_video(video, id, sink.clone()) => match result {
                    Err(error) if error.kind() != std::io::ErrorKind::UnexpectedEof => {
                        Some(error.to_string())
                    }
                    _ => {
                        let errors = log.errors();
                        (!errors.is_empty()).then(|| errors.trim_start_matches(": ").to_owned())
                    }
                },
                _ = stop_rx => None,
            };
            if let Some(task) = audio_task {
                task.abort();
            }
            let _ = child.kill().await;
            remove_forward().await;
            sink(StreamEvent::Ended { id, reason });
        }
    });
    Ok(MirrorHandle {
        id,
        serial: serial.to_owned(),
        stop: Some(stop_tx),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(first: u64, size: u32) -> Vec<u8> {
        let mut out = first.to_be_bytes().to_vec();
        out.extend_from_slice(&size.to_be_bytes());
        out
    }

    #[tokio::test]
    async fn reads_session_config_and_media_packets() {
        let mut wire = Vec::new();
        wire.extend(header(FLAG_SESSION | 1080, 2400));
        wire.extend(header(FLAG_CONFIG, 3));
        wire.extend([0x67, 0x68, 0x69]);
        wire.extend(header(FLAG_KEY_FRAME | 16_666, 2));
        wire.extend([0x65, 0x01]);
        wire.extend(header(33_333, 1));
        wire.extend([0x41]);
        let mut reader = &wire[..];
        assert_eq!(
            read_packet(&mut reader).await.unwrap(),
            Packet::Session {
                width: 1080,
                height: 2400
            }
        );
        assert_eq!(
            read_packet(&mut reader).await.unwrap(),
            Packet::Config(vec![0x67, 0x68, 0x69])
        );
        assert_eq!(
            read_packet(&mut reader).await.unwrap(),
            Packet::Media {
                key: true,
                pts_us: 16_666,
                data: vec![0x65, 0x01]
            }
        );
        assert_eq!(
            read_packet(&mut reader).await.unwrap(),
            Packet::Media {
                key: false,
                pts_us: 33_333,
                data: vec![0x41]
            }
        );
        assert!(read_packet(&mut reader).await.is_err());
    }

    #[tokio::test]
    async fn refuses_huge_packets() {
        let wire = header(0, u32::MAX);
        assert!(read_packet(&mut &wire[..]).await.is_err());
    }
}
