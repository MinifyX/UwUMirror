//! SRP-6a the way Apple's legacy PIN pairing speaks it.
//!
//! The PIN on the receiver's screen is the password, the sender's device id
//! the user name. SRP lets both sides prove they know the PIN without either
//! sending it, and leaves them with a shared session key.
//!
//! The arithmetic is RFC 5054's: the 2048-bit group from its appendix A,
//! SHA-1, `k = H(N | PAD(g))`, `u = H(PAD(A) | PAD(B))`,
//! `x = H(s | H(I ":" P))`. Apple changed two things, and so must we, or the
//! proofs never match:
//!
//! - The session key is 40 bytes, not one hash: `K = H(S | 00000000) |
//!   H(S | 00000001)`.
//! - The proofs hash that whole 40-byte key:
//!   `M1 = H(H(N) ^ H(g) | H(I) | s | A | B | K)`, `M2 = H(A | M1 | K)`.
//!
//! Numbers go into `M1`, `M2` and `K` as their shortest big-endian bytes, into
//! `k` and `u` padded to the group's length — as UxPlay's receiver does, which
//! Macs pair with. (The receiver keeps its own `B` and salt free of leading
//! zeros, so for them the two spellings agree anyway.)
//!
//! `num-bigint` doesn't promise constant time. The secret exponent `b` lives
//! for one pairing attempt, and a PIN gets a single guess (see `pin.rs`), so
//! a timing leak has little to give away.

use num_bigint::BigUint;
use sha1::{Digest, Sha1};
use subtle::ConstantTimeEq;

/// RFC 5054, appendix A, the 2048-bit group; the generator is 2.
const N_2048: &str = "AC6BDB41324A9A9BF166DE5E1389582FAF72B6651987EE07FC3192943DB56050\
A37329CBB4A099ED8193E0757767A13DD52312AB4B03310DCD7F48A9DA04FD50E8083969EDB767B0CF609517\
9A163AB3661A05FBD5FAAAE82918A9962F0B93B855F97993EC975EEAA80D740ADBF4FF747359D041D5C33EA7\
1D281E446B14773BCA97B43A23FB801676BD207A436C6481F1D2B9078717461A5B9D32E688F87748544523B5\
24B0D57D5EA77A2775D2ECFA032CFBDBF52FB3786160279004E57AE6AF874E7303CE53299CCC041C7BC308D8\
2A5698F3A8D0C38271AE35F8E9DBFBB694B5C803D89F7AE435DE236D525F54759B65E372FCD68EF20FA7111F\
9E4AFF73";

/// Apple's session key: two SHA-1 hashes back to back.
pub const SESSION_KEY_LEN: usize = 40;
/// A SHA-1 proof.
pub const PROOF_LEN: usize = 20;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SrpError {
    /// `A` is zero modulo `N`, too long, or `u` came out zero: a sender that
    /// would learn the key without the PIN.
    #[error("the sender's SRP public key is unusable")]
    BadPublicKey,
    /// The sender's proof doesn't match: a wrong PIN.
    #[error("the sender's SRP proof doesn't match")]
    WrongProof,
}

/// A prime and its generator.
pub struct Group {
    n: BigUint,
    g: BigUint,
}

fn sha1(parts: &[&[u8]]) -> [u8; PROOF_LEN] {
    let mut hash = Sha1::new();
    for part in parts {
        hash.update(part);
    }
    hash.finalize().into()
}

fn number(hash: &[u8]) -> BigUint {
    BigUint::from_bytes_be(hash)
}

impl Group {
    pub fn new(n_hex: &str, g: u32) -> Self {
        Self {
            n: BigUint::parse_bytes(n_hex.as_bytes(), 16).expect("a prime in hex"),
            g: BigUint::from(g),
        }
    }

    /// The group Apple's PIN pairing uses.
    pub fn rfc5054_2048() -> Self {
        Self::new(N_2048, 2)
    }

    /// The prime's length in bytes; `PAD()` pads to it.
    fn len(&self) -> usize {
        self.n.bits().div_ceil(8) as usize
    }

