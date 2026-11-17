//! Export a folder tree into a new folder: the same sub-folders, every photo (or only the
//! starred ones) as a JPEG at archive quality, with its edits applied.
//!
//! - Edited photos render from the untouched original ([`store::original`]) with their
//!   current edit stack, so they go through one lossy step.
//! - Unedited JPEGs are copied byte for byte, unless the archive encoding is at least
//!   [`Options::min_saving`] smaller (a high-quality camera JPEG), or the photo is bigger
//!   than [`Options::max_side`]; then that is written.
//! - Everything else (HEIC, PNG, WebP, …) is encoded.
//! - Photos bigger than [`Options::max_side`] are scaled down (Lanczos).
//! - Chroma sampling follows the original (4:2:2 stays 4:2:2, PNG gets 4:4:4).
//! - ICC and EXIF (patched like Save does), the star and the file time carry over.
//!
//! The output folder must be new or empty: nothing is ever overwritten. One photo failing
//! doesn't stop the others; it is listed in the report. So are the files that aren't photos
//! pcr can read (videos, TIFF, …), which aren't exported at all.
//!
//! The defaults ([`Options::default`]) and why: the README's "Export and archive" table, with
//! the measurements in `.aiwork/2026-11-17_batch-export/research.md`.

use crate::catalog::{self, Folder, natural_cmp};
use crate::decode::{self, Format};
use crate::export::{self, Encoding};
use crate::render::{self, RenderOpts};
use crate::{EditStack, Error, Result, rating, store};
use image::RgbaImage;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime};

#[derive(Clone, Debug)]
pub struct Options {
    /// Only photos with a star.
    pub starred: bool,
    /// JPEG quality of the archive encoding.
    pub quality: u8,
    /// An unedited JPEG is re-encoded only if that saves at least this share of its size
    /// (0.2 = 20 %); otherwise it is copied.
    pub min_saving: f64,
    /// Photos with a longer side than this are scaled down to it; `None` keeps the size.
    pub max_side: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            starred: false,
            quality: export::ARCHIVE_QUALITY,
            min_saving: MIN_SAVING,
            max_side: Some(MAX_SIDE),
        }
    }
}

/// Default [`Options::max_side`]: the long side of 12 MP phone photos (4032×3024), so they
/// keep every pixel; only bigger ones are scaled. Enough for a 5K screen and an A4 print
/// at 300 ppi.
pub const MAX_SIDE: u32 = 4032;

/// See [`Options::min_saving`].
pub const MIN_SAVING: f64 = 0.2;

/// A decoded 24 MP photo with its render takes a few hundred MB; more workers than this
/// mostly add memory.
const MAX_WORKERS: usize = 8;

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub source: PathBuf,
    pub output: PathBuf,
    /// Photos looked at (after the star filter).
    pub count: usize,
    pub encoded: usize,
    pub copied: usize,
    pub failed: usize,
    /// Of the photos exported (originals, as read).
    pub source_bytes: u64,
    pub bytes: u64,
    pub ms: f64,
    pub files: Vec<FileReport>,
    /// Files under `source` that aren't photos pcr can read (videos, TIFF, RAW, documents),
    /// so nothing was exported for them. Hidden files and [`COMPANIONS`] aren't listed.
    pub unreadable: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileReport {
    pub source: PathBuf,
    pub output: PathBuf,
    /// `encoded`, `copied` or `failed`.
    pub action: &'static str,
    /// Why: `edited`, `converted` (not a JPEG), `resized` (unedited JPEG bigger than
    /// `max_side`), `smaller` (re-encoded unedited JPEG), `original` (copied: re-encoding
    /// wouldn't save enough). Empty when failed.
    pub reason: &'static str,
    /// Scaled down to `max_side`.
    pub resized: bool,
    pub width: u32,
    pub height: u32,
    /// Chroma sampling of the output, kept from the original: `4:2:0`, `4:2:2`, `4:4:4`.
    pub sampling: &'static str,
    /// The output name differs from the planned one because another photo has it.
    pub renamed: bool,
    pub source_bytes: u64,
    pub bytes: u64,
    pub ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<FileError>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileError {
    pub kind: &'static str,
    pub message: String,
}

/// One photo to export.
#[derive(Clone, Debug)]
struct Job {
    source: PathBuf,
    output: PathBuf,
    renamed: bool,
}

/// Export `source` (recursively) into `output`, which must not exist or be empty. Errors
/// only for the whole run (bad folders); a photo that fails is in the report.
pub fn export_tree(source: &Path, output: &Path, opts: &Options) -> Result<Report> {
    let t0 = Instant::now();
    if !source.is_dir() {
        return Err(Error::io(source, std::io::Error::from(std::io::ErrorKind::NotADirectory)));
    }
    let source = std::fs::canonicalize(source).map_err(|e| Error::io(source, e))?;
    check_output(output)?;
    std::fs::create_dir_all(output).map_err(|e| Error::io(output, e))?;
    let output_abs = std::fs::canonicalize(output).map_err(|e| Error::io(output, e))?;
    if output_abs == source {
        return Err(Error::NotEmpty { path: output.to_path_buf() });
    }

    let (mut photos, mut unreadable) = (Vec::new(), Vec::new());
    walk(&source, &output_abs, &mut photos, &mut unreadable)?;
    if opts.starred {
        photos.retain(|p| rating::read(p).is_ok_and(rating::is_starred));
    }
    let jobs = plan(&source, output, &photos);

    // One photo per CPU core, at most [`MAX_WORKERS`].
    let workers =
        std::thread::available_parallelism().map_or(1, |n| n.get()).min(MAX_WORKERS).min(jobs.len().max(1));
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<FileReport>>> = Mutex::new(vec![None; jobs.len()]);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let r = export_one(job, opts);
                    results.lock().unwrap_or_else(|e| e.into_inner())[i] = Some(r);
                }
            });
        }
    });
    let files: Vec<FileReport> =
        results.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().flatten().collect();

    let count = |action| files.iter().filter(|f| f.action == action).count();
    let done = || files.iter().filter(|f| f.action != "failed");
    Ok(Report {
        source,
        output: output.to_path_buf(),
        count: files.len(),
        encoded: count("encoded"),
        copied: count("copied"),
        failed: count("failed"),
        source_bytes: done().map(|f| f.source_bytes).sum(),
        bytes: done().map(|f| f.bytes).sum(),
        ms: ms(t0),
        files,
        unreadable,
    })
}

