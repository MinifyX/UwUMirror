//! AirPlay's legacy pairing: who we are, and a key shared with the sender.
//!
//! UwUMirror has one Ed25519 key for good (its public half is the `pk` in the
//! Bonjour record), kept in the app's data folder by [`Identity`]. Each
//! connection then runs pair-verify: both sides trade X25519 keys, sign both
//! public keys with their Ed25519 key, and send the signature encrypted with
//! AES-CTR under a key derived from the shared secret. That shared secret
//! later seasons the stream key (see `session.rs`).
//!
//! We don't ask for a PIN: anyone on the network may mirror to the receiver
//! while it is switched on, exactly like an Apple TV set to "everyone on the
//! same network". The app shows who is connecting and can end it. Only a
//! sender that asks for a PIN itself gets one (see `pin.rs`); it pairs with
//! SRP in place of `/pair-setup`, and runs the same pair-verify afterwards.

use std::path::Path;

use aes::cipher::{KeyIvInit, StreamCipher};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha512};
use x25519_dalek::{PublicKey, StaticSecret};

type Aes128Ctr = ctr::Ctr128BE<aes::Aes128>;

/// The receiver's lasting identity: its Ed25519 key and its hardware address.
///
/// AirPlay names a receiver by a MAC address. It needn't be a real one — it
/// only has to stay the same, or every iPhone that knew the receiver would
/// see a new one. So it is random (with the "locally administered" bit set)
/// and saved next to the key.
#[derive(Clone)]
pub struct Identity {
    pub key: SigningKey,
    pub device_id: [u8; 6],
}

impl Identity {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let mut device_id = [0u8; 6];
        OsRng.fill_bytes(&mut device_id);
        device_id[0] = (device_id[0] | 0x02) & 0xfe;
        Self {
            key: SigningKey::from_bytes(&seed),
            device_id,
        }
    }

    /// Reads the identity from `path`, or makes one and writes it there.
    pub fn load_or_create(path: &Path) -> std::io::Result<Self> {
        if let Ok(bytes) = std::fs::read(path) {
            if bytes.len() == 38 {
                let mut seed = [0u8; 32];
                seed.copy_from_slice(&bytes[..32]);
                let mut device_id = [0u8; 6];
                device_id.copy_from_slice(&bytes[32..]);
                return Ok(Self {
                    key: SigningKey::from_bytes(&seed),
                    device_id,
                });
            }
            tracing::warn!(path = %path.display(), "AirPlay identity unreadable, making a new one");
        }
        let identity = Self::generate();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut bytes = identity.key.to_bytes().to_vec();
        bytes.extend_from_slice(&identity.device_id);
        std::fs::write(path, bytes)?;
        Ok(identity)
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// "AA:BB:CC:DD:EE:FF", as AirPlay's `deviceid`.
    pub fn device_id_string(&self) -> String {
        self.device_id
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    /// "AABBCCDDEEFF", as the first half of the RAOP service name.
    pub fn device_id_hex(&self) -> String {
        self.device_id.iter().map(|b| format!("{b:02X}")).collect()
    }
}

fn derive(salt: &[u8], secret: &[u8; 32]) -> [u8; 16] {
    let hash = Sha512::new()
        .chain_update(salt)
        .chain_update(secret)
        .finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&hash[..16]);
    out
}

