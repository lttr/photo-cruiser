//! The app model: folder, index, loader, what is on screen, crop, mode. Plain data +
//! [`Action`] handling, no iced runtime, so it can be driven directly in tests (decode
//! synchronously with [`decode_job`] and feed results to [`State::finish`]).
//!
//! Switching never blanks the image area: until the new photo is decoded, its embedded
//! preview is shown, or else the previous photo stays. Each change of the shown image
//! cross-fades over [`State::transition`], except while the user is flipping through
//! quickly (a held arrow key), where fades would only add lag.

use crate::action::Action;
use iced::widget::image::Handle;
use photo_cruiser_core::crop::{CropTool, MouseKind};
use photo_cruiser_core::geom::PixelRect;
use photo_cruiser_core::loader::{Job, Kind, Loader};
use photo_cruiser_core::{Folder, Point, Size};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Longest side of the displayed (GPU) image. Real GPUs take 8192+; 4096 keeps uploads
/// small. Crop coordinates are unaffected (always source pixels).
pub const DEFAULT_MAX_SIDE: u32 = 4096;

/// Default cross-fade between shown images. Short enough not to feel like lag.
pub const DEFAULT_TRANSITION: Duration = Duration::from_millis(120);

/// Selections closer together than this count as flipping through (no fades).
const RAPID: Duration = Duration::from_millis(200);

/// A decoded image ready for display.
#[derive(Clone, Debug)]
pub struct Picture {
    pub handle: Handle,
    /// Full-resolution size (after orientation): the crop coordinate space.
    pub source: (u32, u32),
}

impl Picture {
    pub fn source_size(&self) -> Size {
        self.source.into()
    }
}

pub type Loaded = Result<Picture, String>;

/// Run a loader job on `path` (blocking). Errors become strings for the UI; a preview
/// job for a photo without an embedded preview is an `Err` too (never shown).
pub fn decode_job(path: &Path, kind: Kind, max_side: u32) -> Loaded {
    let msg = |e: photo_cruiser_core::Error| e.to_string().trim_end().to_string();
    let d = match kind {
        Kind::Full => photo_cruiser_core::decode::decode_max_side(path, Some(max_side)).map_err(msg)?,
        Kind::Preview => {
            photo_cruiser_core::embedded_preview(path).map_err(msg)?.ok_or("no embedded preview")?
        }
    };
    let (w, h) = d.image.dimensions();
    Ok(Picture { source: d.source_size(), handle: Handle::from_rgba(w, h, d.image.into_raw()) })
}

/// The image in the image area. It may lag behind the current index (the previous photo
/// stays until the new one, or its preview, is decoded).
#[derive(Clone, Debug)]
pub struct Shown {
    pub index: usize,
    pub handle: Handle,
    /// Full decode, not an embedded preview.
    pub sharp: bool,
}

/// A cross-fade from `from` to [`State::shown`].
#[derive(Clone, Debug)]
pub struct Fade {
    pub from: Handle,
    pub start: Instant,
}

/// What the image area shows, relative to the current index (in the state dump).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Display {
    /// Nothing (no image yet, an error or a message).
    None,
    /// The current photo, fully decoded.
    Sharp,
    /// The current photo's embedded preview.
    Preview,
    /// Still the previous photo; the current one is decoding.
    Previous,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Browse,
    Crop,
}

/// What the runtime must do after an action, beyond scheduling decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
}

#[derive(Debug)]
pub struct State {
    pub folder: Option<Folder>,
    /// The folder could not be read.
    pub folder_error: Option<String>,
    pub index: usize,
    pub loader: Loader<Loaded>,
    pub crop: CropTool,
    pub mode: Mode,
    pub max_side: u32,
    /// Rejected actions and folder errors, oldest first.
    pub errors: Vec<String>,
    pub shown: Option<Shown>,
    pub fade: Option<Fade>,
    /// Cross-fade duration; zero disables fades.
    pub transition: Duration,
    /// The last two selections (for detecting rapid flipping).
    selected_at: [Option<Instant>; 2],
    /// `PC_TRACE=1`: log switch latency and decode times to stderr.
    trace: bool,
    /// When the current index was selected, until its sharp image is shown.
    waiting_since: Option<Instant>,
    /// Start of each decode in flight.
    job_started: Vec<(Job, Instant)>,
}

