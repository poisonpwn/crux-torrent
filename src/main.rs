use anyhow;
use clap::{self, Parser};

mod torrent;
use torrent::TorrentFilePath;

#[derive(Parser, Debug)]
#[command(author, about)]
struct Cli {
    #[arg(required = true)]
    source: TorrentFilePath,
}

fn main() -> Result<(), anyhow::Error> {
    let matches = Cli::parse();
    let torrent = matches.source.decode_file_contents()?;

    let info_hash = torrent.info.get_sha1_digest()?;

    dbg!(info_hash);
    Ok(())
}
