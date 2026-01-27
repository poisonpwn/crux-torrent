use super::comms::PiecePickerMessage;
use super::{LockPool, PieceDone, PieceGaurd, PieceInfo, PieceQueue};
use crate::peers::PieceIndex;
use crate::{metainfo::PieceHash, prelude::*, torrent::Bitfield};
use std::mem::ManuallyDrop;
use std::{sync::Arc, time::Duration};

use tokio::sync::mpsc;

// struct which is given out by the handle to a worker. holds all the infromation needed to request
// the piece, and the permit to download the piece and a channel to send
pub struct PieceHandle {
    pub piece_id: PieceIndex,
    pub piece_length: u32,
    pub piece_hash: PieceHash,
    piece_gaurd: PieceGaurd,
    piece_tx: mpsc::Sender<PiecePickerMessage>,
}

impl PieceHandle {
    pub async fn submit(self, piece: Vec<u8>) -> anyhow::Result<()> {
        self.piece_tx
            .send(PiecePickerMessage::PieceDone(PieceDone {
                piece_id: self.piece_id,
                piece,
                _piece_gaurd: self.piece_gaurd,
            }))
            .await?;

        Ok(())
    }
}

/// this prototype class is created by the piece picker, gets cloned and passed to each peer
/// thread, which then takes its field objects to create a piece picker handle
#[derive(Debug, Clone)]
pub struct PiecePickerPrototype {
    piece_queue: Arc<PieceQueue>,
    lock_pool: Arc<LockPool>,
    piece_infos: Arc<Vec<PieceInfo>>,
    piece_tx: mpsc::Sender<PiecePickerMessage>,
}

impl PiecePickerPrototype {
    pub(super) fn new(
        piece_queue: Arc<PieceQueue>,
        lock_pool: Arc<LockPool>,
        piece_infos: Arc<Vec<PieceInfo>>,
        piece_tx: mpsc::Sender<PiecePickerMessage>,
    ) -> Self {
        Self {
            piece_queue,
            piece_tx,
            lock_pool,
            piece_infos,
        }
    }
}

/// this struct manages that manages comms about frequency information and manages updating the
/// bitfield of the peer (which pieces the peer has) and notifies the the piece picker of frequency
/// updates (piece have) and download completion (submit piece to piece picker and remove it from
/// queue). When the piece handle is created in a worker with the initial bitfield, it sends the
/// init message with a copy of the bitfield, which the piece picker uses to increment the
/// appropriate pieces' frequency count, when the handle is dropped (probably becuase the peer
/// died) the death message is sent to piece picker to decrement the frequency of the appropraiate
/// pieces.
#[derive(Debug, Clone)]
pub struct PiecePickerHandle {
    piece_queue: Arc<PieceQueue>,
    piece_infos: Arc<Vec<PieceInfo>>,
    lock_pool: Arc<LockPool>,
    // these two are wrapped in manually drop, they're to be used and dropped manually only AFTER
    // sending the Died message to piece picker
    piece_picker_tx: ManuallyDrop<mpsc::Sender<PiecePickerMessage>>,
    bitfield: ManuallyDrop<Bitfield>,
}

impl PiecePickerHandle {
    const IDLE_CHECK_WAIT_DURATION: Duration = Duration::from_millis(20);

    // dismantles a piece picker proto and uses it's fields to create a PiecePickerHandle
    // with the provided bitfield.
    pub async fn init_with_bitfield(
        proto: PiecePickerPrototype,
        bitfield: Bitfield,
    ) -> anyhow::Result<Self> {
        let init_mesg = PiecePickerMessage::Init(bitfield.clone());
        proto
            .piece_tx
            .send(init_mesg)
            .await
            .inspect_err(|_| warn!("failed to send bitfield init message to piece picker"))?;

        Ok(Self {
            lock_pool: proto.lock_pool,
            piece_queue: proto.piece_queue,
            piece_infos: proto.piece_infos,
            bitfield: ManuallyDrop::new(bitfield), // manually drop along with sending message to
            // piece picker in drop impl
            piece_picker_tx: ManuallyDrop::new(proto.piece_tx),
        })
    }
    pub async fn init_with_have_id(
        proto: PiecePickerPrototype,
        piece_id: PieceIndex,
    ) -> anyhow::Result<Self> {
        let npieces = proto.piece_infos.len();
        let bitfield = {
            let mut bf = Bitfield::new();
            bf.resize(npieces, false);
            bf.set(piece_id, true);
            bf
        };

        PiecePickerHandle::init_with_bitfield(proto, bitfield).await
    }

    #[instrument("handle have_piece", level = "debug", skip_all, fields(piece_id))]
    pub async fn have_piece(&mut self, piece_id: PieceIndex) -> anyhow::Result<()> {
        if self.bitfield.get(piece_id).is_none_or(|bit| *bit) {
            warn!("piece id bit to be set either doesn't exist or is already set");
            anyhow::bail!("piece id bit to be set either doesn't exist or is already set");
        }

        self.bitfield.set(piece_id, true);
        self.piece_picker_tx
            .send(PiecePickerMessage::Have(piece_id))
            .await?;
        Ok(())
    }

    #[instrument("handle next_piece", level = "debug", skip_all)]
    pub async fn next_piece(&self) -> anyhow::Result<PieceHandle> {
        debug!("fetching next piece to download from the download queue");
        loop {
            if self.piece_queue.is_empty() {
                debug!("piece picker queue is empty!");
            } else {
                debug!(piece_picker_queue_size = self.piece_queue.len());
            }

            // iterate in frequency order
            for entry in self.piece_queue.iter() {
                // if bitfield has piece bit set try acquiring it's lock else move on to next piece

                let piece_id = entry.piece_id;
                if self.bitfield.get(piece_id).is_none_or(|bitref| !(*bitref)) {
                    continue;
                }

                debug!("trying to acquire lock for piece: {}", piece_id);
                if let Some(gaurd) = self.lock_pool[piece_id].try_acquire() {
                    debug!("lock acquired for piece: {}", piece_id);
                    let PieceInfo {
                        piece_id,
                        hash,
                        length,
                    } = self.piece_infos[piece_id];

                    return Ok(PieceHandle {
                        piece_id,
                        piece_hash: hash,
                        piece_length: length,
                        piece_gaurd: gaurd,
                        piece_tx: (*self.piece_picker_tx).clone(),
                    });
                }
            }

            debug!("no pieces free to be downloaded");
            tokio::time::sleep(Self::IDLE_CHECK_WAIT_DURATION).await;
        }
    }
}

impl Drop for PiecePickerHandle {
    // when dropping the piece handle a Died message is sent to piece picker for it to decrement
    // all the frequency counts that this peer counts towards.
    fn drop(&mut self) {
        let Self {
            bitfield,
            piece_picker_tx,
            ..
        } = self;

        // SAFETY: since we're in the Drop::drop of this type, these fields cannot be accessed again,
        // so it is safe to call ManuallyDrop::take
        let bitfield = unsafe { ManuallyDrop::take(bitfield) };
        let piece_picker_tx = unsafe { ManuallyDrop::take(piece_picker_tx) };

        // spawn a new async task to push the message about the peer dying
        tokio::spawn(async move {
            let mesg = PiecePickerMessage::Died(bitfield);
            piece_picker_tx.send(mesg).await
        });
    }
}
