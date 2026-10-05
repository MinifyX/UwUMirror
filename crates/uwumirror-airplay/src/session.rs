//! One sender's control connection, from `GET /info` to TEARDOWN.
//!
//! The order a mirroring iPhone follows:
//!
//! 1. `GET /info` — what are you?
//! 2. `POST /pair-setup`, `POST /pair-verify` (twice) — legacy pairing.
//!    A Mac that wants a PIN sends `POST /pair-pin-start` and three
//!    `POST /pair-setup-pin` in place of `/pair-setup` the first time, and
//!    later goes straight to pair-verify (see `pin.rs`).
//! 3. `POST /fp-setup` (twice) — FairPlay.
//! 4. `SETUP` with `ekey`/`eiv` — the session key, and the timing port.
//! 5. `SETUP` with stream 110 — the picture; we answer with a TCP port.
//! 6. `SETUP` with stream 96 — the sound; we answer with two UDP ports.
//! 7. `RECORD`, `SET_PARAMETER` (volume), `POST /feedback` every two seconds.
//! 8. `TEARDOWN` for each stream, then for the whole session.
//!
//! AirPlay audio (Music, Podcasts) is the same without step 5.

use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use plist::{Dictionary, Value};
use sha2::{Digest, Sha512};
use tokio::io::BufReader;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::task::JoinHandle;
use uwumirror_core::decode::AudioCodec;
use uwumirror_core::{next_stream_id, StreamEvent, StreamInfo, StreamKind};

use crate::advertise;
use crate::fairplay::FairPlay;
use crate::pairing::PairVerify;
use crate::pin::{Outcome, PairingEvent, PinSetup};
use crate::rtsp::{self, Request, Response};
use crate::{mirror, sound, timing, Shared};

/// A running stream and everything that serves it.
struct Stream {
    id: u64,
    kind: Option<StreamKind>,
    info: StreamInfo,
    shared: Arc<Shared>,
    timing: Option<JoinHandle<()>>,
    mirror: Option<JoinHandle<()>>,
    sound: Option<JoinHandle<()>>,
    /// AirPlay volume in dB, as f32 bits.
    volume: Arc<AtomicU32>,
}

impl Stream {
    fn announce(&mut self, kind: StreamKind) {
        // Sound-only turns into mirroring when a picture joins (never back).
        if self.kind == Some(StreamKind::Airplay) || self.kind == Some(kind) {
            return;
        }
        self.kind = Some(kind);
        self.info.kind = kind;
        self.shared
            .streams
            .lock()
            .insert(self.id, self.info.clone());
        (self.shared.sink)(StreamEvent::Started(self.info.clone()));
    }

    fn stop_mirror(&mut self) {
        if let Some(task) = self.mirror.take() {
            task.abort();
        }
    }

    fn stop_sound(&mut self) {
        if let Some(task) = self.sound.take() {
            task.abort();
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        for task in [self.timing.take(), self.mirror.take(), self.sound.take()]
            .into_iter()
            .flatten()
        {
            task.abort();
        }
        if self.kind.is_some() {
            self.shared.streams.lock().remove(&self.id);
            (self.shared.sink)(StreamEvent::Ended {
                id: self.id,
                reason: None,
            });
        }
    }
}

pub struct Connection {
    shared: Arc<Shared>,
    peer: SocketAddr,
    fairplay: FairPlay,
    verify: PairVerify,
    pin: PinSetup,
    /// This connection did pair-setup (with or without a PIN); without it,
    /// pair-verify is for devices that paired with a PIN before.
    set_up: bool,
    session_key: Option<[u8; 16]>,
    iv: Option<[u8; 16]>,
    stream: Option<Stream>,
    /// The id of the stream this connection carries, for `Receiver::end_stream`.
    watch: Arc<AtomicU64>,
}

fn parse_plist(body: &[u8]) -> Option<Value> {
    Value::from_reader(Cursor::new(body)).ok()
}

fn unsigned(dict: &Dictionary, key: &str) -> Option<u64> {
    let value = dict.get(key)?;
    // plist integers can arrive signed; AirPlay means the raw 64 bits.
    value
        .as_unsigned_integer()
        .or_else(|| value.as_signed_integer().map(|v| v as u64))
}

fn string(dict: &Dictionary, key: &str) -> Option<String> {
    dict.get(key).and_then(|v| v.as_string()).map(str::to_owned)
}

fn data16(dict: &Dictionary, key: &str) -> Option<[u8; 16]> {
    dict.get(key)?.as_data()?.get(..16)?.try_into().ok()
}

fn ipv4_any() -> SocketAddr {
    SocketAddr::from(([0, 0, 0, 0], 0))
}

impl Connection {
    pub fn new(shared: Arc<Shared>, peer: SocketAddr, watch: Arc<AtomicU64>) -> Self {
        Self {
            shared,
            peer,
            fairplay: FairPlay::default(),
            verify: PairVerify::default(),
            pin: PinSetup::default(),
            set_up: false,
            session_key: None,
            iv: None,
            stream: None,
            watch,
        }
    }

