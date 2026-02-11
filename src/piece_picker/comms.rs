use crate::torrent::{Bitfield, PieceIndex};

use super::PieceDone;

pub enum PiecePickerMessage {
    // peer first sends bitfield to worker, send the bitfield to piece picker to
    // update frequencies of the set bit pieces
    Init(Bitfield),

    // for each have message the peer sends, send an update to piece picker
    // to increase the freq count of that piece
    Have(PieceIndex),

    // after piece done send the message to the piece picker to remove the piece from queue
    PieceDone(PieceDone),

    // if the peer at the other end dies, send the bitfield to the piece picker to
    // decrease the frequency of all pieces that the peer held
    Died(Bitfield),
}
