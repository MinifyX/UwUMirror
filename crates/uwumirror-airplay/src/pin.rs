//! PIN pairing: how a Mac that insists on it gets to mirror.
//!
//! iPhones pair without asking anything (see `pairing.rs`). Macs on recent
//! macOS — and devices an organisation manages — first want the user to type
//! a PIN the receiver shows. They ask with `POST /pair-pin-start`; the
//! receiver makes up four digits and puts them on screen. Then come three
//! `POST /pair-setup-pin`, each a binary plist:
//!
//! 1. `{method: "pin", user: <the Mac's id>}` — we answer with an SRP salt
//!    and our public key `B` (`{salt, pk}`), the PIN being the password
//!    (see `srp.rs` for Apple's flavour of SRP-6a).
//! 2. `{pk: A, proof: M1}` — the Mac's proof that it knows the PIN. With the
//!    right PIN we answer `{proof: M2}`; with a wrong one, 470.
//! 3. `{epk, authTag}` — the Mac's lasting Ed25519 key, sealed with
//!    AES-128-GCM under a key from the SRP session key `K`:
//!    `key = SHA-512("Pair-Setup-AES-Key" | K)[..16]`,
//!    `iv = SHA-512("Pair-Setup-AES-IV" | K)[..16]`, the IV's last byte plus
//!    one for the Mac's message and plus two for ours (a 16-byte GCM nonce,
//!    no associated data). We answer with our own key, sealed the same way.
//!
//! Pair-verify follows as usual, and from then on the Mac knows the receiver:
//! next time it goes straight to pair-verify. We remember its key, too
//! ([`TrustedDevices`]), so that a returning Mac is let in and anyone else
//! skipping straight to pair-verify is asked to pair first.
//!
//! A PIN is good for one try and one minute. After five wrong ones in a row,
//! no new PIN is shown for a minute: four digits must not be guessable by a
//! script that simply keeps asking.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aes_gcm::aead::generic_array::typenum::U16;
use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::AesGcm;
use plist::{Dictionary, Value};
use rand::rngs::OsRng;
use rand::{Rng, RngCore};
use serde::Serialize;
use sha2::{Digest, Sha512};

use crate::pairing::Identity;
use crate::srp::{self, Group, SESSION_KEY_LEN};

/// AES-128-GCM with Apple's 16-byte nonce.
type Aes128Gcm16 = AesGcm<aes::Aes128, U16>;

/// How long a PIN on screen stays good.
pub const PIN_LIFETIME: Duration = Duration::from_secs(60);
/// Wrong PINs in a row before the receiver stops showing new ones…
const MAX_FAILURES: u32 = 5;
/// …for this long.
const LOCKOUT: Duration = Duration::from_secs(60);
/// The longest device id we accept as SRP user name; Macs send a MAC address.
const MAX_USER: usize = 64;
/// More trusted devices than anyone owns; the oldest go first.
const MAX_TRUSTED: usize = 64;
const MAX_TRUSTED_FILE: u64 = 16 * 1024;

/// What the app hears about PIN pairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PairingEvent {
    /// A device asks for a PIN: show `pin` until it pairs, fails or a minute
    /// passes.
    PinRequested { pin: String, address: String },
    /// The device typed the right PIN and is trusted from now on.
    Paired { address: String, device: String },
    Failed {
        address: String,
        reason: PairingFailure,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PairingFailure {
    WrongPin,
    /// No PIN was showing, or it had run out.
    Expired,
    /// Too many wrong PINs; no new one for a while.
    Locked,
    /// The device broke off or sent something that doesn't add up.
    Broken,
}

/// Where the receiver tells the app about PIN pairing.
pub type PairingSink = Arc<dyn Fn(PairingEvent) + Send + Sync>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PinError {
    #[error("pair-setup-pin message is malformed")]
    Malformed,
    #[error("pair-setup-pin steps came out of order")]
    OutOfOrder,
    #[error("no PIN is showing")]
    NoPin,
    #[error("too many wrong PINs")]
    Locked,
    #[error("wrong PIN")]
    WrongPin,
    #[error("the device's sealed key doesn't open")]
    BadSeal,
}

impl PinError {
    pub fn failure(&self) -> PairingFailure {
        match self {
            PinError::NoPin => PairingFailure::Expired,
            PinError::Locked => PairingFailure::Locked,
            PinError::WrongPin => PairingFailure::WrongPin,
            PinError::Malformed | PinError::OutOfOrder | PinError::BadSeal => {
                PairingFailure::Broken
            }
        }
    }
}

/// The receiver's PIN: at most one showing, and the count of wrong ones.
#[derive(Default)]
pub struct PinGate {
    pending: Option<(String, Instant)>,
    failures: u32,
    locked_until: Option<Instant>,
}

impl PinGate {
    /// Makes up a new PIN for `POST /pair-pin-start`, replacing any before.
    pub fn start(&mut self, now: Instant) -> Result<String, PinError> {
        if self.locked_until.is_some_and(|until| now < until) {
            return Err(PinError::Locked);
        }
        self.locked_until = None;
        let pin = format!("{:04}", OsRng.gen_range(0..10_000u32));
        self.pending = Some((pin.clone(), now));
        Ok(pin)
    }

    /// Takes the PIN showing, if it hasn't run out: each one gets one try.
    fn take(&mut self, now: Instant) -> Result<String, PinError> {
        match self.pending.take() {
            Some((pin, since)) if now.duration_since(since) <= PIN_LIFETIME => Ok(pin),
            _ => Err(PinError::NoPin),
        }
    }

    fn failed(&mut self, now: Instant) {
        self.pending = None;
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            self.failures = 0;
            self.locked_until = Some(now + LOCKOUT);
        }
    }

    fn succeeded(&mut self) {
        self.failures = 0;
    }
}

