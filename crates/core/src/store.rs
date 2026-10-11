//! Saved edits. A photo's edits live in a sidecar; Save renders them into the photo file
//! itself, and the untouched original moves to a hidden backup folder:
//!
//! ```text
//! <dir>/IMG_1234.jpg                              the edited result (after Save)
//! <dir>/.photo-cruiser/edits/IMG_1234.jpg.json    sidecar, keyed by the original's name
//! <dir>/.photo-cruiser/originals/IMG_1234.jpg     the original, moved there by the first Save
//! ```
//!
//! Saves always render from the original, so they never stack quality loss. Revert moves
//! the backup back, byte for byte. In a read-only folder the sidecar goes to
//! `$XDG_DATA_HOME/photo-cruiser/edits/<sha256 of the photo's path>.json` (edits and undo
//! still work; Save fails with [`Error::ReadOnly`]).

use crate::decode::{self, Format, Metadata};
use crate::edit::{EDIT_VERSION, EditStack, Op};
use crate::export::{self, Encoding};
use crate::meta::{self, JpegSettings};
use crate::render::{self, RenderOpts};
use crate::{Error, Rect, Result};
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

/// The hidden folder next to the photos.
pub const DIR: &str = ".photo-cruiser";

/// Quality of re-encoded lossy photos when the original's can't be kept, and the floor
/// the original's JPEG tables are raised to.
pub const QUALITY: u8 = 95;

/// A photo's sidecar.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PhotoEdits {
    /// The current stack (what the app shows).
    #[serde(flatten)]
    pub stack: EditStack,
    /// The stack the file on disk was rendered from; `None` if never saved.
    #[serde(default)]
    pub saved: Option<Saved>,
    /// The photo file as Save left it (to notice changes by other programs).
    #[serde(default)]
    pub file: Option<FileStamp>,
    /// The backup of the original, if the photo was saved (filled in by [`load`]).
    #[serde(skip)]
    pub backup: Option<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub ops: Vec<Op>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    pub name: String,
    pub size: u64,
    /// RFC 3339, UTC, nanoseconds.
    pub mtime: String,
}

impl PhotoEdits {
    pub fn new(stack: EditStack) -> Self {
        PhotoEdits { stack, ..Default::default() }
    }

    /// The ops the file on disk was rendered from (none if never saved).
    pub fn saved_ops(&self) -> &[Op] {
        self.saved.as_ref().map_or(&[], |s| &s.ops)
    }

    /// The current stack differs from the one the file was saved with.
    pub fn dirty(&self) -> bool {
        self.stack.ops != self.saved_ops()
    }

    /// Nothing to remember: no edits, never saved.
    fn is_blank(&self) -> bool {
        self.stack.is_empty() && self.saved_ops().is_empty() && self.file.is_none()
    }
}

/// What a [`save`] did.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SaveReport {
    /// The photo's path now (a converted photo has a new extension).
    pub path: PathBuf,
    /// `saved`, `reverted` or `unchanged` (nothing to save).
    pub action: &'static str,
    /// Format of the file written (`jpeg`, `png`, `webp`, ...).
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    /// `original tables`, `raised to 95`, `default 95`, `95` (lossy WebP) or `lossless`.
    pub quality: String,
    pub ms: f64,
}

/// The photo's sidecar, `None` for an unedited photo. A sidecar whose photo was changed
/// by another program since our save is stale: it is dropped (the current file counts as
/// the original from now on) and [`Error::StaleEdits`] returned, a warning.
pub fn load(photo: &Path) -> Result<Option<PhotoEdits>> {
    match current(photo)? {
        (_, true) => Err(Error::StaleEdits { path: photo.to_path_buf() }),
        (None, false) => Ok(None),
        (Some(f), false) => {
            let (dir, _) = split(photo)?;
            let backup = owned_backup(&dir, &f);
            Ok(Some(PhotoEdits { backup, ..f.edits }))
        }
    }
}

/// Replace the photo's current stack in its sidecar, keeping what Save recorded there.
/// Stale edits are replaced.
pub fn store_stack(photo: &Path, stack: &EditStack) -> Result<()> {
    let mut edits = match load(photo) {
        Err(Error::StaleEdits { .. }) => None,
        res => res?,
    }
    .unwrap_or_default();
    edits.stack = stack.clone();
    store(photo, &edits)
}

