mod cli;
mod disk_worker;
mod metainfo;
mod peer_protocol;
mod peers;
mod piece_picker;
mod prelude;
mod torrent;
mod tracker;

use clap::Parser;
use cli::Cli;
use piece_picker::{PiecePicker, PiecePickerPrototype};
use prelude::*;

use tokio::task;
use tokio_util::sync::CancellationToken;

use metainfo::{url::TrackerUrl, DownloadInfo};
use peers::{
    download_worker::{connect_and_handshake, PeerDownloadWorker},
    PeerAddr,
};
use torrent::{InfoHash, PeerId};

use tracker::{
    request::{Requestable, TrackerRequest},
    Announce, HttpTracker,
};

use tracing_flame::FlameLayer;
use tracing_subscriber::{filter, fmt, layer::SubscriberExt, registry::Registry, Layer};

#[tokio::main]
#[instrument(err, skip_all)]
async fn main() -> eyre::Result<()> {
    let fmt_layer = fmt::Layer::default()
        .pretty()
        .with_filter(filter::LevelFilter::TRACE);

    let (flame_layer, _flush_gaurd) =
        FlameLayer::with_file("./tracing.folded").expect("could not initialize flame layer");

    let subscriber = Registry::default().with(fmt_layer).with(flame_layer);

    tracing::subscriber::set_global_default(subscriber)
        .expect("could not set global tracing subscriber");

    tokio::select! {
        Ok(_) = tokio::signal::ctrl_c() => {Ok(())},
        result = run_app() => {result}
    }

    // let fmt_layer = tracing_subscriber::fmt::tracing_subscriber::fmt()
    //     .with_max_level(Level::INFO)
    //     .pretty()
    //     .with_target(false)
    //     .init();
}

async fn run_app() -> eyre::Result<()> {
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source)
        .await
        .context("read torrent file")?;

    let peer_id = PeerId::random();
    let request = TrackerRequest::new(peer_id.clone(), matches.port, &metainfo.file_info)?;
    let client = reqwest::Client::new();
    let response = match metainfo.announce {
        // TODO: handle udp trackers, BEP: https://www.bittorrent.org/beps/bep_0015.html
        #[allow(unused)]
        TrackerUrl::Udp(udp_url) => todo!(),
        TrackerUrl::Http(http_url) => {
            HttpTracker::new(&client, http_url)
                .announce(&request)
                .await?
        }
    };
    let (piece_length, piece_hashes, torrent_length) = match &metainfo.file_info {
        DownloadInfo::MultiFile {
            piece_length,
            pieces,
            files,
            ..
        } => {
            let torrent_length = files.iter().map(|f| f.length as u32).sum();
            (*piece_length as u32, pieces, torrent_length)
        }
        DownloadInfo::SingleFile {
            piece_length,
            pieces,
            length,
            ..
        } => (*piece_length as u32, pieces, *length as u32),
    };

    let npieces = piece_hashes.len();
    let piece_infos = piece_hashes
        .iter()
        .enumerate()
        .map(|(piece_id, piece_hash)| piece_picker::PieceInfo {
            piece_id,
            hash: *piece_hash,
            length: if piece_id == npieces - 1 {
                torrent_length % piece_length
            } else {
                piece_length
            },
        })
        .collect();

    let shutdown_token = CancellationToken::new();

    let info_hash = metainfo.file_info.get_info_hash()?;
    let (mut piece_picker, piece_picker_handle, done_notify) =
        PiecePicker::new(piece_infos, shutdown_token.clone());

    let piece_picker_join_handle = tokio::spawn(async move { piece_picker.run().await });

    let mut join_set = task::JoinSet::<eyre::Result<()>>::new();

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

    done_notify.notified().await;
    shutdown_token.cancel();

    // don't return error on this since some of the peers might have
    // errored out but it doesn't matter if we downloaded everything.
    join_set.join_all().await;

    // but this needs to be checked, since it could be that pieces were not flushed properly.
    piece_picker_join_handle.await??;
    Ok(())
}

#[instrument(
    level = "info",
    name = "peer worker",
    fields(peer = %peer_addr),
    skip_all,
    err,
)]
async fn spawn_peer(
    peer_addr: PeerAddr,
    piece_picker_proto: PiecePickerPrototype,
    shutdown_token: CancellationToken,
    info_hash: InfoHash,
    peer_id: PeerId,
) -> eyre::Result<()> {
    let connx = connect_and_handshake(peer_addr, info_hash, peer_id).await?;
    PeerDownloadWorker::start_from(connx, shutdown_token, piece_picker_proto).await
}
