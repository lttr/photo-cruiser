//! A folder of photos: filtering and natural ordering.

use crate::Result;
use crate::source::{LocalDir, Source};
use serde::Serialize;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

/// Extensions (lower case) that are listed as photos.
pub const EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "heic", "heif"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Folder {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
}

impl Folder {
    /// List a local directory: top level only, hidden files skipped, known photo
    /// extensions (case-insensitive), natural case-insensitive order.
    pub fn open(path: impl AsRef<Path>) -> Result<Folder> {
        let path = path.as_ref();
        Folder::from_source(path, &LocalDir::new(path))
    }

    pub fn from_source(path: &Path, source: &impl Source) -> Result<Folder> {
        let mut entries: Vec<Entry> = source
            .list()?
            .into_iter()
            .filter(|e| e.is_file && !e.name.starts_with('.') && is_photo_name(&e.name))
            .map(|e| Entry { name: e.name, path: e.path })
            .collect();
        entries.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        Ok(Folder { path: path.to_path_buf(), entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Lower-case extension of `name`, if any.
pub fn extension(name: &str) -> Option<String> {
    Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase())
}

pub fn is_photo_name(name: &str) -> bool {
    extension(name).is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

/// Natural, case-insensitive compare: digit runs compare numerically (`img2 < img10`).
/// Ties fall back to a plain comparison so the order is total and stable.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let num = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.next_if(char::is_ascii_digit) {
                        s.push(c);
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (na, nb) = (num(&mut x), num(&mut y));
                let ord = na.len().cmp(&nb.len()).then(na.cmp(&nb));
                if ord.is_ne() {
                    return ord;
                }
            }
            (Some(p), Some(q)) => {
                let ord = p.to_lowercase().cmp(q.to_lowercase());
                if ord.is_ne() {
                    return ord;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::RawEntry;

    struct Fake(Vec<(&'static str, bool)>);

    impl Source for Fake {
        fn list(&self) -> Result<Vec<RawEntry>> {
            Ok(self
                .0
                .iter()
                .map(|(n, f)| RawEntry { name: n.to_string(), path: n.into(), is_file: *f })
                .collect())
        }
        fn read(&self, _: &Path) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["img10.jpg", "IMG2.jpg", "img1.jpg", "b.jpg", "A.jpg", "img02.jpg"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["A.jpg", "b.jpg", "img1.jpg", "IMG2.jpg", "img02.jpg", "img10.jpg"]);
    }

    #[test]
    fn filtering() {
        let src = Fake(vec![
            ("b.HEIC", true),
            ("a.jpg", true),
            (".hidden.jpg", true),
            ("notes.txt", true),
            ("dir.jpg", false),
            ("noext", true),
            ("c.Avif", true),
        ]);
        let f = Folder::from_source(Path::new("x"), &src).unwrap();
        let names: Vec<_> = f.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.jpg", "b.HEIC", "c.Avif"]);
    }
}