/// The photos of a folder whose edits were never saved (the app was killed before it
/// saved them), by their sidecars in the folder.
pub fn dirty_photos(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir.join(DIR).join("edits")) else { return Vec::new() };
    let mut photos: Vec<PathBuf> = entries
        .filter_map(|e| {
            let path = e.ok()?.path();
            let key = path.file_name()?.to_str()?.strip_suffix(".json")?.to_string();
            let edits = read_sidecar(&path).ok()?;
            let name = edits.file.as_ref().map_or(key, |f| f.name.clone());
            let photo = dir.join(name);
            (edits.dirty() && photo.is_file()).then_some(photo)
        })
        .collect();
    photos.sort();
    photos
}

/// Write the sidecar (a blank one is removed). In a folder that can't be written, it goes
/// to the per-user fallback location.
pub fn store(photo: &Path, edits: &PhotoEdits) -> Result<()> {
    let (dir, name) = split(photo)?;
    let found = current(photo)?.0;
    let (key, path) = match &found {
        Some(f) => (f.key.clone(), f.path.clone()),
        None => (name.clone(), sidecar_path(&dir, &name)),
    };
    if edits.is_blank() && !backup_path(&dir, &key).exists() {
        if let Some(f) = found {
            remove(&f.path)?;
        }
        return Ok(());
    }
    let json = serde_json::to_string_pretty(edits).expect("edits serialise") + "\n";
    match write_atomic(&path, &json) {
        Err(e) if read_only(&e) => {
            let fallback = fallback_path(&dir, &key)?;
            write_atomic(&fallback, &json).map_err(|e| Error::io(&fallback, e))
        }
        res => res.map_err(|e| Error::io(&path, e)),
    }
}

/// The untouched original: the backup if Save made one, else the photo itself. Renders
/// (and `pcr export`) start from it, so a saved crop isn't applied twice.
pub fn original(photo: &Path) -> PathBuf {
    backup(photo).unwrap_or_else(|| photo.to_path_buf())
}

/// The backup of the original, if the photo was saved.
pub fn backup(photo: &Path) -> Option<PathBuf> {
    let (dir, _) = split(photo).ok()?;
    owned_backup(&dir, &current(photo).ok()?.0?)
}

/// The backup belongs to the photo only if its sidecar says the photo was saved.
fn owned_backup(dir: &Path, f: &Found) -> Option<PathBuf> {
    let b = backup_path(dir, &f.key);
    (f.edits.file.is_some() && b.exists()).then_some(b)
}

