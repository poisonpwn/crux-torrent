pub mod request;
pub mod response;

use crate::metainfo::url::HttpUrl;
use crate::prelude::*;
use reqwest::Client as HttpClient;

use request::TrackerRequest;

use self::response::{TrackerResponse, TrackerResponseResult};

// #[derive(Debug, Clone)]
// pub struct UdpTracker<'a> {
//     client: &'a UdpSocket,
//     announce_url: UdpUrl,
// }

#[derive(Debug, Clone)]
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
    type Error;
    async fn announce(self, request: &TrackerRequest) -> Result<TrackerResponse, Self::Error>;
}

impl<'a> Announce for HttpTracker<'a> {
    type Error = eyre::Error;
    async fn announce(self, request: &TrackerRequest) -> Result<TrackerResponse, Self::Error> {
        let mut request_url = self.announce_url.into_inner();
        request_url.set_query(Some(&request.to_url_query()));
        let response = self.client.get(request_url).send().await?.bytes().await?;
        let response: TrackerResponseResult = serde_bencode::from_bytes(&response)?;
        response.into()
    }
}

// impl<'a> Announce for UdpTracker<'a> {
//     async fn announce(self, request: &TrackerRequest) -> anyhow::Result<TrackerResponse> {
//         todo!()
//     }
// }
