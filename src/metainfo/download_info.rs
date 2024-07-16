use super::{FileInfo, PieceHash};
use crate::torrent::InfoHash;
use crate::tracker::request::Requestable;
use serde::{Deserialize, Serialize};
use sha1_smol::Sha1;

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum DownloadInfo {
    MultiFile {
        #[serde(rename = "name")]
        dirname: String,

        files: Vec<FileInfo>,

        #[serde(rename = "piece length")]
        piece_length: usize,

        #[serde(with = "piece_hashes_parser")]
        pieces: Vec<PieceHash>,

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

        #[serde(with = "piece_hashes_parser")]
        pieces: Vec<PieceHash>,

        #[serde(default)]
        private: Option<i64>,
    },
}

impl Requestable for DownloadInfo {
    fn get_info_hash(&self) -> anyhow::Result<InfoHash> {
        let info_hash = serde_bencode::to_bytes(self)?;
        Ok(InfoHash::new(Sha1::from(info_hash).digest().bytes()))
    }

    fn get_request_length(&self) -> usize {
        match self {
            Self::SingleFile { length, .. } => *length,
            Self::MultiFile { files, .. } => files.iter().map(|file| file.length).sum(),
        }
    }
}

mod piece_hashes_parser {
    use crate::metainfo::PieceHash;
    use serde::de::{self, Visitor};
    use static_str_ops::static_format;
    const HASH_SIZE: usize = std::mem::size_of::<PieceHash>();

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<PieceHash>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(PieceHashVisitor)
    }

    pub fn serialize<S>(
        piece_hashes: impl AsRef<[PieceHash]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde_bytes::serialize(&piece_hashes.as_ref().concat(), serializer)
    }

    struct PieceHashVisitor;
    impl<'de> Visitor<'de> for PieceHashVisitor {
        type Value = Vec<PieceHash>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(static_format!(
                "a byte sequence whose length is a multiple of {}",
                HASH_SIZE
            ))
        }

        fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let n_bytes = bytes.len();

            if n_bytes % HASH_SIZE != 0 {
                return Err(E::custom(static_format!(
                    "piece hash pieces should be a multiple of length {}",
                    HASH_SIZE
                )));
            }

            //TODO: use array_chunks::<20> instead of chunks_exact when it becomes stable.
            let piece_hash_slices = bytes
                .chunks_exact(HASH_SIZE)
                .map(|chunk| {
                    chunk.try_into().expect(static_format!(
                        "chunks_exact returns only chunks which are length {}",
                        HASH_SIZE
                    ))
                })
                .collect();

            Ok(piece_hash_slices)
        }
    }
}