/// Render `stack` from the original into the photo file. The first save moves the
/// original to the backup folder; an empty stack moves it back (revert). A photo that
/// isn't dirty is left alone (`action: "unchanged"`). On failure the photo is still
/// there under its name, as the old file or the original.
pub fn save(photo: &Path, stack: &EditStack) -> Result<SaveReport> {
    let t0 = Instant::now();
    let (dir, name) = split(photo)?;
    let found = current(photo)?.0;
    let owned = found.as_ref().is_some_and(|f| owned_backup(&dir, f).is_some());
    let key = match &found {
        Some(f) if owned => f.key.clone(),
        _ => name.clone(),
    };
    let backup = backup_path(&dir, &key);
    let saved_ops = found.as_ref().map_or(&[][..], |f| f.edits.saved_ops());
    let report = |path: PathBuf, action, format: String, quality: String| -> Result<SaveReport> {
        let bytes = std::fs::metadata(&path).map_err(|e| Error::io(&path, e))?.len();
        let (width, height) = decode::probe(&path).map(|p| (p.width, p.height)).unwrap_or_default();
        let ms = (t0.elapsed().as_secs_f64() * 1e5).round() / 100.0;
        Ok(SaveReport { path, action, format, width, height, bytes, quality, ms })
    };
    let unchanged = |path: &Path| {
        let format = decode::probe(path).map_or_else(|_| String::new(), |p| p.format.to_string());
        report(path.to_path_buf(), "unchanged", format, String::new())
    };
    if (owned && stack.ops == saved_ops) || (!owned && stack.is_empty()) {
        if !owned && let Some(f) = &found {
            remove(&f.path)?;
        }
        return unchanged(photo);
    }
    if stack.is_empty() {
        // Revert: the original goes back, no decode, no encode.
        let target = dir.join(&key);
        rename(&backup, &target)?;
        if key != name && trash::delete(photo).is_err() {
            std::fs::remove_file(photo).map_err(|e| Error::io(photo, e))?;
        }
        remove(&found.expect("owned").path)?;
        let format = decode::probe(&target).map_or_else(|_| String::new(), |p| p.format.to_string());
        return report(target, "reverted", format, "original".into());
    }

    let source = if owned { backup.clone() } else { photo.to_path_buf() };
    let target = target_for(&source)?;
    let out_name = match target.converted {
        true => format!("{}.jpg", Path::new(&key).file_stem().unwrap_or_default().to_string_lossy()),
        false => key.clone(),
    };
    let out = dir.join(&out_name);
    if out_name != name && out.exists() {
        return Err(Error::NameTaken { path: out });
    }
    let tmp = dir.join(format!(".{out_name}.pc-tmp"));
    // Fail before the (slow) decode if the folder can't be written.
    std::fs::File::create(&tmp).map_err(|e| io_error(&tmp, &dir, e))?;
    let written = (|| {
        let src = decode::decode(&source)?;
        let stack = match &target.encoding {
            Encoding::Jpeg { settings, .. } if settings.subsampled() => snap_crop(stack, src.source_size()),
            _ => stack.clone(),
        };
        let img = render::render(&src, &stack, RenderOpts::default())?;
        let meta = decode::metadata(&source)?;
        let meta = Metadata {
            icc: meta.icc,
            exif: meta.exif.and_then(|e| meta::patch_exif(&e, img.width(), img.height())),
        };
        export::write(&img, &tmp, &target.encoding, &meta)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let source_meta = std::fs::metadata(&source).map_err(|e| Error::io(&source, e))?;
    let mtime = source_meta.modified().map_err(|e| Error::io(&source, e))?;
    let _ = std::fs::set_permissions(&tmp, source_meta.permissions());
    if !owned {
        if backup.exists() {
            // A backup left from before the photo was changed elsewhere: keep it aside.
            rename(&backup, &free_name(&backup))?;
        }
        let originals = backup.parent().expect("backup has a folder");
        std::fs::create_dir_all(originals).map_err(|e| io_error(originals, &dir, e))?;
        if let Err(e) = rename(photo, &backup) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if let Err(e) = rename(&tmp, &out) {
        if !owned {
            let _ = std::fs::rename(&backup, photo);
        }
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let _ = std::fs::File::options().write(true).open(&out).and_then(|f| f.set_modified(mtime));
    let edits = PhotoEdits {
        stack: stack.clone(),
        saved: Some(Saved { ops: stack.ops.clone() }),
        file: Some(stamp(&out)?),
        backup: None,
    };
    write_sidecar(&dir, &key, found.map(|f| f.path), &edits)?;
    report(out, "saved", target.format.into(), target.quality)
}

/// Restore the original and drop the edits: [`save`] with an empty stack.
pub fn revert(photo: &Path) -> Result<SaveReport> {
    save(photo, &EditStack::default())
}

/// How Save writes a photo of a given source.
struct Target {
    encoding: Encoding,
    /// Replaced by a JPEG (the name changes).
    converted: bool,
    format: &'static str,
    quality: String,
}

/// JPEG, PNG and WebP keep their format, the rest becomes JPEG. Refuses animations and
/// 16-bit PNGs (the pipeline is 8-bit).
fn target_for(source: &Path) -> Result<Target> {
    let format = decode::probe(source)?.format;
    let (lossless, animated) = match format {
        Format::Png => {
            use image::ImageDecoder;
            let file = std::fs::File::open(source).map_err(|e| Error::io(source, e))?;
            let d = image::codecs::png::PngDecoder::new(std::io::BufReader::new(file))
                .map_err(|e| Error::decode(source, e))?;
            if d.color_type().bytes_per_pixel() > d.color_type().channel_count() {
                return Err(Error::HighBitDepth { path: source.to_path_buf() });
            }
            (true, d.is_apng().map_err(|e| Error::decode(source, e))?)
        }
        Format::Gif => {
            use image::AnimationDecoder;
            let file = std::fs::File::open(source).map_err(|e| Error::io(source, e))?;
            let d = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file))
                .map_err(|e| Error::decode(source, e))?;
            (true, d.into_frames().take(2).count() > 1)
        }
        Format::Webp => {
            let bytes = std::fs::read(source).map_err(|e| Error::io(source, e))?;
            let info = meta::webp_info(&bytes).ok_or_else(|| Error::decode(source, "not a WebP file"))?;
            (info.lossless, info.animated)
        }
        _ => (false, false),
    };
    save_format(source, format, lossless, animated)
}

fn save_format(source: &Path, format: Format, lossless: bool, animated: bool) -> Result<Target> {
    if animated {
        return Err(Error::Animated { path: source.to_path_buf() });
    }
    let default_jpeg = |settings| Encoding::Jpeg { settings: Box::new(settings), quality: QUALITY };
    Ok(match format {
        Format::Jpeg => {
            let settings = decode::jpeg_settings(source)?.unwrap_or_default();
            let (settings, quality) = raise_tables(settings);
            Target { encoding: default_jpeg(settings), converted: false, format: "jpeg", quality }
        }
        Format::Png => {
            Target { encoding: Encoding::Png, converted: false, format: "png", quality: "lossless".into() }
        }
        Format::Webp => Target {
            encoding: Encoding::Webp { lossless, quality: QUALITY },
            converted: false,
            format: "webp",
            quality: if lossless { "lossless".into() } else { QUALITY.to_string() },
        },
        Format::Avif | Format::Heic | Format::Bmp | Format::Gif => Target {
            encoding: default_jpeg(JpegSettings::default()),
            converted: true,
            format: "jpeg",
            quality: format!("default {QUALITY}"),
        },
    })
}

/// Each table entry becomes `min(original, IJG at quality 95)`: a high-quality original
/// keeps its tables, a low-quality one gets finer ones. Returns the quality label.
fn raise_tables(mut s: JpegSettings) -> (JpegSettings, String) {
    let Some(tables) = &mut s.tables else { return (s, format!("default {QUALITY}")) };
    let floor = meta::ijg_tables(QUALITY);
    let mut raised = false;
    for (t, f) in tables.iter_mut().zip(floor) {
        for (v, f) in t.iter_mut().zip(f) {
            if *v > f {
                (*v, raised) = (f, true);
            }
        }
    }
    let label = if raised { format!("raised to {QUALITY}") } else { "original tables".into() };
    (s, label)
}

/// Snap the crop's top-left corner down to even pixels, so the chroma samples of a
/// subsampled source line up with the original's and the crop adds no colour blur.
fn snap_crop(stack: &EditStack, size: (u32, u32)) -> EditStack {
    let mut out = stack.clone();
    if let Some(px) = stack.crop().and_then(|r| r.to_pixels(size)) {
        let (x, y) = (px.x - px.x % 2, px.y - px.y % 2);
        out.set_crop(Some(Rect::new(x as f32, y as f32, px.w as f32, px.h as f32)));
    }
    out
}

/// A sidecar found for a photo.
struct Found {
    /// The original's file name (differs from the photo's after a conversion).
    key: String,
    path: PathBuf,
    edits: PhotoEdits,
}

/// The photo's valid sidecar, and whether a stale one was dropped.
fn current(photo: &Path) -> Result<(Option<Found>, bool)> {
    let Some(f) = find(photo)? else { return Ok((None, false)) };
    let stale = match &f.edits.file {
        Some(file) => stamp(photo).ok().as_ref() != Some(file),
        None => false,
    };
    if stale {
        remove(&f.path)?;
        return Ok((None, true));
    }
    Ok((Some(f), false))
}

fn find(photo: &Path) -> Result<Option<Found>> {
    let (dir, name) = split(photo)?;
    let mut keys = vec![name.clone()];
    // A photo that Save converted to JPEG keeps its sidecar under the original's name,
    // which has the same stem.
    let stem = Path::new(&name).file_stem().unwrap_or_default().to_string_lossy().into_owned();
    if let Ok(entries) = std::fs::read_dir(dir.join(DIR).join("edits")) {
        let mut more: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok()?.strip_suffix(".json").map(String::from))
            .filter(|k| *k != name && Path::new(k).file_stem().is_some_and(|s| s.to_string_lossy() == stem))
            .collect();
        more.sort();
        keys.extend(more);
    }
    for key in keys {
        for path in [Some(sidecar_path(&dir, &key)), fallback_path(&dir, &key).ok()].into_iter().flatten() {
            if !path.is_file() {
                continue;
            }
            let edits = read_sidecar(&path)?;
            if key != name && edits.file.as_ref().is_none_or(|f| f.name != name) {
                continue;
            }
            return Ok(Some(Found { key, path, edits }));
        }
    }
    Ok(None)
}

fn read_sidecar(path: &Path) -> Result<PhotoEdits> {
    let text = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
    let edits: PhotoEdits =
        serde_json::from_str(&text).map_err(|e| Error::Edits(format!("{}: {e}", path.display())))?;
    if edits.stack.version != EDIT_VERSION {
        return Err(Error::Edits(format!(
            "{}: unsupported version {} (expected {EDIT_VERSION})",
            path.display(),
            edits.stack.version
        )));
    }
    Ok(edits)
}

fn write_sidecar(dir: &Path, key: &str, at: Option<PathBuf>, edits: &PhotoEdits) -> Result<()> {
    let path = at.unwrap_or_else(|| sidecar_path(dir, key));
    let json = serde_json::to_string_pretty(edits).expect("edits serialise") + "\n";
    write_atomic(&path, &json).map_err(|e| Error::io(&path, e))
}

fn split(photo: &Path) -> Result<(PathBuf, String)> {
    let bad = || Error::io(photo, std::io::Error::new(ErrorKind::InvalidInput, "not a file path"));
    let name = photo.file_name().ok_or_else(bad)?.to_string_lossy().into_owned();
    let dir = match photo.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    Ok((dir, name))
}

fn sidecar_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(DIR).join("edits").join(format!("{key}.json"))
}