    /// Serves requests until the sender hangs up (or the task is aborted).
    pub async fn run(mut self, socket: TcpStream) {
        let (read, mut write) = socket.into_split();
        let mut reader = BufReader::new(read);
        loop {
            let request = match rtsp::read_request(&mut reader).await {
                Ok(Some(request)) => request,
                Ok(None) => break,
                Err(error) => {
                    tracing::info!(peer = %self.peer, %error, "AirPlay connection");
                    break;
                }
            };
            tracing::debug!(method = %request.method, uri = %request.uri, "AirPlay request");
            let response = self.handle(&request).await;
            if rtsp::write_response(&mut write, &response, &request)
                .await
                .is_err()
            {
                break;
            }
            if response.close {
                break;
            }
        }
        // Dropping the stream ends it and tells the app.
    }

    async fn handle(&mut self, request: &Request) -> Response {
        let response = match (request.method.as_str(), request.path()) {
            ("GET", "/info") => {
                let body = if request.is_plist() {
                    parse_plist(&request.body)
                } else {
                    None
                };
                Response::ok().plist(&advertise::info(
                    &self.shared.identity,
                    &self.shared.offer,
                    body.as_ref(),
                ))
            }
            ("POST", "/pair-setup") if request.body.len() == 32 => {
                self.set_up = true;
                Response::ok().octets(self.shared.identity.public_key().to_vec())
            }
            ("POST", "/pair-pin-start") => self.pair_pin_start(),
            ("POST", "/pair-setup-pin") => self.pair_setup_pin(request),
            ("POST", "/pair-verify") if !self.may_verify(&request.body) => {
                tracing::info!(peer = %self.peer, "pair-verify from a device that must pair first");
                Response::status(470, "Connection Authorization Required")
            }
            ("POST", "/pair-verify") => {
                match self.verify.step(&self.shared.identity, &request.body) {
                    Ok(reply) => Response::ok().octets(reply),
                    Err(error) => {
                        tracing::warn!(peer = %self.peer, %error, "pair-verify");
                        Response::status(470, "Connection Authorization Required").closing()
                    }
                }
            }
            ("POST", "/fp-setup") => match self.fairplay.setup(&request.body) {
                Some(reply) => Response::ok().octets(reply),
                None => Response::status(501, "Not Implemented"),
            },
            ("POST", "/feedback") | ("POST", "/audioMode") => Response::ok(),
            ("OPTIONS", _) => Response::ok().header(
                "Public",
                "SETUP, RECORD, FLUSH, TEARDOWN, OPTIONS, GET_PARAMETER, SET_PARAMETER",
            ),
            ("SETUP", _) => self.setup(request).await,
            ("RECORD", _) => Response::ok()
                .header("Audio-Latency", "11025")
                .header("Audio-Jack-Status", "connected; type=analog"),
            ("GET_PARAMETER", _) => {
                let db = self
                    .stream
                    .as_ref()
                    .map(|s| f32::from_bits(s.volume.load(Ordering::Relaxed)))
                    .unwrap_or(0.0);
                Response::ok().body(
                    "text/parameters",
                    format!("volume: {db:.6}\r\n").into_bytes(),
                )
            }
            ("SET_PARAMETER", _) => {
                self.set_parameter(request);
                Response::ok()
            }
            ("FLUSH", _) => Response::ok(),
            ("TEARDOWN", _) => self.teardown(request),
            _ => {
                tracing::debug!(method = %request.method, uri = %request.uri, "not handled");
                Response::status(501, "Not Implemented")
            }
        };
        if request.method != "RECORD" && request.header("CSeq").is_some() {
            response.header("Audio-Jack-Status", "connected; type=digital")
        } else {
            response
        }
    }

    fn pairing_event(&self, event: PairingEvent) {
        (self.shared.on_pairing)(event);
    }

    fn address(&self) -> String {
        self.peer.ip().to_string()
    }

    /// `POST /pair-pin-start`: a new PIN on screen.
    fn pair_pin_start(&mut self) -> Response {
        let started = self.shared.pin.lock().start(Instant::now());
        match started {
            Ok(pin) => {
                tracing::info!(peer = %self.peer, "a device asks for a PIN");
                self.pairing_event(PairingEvent::PinRequested {
                    pin,
                    address: self.address(),
                });
                Response::ok()
            }
            Err(error) => {
                tracing::warn!(peer = %self.peer, %error, "pair-pin-start");
                self.pairing_event(PairingEvent::Failed {
                    address: self.address(),
                    reason: error.failure(),
                });
                Response::status(503, "Service Unavailable")
            }
        }
    }

