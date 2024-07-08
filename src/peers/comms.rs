use super::{PieceIndex, PieceLength};
use crate::Bitfield;
use std::net::SocketAddrV4;
use tokio::sync::mpsc;

use crate::metainfo::files::PieceHash;

#[derive(Debug, Clone)]
pub struct PieceRequestInfo {
    pub index: PieceIndex,
    pub length: u32,
    pub hash: PieceHash,
}

impl PieceRequestInfo {
    pub fn new(index: PieceIndex, length: PieceLength, hash: PieceHash) -> Self {
        Self {
            index,
            length,
            hash,
        }
    }
}

pub enum PeerCommands {
    NotInterested,
    DownloadPiece(PieceRequestInfo),
    Shutdown,
}

pub enum PeerAlerts {
    InitPeer {
        peer_addr: SocketAddrV4,
        bitfield: Bitfield,
        commands_tx: mpsc::Sender<PeerCommands>,
    },
    UpdateBitfield {
        peer_addr: std::net::SocketAddrV4,
        has_piece: PieceIndex,
    },
    DonePiece {
        piece_index: PieceIndex,
        piece: Vec<u8>,
    },
}
