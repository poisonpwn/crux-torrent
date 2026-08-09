use clap::ArgAction;
use clap::{self, Parser};

use crate::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct MetainfoFilePath(PathBuf);

impl MetainfoFilePath {
    pub fn new(path: impl Into<PathBuf>) -> eyre::Result<Self> {
        let path: PathBuf = path.into();

        if !path.is_file() {
            eyre::bail!("could not find file at {}", path.display());
        }

        // let extension_is_torrent = path
        //     .extension() // must have extension
        //     .is_some_and(|s| s == OsStr::new("torrent"));
        //
        // if !extension_is_torrent {
        //     eyre::bail!("torrent files must end have a .torrent extension");
        // }

        Ok(MetainfoFilePath(path))
    }
}

impl FromStr for MetainfoFilePath {
    type Err = eyre::Error;

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

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
/// a cli bittorrent (v1) client written in rust.
pub struct Cli {
    #[arg(required = true)]
    /// the source for the torrent information, i.e a torrent file.
    /// torrent files must have the .torrent extention
    pub source: MetainfoFilePath,

    #[arg(short, long, default_value = "8860")]
    /// the port on which to listen to incoming messages.
    pub port: u16,

    #[arg(short, long, action = ArgAction::Count, conflicts_with = "quiet")]
    pub verbose: u8,

    #[arg(short, long, conflicts_with = "verbose")]
    pub quiet: bool,

    #[arg(short, long)]
    pub generate_trace: bool,

    #[arg(short, long, default_value = ".")]
    /// the directory downloaded files will be written into.
    pub output_dir: PathBuf,
}
