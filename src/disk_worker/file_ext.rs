use std::fs::File;
use std::io::{self, IoSlice};
use std::os::fd::AsFd;

pub trait PwritevExt {
    fn pwritev_all<B: AsRef<[u8]>>(&self, bufs: &[B], offset: u64) -> io::Result<()>;
}

impl PwritevExt for File {
    fn pwritev_all<B: AsRef<[u8]>>(&self, bufs: &[B], offset: u64) -> io::Result<()> {
        pwritev_all_with(bufs, offset, |iov, offset| pwritev(self, iov, offset))
    }
}

fn pwritev(fd: impl AsFd, iov: &[IoSlice<'_>], offset: u64) -> io::Result<usize> {
    nix::sys::uio::pwritev(fd, iov, offset as nix::libc::off_t).map_err(io::Error::from)
}

fn pwritev_all_with<B: AsRef<[u8]>>(
    bufs: &[B],
    offset: u64,
    mut do_write: impl FnMut(&[IoSlice<'_>], u64) -> io::Result<usize>,
) -> io::Result<()> {
    let total_len: usize = bufs.iter().map(|buf| buf.as_ref().len()).sum();
    let mut written: usize = 0;

    while written < total_len {
        let iov = remaining_iovecs(bufs, written);
        let n = do_write(&iov, offset + written as u64)?;

        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "pwritev wrote zero bytes",
            ));
        }

        written += n;
    }

    Ok(())
}

/// builds the iovec list for the bytes still unwritten, skipping the first `skip` bytes across
/// `bufs` (this could land in the middle of a buffer if the previous pwritev call was short).
fn remaining_iovecs<B: AsRef<[u8]>>(bufs: &[B], skip: usize) -> Vec<IoSlice<'_>> {
    let mut skip = skip;
    let mut iov = Vec::with_capacity(bufs.len());

    for buf in bufs {
        let buf = buf.as_ref();
        if skip >= buf.len() {
            skip -= buf.len();
            continue;
        }
        iov.push(IoSlice::new(&buf[skip..]));
        skip = 0;
    }

    iov
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::os::unix::fs::FileExt;

    #[test]
    fn single_call_when_backend_writes_everything_at_once() {
        let calls = Cell::new(0);
        let bufs: &[&[u8]] = &[b"hello ", b"world"];

        pwritev_all_with(bufs, 100, |iov, offset| {
            calls.set(calls.get() + 1);
            assert_eq!(offset, 100);
            let total: usize = iov.iter().map(|s| s.len()).sum();
            assert_eq!(total, 11);
            Ok(total)
        })
        .unwrap();

        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn retries_and_advances_offset_on_short_writes() {
        // backend that only ever accepts up to 3 bytes per call, but (like real pwritev) may
        // span multiple buffers within that cap.
        let bufs: &[&[u8]] = &[b"abc", b"defg", b"hi"];
        let mut seen: Vec<(u64, Vec<u8>)> = Vec::new();

        pwritev_all_with(bufs, 1000, |iov, offset| {
            const CAP: usize = 3;
            let mut chunk = Vec::with_capacity(CAP);
            for slice in iov {
                let take = std::cmp::min(slice.len(), CAP - chunk.len());
                chunk.extend_from_slice(&slice[..take]);
                if chunk.len() == CAP {
                    break;
                }
            }
            let n = chunk.len();
            seen.push((offset, chunk));
            Ok(n)
        })
        .unwrap();

        // "abcdefghi" written 3 bytes at a time, offsets advancing correctly. the third call
        // ("ghi") must span the boundary between the "defg" and "hi" buffers.
        assert_eq!(
            seen,
            vec![
                (1000, b"abc".to_vec()),
                (1003, b"def".to_vec()),
                (1006, b"ghi".to_vec()),
            ]
        );
    }

    #[test]
    fn zero_length_backend_write_is_an_error() {
        let bufs: &[&[u8]] = &[b"x"];
        let err = pwritev_all_with(bufs, 0, |_, _| Ok(0)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::WriteZero);
    }

    #[test]
    fn empty_bufs_never_calls_backend() {
        let bufs: &[&[u8]] = &[];
        let calls = Cell::new(0);
        pwritev_all_with(bufs, 0, |_, _| {
            calls.set(calls.get() + 1);
            Ok(0)
        })
        .unwrap();
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn writes_to_real_file_at_offset_via_pwritev() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let file = tmp.reopen().unwrap();
        file.set_len(20).unwrap();

        let bufs: &[&[u8]] = &[b"foo", b"bar", b"baz"];
        file.pwritev_all(bufs, 5).unwrap();

        let mut buf = [0u8; 9];
        file.read_exact_at(&mut buf, 5).unwrap();
        assert_eq!(&buf, b"foobarbaz");
    }
}
