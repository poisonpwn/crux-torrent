use tokio_util::{
    bytes::{self, Buf, BufMut},
    codec::{Decoder, Encoder},
};

#[repr(u8)]
enum PeerMessage {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have(u32) = 4,
    Bitfield(Vec<u8>) = 5,
    Request {
        index: u32,
        begin: u32,
        length: u32,
    } = 6,
    Piece {
        index: u32,
        begin: u32,
        piece: Vec<u8>,
    } = 7,
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    } = 8,
}

impl PeerMessage {
    pub fn tag(&self) -> u8 {
        // SAFETY: because PeerMessage is a repr(u8) its also repr(C) and the first byte(u8) represents
        // the enum tag (dereferencing the *self casted to a *u8 gives first byte).
        // taken from std::mem::discriminant docs.
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

struct PeerMessageCodec;

impl PeerMessageCodec {
    const MAX_SIZE: usize = 2 * (1 >> 10);

    // bail if the peer sends invalid(less than what is required) length for the particular variant.
    fn bail_on_size_mismatch(src: &mut bytes::BytesMut, min_size: usize) -> anyhow::Result<()> {
        let len = src.len();
        if len < min_size {
            anyhow::bail!("buf size {} does not match size for tag {}", len, min_size)
        }
        Ok(())
    }

    // helper for the Cancel and Request variants only.
    fn decode_triple_variant(src: &mut bytes::BytesMut) -> anyhow::Result<(u32, u32, u32)> {
        const TRIPLE_SIZE: usize = 3 * std::mem::size_of::<u32>();
        Self::bail_on_size_mismatch(src, TRIPLE_SIZE);
        Ok((src.get_u32(), src.get_u32(), src.get_u32()))
    }
}

impl Decoder for PeerMessageCodec {
    type Item = Option<PeerMessage>;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> anyhow::Result<Option<Self::Item>> {
        if src.len() < std::mem::size_of::<u32>() {
            // return None to signify that more bytes need to be read for current frame.
            return Ok(None);
        }

        let len_header = src.get_u32() as usize;
        if len_header == 0 {
            // return Some(None) when message was a keepalive
            return Ok(Some(None));
        }

        // prevent malicious peers from hogging us.
        if len_header > Self::MAX_SIZE {
            anyhow::bail!("frames larger than 2 MiB will not be parsed.")
        }

        if src.len() < len_header {
            src.reserve(len_header);
            return Ok(None);
        }

        let tag = src.get_u32() as u8;

        type PM = PeerMessage;
        let msg = match tag {
            0 => PM::Choke,
            1 => PM::Unchoke,
            2 => PM::Interested,
            3 => PM::NotInterested,
            4 => {
                Self::bail_on_size_mismatch(src, std::mem::size_of::<u32>())?;
                PM::Have(src.get_u32())
            }
            // a panic shouldn't happen here as any amount of bytes is valid
            5 => PM::Bitfield(src.to_vec()),
            6 => {
                let (index, begin, length) = Self::decode_triple_variant(src)?;

                PM::Request {
                    index,
                    begin,
                    length,
                }
            }
            7 => {
                Self::bail_on_size_mismatch(src, 2 * std::mem::size_of::<u32>())?;
                let (index, begin) = (src.get_u32(), src.get_u32());

                PM::Piece {
                    index,
                    begin,
                    piece: src.to_vec(), // don't need to check size as any length is valid.
                }
            }
            8 => {
                let (index, begin, length) = Self::decode_triple_variant(src)?;

                PM::Cancel {
                    index,
                    begin,
                    length,
                }
            }
            _ => anyhow::bail!("invalid protocol tag for peer message: {}", tag),
        };

        Ok(Some(Some(msg)))
    }
}
