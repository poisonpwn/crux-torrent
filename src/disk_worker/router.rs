use tokio_util::bytes::Bytes;

use crate::prelude::*;
use crate::torrent::{Piece, PieceLength, TorrentLength};

// chunk of a file from a piece that got sliced up at file boundaries
// to only correspond to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChunk {
    pub file_index: usize,
    pub file_offset: usize,
    pub data: Bytes,
}

// offset into the flat torrent byte space. eg. piece_id * piece_size + inside_piece_offset would be the global index of a byte inside a piece.

pub type GlobalOffset = usize;
/// chops downloaded pieces at file boundaries, so each resulting [`FileChunk`] can be routed
/// straight to the file it belongs to.
pub struct PieceRouter {
    piece_length: PieceLength,
    // cumulative file-start offsets in the flat torrent space, ascending, length == n_files + 1.
    // i'th file occupies the half-open range [boundaries[i], boundaries[i + 1]).
    boundaries: Vec<GlobalOffset>,
}

impl PieceRouter {
    pub fn new(file_lengths: impl IntoIterator<Item = usize>, piece_length: PieceLength) -> Self {
        let mut boundaries = vec![0];
        let mut end = 0;
        for length in file_lengths {
            end += length;
            boundaries.push(end);
        }

        Self {
            piece_length,
            boundaries,
        }
    }