    /// `POST /pair-setup-pin`, one of its three steps.
    fn pair_setup_pin(&mut self, request: &Request) -> Response {
        let refused = || Response::status(470, "Client Authentication Failure");
        // Each step is a small plist; nothing larger needs reading.
        if request.body.len() > 4096 {
            return refused();
        }
        let Some(body) = parse_plist(&request.body).and_then(Value::into_dictionary) else {
            return refused();
        };
        let outcome = self.pin.step(
            &self.shared.pin,
            &self.shared.identity,
            &body,
            Instant::now(),
        );
        match outcome {
            Ok(Outcome::Reply(reply)) => Response::ok().plist(&Value::Dictionary(reply)),
            Ok(Outcome::Paired { reply, user, key }) => {
                tracing::info!(peer = %self.peer, device = %user, "paired with a PIN");
                if let Err(error) = self.shared.trusted.lock().add(key, &user) {
                    tracing::warn!(%error, "remembering a trusted AirPlay device");
                }
                self.set_up = true;
                self.pairing_event(PairingEvent::Paired {
                    address: self.address(),
                    device: user,
                });
                Response::ok().plist(&Value::Dictionary(reply))
            }
            Err(error) => {
                tracing::warn!(peer = %self.peer, %error, "pair-setup-pin");
                self.pairing_event(PairingEvent::Failed {
                    address: self.address(),
                    reason: error.failure(),
                });
                refused()
            }
        }
    }

    /// Whether this `pair-verify` may go ahead. After pair-setup on this
    /// connection, always — that is how iPhones come. Without it, only a
    /// device that paired with a PIN before: Macs remember the receiver and
    /// skip pair-setup next time, and anyone else doing that is told to pair
    /// (470), which makes a Mac ask for a PIN again.
    fn may_verify(&self, body: &[u8]) -> bool {
        if self.set_up || body.first() != Some(&1) || body.len() != 4 + 32 + 32 {
            return true;
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&body[36..68]);
        self.shared.trusted.lock().contains(&key)
    }

    fn set_parameter(&mut self, request: &Request) {
        let Some(stream) = &self.stream else { return };
        let text = String::from_utf8_lossy(&request.body);
        if let Some(value) = text.trim().strip_prefix("volume:") {
            if let Ok(db) = value.trim().parse::<f32>() {
                stream.volume.store(db.to_bits(), Ordering::Relaxed);
            }
        }
    }

    fn teardown(&mut self, request: &Request) -> Response {
        let types: Vec<u64> = parse_plist(&request.body)
            .as_ref()
            .and_then(|v| v.as_dictionary())
            .and_then(|d| d.get("streams"))
            .and_then(|s| s.as_array())
            .map(|streams| {
                streams
                    .iter()
                    .filter_map(|s| s.as_dictionary().and_then(|d| unsigned(d, "type")))
                    .collect()
            })
            .unwrap_or_default();
        match &mut self.stream {
            Some(stream) if !types.is_empty() => {
                if types.contains(&96) {
                    stream.stop_sound();
                }
                if types.contains(&110) {
                    stream.stop_mirror();
                }
                // A sound-only stream is over when its sound is.
                if stream.kind == Some(StreamKind::AirplayAudio) && stream.sound.is_none() {
                    self.stream = None;
                }
            }
            _ => self.stream = None,
        }
        Response::ok().header("Connection", "close")
    }

