mod cli;
mod metainfo;
mod tracker;

use cli::Cli;

use clap::Parser;
use tracker::request::{PeerId, TrackerRequest};

fn main() -> Result<(), anyhow::Error> {
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source)?;
    let request = TrackerRequest::new(PeerId::random(), matches.port, &metainfo.file_info)?;

    dbg!(&request);

    Ok(())
}
