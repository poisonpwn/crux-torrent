use super::{BlockLength, BlockOffset, PieceLength};
use crate::prelude::*;
use std::cmp::min;

#[derive(Debug, Clone)]
pub(super) struct PieceDownloadProgress {
    piece_length: PieceLength,
    request_pending: BlockOffset,
    downloaded: BlockOffset,
    pending_blocks: u32,
}

impl PieceDownloadProgress {
    const MAX_BLOCK_SIZE: u32 = 1 << 14;
    const MAX_PENDING_BLOCKS: u32 = 5;

    pub fn new(piece_length: u32) -> Self {
        Self {
            piece_length,
            request_pending: 0,
            downloaded: 0,
            pending_blocks: 0,
        }
    }

    pub fn next_block_info(&mut self) -> Option<(BlockOffset, BlockLength)> {
        if self.request_pending == self.piece_length || self.reached_max_pending() {
            trace!("request blocks pipeline filled");
            return None;
        }

        let nbytes_to_end = self.piece_length - self.request_pending;
        debug_assert!(self.request_pending < self.piece_length);

        let length = min(nbytes_to_end, Self::MAX_BLOCK_SIZE);
        let out = Some((self.request_pending, length));

        trace!("increment pending blocks");
        self.pending_blocks += 1;
        trace!(
            "move forward request pending offset by next_block_len={}",
            length
        );
        self.request_pending += length;
        out
    }

    pub fn update_downloaded(
        &mut self,
        block_begin: BlockOffset,
        length: BlockLength,
    ) -> anyhow::Result<()> {
        if block_begin != self.downloaded {
            warn!(
                last_downloaded_block = self.downloaded,
                incoming_block = block_begin,
                "blocks given out of order by peer"
            );
            anyhow::bail!("blocks downloaded out of order. last downloaded offset: {}, incoming block offset: {}", self.downloaded, block_begin)
        }

        self.downloaded += length;
        self.pending_blocks -= 1;
        trace!(
            downloaded_end_offset = self.downloaded,
            num_pending_blocks = self.pending_blocks,
            "update download progress",
        );
        Ok(())
    }

    pub fn reset_progress(&mut self) {
        debug!(
            "reset download progress to {last_requested_block_end}",
            last_requested_block_end = self.request_pending
        );
        self.request_pending = self.downloaded;
        self.pending_blocks = 0;
    }

    pub fn is_done(&self) -> bool {
        trace!(
            "checking if block done {last_downloaded_block_end} {piece_end}",
            last_downloaded_block_end = self.downloaded,
            piece_end = self.piece_length
        );
        self.downloaded == self.piece_length
    }

    fn reached_max_pending(&self) -> bool {
        trace!(
            "check if reached max pending {num_pending}",
            num_pending = self.pending_blocks
        );
        self.pending_blocks >= Self::MAX_PENDING_BLOCKS
    }
}
