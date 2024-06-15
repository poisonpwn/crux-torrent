use crate::torrent::{InfoHash, PeerId};
use urlencoding;

#[derive(Debug, Clone)]
pub struct TrackerRequest {
    /// urlencoded byte representation of the sha1 hash of info.
    pub info_hash: InfoHash,

    /// unique peer id string of length 20 bytes.
    pub peer_id: PeerId,

    /// port to listen on
    pub port: u16,

    ///total amount uploaded, start with 0.
    pub uploaded: usize,

    /// total amount downloaded, start with 0
    pub downloaded: usize,

    /// total amount left in the file, set to file size in bytes.
    pub left: usize,

    /// boolean(encoded as a number) for whether to use the
    /// compact reprsentation usually enabled except for backwards compatibility.
    compact: u8,
}

impl TrackerRequest {
    pub fn new(peer_id: PeerId, port: u16, requestable: &impl Requestable) -> anyhow::Result<Self> {
        Ok(Self {
            info_hash: requestable.get_info_hash()?,
            peer_id,
            port,
            downloaded: 0,
            uploaded: 0,
            left: requestable.get_request_length(),
            compact: 1,
        })
    }

    pub fn to_url_query(&self) -> String {
        let query_pairs = [
            (
                "info_hash",
                urlencoding::encode_binary(&self.info_hash.as_ref()[..]).to_string(),
            ),
            (
                "peer_id",
                urlencoding::encode_binary(&self.peer_id.as_ref()[..]).to_string(),
            ),
            ("port", self.port.to_string()),
            ("uploaded", self.uploaded.to_string()),
            ("downloaded", self.downloaded.to_string()),
            ("left", self.left.to_string()),
            ("compact", self.compact.to_string()),
        ];
        let mut query_pairs = query_pairs.into_iter();
        // unwrap here should be fine as the query pairs iter is never empty.
        let (first_key, first_val) = query_pairs.next().unwrap();

        query_pairs
            .fold(
                // we don't need to percent encode again as string fields are alphanumeric.
                &mut format!("{}={}", first_key, first_val),
                |output: &mut String, (key, val)| {
                    output.extend(["&", key.as_ref(), "=", val.as_ref()]);
                    output
                },
            )
            .to_string()
    }
}

pub trait Requestable {
    fn get_info_hash(&self) -> anyhow::Result<InfoHash>;
    fn get_request_length(&self) -> usize;
}
