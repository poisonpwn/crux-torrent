mod cli;
mod metainfo;
mod peer_protocol;
mod peers;
mod prelude;
mod torrent;
mod tracker;

use clap::Parser;
use cli::Cli;
use peer_protocol::handshake::PeerHandshake;
use prelude::*;

use tokio::sync::mpsc;
use tokio::task;
use tracing::Level;

use metainfo::{url::TrackerUrl, DownloadInfo};
use peers::{
    download_worker::{PeerConnector, PeerDownloadWorker},
    PeerAlerts, PeerCommands, PieceRequestInfo,
};
use tokio::net::TcpStream;
use torrent::{Bitfield, InfoHash, PeerId};

use std::net::SocketAddrV4;

use tracker::{
    request::{Requestable, TrackerRequest},
    Announce, HttpTracker,
};

struct PeerSession {
    peer_addr: std::net::SocketAddrV4,
    bitfield: Bitfield,
    commands_tx: mpsc::Sender<PeerCommands>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .pretty()
        .with_target(false)
        .init();
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source).await?;

    let peer_id = PeerId::with_random_suffix();
    let request = TrackerRequest::new(peer_id.clone(), matches.port, &metainfo.file_info)?;
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
    let piece_index = 0;
    let (length, hash) = match &metainfo.file_info {
        DownloadInfo::MultiFile {
            piece_length,
            pieces,
            ..
        } => (*piece_length as u32, pieces[piece_index]),
        DownloadInfo::SingleFile {
            piece_length,
            pieces,
            ..
        } => (*piece_length as u32, pieces[piece_index]),
    };

    let piece_request_info = PieceRequestInfo::new(piece_index, length, hash);

    let info_hash = metainfo.file_info.get_info_hash()?;
    let mut join_set = task::JoinSet::<anyhow::Result<()>>::new();

    let (alerts_tx, alerts_rx) = mpsc::channel::<PeerAlerts>(100);
    let mut abort_handles = Vec::new();
    for addr in &response.peer_addreses {
        let addr = *addr;
        let info_hash = info_hash.clone();
        let peer_id = peer_id.clone();
        let alerts_channel = alerts_tx.clone();

        let handle = join_set.spawn(spawn_peer(addr, alerts_channel, info_hash, peer_id));

        abort_handles.push(handle);
    }

    let mut engine_handle = task::spawn(engine(alerts_rx, piece_request_info));
    loop {
        tokio::select! {
            Some(result) = join_set.join_next() => {
                eprintln!("{:?}", result);
            }

            _ = &mut engine_handle => {
                break;
            }
        }
    }
    Ok(())
}

#[instrument(
    level = "info",
    name = "peer worker",
    fields(peer = %peer_addr),
    skip_all
)]
async fn spawn_peer(
    peer_addr: SocketAddrV4,
    alerts_channel: mpsc::Sender<PeerAlerts>,
    info_hash: InfoHash,
    peer_id: PeerId,
) -> anyhow::Result<()> {
    let connx = PeerConnector::connect(peer_addr).await?;
    let mut worker: PeerDownloadWorker<TcpStream> = PeerDownloadWorker::init_from(
        connx
            .handshake(PeerHandshake::new(info_hash, peer_id))
            .await?,
        alerts_channel,
    )
    .await?;
    worker.start_peer_event_loop().await?;
    Ok(())
}

#[instrument(level = "info", name = "engine", skip(alerts_rx))]
async fn engine(
    mut alerts_rx: mpsc::Receiver<PeerAlerts>,
    request_info: PieceRequestInfo,
) -> anyhow::Result<()> {
    let mut peers_listen = Vec::new();
    loop {
        let alert = match alerts_rx.recv().await {
            Some(alert) => alert,
            None => anyhow::bail!("all peers closed down"),
        };

        type PA = PeerAlerts;
        match alert {
            PA::InitPeer {
                peer_addr,
                bitfield,
                commands_tx,
            } => {
                let span = info_span!("processing init from {peer}", peer = peer_addr.to_string());
                let _gaurd = span.enter();
                info!("received init peer");
                if bitfield[request_info.index] {
                    commands_tx
                        .send(PeerCommands::DownloadPiece(request_info.clone()))
                        .await?;
                };
                peers_listen.push(PeerSession {
                    peer_addr,
                    bitfield,
                    commands_tx,
                });
            }
            PA::DonePiece {
                piece_index,
                piece: _,
            } => {
                info!(piece_index, "received piece done");
                break;
            }
            alert => {
                println!("{:?}", alert);
                todo!();
            }
        }
    }
    Ok(())
}