fn cipher(secret: &[u8; 32]) -> Aes128Ctr {
    let key = derive(b"Pair-Verify-AES-Key", secret);
    let iv = derive(b"Pair-Verify-AES-IV", secret);
    Aes128Ctr::new(&key.into(), &iv.into())
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PairError {
    #[error("pair-verify message has the wrong size")]
    Malformed,
    #[error("pair-verify steps came out of order")]
    OutOfOrder,
    #[error("the sender's signature doesn't verify")]
    BadSignature,
}

struct Handshake {
    ours: PublicKey,
    theirs: PublicKey,
    their_signing_key: [u8; 32],
    secret: [u8; 32],
}

/// One connection's pair-verify.
#[derive(Default)]
pub struct PairVerify {
    handshake: Option<Handshake>,
    verified: bool,
}

impl PairVerify {
    /// Handles one `POST /pair-verify` body and returns the reply body.
    pub fn step(&mut self, identity: &Identity, body: &[u8]) -> Result<Vec<u8>, PairError> {
        match body.first() {
            Some(1) => self.first(identity, body),
            Some(0) => self.second(body).map(|()| Vec::new()),
            _ => Err(PairError::Malformed),
        }
    }

    fn first(&mut self, identity: &Identity, body: &[u8]) -> Result<Vec<u8>, PairError> {
        if body.len() != 4 + 32 + 32 {
            return Err(PairError::Malformed);
        }
        let mut theirs = [0u8; 32];
        theirs.copy_from_slice(&body[4..36]);
        let mut their_signing_key = [0u8; 32];
        their_signing_key.copy_from_slice(&body[36..68]);

        let secret = StaticSecret::random_from_rng(OsRng);
        let ours = PublicKey::from(&secret);
        let theirs = PublicKey::from(theirs);
        let shared = *secret.diffie_hellman(&theirs).as_bytes();

        let mut message = [0u8; 64];
        message[..32].copy_from_slice(ours.as_bytes());
        message[32..].copy_from_slice(theirs.as_bytes());
        let mut signature = identity.key.sign(&message).to_bytes();
        cipher(&shared).apply_keystream(&mut signature);

        let mut reply = ours.as_bytes().to_vec();
        reply.extend_from_slice(&signature);
        self.handshake = Some(Handshake {
            ours,
            theirs,
            their_signing_key,
            secret: shared,
        });
        self.verified = false;
        Ok(reply)
    }

    fn second(&mut self, body: &[u8]) -> Result<(), PairError> {
        if body.len() != 4 + 64 {
            return Err(PairError::Malformed);
        }
        let handshake = self.handshake.as_ref().ok_or(PairError::OutOfOrder)?;
        let mut cipher = cipher(&handshake.secret);
        // Our own signature used the first 64 bytes of this key stream.
        let mut skip = [0u8; 64];
        cipher.apply_keystream(&mut skip);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&body[4..]);
        cipher.apply_keystream(&mut signature);

        let mut message = [0u8; 64];
        message[..32].copy_from_slice(handshake.theirs.as_bytes());
        message[32..].copy_from_slice(handshake.ours.as_bytes());
        let key = VerifyingKey::from_bytes(&handshake.their_signing_key)
            .map_err(|_| PairError::BadSignature)?;
        key.verify(&message, &Signature::from_bytes(&signature))
            .map_err(|_| PairError::BadSignature)?;
        self.verified = true;
        Ok(())
    }

    /// The X25519 secret shared with the sender, once pair-verify began.
    pub fn shared_secret(&self) -> Option<[u8; 32]> {
        self.handshake.as_ref().map(|h| h.secret)
    }

    pub fn verified(&self) -> bool {
        self.verified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plays the sender's side of pair-verify against ours.
    #[test]
    fn a_sender_that_follows_the_protocol_verifies() {
        let receiver = Identity::generate();
        let sender_key = SigningKey::from_bytes(&[9u8; 32]);
        let sender_secret = StaticSecret::from([5u8; 32]);
        let sender_public = PublicKey::from(&sender_secret);

        let mut first = vec![1, 0, 0, 0];
        first.extend_from_slice(sender_public.as_bytes());
        first.extend_from_slice(&sender_key.verifying_key().to_bytes());
        let mut verify = PairVerify::default();
        let reply = verify.step(&receiver, &first).unwrap();
        assert_eq!(reply.len(), 96);

        // The sender checks our signature...
        let mut receiver_public = [0u8; 32];
        receiver_public.copy_from_slice(&reply[..32]);
        let shared = *sender_secret
            .diffie_hellman(&PublicKey::from(receiver_public))
            .as_bytes();
        assert_eq!(verify.shared_secret(), Some(shared));
        let mut stream = cipher(&shared);
        let mut signature = [0u8; 64];
        signature.copy_from_slice(&reply[32..]);
        stream.apply_keystream(&mut signature);
        let mut message = receiver_public.to_vec();
        message.extend_from_slice(sender_public.as_bytes());
        receiver
            .key
            .verifying_key()
            .verify(&message, &Signature::from_bytes(&signature))
            .expect("our signature verifies");

        // ...and sends its own, continuing the same key stream.
        let mut message = sender_public.as_bytes().to_vec();
        message.extend_from_slice(&receiver_public);
        let mut theirs = sender_key.sign(&message).to_bytes();
        stream.apply_keystream(&mut theirs);
        let mut second = vec![0, 0, 0, 0];
        second.extend_from_slice(&theirs);
        assert_eq!(verify.step(&receiver, &second), Ok(Vec::new()));
        assert!(verify.verified());
    }

    #[test]
    fn a_wrong_signature_is_refused() {
        let receiver = Identity::generate();
        let mut first = vec![1, 0, 0, 0];
        first.extend_from_slice(&[3u8; 32]);
        first.extend_from_slice(
            &SigningKey::from_bytes(&[4u8; 32])
                .verifying_key()
                .to_bytes(),
        );
        let mut verify = PairVerify::default();
        verify.step(&receiver, &first).unwrap();
        let mut second = vec![0, 0, 0, 0];
        second.extend_from_slice(&[0u8; 64]);
        assert_eq!(
            verify.step(&receiver, &second),
            Err(PairError::BadSignature)
        );
        assert!(!verify.verified());
    }

    #[test]
    fn identity_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("uwumirror-id-{}", std::process::id()));
        let path = dir.join("airplay-identity");
        let a = Identity::load_or_create(&path).unwrap();
        let b = Identity::load_or_create(&path).unwrap();
        assert_eq!(a.public_key(), b.public_key());
        assert_eq!(a.device_id, b.device_id);
        assert_eq!(a.device_id[0] & 0x03, 0x02, "locally administered, unicast");
        assert_eq!(a.device_id_string().len(), 17);
        std::fs::remove_dir_all(dir).ok();
    }
}
