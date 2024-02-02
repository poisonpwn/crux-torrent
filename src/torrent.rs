use anyhow;
use sha1_smol::Sha1;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Serialize};
use serde_bencode;
use serde_bytes;
use static_str_ops::static_format;

#[derive(Debug, Clone)]
pub struct TorrentFilePath(PathBuf);

impl TorrentFilePath {
    pub fn new(path: impl Into<PathBuf>) -> Result<Self, anyhow::Error> {
        let path: PathBuf = path.into();

        if !path.is_file() {
            anyhow::bail!("could not find file at {}", path.display());
        }

        let extension_is_torrent = path
            .extension() // must have extension
            .is_some_and(|s| s == OsStr::new("torrent"));

        if !extension_is_torrent {
            anyhow::bail!("torrent files must end have a .torrent extension");
        }

        Ok(TorrentFilePath(path))
    }

    pub fn decode_file_contents(&self) -> Result<Torrent, anyhow::Error> {
        let file_contents = fs::read(&self.0)?;
        let torrent: Torrent =
            serde_bencode::from_bytes(&file_contents).map_err(anyhow::Error::msg)?;
        Ok(torrent)
    }
}

impl FromStr for TorrentFilePath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(s);
        Self::new(path)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileHashes(Vec<[u8; FileHashes::SHA1_SIZE]>);
impl FileHashes {
    const SHA1_SIZE: usize = 20; // length of the sha1 hash of a file
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
    type Value = Vec<[u8; FileHashes::SHA1_SIZE]>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str(static_format!(
            "a byte sequence  whose length is a multiple of {}",
            FileHashes::SHA1_SIZE
        ))
    }

    fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let len_bytes = bytes.len();

        if bytes.len() % FileHashes::SHA1_SIZE != 0 && len_bytes != 0 {
            return Err(E::custom(static_format!(
                "file hash pieces should be a multiple of length {}",
                FileHashes::SHA1_SIZE
            )));
        }

        let file_hashes = bytes
            .chunks_exact(FileHashes::SHA1_SIZE)
            .map(|chunk| {
                chunk.try_into().expect(static_format!(
                    "all chunks are size {}",
                    FileHashes::SHA1_SIZE
                ))
            })
            .collect();

        Ok(file_hashes)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    pub path: Vec<String>,
    pub length: i64,

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
        piece_length: i64,

        #[serde(serialize_with = "serde_bytes::serialize")]
        pieces: FileHashes,

        #[serde(default)]
        private: Option<i64>,
    },

    SingleFile {
        #[serde(rename = "name")]
        filename: String,
        length: i64,

        #[serde(default)]
        md5sum: Option<String>,

        #[serde(rename = "piece length")]
        piece_length: i64,

        #[serde(serialize_with = "serde_bytes::serialize")]
        pieces: FileHashes,

        private: Option<i64>,
    },
}

impl FileInfo {
    pub fn get_sha1_digest(&self) -> anyhow::Result<String> {
        let info_hash = serde_bencode::to_bytes(self)?;
        Ok(Sha1::from(info_hash).hexdigest())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Torrent {
    pub announce: String,
    pub info: FileInfo,

    #[serde(default)]
    #[serde(rename = "announce-list")]
    pub announce_list: Option<Vec<Vec<String>>>,

    #[serde(default)]
    #[serde(rename = "creation date")]
    pub creation_date: Option<u64>, // seconds since unix epoch
    //
    #[serde(default)]
    #[serde(rename = "created by")]
    pub created_by: Option<String>,

    #[serde(default)]
    pub comment: Option<String>,

    #[serde(default)]
    pub encoding: Option<String>,
}