/// `output` must be missing or an empty directory.
fn check_output(output: &Path) -> Result<()> {
    match std::fs::read_dir(output) {
        Ok(mut entries) => match entries.next() {
            None => Ok(()),
            Some(_) => Err(Error::NotEmpty { path: output.to_path_buf() }),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let parent = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            match parent.is_dir() {
                true => Ok(()),
                false => Err(Error::io(parent, std::io::Error::from(std::io::ErrorKind::NotFound))),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => {
            Err(Error::NotEmpty { path: output.to_path_buf() })
        }
        Err(e) => Err(Error::io(output, e)),
    }
}

/// The photos under `dir` in natural order, each folder's photos before its sub-folders.
/// Hidden entries (`.photo-cruiser/` too), folder symlinks and `skip` (the output, if it
/// lies inside) are left out. Other files go to `unreadable`, except [`COMPANIONS`].
fn walk(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>, unreadable: &mut Vec<PathBuf>) -> Result<()> {
    out.extend(Folder::open(dir)?.entries.into_iter().map(|e| e.path));
    let mut subdirs: Vec<(String, PathBuf)> = Vec::new();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?.filter_map(|e| e.ok()) {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if name.starts_with('.') || path == skip {
            continue;
        }
        match e.file_type() {
            Ok(t) if t.is_dir() => subdirs.push((name, path)),
            Ok(_) if path.is_file() && !catalog::is_photo_name(&name) => {
                let ext = catalog::extension(&name).unwrap_or_default();
                if !COMPANIONS.contains(&ext.as_str()) {
                    files.push((name, path));
                }
            }
            _ => {}
        }
    }
    files.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    unreadable.extend(files.into_iter().map(|(_, p)| p));
    subdirs.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    for (_, sub) in subdirs {
        walk(&sub, skip, out, unreadable)?;
    }
    Ok(())
}

/// Files that belong to a photo and aren't missed when they're not exported: XMP sidecars
/// (the star carries over into the JPEG) and Apple's edit files.
const COMPANIONS: &[&str] = &["xmp", "aae"];

/// Output paths: the same relative folder, JPEG names kept, other formats as `<stem>.jpg`.
/// A name taken already (case-insensitive) gets ` (2)`, ` (3)`, …: JPEGs keep their own
/// names first, then the converted photos are named, each in walk order.
fn plan(source: &Path, output: &Path, photos: &[PathBuf]) -> Vec<Job> {
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut jobs: Vec<Option<Job>> = vec![None; photos.len()];
    for jpegs_first in [true, false] {
        for (i, photo) in photos.iter().enumerate() {
            let name = photo.file_name().unwrap_or_default().to_string_lossy();
            let ext = catalog::extension(&name).unwrap_or_default();
            let is_jpeg = matches!(ext.as_str(), "jpg" | "jpeg");
            if is_jpeg != jpegs_first {
                continue;
            }
            let rel = photo.strip_prefix(source).unwrap_or(photo);
            let dir = output.join(rel.parent().unwrap_or(Path::new("")));
            let stem = Path::new(&*name).file_stem().unwrap_or_default().to_string_lossy().into_owned();
            // A JPEG keeps its extension as written (`.JPG`, `.jpeg`).
            let ext = match is_jpeg {
                true => name.rsplit('.').next().unwrap_or("jpg").to_string(),
                false => "jpg".to_string(),
            };
            let key = |n: &str| dir.join(n.to_lowercase());
            let mut out_name = format!("{stem}.{ext}");
            let mut n = 1;
            while taken.contains(&key(&out_name)) {
                n += 1;
                out_name = format!("{stem} ({n}).{ext}");
            }
            taken.insert(key(&out_name));
            jobs[i] = Some(Job { source: photo.clone(), output: dir.join(out_name), renamed: n > 1 });
        }
    }
    jobs.into_iter().flatten().collect()
}

fn export_one(job: &Job, opts: &Options) -> FileReport {
    let t0 = Instant::now();
    let mut report = FileReport {
        source: job.source.clone(),
        output: job.output.clone(),
        action: "failed",
        reason: "",
        renamed: job.renamed,
        resized: false,
        width: 0,
        height: 0,
        sampling: "",
        source_bytes: 0,
        bytes: 0,
        ms: 0.0,
        error: None,
    };
    match write_one(job, opts, &mut report) {
        Ok(()) => {}
        Err(e) => {
            report.action = "failed";
            report.reason = "";
            report.bytes = 0;
            report.error = Some(FileError { kind: e.kind(), message: e.to_string() });
        }
    }
    report.ms = ms(t0);
    report
}

fn write_one(job: &Job, opts: &Options, report: &mut FileReport) -> Result<()> {
    let photo = &job.source;
    // Edits changed by another program since our save are ignored, as everywhere: the
    // photo file then counts as the original.
    let (original, stack) = match store::load(photo) {
        Ok(Some(edits)) => (store::original(photo), edits.stack),
        Ok(None) | Err(Error::StaleEdits { .. }) => (photo.clone(), EditStack::default()),
        Err(e) => return Err(e),
    };
    let star = rating::read(photo).unwrap_or(0);
    let src_meta = std::fs::metadata(&original).map_err(|e| Error::io(&original, e))?;
    report.source_bytes = src_meta.len();
    let mtime = src_meta.modified().ok();
    let format = decode::probe(&original)?.format;

    let dir = job.output.parent().expect("output has a folder");
    std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    let name = job.output.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.pc-tmp"));

    let written = (|| {
        let sampling = decode::chroma_sampling(&original)?;
        let out = encode(&original, &stack, opts.quality, sampling, opts.max_side)?;
        (report.width, report.height, report.resized) = (out.width, out.height, out.resized);
        report.sampling = export::sampling_name(sampling);
        let unedited_jpeg = format == Format::Jpeg && stack.is_empty();
        let copy = unedited_jpeg
            && !out.resized
            && (out.bytes.len() as f64) > report.source_bytes as f64 * (1.0 - opts.min_saving);
        if copy {
            std::fs::copy(&original, &tmp).map_err(|e| Error::io(&tmp, e))?;
            report.action = "copied";
            report.reason = "original";
        } else {
            export::write_bytes(&tmp, &out.bytes)?;
            report.action = "encoded";
            report.reason = match (stack.is_empty(), format) {
                (false, _) => "edited",
                (true, Format::Jpeg) if out.resized => "resized",
                (true, Format::Jpeg) => "smaller",
                (true, _) => "converted",
            };
        }
        // Before the rename: a photo whose star can't be written fails, with nothing left
        // behind, since `--starred` chose it for the star.
        rating::try_carry(&tmp, star)
    })();
    let written = written.and_then(|()| std::fs::rename(&tmp, &job.output).map_err(|e| Error::io(&tmp, e)));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Some(t) = mtime {
        set_mtime(&job.output, t);
    }
    report.bytes = std::fs::metadata(&job.output).map_err(|e| Error::io(&job.output, e))?.len();
    Ok(())
}

struct Encoded {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
    resized: bool,
}

/// `original` rendered with `stack`, scaled to `max_side`, as an archive JPEG with its
/// metadata.
fn encode(
    original: &Path,
    stack: &EditStack,
    quality: u8,
    sampling: (u8, u8),
    max_side: Option<u32>,
) -> Result<Encoded> {
    let src = decode::decode(original)?;
    let mut img = render::render(&src, stack, RenderOpts::default())?;
    let resized = match max_side.filter(|&m| m > 0 && img.width().max(img.height()) > m) {
        Some(m) => {
            img = downscale(&img, m);
            true
        }
        None => false,
    };
    let (width, height) = img.dimensions();
    let meta = export::saved_metadata(original, width, height)?;
    let bytes = export::encode_to_vec(&img, &Encoding::Archive { quality, sampling }, &meta)
        .map_err(|e| Error::encode(original, e))?;
    Ok(Encoded { bytes, width, height, resized })
}

/// Longer side `max_side`, Lanczos: sharper than the quick filter the app's previews use.
fn downscale(img: &RgbaImage, max_side: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let k = max_side as f64 / w.max(h) as f64;
    let (nw, nh) = (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1));
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::Lanczos3)
}

fn set_mtime(path: &Path, t: SystemTime) {
    let _ = std::fs::File::options().write(true).open(path).and_then(|f| f.set_modified(t));
}

fn ms(t0: Instant) -> f64 {
    (t0.elapsed().as_secs_f64() * 1e5).round() / 100.0
}
