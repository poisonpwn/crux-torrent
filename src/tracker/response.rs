use super::peers::PeerAddresses;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    #[serde(rename = "interval")]
    pub request_interval_seconds: u64,

    #[serde(rename = "peers")]
    pub peer_addreses: PeerAddresses,
}