fn backup_path(dir: &Path, key: &str) -> PathBuf {
    dir.join(DIR).join("originals").join(key)
}

/// `$XDG_DATA_HOME/photo-cruiser/edits/<sha256 of the photo's absolute path>.json`.
fn fallback_path(dir: &Path, key: &str) -> Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| Error::io(dir, std::io::Error::new(ErrorKind::NotFound, "no HOME")))?;
    let abs = std::path::absolute(dir.join(key)).map_err(|e| Error::io(dir, e))?;
    let hash = Sha256::digest(abs.as_os_str().as_encoded_bytes());
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Ok(data.join("photo-cruiser/edits").join(format!("{hex}.json")))
}

/// A free `<path>~N` name, N from 1.
fn free_name(path: &Path) -> PathBuf {
    (1..)
        .map(|n| PathBuf::from(format!("{}~{n}", path.display())))
        .find(|p| !p.exists())
        .expect("a free name")
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = PathBuf::from(format!("{}.tmp", path.display()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn remove(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(Error::io(path, e)),
        _ => Ok(()),
    }
}

fn rename(from: &Path, to: &Path) -> Result<()> {
    std::fs::rename(from, to).map_err(|e| Error::io(from, e))
}

fn read_only(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem)
}

/// An I/O error, or [`Error::ReadOnly`] when the folder can't be written.
fn io_error(path: &Path, dir: &Path, e: std::io::Error) -> Error {
    if read_only(&e) { Error::ReadOnly { path: dir.to_path_buf() } } else { Error::io(path, e) }
}

fn stamp(path: &Path) -> Result<FileStamp> {
    let m = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
    let mtime = m.modified().map_err(|e| Error::io(path, e))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    Ok(FileStamp { name, size: m.len(), mtime: rfc3339(mtime) })
}

/// `2026-10-11T09:12:44.123456789Z`.
fn rfc3339(t: SystemTime) -> String {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let (days, secs) = ((d.as_secs() / 86400) as i64, d.as_secs() % 86400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}.{:09}Z", d.subsec_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rfc3339_dates() {
        let at = |s: u64| rfc3339(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        assert_eq!(at(0), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(at(1_791_710_000), "2026-10-11T09:13:20.000000000Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00.000000000Z");
    }

    #[test]
    fn sidecar_json_format() {
        let stack = EditStack::default().with(Op::Crop(Rect::new(1.0, 2.0, 3.0, 4.0)));
        let e = PhotoEdits::new(stack.clone());
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["ops"][0]["op"], "crop");
        assert!(json["saved"].is_null());
        assert!(e.dirty());
        let saved = PhotoEdits { saved: Some(Saved { ops: stack.ops.clone() }), ..e };
        assert!(!saved.dirty());
    }

    #[test]
    fn tables_raised_to_the_floor() {
        let floor = meta::ijg_tables(QUALITY);
        let fine = JpegSettings { tables: Some(meta::ijg_tables(98)), ..Default::default() };
        assert_eq!(raise_tables(fine.clone()), (fine, "original tables".into()));
        let coarse = JpegSettings { tables: Some(meta::ijg_tables(75)), ..Default::default() };
        let (s, label) = raise_tables(coarse);
        assert_eq!((s.tables, label.as_str()), (Some(floor), "raised to 95"));
        assert_eq!(raise_tables(JpegSettings::default()).1, "default 95");
    }

    #[test]
    fn snap_to_even() {
        let stack = EditStack::default().with(Op::Crop(Rect::new(11.0, 7.0, 50.0, 40.0)));
        assert_eq!(snap_crop(&stack, (100, 100)).crop(), Some(Rect::new(10.0, 6.0, 50.0, 40.0)));
    }
}
