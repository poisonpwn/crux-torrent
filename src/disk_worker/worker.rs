use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task;
use tokio_util::sync::CancellationToken;

use crate::prelude::*;
use crate::torrent::{Piece, PieceLength};

use super::{DownloadFile, PendingRanges, PieceRouter, PwritevExt, Run};

#[derive(Clone)]
pub struct DiskWorkerHandle {
    tx: mpsc::UnboundedSender<Message>,
}

impl DiskWorkerHandle {
    pub fn submit(&self, piece: Piece) -> eyre::Result<()> {
        self.tx
            .send(Message::Piece(piece))
            .map_err(|_| eyre::eyre!("disk worker task has shut down"))
    }
}

enum Message {
    Piece(Piece),
    CheckStale,
}

pub struct DiskWorker {
    files: Vec<DownloadFile>,
    router: PieceRouter,
    // one entry per file, same order/index as `files`.
    pending: Vec<PendingRanges>,
    flush_threshold_bytes: usize,
}

impl DiskWorker {
    /// a run gets flushed as soon as it reaches this size, regardless of age.
    const FLUSH_THRESHOLD_BYTES: usize = 4 * 1024 * 1024;
    // flush run if if any of its chunks gets this old.
    const STALE_RUN_MAX_AGE: Duration = Duration::from_secs(30);
    /// how often the background ticker checks for stale runs.
    const STALE_CHECK_INTERVAL: Duration = Duration::from_secs(5);

    pub fn new(
        files: Vec<DownloadFile>,
        piece_length: PieceLength,
        flush_threshold_bytes: usize,
    ) -> Self {
        let router = PieceRouter::new(files.iter().map(|f| f.length), piece_length);
        let pending = files.iter().map(|_| PendingRanges::new()).collect();

        Self {
            files,
            router,
            pending,
            flush_threshold_bytes,
        }
    }

    pub fn spawn(
        files: Vec<DownloadFile>,
        piece_length: PieceLength,
        shutdown_token: CancellationToken,
    ) -> (DiskWorkerHandle, task::JoinHandle<eyre::Result<()>>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

        let ticker_tx = tx.clone();
        // this ticker task periodically wakes up the disk worker even if there's no piece to check
        // any really any old runs.
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Self::STALE_CHECK_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = shutdown_token.cancelled() => {
                        debug!("disk worker stale-check ticker shutting down");
                        return;
                    }
                    _ = interval.tick() => {
                        if ticker_tx.send(Message::CheckStale).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        let join_handle = task::spawn_blocking(move || {
            let mut worker = DiskWorker::new(files, piece_length, Self::FLUSH_THRESHOLD_BYTES);

            while let Some(message) = rx.blocking_recv() {
                match message {
                    Message::Piece(piece) => worker.handle_piece(piece)?,
                    Message::CheckStale => worker.flush_stale(Self::STALE_RUN_MAX_AGE)?,
                }
            }

            info!("disk worker channel closed, flushing all remaining pending ranges before exit");
            worker.flush_all()
        });

        (DiskWorkerHandle { tx }, join_handle)
    }

    #[instrument(level = "debug", skip_all, fields(piece_id = piece.piece_id))]
    pub fn handle_piece(&mut self, piece: Piece) -> eyre::Result<()> {
        let piece_id = piece.piece_id;
        let chunks = self
            .router
            .route(piece)
            .wrap_err_with(|| format!("failed to route piece {piece_id} to its files"))?;

        let mut touched_files = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            self.pending[chunk.file_index].insert(chunk.file_offset, chunk.data);

            // is there a better way to do this check?
            if !touched_files.contains(&chunk.file_index) {
                touched_files.push(chunk.file_index);
            }
        }

        for file_index in touched_files {
            while let Some(run) =
                self.pending[file_index].take_largest_if_at_least(self.flush_threshold_bytes)
            {
                self.flush_run(file_index, run)?;
            }
        }

        Ok(())
    }

