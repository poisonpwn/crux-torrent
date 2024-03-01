use clap::{self, Parser};

pub mod metainfo_file_path;
use metainfo_file_path::MetainfoFilePath;

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
