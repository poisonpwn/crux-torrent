use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::metainfo::DownloadInfo;
use crate::prelude::*;

// represents the handle to a file to be downloaded, should be opened and preallocated to total
// length
pub struct DownloadFile {
    pub path: PathBuf,
    pub file: File,
    pub length: usize,
}

pub fn open_download_files(
    download_info: &DownloadInfo,
    root: impl AsRef<Path>,
) -> eyre::Result<Vec<DownloadFile>> {
    let root = root.as_ref();

    match download_info {
        DownloadInfo::SingleFile {
            filename, length, ..
        } => {
            let path = resolve_file_path(root, None, std::slice::from_ref(filename))?;
            let file = open_and_preallocate(&path, *length)?;
            Ok(vec![DownloadFile {
                path,
                file,
                length: *length,
            }])
        }

        DownloadInfo::MultiFile { dirname, files, .. } => files
            .iter()
            .map(|file_info| {
                let path = resolve_file_path(root, Some(dirname), &file_info.path)?;
                let file = open_and_preallocate(&path, file_info.length)?;
                Ok(DownloadFile {
                    path,
                    file,
                    length: file_info.length,
                })
            })
            .collect(),
    }
}

/// joins `root` with `dirname` (multi-file torrents only) and `components` (the per-file path
/// segments from the metainfo), rejecting any component that could escape `root`.
fn resolve_file_path(
    root: &Path,
    dirname: Option<&str>,
    components: &[String],
) -> eyre::Result<PathBuf> {
    eyre::ensure!(
        !components.is_empty(),
        "file entry in torrent metainfo has an empty path"
    );

    let mut path = root.to_path_buf();
    for component in dirname
        .into_iter()
        .chain(components.iter().map(String::as_str))
    {
        path.push(sanitize_path_component(component)?);
    }

    Ok(path)
}

/// rejects suspicious path segments from being used
fn sanitize_path_component(component: &str) -> eyre::Result<&str> {
    if component.is_empty() || component == "." || component == ".." {
        eyre::bail!(
            "refusing to use unsafe path component from torrent metainfo: {:?}",
            component
        );
    }

    if component.contains(std::path::MAIN_SEPARATOR) || component.contains('\0') {
        eyre::bail!(
            "refusing to use path component containing a separator or NUL byte: {:?}",
            component
        );
    }

    Ok(component)
}

#[instrument(level = "debug", skip_all, fields(path = %path.display(), length))]
fn open_and_preallocate(path: &Path, length: usize) -> eyre::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).wrap_err_with(|| {
            format!("failed to create parent directories for {}", path.display())
        })?;
    }

    debug!("opening download file");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .wrap_err_with(|| format!("failed to open download file at {}", path.display()))?;

    // because of holepunching this should be completely disk io free on linux with common filesystems
    file.set_len(length as u64)
        .wrap_err_with(|| format!("failed to preallocate download file at {}", path.display()))?;

    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metainfo::FileInfo;
    use rstest::rstest;

    #[rstest]
    #[case("ok")]
    #[case("file.txt")]
    #[case("...")] // three dots is not the same as ".."
    fn accepts_normal_components(#[case] component: &str) {
        assert_eq!(sanitize_path_component(component).unwrap(), component);
    }

    #[rstest]
    #[case("")]
    #[case(".")]
    #[case("..")]
    #[case("a/b")]
    #[case("/etc/passwd")]
    fn rejects_unsafe_components(#[case] component: &str) {
        assert!(sanitize_path_component(component).is_err());
    }

    #[test]
    fn single_file_torrent_opens_one_preallocated_file() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "movie.mkv".to_string(),
            length: 1234,
            md5sum: None,
            piece_length: 16384,
            pieces: Vec::new(),
            private: None,
        };

        let files = open_download_files(&info, root.path()).unwrap();

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, root.path().join("movie.mkv"));
        assert_eq!(files[0].file.metadata().unwrap().len(), 1234);
    }

    #[test]
    fn multi_file_torrent_nests_under_dirname_and_preallocates_each_file() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::MultiFile {
            dirname: "my-torrent".to_string(),
            files: vec![
                FileInfo {
                    path: vec!["a.txt".to_string()],
                    length: 10,
                    md5sum: None,
                },
                FileInfo {
                    path: vec!["subdir".to_string(), "b.txt".to_string()],
                    length: 20,
                    md5sum: None,
                },
            ],
            piece_length: 16384,
            pieces: Vec::new(),
            private: None,
        };

        let files = open_download_files(&info, root.path()).unwrap();

        assert_eq!(files.len(), 2);

        assert_eq!(files[0].path, root.path().join("my-torrent/a.txt"));
        assert_eq!(files[0].file.metadata().unwrap().len(), 10);

        assert_eq!(files[1].path, root.path().join("my-torrent/subdir/b.txt"));
        assert_eq!(files[1].file.metadata().unwrap().len(), 20);
        assert!(root.path().join("my-torrent/subdir").is_dir());
    }

    #[test]
    fn path_traversal_in_metainfo_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::MultiFile {
            dirname: "my-torrent".to_string(),
            files: vec![FileInfo {
                path: vec!["..".to_string(), "..".to_string(), "escaped".to_string()],
                length: 10,
                md5sum: None,
            }],
            piece_length: 16384,
            pieces: Vec::new(),
            private: None,
        };

        assert!(open_download_files(&info, root.path()).is_err());
        assert!(!root
            .path()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("escaped")
            .exists());
    }

    #[test]
    fn reopening_an_existing_file_does_not_truncate_it() {
        let root = tempfile::tempdir().unwrap();
        let info = DownloadInfo::SingleFile {
            filename: "resume.bin".to_string(),
            length: 8,
            md5sum: None,
            piece_length: 16384,
            pieces: Vec::new(),
            private: None,
        };

        {
            let files = open_download_files(&info, root.path()).unwrap();
            std::os::unix::fs::FileExt::write_at(&files[0].file, b"deadbeef", 0).unwrap();
        }

        let files = open_download_files(&info, root.path()).unwrap();
        let mut buf = [0u8; 8];
        std::os::unix::fs::FileExt::read_exact_at(&files[0].file, &mut buf, 0).unwrap();
        assert_eq!(&buf, b"deadbeef");
    }
}