    pub fn flush_stale(&mut self, max_age: Duration) -> eyre::Result<()> {
        for file_index in 0..self.files.len() {
            while let Some(run) = self.pending[file_index].take_older_than(max_age) {
                debug!(
                    file = %self.files[file_index].path.display(),
                    age_secs = run.age().as_secs_f64(),
                    "flushing stale run"
                );
                self.flush_run(file_index, run)?;
            }
        }
        Ok(())
    }

    /// flushes every pending run, across every file, regardless of size or age. meant to be
    /// called once on shutdown, just so that no downloaded byte is left unwritten.
    pub fn flush_all(&mut self) -> eyre::Result<()> {
        for file_index in 0..self.files.len() {
            while let Some(run) = self.pending[file_index].take_largest() {
                self.flush_run(file_index, run)?;
            }
        }
        Ok(())
    }

    fn flush_run(&mut self, file_index: usize, run: Run) -> eyre::Result<()> {
        let download_file = &self.files[file_index];
        let (start, len) = (run.start, run.len());

        download_file
            .file
            .pwritev_all(run.chunks(), start as u64)
            .wrap_err_with(|| {
                format!(
                    "failed to flush {len} bytes at offset {start} to {}",
                    download_file.path.display()
                )
            })?;

        debug!(
            file = %download_file.path.display(),
            offset = start,
            len,
            "flushed run to disk"
        );
        Ok(())
    }

    #[cfg(test)]
    fn total_pending_bytes(&self) -> usize {
        self.pending
            .iter()
            .map(PendingRanges::total_pending_bytes)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::super::open_download_files;
    use super::*;
    use crate::metainfo::{DownloadInfo, FileInfo};
    use std::os::unix::fs::FileExt as _;

    fn read_file(path: &std::path::Path) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    fn piece(piece_id: usize, piece_length: usize, byte: u8, len: usize) -> Piece {
        let _ = piece_length;
        Piece {
            piece_id,
            data: vec![byte; len],
        }
    }

    #[test]
    fn pieces_stay_buffered_until_flush_all_is_called() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 30,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let path = files[0].path.clone();

        // threshold far above anything this test writes, so nothing auto-flushes.
        let mut worker = DiskWorker::new(files, 10, 1024);

        worker.handle_piece(piece(0, 10, 0xAA, 10)).unwrap();
        worker.handle_piece(piece(1, 10, 0xBB, 10)).unwrap();

        assert_eq!(worker.total_pending_bytes(), 20);
        assert_eq!(
            read_file(&path),
            vec![0u8; 30],
            "must still be all zero, nothing flushed yet"
        );

        worker.flush_all().unwrap();

        let mut expected = vec![0xAAu8; 10];
        expected.extend(vec![0xBBu8; 10]);
        expected.extend(vec![0u8; 10]); // piece 2 never arrived
        assert_eq!(read_file(&path), expected);
        assert_eq!(worker.total_pending_bytes(), 0);
    }

    #[test]
    fn a_run_crossing_the_threshold_is_flushed_automatically() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 20,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let path = files[0].path.clone();

        // threshold of 15: a single 10-byte piece must stay buffered, but once it merges with
        // its neighbour into a 20-byte run, that run must get auto-flushed.
        let mut worker = DiskWorker::new(files, 10, 15);

        worker.handle_piece(piece(0, 10, 0xAA, 10)).unwrap();
        assert_eq!(worker.total_pending_bytes(), 10);
        assert_eq!(read_file(&path), vec![0u8; 20]);

        worker.handle_piece(piece(1, 10, 0xBB, 10)).unwrap();

