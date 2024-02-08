use clap::{self, Parser};

mod torrent;
use torrent::TorrentFilePath;
use tracker::request::{PeerId, TrackerRequest};

mod tracker;

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
/// a cli bittorrent (v1) client written in rust.
struct Cli {
    #[arg(required = true)]
    /// the source for the torrent information, i.e a torrent file.
    /// torrent files must have the .torrent extention
    source: TorrentFilePath,

    #[arg(short, long, default_value = "8860")]
    /// the port on which to listen to incoming messages.
    port: u16,
}

fn main() -> Result<(), anyhow::Error> {
    let matches = Cli::parse();
    let torrent = matches.source.decode_file_contents()?;

    let peer_id = PeerId::random();

    let request = TrackerRequest::new(peer_id, matches.port, &torrent.info)?;
    dbg!(request);

    Ok(())
}
