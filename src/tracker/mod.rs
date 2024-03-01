pub mod peers;
pub mod request;
pub mod response;

/*
struct Tracker {}
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
} */
