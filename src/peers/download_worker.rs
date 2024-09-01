use super::PeerAddr;
use super::PeerAlerts;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::prelude::*;
use crate::torrent::{InfoHash, PeerId};

use super::descriptor::WorkerStateDescriptor;
use super::worker_fsm::WorkerState;

use super::PeerStream;
use crate::peer_protocol::codec::{self, PeerMessage};
use crate::peer_protocol::handshake::PeerHandshake;

#[derive(Debug, Clone)]
pub struct PeerConnector<T: PeerStream> {
    peer_addr: PeerAddr,
    stream: T,
}

/// interface type between PeerAddr and PeerDownloadWorker
#[derive(Debug)]
pub struct PeerDownloaderConnection<S: PeerStream> {
    peer_addr: PeerAddr,
    peer_id: PeerId,
    stream: S,
}

#[derive(Debug)]
pub struct PeerDownloadWorker<S: PeerStream> {
    state: WorkerState,
    descriptor: WorkerStateDescriptor<S>,
}

pub struct HandshakeError<E: std::error::Error, S: PeerStream> {
    error: E,
    stream: S,
}

impl PeerConnector<TcpStream> {
    #[instrument(name = "connect to peer", level = "info", fields(%peer_addr), skip_all)]
    pub async fn connect(peer_addr: PeerAddr) -> anyhow::Result<Self> {
        info!("connecting to peer");
        let stream = TcpStream::connect(peer_addr).await.map_err(|e| {
            error!("connection failure");
            e
        })?;

        Ok(Self::from_parts(peer_addr, stream))
    }
}

impl<T: PeerStream> PeerConnector<T> {
    pub fn from_parts(peer_addr: PeerAddr, stream: T) -> Self {
        Self { peer_addr, stream }
    }

    #[instrument(name = "handshake mode", level = "info", skip_all)]
    pub async fn handshake(
        self,
        info_hash: InfoHash,
        peer_id: PeerId,
    ) -> anyhow::Result<PeerDownloaderConnection<T>> {
        let Self {
            peer_addr,
            mut stream,
            ..
        } = self;
        let handshake = PeerHandshake::new(info_hash, peer_id);
        let mut bytes = handshake.into_bytes();

        info!("sending handshake to peer");
        stream.write_all(&bytes).await?;

        info!("waiting for peer handshake");
        stream.read_exact(&mut bytes).await?;

        let handshake = PeerHandshake::from_bytes(bytes);
        info!("peer handshake received");
        debug!(peer_handshake_reply = ?handshake);

        Ok(PeerDownloaderConnection {
            stream,
            peer_id: handshake.peer_id,
            peer_addr,
        })
    }
}

impl<T: PeerStream> PeerDownloadWorker<T> {
    const COMMAND_BUFFER_SIZE: usize = 5;

    pub async fn init_from(
        PeerDownloaderConnection {
            stream,
            peer_id,
            peer_addr,
            ..
        }: PeerDownloaderConnection<T>,
        alerts_tx: mpsc::Sender<PeerAlerts>,
    ) -> anyhow::Result<Self> {
        let mut peer_stream = codec::upgrade_stream(stream);

        let msg = match peer_stream.next().await {
            Some(msg_res) => msg_res?,
            None => {
                warn!("peer closed connection before handshake");
                anyhow::bail!("peer closed connection before handshake");
            }
        };

        type PM = PeerMessage;
        let bitfield = match msg {
            PM::Bitfield(bitfield) => bitfield,
            _ => {
                warn!("first message sent by peer was not a bitfield");
                anyhow::bail!("first message sent by peer not bitfield {:?}", msg);
            }
        };

        let (commands_tx, commands_rx) = mpsc::channel(Self::COMMAND_BUFFER_SIZE);

        info!("sending init peer alert to engine");
        alerts_tx
            .send(PeerAlerts::InitPeer {
                peer_addr,
                bitfield,
                commands_tx,
            })
            .await?;
        let descriptor =
            WorkerStateDescriptor::new(peer_stream, peer_addr, peer_id, alerts_tx, commands_rx);

        Ok(Self {
            descriptor,
            state: WorkerState::Idle,
        })
    }

    pub async fn start_peer_event_loop(&mut self) -> anyhow::Result<()> {
        loop {
            self.state.transition(&mut self.descriptor).await?;
        }
    }
}

#[cfg(test)]
mod test {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::peers::PeerAddr;
    use rstest::rstest;

    #[rstest]
    fn test_handshake() {
        // TODO: mock TcpStream
        let connx = PeerConnector::from_parts(PeerAddr::new(Ipv4Addr::new(0, 0, 0, 0), 0), stream);
    }
}
