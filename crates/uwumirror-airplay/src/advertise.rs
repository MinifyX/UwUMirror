//! How iPhones find the receiver: two Bonjour services and `GET /info`.
//!
//! `_airplay._tcp` carries the screen mirroring, `_raop._tcp` ("Remote Audio
//! Output Protocol") the sound; both point at the same port. We present
//! ourselves the way the open-source receivers before us do (UxPlay, RPiPlay):
//! as an Apple TV 3 with AirPlay's legacy pairing and FairPlay, H.264 only.
//! Newer features (HomeKit pairing, HEVC, AirPlay 2 buffered audio) stay off,
//! because every sender still speaks this older dialect.

use mdns_sd::{ServiceDaemon, ServiceInfo};
use plist::{Dictionary, Value};

use crate::pairing::Identity;

/// Bits 0–31 of AirPlay's feature flags: screen mirroring, audio, FairPlay,
/// legacy pairing (bit 27) — UxPlay's set.
pub const FEATURES: u64 = 0x5A7F_FEE6;
const FEATURES_TXT: &str = "0x5A7FFEE6,0x0";
pub const MODEL: &str = "AppleTV3,2";
pub const SOURCE_VERSION: &str = "220.68";
/// A fixed "pairing identity"; senders only check that it is there.
const PI: &str = "2e388006-13ba-4041-9a67-25dd4a43d536";

