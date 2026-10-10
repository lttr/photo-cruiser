//! The app model: folder, index, loader, what is on screen, crop, mode, grid or single
//! photo view. Plain data +
//! [`Action`] handling, no iced runtime, so it can be driven directly in tests (decode
//! synchronously with [`decode_job`] and feed results to [`State::finish`]).
//!
//! Switching never blanks the image area: until the new photo is decoded, its embedded
//! preview is shown, or else the previous photo stays. Each change of the shown image
//! cross-fades over [`State::transition`], except while the user is flipping through
//! quickly (a held arrow key), where fades would only add lag.
//!
//! Opening a folder shows the grid of all its photos; the grid's selection is the current
//! index, so the photo view opens on it and returns to it. Thumbnails load only while the
//! grid shows, visible ones first. The justified layout also needs every photo's size:
//! those are probed (headers only) in the background, the layout settling as they come.

use crate::action::Action;
use crate::grid;
use iced::widget::image::Handle;
use photo_cruiser_core::crop::{CropTool, MouseKind};
use photo_cruiser_core::geom::PixelRect;
use photo_cruiser_core::loader::{Job, Kind, Loader, SizeJob, Sizes, ThumbJob, Thumbs};
use photo_cruiser_core::{Folder, Point, Size};
use serde::Serialize;
use std::ops::Range;
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

/// Decode a grid thumbnail of `path` (blocking).
pub fn thumb_job(path: &Path, side: u32) -> Loaded {
    let d = photo_cruiser_core::thumbnail(path, side).map_err(|e| e.to_string().trim_end().to_string())?;
    let (w, h) = d.image.dimensions();
    Ok(Picture { source: d.source_size(), handle: Handle::from_rgba(w, h, d.image.into_raw()) })
}

/// Probe the displayed sizes of `paths` (blocking); `None` for unreadable ones.
pub fn size_job(paths: &[PathBuf]) -> Vec<Option<(u32, u32)>> {
    paths.iter().map(|p| photo_cruiser_core::probe(p).ok().map(|p| (p.width, p.height))).collect()
}

/// The image area's size before the overlay has reported it (the default window minus
/// the footer).
const DEFAULT_BODY: iced::Size = iced::Size::new(1100.0, 815.0);

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

/// What the image area shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    /// Thumbnails of every photo in the folder; the current one is selected.
    #[default]
    Grid,
    /// The current photo, large.
    Photo,
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
    /// Put the window into (`true`) or out of full screen.
    Fullscreen(bool),
}

#[derive(Debug)]
pub struct State {
    pub folder: Option<Folder>,
    /// The folder could not be read.
    pub folder_error: Option<String>,
    pub index: usize,
    pub loader: Loader<Loaded>,
    pub view: View,
    pub thumbs: Thumbs<Loaded>,
    /// Photo sizes, for the justified layout.
    pub sizes: Sizes,
    /// Size of the image / grid area, as last reported by the overlay.
    pub body: Option<iced::Size>,
    /// Vertical scroll position of the grid.
    pub grid_offset: f32,
    /// A scroll position the runtime should move the grid to (taken by the app).
    pub scroll: Option<f32>,
    pub crop: CropTool,
    pub mode: Mode,
    /// Full screen: no status line. The runtime mirrors it to the window.
    pub fullscreen: bool,
    /// The menu (Open folder, Crop, Full screen) is showing.
    pub menu: bool,
    /// The keyboard shortcuts panel is showing.
    pub help: bool,
    /// The About panel is showing.
    pub about: bool,
    /// File names are shown (status line, grid labels). Off by default; remembered between runs.
    pub names: bool,
    /// How the grid arranges thumbnails. Small at first start, then remembered.
    pub layout: grid::Layout,
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
            view: View::Grid,
            thumbs: Thumbs::default(),
            sizes: Sizes::default(),
            body: None,
            grid_offset: 0.0,
            scroll: None,
            crop: CropTool::default(),
            mode: Mode::Browse,
            fullscreen: false,
            menu: false,
            help: false,
            about: false,
            names: false,
            layout: grid::Layout::default(),
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

