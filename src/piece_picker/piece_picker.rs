use simple_semaphore::Semaphore;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::prelude::*;

use super::{
    comms::PiecePickerMessage, PieceDone, PieceFreq, PieceInfo, PieceKey, PiecePickerPrototype,
    PieceQueue,
};

use tokio::sync::Notify;

/// this should be run as a task. It manages which pieces should be downladed, and so contains all
/// the frequency information about all the pieces. each worker has a [super::PiecePickerHandle] which
/// handles communication of piece frequency and download completion information between the worker
/// and piece picker.
pub struct PiecePicker {
    // pieces that need to be downloaded in an ordered iterable data structure
    piece_queue: Arc<PieceQueue>,
    // handle to receive availability and piece update messsages from workers
    piece_rx: mpsc::Receiver<PiecePickerMessage>,
    // lookup from PieceIndex -> PieceFreq
    piece_freq: Vec<PieceFreq>,
    shutdown_token: CancellationToken,
    done_notify: Arc<Notify>,
    n_received: u32,
    npieces: u32,
}

impl PiecePicker {
    const PIECE_BUFFER_SIZE: usize = 50;

    pub fn new(
        piece_infos: Vec<PieceInfo>,
        shutdown_token: CancellationToken,
    ) -> (Self, PiecePickerPrototype, Arc<Notify>) {
        let (piece_tx, piece_rx) = mpsc::channel(Self::PIECE_BUFFER_SIZE);

        let lock_pool = {
            let mut pool = Vec::new();
            pool.resize_with(piece_infos.len(), || Semaphore::new(1));
            Arc::new(pool)
        };

        let npieces = piece_infos.len() as u32;
        let piece_freq = vec![0; npieces as usize];

        let piece_queue = PieceQueue::new();
        let piece_infos = Arc::new(piece_infos);
        let done_notify = Arc::new(Notify::new());

        for (piece_id, _) in piece_freq.iter().enumerate() {
            piece_queue.insert(PieceKey { freq: 0, piece_id });
        }

        let piece_queue = Arc::new(piece_queue);

        let piece_picker = Self {
            piece_queue: piece_queue.clone(),
            npieces,
            piece_rx,
            done_notify: Arc::clone(&done_notify),
            piece_freq,
            n_received: 0,
            shutdown_token,
        };
        let picker_handle =
            PiecePickerPrototype::new(piece_queue, lock_pool, piece_infos, piece_tx);

        (piece_picker, picker_handle, done_notify)
    }

    #[instrument("piece picker", level = "debug", skip_all)]
    pub async fn run(&mut self) -> eyre::Result<()> {
        loop {
            if self.n_received == self.npieces {
                info!(
                    "received all pieces ({} out of {}), shutting down piece picker",
                    self.n_received, self.npieces,
                );
                self.done_notify.notify_one();
                return Ok(());
            }

            tokio::select! {
                _ = self.shutdown_token.cancelled() => {
                    info!("received shutdown signal, shutting down piece picker");
                    return Ok(());
                }

                Some(msg) = self.piece_rx.recv() => {
                    debug!("received piece picker message");
                    self.handle_message(msg)?;
                }
            }
        }
    }

    fn handle_message(&mut self, piece_picker_message: PiecePickerMessage) -> eyre::Result<()> {
        type PM = PiecePickerMessage;
        match piece_picker_message {
            PM::Init(bitfield) => {
                for piece_id in bitfield.iter_ones() {
                    let freq = self.piece_freq[piece_id];
                    if self
                        .piece_queue
                        .remove(&PieceKey { freq, piece_id })
                        .is_none()
                    {
                        // piece was done and removed from the queue (or, there was a freq mismatch,
                        // this shouldn't happen because the piece queue is only modified within
                        // this task, which is "single threaded", so there should no concurrent
                        // modification of the frequency information).

                        debug_assert!(
                            !self
                                .piece_queue
                                .iter()
                                .any(|item| item.piece_id == piece_id),
                            "found piece in piece queue with but mismatching frequency. piece_queue \
                             freq out of sync with frequency table"
                        );
                        continue;
                    };

                    self.piece_queue.insert(PieceKey {
                        freq: freq + 1,
                        piece_id,
                    });

                    self.piece_freq[piece_id] += 1;
                }
            }

            PM::PieceDone(PieceDone {
                piece_id,
                // TODO: remove directive once flush to disk is implemented correctly
                #[allow(unused)]
                piece,
                _piece_gaurd,
            }) => {
                debug!("receieved piece done {}", piece_id);

                //
                // TODO flush the piece to disk here or pass it on to disk io manager.
                //

                let freq = self.piece_freq[piece_id];

                if self
                    .piece_queue
                    .remove(&PieceKey { freq, piece_id })
                    .is_some()
                {
                    debug!("receieved piece {piece_id}, incrementing n received");
                    self.n_received += 1;
                } else {
                    warn!(
                        "piece received from worker that was not in piece queue, id: {}, freq: {}",
                        piece_id, freq
                    );
                }
            }

            PM::Have(piece_id) => {
                let span = debug_span!(
                    "processing have message from worker for piece_id: {}",
                    piece_id
                );

                let _gaurd = span.enter();
                let freq = self.piece_freq[piece_id];
                if self
                    .piece_queue
                    .remove(&PieceKey { freq, piece_id })
                    .is_none()
                {
                    debug!(
                        "received have message for piece that was not present in queue \
                        (must have been already downloaded)"
                    );
                    return Ok(());
                }

                debug!("incrememnt the piece freq in the queue");
                self.piece_queue.insert(PieceKey {
                    freq: freq + 1,
                    piece_id,
                });

                self.piece_freq[piece_id] += 1;
            }

            PM::Died(bitfield) => {
                for piece_id in bitfield.iter_ones() {
                    let freq = self.piece_freq[piece_id];
                    // if this peer was alive before this and had this piece, freq should be positive.
                    assert!(freq > 0);
                    if self
                        .piece_queue
                        .remove(&PieceKey { freq, piece_id })
                        .is_none()
                    {
                        // same as in PM::Init handling
                        continue;
                    };

                    self.piece_queue.insert(PieceKey {
                        freq: freq - 1,
                        piece_id,
                    });

                    self.piece_freq[piece_id] -= 1;
                }
            }
        };
        Ok(())
    }
}
