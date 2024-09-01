pub mod download_worker;

mod comms;
mod descriptor;
mod progress;
mod worker_fsm;

pub use comms::*;

use tokio::io::{AsyncRead, AsyncWrite};

pub trait PeerStream: AsyncWrite + AsyncRead + Unpin {}
impl<T: AsyncWrite + AsyncRead + Unpin> PeerStream for T {}

pub type PeerAddr = std::net::SocketAddrV4;
pub type PieceIndex = usize;
pub type PieceLength = u32;
type BlockLength = u32;
type BlockOffset = u32;
