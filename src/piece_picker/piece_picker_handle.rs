use super::{PieceDone, PieceInfo, PieceQueue};
use crate::prelude::*;
use std::{
    sync::{mpsc, Arc, Mutex, MutexGuard, RwLock, TryLockError},
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct PiecePickerHandle {
    piece_queue: Arc<RwLock<PieceQueue>>,
    piece_tx: mpsc::Sender<PieceDone>,
}

impl PiecePickerHandle {
    const IDLE_CHECK_WAIT_DURATION: Duration = Duration::from_millis(200);

    pub(super) fn new(
        piece_queue: Arc<RwLock<PieceQueue>>,
        piece_tx: mpsc::Sender<PieceDone>,
    ) -> Self {
        Self {
            piece_queue,
            piece_tx,
        }
    }

    async fn next_piece(&mut self) -> anyhow::Result<MutexGuard<'a, PieceInfo>> {
        loop {
            let queue = self.piece_queue.read()?;

            for piece_info in queue.values() {
                match piece_info.try_lock() {
                    Ok(res) => return Ok(res),
                    Err(TryLockError::Poisoned(e)) => return Ok(e.into_inner()),
                    _ => continue,
                }
            }

            tokio::time::sleep(Self::IDLE_CHECK_WAIT_DURATION).await;
        }
    }

    async fn send_piece(
        &mut self,
        piece_info: MutexGuard<'a, PieceInfo>,
        piece: Vec<u8>,
    ) -> anyhow::Result<()> {
        debug!(
            "sending done piece to piece_picker: {}",
            piece_info.piece_id
        );
        self.piece_tx.send(PieceDone {
            piece_id: piece_info.piece_id,
            piece,
        })?;
        Ok(())
    }
}
