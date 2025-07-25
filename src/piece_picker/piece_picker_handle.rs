use lockable::LockPool;

use super::{PieceDone, PieceGaurd, PieceInfo, PieceQueue};
use crate::{metainfo::PieceHash, peers::PieceIndex, prelude::*, torrent::Bitslice};
use std::{
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::sync::Notify;

use tokio::sync::mpsc;

#[derive(Clone)]
pub struct PiecePickerHandle {
    piece_queue: Arc<RwLock<PieceQueue>>,
    lock_pool: Arc<LockPool<PieceIndex>>,
    piece_tx: mpsc::Sender<PieceDone>,
}

pub struct PieceHandle<'a> {
    pub piece_id: PieceIndex,
    pub piece_length: u32,
    pub piece_hash: PieceHash,
    _gaurd: PieceGaurd<'a>,
    piece_tx: mpsc::Sender<PieceDone>,
}

impl PieceHandle<'_> {
    pub async fn submit(self, piece: Vec<u8>) -> anyhow::Result<()> {
        let notify = Arc::new(Notify::new());
        self.piece_tx
            .send(PieceDone {
                piece_id: self.piece_id,
                piece,
                notify: notify.clone(),
            })
            .await?;

        notify.notified().await;
        debug!("received drop notify: {}", self.piece_id);
        Ok(())
    }
}

impl PiecePickerHandle {
    const IDLE_CHECK_WAIT_DURATION: Duration = Duration::from_millis(200);

    pub(super) fn new(
        piece_queue: Arc<RwLock<PieceQueue>>,
        lock_pool: Arc<LockPool<PieceIndex>>,
        piece_tx: mpsc::Sender<PieceDone>,
    ) -> Self {
        Self {
            piece_queue,
            piece_tx,
            lock_pool,
        }
    }

    #[instrument("next piece", level = "debug", skip_all)]
    pub async fn next_piece(&self, bitfield: &Bitslice) -> anyhow::Result<PieceHandle<'_>> {
        debug!("acquiring next piece to download from piece picker");
        loop {
            {
                let queue = self.piece_queue.read().map_err(|_| {
                    error!("error encountered while trying to acquire lock to read piece queue");
                    anyhow::Error::msg(
                        "error encountered while trying to acquire lock to read piece queue",
                    )
                })?;

                if queue.is_empty() {
                    debug!("piece picker queue is empty!");
                } else {
                    debug!(piece_picker_queue_size = queue.len());
                }

                for (piece_id, piece_info) in queue.iter() {
                    if !bitfield.get(*piece_id).is_some_and(|bitref| *bitref) {
                        continue;
                    }

                    if let Some(gaurd) = self.lock_pool.try_lock(*piece_id) {
                        debug!("lock acquired for piece: {}", *piece_id);
                        debug!(result = self.lock_pool.try_lock(*piece_id).is_none());
                        let PieceInfo {
                            piece_id,
                            hash,
                            length,
                        } = *piece_info;
                        return Ok(PieceHandle {
                            piece_id,
                            piece_hash: hash,
                            piece_length: length,
                            _gaurd: gaurd,
                            piece_tx: self.piece_tx.clone(),
                        });
                    }
                }
            }

            debug!("no pieces free to be downloaded");
            tokio::time::sleep(Self::IDLE_CHECK_WAIT_DURATION).await;
        }
    }
}
