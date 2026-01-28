mod comms;
mod piece_picker;
mod piece_picker_handle;

use crossbeam_skiplist::SkipSet;
use simple_semaphore::{Permit, Semaphore};
use std::sync::Arc;

use crate::{metainfo::PieceHash, peers::PieceIndex, peers::PieceLength};

pub use piece_picker::PiecePicker;
pub use piece_picker_handle::{PieceHandle, PiecePickerHandle, PiecePickerPrototype};

type PieceQueue = SkipSet<PieceKey>;
type PieceFreq = u32;
type PieceGaurd = Permit;
type LockPool = Vec<Arc<Semaphore>>;

#[derive(Debug, Clone, Copy)]
pub struct PieceInfo {
    pub piece_id: PieceIndex,
    pub hash: PieceHash,
    pub length: PieceLength,
}

#[derive(Debug)]
struct PieceDone {
    piece_id: PieceIndex,
    piece: Vec<u8>,
    _piece_gaurd: PieceGaurd,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PieceKey {
    freq: PieceFreq,
    piece_id: PieceIndex,
}
