use rand::distributions::{Alphanumeric, DistString};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

    pub fn new(bytes: [u8; Self::PEER_ID_SIZE]) -> Self {
        Self(bytes)
    }

    pub fn with_suffix(suffix: &[u8; Self::SUFFIX_LEN]) -> Self {
        let mut peer_id = [0; Self::PEER_ID_SIZE];

        let (prefix_segment, suffix_segment) =
            peer_id.split_at_mut(Self::PEER_ID_VENDOR_PREFIX.len());
        prefix_segment.copy_from_slice(Self::PEER_ID_VENDOR_PREFIX);

        suffix_segment.copy_from_slice(suffix);

        PeerId::new(peer_id)
    }

    pub fn with_random_suffix() -> Self {
        let mut rng = rand::thread_rng();
        let suffix = Alphanumeric.sample_string(&mut rng, Self::SUFFIX_LEN);

        Self::with_suffix(
            suffix
                .as_bytes()
                .try_into()
                .expect("can't fail as suffix is exactly SUFFIX_LEN long"),
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use rand::distributions::Uniform;
    use rand::prelude::*;
    use rstest::rstest;

    const PEER_ID_LEN: usize = 20;
    const PREFIX: &[u8; 8] = b"-CX0000-";

    #[rstest]
    fn test_peer_id() {
        let suffix = {
            let mut suffix = [0u8; PEER_ID_LEN - PREFIX.len()];
            let mut rng = rand::thread_rng();
            let dist = Uniform::from(0..=u8::MAX);
            suffix.iter_mut().for_each(|pos| {
                *pos = rng.sample(dist);
            });
            suffix
        };

        let peer_id = PeerId::with_suffix(&suffix);
        let peer_id_slice = peer_id.as_ref();

        let test_peer_id_slice = {
            let mut buffer = [0; 20];
            buffer[..PREFIX.len()].copy_from_slice(PREFIX);
            buffer[PREFIX.len()..].copy_from_slice(&suffix);
            buffer
        };
        assert_eq!(peer_id_slice, &test_peer_id_slice);
    }
}
