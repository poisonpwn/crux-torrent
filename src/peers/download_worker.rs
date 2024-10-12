use super::{PeerAddr, PeerAlerts, PeerStream};
use futures::StreamExt;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::prelude::*;
use crate::torrent::PeerId;
use std::net::SocketAddrV4;

use super::descriptor::WorkerStateDescriptor;
use super::worker_fsm::WorkerState;

use crate::peer_protocol::codec::{self, PeerMessage};
use crate::peer_protocol::handshake::PeerHandshake;

pub struct PeerConnector<S: PeerStream> {
    peer_addr: PeerAddr,
    stream: S,
}

/// interface type between PeerAddr and PeerDownloadWorker
#[derive(Debug)]
pub struct PeerDownloaderConnection<S: PeerStream> {
    peer_addr: SocketAddrV4,
    peer_id: PeerId,
    stream: S,
}

#[derive(Debug)]
pub struct PeerDownloadWorker<T: PeerStream> {
    state: WorkerState,
    descriptor: WorkerStateDescriptor<T>,
}

impl PeerConnector<TcpStream> {
    #[instrument(name = "connect to peer", level = "info", fields(%peer_addr), skip_all)]
    pub async fn connect(peer_addr: PeerAddr) -> anyhow::Result<Self> {
        info!("connecting to peer");
        let stream = TcpStream::connect(peer_addr).await.inspect_err(|_| {
            error!("failed to connect to peer");
        })?;

        Ok(Self::from_parts(peer_addr, stream))
    }
}

impl<S: PeerStream> PeerConnector<S> {
    pub(super) fn from_parts(peer_addr: PeerAddr, stream: S) -> Self {
        Self { peer_addr, stream }
    }

    #[instrument(name = "handshake mode", level = "info", skip_all)]
    pub async fn handshake(
        self,
        handshake: PeerHandshake,
    ) -> anyhow::Result<PeerDownloaderConnection<S>> {
        //unwrap self inside function instead of the signature.
        let Self {
            peer_addr,
            mut stream,
        } = self;
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

impl<S: PeerStream> PeerDownloadWorker<S> {
    const COMMAND_BUFFER_SIZE: usize = 5;

    pub async fn init_from(
        PeerDownloaderConnection {
            stream,
            peer_id,
            peer_addr,
            ..
        }: PeerDownloaderConnection<S>,
        alerts_tx: mpsc::Sender<PeerAlerts>,
    ) -> anyhow::Result<PeerDownloadWorker<S>> {
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
    use crate::torrent::{Bitfield, InfoHash};

    use super::*;
    use bitvec::vec::BitVec;
    use codec::{PeerFrames, PeerMessageCodec};
    use rand::Rng;
    use rstest::{fixture, rstest};
    use std::net::Ipv4Addr;
    use tokio_test::io::{Builder, Mock};
    use tokio_util::{
        bytes::BytesMut,
        codec::{Encoder, FramedWrite},
    };

    #[fixture]
    fn client_peer_id() -> PeerId {
        PeerId::with_random_suffix()
    }

    #[fixture]
    fn info_hash() -> InfoHash {
        let mut rng = rand::thread_rng();
        InfoHash::new([0u8; InfoHash::INFO_HASH_SIZE].map(|_| rng.gen()))
    }

    #[fixture]
    fn peer_addr() -> PeerAddr {
        let mut rng = rand::thread_rng();
        let ip_addr = Ipv4Addr::from(rng.gen::<u32>());
        let port: u16 = rng.gen();
        PeerAddr::new(ip_addr, port)
    }

    #[rstest(peer_addr as test_addr)]
    #[tokio::test]
    async fn test_init(test_addr: PeerAddr) -> anyhow::Result<()> {
        let mut rng = rand::thread_rng();
        let test_peer_id = PeerId::new([0; PeerId::PEER_ID_SIZE].map(|_| rng.gen()));

        // Bitfield is 5bytes with all ones
        let test_bitfield = Bitfield::from_iter(std::iter::repeat(0xFF).take(5));

        let mut encoder = PeerMessageCodec::new();
        let mut buffer = BytesMut::new();
        encoder.encode(PeerMessage::Bitfield(test_bitfield.clone()), &mut buffer)?;

        let current = buffer.split();
        let mock_stream = Builder::new().read(&current).build();

        let (alerts_tx, mut alerts_rx) = mpsc::channel(12);
        let connx = PeerDownloaderConnection {
            peer_addr: test_addr,
            peer_id: test_peer_id,
            stream: mock_stream,
        };

        let peer_handle = tokio::spawn(PeerDownloadWorker::init_from(connx, alerts_tx));
        let message = alerts_rx.recv().await;

        assert!(message.is_some_and(|msg| {
            match msg {
                PeerAlerts::InitPeer {
                    peer_addr,
                    bitfield,
                    ..
                } => (peer_addr == test_addr) && (bitfield == test_bitfield),
                _ => false,
            }
        }));
        let _worker = peer_handle.await??;

        Ok(())
    }

    #[rstest(peer_addr as test_addr)]
    #[tokio::test]
    async fn test_handshake(
        info_hash: InfoHash,
        client_peer_id: PeerId,
        test_addr: PeerAddr,
    ) -> anyhow::Result<()> {
        let mut rng = rand::thread_rng();
        let handshake_sent = PeerHandshake::new(info_hash.clone(), client_peer_id);

        let test_peer_id = PeerId::new([0; PeerId::PEER_ID_SIZE].map(|_| rng.gen()));
        let handshake_back = PeerHandshake::new(info_hash.clone(), test_peer_id.clone());

        let mock_io = Builder::new()
            .write(handshake_sent.as_ref())
            .read(handshake_back.as_ref())
            .build();

        let connector = PeerConnector::from_parts(test_addr, mock_io);

        let connx = connector
            .handshake(handshake_sent)
            .await
            .expect("mock io should not fail, no errors other than io errors should be generated");

        assert_eq!(connx.peer_addr, test_addr);
        assert_eq!(connx.peer_id, test_peer_id);

        Ok(())
    }
}
