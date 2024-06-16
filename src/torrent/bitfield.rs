use bitvec::{order::Msb0, prelude as bv};

// TODO: maybe swapping this to a u64 might enable some SIMD optimization.
// bitfields sent on the peer messages codec are big endian byte order (i.e Most significant bit
// first)
pub type Bitfield = bv::BitVec<u8, Msb0>;