fn derive(salt: &[u8], key: &[u8; SESSION_KEY_LEN]) -> [u8; 16] {
    let hash = Sha512::new()
        .chain_update(salt)
        .chain_update(key)
        .finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash[..16]);
    out
}

/// The AES key and the IV before its last byte is counted up.
fn seal_keys(key: &[u8; SESSION_KEY_LEN]) -> (Aes128Gcm16, [u8; 16]) {
    let cipher = Aes128Gcm16::new(&derive(b"Pair-Setup-AES-Key", key).into());
    (cipher, derive(b"Pair-Setup-AES-IV", key))
}

fn nonce(iv: &[u8; 16], step: u8) -> GenericArray<u8, U16> {
    let mut nonce = *iv;
    nonce[15] = nonce[15].wrapping_add(step);
    nonce.into()
}

/// Seals our key for the device (step 3's answer), as `(epk, authTag)`.
fn seal(key: &[u8; SESSION_KEY_LEN], public: [u8; 32]) -> ([u8; 32], [u8; 16]) {
    let (cipher, iv) = seal_keys(key);
    let mut sealed = public;
    let tag = cipher
        .encrypt_in_place_detached(&nonce(&iv, 2), b"", &mut sealed)
        .expect("32 bytes fit AES-GCM");
    (sealed, tag.into())
}

/// Opens the device's sealed key (step 3).
fn open(key: &[u8; SESSION_KEY_LEN], sealed: &[u8], tag: &[u8]) -> Result<[u8; 32], PinError> {
    let (Ok(sealed), Ok(tag)) = (<[u8; 32]>::try_from(sealed), <[u8; 16]>::try_from(tag)) else {
        return Err(PinError::Malformed);
    };
    let (cipher, iv) = seal_keys(key);
    let mut public = sealed;
    cipher
        .decrypt_in_place_detached(&nonce(&iv, 1), b"", &mut public, &tag.into())
        .map_err(|_| PinError::BadSeal)?;
    Ok(public)
}

enum Step {
    Idle,
    Challenged {
        user: String,
        server: Box<srp::Server>,
    },
    Proven {
        user: String,
        key: [u8; SESSION_KEY_LEN],
    },
}

/// What a `pair-setup-pin` step comes to.
pub enum Outcome {
    /// Answer with this and wait for the next step.
    Reply(Dictionary),
    /// Paired: answer with `reply`; the device's lasting key is `key`.
    Paired {
        reply: Dictionary,
        user: String,
        key: [u8; 32],
    },
}

/// One connection's PIN pairing.
pub struct PinSetup {
    step: Step,
}

impl Default for PinSetup {
    fn default() -> Self {
        Self { step: Step::Idle }
    }
}

fn data<'a>(dict: &'a Dictionary, key: &str) -> Option<&'a [u8]> {
    dict.get(key).and_then(Value::as_data)
}

