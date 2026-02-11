use std::path::PathBuf;

use crate::torrent::{PieceIndex, PieceLength};
mod noitree;

struct FileSlice {
    file_offset: u32,
}

struct FileInfo {
    path: PathBuf,
    md5sum: Option<String>,
}

struct Piece {
    index: PieceIndex,
    data: Vec<u8>,
}

struct DiskWorker {
    piece_length: PieceLength,
    torrent_length: usize,
}

impl DiskWorker {
    // fn new(
    //     fileinfos: &[crate::metainfo::FileInfo],
    //     piece_length: PieceLength,
    // ) -> anyhow::Result<(DiskWorker, mpsc::Sender<Piece>)> {
    //     let files = BTreeMap::new();
    //     let start = 0;
    //     for file in fileinfos {
    //         let len = file.length;
    //         files[(start, start + len)] = FileInfo {
    //             path: PathBuf::from(file.path),
    //             md5sum: file.md5sum,
    //         };
    //         start += len;
    //     }
    //     let torrent_length = start;
    //     let (piece_tx, piece_rx) = mpsc::channel();
    //
    //     let worker = DiskWorker {
    //         files,
    //         piece_rx,
    //         piece_length,
    //         torrent_length,
    //     };
    //
    //     (worker, tx)
    // }

    // async fn run(&mut self) {
    //     tokio::select! {
    //         Some(piece) = self.piece_rx.recv() => {
    //             self.handle_piece(piece);
    //         }
    //     }
    // }

    // fn handle_piece(&mut self, piece: Piece) {
    //     let begin = self.piece_length * piece.index;
    //     let end = begin + piece.data.len();
    // }
}
