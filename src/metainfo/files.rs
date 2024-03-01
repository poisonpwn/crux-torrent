use crate::tracker::request::Requestable;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};
use sha1_smol::Sha1;
use static_str_ops::static_format;

#[derive(Debug, Deserialize, Serialize)]
pub struct File {
    pub path: Vec<String>,
    pub length: usize,

    #[serde(default)]
    pub md5sum: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FileInfo {
    MultiFile {
        #[serde(rename = "name")]
        dirname: String,

        files: Vec<File>,

        #[serde(rename = "piece length")]
        piece_length: usize,

        #[serde(serialize_with = "serde_bytes::serialize")]
        pieces: FileHashes,

        #[serde(default)]
        private: Option<i64>,
    },

    SingleFile {
        #[serde(rename = "name")]
        filename: String,
        length: usize,

        #[serde(default)]
        md5sum: Option<String>,

        #[serde(rename = "piece length")]
        piece_length: usize,

        #[serde(serialize_with = "serde_bytes::serialize")]
        pieces: FileHashes,

        private: Option<i64>,
    },
}

impl Requestable for FileInfo {
    fn get_info_hash(&self) -> anyhow::Result<[u8; sha1_smol::DIGEST_LENGTH]> {
        let info_hash = serde_bencode::to_bytes(self)?;
        Ok(dbg!(Sha1::from(info_hash).digest()).bytes())
    }

    fn get_request_length(&self) -> usize {
        match self {
            Self::SingleFile { length, .. } => *length,
            Self::MultiFile { files, .. } => files.iter().map(|file| file.length).sum(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileHashes(Vec<[u8; FileHashes::HASH_SIZE]>);
impl FileHashes {
    pub const HASH_SIZE: usize = 20;
}

impl serde_bytes::Serialize for FileHashes {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_bytes(&self.0.concat())
    }
}

impl<'de> Deserialize<'de> for FileHashes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(FileHashes(deserializer.deserialize_bytes(FileHashVisitor)?))
    }
}

struct FileHashVisitor;
impl<'de> Visitor<'de> for FileHashVisitor {
    type Value = Vec<[u8; FileHashes::HASH_SIZE]>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(static_format!(
            "a byte sequence  whose length is a multiple of {}",
            FileHashes::HASH_SIZE
        ))
    }

    fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let n_bytes = bytes.len();

        type FH = FileHashes;
        if n_bytes % FH::HASH_SIZE != 0 {
            return Err(E::custom(static_format!(
                "file hash pieces should be a multiple of length {}",
                FH::HASH_SIZE
            )));
        }

        //TODO: use array_chunks::<20> instead of chunks_exact when it becomes stable.
        let file_hash_slices = bytes
            .chunks_exact(FH::HASH_SIZE)
            .map(|chunk| {
                chunk.try_into().expect(static_format!(
                    "chunks_exact returns only chunks which are length {}",
                    FH::HASH_SIZE
                ))
            })
            .collect();

        Ok(file_hash_slices)
    }
}
