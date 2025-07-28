use futures::{SinkExt, StreamExt};
use sha1_smol::Sha1;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::piece_picker::{PieceHandle, PiecePickerHandle};
use crate::prelude::*;
use crate::torrent::{Bitfield, InfoHash, PeerId};
use std::net::SocketAddrV4;

use super::progress::PieceDownloadProgress;
use super::PieceIndex;

use crate::peer_protocol::codec::{self, PeerFrames, PeerMessage};
use crate::peer_protocol::handshake::PeerHandshake;

#[derive(Debug, Clone)]
pub struct PeerAddr {
    peer_addr: SocketAddrV4,
}

/// interface type between PeerAddr and PeerDownloadWorker
#[derive(Debug)]
pub struct PeerDownloaderConnection {
    peer_addr: SocketAddrV4,
    peer_id: PeerId,
    stream: TcpStream,
}

pub struct PeerDownloadWorker {
    peer_addr: SocketAddrV4,
    peer_id: PeerId,
    bitfield: Bitfield,
    peer_stream: PeerFrames<TcpStream>,
    peer_is_choked: bool,
    we_are_interested: bool,
    shutdown_token: CancellationToken,
}

impl PeerAddr {
    pub fn new(peer_addr: SocketAddrV4) -> Self {
        Self { peer_addr }
    }

    #[instrument(name = "handshake mode", level = "info", skip_all)]
    pub async fn handshake(
        self,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> anyhow::Result<PeerDownloaderConnection> {
        info!("connecting to peer");
        let mut stream = TcpStream::connect(&self.peer_addr).await?;

        let handshake = PeerHandshake::new(info_hash, peer_id);
        let mut bytes = handshake.into_bytes();

        info!("sending handshake to peer");
        stream.write_all(&bytes).await?;

        info!("waiting for peer handshake");
        stream.read_exact(&mut bytes).await?;

        let handshake = PeerHandshake::from_bytes(bytes);
        info!("peer handshake received");
        debug!(peer_handshake_reply = ?handshake);

        Ok(PeerDownloaderConnection {
            stream,
            peer_id: handshake.peer_id,
            peer_addr: self.peer_addr,
        })
    }
}

impl PeerDownloadWorker {
    pub async fn init_from(
        PeerDownloaderConnection {
            stream,
            peer_id,
            peer_addr,
            ..
        }: PeerDownloaderConnection,
        shutdown_token: CancellationToken,
    ) -> anyhow::Result<Self> {
        let mut peer_stream = codec::upgrade_stream(stream);

        let msg = match peer_stream.next().await {
            Some(msg_res) => msg_res?,
            None => {
                warn!("peer closed connection before handshake");
                anyhow::bail!("peer closed connection before handshake");
            }
        };

        type PM = PeerMessage;
        let bitfield = match msg {
            PM::Bitfield(bitfield) => bitfield,
            _ => {
                warn!("first message sent by peer was not a bitfield");
                anyhow::bail!("first message sent by peer not bitfield {:?}", msg);
            }
        };

        Ok(Self {
            peer_stream,
            peer_addr,
            peer_id,
            bitfield,
            shutdown_token,
            peer_is_choked: true,
            we_are_interested: false,
        })
    }

    #[instrument("download worker", level = "debug", skip_all)]
    async fn run(&mut self, piece_picker_handle: &PiecePickerHandle) -> anyhow::Result<()> {
        debug!("worker fetching next piece");

        let piece_handle = {
            tokio::select! {
                _ = self.shutdown_token.cancelled() => {
                    info!("shutdown signal received, shutting down peer");
                    anyhow::bail!("shutdown signal received, shutting down peer: {:?}", self.peer_id);
                }

                next_piece_res = piece_picker_handle.next_piece(self.bitfield.as_bitslice()) => { match next_piece_res {
                        Ok(next_piece) => next_piece,
                        Err(_) => {
                            error!("error while fetching next piece from piece picker");
                            anyhow::bail!("error while fetching next piece from piece picker: {:?}", self.peer_id);
                        }
                    }
                }
            }
        };

        if let Ok(piece) = self.download_piece(&piece_handle).await {
            debug!(
                "download complete, submitting piece {}",
                piece_handle.piece_id
            );
            piece_handle.submit(piece).await?;
        } else {
            self.bitfield.set(piece_handle.piece_id, false);
            warn!("error piece download failed, {}", piece_handle.piece_id); // download failure is not fatal
        }

        Ok(())
    }

    #[instrument("download piece", level = "info", skip_all, fields(piece_id = piece_handle.piece_id))]
    pub async fn download_piece(
        &mut self,
        piece_handle: &PieceHandle<'_>,
    ) -> anyhow::Result<Vec<u8>> {
        let mut progress = PieceDownloadProgress::new(piece_handle.piece_length);
        let mut piece = Vec::new();

        while !progress.is_done() {
            if !self.we_are_interested {
                info!("sending unchoke");
                self.peer_stream.send(PeerMessage::Unchoke).await?;
                info!("sending interested");
                self.peer_stream.send(PeerMessage::Interested).await?;
                self.we_are_interested = true;
            }

            if !self.peer_is_choked {
                while let Some((begin, length)) = progress.next_block_info() {
                    let request = PeerMessage::Request {
                        index: piece_handle.piece_id as u32,
                        begin,
                        length,
                    };

                    info!("sending request to peer {:?}", request);
                    self.peer_stream.send(request).await?;
                }
            }

            tokio::select! {
                msg = self.peer_stream.next() => {
                    let msg = match msg {
                        Some(msg) => msg?,
                        None => {
                            warn!("peer closed connection before piece could be downloaded");
                            anyhow::bail!("peer closed connection before piece could be downloaded");
                        }
                    };

                    self.handle_peer_message(msg, piece_handle.piece_id, &mut piece, &mut progress).await?;
                }

                _ = self.shutdown_token.cancelled() => {
                    info!("shutdown signal received shutting down worker");
                    anyhow::bail!("shutting down.");
                }
            }
        }
        let piece_hash = Sha1::from(&piece).digest().bytes();
        if piece_hash != piece_handle.piece_hash {
            warn!("downloaded piece hash check failed");
            anyhow::bail!("piece hash check failed");
        }

        info!("piece hash check succeeded");
        debug!("piece download complete");
        Ok(piece)
    }

    async fn handle_peer_message(
        &mut self,
        msg: PeerMessage,
        piece_id: PieceIndex,
        piece: &mut Vec<u8>,
        download_progress: &mut PieceDownloadProgress,
    ) -> anyhow::Result<()> {
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
                    error!("unrequested piece receieved from peer");
                    anyhow::bail!("unrequested piece received from peer");
                }

                trace!(block_length = block.len());

                download_progress.update_downloaded(begin, block.len() as u32)?;

                trace!("appending block onto piece");
                piece.extend(block);
                trace!(new_piece_length = piece.len());
            }
            PM::Have(piece_index) => {
                let span = debug_span!("handle have message", piece_index);
                let _guard = span.enter();
                info!("received have message");

                self.bitfield.set(piece_index as usize, true);
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

    pub async fn start_peer_event_loop(
        &mut self,
        piece_picker_handle: impl AsRef<PiecePickerHandle>,
    ) -> anyhow::Result<()> {
        loop {
            self.run(piece_picker_handle.as_ref()).await?;
        }
    }
}
