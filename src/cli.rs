use clap::{self, Parser};

use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct MetainfoFilePath(PathBuf);

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
}