/// What the receiver offers: its name and the picture it asks senders for.
#[derive(Debug, Clone)]
pub struct Offer {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn airplay_txt(identity: &Identity) -> Vec<(&'static str, String)> {
    vec![
        ("deviceid", identity.device_id_string()),
        ("features", FEATURES_TXT.into()),
        ("pw", "false".into()),
        ("flags", "0x4".into()),
        ("model", MODEL.into()),
        ("pk", hex(&identity.public_key())),
        ("pi", PI.into()),
        ("srcvers", SOURCE_VERSION.into()),
        ("vv", "2".into()),
    ]
}

pub fn raop_txt(identity: &Identity) -> Vec<(&'static str, String)> {
    vec![
        ("ch", "2".into()),
        ("cn", "0,1,2,3".into()),
        ("da", "true".into()),
        ("et", "0,3,5".into()),
        ("vv", "2".into()),
        ("ft", FEATURES_TXT.into()),
        ("am", MODEL.into()),
        ("md", "0,1,2".into()),
        ("rhd", "5.6.0.0".into()),
        ("pw", "false".into()),
        ("sr", "44100".into()),
        ("ss", "16".into()),
        ("sv", "false".into()),
        ("tp", "UDP".into()),
        ("txtvers", "1".into()),
        ("sf", "0x4".into()),
        ("vs", SOURCE_VERSION.into()),
        ("vn", "65537".into()),
        ("pk", hex(&identity.public_key())),
    ]
}

/// A TXT record in DNS wire form, which `GET /info` hands out as data.
pub fn txt_record(pairs: &[(&str, String)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (key, value) in pairs {
        let entry = format!("{key}={value}");
        let bytes = &entry.as_bytes()[..entry.len().min(255)];
        out.push(bytes.len() as u8);
        out.extend_from_slice(bytes);
    }
    out
}

/// Answers `GET /info`. A plist body with a `qualifier` asks only for one of
/// the TXT records; without one, the sender wants the whole description.
pub fn info(identity: &Identity, offer: &Offer, request_body: Option<&Value>) -> Value {
    let mut dict = Dictionary::new();
    let qualifier = request_body
        .and_then(|body| body.as_dictionary())
        .and_then(|body| body.get("qualifier"))
        .and_then(|q| q.as_array())
        .and_then(|q| q.first())
        .and_then(|q| q.as_string());
    match qualifier {
        Some("txtAirPlay") => {
            dict.insert(
                "txtAirPlay".into(),
                Value::Data(txt_record(&airplay_txt(identity))),
            );
            return Value::Dictionary(dict);
        }
        Some("txtRAOP") => {
            dict.insert(
                "txtRAOP".into(),
                Value::Data(txt_record(&raop_txt(identity))),
            );
            return Value::Dictionary(dict);
        }
        _ => {}
    }
    let id = identity.device_id_string();
    dict.insert("deviceID".into(), id.clone().into());
    dict.insert("macAddress".into(), id.into());
    dict.insert("pk".into(), Value::Data(identity.public_key().to_vec()));
    dict.insert("features".into(), Value::Integer(FEATURES.into()));
    dict.insert("name".into(), offer.name.clone().into());
    dict.insert("pi".into(), PI.into());
    dict.insert("vv".into(), Value::Integer(2.into()));
    dict.insert("statusFlags".into(), Value::Integer(68.into()));
    dict.insert("keepAliveLowPower".into(), Value::Integer(1.into()));
    dict.insert("sourceVersion".into(), SOURCE_VERSION.into());
    dict.insert("keepAliveSendStatsAsBody".into(), Value::Boolean(true));
    dict.insert("model".into(), MODEL.into());
    dict.insert("initialVolume".into(), Value::Real(0.0));

    let audio = |kind: u64, entries: &[(&str, Value)]| {
        let mut d = Dictionary::new();
        d.insert("type".into(), Value::Integer(kind.into()));
        for (key, value) in entries {
            d.insert((*key).into(), value.clone());
        }
        Value::Dictionary(d)
    };
    let latency = [
        ("audioType", Value::String("default".into())),
        ("inputLatencyMicros", Value::Integer(0.into())),
        ("outputLatencyMicros", Value::Boolean(false)),
    ];
    dict.insert(
        "audioLatencies".into(),
        Value::Array(vec![audio(100, &latency), audio(101, &latency)]),
    );
    let formats = [
        ("audioInputFormats", Value::Integer(0x3ff_fffc_u64.into())),
        ("audioOutputFormats", Value::Integer(0x3ff_fffc_u64.into())),
    ];
    dict.insert(
        "audioFormats".into(),
        Value::Array(vec![audio(100, &formats), audio(101, &formats)]),
    );

    let mut display = Dictionary::new();
    display.insert("uuid".into(), "e0ff8a27-6738-3d56-8a16-cc53aacee925".into());
    display.insert("widthPhysical".into(), Value::Integer(0.into()));
    display.insert("heightPhysical".into(), Value::Integer(0.into()));
    for key in ["width", "widthPixels"] {
        display.insert(key.into(), Value::Integer(u64::from(offer.width).into()));
    }
    for key in ["height", "heightPixels"] {
        display.insert(key.into(), Value::Integer(u64::from(offer.height).into()));
    }
    display.insert("rotation".into(), Value::Boolean(false));
    display.insert("refreshRate".into(), Value::Real(1.0 / 60.0));
    display.insert("maxFPS".into(), Value::Integer(u64::from(offer.fps).into()));
    display.insert("overscanned".into(), Value::Boolean(false));
    display.insert("features".into(), Value::Integer(14.into()));
    dict.insert(
        "displays".into(),
        Value::Array(vec![Value::Dictionary(display)]),
    );
    Value::Dictionary(dict)
}

/// The two Bonjour services, registered for as long as this lives.
pub struct Announcement {
    daemon: ServiceDaemon,
    names: Vec<String>,
}

impl Announcement {
    pub fn start(identity: &Identity, name: &str, port: u16) -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        let host = format!(
            "uwumirror-{}.local.",
            identity.device_id_hex().to_lowercase()
        );
        let services = [
            (
                "_airplay._tcp.local.",
                name.to_owned(),
                airplay_txt(identity),
            ),
            (
                "_raop._tcp.local.",
                format!("{}@{name}", identity.device_id_hex()),
                raop_txt(identity),
            ),
        ];
        let mut names = Vec::new();
        for (kind, instance, txt) in services {
            let properties: Vec<(&str, &str)> = txt
                .iter()
                .map(|(key, value)| (*key, value.as_str()))
                .collect();
            let info = ServiceInfo::new(kind, &instance, &host, "", port, &properties[..])?
                .enable_addr_auto();
            names.push(info.get_fullname().to_owned());
            daemon.register(info)?;
        }
        Ok(Self { daemon, names })
    }
}

impl Drop for Announcement {
    fn drop(&mut self) {
        // Say goodbye, so senders drop us from their list right away instead
        // of offering a receiver that is gone until the record times out.
        for name in &self.names {
            if let Ok(done) = self.daemon.unregister(name) {
                let _ = done.recv_timeout(std::time::Duration::from_millis(500));
            }
        }
        let _ = self.daemon.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txt_record_is_length_prefixed() {
        let record = txt_record(&[("a", "b".into()), ("pw", "false".into())]);
        assert_eq!(record, b"\x03a=b\x08pw=false");
    }

    #[test]
    fn info_answers_a_qualifier_with_just_that_record() {
        let identity = Identity::generate();
        let offer = Offer {
            name: "Test".into(),
            width: 1920,
            height: 1080,
            fps: 30,
        };
        let mut body = Dictionary::new();
        body.insert("qualifier".into(), Value::Array(vec!["txtAirPlay".into()]));
        let reply = info(&identity, &offer, Some(&Value::Dictionary(body)));
        let reply = reply.as_dictionary().unwrap();
        assert_eq!(reply.len(), 1);
        let txt = reply.get("txtAirPlay").unwrap().as_data().unwrap();
        assert!(String::from_utf8_lossy(txt).contains("features=0x5A7FFEE6,0x0"));

        let full = info(&identity, &offer, None);
        let full = full.as_dictionary().unwrap();
        assert_eq!(full.get("name").unwrap().as_string(), Some("Test"));
        let display = full.get("displays").unwrap().as_array().unwrap()[0]
            .as_dictionary()
            .unwrap();
        assert_eq!(
            display.get("width").unwrap().as_unsigned_integer(),
            Some(1920)
        );
    }
}