    /// splits `piece` into the [`FileChunk`]s it overlaps, in ascending file order. a piece
    /// that spans a zero-length file simply produces no chunk for that file.
    pub fn route(&self, piece: Piece) -> eyre::Result<Vec<FileChunk>> {
        let piece_start = piece.piece_id * self.piece_length as usize;
        let piece_end = piece_start + piece.data.len();
        let torrent_length = *self
            .boundaries
            .last()
            .expect("boundaries always has >= 1 entry");

        eyre::ensure!(
            piece_end <= torrent_length,
            "piece {} range [{}, {}) falls outside the torrent's total length ({})",
            piece.piece_id,
            piece_start,
            piece_end,
            torrent_length
        );

        let data = Bytes::from(piece.data);
        let n_files = self.boundaries.len() - 1;
        let file_starts = &self.boundaries[..n_files];

        // rightmost file whose start is <= piece_start, if a zero length file exists, two items in
        // boundaries would be equal, if the rightmost value is chosen, as to not choose any zero
        // length files to route to.
        let mut file_index = file_starts.partition_point(|&start| start <= piece_start);
        file_index = file_index.saturating_sub(1);

        let mut chunks = Vec::new();
        while file_index < n_files && self.boundaries[file_index] < piece_end {
            let file_start = self.boundaries[file_index];
            let file_end = self.boundaries[file_index + 1];

            let chunk_start = piece_start.max(file_start);
            let chunk_end = piece_end.min(file_end);

            if chunk_end > chunk_start {
                chunks.push(FileChunk {
                    file_index,
                    file_offset: chunk_start - file_start,
                    data: data.slice((chunk_start - piece_start)..(chunk_end - piece_start)),
                });
            }

            file_index += 1;
        }

        Ok(chunks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // piece data where byte `i` (relative to the start of the torrent, not the piece) has
    // value `i as u8`, so chunk contents can be checked against their absolute position.
    fn piece_at(piece_id: usize, piece_length: usize, len: usize) -> Piece {
        let start = piece_id * piece_length;
        Piece {
            piece_id,
            data: (start..start + len).map(|i| i as u8).collect(),
        }
    }

    #[test]
    fn piece_fully_inside_one_file() {
        // files: A=5, B=3, C=4 (total 12), piece_length=4
        let router = PieceRouter::new([5, 3, 4], 4);
        let chunks = router.route(piece_at(0, 4, 4)).unwrap();

        assert_eq!(
            chunks,
            vec![FileChunk {
                file_index: 0,
                file_offset: 0,
                data: Bytes::from_static(&[0, 1, 2, 3]),
            }]
        );
    }

    #[test]
    fn piece_spanning_two_files() {
        // files: A=5, B=3, C=4 (total 12), piece_length=4
        // piece 1 covers [4, 8): 1 byte in A ([4,5)), 3 bytes in B ([5,8))
        let router = PieceRouter::new([5, 3, 4], 4);
        let chunks = router.route(piece_at(1, 4, 4)).unwrap();

        assert_eq!(
            chunks,
            vec![
                FileChunk {
                    file_index: 0,
                    file_offset: 4,
                    data: Bytes::from_static(&[4]),
                },
                FileChunk {
                    file_index: 1,
                    file_offset: 0,
                    data: Bytes::from_static(&[5, 6, 7]),
                },
            ]
        );
    }

    #[test]
    fn piece_exactly_filling_the_last_file() {
        // files: A=5, B=3, C=4 (total 12), piece_length=4
        // piece 2 covers [8, 12): exactly all of C
        let router = PieceRouter::new([5, 3, 4], 4);
        let chunks = router.route(piece_at(2, 4, 4)).unwrap();

        assert_eq!(
            chunks,
            vec![FileChunk {
                file_index: 2,
                file_offset: 0,
                data: Bytes::from_static(&[8, 9, 10, 11]),
            }]
        );
    }

    #[test]
    fn piece_spanning_three_files_including_a_short_middle_one() {
        // files: A=3, B=1, C=6 (total 10), piece_length=8
        // piece 0 covers [0, 8): all of A ([0,3)), all of B ([3,4)), 4 bytes of C ([4,8))
        let router = PieceRouter::new([3, 1, 6], 8);
        let chunks = router.route(piece_at(0, 8, 8)).unwrap();

        assert_eq!(
            chunks,
            vec![
                FileChunk {
                    file_index: 0,
                    file_offset: 0,
                    data: Bytes::from_static(&[0, 1, 2]),
                },
                FileChunk {
                    file_index: 1,
                    file_offset: 0,
                    data: Bytes::from_static(&[3]),
                },
                FileChunk {
                    file_index: 2,
                    file_offset: 0,
                    data: Bytes::from_static(&[4, 5, 6, 7]),
                },
            ]
        );
    }

    #[test]
    fn zero_length_file_never_receives_a_chunk() {
        // files: A=4, B=0, C=4 (total 8), piece_length=8, one piece covering everything
        let router = PieceRouter::new([4, 0, 4], 8);
        let chunks = router.route(piece_at(0, 8, 8)).unwrap();

        assert_eq!(
            chunks,
            vec![
                FileChunk {
                    file_index: 0,
                    file_offset: 0,
                    data: Bytes::from_static(&[0, 1, 2, 3]),
                },
                FileChunk {
                    file_index: 2,
                    file_offset: 0,
                    data: Bytes::from_static(&[4, 5, 6, 7]),
                },
            ]
        );
    }

    #[test]
    fn undersized_last_piece() {
        // files: A=10 (total 10), piece_length=4 -> pieces of length 4, 4, 2
        let router = PieceRouter::new([10], 4);
        let chunks = router.route(piece_at(2, 4, 2)).unwrap();

        assert_eq!(
            chunks,
            vec![FileChunk {
                file_index: 0,
                file_offset: 8,
                data: Bytes::from_static(&[8, 9]),
            }]
        );
    }

    #[test]
    fn single_file_torrent_never_splits() {
        let router = PieceRouter::new([100], 16);
        let chunks = router.route(piece_at(1, 16, 16)).unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].file_index, 0);
        assert_eq!(chunks[0].file_offset, 16);
        assert_eq!(chunks[0].data.len(), 16);
    }

    #[test]
    fn piece_past_the_end_of_the_torrent_is_rejected() {
        let router = PieceRouter::new([10], 4);
        // piece_id 3 would cover [12, 16), entirely past the torrent's 10-byte length.
        let piece = Piece {
            piece_id: 3,
            data: vec![0; 4],
        };
        assert!(router.route(piece).is_err());
    }
}
