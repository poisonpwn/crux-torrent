mod cli;
mod metainfo;
mod peer_protocol;
mod peers;
mod prelude;
mod torrent;
mod tracker;

use crate::torrent::PeerId;
use anyhow::Context;
use clap::Parser;
use cli::Cli;
use futures::future::FutureExt;
use futures::SinkExt;
use metainfo::tracker_url::TrackerUrl;
use peer_protocol::{
    codec::{PeerMessage, PeerMessageCodec},
    handshake::PeerHandshake,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;
use tracker::{
    request::{Requestable, TrackerRequest},
    Announce, HttpTracker,
};

use prelude::*;
use tracing::Level;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .pretty()
        .with_target(false)
        .init();
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source)?;

    let peer_id = PeerId::random();
    let request = TrackerRequest::new(peer_id.clone(), matches.port, &metainfo.file_info)?;

    dbg!(&request);

    let client = reqwest::Client::new();
    let response = match metainfo.announce {
        // TODO: handle udp trackers, BEP: https://www.bittorrent.org/beps/bep_0015.html
        TrackerUrl::Udp(udp_url) => todo!(),
        TrackerUrl::Http(http_url) => {
            HttpTracker::new(&client, http_url)
                .announce(&request)
                .await?
        }
    };

    let connections = response
        .peer_addreses
        .iter()
        .map(|addr| tokio::net::TcpStream::connect(addr).boxed());

    //  CHECK: does the remaining futures being forgotten cause problems.
    let (mut connection, _remaining_futures) = futures::future::select_ok(connections)
        .await
        .context("all peers failed to connect")?;

    let info_hash = metainfo.file_info.get_info_hash()?;
    let mut bytes = PeerHandshake::new(info_hash, peer_id.clone()).into_bytes();
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
    let handshake = PeerHandshake::from_bytes(bytes);
    dbg!(&handshake);

    let mut framed_connection = Framed::new(connection, PeerMessageCodec);
    let msg = framed_connection.next().await;
    dbg!(&msg);
    framed_connection.send(PeerMessage::Unchoke).await?;
    framed_connection.send(PeerMessage::Interested).await?;

    let req_mesg = PeerMessage::Request {
        index: 1,
        begin: 0,
        length: 1 << 5,
    };

    //FIXME: fix shitty temp test code dw bout it for now, write integration tests.
    let _piece_mesg = loop {
        // unexpectedly finished
        let msg = match framed_connection.next().await {
            Some(msg_res) => msg_res?,
            None => anyhow::bail!("peer closed connection before giving a piece"),
        };

        // keep alive
        dbg!(&msg);

        type PM = PeerMessage;
        match msg {
            PM::Choke => {
                continue;
            }
            PM::Unchoke => {
                framed_connection
                    .send(req_mesg.clone())
                    .await
                    .context("sending request for index 1")?;

                continue;
            }
            piece @ PM::Piece { .. } => {
                break piece;
            }

            // all this code in main should be nuked.
            _ => panic!("peer shouldn't send anything other than these, but dw it for now."),
        }
    };

    Ok(())
}
