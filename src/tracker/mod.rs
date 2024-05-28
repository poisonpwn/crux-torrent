pub mod request;
pub mod response;
pub mod udp;

use crate::metainfo::tracker_url::{HttpUrl, UdpUrl};
use anyhow::Context;
use rand::Rng;
use reqwest::Client as HttpClient;
use tokio::net::UdpSocket;

use request::TrackerRequest;
use udp::{UDPConnectRequest, UDPConnectResponse};

use self::response::{TrackerResponse, TrackerResponseResult};

pub struct UdpTracker<'a> {
    client: &'a UdpSocket,
    announce_url: UdpUrl,
}

pub struct HttpTracker<'a> {
    client: &'a HttpClient,
    announce_url: HttpUrl,
}

impl<'a> HttpTracker<'a> {
    pub fn new(client: &'a HttpClient, announce_url: HttpUrl) -> Self {
        Self {
            client,
            announce_url,
        }
    }
}

impl<'a> UdpTracker<'a> {
    pub fn new(client: &'a UdpSocket, announce_url: UdpUrl) -> Self {
        Self {
            client,
            announce_url,
        }
    }

    async fn connect(&self, addr: (&str, u16)) -> anyhow::Result<u64> {
        self.client.connect(addr).await?;

        let mut rng = rand::thread_rng();
        let transaction_id = rng.gen::<u32>();

        let request = UDPConnectRequest::new(transaction_id);
        let request_bytes = request.to_bytes().await?;
        loop {
            self.client.send(&request_bytes).await?;
            // NOTE: this assumes the packet size to be 16 for efficiency, which might break in future extensions of the spec (but this is quite unlikely)
            let mut response_bytes = vec![0; 16];
            let n = self.client.recv(&mut response_bytes).await?;
            if n == 16 {
                let response = UDPConnectResponse::from_bytes(response_bytes).await?;
                if response.transaction_id == request.transaction_id && response.action == 0 {
                    return Ok(response.connection_id);
                }
            }
        }
    }
}

pub trait Announce {
    async fn announce(self, request: &TrackerRequest) -> anyhow::Result<TrackerResponse>;
}

impl<'a> Announce for HttpTracker<'a> {
    async fn announce(self, request: &TrackerRequest) -> anyhow::Result<TrackerResponse> {
        let mut request_url = self.announce_url.into_inner();
        request_url.set_query(Some(&request.to_url_query()));
        let response = self.client.get(request_url).send().await?.bytes().await?;
        let response: TrackerResponseResult = serde_bencode::from_bytes(&response)?;
        response.into()
    }
}

impl<'a> Announce for UdpTracker<'a> {
    async fn announce(self, request: &TrackerRequest) -> anyhow::Result<TrackerResponse> {
        let tracker_url = &self.announce_url.0;
        let host = tracker_url
            .host_str()
            .context("Missing hostname in UDP tracker url")?;
        let port = tracker_url
            .port()
            .context("Missing port in UDP tracker url")?;
        let connection_id = self.connect((host, port)).await?;
        dbg!(connection_id);
        anyhow::bail!("UDP tracker is WIP")
    }
}
