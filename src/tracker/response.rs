use super::peers::PeerAddresses;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TrackerResponse {
    #[serde(rename = "interval")]
    pub request_interval_seconds: u64,

    #[serde(rename = "peers")]
    pub peer_addreses: PeerAddresses,
}

// this struct is seperate so that it  can be deserialized properly and can be converted into a Result whose Ok variant gives the successful TrackerResponse.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum TrackerResponseResult {
    Success(TrackerResponse),
    Failure {
        #[serde(rename = "failure reason")]
        failure_reason: String,
    },
}

impl Into<anyhow::Result<TrackerResponse>> for TrackerResponseResult {
    fn into(self) -> anyhow::Result<TrackerResponse> {
        type TR = TrackerResponseResult;
        match self {
            TR::Success(tracker_response) => Ok(tracker_response),
            TR::Failure { failure_reason } => {
                anyhow::bail!(format!("{} (Tracker)", failure_reason))
            }
        }
    }
}
