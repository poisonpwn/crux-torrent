use super::{BlockLength, BlockOffset, PieceLength};
use crate::{prelude::*, torrent::Bitfield};
use std::{
    cmp::min,
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
struct Requested {
    block_id: u32,
    request_time: Instant,
}

#[derive(Debug, Clone)]
pub(super) struct PieceDownloadProgress {
    piece_length: PieceLength,
    pending: VecDeque<Requested>,
    block_status: Bitfield,
}

impl PieceDownloadProgress {
    const MAX_BLOCK_SIZE: u32 = 1 << 14;
    const MAX_PENDING_BLOCKS: u32 = 5;
    const REQUEUE_TIMEOUT: Duration = Duration::from_millis(800);

    pub fn new(piece_length: u32) -> Self {
        let nblocks = piece_length.div_ceil(Self::MAX_BLOCK_SIZE);
        let mut block_status = Bitfield::new();
        block_status.resize(nblocks as usize, false);

        Self {
            piece_length,
            pending: VecDeque::new(),
            block_status,
        }
    }

    pub fn next_block_info(&mut self) -> Option<(BlockOffset, BlockLength)> {
        let now = Instant::now();

        // if the oldest block request became stale, requeue it and return the block to be re
        // requested.
        if self
            .pending
            .front()
            .is_some_and(|requested| now - requested.request_time >= Self::REQUEUE_TIMEOUT)
        {
            let Requested { block_id, .. } = self.pending.pop_front().unwrap(); // unwrap safety: we've checked that the front is not None.
            trace!("block request timed out, requeing block_id: {}", block_id);

            self.pending.push_back(Requested {
                block_id,
                request_time: now,
            });

            return Some(self.get_block_info(block_id));
        }

        if self.reached_max_pending() {
            trace!("request blocks pipeline filled");
            return None;
        }

        let block_id = self.block_status.iter_zeros().next().or_else(|| {
            trace!("no more blocks left to request, waiting on requested blocks");
            None
        })? as u32;

        trace!(
            "append to pending queue: block_id: {}, request_time: {:?}",
            block_id,
            now
        );
        let requested_info = Requested {
            block_id,
            request_time: now,
        };

        self.block_status.set(block_id as usize, true);
        self.pending.push_back(requested_info);

        Some(self.get_block_info(block_id))
    }

    #[instrument(level = "trace", skip_all, fields(offset))]
    pub fn update_downloaded(&mut self, offset: BlockOffset) -> anyhow::Result<()> {
        let block_id = offset / Self::MAX_BLOCK_SIZE;
        match self
            .pending
            .iter()
            .enumerate()
            .find_map(|(index, requested)| {
                if requested.block_id == block_id {
                    Some(index)
                } else {
                    None
                }
            }) {
            Some(index) => {
                trace!("removing block {block_id} from pending",);
                self.pending.remove(index);

                trace!("setting block status bit of block {block_id}",);
                self.block_status.set(block_id as usize, true);
            }
            None => {
                warn!(
                    "received block not in queue, block_id: {} (offset: {})",
                    block_id, offset
                );
                anyhow::bail!(
                    "received block not in queue, block_id: {} (offset: {})",
                    block_id,
                    offset
                )
            }
        };
        Ok(())
    }

    fn get_block_info(&self, block_id: u32) -> (BlockOffset, BlockLength) {
        let offset = block_id * Self::MAX_BLOCK_SIZE;
        let length = min(self.piece_length - offset, Self::MAX_BLOCK_SIZE);

        (offset, length)
    }

    #[instrument(level = "trace", skip_all)]
    pub fn reset_progress(&mut self) {
        self.pending.iter().for_each(|requested| {
            trace!("unset block status bit of block {}", requested.block_id);
            self.block_status.set(requested.block_id as usize, false)
        });
        trace!("clear pending queue");
        self.pending.clear();
    }

    pub fn is_done(&self) -> bool {
        self.pending.is_empty() && self.block_status.all()
    }

    fn reached_max_pending(&self) -> bool {
        self.pending.len() as u32 >= Self::MAX_PENDING_BLOCKS
    }
}
