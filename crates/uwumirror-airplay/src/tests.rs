//! A whole mirroring session against the real receiver, with this test as
//! the iPhone: info, pairing, FairPlay, both SETUPs, an encrypted frame on
//! the mirroring connection, sound set up, teardown.

use std::sync::Arc;
use std::time::Duration;

use aes::cipher::StreamCipher;
use ed25519_dalek::{Signer, SigningKey};
use parking_lot::Mutex;
use plist::{Dictionary, Value};
use sha2::{Digest, Sha512};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use uwumirror_core::{AudioStatus, EventSink, StreamEvent, StreamKind};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::fairplay::FairPlay;
use crate::mirror::stream_cipher;
use crate::pin::tests::Mac;
use crate::{PairingEvent, PairingFailure, PairingSink, Receiver, ReceiverConfig};

struct Sender {
    stream: BufReader<TcpStream>,
    cseq: u32,
}

impl Sender {
    async fn request(
        &mut self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> (u16, Vec<u8>) {
        self.cseq += 1;
        let mut head = format!("{method} {uri} RTSP/1.0\r\nCSeq: {}\r\n", self.cseq);
        if let Some(content_type) = content_type {
            head.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        let socket = self.stream.get_mut();
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(body).await.unwrap();

        let mut status = String::new();
        self.stream.read_line(&mut status).await.unwrap();
        let code: u16 = status.split(' ').nth(1).unwrap().parse().unwrap();
        let mut length = 0;
        let mut cseq_seen = false;
        loop {
            let mut line = String::new();
            self.stream.read_line(&mut line).await.unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = value.parse().unwrap();
            }
            if line == format!("CSeq: {}", self.cseq) {
                cseq_seen = true;
            }
        }
        assert!(cseq_seen, "response to {method} {uri} without our CSeq");
        let mut body = vec![0; length];
        self.stream.read_exact(&mut body).await.unwrap();
        (code, body)
    }

    async fn plist(&mut self, method: &str, uri: &str, value: Value) -> Dictionary {
        let mut body = Vec::new();
        plist::to_writer_binary(&mut body, &value).unwrap();
        let (code, reply) = self
            .request(method, uri, Some("application/x-apple-binary-plist"), &body)
            .await;
        assert_eq!(code, 200, "{method} {uri}");
        Value::from_reader(std::io::Cursor::new(reply))
            .unwrap()
            .into_dictionary()
            .unwrap()
    }
}

fn dict(entries: Vec<(&str, Value)>) -> Value {
    let mut d = Dictionary::new();
    for (key, value) in entries {
        d.insert(key.into(), value);
    }
    Value::Dictionary(d)
}

async fn next_event(
    events: &Arc<Mutex<Vec<StreamEvent>>>,
    seen: &mut usize,
    what: &str,
) -> StreamEvent {
    for _ in 0..200 {
        if let Some(event) = events.lock().get(*seen).cloned() {
            *seen += 1;
            return event;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no event: waited for {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_iphone_mirrors_a_frame() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink: EventSink = {
        let events = events.clone();
        Arc::new(move |event| events.lock().push(event))
    };
    let dir = std::env::temp_dir().join(format!("uwumirror-e2e-{}", std::process::id()));
    let config = ReceiverConfig {
        name: "UwUMirror Test".into(),
        width: 1280,
        height: 720,
        fps: 30,
        audio: false,
        identity_path: dir.join("airplay-identity"),
        trusted_path: dir.join("airplay-trusted"),
    };
    let receiver = match Receiver::start(config, sink, Arc::new(|_| {})).await {
        Ok(receiver) => receiver,
        Err(crate::ReceiverError::Announce(error)) => {
            eprintln!("no multicast here ({error}), skipping");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    let socket = TcpStream::connect(("127.0.0.1", receiver.port()))
        .await
        .unwrap();
    let mut sender = Sender {
        stream: BufReader::new(socket),
        cseq: 0,
    };

    // What are you?
    let (code, info) = sender.request("GET", "/info", None, &[]).await;
    assert_eq!(code, 200);
    let info = Value::from_reader(std::io::Cursor::new(info)).unwrap();
    let info = info.as_dictionary().unwrap();
    assert_eq!(
        info.get("name").unwrap().as_string(),
        Some("UwUMirror Test")
    );
    let receiver_pk = info.get("pk").unwrap().as_data().unwrap().to_vec();

    // Pairing.
    let signing = SigningKey::from_bytes(&[11u8; 32]);
    let (code, pk) = sender
        .request(
            "POST",
            "/pair-setup",
            None,
            &signing.verifying_key().to_bytes(),
        )
        .await;
    assert_eq!((code, pk.as_slice()), (200, receiver_pk.as_slice()));
    let secret = StaticSecret::from([12u8; 32]);
    let public = PublicKey::from(&secret);
    let mut first = vec![1, 0, 0, 0];
    first.extend_from_slice(public.as_bytes());
    first.extend_from_slice(&signing.verifying_key().to_bytes());
    let (code, reply) = sender.request("POST", "/pair-verify", None, &first).await;
    assert_eq!(code, 200);
    let theirs: [u8; 32] = reply[..32].try_into().unwrap();
    let shared = *secret.diffie_hellman(&PublicKey::from(theirs)).as_bytes();
    let derive = |salt: &[u8]| -> [u8; 16] {
        Sha512::new()
            .chain_update(salt)
            .chain_update(shared)
            .finalize()[..16]
            .try_into()
            .unwrap()
    };
    let mut ctr = ctr::Ctr128BE::<aes::Aes128>::new(
        &derive(b"Pair-Verify-AES-Key").into(),
        &derive(b"Pair-Verify-AES-IV").into(),
    );
    use aes::cipher::KeyIvInit;
    let mut skip = [0u8; 64];
    ctr.apply_keystream(&mut skip);
    let mut message = public.as_bytes().to_vec();
    message.extend_from_slice(&theirs);
    let mut signature = signing.sign(&message).to_bytes();
    ctr.apply_keystream(&mut signature);
    let mut second = vec![0, 0, 0, 0];
    second.extend_from_slice(&signature);
    assert_eq!(
        sender
            .request("POST", "/pair-verify", None, &second)
            .await
            .0,
        200
    );

    // FairPlay: we can't wrap a key like an iPhone would, but playfair turns
    // any ekey into some key, and our own FairPlay tells us which.
    let mut fp_first = [0u8; 16];
    fp_first[4] = 3;
    fp_first[14] = 1;
    let (code, reply) = sender.request("POST", "/fp-setup", None, &fp_first).await;
    assert_eq!((code, reply.len()), (200, 142));
    let mut fp_second = [5u8; 164];
    fp_second[4] = 3;
    fp_second[12] = 1;
    let (code, reply) = sender.request("POST", "/fp-setup", None, &fp_second).await;
    assert_eq!((code, reply.len()), (200, 32));
    let ekey = [9u8; 72];
    let mut ours = FairPlay::default();
    ours.setup(&fp_second).unwrap();
    let raw_key = ours.decrypt(&ekey).unwrap();
    let session_key: [u8; 16] = Sha512::new()
        .chain_update(raw_key)
        .chain_update(shared)
        .finalize()[..16]
        .try_into()
        .unwrap();

    // SETUP 1: keys and timing.
    let timing = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let reply = sender
        .plist(
            "SETUP",
            "rtsp://127.0.0.1/1",
            dict(vec![
                ("ekey", Value::Data(ekey.to_vec())),
                ("eiv", Value::Data(vec![3; 16])),
                ("name", "Test iPhone".into()),
                ("model", "iPhone15,2".into()),
                ("timingProtocol", "NTP".into()),
                (
                    "timingPort",
                    Value::Integer(u64::from(timing.local_addr().unwrap().port()).into()),
                ),
            ]),
        )
        .await;
    assert!(
        reply
            .get("timingPort")
            .unwrap()
            .as_unsigned_integer()
            .unwrap()
            > 0
    );
    let mut ntp = [0u8; 64];
    let (len, _) = tokio::time::timeout(Duration::from_secs(2), timing.recv_from(&mut ntp))
        .await
        .expect("a timing request")
        .unwrap();
    assert_eq!((len, &ntp[..2]), (32, &[0x80, 0xd2][..]));

    // SETUP 2: the picture.
    let reply = sender
        .plist(
            "SETUP",
            "rtsp://127.0.0.1/1",
            dict(vec![(
                "streams",
                Value::Array(vec![dict(vec![
                    ("type", Value::Integer(110.into())),
                    ("streamConnectionID", Value::Integer((-42i64).into())),
                ])]),
            )]),
        )
        .await;
    let streams = reply.get("streams").unwrap().as_array().unwrap();
    let data_port = streams[0]
        .as_dictionary()
        .unwrap()
        .get("dataPort")
        .unwrap()
        .as_unsigned_integer()
        .unwrap();
    let mut seen = 0;
    let StreamEvent::Started(info) = next_event(&events, &mut seen, "start").await else {
        panic!("expected Started");
    };
    assert_eq!(
        (info.kind, info.name.as_str()),
        (StreamKind::Airplay, "Test iPhone")
    );
    assert_eq!(receiver.streams().len(), 1);

    // One codec packet, one encrypted IDR frame.
    let mut mirror = TcpStream::connect(("127.0.0.1", data_port as u16))
        .await
        .unwrap();
    let parameters = [
        1u8, 0x64, 0, 0x28, 0xff, 0xe1, 0, 2, 0x67, 0x64, 1, 0, 1, 0x68,
    ];
    let mut header = [0u8; 128];
    header[..4].copy_from_slice(&(parameters.len() as u32).to_le_bytes());
    header[4] = 1;
    header[56..60].copy_from_slice(&1170f32.to_le_bytes());
    header[60..64].copy_from_slice(&2532f32.to_le_bytes());
    mirror.write_all(&header).await.unwrap();
    mirror.write_all(&parameters).await.unwrap();
    let mut frame = vec![0u8, 0, 0, 3, 0x65, 0x88, 0x84];
    stream_cipher(&session_key, (-42i64) as u64).apply_keystream(&mut frame);
    let mut header = [0u8; 128];
    header[..4].copy_from_slice(&(frame.len() as u32).to_le_bytes());
    header[8..16].copy_from_slice(&((5u64 << 32) | (1 << 31)).to_le_bytes());
    mirror.write_all(&header).await.unwrap();
    mirror.write_all(&frame).await.unwrap();

    let StreamEvent::VideoSize { width, height, .. } = next_event(&events, &mut seen, "size").await
    else {
        panic!("expected VideoSize");
    };
    assert_eq!((width, height), (1170, 2532));
    let StreamEvent::Video { packet, .. } = next_event(&events, &mut seen, "frame").await else {
        panic!("expected Video");
    };
    assert!(packet.key);
    assert_eq!(packet.pts_us, 5_500_000);
    assert_eq!(
        packet.data,
        vec![0, 0, 0, 1, 0x67, 0x64, 0, 0, 0, 1, 0x68, 0, 0, 0, 1, 0x65, 0x88, 0x84]
    );

    // SETUP 3: sound (switched off in this receiver).
    let reply = sender
        .plist(
            "SETUP",
            "rtsp://127.0.0.1/1",
            dict(vec![(
                "streams",
                Value::Array(vec![dict(vec![
                    ("type", Value::Integer(96.into())),
                    ("ct", Value::Integer(8.into())),
                    ("spf", Value::Integer(480.into())),
                    ("controlPort", Value::Integer(6001.into())),
                ])]),
            )]),
        )
        .await;
    let audio = reply.get("streams").unwrap().as_array().unwrap()[0]
        .as_dictionary()
        .unwrap()
        .clone();
    assert!(
        audio
            .get("dataPort")
            .unwrap()
            .as_unsigned_integer()
            .unwrap()
            > 0
    );
    assert!(
        audio
            .get("controlPort")
            .unwrap()
            .as_unsigned_integer()
            .unwrap()
            > 0
    );
    let StreamEvent::Audio { status, .. } = next_event(&events, &mut seen, "audio").await else {
        panic!("expected Audio");
    };
    assert_eq!(status, AudioStatus::Off);

    let (code, _) = sender
        .request(
            "SET_PARAMETER",
            "rtsp://127.0.0.1/1",
            Some("text/parameters"),
            b"volume: -11.5\r\n",
        )
        .await;
    assert_eq!(code, 200);
    let (code, body) = sender
        .request(
            "GET_PARAMETER",
            "rtsp://127.0.0.1/1",
            Some("text/parameters"),
            b"volume\r\n",
        )
        .await;
    assert_eq!(
        (code, String::from_utf8(body).unwrap()),
        (200, "volume: -11.500000\r\n".into())
    );

    // The app ends it.
    assert!(receiver.end_stream(info.id));
    let StreamEvent::Ended { id, .. } = next_event(&events, &mut seen, "end").await else {
        panic!("expected Ended");
    };
    assert_eq!(id, info.id);
    assert!(receiver.streams().is_empty());
    drop(receiver);
    std::fs::remove_dir_all(dir).ok();
}

/// A receiver of its own for one test, with what it tells the app.
struct TestReceiver {
    receiver: Receiver,
    pairing: Arc<Mutex<Vec<PairingEvent>>>,
    dir: std::path::PathBuf,
}

impl TestReceiver {
    /// `None` where multicast is missing (some CI machines).
    async fn start(tag: &str) -> Option<Self> {
        let dir = std::env::temp_dir().join(format!("uwumirror-{tag}-{}", std::process::id()));
        let pairing = Arc::new(Mutex::new(Vec::new()));
        let on_pairing: PairingSink = {
            let pairing = pairing.clone();
            Arc::new(move |event| pairing.lock().push(event))
        };
        let config = ReceiverConfig {
            name: format!("UwUMirror {tag}"),
            width: 1280,
            height: 720,
            fps: 30,
            audio: false,
            identity_path: dir.join("airplay-identity"),
            trusted_path: dir.join("airplay-trusted"),
        };
        match Receiver::start(config, Arc::new(|_| {}), on_pairing).await {
            Ok(receiver) => Some(Self {
                receiver,
                pairing,
                dir,
            }),
            Err(crate::ReceiverError::Announce(error)) => {
                eprintln!("no multicast here ({error}), skipping");
                None
            }
            Err(error) => panic!("{error}"),
        }
    }

    async fn connect(&self) -> Sender {
        let socket = TcpStream::connect(("127.0.0.1", self.receiver.port()))
            .await
            .unwrap();
        Sender {
            stream: BufReader::new(socket),
            cseq: 0,
        }
    }

    fn pairing_events(&self) -> Vec<PairingEvent> {
        self.pairing.lock().clone()
    }
}

impl Drop for TestReceiver {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

impl Sender {
    /// A `pair-setup-pin` step: the status, and the reply's plist on success.
    async fn pin_step(&mut self, body: Dictionary) -> (u16, Option<Dictionary>) {
        let mut bytes = Vec::new();
        plist::to_writer_binary(&mut bytes, &Value::Dictionary(body)).unwrap();
        let (code, reply) = self
            .request(
                "POST",
                "/pair-setup-pin",
                Some("application/x-apple-binary-plist"),
                &bytes,
            )
            .await;
        let reply = (code == 200).then(|| {
            Value::from_reader(std::io::Cursor::new(reply))
                .unwrap()
                .into_dictionary()
                .unwrap()
        });
        (code, reply)
    }

    /// Both pair-verify steps with `signing` as the lasting key. Returns the
    /// first step's status (the second only runs after a 200), and the
    /// shared secret.
    async fn pair_verify(&mut self, signing: &SigningKey, seed: u8) -> (u16, Option<[u8; 32]>) {
        let secret = StaticSecret::from([seed; 32]);
        let public = PublicKey::from(&secret);
        let mut first = vec![1, 0, 0, 0];
        first.extend_from_slice(public.as_bytes());
        first.extend_from_slice(&signing.verifying_key().to_bytes());
        let (code, reply) = self.request("POST", "/pair-verify", None, &first).await;
        if code != 200 {
            return (code, None);
        }
        let theirs: [u8; 32] = reply[..32].try_into().unwrap();
        let shared = *secret.diffie_hellman(&PublicKey::from(theirs)).as_bytes();
        let derive = |salt: &[u8]| -> [u8; 16] {
            Sha512::new()
                .chain_update(salt)
                .chain_update(shared)
                .finalize()[..16]
                .try_into()
                .unwrap()
        };
        use aes::cipher::KeyIvInit;
        let mut ctr = ctr::Ctr128BE::<aes::Aes128>::new(
            &derive(b"Pair-Verify-AES-Key").into(),
            &derive(b"Pair-Verify-AES-IV").into(),
        );
        let mut skip = [0u8; 64];
        ctr.apply_keystream(&mut skip);
        let mut message = public.as_bytes().to_vec();
        message.extend_from_slice(&theirs);
        let mut signature = signing.sign(&message).to_bytes();
        ctr.apply_keystream(&mut signature);
        let mut second = vec![0, 0, 0, 0];
        second.extend_from_slice(&signature);
        let (code, _) = self.request("POST", "/pair-verify", None, &second).await;
        assert_eq!(code, 200, "the second pair-verify step");
        (200, Some(shared))
    }
}

/// The PIN the receiver last showed.
fn shown_pin(events: &[PairingEvent]) -> String {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            PairingEvent::PinRequested { pin, .. } => Some(pin.clone()),
            _ => None,
        })
        .expect("a PIN on screen")
}

/// A Mac on macOS Sequoia: asks for a PIN, pairs with SRP, then pair-verify,
/// FairPlay and SETUP as usual. Next time it skips straight to pair-verify,
/// and is let in — until the app forgets it.
#[tokio::test(flavor = "multi_thread")]
async fn a_mac_pairs_with_a_pin() {
    let Some(test) = TestReceiver::start("pin-e2e").await else {
        return;
    };
    let mut sender = test.connect().await;
    let (code, info) = sender.request("GET", "/info", None, &[]).await;
    assert_eq!(code, 200);
    let info = Value::from_reader(std::io::Cursor::new(info)).unwrap();
    let receiver_pk = info
        .as_dictionary()
        .unwrap()
        .get("pk")
        .unwrap()
        .as_data()
        .unwrap()
        .to_vec();

    let (code, _) = sender.request("POST", "/pair-pin-start", None, &[]).await;
    assert_eq!(code, 200);
    let pin = shown_pin(&test.pairing_events());
    assert!(pin.len() == 4 && pin.chars().all(|c| c.is_ascii_digit()));

    let signing = SigningKey::from_bytes(&[31u8; 32]);
    let mut mac = Mac::new("4C:32:75:9B:2E:F1", signing.verifying_key().to_bytes());
    let (code, first) = sender.pin_step(mac.first()).await;
    assert_eq!(code, 200);
    let first = first.unwrap();
    assert_eq!(first.get("salt").unwrap().as_data().unwrap().len(), 16);
    assert_eq!(first.get("pk").unwrap().as_data().unwrap().len(), 256);
    let (code, second) = sender.pin_step(mac.second(&first, &pin)).await;
    assert_eq!(code, 200);
    let (code, third) = sender.pin_step(mac.third(&second.unwrap())).await;
    assert_eq!(code, 200);
    assert_eq!(mac.receiver_key(&third.unwrap()).to_vec(), receiver_pk);
    assert!(test.pairing_events().contains(&PairingEvent::Paired {
        address: "127.0.0.1".into(),
        device: "4C:32:75:9B:2E:F1".into(),
    }));
    assert_eq!(test.receiver.trusted_devices(), 1);

    // Pair-verify, FairPlay and the first SETUP, as after `/pair-setup`.
    let (code, shared) = sender.pair_verify(&signing, 32).await;
    assert_eq!(code, 200);
    assert!(shared.is_some());
    let mut fp_first = [0u8; 16];
    fp_first[4] = 3;
    fp_first[14] = 1;
    let (code, reply) = sender.request("POST", "/fp-setup", None, &fp_first).await;
    assert_eq!((code, reply.len()), (200, 142));
    let mut fp_second = [5u8; 164];
    fp_second[4] = 3;
    fp_second[12] = 1;
    let (code, reply) = sender.request("POST", "/fp-setup", None, &fp_second).await;
    assert_eq!((code, reply.len()), (200, 32));
    sender
        .plist(
            "SETUP",
            "rtsp://127.0.0.1/1",
            dict(vec![
                ("ekey", Value::Data(vec![9u8; 72])),
                ("eiv", Value::Data(vec![3; 16])),
                ("name", "Test Mac".into()),
                ("model", "MacBookPro16,1".into()),
            ]),
        )
        .await;
    drop(sender);

    // Next time: straight to pair-verify, no PIN.
    let mut again = test.connect().await;
    assert_eq!(again.pair_verify(&signing, 33).await.0, 200);
    // An iPhone that knows the receiver comes straight to pair-verify too,
    // and needs no PIN: one is only for senders that ask for it.
    let mut iphone = test.connect().await;
    let other = SigningKey::from_bytes(&[34u8; 32]);
    assert_eq!(iphone.pair_verify(&other, 35).await.0, 200);
    // So does one doing pair-setup first.
    let mut fresh = test.connect().await;
    let (code, _) = fresh
        .request(
            "POST",
            "/pair-setup",
            None,
            &other.verifying_key().to_bytes(),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(fresh.pair_verify(&other, 36).await.0, 200);

    // Forgetting empties the list.
    test.receiver.forget_trusted_devices().unwrap();
    assert_eq!(test.receiver.trusted_devices(), 0);
}

/// A wrong PIN: refused, the PIN used up, and no way around it.
#[tokio::test(flavor = "multi_thread")]
async fn a_mac_with_the_wrong_pin_is_refused() {
    let Some(test) = TestReceiver::start("pin-wrong").await else {
        return;
    };
    let mut sender = test.connect().await;
    let (code, _) = sender.request("POST", "/pair-pin-start", None, &[]).await;
    assert_eq!(code, 200);
    let pin = shown_pin(&test.pairing_events());
    let wrong = format!("{:04}", (pin.parse::<u32>().unwrap() + 5000) % 10_000);

    let signing = SigningKey::from_bytes(&[41u8; 32]);
    let mut mac = Mac::new("4C:32:75:9B:2E:F2", signing.verifying_key().to_bytes());
    let (code, first) = sender.pin_step(mac.first()).await;
    assert_eq!(code, 200);
    let (code, _) = sender.pin_step(mac.second(&first.unwrap(), &wrong)).await;
    assert_eq!(code, 470);
    assert!(test.pairing_events().contains(&PairingEvent::Failed {
        address: "127.0.0.1".into(),
        reason: PairingFailure::WrongPin,
    }));

    // Going on to step 3 anyway gets nowhere…
    let mut fake = Dictionary::new();
    fake.insert("epk".into(), Value::Data(vec![0; 32]));
    fake.insert("authTag".into(), Value::Data(vec![0; 16]));
    assert_eq!(sender.pin_step(fake).await.0, 470);
    // …nor does trying the right PIN without a new one on screen.
    assert_eq!(sender.pin_step(mac.first()).await.0, 470);
    assert_eq!(test.receiver.trusted_devices(), 0);
}

/// Plays an iPhone against a receiver that is already running — the real app
/// — and mirrors an H.264 file to it, 30 frames a second:
///
/// ```sh
/// UWUMIRROR_PORT=7000 UWUMIRROR_VIDEO=clip.h264 \
///   cargo test -p uwumirror-airplay -- --ignored --nocapture mirror_a_file
/// ```
///
/// The file must be Annex B with access unit delimiters (x264's `aud=1`), so
/// frames can be told apart without parsing slices.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a running receiver and a video file"]
async fn mirror_a_file() {
    let port: u16 = std::env::var("UWUMIRROR_PORT").map_or(7000, |p| p.parse().unwrap());
    let video = std::fs::read(std::env::var("UWUMIRROR_VIDEO").expect("UWUMIRROR_VIDEO")).unwrap();
    let seconds: u64 = std::env::var("UWUMIRROR_SECONDS").map_or(20, |s| s.parse().unwrap());

    let socket = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut sender = Sender {
        stream: BufReader::new(socket),
        cseq: 0,
    };
    let signing = SigningKey::from_bytes(&[21u8; 32]);
    sender
        .request(
            "POST",
            "/pair-setup",
            None,
            &signing.verifying_key().to_bytes(),
        )
        .await;
    let secret = StaticSecret::from([22u8; 32]);
    let public = PublicKey::from(&secret);
    let mut first = vec![1, 0, 0, 0];
    first.extend_from_slice(public.as_bytes());
    first.extend_from_slice(&signing.verifying_key().to_bytes());
    let (_, reply) = sender.request("POST", "/pair-verify", None, &first).await;
    let theirs: [u8; 32] = reply[..32].try_into().unwrap();
    let shared = *secret.diffie_hellman(&PublicKey::from(theirs)).as_bytes();
    let derive = |salt: &[u8]| -> [u8; 16] {
        Sha512::new()
            .chain_update(salt)
            .chain_update(shared)
            .finalize()[..16]
            .try_into()
            .unwrap()
    };
    use aes::cipher::KeyIvInit;
    let mut ctr = ctr::Ctr128BE::<aes::Aes128>::new(
        &derive(b"Pair-Verify-AES-Key").into(),
        &derive(b"Pair-Verify-AES-IV").into(),
    );
    let mut skip = [0u8; 64];
    ctr.apply_keystream(&mut skip);
    let mut message = public.as_bytes().to_vec();
    message.extend_from_slice(&theirs);
    let mut signature = signing.sign(&message).to_bytes();
    ctr.apply_keystream(&mut signature);
    let mut second = vec![0, 0, 0, 0];
    second.extend_from_slice(&signature);
    sender.request("POST", "/pair-verify", None, &second).await;
    let mut fp_first = [0u8; 16];
    fp_first[4] = 3;
    fp_first[14] = 0;
    sender.request("POST", "/fp-setup", None, &fp_first).await;
    let mut fp_second = [6u8; 164];
    fp_second[4] = 3;
    fp_second[12] = 0;
    sender.request("POST", "/fp-setup", None, &fp_second).await;
    let ekey = [8u8; 72];
    let mut ours = FairPlay::default();
    ours.setup(&fp_second).unwrap();
    let raw_key = ours.decrypt(&ekey).unwrap();
    let session_key: [u8; 16] = Sha512::new()
        .chain_update(raw_key)
        .chain_update(shared)
        .finalize()[..16]
        .try_into()
        .unwrap();
    sender
        .plist(
            "SETUP",
            "rtsp://127.0.0.1/2",
            dict(vec![
                ("ekey", Value::Data(ekey.to_vec())),
                ("eiv", Value::Data(vec![1; 16])),
                ("name", "Nyus iPhone".into()),
                ("model", "iPhone16,2".into()),
            ]),
        )
        .await;
    let reply = sender
        .plist(
            "SETUP",
            "rtsp://127.0.0.1/2",
            dict(vec![(
                "streams",
                Value::Array(vec![dict(vec![
                    ("type", Value::Integer(110.into())),
                    ("streamConnectionID", Value::Integer(4242.into())),
                ])]),
            )]),
        )
        .await;
    let data_port = reply.get("streams").unwrap().as_array().unwrap()[0]
        .as_dictionary()
        .unwrap()
        .get("dataPort")
        .unwrap()
        .as_unsigned_integer()
        .unwrap();
    let mut mirror = TcpStream::connect(("127.0.0.1", data_port as u16))
        .await
        .unwrap();
    let mut cipher = stream_cipher(&session_key, 4242);

    // Access units, split at the delimiters.
    let mut units: Vec<Vec<Vec<u8>>> = Vec::new();
    let mut at = 0;
    let mut starts = Vec::new();
    while at + 3 < video.len() {
        if video[at..at + 3] == [0, 0, 1] {
            starts.push(at + 3);
            at += 3;
        } else {
            at += 1;
        }
    }
    for (index, &start) in starts.iter().enumerate() {
        let mut end = starts.get(index + 1).map_or(video.len(), |next| next - 3);
        while end > start && video[end - 1] == 0 {
            end -= 1;
        }
        let nal = video[start..end].to_vec();
        if nal[0] & 0x1f == 9 || units.is_empty() {
            units.push(Vec::new());
        }
        if nal[0] & 0x1f != 9 {
            units.last_mut().unwrap().push(nal);
        }
    }
    let started = std::time::Instant::now();
    // Round and round until the time is up.
    for (index, unit) in units.iter().cycle().enumerate() {
        if started.elapsed().as_secs() >= seconds {
            break;
        }
        let ntp = ((index as u64 / 30) << 32) | (((index as u64 % 30) << 32) / 30);
        let sps = unit.iter().find(|n| n[0] & 0x1f == 7);
        let pps = unit.iter().find(|n| n[0] & 0x1f == 8);
        if let (Some(sps), Some(pps)) = (sps, pps) {
            let mut payload = vec![1, sps[1], sps[2], sps[3], 0xff, 0xe1];
            payload.extend_from_slice(&(sps.len() as u16).to_be_bytes());
            payload.extend_from_slice(sps);
            payload.push(1);
            payload.extend_from_slice(&(pps.len() as u16).to_be_bytes());
            payload.extend_from_slice(pps);
            let mut header = [0u8; 128];
            header[..4].copy_from_slice(&(payload.len() as u32).to_le_bytes());
            header[4] = 1;
            header[6] = 0x16;
            header[8..16].copy_from_slice(&ntp.to_le_bytes());
            header[56..60].copy_from_slice(&1170f32.to_le_bytes());
            header[60..64].copy_from_slice(&2532f32.to_le_bytes());
            mirror.write_all(&header).await.unwrap();
            mirror.write_all(&payload).await.unwrap();
        }
        let mut frame = Vec::new();
        for nal in unit.iter().filter(|n| !matches!(n[0] & 0x1f, 7 | 8)) {
            frame.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            frame.extend_from_slice(nal);
        }
        cipher.apply_keystream(&mut frame);
        let mut header = [0u8; 128];
        header[..4].copy_from_slice(&(frame.len() as u32).to_le_bytes());
        header[8..16].copy_from_slice(&ntp.to_le_bytes());
        mirror.write_all(&header).await.unwrap();
        mirror.write_all(&frame).await.unwrap();
        tokio::time::sleep(Duration::from_millis(33)).await;
        if index % 30 == 0 {
            // The heartbeat every sender sends.
            sender.request("POST", "/feedback", None, &[]).await;
        }
    }
    sender
        .request("TEARDOWN", "rtsp://127.0.0.1/2", None, &[])
        .await;
}
