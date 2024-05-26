mod cli;
mod metainfo;
mod peer_protocol;
mod tracker;

use cli::Cli;

use clap::Parser;
use tracker::{
    request::{PeerId, TrackerRequest},
    HttpTracker,
};

use anyhow::Context;
use futures::future::FutureExt;
use metainfo::tracker_url::TrackerUrl;
use peer_protocol::handshake::PeerHandshake;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracker::request::Requestable;
use tracker::Announce;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source)?;
    let peer_id = PeerId::random();
    let request = TrackerRequest::new(peer_id.clone(), matches.port, &metainfo.file_info)?;

    dbg!(&request);

    let client = reqwest::Client::new();
    let response = match metainfo.announce {
        // TODO: handle udp trackers, BEP: https://www.bittorrent.org/beps/bep_0015.html
        TrackerUrl::UDP(udp_url) => todo!(),
        TrackerUrl::HTTP(http_url) => {
            HttpTracker::new(&client, http_url)
                .announce(&request)
                .await?
        }
    };

    dbg!(&response);

    let connections = response
        .peer_addreses
        .iter()
        .map(|addr| tokio::net::TcpStream::connect(addr).boxed())
        .into_iter();

    //  CHECK: does the remainint futures being forgotten cause problems.
    let (mut connection, _remaining_futures) = futures::future::select_ok(connections)
        .await
        .context("all peers failed to connect")?;

    let info_hash = metainfo.file_info.get_info_hash()?;
    let mut handshake = PeerHandshake::new(info_hash, peer_id.clone());

    let mut bytes = handshake.into_bytes();
    dbg!(bytes
        .clone()
        .iter()
        .map(|byte| *byte as char)
        .collect::<Vec<_>>());

    connection.write_all(&bytes).await.context(format!(
        "write handshake bytes to peer {:?}",
        &connection.peer_addr()
    ))?;

    connection.read_exact(&mut bytes).await.context(format!(
        "write handshake bytes to peer {:?}",
        &connection.peer_addr()
    ))?;
    handshake = PeerHandshake::from_bytes(bytes);
    dbg!(&handshake);

    Ok(())
}
