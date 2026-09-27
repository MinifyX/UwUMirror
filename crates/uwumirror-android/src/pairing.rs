//! Wireless debugging's pairing, the way Android Studio does it with a QR code.
//!
//! On the phone: Developer options → Wireless debugging → "Pair device with
//! QR code". The QR code we show says `WIFI:T:ADB;S:<name>;P:<password>;;`.
//! The phone scans it and announces a pairing service under exactly that
//! name on the network; we find it with mDNS and run `adb pair` with the
//! password. Once paired, the phone announces its debugging port as
//! `_adb-tls-connect._tcp`, and we `adb connect` to it.
//!
//! The same works without a camera: "Pair device with pairing code" shows an
//! address and six digits, which the user types into UwUMirror.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use mdns_sd::{ServiceDaemon, ServiceEvent};
use qrcode::render::svg;
use qrcode::QrCode;
use rand::Rng;
use serde::{Deserialize, Serialize};

const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp.local.";
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp.local.";

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("the network search could not start: {0}")]
    Mdns(#[from] mdns_sd::Error),
    #[error("no phone answered in time")]
    Timeout,
}

/// What the QR code carries, and the code itself as SVG.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrPairing {
    pub name: String,
    pub password: String,
    pub svg: String,
}

fn random_text(len: usize) -> String {
    // Letters and digits only: the WIFI: format would need escaping for
    // anything else, and phones differ in whether they undo it.
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

pub fn qr_payload(name: &str, password: &str) -> String {
    format!("WIFI:T:ADB;S:{name};P:{password};;")
}

/// A fresh name and password, and the QR code for them.
pub fn new_qr_pairing() -> QrPairing {
    let name = format!("uwumirror-{}", random_text(6));
    let password = random_text(10);
    let svg = QrCode::new(qr_payload(&name, &password).as_bytes())
        .map(|code| {
            code.render::<svg::Color<'_>>()
                .min_dimensions(240, 240)
                .quiet_zone(true)
                .dark_color(svg::Color("#1c1420"))
                .light_color(svg::Color("#ffffff"))
                .build()
        })
        .unwrap_or_default();
    QrPairing {
        name,
        password,
        svg,
    }
}

/// Prefers an IPv4 address: adb's pairing handles those most reliably.
fn pick(addresses: impl IntoIterator<Item = IpAddr>, port: u16) -> Option<SocketAddr> {
    let addresses: Vec<IpAddr> = addresses.into_iter().collect();
    addresses
        .iter()
        .find(|ip| ip.is_ipv4())
        .or_else(|| addresses.first())
        .map(|ip| SocketAddr::new(*ip, port))
}

/// Waits for a service of `kind` whose instance name passes `wanted`.
async fn wait_for(
    kind: &str,
    timeout: Duration,
    wanted: impl Fn(&str, &[IpAddr]) -> bool,
) -> Result<SocketAddr, PairingError> {
    let daemon = ServiceDaemon::new()?;
    let events = daemon.browse(kind)?;
    let found = tokio::time::timeout(timeout, async {
        while let Ok(event) = events.recv_async().await {
            if let ServiceEvent::ServiceResolved(info) = event {
                let instance = info.get_fullname().strip_suffix(kind).unwrap_or_default();
                let instance = instance.trim_end_matches('.');
                let addresses: Vec<IpAddr> = info.get_addresses().iter().copied().collect();
                if wanted(instance, &addresses) {
                    if let Some(address) = pick(addresses, info.get_port()) {
                        return Some(address);
                    }
                }
            }
        }
        None
    })
    .await;
    let _ = daemon.shutdown();
    found.ok().flatten().ok_or(PairingError::Timeout)
}

/// The pairing service the phone announces after scanning our QR code.
pub async fn wait_for_pairing(name: &str, timeout: Duration) -> Result<SocketAddr, PairingError> {
    wait_for(PAIRING_SERVICE, timeout, |instance, _| instance == name).await
}

/// The debugging port a paired phone at `ip` announces.
pub async fn wait_for_connect(ip: IpAddr, timeout: Duration) -> Result<SocketAddr, PairingError> {
    wait_for(CONNECT_SERVICE, timeout, |_, addresses| {
        addresses.contains(&ip)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_code_says_what_android_studio_says() {
        let pairing = new_qr_pairing();
        assert!(pairing.name.starts_with("uwumirror-"));
        assert_eq!(pairing.password.len(), 10);
        assert!(pairing.svg.starts_with("<?xml") || pairing.svg.starts_with("<svg"));
        assert_eq!(qr_payload("studio-x", "pw"), "WIFI:T:ADB;S:studio-x;P:pw;;");
    }

    #[test]
    fn prefers_ipv4() {
        let v6: IpAddr = "fe80::1".parse().unwrap();
        let v4: IpAddr = "192.168.1.9".parse().unwrap();
        assert_eq!(pick([v6, v4], 37000), Some(SocketAddr::new(v4, 37000)));
        assert_eq!(pick([v6], 1), Some(SocketAddr::new(v6, 1)));
        assert_eq!(pick([], 1), None);
    }
}
