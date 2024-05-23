use crate::tracker::request::{InfoHash, PeerId};

#[derive(Debug, Clone)]
#[repr(C)] // makes sure the struct fields are arranged in the same order, there's also no padding
           // in between because all the fields are byte aligned. (i.e this can be treated as a
           // simple array of bytes).
pub struct PeerHandshake {
    protocol_prefix_length: u8,
    protocol_prefix: [u8; 19],
    reserved_bytes: [u8; 8],
    info_hash: InfoHash,
    peer_id: PeerId,
}

impl PeerHandshake {
    pub const PROTOCOL_PREFIX: [u8; 19] = *b"BitTorrent protocol";
    pub fn new(info_hash: InfoHash, peer_id: PeerId) -> Self {
        Self {
            protocol_prefix_length: Self::PROTOCOL_PREFIX.len() as u8,
            protocol_prefix: Self::PROTOCOL_PREFIX,
            reserved_bytes: [0; 8],
            info_hash,
            peer_id,
        }
    }

    // the unsafe is fine becuase the struct is just plain old data, any sequence of bits is valid.
    pub fn from_bytes(bytes: [u8; std::mem::size_of::<Self>()]) -> Self {
        unsafe { std::mem::transmute::<[u8; std::mem::size_of::<Self>()], Self>(bytes) }
    }
    // the unsafe is fine becuase the struct is just plain old data, any sequence of bits is valid.
    pub fn into_bytes(self) -> [u8; std::mem::size_of::<Self>()] {
        unsafe { std::mem::transmute::<Self, [u8; std::mem::size_of::<Self>()]>(self) }
    }
}
