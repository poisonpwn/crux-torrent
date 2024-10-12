use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::{
    bytes::{self, Buf, BufMut},
    codec::{length_delimited::LengthDelimitedCodec, Decoder, Encoder, Framed},
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
#[derive(Debug, Clone, Eq, PartialEq)]
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
        // taken from `std::mem::discriminant` docs.
        // https://doc.rust-lang.org/std/mem/fn.discriminant.html
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
}

#[derive(Debug, Clone)]
pub struct PeerMessageCodec {
    // codec only used on decode, to decode length delimited frames.
    inner_codec: LengthDelimitedCodec,
}

impl PeerMessageCodec {
    const MAX_FRAME_SIZE: usize = 2 * (1 << 20);

    pub fn new() -> Self {
        Self {
            inner_codec: LengthDelimitedCodec::builder()
                .max_frame_length(Self::MAX_FRAME_SIZE)
                .new_codec(),
        }
    }

    // helper method to bail if the peer sends invalid (less than what is required) payload for the particular variant.
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

    // helper method for the Cancel and Request variants only.
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
        let mut frame = match self.inner_codec.decode(src)? {
            Some(frame) => frame,
            None => return Ok(None),
        };

        // message was a keepalive (length = 0)
        if frame.is_empty() {
            return Ok(None);
        }

        let tag = frame.get_u8();

        type PM = PeerMessage;
        let msg = match tag {
            PeerMessageTags::CHOKE => PM::Choke,
            PeerMessageTags::UNCHOKE => PM::Unchoke,
            PeerMessageTags::INTERERSTED => PM::Interested,
            PeerMessageTags::NOT_INTERESTED => PM::NotInterested,
            PeerMessageTags::HAVE => {
                Self::bail_on_size_mismatch(&mut frame, std::mem::size_of::<u32>())?;
                PM::Have(frame.get_u32())
            }
            // a panic shouldn't happen here as any amount of bytes is valid
            PeerMessageTags::BITFIELD => PM::Bitfield(Bitfield::from_vec(frame.to_vec())),
            PeerMessageTags::REQUEST => {
                let (index, begin, length) = Self::decode_triple_variant(&mut frame)?;

                PM::Request {
                    index,
                    begin,
                    length,
                }
            }
            PeerMessageTags::PIECE => {
                Self::bail_on_size_mismatch(&mut frame, 2 * std::mem::size_of::<u32>())?;

                PM::Piece {
                    index: frame.get_u32(),
                    begin: frame.get_u32(),
                    piece: frame.to_vec(),
                }
            }
            PeerMessageTags::CANCEL => {
                let (index, begin, length) = Self::decode_triple_variant(&mut frame)?;

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
        // inner codec is not used as it would require allocating another BytesMut
        // instead we write directly to the dst buffer of the Framed instance.
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
                let bf_byte_slice = bitfield.as_raw_slice();
                dst.put_u32(TAG_LEN + bf_byte_slice.len() as u32);
                dst.put_u8(tag);

                dst.put(bf_byte_slice);
            }
        }
        Ok(())
    }
}

pub type PeerFrames<T> = Framed<T, PeerMessageCodec>;

