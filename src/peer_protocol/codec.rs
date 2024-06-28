use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::{
    bytes::{self, Buf, BufMut},
    codec::{Decoder, Encoder},
};

use crate::torrent::Bitfield;

struct PeerMessageTags;
impl PeerMessageTags {
    // tags according to https://www.bittorrent.org/beps/bep_0003.html
    const CHOKE: u8 = 0;
    const UNCHOKE: u8 = 1;
    const INTERERSTED: u8 = 2;
    const NOT_INTERESTED: u8 = 3;
    const HAVE: u8 = 4;
    const BITFIELD: u8 = 5;
    const REQUEST: u8 = 6;
    const PIECE: u8 = 7;
    const CANCEL: u8 = 8;
}

#[repr(u8)]
#[derive(Debug, Clone)]
pub enum PeerMessage {
    Choke = PeerMessageTags::CHOKE,
    Unchoke = PeerMessageTags::UNCHOKE,
    Interested = PeerMessageTags::INTERERSTED,
    NotInterested = PeerMessageTags::NOT_INTERESTED,
    Have(u32) = PeerMessageTags::HAVE,
    Bitfield(Bitfield) = PeerMessageTags::BITFIELD,
    Request {
        index: u32,
        begin: u32,
        length: u32,
    } = PeerMessageTags::REQUEST,
    Piece {
        index: u32,
        begin: u32,
        piece: Vec<u8>,
    } = PeerMessageTags::PIECE,
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    } = PeerMessageTags::CANCEL,
}

impl PeerMessage {
    pub fn tag(&self) -> u8 {
        // SAFETY: because PeerMessage is a repr(u8) its also repr(C) and the first byte(u8) represents
        // the enum tag (dereferencing the *self casted to a *u8 gives first byte).
        // taken from std::mem::discriminant docs.
        // https://doc.rust-lang.org/std/mem/fn.discriminant.html
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

pub struct PeerMessageCodec;

impl PeerMessageCodec {
    const MAX_SIZE: usize = 2 * (1 << 20);

    // bail if the peer sends invalid(less than what is required) length for the particular variant.
    fn bail_on_size_mismatch(src: &mut bytes::BytesMut, min_size: usize) -> anyhow::Result<()> {
        let len = src.len();
        if len < min_size {
            anyhow::bail!(
                "buf size sent by peer {} does not match size for tag {}",
                len,
                min_size
            )
        }
        Ok(())
    }

    // helper fn for the Cancel and Request variants only.
    fn decode_triple_variant(src: &mut bytes::BytesMut) -> anyhow::Result<(u32, u32, u32)> {
        const TRIPLE_SIZE: usize = 3 * std::mem::size_of::<u32>();
        Self::bail_on_size_mismatch(src, TRIPLE_SIZE)?;
        Ok((src.get_u32(), src.get_u32(), src.get_u32()))
    }
}

impl Decoder for PeerMessageCodec {
    type Item = PeerMessage;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut bytes::BytesMut) -> anyhow::Result<Option<Self::Item>> {
        const LEN_HEADER_SIZE: usize = std::mem::size_of::<u32>();

        if src.len() < LEN_HEADER_SIZE {
            // return None to signify that more bytes need to be read for current frame to be
            // decoded.
            return Ok(None);
        }

        let len_header = {
            // peek at the length and see if enough data has been read. otherwise do not advance
            // the cursor.
            let mut len_header: [u8; 4] = [0; 4];
            len_header.copy_from_slice(&src[0..4]);
            let len_header = u32::from_be_bytes(len_header) as usize;

            // prevent malicious peers (if they exist) from hogging us.
            if len_header > Self::MAX_SIZE {
                anyhow::bail!(
                    "frames of size {} (>2 MiB) prevented from being decoded.",
                    len_header
                )
            }

            if src.len() < len_header {
                src.reserve(len_header);
                return Ok(None);
            }

            src.advance(LEN_HEADER_SIZE);

            len_header
        };
        // enough data has been read for a full frame, now it is safe to to use get methods.

        if len_header == 0 {
            // message was a keep alive
            return Ok(None);
        }
        let mut src = src.split_to(len_header);

        let tag = src.get_u8();
        type PM = PeerMessage;
        let msg = match tag {
            PeerMessageTags::CHOKE => PM::Choke,
            PeerMessageTags::UNCHOKE => PM::Unchoke,
            PeerMessageTags::INTERERSTED => PM::Interested,
            PeerMessageTags::NOT_INTERESTED => PM::NotInterested,
            PeerMessageTags::HAVE => {
                Self::bail_on_size_mismatch(&mut src, std::mem::size_of::<u32>())?;
                PM::Have(src.get_u32())
            }
            // a panic shouldn't happen here as any amount of bytes is valid
            PeerMessageTags::BITFIELD => PM::Bitfield(Bitfield::from_vec(src.to_vec())),
            PeerMessageTags::REQUEST => {
                let (index, begin, length) = Self::decode_triple_variant(&mut src)?;

                PM::Request {
                    index,
                    begin,
                    length,
                }
            }
            PeerMessageTags::PIECE => {
                Self::bail_on_size_mismatch(&mut src, 2 * std::mem::size_of::<u32>())?;

                PM::Piece {
                    index: src.get_u32(),
                    begin: src.get_u32(),
                    piece: src.to_vec(),
                }
            }
            PeerMessageTags::CANCEL => {
                let (index, begin, length) = Self::decode_triple_variant(&mut src)?;

                PM::Cancel {
                    index,
                    begin,
                    length,
                }
            }
            invalid_tag => anyhow::bail!("invalid protocol tag for peer message: {}", invalid_tag),
        };

        Ok(Some(msg))
    }
}

impl Encoder<PeerMessage> for PeerMessageCodec {
    type Error = anyhow::Error;
    fn encode(&mut self, item: PeerMessage, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        const TAG_LEN: u32 = std::mem::size_of::<u8>() as u32;
        let tag = item.tag();

        type PM = PeerMessage;
        match item {
            PM::Choke | PM::Unchoke | PM::Interested | PM::NotInterested => {
                dst.put_u32(TAG_LEN);
                dst.put_u8(tag);
            }
            PM::Have(index) => {
                dst.put_u32(TAG_LEN + std::mem::size_of::<u32>() as u32);
                dst.put_u8(tag);

                dst.put_u32(index);
            }
            PM::Request {
                index,
                begin,
                length,
            }
            | PM::Cancel {
                index,
                begin,
                length,
            } => {
                dst.put_u32(TAG_LEN + 3 * std::mem::size_of::<u32>() as u32);
                dst.put_u8(tag);

                dst.put_u32(index);
                dst.put_u32(begin);
                dst.put_u32(length);
            }

            PM::Piece {
                index,
                begin,
                piece,
            } => {
                dst.put_u32(TAG_LEN + (2 * std::mem::size_of::<u32>() + piece.len()) as u32);
                dst.put_u8(tag);

                dst.put_u32(index);
                dst.put_u32(begin);
                dst.put(piece.as_slice());
            }

            PM::Bitfield(bitfield) => {
                dst.put_u32(TAG_LEN + bitfield.len() as u32);
                dst.put_u8(tag);

                dst.put(bitfield.as_raw_slice());
            }
        }
        Ok(())
    }
}

pub type PeerFrames<T> = tokio_util::codec::Framed<T, PeerMessageCodec>;

pub fn upgrade_stream<T>(stream: T) -> PeerFrames<T>
where
    T: AsyncRead + AsyncWrite,
{
    PeerFrames::new(stream, PeerMessageCodec)
}