impl PinSetup {
    /// Handles one `POST /pair-setup-pin` body.
    pub fn step(
        &mut self,
        gate: &parking_lot::Mutex<PinGate>,
        identity: &Identity,
        body: &Dictionary,
        now: Instant,
    ) -> Result<Outcome, PinError> {
        if let (Some(method), Some(user)) = (
            body.get("method").and_then(Value::as_string),
            body.get("user").and_then(Value::as_string),
        ) {
            self.step = Step::Idle;
            if method != "pin" || user.is_empty() || user.len() > MAX_USER {
                return Err(PinError::Malformed);
            }
            let pin = gate.lock().take(now)?;
            return Ok(Outcome::Reply(self.challenge(user, &pin)));
        }
        if let (Some(a), Some(proof)) = (data(body, "pk"), data(body, "proof")) {
            let Step::Challenged { user, server } = std::mem::replace(&mut self.step, Step::Idle)
            else {
                return Err(PinError::OutOfOrder);
            };
            let session = match server.verify(a, proof) {
                Ok(session) => session,
                Err(error) => {
                    gate.lock().failed(now);
                    return Err(match error {
                        srp::SrpError::WrongProof => PinError::WrongPin,
                        srp::SrpError::BadPublicKey => PinError::Malformed,
                    });
                }
            };
            self.step = Step::Proven {
                user,
                key: session.key,
            };
            let mut reply = Dictionary::new();
            reply.insert("proof".into(), Value::Data(session.proof.to_vec()));
            return Ok(Outcome::Reply(reply));
        }
        if let (Some(sealed), Some(tag)) = (data(body, "epk"), data(body, "authTag")) {
            let Step::Proven { user, key } = std::mem::replace(&mut self.step, Step::Idle) else {
                return Err(PinError::OutOfOrder);
            };
            let theirs = match open(&key, sealed, tag) {
                Ok(theirs) => theirs,
                Err(error) => {
                    gate.lock().failed(now);
                    return Err(error);
                }
            };
            gate.lock().succeeded();
            let (epk, auth_tag) = seal(&key, identity.public_key());
            let mut reply = Dictionary::new();
            reply.insert("epk".into(), Value::Data(epk.to_vec()));
            reply.insert("authTag".into(), Value::Data(auth_tag.to_vec()));
            return Ok(Outcome::Paired {
                reply,
                user,
                key: theirs,
            });
        }
        Err(PinError::Malformed)
    }

    /// Step 1's answer: a fresh salt and `B`.
    fn challenge(&mut self, user: &str, pin: &str) -> Dictionary {
        let mut salt = [0u8; 16];
        let server = loop {
            OsRng.fill_bytes(&mut salt);
            // No leading zero: see `srp.rs` on how numbers are spelled.
            salt[0] |= 0x80;
            let mut b = [0u8; 32];
            OsRng.fill_bytes(&mut b);
            if let Some(server) = srp::Server::new(Group::rfc5054_2048(), user, pin, &salt, &b) {
                break server;
            }
        };
        let mut reply = Dictionary::new();
        reply.insert("pk".into(), Value::Data(server.public_key()));
        reply.insert("salt".into(), Value::Data(salt.to_vec()));
        self.step = Step::Challenged {
            user: user.to_owned(),
            server: Box::new(server),
        };
        reply
    }
}

/// A device that paired with a PIN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedDevice {
    /// Its lasting Ed25519 key, the one it signs pair-verify with.
    pub key: [u8; 32],
    /// The id it gave as SRP user name.
    pub id: String,
}

/// The devices that paired with a PIN, kept in a small text file (one
/// `<key in hex> <id>` per line) next to the receiver's identity.
pub struct TrustedDevices {
    path: PathBuf,
    devices: Vec<TrustedDevice>,
}

fn parse_hex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// An id as it may go into the file: printable, no spaces, not too long.
fn clean_id(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_graphic())
        .take(MAX_USER)
        .collect()
}

impl TrustedDevices {
    /// Reads the list from `path`; a missing or unreadable file is an empty
    /// list (the devices in it then just pair again).
    pub fn load(path: &Path) -> Self {
        let mut text = String::new();
        if let Ok(file) = std::fs::File::open(path) {
            if file
                .take(MAX_TRUSTED_FILE)
                .read_to_string(&mut text)
                .is_err()
            {
                tracing::warn!(path = %path.display(), "trusted AirPlay devices unreadable");
                text.clear();
            }
        }
        let devices = text
            .lines()
            .filter_map(|line| {
                let (key, id) = line.trim().split_once(' ')?;
                Some(TrustedDevice {
                    key: parse_hex32(key)?,
                    id: clean_id(id),
                })
            })
            .take(MAX_TRUSTED)
            .collect();
        Self {
            path: path.to_owned(),
            devices,
        }
    }