    fn pad(&self, value: &BigUint) -> Vec<u8> {
        let bytes = value.to_bytes_be();
        let mut out = vec![0u8; self.len().saturating_sub(bytes.len())];
        out.extend_from_slice(&bytes);
        out
    }

    /// `k = H(N | PAD(g))`.
    fn k(&self) -> BigUint {
        number(&sha1(&[&self.pad(&self.n), &self.pad(&self.g)]))
    }

    /// `u = H(PAD(A) | PAD(B))`.
    fn u(&self, a: &BigUint, b: &BigUint) -> BigUint {
        number(&sha1(&[&self.pad(a), &self.pad(b)]))
    }

    /// `x = H(s | H(I ":" P))`.
    fn x(salt: &[u8], user: &str, password: &str) -> BigUint {
        let inner = sha1(&[user.as_bytes(), b":", password.as_bytes()]);
        number(&sha1(&[&number(salt).to_bytes_be(), &inner]))
    }

    /// `v = g^x`: what the server keeps instead of the password.
    fn verifier(&self, salt: &[u8], user: &str, password: &str) -> BigUint {
        self.g.modpow(&Self::x(salt, user, password), &self.n)
    }

    /// `M1 = H(H(N) ^ H(g) | H(I) | s | A | B | K)`.
    fn client_proof(
        &self,
        user: &str,
        salt: &[u8],
        a: &BigUint,
        b: &BigUint,
        key: &[u8; SESSION_KEY_LEN],
    ) -> [u8; PROOF_LEN] {
        let hn = sha1(&[&self.n.to_bytes_be()]);
        let hg = sha1(&[&self.g.to_bytes_be()]);
        let mut xor = [0u8; PROOF_LEN];
        for (out, (n, g)) in xor.iter_mut().zip(hn.iter().zip(hg.iter())) {
            *out = n ^ g;
        }
        sha1(&[
            &xor,
            &sha1(&[user.as_bytes()]),
            &number(salt).to_bytes_be(),
            &a.to_bytes_be(),
            &b.to_bytes_be(),
            key,
        ])
    }
}

/// Apple's 40-byte session key from the premaster secret `S`.
fn session_key(premaster: &BigUint) -> [u8; SESSION_KEY_LEN] {
    let s = premaster.to_bytes_be();
    let mut key = [0u8; SESSION_KEY_LEN];
    key[..PROOF_LEN].copy_from_slice(&sha1(&[&s, &[0, 0, 0, 0]]));
    key[PROOF_LEN..].copy_from_slice(&sha1(&[&s, &[0, 0, 0, 1]]));
    key
}

/// `M2 = H(A | M1 | K)`: the server's proof that it knew the PIN, too.
fn server_proof(
    a: &BigUint,
    client_proof: &[u8; PROOF_LEN],
    key: &[u8; SESSION_KEY_LEN],
) -> [u8; PROOF_LEN] {
    sha1(&[&a.to_bytes_be(), client_proof, key])
}

/// What a successful exchange leaves the server with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub key: [u8; SESSION_KEY_LEN],
    /// `M2`, for the sender.
    pub proof: [u8; PROOF_LEN],
}

/// The server's half of one exchange.
pub struct Server {
    group: Group,
    user: String,
    salt: Vec<u8>,
    verifier: BigUint,
    secret: BigUint,
    public: BigUint,
}

impl Server {
    /// Begins an exchange for `user` with `password`, the salt and the secret
    /// exponent `b` given.
    ///
    /// Returns `None` when `B` would start with a zero byte: whether a sender
    /// hashes that zero or not is anyone's guess, so the caller draws another
    /// `b` (it happens about once in 170).
    pub fn new(group: Group, user: &str, password: &str, salt: &[u8], b: &[u8]) -> Option<Self> {
        let verifier = group.verifier(salt, user, password);
        let secret = BigUint::from_bytes_be(b);
        let public = (group.k() * &verifier + group.g.modpow(&secret, &group.n)) % &group.n;
        if public.to_bytes_be().len() != group.len() {
            return None;
        }
        Some(Self {
            group,
            user: user.to_owned(),
            salt: salt.to_vec(),
            verifier,
            secret,
            public,
        })
    }

