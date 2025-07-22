mod piece_picker;
mod piece_picker_handle;

pub use piece_picker::PiecePicker;
pub use piece_picker_handle::PiecePickerHandle;
use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::{
    metainfo::PieceHash,
    peers::{PieceIndex, PieceLength},
};

pub type PieceQueue = BTreeMap<PieceIndex, Mutex<PieceInfo>>;

struct PieceInfo {
    piece_id: PieceIndex,
    hash: PieceHash,
    length: PieceLength,
}

struct PieceDone {
    piece_id: PieceIndex,
    piece: Vec<u8>,
}
