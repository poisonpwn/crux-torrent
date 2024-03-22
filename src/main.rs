mod cli;
mod metainfo;
mod tracker;

use cli::Cli;

use clap::Parser;
use tracker::{
    request::{PeerId, TrackerRequest},
    HttpTracker,
};

use crate::{metainfo::tracker_url::TrackerUrl, tracker::Announce};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let matches = Cli::parse();
    let metainfo = metainfo::Metainfo::from_bencode_file(matches.source)?;
    let request = TrackerRequest::new(PeerId::random(), matches.port, &metainfo.file_info)?;

    let client = reqwest::Client::new();
    let response = match metainfo.announce {
        TrackerUrl::UDP(udp_url) => todo!(),
        TrackerUrl::HTTP(http_url) => {
            HttpTracker::new(&client, http_url)
                .announce(&request)
                .await?
        }
    };
    dbg!(response);

    Ok(())
}
