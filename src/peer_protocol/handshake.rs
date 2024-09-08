use crate::prelude::*;
use crate::torrent::{InfoHash, PeerId};

#[derive(Debug, Clone, PartialEq)]
#[repr(C)] // makes sure the struct fields are arranged in the same order, there's also no padding
           // in between because all the fields are byte aligned. (i.e this can be treated as a
           // simple array of bytes).
pub struct PeerHandshake {
    protocol_prefix_length: u8,
    protocol_prefix: [u8; 19],
    reserved_bytes: [u8; 8],
    pub info_hash: InfoHash,
    pub peer_id: PeerId,
}

impl AsRef<[u8; std::mem::size_of::<Self>()]> for PeerHandshake {
    fn as_ref(&self) -> &[u8; std::mem::size_of::<Self>()] {
        unsafe { std::mem::transmute::<&Self, &[u8; std::mem::size_of::<Self>()]>(self) }
    }
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
        let handshake =
            unsafe { std::mem::transmute::<[u8; std::mem::size_of::<Self>()], Self>(bytes) };
        if handshake.protocol_prefix != Self::PROTOCOL_PREFIX {
            warn!(
                "unknown protocol prefix in handshake '{}'",
                String::from_utf8_lossy(&handshake.protocol_prefix[..])
            );
        }

        handshake
    }
    // the unsafe is fine becuase the struct is just plain old data, any sequence of bits is valid.
    pub fn into_bytes(self) -> [u8; std::mem::size_of::<Self>()] {
        unsafe { std::mem::transmute::<Self, [u8; std::mem::size_of::<Self>()]>(self) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent::{InfoHash, PeerId};
    use rstest::*;
    const INFO_HASH: [u8; 20] = [0; 20];
    const PEER_ID_SUFFIX: [u8; PeerId::SUFFIX_LEN] = [7; PeerId::SUFFIX_LEN];
    type PH = PeerHandshake;
    type HB = [u8; std::mem::size_of::<PeerHandshake>()];

    #[fixture]
    fn peer_id() -> PeerId {
        PeerId::with_suffix(&PEER_ID_SUFFIX)
    }

    #[fixture]
    fn info_hash() -> InfoHash {
        InfoHash::new(INFO_HASH)
    }

    #[fixture]
    fn handshake(info_hash: InfoHash, peer_id: PeerId) -> PH {
        PH::new(info_hash, peer_id)
    }

    #[fixture]
    fn handshake_bytes(info_hash: InfoHash, peer_id: PeerId) -> HB {
        let out = {
            let mut out: Vec<u8> = Vec::new();
            out.push(19);
            out.extend_from_slice(b"BitTorrent protocol");
            out.extend_from_slice(&[0; 8]);
            out.extend_from_slice(info_hash.as_ref());
            out.extend_from_slice(peer_id.as_ref());
            out
        };

        out.try_into().unwrap()
    }

    #[rstest]
    fn test_into_bytes(handshake: PeerHandshake, handshake_bytes: HB) {
        assert_eq!(handshake.into_bytes(), handshake_bytes);
    }

    #[rstest]
    fn test_decode_from_bytes(handshake_bytes: HB) {
        let out = PH::from_bytes(handshake_bytes);
        assert_eq!(out.into_bytes(), handshake_bytes);
    }
}
