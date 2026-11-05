//! Where photos come from. Milestone 1 has only [`LocalDir`]; GVfs-mounted SMB shares are
//! plain paths and work through it unchanged. A native network source can implement
//! [`Source`] later.

use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// One item of a source listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_file: bool,
}

pub trait Source {
    /// Top-level entries, unfiltered and unsorted.
    fn list(&self) -> Result<Vec<RawEntry>>;
    /// The whole file.
    fn read(&self, path: &Path) -> Result<Vec<u8>>;
}

/// A directory on the local file system.
#[derive(Clone, Debug)]
pub struct LocalDir {
    pub root: PathBuf,
}

impl LocalDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalDir { root: root.into() }
    }
}

impl Source for LocalDir {
    fn list(&self) -> Result<Vec<RawEntry>> {
        let dir = std::fs::read_dir(&self.root).map_err(|e| Error::io(&self.root, e))?;
        Ok(dir
            .filter_map(|e| e.ok())
            .map(|e| {
                let path = e.path();
                // `is_file` follows symlinks, so linked photos are listed too.
                RawEntry { name: e.file_name().to_string_lossy().into_owned(), is_file: path.is_file(), path }
            })
            .collect())
    }

    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        read(path)
    }
}

/// The whole file, read while no other gvfs file is open (see [`one_at_a_time`]).
pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    let _one = one_at_a_time(path);
    std::fs::read(path).map_err(|e| Error::io(path, e))
}

static GVFS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Hold the returned guard while a file under `path` is open. gvfs-fuse (SMB shares under
/// `/run/user/<uid>/gvfs/`) fails reads with EIO on an open file once another file on it
/// has been read in the meantime, so in this process only one gvfs file is open at a
/// time. `None` (no lock) elsewhere.
pub(crate) fn one_at_a_time(path: &Path) -> Option<std::sync::MutexGuard<'static, ()>> {
    is_gvfs(path).then(|| GVFS.lock().unwrap_or_else(|e| e.into_inner()))
}

fn is_gvfs(path: &Path) -> bool {
    let Ok(abs) = std::path::absolute(path) else { return false };
    abs.starts_with("/run/user") && abs.components().take(5).any(|c| c.as_os_str() == "gvfs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gvfs_paths_are_serialised() {
        assert!(is_gvfs(Path::new("/run/user/1000/gvfs/smb-share:server=nas,share=photo/a.jpg")));
        assert!(!is_gvfs(Path::new("/home/me/gvfs/a.jpg")));
        assert!(!is_gvfs(Path::new("/run/user/1000/a.jpg")));
        assert!(one_at_a_time(Path::new("/tmp/a.jpg")).is_none());
    }
}
