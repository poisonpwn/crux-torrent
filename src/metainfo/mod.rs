mod download_info;
mod fileinfo;
#[allow(clippy::module_inception)]
mod metainfo;
pub mod url;

pub type PieceHash = [u8; sha1_smol::DIGEST_LENGTH];

pub use download_info::DownloadInfo;
pub use fileinfo::FileInfo;
pub use metainfo::Metainfo;