    /// Nothing decoding, the photos around the current one loaded, no fade running, in the
    /// grid every visible thumbnail loaded, and for the justified layout every size probed.
    pub fn is_idle(&self) -> bool {
        let side = self.layout.thumb_side();
        let thumbs = self.view != View::Grid || self.thumbs.is_idle(self.visible(), self.len(), side);
        let sizes = self.layout != grid::Layout::Justified || self.sizes.is_done();
        self.loader.is_idle(self.index, self.len()) && self.fade_progress().is_none() && thumbs && sizes
    }

    pub fn body_size(&self) -> iced::Size {
        self.body.unwrap_or(DEFAULT_BODY)
    }

    /// The grid's rows at the current size. Photos of unknown size count as 3:2.
    pub fn grid(&self) -> grid::Grid {
        let aspect = |i| self.sizes.get(i).map_or(grid::DEFAULT_ASPECT, |(w, h)| w as f32 / h.max(1) as f32);
        grid::Grid::new(self.layout, self.body_size().width, self.names, self.len(), aspect)
    }

    /// Indices of the photos in the grid rows on screen.
    pub fn visible(&self) -> Range<usize> {
        self.grid().visible(self.grid_offset, self.body_size().height)
    }

    /// In the grid, scroll so the current photo is fully visible.
    fn reveal(&mut self) {
        if self.view == View::Grid
            && let Some(y) = self.grid().reveal(self.index, self.grid_offset, self.body_size().height)
        {
            self.grid_offset = y;
            self.scroll = Some(y);
        }
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
        self.menu = matches!(action, Action::ToggleMenu) && !self.menu;
        self.help = matches!(action, Action::ToggleHelp) && !self.help;
        self.about = matches!(action, Action::ToggleAbout) && !self.about;
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
            Action::Up | Action::Down if self.view == View::Photo => {}
            Action::Up | Action::Down => {
                if let Some(i) = self.grid().vertical(self.index, *action == Action::Down) {
                    self.select(i);
                }
            }
            Action::ShowPhoto { index } => match index {
                Some(i) if *i >= self.len() => {
                    self.error(format!("show_photo {i}: out of range ({} images)", self.len()));
                }
                _ => {
                    self.view = View::Photo;
                    if let Some(i) = index {
                        self.select(*i);
                    }
                }
            },
            Action::ShowGrid => {
                self.view = View::Grid;
                self.mode = Mode::Browse;
                self.crop.clear();
                // The grid's scrollable is new (at the top): bring the current photo into view.
                self.grid_offset = 0.0;
                self.reveal();
            }
            Action::First if self.len() > 0 => self.select(0),
            Action::Last if self.len() > 0 => self.select(self.len() - 1),
            Action::First | Action::Last => {}
            Action::Goto { index } => {
                if *index < self.len() {
                    self.select(*index);
                } else {
                    self.error(format!("goto {index}: out of range ({} images)", self.len()));
                }
            }
            Action::ToggleCrop => {
                self.view = View::Photo;
                self.mode = if self.mode == Mode::Crop { Mode::Browse } else { Mode::Crop };
                self.crop.clear();
            }
            Action::SetCrop(rect) => match self.picture().map(Picture::source_size) {
                Some(size) => {
                    self.view = View::Photo;
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
            Action::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                return Effect::Fullscreen(self.fullscreen);
            }
            Action::ExitFullscreen if self.fullscreen => {
                self.fullscreen = false;
                return Effect::Fullscreen(false);
            }
            Action::ExitFullscreen => {}
            Action::ToggleNames => {
                self.names = !self.names;
                // Rows change height: keep the current photo in view.
                self.reveal();
            }
            Action::NextLayout | Action::SetLayout { .. } => {
                self.layout = match action {
                    Action::SetLayout { layout } => *layout,
                    _ => self.layout.next(),
                };
                // Rows and columns change: keep the current photo in view.
                self.reveal();
            }
            Action::ToggleMenu | Action::CloseMenu | Action::ToggleHelp | Action::ToggleAbout => {}
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
        self.reveal();
    }

    fn trace_name(&self, index: usize) -> String {
        let name = self.folder.as_ref().and_then(|f| f.entries.get(index)).map_or("?", |e| e.name.as_str());
        format!("{} ({name})", index + 1)
    }

    /// Open a folder (in the grid), or a file's folder positioned on that file (in the photo
    /// view). Resets everything except the error log and full screen.
    fn open(&mut self, path: &Path) {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let (dir, file) = match path.is_file() {
            true => (path.parent().map_or_else(|| path.clone(), Path::to_path_buf), Some(path.clone())),
            false => (path.clone(), None),
        };
        self.loader.reset();
        self.thumbs.reset();
        self.sizes.reset(0);
        self.view = if file.is_some() { View::Photo } else { View::Grid };
        self.grid_offset = 0.0;
        self.scroll = Some(0.0);
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
                self.sizes.reset(folder.len());
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

    /// The next grid thumbnail to decode (`job.side` is the side to decode at), while the
    /// grid shows and a slot is free. Call until `None`.
    pub fn next_thumb_job(&mut self) -> Option<(ThumbJob, PathBuf)> {
        if self.view != View::Grid {
            return None;
        }
        let job = self.thumbs.next_job(self.visible(), self.len(), self.layout.thumb_side())?;
        let path = self.folder.as_ref()?.entries.get(job.index)?.path.clone();
        Some((job, path))
    }

    pub fn finish_thumb(&mut self, job: ThumbJob, result: Loaded) {
        // The thumbnail knows its photo's size: no need to wait for the probe.
        let source = result.as_ref().ok().map(|p| p.source);
        if self.thumbs.finish(job, result)
            && let Some(size) = source
            && self.sizes.get(job.index).is_none()
        {
            self.sizes.set(job.index, size);
        }
    }

    /// The next chunk of photo sizes to probe, while the justified layout is on and a slot
    /// is free. Call until `None`.
    pub fn next_size_job(&mut self) -> Option<(SizeJob, Vec<PathBuf>)> {
        if self.layout != grid::Layout::Justified {
            return None;
        }
        let job = self.sizes.next_job()?;
        let entries = &self.folder.as_ref()?.entries[job.items.clone()];
        let paths = entries.iter().map(|e| e.path.clone()).collect();
        Some((job, paths))
    }

    /// The justified rows reflow as sizes come in: a current photo on screen stays there.
    pub fn finish_sizes(&mut self, job: &SizeJob, results: &[Option<(u32, u32)>]) {
        let shown = self.visible().contains(&self.index);
        if self.sizes.finish(job, results) && shown {
            self.reveal();
        }
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
        match &self.folder {
            Some(f) => format!("PhotoCruiser: {}", tilde(&f.path)),
            None => "PhotoCruiser".into(),
        }
    }

    /// `[name · ]i / N[ · crop W×H @ X,Y]` (the name only while names are shown); empty
    /// without a current file.
    pub fn status(&self) -> String {
        let Some(name) = self.current_name() else { return String::new() };
        let mut s = match self.names {
            true => format!("{name} · {}", self.position()),
            false => self.position(),
        };
        if let Some(px) = self.crop_pixels() {
            s += &format!(" · crop {px}");
        } else if self.mode == Mode::Crop {
            s += " · crop: drag on the image (Esc to leave)";
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
            view: self.view,
            mode: self.mode,
            fullscreen: self.fullscreen,
            menu: self.menu,
            help: self.help,
            about: self.about,
            names: self.names,
            layout: self.layout,
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
    pub view: View,
    pub mode: Mode,
    pub fullscreen: bool,
    pub menu: bool,
    pub help: bool,
    pub about: bool,
    pub names: bool,
    pub layout: grid::Layout,
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

/// `path` for display, with the home directory shortened to `~`.
pub fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
