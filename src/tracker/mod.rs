pub mod peers;
pub mod request;
pub mod response;

use reqwest::{Client, IntoUrl};
use tokio::net::UdpSocket;

enum NetworkClient {
    HTTP(Client),
    UDP(UdpSocket),
}

enum TrackerUrl {
    HTTP(String),
    UDP(String),
}

enum Tracker {
    UDP { socket: UdpSocket, url: String },
    HTTP { client: Client, url: String },
}

impl TrackerUrl {
    // can't do  impl IntoUrl + ToString as Url itself doens't implement ToString.
    fn new(url: impl IntoUrl) -> anyhow::Result<Self> {
        let url = url.into_url()?;
        Ok(match url.scheme() {
            "http" => Self::HTTP(url.as_str().to_string()),
            "udp" => Self::UDP(url.as_str().to_string()),
            scheme @ _ => anyhow::bail!(format!("unsupported scheme {:?} for tracker", scheme)),
        })
    }
}

impl Tracker {
    fn new(tracker_url: TrackerUrl, client: NetworkClient) -> anyhow::Result<Self> {
        Ok(match (tracker_url, client) {
            (TrackerUrl::HTTP(url), NetworkClient::HTTP(client)) => Self::HTTP { client, url },
            (TrackerUrl::UDP(url), NetworkClient::UDP(socket)) => Self::UDP { socket, url },
            (_, _) => anyhow::bail!(format!("mismatched client for url scheme")),
        })
    }

    async fn announce_http(client: Client, url: String) -> TrackerResponse {
        todo!()
    }

    async fn announce_udp(socket: UdpSocket, url: String) -> TrackerResponse {
        todo!()
    }

    async fn announce(self) {
        match self {
            Self::HTTP { url, client } => Self::announce_http(client, url).await,
            Self::UDP { url, socket } => Self::announce_udp(socket, url).await,
        }
    }
}