        // merged run (20 bytes) crossed the 15-byte threshold and should already be on disk.
        assert_eq!(worker.total_pending_bytes(), 0);
        let mut expected = vec![0xAAu8; 10];
        expected.extend(vec![0xBBu8; 10]);
        assert_eq!(read_file(&path), expected);
    }

    #[test]
    fn out_of_order_pieces_across_two_files_are_routed_independently() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::MultiFile {
            dirname: "t".to_string(),
            files: vec![
                FileInfo {
                    path: vec!["a.bin".to_string()],
                    length: 10,
                    md5sum: None,
                },
                FileInfo {
                    path: vec!["b.bin".to_string()],
                    length: 10,
                    md5sum: None,
                },
            ],
            piece_length: 20,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let (path_a, path_b) = (files[0].path.clone(), files[1].path.clone());

        let mut worker = DiskWorker::new(files, 20, 1024);
        // a single 20-byte piece spans both 10-byte files.
        worker.handle_piece(piece(0, 20, 0xCC, 20)).unwrap();
        worker.flush_all().unwrap();

        assert_eq!(read_file(&path_a), vec![0xCCu8; 10]);
        assert_eq!(read_file(&path_b), vec![0xCCu8; 10]);
    }

    #[test]
    fn flush_stale_flushes_small_runs_that_have_aged_out() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 10,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let path = files[0].path.clone();

        // threshold never reached by a single 10-byte piece.
        let mut worker = DiskWorker::new(files, 10, 1024);
        worker.handle_piece(piece(0, 10, 0xDD, 10)).unwrap();

        std::thread::sleep(Duration::from_millis(20));
        worker.flush_stale(Duration::from_millis(10)).unwrap();

        assert_eq!(read_file(&path), vec![0xDDu8; 10]);
        assert_eq!(worker.total_pending_bytes(), 0);
    }

    #[test]
    fn out_of_order_piece_arrival_still_produces_correct_bytes() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 40,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let path = files[0].path.clone();

        let mut worker = DiskWorker::new(files, 10, 1024);
        // arrive in shuffled order: 2, 0, 3, 1
        worker.handle_piece(piece(2, 10, 2, 10)).unwrap();
        worker.handle_piece(piece(0, 10, 0, 10)).unwrap();
        worker.handle_piece(piece(3, 10, 3, 10)).unwrap();
        worker.handle_piece(piece(1, 10, 1, 10)).unwrap();
        worker.flush_all().unwrap();

        let expected: Vec<u8> = (0..4u8).flat_map(|b| vec![b; 10]).collect();
        assert_eq!(read_file(&path), expected);
    }

    #[test]
    fn routing_error_propagates_out_of_handle_piece() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 10,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let mut worker = DiskWorker::new(files, 10, 1024);

        // piece_id 5 covers [50, 60), entirely past the 10-byte torrent.
        assert!(worker.handle_piece(piece(5, 10, 0, 10)).is_err());
    }

    #[test]
    fn flush_run_writes_at_the_correct_absolute_offset() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 100,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let path = files[0].path.clone();

        let mut worker = DiskWorker::new(files, 10, 1024);
        worker.handle_piece(piece(5, 10, 0xEE, 10)).unwrap(); // covers [50, 60)
        worker.flush_all().unwrap();

        let file = std::fs::File::open(&path).unwrap();
        let mut buf = [0u8; 10];
        file.read_exact_at(&mut buf, 50).unwrap();
        assert_eq!(buf, [0xEEu8; 10]);
    }

    #[tokio::test]
    async fn spawn_flushes_everything_once_shut_down() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "a.bin".to_string(),
            length: 20,
            md5sum: None,
            piece_length: 10,
            pieces: Vec::new(),
            private: None,
        };
        let files = open_download_files(&info, root.path()).unwrap();
        let path = files[0].path.clone();

        let shutdown_token = CancellationToken::new();
        let (handle, join_handle) = DiskWorker::spawn(files, 10, shutdown_token.clone());

        handle.submit(piece(0, 10, 0xAA, 10)).unwrap();
        handle.submit(piece(1, 10, 0xBB, 10)).unwrap();

        // dropping the handle closes the piece channel; cancelling the token stops the stale
        // ticker (which holds its own sender clone). once every sender is gone,
        // `rx.blocking_recv()` returns `None` and the worker flushes everything before exiting.
        drop(handle);
        shutdown_token.cancel();

        join_handle
            .await
            .expect("disk worker task panicked")
            .expect("disk worker returned an error");

        let mut expected = vec![0xAAu8; 10];
        expected.extend(vec![0xBBu8; 10]);
        assert_eq!(read_file(&path), expected);
    }
}
