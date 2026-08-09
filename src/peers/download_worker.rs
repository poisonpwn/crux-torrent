use futures::{SinkExt, StreamExt};
use sha1_smol::Sha1;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::peers::download_worker::errors::PeerShutdown;
use crate::piece_picker::{PieceHandle, PiecePickerHandle, PiecePickerPrototype};
use crate::prelude::*;
use crate::torrent::{InfoHash, PeerId};

use super::progress::PieceDownloadProgress;
use super::request_window::RequestWindow;
use crate::peers::PeerAddr;
use crate::torrent::PieceIndex;

use crate::peer_protocol::codec::{upgrade_stream, PeerFrames, PeerMessage, PeerStream};
use crate::peer_protocol::handshake::PeerHandshake;

#[derive(Debug)]
pub struct PeerDownloaderConnection<T: PeerStream> {
    peer_addr: PeerAddr,
    peer_id: PeerId,
    stream: T,
}

pub struct PeerDownloadWorker<T: PeerStream> {
    peer_id: PeerId,
    peer_stream: T,
    peer_is_choked: bool,
    we_are_interested: bool,
    shutdown_token: CancellationToken,
    // paces the request pipeline to this peer's measured speed rather than a fixed depth; lives
    // on the worker (not per-piece) so its rate estimate carries over across pieces.
    window: RequestWindow,
}

pub async fn connect_and_handshake(
    peer_addr: PeerAddr,
    info_hash: InfoHash,
    self_peer_id: PeerId,
) -> eyre::Result<PeerDownloaderConnection<PeerFrames<TcpStream>>> {
    info!("connecting to peer");
    let mut stream = TcpStream::connect(&peer_addr).await?;

    let handshake = PeerHandshake::new(info_hash, self_peer_id);
    let mut bytes = handshake.into_bytes();

    info!("sending handshake to peer");
    stream.write_all(&bytes).await?;

    info!("waiting for peer handshake");
    stream.read_exact(&mut bytes).await?;

    let handshake = PeerHandshake::from_bytes(bytes);
    info!("peer handshake received");
    debug!(peer_handshake_reply = ?handshake);

    let stream = upgrade_stream(stream);

    Ok(PeerDownloaderConnection {
        stream,
        peer_id: handshake.peer_id,
        peer_addr,
    })
}

