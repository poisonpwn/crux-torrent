use std::{
    collections::{btree_map::Entry, BTreeMap},
    sync::{Arc, Mutex, RwLock},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::peers::PieceIndex;
use crate::prelude::*;

use super::{PieceDone, PieceInfo, PiecePickerHandle, PieceQueue};

pub struct PiecePicker {
    piece_queue: Arc<RwLock<PieceQueue>>,
    piece_infos: Vec<PieceInfo>,
    start: PieceIndex,
    end: PieceIndex,
    piece_rx: mpsc::Receiver<PieceDone>,
    shutdown_token: CancellationToken,
    n_received: u32,
}

impl PiecePicker {
    const MAX_QUEUED: usize = 20;
    const PIECE_BUFFER_SIZE: usize = 10;

    pub fn new(
        piece_infos: Vec<PieceInfo>,
        shutdown_token: CancellationToken,
    ) -> (Self, PiecePickerHandle) {
        let (piece_tx, piece_rx) = mpsc::channel(Self::PIECE_BUFFER_SIZE);

        let piece_queue = Arc::new(RwLock::new(PieceQueue::new()));

        let piece_picker = Self {
            piece_infos,
            piece_queue: piece_queue.clone(),
            piece_rx,
            start: 0,
            end: 0,
            n_received: 0,
            shutdown_token,
        };

        let picker_handle = PiecePickerHandle::new(piece_queue, piece_tx);

        (piece_picker, picker_handle)
    }

    #[instrument("piece picker", fields(skip_all))]
    async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            if (self.n_received == self.piece_infos.len()) {
                info!("received all pieces, shutting down piece picker");
                return Ok(());
            }

            if self.piece_queue.read()?.is_empty() {
                debug!("piece queue empty, refilling piece queue.");
                let piece_queue = self.piece_queue.get_mut()?;
                let next_end = std::cmp::max(self.piece_infos.len(), self.end + Self::MAX_QUEUED);

                for piece_id in (self.end..next_end) {
                    piece_queue.insert(piece_id, self.piece_infos[piece_id]);
                }
                self.start = self.end;
                self.end = next_end;
            }

            tokio::select! {
                _ = self.shutdown_token.cancelled() => {
                    info!("received shutdown signal, shutting down piece picker");
                    return Ok(());
                }

                Some(PieceDone {
                    piece_id,
                    piece
                }) = self.piece_rx.recv() => {
                    debug!("receieved piece done {}", piece_id);

                    let piece_queue = self.piece_queue.get_mut()?;

                    match piece_queue.entry(piece_id) {
                        Entry::Vacant(_) => {
                            warn!("piece received from worker that was not in piece queue, start: {} end: {} receieved: {}", self.start, self.end, piece_id);
                        }
                        Entry::Occupied(e) => {
                        // TODO: queue the piece to be flushed to disk.
                            self.n_received += 1;
                            e.remove()
                        }
                    }
                }

                else => {
                    info!("all receivers closed, shutting down piece picker");
                    return Ok(());
                }
            }
        }
    }
}
