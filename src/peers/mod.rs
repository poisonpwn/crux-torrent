use std::net::SocketAddrV4;

pub mod download_worker;

mod progress;
mod request_window;

pub type PeerAddr = SocketAddrV4;
type BlockLength = u32;
type BlockOffset = u32;
