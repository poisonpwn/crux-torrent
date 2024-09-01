use crate::peer_protocol::codec::PeerFrames;
use tokio::sync::mpsc;

use super::{PeerAlerts, PeerCommands};
use std::collections::VecDeque;

use super::PeerStream;
use super::PieceRequestInfo;
use crate::PeerId;
use std::net::SocketAddrV4;

#[derive(Debug)]
/// data struct that owns all the types which describe the state of the worker independent of the
/// download state.
pub(super) struct WorkerStateDescriptor<T: PeerStream> {
    pub peer_addr: SocketAddrV4,
    pub peer_id: PeerId,
    pub peer_stream: PeerFrames<T>,
    pub commands_rx: mpsc::Receiver<PeerCommands>,
    pub alerts_tx: mpsc::Sender<PeerAlerts>,
    pub download_queue: VecDeque<PieceRequestInfo>,
    pub peer_is_choked: bool,
    pub we_are_interested: bool,
}

impl<T> WorkerStateDescriptor<T>
where
    T: PeerStream,
{
    pub fn new(
        peer_stream: PeerFrames<T>,
        peer_addr: SocketAddrV4,
        peer_id: PeerId,
        alerts_tx: mpsc::Sender<PeerAlerts>,
        commands_rx: mpsc::Receiver<PeerCommands>,
    ) -> Self {
        Self {
            peer_stream,
            peer_addr,
            peer_id,
            alerts_tx,
            commands_rx,
            peer_is_choked: true,
            we_are_interested: false,
            download_queue: VecDeque::new(),
        }
    }
}
