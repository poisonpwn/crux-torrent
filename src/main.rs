mod cli;
mod metainfo;
mod peer_protocol;
mod peers;
mod piece_picker;
mod prelude;
mod torrent;
mod tracker;

use clap::Parser;
use cli::Cli;
use piece_picker::{PiecePicker, PiecePickerHandle};
use prelude::*;

use tokio::sync::mpsc;
use tokio::task;
use tokio_util::sync::CancellationToken;
use tracing::Level;

use metainfo::{url::TrackerUrl, DownloadInfo};
use peers::download_worker::{PeerAddr, PeerDownloadWorker};
use torrent::{InfoHash, PeerId};

use std::{net::SocketAddrV4, sync::Arc};

use tracker::{
    request::{Requestable, TrackerRequest},
    Announce, HttpTracker,
};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .pretty()
        .with_target(false)
        .init();
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source).await?;

    let peer_id = PeerId::random();
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
    let (length, piece_hashes) = match &metainfo.file_info {
        DownloadInfo::MultiFile {
            piece_length,
            pieces,
            ..
        } => (*piece_length as u32, pieces),
        DownloadInfo::SingleFile {
            piece_length,
            pieces,
            ..
        } => (*piece_length as u32, pieces),
    };

    let piece_infos = piece_hashes
        .iter()
        .enumerate()
        .map(|(piece_id, piece_hash)| piece_picker::PieceInfo {
            piece_id,
            hash: *piece_hash,
            length,
        })
        .collect();

    let shutdown_token = CancellationToken::new();

    let info_hash = metainfo.file_info.get_info_hash()?;
    let (mut piece_picker, piece_picker_handle) =
        PiecePicker::new(piece_infos, shutdown_token.clone());

    let piece_picker_handle = Arc::new(piece_picker_handle);
    let mut join_set = task::JoinSet::<anyhow::Result<()>>::new();

    let mut abort_handles = Vec::new();
    for addr in &response.peer_addreses {
        let addr = *addr;
        let info_hash = info_hash.clone();
        let peer_id = peer_id.clone();
        let peer_shutdown_token = shutdown_token.child_token();

        let handle = join_set.spawn(spawn_peer(
            addr,
            piece_picker_handle.clone(),
            peer_shutdown_token,
            info_hash,
            peer_id,
        ));

        abort_handles.push(handle);
    }

    piece_picker.run().await?;
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
    piece_picker_handle: Arc<PiecePickerHandle>,
    shutdown_token: CancellationToken,
    info_hash: InfoHash,
    peer_id: PeerId,
) -> anyhow::Result<()> {
    let connx = PeerAddr::new(peer_addr);
    let mut worker =
        PeerDownloadWorker::init_from(connx.handshake(info_hash, peer_id).await?, shutdown_token)
            .await?;
    worker.start_peer_event_loop(piece_picker_handle).await?;
    Ok(())
}
