use crate::metainfo::files::PieceHash;
use crate::peer_protocol::codec::PeerMessage;
use crate::prelude::*;
use futures::SinkExt;
use sha1_smol::Sha1;
use tokio_stream::StreamExt;

use super::descriptor::WorkerStateDescriptor;
use super::progress::PieceDownloadProgress;
use super::{PeerAlerts, PeerCommands, PieceIndex, PieceRequestInfo};

#[derive(Debug, Clone)]
pub enum WorkerState {
    WaitingforPiece {
        index: PieceIndex,
        download_progress: PieceDownloadProgress,
        hash: PieceHash,
        piece: Vec<u8>,
    },
    Idle,
}

impl WorkerState {
    pub async fn transition(
        &mut self,
        descriptor: &mut WorkerStateDescriptor,
    ) -> anyhow::Result<()> {
        match self {
            Self::WaitingforPiece {
                index,
                download_progress,
                hash,
                piece: piece_vec,
            } => {
                let WorkerStateDescriptor {
                    alerts_tx,
                    peer_is_choked,
                    we_are_interested,
                    peer_stream,
                    commands_rx,
                    ..
                } = descriptor;
                let span = info_span!("waiting for piece", index);
                let _gaurd = span.enter();

                if download_progress.is_done() {
                    info!("piece download complete");

                    let piece_hash = Sha1::from(&piece_vec).digest().bytes();
                    if piece_hash != *hash {
                        warn!("downloaded piece hash check failed");
                        //TODO: what should happen if hash fails?.
                    } else {
                        info!("piece hash check succeeded.");
                    }

                    info!("send piece done");
                    alerts_tx
                        .send(PeerAlerts::DonePiece {
                            piece_index: *index,
                            piece: std::mem::take(piece_vec),
                        })
                        .await?;

                    info!("set peer state idle");
                    *self = WorkerState::Idle;
                    return Ok(());
                }

                if !*we_are_interested {
                    *we_are_interested = true;

                    info!("sending unchoke");
                    peer_stream.send(PeerMessage::Unchoke).await?;
                    info!("sending interested");
                    peer_stream.send(PeerMessage::Interested).await?;
                }

                if !*peer_is_choked {
                    while let Some((begin, length)) = download_progress.next_block_info() {
                        let request = PeerMessage::Request {
                            index: *index as u32,
                            begin,
                            length,
                        };

                        info!("sending request to peer {:?}", request);
                        peer_stream.send(request).await?;
                    }
                }

                tokio::select! {
                    msg = peer_stream.next() => {
                        let msg = match msg {
                            Some(msg) => msg?,
                            None => {
                                warn!("peer closed connection before piece could be downloaded");
                                anyhow::bail!("peer closed connection before piece could be downloaded");
                            }
                        };

                        Self::handle_peer_message(msg, *index, descriptor, piece_vec, download_progress).await?;
                    }

                    // handle commands sent by the engine
                    Some(command) = commands_rx.recv() => {
                        Self::handle_command(command, descriptor).await?;
                    }

                    else => {
                        // commands channel closed and peer connection was closed
                        info!("engine and peer shut down, shutting down worker");
                        anyhow::bail!("shutting down.");
                    }
                }
            }

            Self::Idle => {
                let WorkerStateDescriptor {
                    download_queue,
                    commands_rx,
                    ..
                } = descriptor;

                if let Some(PieceRequestInfo {
                    index,
                    length,
                    hash,
                }) = download_queue.pop_front()
                {
                    info!("change peer state to waiting for piece {}", index);
                    *self = Self::WaitingforPiece {
                        index,
                        download_progress: PieceDownloadProgress::new(length),
                        hash,
                        piece: Vec::new(),
                    };
                    return Ok(());
                }

                info!("queue empty awaiting next command");
                match commands_rx.recv().await {
                    Some(command) => Self::handle_command(command, descriptor).await?,
                    None => {
                        anyhow::bail!("commands channel closed while Peer was idle, shutting down")
                    }
                };
            }
        }
        Ok(())
    }

    async fn handle_command(
        command: PeerCommands,
        WorkerStateDescriptor {
            peer_stream,
            we_are_interested,
            download_queue,
            ..
        }: &mut WorkerStateDescriptor,
    ) -> anyhow::Result<()> {
        type PC = PeerCommands;

        match command {
            PC::NotInterested => {
                info!("sending NotInterested to peer");
                peer_stream.send(PeerMessage::NotInterested).await?;
                *we_are_interested = false;
            }
            PC::Shutdown => {
                info!("received shutdown signal, shutting down");
                anyhow::bail!("received shutdown signal");
            }
            PC::DownloadPiece(req_info) => {
                info!(
                    "received DownloadPiece, appending piece to download queue {index}",
                    index = req_info.index
                );
                download_queue.push_back(req_info);
            }
        }
        Ok(())
    }

    async fn handle_peer_message(
        msg: PeerMessage,
        curr_piece_index: usize,
        WorkerStateDescriptor {
            peer_is_choked,
            peer_addr,
            alerts_tx,
            ..
        }: &mut WorkerStateDescriptor,
        piece: &mut Vec<u8>,
        download_progress: &mut PieceDownloadProgress,
    ) -> anyhow::Result<()> {
        type PM = PeerMessage;
        match msg {
            PM::Choke => {
                info!("peer choked");
                *peer_is_choked = true;
                download_progress.reset_progress();
            }
            PM::Unchoke => {
                info!("peer unchoked");
                *peer_is_choked = false;
            }

            PM::Piece {
                index: recv_index,
                begin,
                piece: block,
            } => {
                let block_span = debug_span!("handle block message", begin, index = recv_index);
                let _gaurd = block_span.enter();

                info!("received block");
                assert!(recv_index == curr_piece_index as u32);

                debug!(block_length = block.len());

                download_progress.update_downloaded(begin, block.len() as u32)?;

                trace!("appending block onto piece");
                piece.extend(block);
                debug!(new_piece_length = piece.len());
            }
            PM::Have(piece_index) => {
                let span = debug_span!("handle have message", piece_index);
                let _guard = span.enter();
                info!("received have message");

                info!("sending update bitfield to engine");
                alerts_tx
                    .send(PeerAlerts::UpdateBitfield {
                        has_piece: piece_index as usize,
                        peer_addr: *peer_addr,
                    })
                    .await?
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
