pub mod files;
pub mod tracker_url;

use serde::Deserialize;
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

use std::ffi::OsStr;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct MetainfoFilePath(pub PathBuf);

impl MetainfoFilePath {
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

        Ok(MetainfoFilePath(path))
    }
}

impl FromStr for MetainfoFilePath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(s);
        Self::new(path)
    }
}

impl AsRef<Path> for MetainfoFilePath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}
