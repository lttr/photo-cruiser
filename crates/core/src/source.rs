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

/// Who goes first when several threads wait for a gvfs file ([`one_at_a_time`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Background work nobody waits for (size probes for a layout).
    Low,
    #[default]
    Normal,
    /// The photo on screen and its neighbours.
    High,
}

thread_local! {
    static PRIORITY: std::cell::Cell<Priority> = const { std::cell::Cell::new(Priority::Normal) };
}

/// Run `f` with its gvfs file access at `priority` (on this thread).
pub fn with_priority<T>(priority: Priority, f: impl FnOnce() -> T) -> T {
    let before = PRIORITY.replace(priority);
    let out = f();
    PRIORITY.set(before);
    out
}

/// The gvfs lock: whether a file is open, and the tickets waiting, as (priority, number).
#[derive(Default)]
struct Turns {
    held: bool,
    next: u64,
    waiting: Vec<(Priority, u64)>,
}

static GVFS: std::sync::Mutex<Turns> =
    std::sync::Mutex::new(Turns { held: false, next: 0, waiting: Vec::new() });
static FREED: std::sync::Condvar = std::sync::Condvar::new();

/// Held while a gvfs file is open; lets the next waiter in when dropped.
pub(crate) struct Turn(());

impl Drop for Turn {
    fn drop(&mut self) {
        GVFS.lock().unwrap_or_else(|e| e.into_inner()).held = false;
        FREED.notify_all();
    }
}

/// Hold the returned guard while a file under `path` is open. gvfs-fuse (SMB shares under
/// `/run/user/<uid>/gvfs/`) fails reads with EIO on an open file once another file on it
/// has been read in the meantime, so in this process only one gvfs file is open at a
/// time. Waiters go in order of their thread's [`Priority`], then of arrival: a plain
/// mutex let a run of size probes keep the photo on screen waiting for seconds. `None`
/// (no lock) elsewhere.
pub(crate) fn one_at_a_time(path: &Path) -> Option<Turn> {
    if !is_gvfs(path) {
        return None;
    }
    let mut t = GVFS.lock().unwrap_or_else(|e| e.into_inner());
    let ticket = (PRIORITY.get(), t.next);
    t.next += 1;
    t.waiting.push(ticket);
    // First: highest priority, then lowest number.
    let first = |t: &Turns| t.waiting.iter().copied().max_by_key(|&(p, n)| (p, std::cmp::Reverse(n)));
    while t.held || first(&t) != Some(ticket) {
        t = FREED.wait(t).unwrap_or_else(|e| e.into_inner());
    }
    t.waiting.retain(|w| *w != ticket);
    t.held = true;
    Some(Turn(()))
}

/// A local path for `path` when it is a URI: `file:///a%20b` becomes `/a b`, and
/// `smb://host/share/dir` becomes its gvfs mount, `$XDG_RUNTIME_DIR/gvfs/
/// smb-share:server=host,share=share/dir`. Folder pickers and file managers hand out such
/// URIs for network shares. Anything else is returned unchanged.
pub fn local_path(path: &Path) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    uri_to_path(path, runtime.as_deref()).unwrap_or_else(|| path.to_path_buf())
}

fn uri_to_path(path: &Path, runtime: Option<&Path>) -> Option<PathBuf> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let s = path.to_str()?;
    if let Some(rest) = s.strip_prefix("file://") {
        // `file:///a` or `file://localhost/a`.
        let rest = &rest[rest.find('/')?..];
        return Some(PathBuf::from(std::ffi::OsString::from_vec(percent_decode(rest))));
    }
    let rest = s.strip_prefix("smb://")?;
    let (authority, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let (share, rest) = rest.split_once('/').unwrap_or((rest, ""));
    if share.is_empty() {
        return None;
    }
    let (user, server) = match authority.rsplit_once('@') {
        Some((user, server)) => (Some(user.split_once(':').map_or(user, |(u, _)| u)), server),
        None => (None, authority),
    };
    let decode = |s: &str| String::from_utf8_lossy(&percent_decode(s)).into_owned();
    let mut mount = format!("smb-share:server={},share={}", decode(server), decode(share));
    if let Some(user) = user {
        mount += &format!(",user={}", decode(user));
    }
    let mut out = runtime?.join("gvfs").join(mount);
    out.push(std::ffi::OsStr::from_bytes(&percent_decode(rest)));
    Some(out)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).and_then(|&c| hex(c)), b.get(i + 2).and_then(|&c| hex(c))) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn is_gvfs(path: &Path) -> bool {
    let Ok(abs) = std::path::absolute(path) else { return false };
    abs.starts_with("/run/user") && abs.components().take(5).any(|c| c.as_os_str() == "gvfs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_become_local_paths() {
        let run = Some(Path::new("/run/user/1000"));
        let to = |s: &str| uri_to_path(Path::new(s), run);
        assert_eq!(
            to("smb://diskstation.local/photo/Rodina/2025-06%20Majda%20narozky/"),
            Some(PathBuf::from(
                "/run/user/1000/gvfs/smb-share:server=diskstation.local,share=photo/Rodina/2025-06 Majda narozky/"
            ))
        );
        assert_eq!(
            to("smb://nas/photo"),
            Some(PathBuf::from("/run/user/1000/gvfs/smb-share:server=nas,share=photo"))
        );
        assert_eq!(
            to("smb://me@nas/photo/a.jpg"),
            Some(PathBuf::from("/run/user/1000/gvfs/smb-share:server=nas,share=photo,user=me/a.jpg"))
        );
        assert_eq!(to("file:///tmp/a%20b%C3%A9"), Some(PathBuf::from("/tmp/a bé")));
        assert_eq!(to("file://localhost/tmp/x"), Some(PathBuf::from("/tmp/x")));
        assert_eq!(to("smb://nas"), None);
        assert_eq!(to("/home/me/100%25 photos"), None);
        assert_eq!(local_path(Path::new("rel/dir")), PathBuf::from("rel/dir"));
    }

    #[test]
    fn gvfs_paths_are_serialised() {
        assert!(is_gvfs(Path::new("/run/user/1000/gvfs/smb-share:server=nas,share=photo/a.jpg")));
        assert!(!is_gvfs(Path::new("/home/me/gvfs/a.jpg")));
        assert!(!is_gvfs(Path::new("/run/user/1000/a.jpg")));
        assert!(one_at_a_time(Path::new("/tmp/a.jpg")).is_none());
    }

    #[test]
    fn gvfs_turns_by_priority_then_arrival() {
        use std::sync::{Arc, Mutex};
        use std::time::Duration;
        let path = Path::new("/run/user/1000/gvfs/smb-share:server=nas,share=photo/a.jpg");
        let order = Arc::new(Mutex::new(Vec::new()));
        let first = one_at_a_time(path);
        let mut threads = Vec::new();
        // Queued one after another while the lock is held.
        for (name, p) in [
            ("low", Priority::Low),
            ("normal1", Priority::Normal),
            ("high", Priority::High),
            ("normal2", Priority::Normal),
        ] {
            let order = order.clone();
            threads.push(std::thread::spawn(move || {
                with_priority(p, || {
                    let _t = one_at_a_time(path);
                    order.lock().unwrap().push(name);
                })
            }));
            while GVFS.lock().unwrap().waiting.len() < threads.len() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        drop(first);
        threads.into_iter().for_each(|t| t.join().unwrap());
        assert_eq!(*order.lock().unwrap(), ["high", "normal1", "normal2", "low"]);
    }
}
