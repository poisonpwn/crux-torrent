use bitvec::{order::Msb0, prelude as bv};

// TODO: maybe swapping this to a u64 might enable some SIMD optimization.
// bitfields sent on the peer messages codec are big endian byte order (i.e Most significant bit
// first)
pub type Bitfield = bv::BitVec<u8, Msb0>;
pub type Bitslice = bv::BitSlice<u8, Msb0>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_bitfield_indexing() {
        // supposed to mock the vector from the stream;
        let stream_buf = vec![0u8, 1, 0, 1];
        let bv = Bitfield::from_vec(stream_buf.clone());
        for (byte_index, byte) in stream_buf.iter().enumerate() {
            for bit_offset in 0..8 {
                // bit mask with the ith index bit set where i is the offset from the msb
                let bitmask = 1u8 << (7 - bit_offset);
                // extract the ith bit of  the byte as a 1 or 0.
                let stream_bit = ((byte & bitmask) > 0) as u8;

                let bit_index = byte_index * 8 + bit_offset;
                let bitfield_bit = bv[bit_index] as u8;
                assert_eq!(stream_bit, bitfield_bit);
            }
        }
        assert_eq!(&[0u8, 1, 0, 1], bv.as_raw_slice());
    }
}
