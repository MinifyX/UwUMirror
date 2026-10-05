//! What the receiver is up to, in words the page can show.
//!
//! Windows tells two things apart: whether its receiver listens
//! (`MiracastReceiverListeningStatus`) and whether the Wi-Fi adapter can do
//! Miracast at all (`MiracastReceiverWiFiStatus`); starting a session can fail
//! with a third (`MiracastReceiverSessionStartStatus`). The numbers below are
//! those enums' values, so the mapping is plain code, tested on every system.

// Only the Windows receiver asks; elsewhere the mapping is there for the tests.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;

/// The receiver's state, as the start page shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MiracastState {
    /// Not Windows: there is no receiver to borrow.
    Unsupported,
    /// Switched off in UwUMirror.
    Off,
    Starting,
    /// Phones and PCs can find this computer and connect.
    Listening,
    /// A sender is connected (or connecting).
    Connected,
    /// The Wi-Fi adapter or its driver can't do Wi-Fi Direct, or there is no
    /// Wi-Fi adapter at all.
    NoWifiDirect,
    /// Windows can't tell yet whether Miracast works: Wi-Fi is off, most
    /// likely, or the adapter still starts.
    WifiOff,
    /// A group policy forbids projecting to this PC.
    DisabledByPolicy,
    /// Windows holds the receiver back for now — while this PC casts its own
    /// screen somewhere, for one.
    Busy,
    /// Something else went wrong; see `error`.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MiracastStatus {
    pub state: MiracastState,
    /// The name senders list: Windows' own receiver name, the computer's
    /// name (see `receiver.rs` for why UwUMirror doesn't change it).
    pub name: String,
    /// A PIN to type on the sender, while one asks for it.
    pub pin: Option<String>,
    /// What went wrong, for `Failed`.
    pub error: Option<String>,
}

impl MiracastStatus {
    pub fn unsupported() -> Self {
        Self::new(MiracastState::Unsupported, String::new())
    }

    pub fn new(state: MiracastState, name: String) -> Self {
        Self {
            state,
            name,
            pin: None,
            error: None,
        }
    }
}

// `MiracastReceiverListeningStatus`.
pub(crate) const NOT_LISTENING: i32 = 0;
pub(crate) const LISTENING: i32 = 1;
pub(crate) const CONNECTION_PENDING: i32 = 2;
pub(crate) const CONNECTED: i32 = 3;
pub(crate) const DISABLED_BY_POLICY: i32 = 4;
pub(crate) const TEMPORARILY_DISABLED: i32 = 5;

// `MiracastReceiverWiFiStatus`.
pub(crate) const WIFI_UNDETERMINED: i32 = 0;
pub(crate) const WIFI_NOT_SUPPORTED: i32 = 1;
pub(crate) const WIFI_NOT_OPTIMIZED: i32 = 2;
pub(crate) const WIFI_SUPPORTED: i32 = 3;

// `MiracastReceiverSessionStartStatus`.
pub(crate) const START_SUCCESS: i32 = 0;
pub(crate) const START_NOT_SUPPORTED: i32 = 2;
pub(crate) const START_ACCESS_DENIED: i32 = 3;

/// The state from Windows' two answers, for a receiver whose session runs.
///
/// "Not optimized" Wi-Fi still receives (Windows' own app then warns that the
/// picture may stutter), so it counts as supported.
pub(crate) fn state_of(listening: i32, wifi: i32) -> MiracastState {
    match listening {
        DISABLED_BY_POLICY => MiracastState::DisabledByPolicy,
        TEMPORARILY_DISABLED => MiracastState::Busy,
        _ if wifi == WIFI_NOT_SUPPORTED => MiracastState::NoWifiDirect,
        LISTENING => MiracastState::Listening,
        CONNECTION_PENDING | CONNECTED => MiracastState::Connected,
        _ if wifi == WIFI_UNDETERMINED => MiracastState::WifiOff,
        _ => MiracastState::Starting,
    }
}

/// Whether the adapter looks able to receive, so that a session which
/// couldn't start is worth starting again.
pub(crate) fn wifi_ready(wifi: i32) -> bool {
    wifi == WIFI_SUPPORTED || wifi == WIFI_NOT_OPTIMIZED
}

/// The state after a failed `MiracastReceiverSession::Start`, with what to
/// say about it.
pub(crate) fn start_failure(status: i32, wifi: i32) -> (MiracastState, Option<String>) {
    match status {
        START_NOT_SUPPORTED if wifi == WIFI_UNDETERMINED => (MiracastState::WifiOff, None),
        START_NOT_SUPPORTED => (MiracastState::NoWifiDirect, None),
        START_ACCESS_DENIED => (MiracastState::Failed, Some("access denied".into())),
        other => (
            MiracastState::Failed,
            Some(format!("session start status {other}")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listening_with_wifi_direct() {
        assert_eq!(
            state_of(LISTENING, WIFI_SUPPORTED),
            MiracastState::Listening
        );
        assert_eq!(
            state_of(LISTENING, WIFI_NOT_OPTIMIZED),
            MiracastState::Listening
        );
        assert_eq!(
            state_of(CONNECTION_PENDING, WIFI_SUPPORTED),
            MiracastState::Connected
        );
        assert_eq!(
            state_of(CONNECTED, WIFI_SUPPORTED),
            MiracastState::Connected
        );
    }

    #[test]
    fn policy_and_busy_win_over_wifi() {
        assert_eq!(
            state_of(DISABLED_BY_POLICY, WIFI_NOT_SUPPORTED),
            MiracastState::DisabledByPolicy
        );
        assert_eq!(
            state_of(TEMPORARILY_DISABLED, WIFI_SUPPORTED),
            MiracastState::Busy
        );
    }

    #[test]
    fn no_wifi_direct_and_wifi_off() {
        assert_eq!(
            state_of(NOT_LISTENING, WIFI_NOT_SUPPORTED),
            MiracastState::NoWifiDirect
        );
        assert_eq!(
            state_of(NOT_LISTENING, WIFI_UNDETERMINED),
            MiracastState::WifiOff
        );
        // Still coming up, on a good adapter.
        assert_eq!(
            state_of(NOT_LISTENING, WIFI_SUPPORTED),
            MiracastState::Starting
        );
    }

    #[test]
    fn failed_starts() {
        assert_eq!(
            start_failure(START_NOT_SUPPORTED, WIFI_NOT_SUPPORTED).0,
            MiracastState::NoWifiDirect
        );
        assert_eq!(
            start_failure(START_NOT_SUPPORTED, WIFI_UNDETERMINED).0,
            MiracastState::WifiOff
        );
        let (state, error) = start_failure(START_ACCESS_DENIED, WIFI_SUPPORTED);
        assert_eq!(state, MiracastState::Failed);
        assert_eq!(error.as_deref(), Some("access denied"));
        assert_eq!(start_failure(1, WIFI_SUPPORTED).0, MiracastState::Failed);
        assert!(wifi_ready(WIFI_SUPPORTED) && !wifi_ready(WIFI_UNDETERMINED));
        assert_eq!(START_SUCCESS, 0);
    }

    #[test]
    fn status_as_json() {
        let status = MiracastStatus::new(MiracastState::NoWifiDirect, "LVDesk1".into());
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"state":"noWifiDirect","name":"LVDesk1","pin":null,"error":null}"#
        );
    }
}
