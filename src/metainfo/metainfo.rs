use super::files::FileInfo;
use super::url::TrackerUrl;
use serde::Deserialize;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize)]
pub struct Metainfo {
    pub announce: TrackerUrl,

    #[serde(rename = "info")]
    pub file_info: FileInfo,

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
    pub async fn from_bencode_file(file: impl AsRef<Path>) -> anyhow::Result<Self> {
        let file_contents = fs::read(file).await?;
        let metainfo: Metainfo =
            serde_bencode::from_bytes(&file_contents).map_err(anyhow::Error::msg)?;
        Ok(metainfo)
    }
}
