use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(transparent)]
#[repr(transparent)]
pub struct InfoHash([u8; Self::INFO_HASH_SIZE]);
impl InfoHash {
    const INFO_HASH_SIZE: usize = sha1_smol::DIGEST_LENGTH;
}

impl InfoHash {
    pub fn new(bytes: [u8; Self::INFO_HASH_SIZE]) -> Self {
        Self(bytes)
    }
}
impl AsRef<[u8; Self::INFO_HASH_SIZE]> for InfoHash {
    fn as_ref(&self) -> &[u8; Self::INFO_HASH_SIZE] {
        &self.0
    }
}
