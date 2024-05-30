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
use peer_protocol::{
    codec::{PeerMessage, PeerMessageCodec},
    handshake::PeerHandshake,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracker::request::Requestable;
use tracker::Announce;

use futures::sink::SinkExt;
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

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
        .map(|addr| tokio::net::TcpStream::connect(addr).boxed());

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
    let handshake = PeerHandshake::from_bytes(bytes);

    let mut framed_connection = Framed::new(connection, PeerMessageCodec);
    let msg = framed_connection.next().await;
    dbg!(&msg);
    framed_connection
        .send(PeerMessage::Unchoke)
        .await
        .context("sending unchoke")?;
    framed_connection
        .send(PeerMessage::Interested)
        .await
        .context("sending interested")?;

    let req_mesg = PeerMessage::Request {
        index: 1,
        begin: 0,
        length: 1 << 5,
    };
    let piece_mesg = loop {
        // unexpectedly finished
        let msg = framed_connection
            .next()
            .await
            .ok_or_else(|| anyhow::Error::msg("peer closed connection without giving a piece."))?;

        // keep alive
        let msg = match msg? {
            Some(msg) => msg,
            None => continue,
        };

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

            _ => panic!("dw bout it for now."),
        }
    };

    Ok(())
}
