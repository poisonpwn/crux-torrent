use anyhow;
use clap::{self, Parser};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

#[derive(Debug, Clone)]
struct TorrentFile(PathBuf);
impl FromStr for TorrentFile {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let path = PathBuf::from(s);

        if !path.is_file() {
            anyhow::bail!("could not find file at {}", path.display());
        }

        let extension_is_torrent = path
            .extension() // must have extension
            .is_some_and(|s| s == OsStr::new("torrent"));

        if !extension_is_torrent {
            anyhow::bail!("torrent files must end have a .torrent extension");
        }

        Ok(TorrentFile(path))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct File {
    path: Vec<String>,
    length: i64,

    #[serde(default)]
    md5sum: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
enum FileInfo {
    MultiFile {
        #[serde(rename = "name")]
        dirname: String,

        #[serde(default)]
        files: Option<Vec<File>>,

        #[serde(rename = "piece length")]
        piece_length: i64,
        pieces: ByteBuf,

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
        pieces: ByteBuf,

        private: Option<i64>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Torrent {
    announce: String,
    info: FileInfo,

    #[serde(default)]
    #[serde(rename = "announce-list")]
    announce_list: Option<Vec<Vec<String>>>,

    #[serde(default)]
    #[serde(rename = "creation date")]
    creation_date: Option<u64>, // seconds since unix epoch
    //
    #[serde(default)]
    #[serde(rename = "created by")]
    created_by: Option<String>,

    #[serde(default)]
    comment: Option<String>,

    #[serde(default)]
    encoding: Option<String>,
}

#[derive(Parser, Debug)]
#[command(author, about)]
struct Cli {
    #[arg(required = true)]
    source: TorrentFile,
}

fn main() {
    let matches = Cli::parse();
}
