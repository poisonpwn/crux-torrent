use std::{
    collections::btree_map::Entry,
    sync::{Arc, Mutex, RwLock},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::peers::PieceIndex;
use crate::prelude::*;

use super::{PieceDone, PieceInfo, PiecePickerHandle, PieceQueue};

pub struct PiecePicker {
    piece_queue: Arc<PieceQueue>,
    piece_infos: Arc<Vec<PieceInfo>>,
    start: PieceIndex,
    end: PieceIndex,
    piece_rx: mpsc::Receiver<PieceDone>,
    shutdown_token: CancellationToken,
    n_received: u32,
}

impl PiecePicker {
    const MAX_QUEUED: usize = 100;
    const PIECE_BUFFER_SIZE: usize = 10;

    pub fn new(
        piece_infos: Vec<PieceInfo>,
        shutdown_token: CancellationToken,
    ) -> (Self, PiecePickerHandle) {
        let (piece_tx, piece_rx) = mpsc::channel(Self::PIECE_BUFFER_SIZE);

        let piece_queue = Arc::new(RwLock::new(PieceQueue::new()));
        let mut lock_pool = Vec::new();
        lock_pool.resize_with(piece_infos.len(), || Mutex::new(()));
        let lock_pool = Arc::new(lock_pool);

        let piece_picker = Self {
            piece_infos,
            piece_queue: piece_queue.clone(),
            piece_rx,
            start: 0,
            end: 0,
            n_received: 0,
            shutdown_token,
        };
        let picker_handle = PiecePickerHandle::new(piece_queue, lock_pool, piece_tx);

        (piece_picker, picker_handle)
    }

    #[instrument("piece picker", level = "debug", skip_all)]
    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            if self.n_received == (self.piece_infos.len() as u32) {
                info!("received all pieces, shutting down piece picker");
                return Ok(());
            }

            {
                let piece_queue = self.piece_queue.read().map_err(|_| {
                    error!("error encountered while reading piece queue");
                    anyhow::Error::msg("error encountered while reading piece queue")
                })?;

                if piece_queue.is_empty() {
                    let span = debug_span!("refill piece queue");
                    let _gaurd = span.enter();
                    debug!("piece queue empty, refilling piece queue.");
                    drop(piece_queue);

                    let mut piece_queue = self.piece_queue.write().map_err(|_| {
                        error!("error encountered while reading piece queue");
                        anyhow::Error::msg("error encountered while reading piece queue")
                    })?;

                    let next_end =
                        std::cmp::min(self.piece_infos.len(), self.end + Self::MAX_QUEUED);

                    debug!(
                        "extending piece queue pieces, curr end: {}, next end: {}",
                        self.end, next_end
                    );

                    for piece_id in self.end..next_end {
                        piece_queue.insert(piece_id, self.piece_infos[piece_id]);
                    }
                    self.start = self.end;
                    self.end = next_end;
                    drop(_gaurd);
                }
            }

            tokio::select! {
                _ = self.shutdown_token.cancelled() => {
                    info!("received shutdown signal, shutting down piece picker");
                    return Ok(());
                }

                Some(PieceDone {
                    piece_id,
                    piece: _piece,
                    notify
                }) = self.piece_rx.recv() => {
                    debug!("receieved piece done {}", piece_id);
                    // TODO flush the piece to disk here.

                    let mut piece_queue = self.piece_queue.write().map_err(|_| {
                        error!("error encountered while reading piece queue");
                        anyhow::Error::msg("error encountered while reading piece queue")
                    })?;

                    match piece_queue.entry(piece_id) {
                        Entry::Vacant(_) => {
                            warn!("piece received from worker that was not in piece queue, start: {} end: {} receieved: {}", self.start, self.end, piece_id);
                        }
                        Entry::Occupied(e) => {
                            // TODO: queue the piece to be flushed to disk.
                            debug!("receieved piece {piece_id}, incrementing n received");
                            self.n_received += 1;
                            e.remove();
                        }
                    }
                    debug!("notifying done piece: {}", piece_id);
                    notify.notify_one();
                }
            }
        }
    }
}
