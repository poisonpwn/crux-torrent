pub mod download_worker;

mod progress;

pub type PieceIndex = usize;
pub type PieceLength = u32;
type BlockLength = u32;
type BlockOffset = u32;