    /// `B`, for the sender.
    pub fn public_key(&self) -> Vec<u8> {
        self.public.to_bytes_be()
    }

    /// Checks the sender's `A` and `M1`; with the right PIN behind them,
    /// returns the session key and `M2`.
    pub fn verify(&self, a: &[u8], proof: &[u8]) -> Result<Session, SrpError> {
        if a.is_empty() || a.len() > self.group.len() {
            return Err(SrpError::BadPublicKey);
        }
        let a = BigUint::from_bytes_be(a);
        let zero = BigUint::from(0u32);
        if &a % &self.group.n == zero {
            return Err(SrpError::BadPublicKey);
        }
        let u = self.group.u(&a, &self.public);
        if u == zero {
            return Err(SrpError::BadPublicKey);
        }
        // S = (A · v^u)^b
        let base = (&a * self.verifier.modpow(&u, &self.group.n)) % &self.group.n;
        let premaster = base.modpow(&self.secret, &self.group.n);
        let key = session_key(&premaster);
        let expected = self
            .group
            .client_proof(&self.user, &self.salt, &a, &self.public, &key);
        if proof.len() != PROOF_LEN || !bool::from(expected.ct_eq(proof)) {
            return Err(SrpError::WrongProof);
        }
        Ok(Session {
            key,
            proof: server_proof(&a, &expected, &key),
        })
    }
}

/// The sender's side, for playing a Mac in tests.
#[cfg(test)]
pub struct Client {
    pub public: Vec<u8>,
    pub proof: [u8; PROOF_LEN],
    pub key: [u8; SESSION_KEY_LEN],
    /// The `M2` a receiver that knew the PIN answers with.
    pub expected: [u8; PROOF_LEN],
}

