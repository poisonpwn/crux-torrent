use rand::distributions::{Alphanumeric, DistString};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(transparent)]
#[repr(transparent)]
pub struct PeerId([u8; Self::PEER_ID_SIZE]);

impl AsRef<[u8; Self::PEER_ID_SIZE]> for PeerId {
    fn as_ref(&self) -> &[u8; Self::PEER_ID_SIZE] {
        &self.0
    }
}

impl PeerId {
    pub const PEER_ID_SIZE: usize = 20;
    pub const PEER_ID_VENDOR_PREFIX: &'static [u8; 8] = b"-CX0000-";
    pub const SUFFIX_LEN: usize = Self::PEER_ID_SIZE - Self::PEER_ID_VENDOR_PREFIX.len();

    pub fn new(suffix: &[u8; Self::SUFFIX_LEN]) -> Self {
        let mut peer_id = [0; Self::PEER_ID_SIZE];

        let (prefix_segment, suffix_segment) =
            peer_id.split_at_mut(Self::PEER_ID_VENDOR_PREFIX.len());
        prefix_segment.copy_from_slice(Self::PEER_ID_VENDOR_PREFIX);

        suffix_segment.copy_from_slice(suffix);

        PeerId(peer_id)
    }

    pub fn random() -> Self {
        let mut rng = rand::thread_rng();
        let suffix = Alphanumeric.sample_string(&mut rng, Self::SUFFIX_LEN);

        Self::new(
            suffix
                .as_bytes()
                .try_into()
                .expect("can't fail as suffix is exactly SUFFIX_LEN long"),
        )
    }
}
