mod bitfield;
mod info_hash;
mod peer_id;

pub use bitfield::Bitfield;
pub use info_hash::InfoHash;
pub use peer_id::PeerId;

pub type PieceIndex = usize;
pub type PieceLength = u32;
pub type TorrentLength = usize;

pub struct Piece {
    pub piece_id: PieceIndex,
    pub data: Vec<u8>,
}
