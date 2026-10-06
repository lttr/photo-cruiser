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
        std::fs::read(path).map_err(|e| Error::io(path, e))
    }
}