#[cfg(test)]
impl Client {
    /// Answers the server's salt and `B` with the secret exponent `a`.
    pub fn respond(
        group: &Group,
        user: &str,
        password: &str,
        salt: &[u8],
        b: &[u8],
        a: &[u8],
    ) -> Self {
        let n = &group.n;
        let secret = BigUint::from_bytes_be(a);
        let public = group.g.modpow(&secret, n);
        let b = BigUint::from_bytes_be(b);
        let u = group.u(&public, &b);
        let x = Group::x(salt, user, password);
        // S = (B - k·g^x)^(a + u·x)
        let kgx = (group.k() * group.g.modpow(&x, n)) % n;
        let base = (&b + n - kgx) % n;
        let premaster = base.modpow(&(&secret + &u * &x), n);
        let key = session_key(&premaster);
        let proof = group.client_proof(user, salt, &public, &b, &key);
        Self {
            public: public.to_bytes_be(),
            proof,
            key,
            expected: server_proof(&public, &proof, &key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    const N_1024: &str = "EEAF0AB9ADB38DD69C33F80AFA8FC5E86072618775FF3C0B9EA2314C9C256576\
D674DF7496EA81D3383B4813D692C6E0E0D5D8E250B98BE48E495C1D6089DAD15DC7D7B46154D6B6CE8EF4AD\
69B15D4982559B297BCF1885C529F566660E57EC68EDBC3C05726CC02FD4CBF4976EAA9AFD5138FE8376435B\
9FC61D2FC0EB06E3";

    /// RFC 5054, appendix B: the parts Apple left as they were.
    #[test]
    fn rfc5054_test_vectors() {
        let group = Group::new(N_1024, 2);
        let salt = hex("BEB25379 D1A8581E B5A72767 3A2441EE");
        let a = hex("60975527 035CF2AD 1989806F 0407210B C81EDC04 E2762A56 AFD529DD DA2D4393");
        let b = hex("E487CB59 D31AC550 471E81F0 0F6928E0 1DDA08E9 74A004F4 9E61F5D1 05284D20");

        assert_eq!(
            group.k().to_bytes_be(),
            hex("7556AA04 5AEF2CDD 07ABAF0F 665C3E81 8913186F")
        );
        assert_eq!(
            Group::x(&salt, "alice", "password123").to_bytes_be(),
            hex("94B7555A ABE9127C C58CCF49 93DB6CF8 4D16C124")
        );
        let v = group.verifier(&salt, "alice", "password123");
        assert_eq!(
            v.to_bytes_be(),
            hex(
                "7E273DE8 696FFC4F 4E337D05 B4B375BE B0DDE156 9E8FA00A 9886D812
                 9BADA1F1 822223CA 1A605B53 0E379BA4 729FDC59 F105B478 7E5186F5
                 C671085A 1447B52A 48CF1970 B4FB6F84 00BBF4CE BFBB1681 52E08AB5
                 EA53D15C 1AFF87B2 B9DA6E04 E058AD51 CC72BFC9 033B564E 26480D78
                 E955A5E2 9E7AB245 DB2BE315 E2099AFB"
            )
        );
        let server = Server::new(Group::new(N_1024, 2), "alice", "password123", &salt, &b)
            .expect("B is full length here");
        assert_eq!(
            server.public_key(),
            hex(
                "BD0C6151 2C692C0C B6D041FA 01BB152D 4916A1E7 7AF46AE1 05393011
                 BAF38964 DC46A067 0DD125B9 5A981652 236F99D9 B681CBF8 7837EC99
                 6C6DA044 53728610 D0C6DDB5 8B318885 D7D82C7F 8DEB75CE 7BD4FBAA
                 37089E6F 9C6059F3 88838E7A 00030B33 1EB76840 910440B1 B27AAEAE
                 EB4012B7 D7665238 A8E3FB00 4B117B58"
            )
        );
        let client = Client::respond(
            &group,
            "alice",
            "password123",
            &salt,
            &server.public_key(),
            &a,
        );
        assert_eq!(
            client.public,
            hex(
                "61D5E490 F6F1B795 47B0704C 436F523D D0E560F0 C64115BB 72557EC4
                 4352E890 3211C046 92272D8B 2D1A5358 A2CF1B6E 0BFCF99F 921530EC
                 8E393561 79EAE45E 42BA92AE ACED8251 71E1E8B9 AF6D9C03 E1327F44
                 BE087EF0 6530E69F 66615261 EEF54073 CA11CF58 58F0EDFD FE15EFEA
                 B349EF5D 76988A36 72FAC47B 0769447B"
            )
        );
        let a_num = BigUint::from_bytes_be(&client.public);
        assert_eq!(
            group.u(&a_num, &server.public).to_bytes_be(),
            hex("CE38B959 3487DA98 554ED47D 70A7AE5F 462EF019")
        );
        let premaster = hex(
            "B0DC82BA BCF30674 AE450C02 87745E79 90A3381F 63B387AA F271A10D
             233861E3 59B48220 F7C4693C 9AE12B0A 6F67809F 0876E2D0 13800D6C
             41BB59B6 D5979B5C 00A172B4 A2A5903A 0BDCAF8A 709585EB 2AFAFA8F
             3499B200 210DCC1F 10EB3394 3CD67FC8 8A2F39A4 BE5BEC4E C0A3212D
             C346D7E4 74B29EDE 8A469FFE CA686E5A",
        );
        // Both sides reach the RFC's premaster secret, so its Apple-style
        // key is the one they share.
        assert_eq!(client.key, session_key(&BigUint::from_bytes_be(&premaster)));
        let session = server.verify(&client.public, &client.proof).unwrap();
        assert_eq!(session.key, client.key);
        assert_eq!(session.proof, client.expected);
    }

    /// The Apple-specific parts, against values worked out independently
    /// (Node's BigInt and `crypto`, from the description in the module docs).
    #[test]
    fn apple_variant_vectors() {
        let group = Group::rfc5054_2048();
        let salt = hex(APPLE_SALT);
        let server = Server::new(
            Group::rfc5054_2048(),
            APPLE_USER,
            "1234",
            &salt,
            &hex(APPLE_B),
        )
        .expect("full-length B");
        assert_eq!(server.public_key(), hex(APPLE_B_PUBLIC));
        let client = Client::respond(
            &group,
            APPLE_USER,
            "1234",
            &salt,
            &server.public_key(),
            &hex(APPLE_A),
        );
        assert_eq!(client.public, hex(APPLE_A_PUBLIC));
        assert_eq!(client.key.to_vec(), hex(APPLE_K));
        assert_eq!(client.proof.to_vec(), hex(APPLE_M1));
        let session = server.verify(&client.public, &client.proof).unwrap();
        assert_eq!(session.proof.to_vec(), hex(APPLE_M2));
    }

    #[test]
    fn a_wrong_pin_is_refused() {
        let group = Group::rfc5054_2048();
        let salt = [7u8; 16];
        let server =
            Server::new(Group::rfc5054_2048(), "AA:BB", "1234", &salt, &[3u8; 32]).unwrap();
        let client = Client::respond(
            &group,
            "AA:BB",
            "4321",
            &salt,
            &server.public_key(),
            &[5u8; 32],
        );
        assert_eq!(
            server.verify(&client.public, &client.proof),
            Err(SrpError::WrongProof)
        );
    }

    #[test]
    fn a_public_key_of_zero_is_refused() {
        let server = Server::new(
            Group::rfc5054_2048(),
            "AA:BB",
            "1234",
            &[7u8; 16],
            &[3u8; 32],
        )
        .unwrap();
        assert_eq!(server.verify(&[0], &[0; 20]), Err(SrpError::BadPublicKey));
        let n = Group::rfc5054_2048().n.to_bytes_be();
        assert_eq!(server.verify(&n, &[0; 20]), Err(SrpError::BadPublicKey));
        assert_eq!(
            server.verify(&[1; 257], &[0; 20]),
            Err(SrpError::BadPublicKey)
        );
    }

    const APPLE_USER: &str = "4C:32:75:9B:2E:F1";
    const APPLE_SALT: &str = "5a1b2c3d4e5f60718293a4b5c6d7e8f9";
    const APPLE_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const APPLE_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const APPLE_A_PUBLIC: &str = "\
24d1e3e550122e1dc571bcefd01f494de5ca82c5ff005ac469a843e5c5a2898b4c3ea0bac3b9b8e552cef73254dceb54\
96f05eb1d82a97523ce07a43e4268468328741403099f4f0f7a4f28c79a75d2d2b9c27744582063df5d31e5ff586fe1e\
0266151a23549e9b61d93c8b575d28b188d045f7b97511afb36d73e6f8f8bc19605ff47c2440fd378d4bb53580d81f01\
f6bd1c608d9def0b7fefe662b2d4a669dcaceba2a2d8b3979371c0b74027231060f640a922b4374190333c4a102c6e76\
0e5208f75c88b396af912509427875e9649ff390e3a19488157c1593cb951401ccd848fc4bf779f86e5c06cf66f9b2ce\
0e8fbd26c96ac9b4d4662155f89b5d0f";
    const APPLE_B_PUBLIC: &str = "\
3264f2e2411a1c8c376a45131dbaa126ef2f11f73f4a6002067a18bf97823648c89072945c3a33777156c8d6dd242df5\
1268dec01b685582f261f04dcae08ea8fed378c53af14f32f8d96d4324841208a009a7c452f5cde8c745d88d43b701a7\
ae48f43193b246a91705c7a977cf955def61e17ccde57fc0f2f6135333c84cac61fc789628ae17204dbd07076c11c757\
d78def481761b8db2608e8a370bf8285e80d5f22ac37fdb0aea53ebaf1d2655556e8cbcfbde7199d29d91e07cc671cf5\
3bbcad1c41635f1893ce2db360200a339e2131c0b5491c787551be38bd7ec6489c410560c44206c5da786b1517dff5fc\
5fed88da71a1699bbc2587b07dc95812";
    const APPLE_K: &str =
        "3f7ffe70e210bcb00d2888d4df946c230cb622191e6647d197b0a8e671322d91bcd796111c08e22a";
    const APPLE_M1: &str = "0230f301c46538878da699e8e3ccc3d15d2561bb";
    const APPLE_M2: &str = "156c989651a15c63b16e3b1432d5b30c616aea6b";
}