impl<T> PeerDownloadWorker<T>
where
    T: PeerStream + Sync + Unpin,
{
    #[instrument(level = "debug", name = "worker loop", skip_all, fields(%peer_addr))]
    pub async fn start_from(
        PeerDownloaderConnection {
            peer_addr,
            stream: mut peer_stream,
            peer_id,
            ..
        }: PeerDownloaderConnection<T>,
        shutdown_token: CancellationToken,
        piece_picker_proto: PiecePickerPrototype,
    ) -> eyre::Result<()> {
        let mut peer_is_choked = true;

        type PM = PeerMessage;

        debug!("sending interested");
        peer_stream
            .send(PM::Interested)
            .await
            .wrap_err("error while sending interested")?;

        let mut piece_picker_handle = loop {
            let msg = peer_stream.next().await.ok_or_else(|| {
                error!("peer closed connection before bitfield message");
                self::errors::PeerConnClosedBeforeBitfield
            })??;

            match msg {
                PM::Bitfield(bitfield) => {
                    break PiecePickerHandle::init_with_bitfield(piece_picker_proto, bitfield)
                        .await
                        .map_err(|e| {
                            error!("piece picker handle initiliaztion failed");
                            e.wrap_err("failed to create piece picker handle")
                        })?
                }
                PM::Choke => {
                    info!("choke received");
                    peer_is_choked = true;
                }
                PM::Unchoke => {
                    info!("unchoke received");
                    peer_is_choked = false;
                }

                PM::Have(piece_id) => {
                    info!("received have message as first message!");
                    break PiecePickerHandle::init_with_have_id(
                        piece_picker_proto,
                        piece_id as usize,
                    )
                    .await?;
                }

                _ => {
                    let err_mesg = format!("first message sent by peer was not bitfield {:?}", msg);
                    error!(err_mesg);
                    eyre::bail!(err_mesg);
                }
            };
        };

        let mut worker = Self {
            peer_stream,
            peer_id,
            shutdown_token,
            peer_is_choked,
            we_are_interested: false,
            window: RequestWindow::new(),
        };

        loop {
            let res = worker.run(&mut piece_picker_handle).await;

            if res
                .as_ref()
                .is_err_and(|e| e.downcast_ref::<PeerShutdown>().is_some())
            {
                return Ok(());
            }

            res?
        }
    }

    #[instrument("download worker", level = "debug", skip_all)]
    async fn run(&mut self, piece_picker_handle: &mut PiecePickerHandle) -> eyre::Result<()> {
        debug!("worker fetching next piece");

        let piece_handle = tokio::select! {
            _ = self.shutdown_token.cancelled() => {
                info!("shutdown signal received, shutting down worker");
                return Err(self::errors::PeerShutdown.into());
            }

            next_piece_res = piece_picker_handle.next_piece() => next_piece_res.map_err(|err| {
                let mesg = "error while fetching next piece from piece picker";
                error!(mesg);
                err.wrap_err(mesg)
            })?
        };

        debug!("downloading piece {}", piece_handle.piece_id);
        if let Ok(piece) = self
            .download_piece(&piece_handle, piece_picker_handle)
            .await
        {
            info!(
                "download complete, submitting piece {}",
                piece_handle.piece_id
            );
            piece_handle.submit(piece).await?;
        } else {
            // note that download failure is not fatal to the worker
            warn!("error piece download failed, {}", piece_handle.piece_id);
        }

        Ok(())
    }

    #[instrument("download piece", level = "info", skip_all, fields(piece_id = piece_handle.piece_id))]
    pub async fn download_piece(
        &mut self,
        piece_handle: &PieceHandle,
        piece_picker_handle: &mut PiecePickerHandle,
    ) -> eyre::Result<Vec<u8>> {
        let mut progress = PieceDownloadProgress::new(piece_handle.piece_length);
        let mut piece = vec![0; piece_handle.piece_length as usize];

        while !progress.is_done() {
            if !self.we_are_interested {
                debug!("sending interested");
                self.peer_stream
                    .send(PeerMessage::Interested)
                    .await
                    .context("error while sending interested")?;
                self.we_are_interested = true;
            }

            if !self.peer_is_choked {
                while let Some((begin, length)) =
                    progress.next_block_info(self.window.desired_pending_blocks())
                {
                    let request = PeerMessage::Request {
                        index: piece_handle.piece_id as u32,
                        begin,
                        length,
                    };

                    debug!("sending request to peer {:?}", request);
                    self.peer_stream.send(request).await?;
                }
            }

            tokio::select! {
                msg = self.peer_stream.next() => {
                    let msg = match msg {
                        Some(msg) => msg?,
                        None => {
                            let err_mesg = "peer closed connection before piece could be downloaded";
                            warn!(err_mesg);
                            eyre::bail!(err_mesg);
                        }
                    };

                    self.handle_peer_message(msg, piece_handle.piece_id, &mut piece, &mut progress, piece_picker_handle).await?;
                }

                _ = self.shutdown_token.cancelled() => {
                    let err_mesg = "shutdown signal received shutting down worker";
                    info!(err_mesg);
                    eyre::bail!(err_mesg);
                }
            }
        }

        let piece_hash = Sha1::from(&piece).digest().bytes();
        if piece_hash != piece_handle.piece_hash {
            error!("downloaded piece hash check failed");
            eyre::bail!("piece hash check failed");
        }

        debug!("piece hash check succeeded");
        info!("piece download complete");
        Ok(piece)
    }

    async fn handle_peer_message(
        &mut self,
        msg: PeerMessage,
        piece_id: PieceIndex,
        mut piece: impl AsMut<[u8]>,
        // used to update the progress when a new block is received
        download_progress: &mut PieceDownloadProgress,
        // used to send piece frequency updates (have and bitfield) to piece picker
        piece_picker_handle: &mut PiecePickerHandle,
    ) -> eyre::Result<()> {
        type PM = PeerMessage;
        match msg {
            PM::Choke => {
                info!("peer choked");
                self.peer_is_choked = true;
                download_progress.reset_progress();
            }

            PM::Unchoke => {
                info!("peer unchoked");
                self.peer_is_choked = false;
            }

            PM::Piece {
                index: recv_index,
                begin,
                piece: block,
            } => {
                let block_span = debug_span!("handle block message", begin, index = recv_index);
                let _gaurd = block_span.enter();

                debug!("received block");

                if piece_id != recv_index as usize {
                    warn!("unrequested piece receieved from peer, curr piece id: {}, received piece id: {}", piece_id, recv_index);
                    return Ok(());
                }

                trace!(block_length = block.len());
                self.window.on_block_received(block.len() as u32);
                let _ = download_progress.update_downloaded(begin);

                trace!("writing block to piece");
                let begin = begin as usize;
                piece.as_mut()[(begin)..(begin + block.len())].copy_from_slice(&block);
            }

            PM::Have(piece_id) => {
                let span = debug_span!("handle have message", piece_id);
                let _guard = span.enter();
                info!("received have message");

                let _ = piece_picker_handle.have_piece(piece_id as usize).await;
            }

            PM::Bitfield(_) => {
                warn!("bitfield message received after first message");
            }

            mesg
            @ (PM::Cancel { .. } | PM::NotInterested | PM::Interested | PM::Request { .. }) => {
                warn!(
                    "received downloader side messages from peer while in inbound mode, {:?}",
                    mesg
                );
            }
        }
        Ok(())
    }
}

mod errors {
    use thiserror::Error;

    #[derive(Error, Debug)]
    #[error("peer closed connection before bitfield message was received")]
    pub struct PeerConnClosedBeforeBitfield;

    #[derive(Error, Debug)]
    #[error("received signal to shut down peer")]
    pub struct PeerShutdown;
}