pub fn upgrade_stream<T>(stream: T) -> PeerFrames<T>
where
    T: AsyncRead + AsyncWrite,
{
    PeerFrames::new(stream, PeerMessageCodec::new())
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::SinkExt;
    use rand::{
        distributions::{Alphanumeric, Slice, Uniform},
        Rng,
    };
    use std::collections::VecDeque;
    use std::io::Cursor;
    use tokio_stream::StreamExt;

    type MesgTuple = (VecDeque<u8>, PeerMessage);

    fn prepend_size(vec: &mut VecDeque<u8>) {
        let len_bytes = (vec.len() as u32).to_be_bytes();
        for byte in len_bytes.into_iter().rev() {
            vec.push_front(byte);
        }
    }

    fn choke() -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(0u8);

        prepend_size(&mut frame);
        (frame, PeerMessage::Choke)
    }

    fn unchoke() -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(1u8);

        prepend_size(&mut frame);
        (frame, PeerMessage::Unchoke)
    }

    fn interested() -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(2u8);

        prepend_size(&mut frame);
        (frame, PeerMessage::Interested)
    }

    fn not_interested() -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(3u8);

        prepend_size(&mut frame);
        (frame, PeerMessage::NotInterested)
    }

    fn have(rng: &mut impl Rng) -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(4u8);

        let uniform_256 = rand::distributions::Uniform::<u32>::from(0..10000);
        let have_piece_index: u32 = rng.sample(uniform_256);
        frame.extend(have_piece_index.to_be_bytes());

        prepend_size(&mut frame);
        (frame, PeerMessage::Have(have_piece_index))
    }

    fn bitfield(rng: &mut impl Rng) -> MesgTuple {
        let mut frame = VecDeque::new();

        frame.push_back(5u8);

        let bf_length = rng.sample(Uniform::from(1..=256));

        // bitfield length should always be a multiple of eight to be encoded and decoded properly
        // into u8 slices.
        let bf: Bitfield = (0..(bf_length * 8))
            .map(|_| rng.sample(Slice::new(&[true, false]).expect("slice is not empty")))
            .collect();

        frame.extend(bf.as_raw_slice());
        prepend_size(&mut frame);

        (frame, PeerMessage::Bitfield(bf))
    }

    fn request(rng: &mut impl Rng) -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(6u8);

        let index: u32 = rng.sample(Uniform::from(0..10000));
        let begin: u32 = rng.sample(Uniform::from(0..(2 * (1 << 30))));
        let length: u32 = rng.sample(Uniform::from(0..(2 << 14)));

        frame.extend(index.to_be_bytes());
        frame.extend(begin.to_be_bytes());
        frame.extend(length.to_be_bytes());

        prepend_size(&mut frame);

        (
            frame,
            PeerMessage::Request {
                index,
                begin,
                length,
            },
        )
    }

    fn piece(rng: &mut impl Rng) -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(7u8);

        let uniform = rand::distributions::Uniform::<u32>::from(0..100000);

        let index = rng.sample(uniform);
        let begin = rng.sample(uniform);
        let piece: Vec<u8> = (0..1024).map(|_| rng.sample(Alphanumeric)).collect();

        frame.extend(index.to_be_bytes());
        frame.extend(begin.to_be_bytes());
        frame.extend(&piece);

        prepend_size(&mut frame);

        (
            frame,
            PeerMessage::Piece {
                index,
                begin,
                piece,
            },
        )
    }

    fn cancel(rng: &mut impl Rng) -> MesgTuple {
        let mut frame = VecDeque::new();
        frame.push_back(8u8);

        let uniform_256 = rand::distributions::Uniform::<u32>::from(0..256);

        let index = rng.sample(uniform_256);
        let begin = rng.sample(uniform_256);
        let length = rng.sample(uniform_256);

        frame.extend(index.to_be_bytes());
        frame.extend(begin.to_be_bytes());
        frame.extend(length.to_be_bytes());

        prepend_size(&mut frame);

        (
            frame,
            PeerMessage::Cancel {
                index,
                begin,
                length,
            },
        )
    }

    #[tokio::test]
    async fn test_decode() {
        let mut rng = rand::thread_rng();

        let (buffer, test_decoded) = [
            choke(),
            unchoke(),
            interested(),
            not_interested(),
            have(&mut rng),
            bitfield(&mut rng),
            request(&mut rng),
            piece(&mut rng),
            cancel(&mut rng),
        ]
        .into_iter()
        .fold(
            (Vec::new(), Vec::new()),
            |(mut buffer, mut mesg_vec), (frame, mesg)| {
                buffer.extend(frame);
                mesg_vec.push(mesg);
                (buffer, mesg_vec)
            },
        );

        let buffer = Cursor::new(buffer);
        let mut decoder = upgrade_stream(buffer);

        let decoded = {
            let mut decoded = Vec::new();
            while let Some(mesg) = decoder.next().await {
                decoded.push(mesg.expect("io error shoudln't occur when using cursor buffer"));
            }
            decoded
        };

        assert_eq!(decoded.len(), test_decoded.len());

        for (mesg, correct_mesg) in std::iter::zip(decoded, test_decoded) {
            assert_eq!(mesg, correct_mesg);
        }
    }

    #[tokio::test]
    async fn test_encode() {
        let mut rng = rand::thread_rng();

        let (test_buffer, messages) = [
            choke(),
            unchoke(),
            interested(),
            not_interested(),
            have(&mut rng),
            bitfield(&mut rng),
            request(&mut rng),
            piece(&mut rng),
            cancel(&mut rng),
        ]
        .into_iter()
        .fold(
            (Vec::new(), Vec::new()),
            |(mut buffer, mut mesg_vec), (frame, mesg)| {
                buffer.extend(frame);
                mesg_vec.push(mesg);
                (buffer, mesg_vec)
            },
        );

        let mut encoder = upgrade_stream(Cursor::new(Vec::new()));

        for mesg in messages {
            encoder.send(mesg).await.unwrap();
        }
        let output_byte = encoder.write_buffer();

        for (correct_byte, output_byte) in std::iter::zip(test_buffer, output_byte) {
            assert_eq!(correct_byte, *output_byte);
        }
    }
}