    async fn setup(&mut self, request: &Request) -> Response {
        let Some(body) = parse_plist(&request.body) else {
            return Response::status(400, "Bad Request");
        };
        let Some(dict) = body.as_dictionary() else {
            return Response::status(400, "Bad Request");
        };
        let mut reply = Dictionary::new();

        if let (Some(ekey), Some(iv)) = (
            dict.get("ekey").and_then(|v| v.as_data()),
            data16(dict, "eiv"),
        ) {
            if self.verify.shared_secret().is_some() && !self.verify.verified() {
                tracing::warn!(peer = %self.peer, "SETUP without a finished pair-verify");
            }
            let Some(mut key) = self.fairplay.decrypt(ekey) else {
                tracing::warn!(peer = %self.peer, "SETUP before FairPlay");
                return Response::status(403, "Forbidden").closing();
            };
            // With pair-verify done, the key is seasoned with its secret.
            if let Some(secret) = self.verify.shared_secret() {
                let hash = Sha512::new()
                    .chain_update(key)
                    .chain_update(secret)
                    .finalize();
                key.copy_from_slice(&hash[..16]);
            }
            self.session_key = Some(key);
            self.iv = Some(iv);

            let name = string(dict, "name").unwrap_or_else(|| "AirPlay".into());
            let id = next_stream_id();
            let info = StreamInfo {
                id,
                kind: StreamKind::AirplayAudio,
                name,
                model: string(dict, "model"),
                address: self.peer.ip().to_string(),
            };
            let mut stream = Stream {
                id,
                kind: None,
                info,
                shared: self.shared.clone(),
                timing: None,
                mirror: None,
                sound: None,
                volume: Arc::new(AtomicU32::new((-15.0f32).to_bits())),
            };
            let mut timing_port = 0u16;
            if let Some(remote) = unsigned(dict, "timingPort").filter(|p| *p > 0 && *p < 65536) {
                if let Ok(socket) = UdpSocket::bind(ipv4_any()).await {
                    timing_port = socket.local_addr().map(|a| a.port()).unwrap_or(0);
                    let sender = SocketAddr::new(self.peer.ip(), remote as u16);
                    stream.timing = Some(tokio::spawn(timing::run(socket, sender)));
                }
            }
            reply.insert(
                "timingPort".into(),
                Value::Integer(u64::from(timing_port).into()),
            );
            reply.insert("eventPort".into(), Value::Integer(0.into()));
            // A new SETUP 1 on the same connection replaces the old stream.
            self.watch.store(id, Ordering::Relaxed);
            self.stream = Some(stream);
        }

        if let Some(streams) = dict.get("streams").and_then(|v| v.as_array()) {
            let (Some(key), Some(iv), Some(stream)) = (self.session_key, self.iv, &mut self.stream)
            else {
                return Response::status(455, "Method Not Valid In This State");
            };
            let mut answers = Vec::new();
            for entry in streams.iter().filter_map(|s| s.as_dictionary()) {
                match unsigned(entry, "type") {
                    Some(110) => {
                        let connection_id = unsigned(entry, "streamConnectionID").unwrap_or(0);
                        let Ok(listener) = TcpListener::bind(ipv4_any()).await else {
                            return Response::status(500, "Internal Server Error").closing();
                        };
                        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                        stream.announce(StreamKind::Airplay);
                        stream.stop_mirror();
                        let cipher = mirror::stream_cipher(&key, connection_id);
                        stream.mirror = Some(tokio::spawn(mirror::serve(
                            listener,
                            cipher,
                            stream.id,
                            self.shared.sink.clone(),
                        )));
                        let mut answer = Dictionary::new();
                        answer.insert("type".into(), Value::Integer(110.into()));
                        answer.insert("dataPort".into(), Value::Integer(u64::from(port).into()));
                        answers.push(Value::Dictionary(answer));
                    }
                    Some(96) => {
                        let codec = unsigned(entry, "ct").and_then(AudioCodec::from_airplay_ct);
                        tracing::info!(
                            peer = %self.peer,
                            ct = ?unsigned(entry, "ct"),
                            audio_format = ?unsigned(entry, "audioFormat"),
                            spf = ?unsigned(entry, "spf"),
                            "SETUP for sound"
                        );
                        let (Ok(data), Ok(control)) = (
                            UdpSocket::bind(ipv4_any()).await,
                            UdpSocket::bind(ipv4_any()).await,
                        ) else {
                            return Response::status(500, "Internal Server Error").closing();
                        };
                        let data_port = data.local_addr().map(|a| a.port()).unwrap_or(0);
                        let control_port = control.local_addr().map(|a| a.port()).unwrap_or(0);
                        stream.announce(StreamKind::AirplayAudio);
                        stream.stop_sound();
                        let params = sound::SoundParams {
                            id: stream.id,
                            codec,
                            key,
                            iv,
                            sink: self.shared.sink.clone(),
                            volume: stream.volume.clone(),
                            enabled: self.shared.audio_enabled(),
                        };
                        stream.sound = Some(tokio::spawn(sound::run(data, control, params)));
                        let mut answer = Dictionary::new();
                        answer.insert("type".into(), Value::Integer(96.into()));
                        answer.insert(
                            "dataPort".into(),
                            Value::Integer(u64::from(data_port).into()),
                        );
                        answer.insert(
                            "controlPort".into(),
                            Value::Integer(u64::from(control_port).into()),
                        );
                        answers.push(Value::Dictionary(answer));
                    }
                    other => {
                        tracing::warn!(?other, "SETUP for a stream type we don't know");
                    }
                }
            }
            reply.insert("streams".into(), Value::Array(answers));
        }
        Response::ok().plist(&Value::Dictionary(reply))
    }
}
