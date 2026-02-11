use memmap2::MmapMut;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio_util::bytes::Bytes;

use crate::torrent::{Piece, PieceLength, TorrentLength};
use std::fs::OpenOptions;

struct FileInfo {
    path: PathBuf,
    md5sum: Option<String>,
    store: MmapMut,
}

struct DiskWorker {
    files: BTreeMap<usize, FileInfo>, // file_starts -> FileInfo mapping, sorted ascending by file starts
    piece_length: PieceLength,
    torrent_length: TorrentLength,
}

impl DiskWorker {
    fn new(
        fileinfos: &[crate::metainfo::FileInfo],
        piece_length: PieceLength,
    ) -> anyhow::Result<Self> {
        let mut file_starts = BTreeMap::new();
        let mut start = 0;
        for file in fileinfos {
            let len = file.length;

            // FIXME: this concat is probably a bug?, check if it is actually valid.
            let path = PathBuf::from(file.path[..].concat());
            let store = unsafe {
                // SAFETY: todo!() guarentee that this file is only modified by this program/thread
                // ONLY, otherwise it will be unsafe and (might) crash
                MmapMut::map_mut(
                    &OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create(true)
                        .truncate(false)
                        .open(&path)?,
                )?
            };

            file_starts.insert(
                start,
                FileInfo {
                    md5sum: file.md5sum.clone(),
                    store,
                    path,
                },
            );
            start += len;
        }

        let torrent_length = start;

        let worker = DiskWorker {
            files: file_starts,
            piece_length,
            torrent_length,
        };

        Ok(worker)
    }

    fn write_piece(&mut self, piece: Piece) -> anyhow::Result<()> {
        let piece_start = piece.piece_id * self.piece_length as usize;
        let piece_end = piece.piece_id + piece.data.len();

        let mut piece = Bytes::from(piece.data); // this makes it zero copy and mostly zero alloc.
        let first = *self
            .files
            .range(..=piece_start)
            .next_back()
            .ok_or_else(|| anyhow::anyhow!("piece found to fall outside torrent range"))?
            .0;

        let mut iter = self.files.range_mut(&first..&piece_end).peekable();
        let mut cut_total = 0;

        // this has to be a while let Some loop because of the mutable borrow of the iterator.
        while let Some((start, file_info)) = iter.next() {
            let chunk_start = std::cmp::min(*start, piece_start);
            let chunk_end = if let Some((end, _)) = iter.peek() {
                **end
            } else {
                piece_end
            };
            let chunk = piece.split_to(chunk_end - chunk_start);
            cut_total += chunk_end - chunk_start;

            file_info.store[(chunk_start - start)..(chunk_start - start)].copy_from_slice(&chunk);
        }

        debug_assert!(cut_total == (piece_start - piece_end));
        Ok(())
    }
}