    pub fn contains(&self, key: &[u8; 32]) -> bool {
        self.devices.iter().any(|device| &device.key == key)
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    /// Trusts a device from now on (again, if it paired before) and saves the
    /// list.
    pub fn add(&mut self, key: [u8; 32], id: &str) -> std::io::Result<()> {
        let id = clean_id(id);
        self.devices
            .retain(|device| device.key != key && (id.is_empty() || device.id != id));
        self.devices.push(TrustedDevice { key, id });
        if self.devices.len() > MAX_TRUSTED {
            let extra = self.devices.len() - MAX_TRUSTED;
            self.devices.drain(..extra);
        }
        self.save()
    }

    /// Forgets every device: each must enter a PIN again.
    pub fn forget(&mut self) -> std::io::Result<()> {
        self.devices.clear();
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text: String = self
            .devices
            .iter()
            .map(|device| {
                let key: String = device.key.iter().map(|b| format!("{b:02x}")).collect();
                format!("{key} {}\n", device.id)
            })
            .collect();
        std::fs::write(&self.path, text)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::srp::Client;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Step 3's sealing, against Node's AES-GCM (with the SRP key from
    /// `srp.rs`'s Apple vectors).
    #[test]
    fn sealing_matches_an_independent_aes_gcm() {
        let key: [u8; 40] =
            hex("3f7ffe70e210bcb00d2888d4df946c230cb622191e6647d197b0a8e671322d91bcd796111c08e22a")
                .try_into()
                .unwrap();
        let theirs = open(
            &key,
            &hex("0b6aed1505e48c177e475a4835f0475b3be84187612273df9c90982e1b2e80c7"),
            &hex("24f4a0ed5e2631e9c576954cd841ab6d"),
        )
        .unwrap();
        assert_eq!(theirs, [0x33; 32]);
        let (epk, tag) = seal(&key, [0x44; 32]);
        assert_eq!(
            epk.to_vec(),
            hex("4f3b5102bcbad184297bfc8c35b93143ba7b65903f24da03647412f7768d5730")
        );
        assert_eq!(tag.to_vec(), hex("3d9b3dc6d48ecc10ae68a44f3b826c42"));
        assert_eq!(
            open(&key, &epk, &[0u8; 16]),
            Err(PinError::BadSeal),
            "a wrong tag doesn't open"
        );
    }

    /// The Mac's side of `pair-setup-pin`, all three steps, as plists.
    pub(crate) struct Mac {
        pub user: String,
        pub key: [u8; 32],
        session: Option<Client>,
    }

    impl Mac {
        pub fn new(user: &str, key: [u8; 32]) -> Self {
            Self {
                user: user.into(),
                key,
                session: None,
            }
        }

        pub fn first(&self) -> Dictionary {
            let mut d = Dictionary::new();
            d.insert("method".into(), "pin".into());
            d.insert("user".into(), self.user.clone().into());
            d
        }

        pub fn second(&mut self, reply: &Dictionary, pin: &str) -> Dictionary {
            let salt = reply.get("salt").unwrap().as_data().unwrap();
            let b = reply.get("pk").unwrap().as_data().unwrap();
            let mut a = [0u8; 32];
            OsRng.fill_bytes(&mut a);
            let client = Client::respond(&Group::rfc5054_2048(), &self.user, pin, salt, b, &a);
            let mut d = Dictionary::new();
            d.insert("pk".into(), Value::Data(client.public.clone()));
            d.insert("proof".into(), Value::Data(client.proof.to_vec()));
            self.session = Some(client);
            d
        }

        /// Checks `M2` and seals our key.
        pub fn third(&self, reply: &Dictionary) -> Dictionary {
            let client = self.session.as_ref().unwrap();
            assert_eq!(
                reply.get("proof").unwrap().as_data().unwrap(),
                client.expected,
                "the receiver proves it knew the PIN"
            );
            let (cipher, iv) = seal_keys(&client.key);
            let mut sealed = self.key;
            let tag = cipher
                .encrypt_in_place_detached(&nonce(&iv, 1), b"", &mut sealed)
                .unwrap();
            let mut d = Dictionary::new();
            d.insert("epk".into(), Value::Data(sealed.to_vec()));
            d.insert("authTag".into(), Value::Data(tag.to_vec()));
            d
        }

        /// Opens the receiver's sealed key from step 3's answer.
        pub fn receiver_key(&self, reply: &Dictionary) -> [u8; 32] {
            let client = self.session.as_ref().unwrap();
            let (cipher, iv) = seal_keys(&client.key);
            let mut key: [u8; 32] = reply
                .get("epk")
                .unwrap()
                .as_data()
                .unwrap()
                .try_into()
                .unwrap();
            let tag: [u8; 16] = reply
                .get("authTag")
                .unwrap()
                .as_data()
                .unwrap()
                .try_into()
                .unwrap();
            cipher
                .decrypt_in_place_detached(&nonce(&iv, 2), b"", &mut key, &tag.into())
                .expect("the receiver's key opens");
            key
        }
    }

    fn reply(outcome: Outcome) -> Dictionary {
        match outcome {
            Outcome::Reply(reply) => reply,
            Outcome::Paired { .. } => panic!("paired too early"),
        }
    }

    #[test]
    fn the_right_pin_pairs() {
        let identity = Identity::generate();
        let gate = parking_lot::Mutex::new(PinGate::default());
        let now = Instant::now();
        let pin = gate.lock().start(now).unwrap();
        assert_eq!(pin.len(), 4);
        let mut mac = Mac::new("4C:32:75:9B:2E:F1", [0x33; 32]);
        let mut setup = PinSetup::default();
        let first = reply(setup.step(&gate, &identity, &mac.first(), now).unwrap());
        let second = mac.second(&first, &pin);
        let second = reply(setup.step(&gate, &identity, &second, now).unwrap());
        let third = mac.third(&second);
        let Outcome::Paired { reply, user, key } =
            setup.step(&gate, &identity, &third, now).unwrap()
        else {
            panic!("expected Paired");
        };
        assert_eq!((user.as_str(), key), ("4C:32:75:9B:2E:F1", [0x33; 32]));
        assert_eq!(mac.receiver_key(&reply), identity.public_key());
    }

    #[test]
    fn a_wrong_pin_fails_and_the_pin_is_used_up() {
        let identity = Identity::generate();
        let gate = parking_lot::Mutex::new(PinGate::default());
        let now = Instant::now();
        let pin = gate.lock().start(now).unwrap();
        let wrong = format!("{:04}", (pin.parse::<u32>().unwrap() + 1) % 10_000);
        let mut mac = Mac::new("mac", [1; 32]);
        let mut setup = PinSetup::default();
        let first = reply(setup.step(&gate, &identity, &mac.first(), now).unwrap());
        let second = mac.second(&first, &wrong);
        assert_eq!(
            setup.step(&gate, &identity, &second, now).err(),
            Some(PinError::WrongPin)
        );
        // Trying again needs a new PIN.
        assert_eq!(
            setup.step(&gate, &identity, &mac.first(), now).err(),
            Some(PinError::NoPin)
        );
    }

    #[test]
    fn a_pin_runs_out_and_guessing_locks() {
        let mut gate = PinGate::default();
        let now = Instant::now();
        gate.start(now).unwrap();
        assert_eq!(
            gate.take(now + PIN_LIFETIME + Duration::from_secs(1)),
            Err(PinError::NoPin)
        );
        for _ in 0..MAX_FAILURES {
            gate.start(now).unwrap();
            gate.failed(now);
        }
        assert_eq!(gate.start(now), Err(PinError::Locked));
        assert!(gate.start(now + LOCKOUT).is_ok());
    }

    #[test]
    fn steps_out_of_order_are_refused() {
        let identity = Identity::generate();
        let gate = parking_lot::Mutex::new(PinGate::default());
        let mut setup = PinSetup::default();
        let mut early = Dictionary::new();
        early.insert("epk".into(), Value::Data(vec![0; 32]));
        early.insert("authTag".into(), Value::Data(vec![0; 16]));
        assert_eq!(
            setup.step(&gate, &identity, &early, Instant::now()).err(),
            Some(PinError::OutOfOrder)
        );
        assert_eq!(
            setup
                .step(&gate, &identity, &Dictionary::new(), Instant::now())
                .err(),
            Some(PinError::Malformed)
        );
    }

    #[test]
    fn trusted_devices_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("uwumirror-trusted-{}", std::process::id()));
        let path = dir.join("airplay-trusted");
        let mut trusted = TrustedDevices::load(&path);
        assert_eq!(trusted.len(), 0);
        trusted.add([7; 32], "4C:32:75:9B:2E:F1").unwrap();
        trusted.add([8; 32], "other mac\nwith a newline").unwrap();
        // The same device pairing again replaces its old key.
        trusted.add([9; 32], "4C:32:75:9B:2E:F1").unwrap();
        let again = TrustedDevices::load(&path);
        assert!(!again.contains(&[7; 32]));
        assert!(again.contains(&[8; 32]) && again.contains(&[9; 32]));
        assert_eq!(again.len(), 2);
        let mut again = again;
        again.forget().unwrap();
        assert!(!TrustedDevices::load(&path).contains(&[9; 32]));
        std::fs::remove_dir_all(dir).ok();
    }
}
