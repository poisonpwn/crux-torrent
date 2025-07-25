mod piece_picker;
mod piece_picker_handle;

use lockable::{LockPool, Lockable};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Notify;

use crate::{
    metainfo::PieceHash,
    peers::{PieceIndex, PieceLength},
};

pub use piece_picker::PiecePicker;
pub use piece_picker_handle::{PieceHandle, PiecePickerHandle};
pub type PieceQueue = BTreeMap<PieceIndex, PieceInfo>;
pub type PieceGaurd<'a> = <LockPool<PieceIndex> as Lockable<PieceIndex, ()>>::Guard<'a>;

#[derive(Debug, Clone, Copy)]
pub struct PieceInfo {
    pub piece_id: PieceIndex,
    pub hash: PieceHash,
    pub length: PieceLength,
}

#[derive(Debug)]
pub struct PieceDone {
    piece_id: PieceIndex,
    piece: Vec<u8>,
    notify: Arc<Notify>,
}
