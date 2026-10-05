//! How UwUMirrors find each other: `_uwumirror._tcp` on mDNS.
//!
//! The instance name is made of a random id (`uwumirror-1a2b3c4d5e6f`), not
//! the receiver's name: two computers called the same would otherwise share
//! one record. The name people read travels in the TXT record, with the
//! app's version, the protocol version and that id — which is also how a
//! sender leaves itself out of its own list.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};

use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use parking_lot::Mutex;
use rand::Rng;
use serde::Serialize;

use crate::protocol::{SERVICE_TYPE, VERSION};

/// This run's id on the network, the same for announcing and browsing.
pub fn instance_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        format!(
            "{:012x}",
            rand::thread_rng().gen::<u64>() & 0xffff_ffff_ffff
        )
    })
}

/// The receiver on the network, for as long as this lives.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Announcement {
    pub fn start(name: &str, port: u16) -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        // IPv4 only, like the listener (and the AirPlay receiver's records).
        daemon.disable_interface(IfKind::IPv6)?;
        let id = instance_id();
        let version = VERSION.to_string();
        let properties = [
            ("name", name),
            ("version", env!("CARGO_PKG_VERSION")),
            ("proto", version.as_str()),
            ("id", id),
        ];
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &format!("uwumirror-{id}"),
            &format!("uwumirror-cast-{id}.local."),
            "",
            port,
            &properties[..],
        )?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_owned();
        daemon.register(info)?;
        Ok(Self { daemon, fullname })
    }
}

impl Drop for Announcement {
    fn drop(&mut self) {
        // Say goodbye, so senders drop us from their list at once.
        if let Ok(done) = self.daemon.unregister(&self.fullname) {
            let _ = done.recv_timeout(std::time::Duration::from_millis(500));
        }
        let _ = self.daemon.shutdown();
    }
}

/// A receiver somewhere on the network.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub id: String,
    pub name: String,
    /// The app's version on that computer.
    pub version: String,
    /// It speaks this computer's protocol version.
    pub compatible: bool,
    #[serde(skip)]
    pub address: SocketAddr,
}

/// Watches the network for receivers until dropped.
pub struct Browser {
    daemon: ServiceDaemon,
    found: Arc<Mutex<HashMap<String, Found>>>,
}

fn describe(info: &ServiceInfo) -> Option<Found> {
    let ip = info.get_addresses().iter().copied().find(IpAddr::is_ipv4)?;
    let text = |key: &str| {
        info.get_property_val_str(key)
            .unwrap_or_default()
            .to_owned()
    };
    let id = text("id");
    if id.is_empty() {
        return None;
    }
    let name = match text("name") {
        name if name.trim().is_empty() => "UwUMirror".to_owned(),
        name => name.chars().filter(|c| !c.is_control()).take(120).collect(),
    };
    Some(Found {
        compatible: text("proto") == VERSION.to_string(),
        id,
        name,
        version: text("version"),
        address: SocketAddr::new(ip, info.get_port()),
    })
}

impl Browser {
    pub fn start() -> Result<Self, mdns_sd::Error> {
        let daemon = ServiceDaemon::new()?;
        daemon.disable_interface(IfKind::IPv6)?;
        let events = daemon.browse(SERVICE_TYPE)?;
        let found: Arc<Mutex<HashMap<String, Found>>> = Arc::default();
        let map = found.clone();
        // Ends when the daemon shuts down and the channel closes.
        std::thread::Builder::new()
            .name("uwumirror-cast-browse".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            if let Some(receiver) = describe(&info) {
                                map.lock().insert(info.get_fullname().to_owned(), receiver);
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            map.lock().remove(&fullname);
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|error| mdns_sd::Error::Msg(error.to_string()))?;
        Ok(Self { daemon, found })
    }

    /// Every receiver seen, except this computer's own, by name.
    pub fn receivers(&self) -> Vec<Found> {
        let mut list: Vec<Found> = self
            .found
            .lock()
            .values()
            .filter(|receiver| receiver.id != instance_id())
            .cloned()
            .collect();
        list.sort_by_key(|receiver| receiver.name.to_lowercase());
        list
    }

    pub fn get(&self, id: &str) -> Option<Found> {
        self.receivers().into_iter().find(|r| r.id == id)
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
