pub mod peers;
pub mod request;
pub mod response;

use crate::metainfo::tracker_url::{HttpUrl, UdpUrl};
use reqwest::Client as HttpClient;
use tokio::net::UdpSocket;

use request::TrackerRequest;

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
        todo!()
    }
}
