pub mod files;
pub mod tracker_url;

use serde::Deserialize;
use serde_bencode;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct Metainfo {
    pub announce: tracker_url::TrackerUrl,

    #[serde(rename = "info")]
    pub file_info: files::FileInfo,

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

impl Metainfo {
    pub fn from_bencode_file(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let file_contents = fs::read(file)?;
        let metainfo: Metainfo =
            serde_bencode::from_bytes(&file_contents).map_err(anyhow::Error::msg)?;
        Ok(metainfo)
    }
}