impl State {
    pub fn new(max_side: u32, transition: Duration) -> Self {
        State {
            folder: None,
            folder_error: None,
            index: 0,
            loader: Loader::default(),
            crop: CropTool::default(),
            mode: Mode::Browse,
            max_side,
            errors: Vec::new(),
            shown: None,
            fade: None,
            transition,
            selected_at: [None; 2],
            trace: std::env::var_os("PC_TRACE").is_some_and(|v| v != "0"),
            waiting_since: None,
            job_started: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.folder.as_ref().map_or(0, Folder::len)
    }

    pub fn current_path(&self) -> Option<&Path> {
        Some(&self.folder.as_ref()?.entries.get(self.index)?.path)
    }

    pub fn current_name(&self) -> Option<&str> {
        Some(&self.folder.as_ref()?.entries.get(self.index)?.name)
    }

    /// The decode result for the current index, if finished.
    pub fn current(&self) -> Option<&Loaded> {
        self.current_path()?;
        self.loader.get(self.index)
    }

    pub fn picture(&self) -> Option<&Picture> {
        self.current()?.as_ref().ok()
    }

    /// Nothing decoding, the photos around the current one loaded, no fade running.
    pub fn is_idle(&self) -> bool {
        self.loader.is_idle(self.index, self.len()) && self.fade_progress().is_none()
    }

    pub fn display(&self) -> Display {
        match &self.shown {
            None => Display::None,
            Some(s) if s.index != self.index => Display::Previous,
            Some(s) if s.sharp => Display::Sharp,
            Some(_) => Display::Preview,
        }
    }

    /// The running fade: the image fading out and the progress (0..1, eased).
    pub fn fade_progress(&self) -> Option<(&Handle, f32)> {
        let f = self.fade.as_ref()?;
        let t = f.start.elapsed().as_secs_f32() / self.transition.as_secs_f32();
        (t < 1.0).then_some((&f.from, t * t * (3.0 - 2.0 * t)))
    }

    /// Show the best image available for the current index (sharp, else preview; else
    /// keep what is shown), fading from the old one unless the user is flipping quickly.
    fn refresh(&mut self) {
        let target = match (self.current(), self.loader.preview(self.index)) {
            (Some(Ok(p)), _) => (p.handle.clone(), true),
            (Some(Err(_)), _) => {
                (self.shown, self.fade) = (None, None);
                return;
            }
            (None, Some(Ok(p))) => (p.handle.clone(), false),
            (None, _) => return,
        };
        let (handle, sharp) = target;
        if self.shown.as_ref().is_some_and(|s| s.index == self.index && s.handle.id() == handle.id()) {
            return;
        }
        if self.trace
            && let Some(t) = self.waiting_since
        {
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let what = if sharp { "sharp" } else { "preview" };
            eprintln!("trace: switch {}: {what} shown after {ms:.0} ms", self.trace_name(self.index));
            if sharp {
                self.waiting_since = None;
            }
        }
        let rapid = match self.selected_at {
            [Some(last), Some(before)] => last - before < RAPID && last.elapsed() < RAPID,
            _ => false,
        };
        let from = self.shown.take().map(|s| s.handle).filter(|_| !rapid && !self.transition.is_zero());
        self.fade = from.map(|from| Fade { from, start: Instant::now() });
        self.shown = Some(Shown { index: self.index, handle, sharp });
    }

    fn error(&mut self, msg: String) {
        eprintln!("photo-cruiser: {msg}");
        self.errors.push(msg);
    }

    pub fn apply(&mut self, action: &Action) -> Effect {
        match action {
            Action::Open { path } => self.open(path),
            Action::Next => {
                if self.index + 1 < self.len() {
                    self.select(self.index + 1);
                }
            }
            Action::Prev => {
                if self.index > 0 {
                    self.select(self.index - 1);
                }
            }
            Action::Goto { index } => {
                if *index < self.len() {
                    self.select(*index);
                } else {
                    self.error(format!("goto {index}: out of range ({} images)", self.len()));
                }
            }
            Action::ToggleCrop => {
                self.mode = if self.mode == Mode::Crop { Mode::Browse } else { Mode::Crop };
                self.crop.clear();
            }
            Action::SetCrop(rect) => match self.picture().map(Picture::source_size) {
                Some(size) => {
                    self.mode = Mode::Crop;
                    self.crop.set(*rect, size);
                    if self.crop.selection.is_none() {
                        self.error(format!("set_crop {rect:?}: empty after clamping to the image"));
                    }
                }
                None => self.error("set_crop: no image is shown".into()),
            },
            Action::ClearCrop => self.crop.clear(),
            Action::ExitCrop => {
                self.mode = Mode::Browse;
                self.crop.clear();
            }
            Action::Quit => return Effect::Quit,
        }
        Effect::None
    }

    fn select(&mut self, index: usize) {
        let now = Instant::now();
        self.selected_at = [Some(now), self.selected_at[0]];
        self.index = index;
        self.crop.clear();
        self.waiting_since = self.trace.then_some(now);
        self.refresh();
    }

    fn trace_name(&self, index: usize) -> String {
        let name = self.folder.as_ref().and_then(|f| f.entries.get(index)).map_or("?", |e| e.name.as_str());
        format!("{} ({name})", index + 1)
    }

    /// Open a folder, or a file's folder positioned on that file. Resets everything
    /// except the error log.
    fn open(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let (dir, file) = match path.is_file() {
            true => (path.parent().map_or_else(|| path.clone(), Path::to_path_buf), Some(path.clone())),
            false => (path.clone(), None),
        };
        self.loader.reset();
        (self.shown, self.fade) = (None, None);
        self.crop.clear();
        self.mode = Mode::Browse;
        self.index = 0;
        self.folder_error = None;
        match Folder::open(&dir) {
            Ok(folder) => {
                if let Some(file) = file {
                    self.index = folder.entries.iter().position(|e| e.path == file).unwrap_or(0);
                }
                self.folder = Some(folder);
            }
            Err(e) => {
                let msg = format!("Cannot open folder: {e}");
                self.folder = Some(Folder { path: dir, entries: Vec::new() });
                self.folder_error = Some(msg.clone());
                self.error(msg);
            }
        }
    }

    /// Mouse event for the crop tool (ignored outside crop mode or without an image).
    pub fn crop_mouse(&mut self, kind: MouseKind, p: Point, tol: f32) {
        if self.mode != Mode::Crop {
            return;
        }
        if let Some(size) = self.picture().map(Picture::source_size) {
            self.crop.mouse(kind, p, tol, size);
        }
    }

    /// The next decode to start, with the file to decode, while a loader slot is free.
    /// Call until `None`.
    pub fn next_job(&mut self) -> Option<(Job, PathBuf)> {
        let job = self.loader.next_job(self.index, self.len())?;
        if self.trace {
            self.job_started.push((job, Instant::now()));
        }
        let path = self.folder.as_ref()?.entries.get(job.index)?.path.clone();
        Some((job, path))
    }

    pub fn finish(&mut self, job: Job, result: Loaded) {
        let fresh = self.loader.finish(job, result);
        if self.trace {
            let started = self.job_started.iter().position(|(j, _)| *j == job);
            let took = started.map_or(0.0, |i| self.job_started.remove(i).1.elapsed().as_secs_f64() * 1000.0);
            let what = match (fresh, job.kind, self.loader.preview(job.index)) {
                (false, ..) => "dropped stale",
                (true, Kind::Full, _) => "decoded",
                (true, Kind::Preview, Some(Ok(_))) => "preview read",
                (true, Kind::Preview, _) => "no preview in",
            };
            eprintln!("trace: {what} {} in {took:.0} ms", self.trace_name(job.index));
        }
        if fresh && job.index == self.index {
            self.refresh();
        }
    }

    /// Integer source-pixel selection.
    pub fn crop_pixels(&self) -> Option<PixelRect> {
        self.crop.selection?.to_pixels(self.picture()?.source)
    }

    pub fn title(&self) -> String {
        match self.folder.as_ref().and_then(|f| f.path.file_name()) {
            Some(n) => format!("Photo Cruiser — {}", n.to_string_lossy()),
            None => "Photo Cruiser".into(),
        }
    }

    /// `name — i / N[ — crop W×H @ X,Y]`; empty without a current file.
    pub fn status(&self) -> String {
        let Some(name) = self.current_name() else { return String::new() };
        let mut s = format!("{name} — {}", self.position());
        if let Some(px) = self.crop_pixels() {
            s += &format!(" — crop {px}");
        } else if self.mode == Mode::Crop {
            s += " — crop: drag on the image (Esc to leave)";
        }
        s
    }

    /// `i / N`, 1-based (`0 / 0` when empty).
    pub fn position(&self) -> String {
        let i = if self.len() == 0 { 0 } else { self.index + 1 };
        format!("{i} / {}", self.len())
    }

    /// Machine-readable snapshot (script mode `--dump-state`).
    pub fn dump(&self, window: Option<WindowSize>, shot: Option<&Path>) -> Dump {
        let mut errors = self.errors.clone();
        if let Some(Err(e)) = self.current() {
            errors.push(e.clone());
        }
        Dump {
            folder: self.folder.as_ref().map(|f| f.path.display().to_string()),
            count: self.len(),
            index: self.index,
            position: self.position(),
            file: self.current_path().map(|p| p.display().to_string()),
            mode: self.mode,
            crop: self.crop_pixels(),
            title: self.title(),
            status: self.status(),
            loading: self.current_path().is_some() && self.current().is_none(),
            display: self.display(),
            errors,
            window,
            shot: shot.map(|p| p.display().to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WindowSize {
    pub width: f32,
    pub height: f32,
}

/// The `--dump-state` JSON.
#[derive(Debug, Serialize)]
pub struct Dump {
    pub folder: Option<String>,
    pub count: usize,
    /// 0-based.
    pub index: usize,
    /// `i / N`, 1-based.
    pub position: String,
    pub file: Option<String>,
    pub mode: Mode,
    /// Integer source pixels.
    pub crop: Option<PixelRect>,
    pub title: String,
    pub status: String,
    /// The current image is still decoding.
    pub loading: bool,
    pub display: Display,
    pub errors: Vec<String>,
    /// Logical size of the window (from the screenshot or `window::size`).
    pub window: Option<WindowSize>,
    pub shot: Option<String>,
}
