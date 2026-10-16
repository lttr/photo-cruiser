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
//!
//! Edits: each photo shows `render(original, stack)`. A crop drawn in crop mode is a
//! draft until applied (Enter, Apply, or leaving the photo). Crop mode shows the whole
//! original, turned like the photo: the selection is on the turned original, the stack's
//! crop on the original itself. Every change of a stack is
//! written to the photo's sidecar, and leaving a dirty photo saves it; both run on one
//! background worker, in order ([`State::next_io`]). No prompts: quitting waits for the
//! saves.

use crate::action::Action;
use crate::grid;
use crate::menu;
use crate::palette::{self, Palette};
use iced::widget::image::Handle;
use image::RgbaImage;
use photo_cruiser_core::adjust::{self, Tool};
use photo_cruiser_core::crop::{Aspect, CropTool, MouseKind};
use photo_cruiser_core::geom::PixelRect;
use photo_cruiser_core::loader::{Job, Kind, Loader, SizeJob, Sizes, ThumbJob, Thumbs};
use photo_cruiser_core::store::{self, PhotoEdits, SaveReport};
use photo_cruiser_core::{EditStack, Error, Folder, History, Op, Point, Rect, Size};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    /// Full-resolution size of what is shown (after orientation and crop), in source pixels.
    pub source: (u32, u32),
    /// The image before its adjustments, if it has any (else the handle's pixels are).
    pub base: Option<Arc<RgbaImage>>,
}

impl Picture {
    pub fn source_size(&self) -> Size {
        self.source.into()
    }

    /// The pixels with the geometry applied but no adjustments.
    pub fn unadjusted(&self) -> Option<RgbaImage> {
        if let Some(base) = &self.base {
            return Some((**base).clone());
        }
        match &self.handle {
            Handle::Rgba { width, height, pixels, .. } => {
                RgbaImage::from_raw(*width, *height, pixels.to_vec())
            }
            _ => None,
        }
    }
}

pub type Loaded = Result<Picture, String>;

/// Decode a photo's original file (its backup, once saved) for showing it without edits,
/// at most `side` px.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalJob {
    pub index: usize,
    pub path: PathBuf,
    pub side: u32,
}

/// Run an [`OriginalJob`] (blocking).
pub fn original_job(job: &OriginalJob) -> Loaded {
    let d = photo_cruiser_core::decode::decode_max_side(&job.path, Some(job.side))
        .map_err(|e| e.to_string().trim_end().to_string())?;
    let (w, h) = d.image.dimensions();
    Ok(Picture { source: d.source_size(), handle: Handle::from_rgba(w, h, d.image.into_raw()), base: None })
}

/// The current photo's original, without any edits (what holding `O` shows).
#[derive(Clone, Debug)]
struct Original {
    job: OriginalJob,
    /// `None` while it decodes.
    image: Option<Loaded>,
}

/// A loader job: the photo (or its original, for a full decode) and the edits to render.
#[derive(Clone, Debug)]
pub struct LoadJob {
    pub job: Job,
    pub path: PathBuf,
    pub stack: EditStack,
}

/// Run a loader job (blocking): decode, then render the edits. Errors become strings for
/// the UI; a preview job for a photo without an embedded preview, or with edits (the
/// preview would show it unedited), is an `Err` too (never shown).
pub fn decode_job(load: &LoadJob, max_side: u32) -> Loaded {
    let msg = |e: Error| e.to_string().trim_end().to_string();
    let d = match load.job.kind {
        Kind::Full => photo_cruiser_core::decode::decode_max_side(&load.path, Some(max_side)).map_err(msg)?,
        Kind::Preview if !load.stack.is_empty() => return Err("edited: no preview".into()),
        Kind::Preview => {
            photo_cruiser_core::embedded_preview(&load.path).map_err(msg)?.ok_or("no embedded preview")?
        }
    };
    let full = d.source_size();
    let (w, h) = match load.stack.crop().and_then(|r| r.to_pixels(full)) {
        Some(px) => (px.w, px.h),
        None => full,
    };
    let source = if load.stack.rotation() % 2 == 1 { (h, w) } else { (w, h) };
    let geometry = load.stack.geometry();
    let img = match geometry.is_empty() {
        true => d.image,
        false => photo_cruiser_core::render::render(&d, &geometry, Default::default()).map_err(msg)?,
    };
    // Same as rendering the whole stack, keeping the image before the adjustments.
    let (img, base) = match load.stack.adjustments().next() {
        None => (img, None),
        Some(_) => {
            let mut adjusted = img.clone();
            adjust::apply_all(&mut adjusted, &load.stack);
            (adjusted, Some(Arc::new(img)))
        }
    };
    let (w, h) = img.dimensions();
    Ok(Picture { source, handle: Handle::from_rgba(w, h, img.into_raw()), base })
}

/// The edits of one photo this session: the sidecar's content and the undo history.
#[derive(Clone, Debug, Default)]
pub struct PhotoState {
    pub edits: PhotoEdits,
    pub history: History,
}

/// Background file work, one at a time and in order. The stack is read when the job
/// starts, so a photo queued twice is written once, with its latest edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Queued {
    /// Write the photo's stack to its sidecar.
    Store(PathBuf),
    /// Save the photo (render its edits into the file).
    Save(PathBuf),
}

/// A started [`Queued`] job.
#[derive(Clone, Debug)]
pub enum Io {
    Store(PathBuf, EditStack),
    Save(PathBuf, EditStack),
}

#[derive(Clone, Debug)]
pub enum IoDone {
    Stored(PathBuf, Result<(), String>),
    Saved(PathBuf, Result<SaveReport, String>),
}

/// Run an [`Io`] job (blocking).
pub fn io_job(io: Io) -> IoDone {
    let msg = |e: Error| e.to_string();
    match io {
        Io::Store(path, stack) => {
            let r = store::store_stack(&path, &stack).map_err(msg);
            IoDone::Stored(path, r)
        }
        Io::Save(path, stack) => {
            let r = store::save(&path, &stack).map_err(msg);
            IoDone::Saved(path, r)
        }
    }
}

/// Decode a grid thumbnail of `path` (blocking).
pub fn thumb_job(path: &Path, side: u32) -> Loaded {
    let d = photo_cruiser_core::thumbnail(path, side).map_err(|e| e.to_string().trim_end().to_string())?;
    let (w, h) = d.image.dimensions();
    Ok(Picture { source: d.source_size(), handle: Handle::from_rgba(w, h, d.image.into_raw()), base: None })
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

/// The open one-slider tool ([`adjust`]). While it is open the image area shows a live
/// [`Preview`] at screen size: dragging the slider re-renders only that, never the photo.
#[derive(Clone, Debug)]
pub struct ToolPanel {
    pub tool: Tool,
    /// The photo it is open on.
    pub index: usize,
    /// The value it opened at; `None` until the photo is loaded (then it is applied).
    pub opening: Option<f32>,
    /// The stack before the tool opened (`Esc` goes back to it).
    pub before: EditStack,
    /// The slider's value while it is dragged (not in the stack yet).
    pub drag: Option<f32>,
    /// The photo at screen size, with its geometry but no adjustments: the preview's
    /// source and what the opening value was measured on (so on the cropped photo).
    base: Option<Arc<RgbaImage>>,
    /// The geometry `base` was made with; a change closes the tool.
    geometry: EditStack,
    /// When the preview was last rendered while dragging (for [`PREVIEW_EVERY`]).
    rendered_at: Option<Instant>,
}

impl ToolPanel {
    /// The value the slider shows.
    pub fn value(&self, stack: &EditStack) -> f32 {
        self.drag.or(stack.adjust(self.tool)).unwrap_or(self.tool.neutral())
    }
}

/// The adjusted photo at screen size, shown instead of the loaded image while a tool is
/// open, and after that until the photo is reloaded with the new edits.
#[derive(Clone, Debug)]
pub struct Preview {
    pub index: usize,
    /// What it shows (with the dragged value).
    pub stack: EditStack,
    pub handle: Handle,
}

/// The preview's base is at most the image area's size times this (sharp on HiDPI).
const PREVIEW_SCALE: f32 = 2.0;

/// While the slider is dragged, the preview is rendered at most this often; the value
/// the drag rests on (or is released at) is always rendered ([`State::tick`]).
pub const PREVIEW_EVERY: Duration = Duration::from_millis(100);

/// An image drawn under the image area's top image until that one is on the GPU.
///
/// The renderer uploads big images (over 2 MB) on a worker thread and draws nothing for
/// them until the upload is done; images not drawn in a frame are dropped from the GPU.
/// So every new image (a re-rendered preview, the reload after closing a tool) would show
/// the background for a frame or more: a blink. The image it replaces stays underneath
/// until the runtime reports the new one allocated ([`State::uploaded`]). Only for the
/// same photo with the same shape: it must cover exactly what the new image covers.
#[derive(Clone, Debug)]
struct Underlay {
    handle: Handle,
    index: usize,
    geometry: EditStack,
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
    /// The focused menu item (keyboard or mouse), an index into [`menu::ITEMS`].
    pub menu_focus: Option<usize>,
    /// The keyboard shortcuts panel is showing.
    pub help: bool,
    /// The About panel is showing.
    pub about: bool,
    /// The custom aspect ratio field is showing, with this text.
    pub aspect_input: Option<String>,
    /// The command palette (`Ctrl+K`) is showing.
    pub palette: Option<Palette>,
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
    /// Edits of the photos shown this session, by path.
    pub edits: HashMap<PathBuf, PhotoState>,
    /// Sidecar writes and saves waiting for the worker, and the one it runs.
    pub queue: VecDeque<Queued>,
    pub running: Option<Queued>,
    /// Photos whose last save failed, with the error (a dot in the grid).
    pub save_failed: HashMap<PathBuf, String>,
    /// The stack each loaded (or loading) full image was rendered with.
    displayed: HashMap<usize, EditStack>,
    /// Quit was asked for: exit once the queue is done.
    pub quitting: bool,
    /// A one-line notice for the status line (saving edits left from last time).
    pub notice: Option<String>,
    /// The open one-slider tool.
    pub tool: Option<ToolPanel>,
    pub preview: Option<Preview>,
    /// The original is held to be seen (`O`, `\`, the footer's Original button).
    pub original: bool,
    /// The current photo's original file, decoded (or decoding) for that. Fetched ahead
    /// for an edited photo, so holding the key shows it at once.
    original_image: Option<Original>,
    /// Fixed time for tests (the preview's pacing); `None`: the real clock.
    pub clock: Option<Instant>,
    /// The image area's top image as last seen: photo index, display geometry, handle.
    top: Option<Underlay>,
    /// What shows under the top image until it is uploaded.
    under: Option<Underlay>,
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
            menu_focus: None,
            help: false,
            about: false,
            aspect_input: None,
            palette: None,
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
            edits: HashMap::new(),
            queue: VecDeque::new(),
            running: None,
            save_failed: HashMap::new(),
            displayed: HashMap::new(),
            quitting: false,
            notice: None,
            tool: None,
            preview: None,
            original: false,
            original_image: None,
            clock: None,
            top: None,
            under: None,
        }
    }

    pub fn now(&self) -> Instant {
        self.clock.unwrap_or_else(Instant::now)
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
        let original = self.original_image.as_ref().is_some_and(|o| o.image.is_none());
        self.loader.is_idle(self.index, self.len())
            && !original
            && self.fade_progress().is_none()
            && thumbs
            && sizes
            && self.io_idle()
    }

    /// No sidecar write or save queued or running.
    pub fn io_idle(&self) -> bool {
        self.queue.is_empty() && self.running.is_none()
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
        // Under a tool's preview (open, or waiting for this reload) the image is the same
        // state: fading from the photo as it was before would flash that state.
        let covered = self.preview.as_ref().is_some_and(|p| p.index == self.index);
        let from =
            self.shown.take().map(|s| s.handle).filter(|_| !rapid && !covered && !self.transition.is_zero());
        self.fade = from.map(|from| Fade { from, start: Instant::now() });
        self.shown = Some(Shown { index: self.index, handle, sharp });
    }

    fn error(&mut self, msg: String) {
        eprintln!("photo-cruiser: {msg}");
        self.errors.push(msg);
    }

    pub fn apply(&mut self, action: &Action) -> Effect {
        let was_open = self.menu;
        self.menu = match action {
            Action::ToggleMenu => !self.menu,
            Action::MenuFocus { .. } => true,
            Action::MenuNext | Action::MenuPrev | Action::MenuActivate => self.menu,
            _ => false,
        };
        if !self.menu || !was_open {
            self.menu_focus = None;
        }
        self.help = matches!(action, Action::ToggleHelp) && !self.help;
        self.about = matches!(action, Action::ToggleAbout) && !self.about;
        self.aspect_input = match (action, &self.aspect_input) {
            (Action::ToggleCustomAspect, None) => {
                Some(if self.crop.aspect.is_preset() { String::new() } else { self.crop.aspect.to_string() })
            }
            _ => None,
        };
        self.palette = match action {
            Action::TogglePalette if self.palette.is_none() => Some(Palette::default()),
            Action::PaletteQuery { .. }
            | Action::PaletteFocus { .. }
            | Action::PaletteNext
            | Action::PalettePrev
            | Action::PaletteActivate => self.palette.take(),
            _ => None,
        };
        // Only while it is held: anything else ends it (a key released elsewhere).
        if !matches!(action, Action::ShowOriginal { .. }) {
            self.original = false;
        }
        let effect = self.act(action);
        self.sync_display();
        self.sync_tool();
        self.sync_under();
        effect
    }

    /// Time passed (a frame while [`State::preview_pending`]): render the preview of a
    /// drag that has rested for [`PREVIEW_EVERY`].
    pub fn tick(&mut self) {
        self.sync_tool();
        self.sync_under();
    }

    /// A dragged value isn't rendered yet: the runtime should [`State::tick`].
    pub fn preview_pending(&self) -> bool {
        let Some(p @ ToolPanel { drag: Some(v), base: Some(_), .. }) = &self.tool else { return false };
        let mut stack = self.stack();
        stack.set_adjust(p.tool, Some(*v));
        self.preview.as_ref().is_none_or(|pr| pr.index != self.index || pr.stack != stack)
    }

    /// The image area's main image: the tool's preview, else the shown photo (its fade's
    /// target).
    pub fn main_handle(&self) -> Option<&Handle> {
        match &self.preview {
            Some(p) if p.index == self.index => Some(&p.handle),
            _ => self.shown.as_ref().map(|s| &s.handle),
        }
    }

    /// The image under the main one while that is being uploaded (see [`Underlay`]).
    pub fn underlay(&self) -> Option<&Handle> {
        let u = self.under.as_ref()?;
        let same = self.view == View::Photo
            && u.index == self.index
            && u.geometry == self.display_stack(self.index).geometry()
            && self.shown.as_ref().is_some_and(|s| s.index == self.index);
        same.then_some(&u.handle)
    }

    /// The main image changed: the one it replaces goes underneath, unless an earlier one
    /// (the last known to be on the GPU) is still there.
    fn sync_under(&mut self) {
        let id = self.main_handle().map(Handle::id);
        if self.top.as_ref().map(|t| t.handle.id()) == id {
            return;
        }
        let old = self.top.take();
        if self.underlay().is_none() {
            self.under = old;
        }
        self.top = self.main_handle().cloned().map(|handle| Underlay {
            handle,
            index: self.index,
            geometry: self.display_stack(self.index).geometry(),
        });
        if self.underlay().is_none() {
            self.under = None;
        }
    }

    /// The renderer has image `id` on the GPU: if it is the main image, nothing needs to
    /// show under it any more (returns whether it is).
    pub fn uploaded(&mut self, id: iced_runtime::core::image::Id) -> bool {
        let main = self.main_handle().is_some_and(|h| h.id() == id);
        if main {
            self.under = None;
        }
        main
    }

    fn act(&mut self, action: &Action) -> Effect {
        match action {
            Action::Open { path } => {
                self.leave();
                self.open(path);
            }
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
                self.leave();
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
            Action::ToggleCrop if self.mode == Mode::Crop => self.apply_crop(),
            Action::ToggleCrop => {
                self.view = View::Photo;
                self.mode = Mode::Crop;
                self.crop.clear();
                // The whole original shows, with the applied crop selected.
                self.crop.selection = self.crop_selection();
            }
            Action::ApplyCrop => self.apply_crop(),
            Action::Undo | Action::Redo => {
                (self.mode, self.crop.selection) = (Mode::Browse, None);
                let Some(path) = self.current_path().map(Path::to_path_buf) else { return Effect::None };
                let p = self.edits.entry(path.clone()).or_default();
                let changed = match action {
                    Action::Undo => p.history.undo(&mut p.edits.stack),
                    _ => p.history.redo(&mut p.edits.stack),
                };
                if changed {
                    self.stored(path);
                }
            }
            Action::OpenTool { tool } => self.open_tool(*tool),
            Action::DragAdjust { value } => match &mut self.tool {
                Some(p) => p.drag = Some(p.tool.clamp(*value)),
                None => self.error("drag_adjust: no tool is open".into()),
            },
            Action::SetAdjust { value } => self.set_adjust(*value),
            Action::AutoAdjust => {
                if let Some(v) = self.tool.as_ref().and_then(|p| p.opening) {
                    self.set_adjust(v);
                }
            }
            Action::NeutralAdjust => match &self.tool {
                Some(p) => self.set_adjust(p.tool.neutral()),
                None => self.error("neutral_adjust: no tool is open".into()),
            },
            Action::CloseTool => self.close_tool(),
            Action::ShowOriginal { on } => {
                self.original =
                    *on && self.view == View::Photo && self.mode == Mode::Browse && self.len() > 0;
            }
            Action::CancelTool => {
                if let Some(p) = self.tool.take() {
                    self.change_stack(p.before);
                }
            }
            Action::RotateRight => self.rotate(1),
            Action::RotateLeft => self.rotate(-1),
            Action::ResetEdits => {
                (self.mode, self.crop.selection) = (Mode::Browse, None);
                self.change_stack(EditStack::default());
            }
            Action::Revert => {
                (self.mode, self.crop.selection) = (Mode::Browse, None);
                self.change_stack(EditStack::default());
                if let Some(path) = self.current_path().map(Path::to_path_buf) {
                    self.save_now(path);
                }
            }
            Action::Save => {
                self.apply_crop();
                if let Some(path) = self.current_path().map(Path::to_path_buf) {
                    self.queue_save(path);
                }
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
            // Drops the draft; the applied crop stays.
            Action::ExitCrop => {
                self.mode = Mode::Browse;
                self.crop.clear();
            }
            Action::SetAspect { aspect } => self.crop.set_aspect(*aspect, self.image_size()),
            Action::NextAspect => self.crop.set_aspect(self.crop.aspect.next(), self.image_size()),
            Action::SwapAspect => self.crop.swap(self.image_size()),
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
            Action::MenuFocus { index } => self.menu_focus = Some((*index).min(menu::ITEMS.len() - 1)),
            Action::MenuNext | Action::MenuPrev if self.menu => {
                let n = menu::ITEMS.len();
                self.menu_focus = Some(match (self.menu_focus, *action == Action::MenuNext) {
                    (None, true) => 0,
                    (None, false) => n - 1,
                    (Some(i), true) => (i + 1) % n,
                    (Some(i), false) => (i + n - 1) % n,
                });
            }
            Action::PaletteQuery { query } => {
                if let Some(p) = &mut self.palette {
                    *p = Palette { query: query.clone(), ..Palette::default() };
                }
            }
            Action::PaletteFocus { index } => {
                let n = palette::filtered(self).len();
                if let Some(p) = &mut self.palette {
                    p.focus(*index, n);
                }
            }
            Action::PaletteNext | Action::PalettePrev => {
                let n = palette::filtered(self).len();
                if let Some(p) = self.palette.as_mut().filter(|_| n > 0) {
                    let i = if *action == Action::PaletteNext { p.focus + 1 } else { p.focus + n - 1 };
                    p.focus(i % n, n);
                }
            }
            // `MenuActivate` and `PaletteActivate` are picked up by the app, which knows the
            // items' messages.
            Action::ToggleMenu
            | Action::TogglePalette
            | Action::ClosePalette
            | Action::PaletteActivate
            | Action::CloseMenu
            | Action::MenuNext
            | Action::MenuPrev
            | Action::MenuActivate
            | Action::ToggleHelp
            | Action::ToggleAbout
            | Action::ToggleCustomAspect => {}
            Action::Quit => {
                self.leave();
                if !self.io_idle() {
                    self.quitting = true;
                    return Effect::None;
                }
                return Effect::Quit;
            }
        }
        Effect::None
    }

    /// The current photo's stack (empty if it has no edits).
    pub fn stack(&self) -> EditStack {
        self.current_path().map(|p| self.stack_of(p)).unwrap_or_default()
    }

    fn stack_of(&self, path: &Path) -> EditStack {
        self.edits.get(path).map(|p| p.edits.stack.clone()).unwrap_or_default()
    }

    /// The backup of the current photo's original, if it was saved.
    pub fn backup(&self) -> Option<&Path> {
        self.current_path().and_then(|p| self.edits.get(p)?.edits.backup.as_deref())
    }

    /// The current photo's edits differ from what its file was saved with.
    pub fn dirty(&self) -> bool {
        self.current_path().and_then(|p| self.edits.get(p)).is_some_and(|p| p.edits.dirty())
    }

    /// Read a photo's sidecar the first time it is needed. Stale edits are a warning.
    fn load_edits(&mut self, path: &Path) {
        if self.edits.contains_key(path) {
            return;
        }
        let edits = match store::load(path) {
            Ok(e) => e.unwrap_or_default(),
            Err(e) => {
                self.error(e.to_string());
                PhotoEdits::default()
            }
        };
        self.edits.insert(path.to_path_buf(), PhotoState { edits, history: History::default() });
    }

    /// Make `stack` the current photo's (undoable) and write it to the sidecar.
    fn change_stack(&mut self, stack: EditStack) {
        let Some(path) = self.current_path().map(Path::to_path_buf) else { return };
        self.load_edits(&path);
        let p = self.edits.get_mut(&path).expect("loaded");
        if p.history.change(&mut p.edits.stack, stack) {
            self.stored(path);
        }
    }

    /// A photo's stack changed: write the sidecar. In the grid, which shows the photos'
    /// files, save it too.
    fn stored(&mut self, path: PathBuf) {
        self.enqueue(Queued::Store(path.clone()));
        if self.view == View::Grid {
            self.queue_save(path);
        }
    }

    /// Turn the current photo by `quarters` × 90° clockwise (negative: counter-clockwise).
    /// A draft crop turns with it.
    fn rotate(&mut self, quarters: i32) {
        let mut stack = self.stack();
        let old = stack.rotation();
        stack.rotate(quarters);
        if let (Mode::Crop, Some(sel)) = (self.mode, self.crop.selection) {
            let turn = quarters.rem_euclid(4) as u8;
            self.crop.selection = self.original_size().map(|size| sel.rotate(turn, size.rotate(old)));
        }
        self.change_stack(stack);
    }

    /// The current photo's original size (oriented, before edits), read from its file.
    fn original_size(&self) -> Option<Size> {
        let path = self.current_path()?;
        let backup = self.edits.get(path).and_then(|p| p.edits.backup.as_deref());
        let p = photo_cruiser_core::probe(backup.unwrap_or(path)).ok()?;
        Some((p.width, p.height).into())
    }

    /// The current photo's crop as a selection on its turned original.
    fn crop_selection(&self) -> Option<Rect> {
        let stack = self.stack();
        match (stack.crop()?, stack.rotation()) {
            (crop, 0) => Some(crop),
            (crop, q) => Some(crop.rotate(q, self.original_size()?)),
        }
    }

    /// Apply the crop mode's draft selection (if any) and leave crop mode.
    fn apply_crop(&mut self) {
        if self.mode == Mode::Crop {
            self.apply_draft();
            self.mode = Mode::Browse;
            self.crop.clear();
        }
    }

    /// Make the draft selection the current photo's crop.
    fn apply_draft(&mut self) {
        if let (Mode::Crop, Some(sel)) = (self.mode, self.crop.selection) {
            let mut stack = self.stack();
            let crop = match stack.rotation() {
                0 => Some(sel),
                q => self.original_size().map(|size| sel.rotate(4 - q, size.rotate(q))),
            };
            if let Some(crop) = crop {
                stack.set_crop(Some(crop));
                self.change_stack(stack);
            }
        }
    }

    /// Open `tool` on the current photo (see [`Action::OpenTool`]); [`State::sync_tool`]
    /// finishes opening it once the photo shows with its current edits.
    fn open_tool(&mut self, tool: Tool) {
        if self.current_path().is_none() {
            return self.error("open_tool: no photo".into());
        }
        self.apply_crop();
        self.view = View::Photo;
        if self.tool.as_ref().is_some_and(|p| p.tool == tool && p.index == self.index) {
            return;
        }
        self.close_tool();
        let before = self.stack();
        let geometry = before.geometry();
        self.tool = Some(ToolPanel {
            tool,
            index: self.index,
            opening: None,
            before,
            drag: None,
            base: None,
            geometry,
            rendered_at: None,
        });
    }

    /// Set the open tool's value as one undoable step.
    fn set_adjust(&mut self, value: f32) {
        let mut stack = self.stack();
        let Some(p) = &mut self.tool else { return self.error("set_adjust: no tool is open".into()) };
        p.drag = None;
        stack.set_adjust(p.tool, Some(value));
        self.change_stack(stack);
    }

    /// Close the tool, keeping a value still being dragged.
    fn close_tool(&mut self) {
        if let Some(v) = self.tool.as_ref().and_then(|p| p.drag) {
            self.set_adjust(v);
        }
        self.tool = None;
    }

    /// Keep the tool and its preview in step with the photo: close the tool when the
    /// photo, its shape or the view changed; finish opening it once the photo shows;
    /// re-render the preview when the values changed; drop a preview no longer needed.
    fn sync_tool(&mut self) {
        if let Some(p) = &self.tool {
            let same_shape = p.base.is_none() || p.geometry == self.stack().geometry();
            if p.index != self.index || self.view != View::Photo || self.mode != Mode::Browse || !same_shape {
                self.close_tool();
            }
        }
        if self.tool.as_ref().is_some_and(|p| p.base.is_none())
            && let Some(base) = self.preview_base()
        {
            let stack = self.stack();
            let Some(p) = &mut self.tool else { return };
            let opening = p.tool.opening(&base);
            (p.base, p.opening, p.geometry) = (Some(Arc::new(base)), Some(opening), stack.geometry());
            if stack.adjust(p.tool).is_none() {
                self.set_adjust(opening);
                self.sync_display();
            }
        }
        let mut stack = self.stack();
        let now = self.now();
        match &self.tool {
            Some(ToolPanel { tool, drag, base: Some(base), rendered_at, .. }) => {
                if let Some(v) = drag {
                    stack.set_adjust(*tool, Some(*v));
                }
                // While dragging, at most every PREVIEW_EVERY; a release renders at once.
                let wait = drag.is_some() && rendered_at.is_some_and(|t| now < t + PREVIEW_EVERY);
                if !wait && self.preview.as_ref().is_none_or(|pr| pr.index != self.index || pr.stack != stack)
                {
                    let t0 = Instant::now();
                    let mut img = (**base).clone();
                    adjust::apply_all(&mut img, &stack);
                    let (w, h) = img.dimensions();
                    let handle = Handle::from_rgba(w, h, img.into_raw());
                    if self.trace {
                        let ms = t0.elapsed().as_secs_f64() * 1000.0;
                        eprintln!("trace: preview {w}x{h} rendered in {ms:.1} ms");
                    }
                    self.preview = Some(Preview { index: self.index, stack, handle });
                    let dragging = drag.is_some();
                    if let Some(p) = &mut self.tool {
                        p.rendered_at = dragging.then_some(now);
                    }
                }
            }
            // Opening: the photo shows as it is.
            Some(_) => {}
            None => {
                if self
                    .preview
                    .as_ref()
                    .is_some_and(|pr| pr.index != self.index || pr.stack != stack || self.picture().is_some())
                {
                    self.preview = None;
                }
            }
        }
    }

    /// The current photo, loaded with its current geometry, at screen size without
    /// adjustments.
    fn preview_base(&self) -> Option<RgbaImage> {
        let img = self.picture()?.unadjusted()?;
        let body = self.body_size();
        let (w, h) = img.dimensions();
        let k = (body.width * PREVIEW_SCALE / w as f32).min(body.height * PREVIEW_SCALE / h as f32);
        Some(match k < 1.0 {
            true => {
                let (nw, nh) =
                    (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
                image::imageops::thumbnail(&img, nw, nh)
            }
            false => img,
        })
    }

    /// The image area's top image (script mode puts it on the GPU before the shot).
    pub fn image_handle(&self) -> Option<&Handle> {
        self.original_handle().or(self.main_handle())
    }

    /// Leaving the current photo: a draft crop is applied (crop mode stays on, for the
    /// next photo), an open tool closed, a dirty photo saved.
    fn leave(&mut self) {
        if self.view != View::Photo {
            return;
        }
        self.apply_draft();
        self.close_tool();
        if self.dirty()
            && let Some(path) = self.current_path().map(Path::to_path_buf)
        {
            self.queue_save(path);
        }
    }

    fn enqueue(&mut self, job: Queued) {
        if !self.queue.contains(&job) {
            self.queue.push_back(job);
        }
    }

    /// Save a dirty photo in the background.
    fn queue_save(&mut self, path: PathBuf) {
        if self.edits.get(&path).is_some_and(|p| p.edits.dirty()) {
            self.enqueue(Queued::Save(path));
        }
    }

    /// Save before anything else queued.
    fn save_now(&mut self, path: PathBuf) {
        let job = Queued::Save(path);
        self.queue.retain(|q| *q != job);
        self.queue.push_front(job);
    }

    /// Paths of the photos queued for saving (and the one being saved).
    pub fn saving(&self) -> Vec<&Path> {
        self.running
            .iter()
            .chain(&self.queue)
            .filter_map(|q| match q {
                Queued::Save(p) => Some(p.as_path()),
                Queued::Store(_) => None,
            })
            .collect()
    }

    /// The next sidecar write or save to run, when the worker is free. The stack is the
    /// photo's latest.
    pub fn next_io(&mut self) -> Option<Io> {
        if self.running.is_some() {
            return None;
        }
        let job = self.queue.pop_front()?;
        let io = match &job {
            Queued::Store(p) => Io::Store(p.clone(), self.stack_of(p)),
            Queued::Save(p) => Io::Save(p.clone(), self.stack_of(p)),
        };
        self.running = Some(job);
        Some(io)
    }

    pub fn finish_io(&mut self, done: IoDone) {
        self.running = None;
        match done {
            IoDone::Stored(_, Ok(())) => {}
            IoDone::Stored(path, Err(e)) => {
                self.error(format!("cannot keep the edits of {}: {e}", path.display()))
            }
            IoDone::Saved(path, Err(e)) => {
                self.error(format!("cannot save: {e}"));
                self.save_failed.insert(path, e);
            }
            IoDone::Saved(path, Ok(report)) => self.saved(path, report),
        }
        if self.io_idle() {
            self.notice = None;
        }
        self.sync_display();
        self.sync_under();
    }

    /// A save finished: the photo may have a new name, its cached images are reloaded,
    /// and the sidecar's record of the save is taken over.
    fn saved(&mut self, path: PathBuf, report: SaveReport) {
        self.save_failed.remove(&path);
        if report.action == "unchanged" {
            return;
        }
        let new = report.path;
        let index = self.folder.as_ref().and_then(|f| f.entries.iter().position(|e| e.path == path));
        let mut p = self.edits.remove(&path).unwrap_or_default();
        if new != path {
            if let Some(i) = index
                && let Some(f) = &mut self.folder
            {
                f.entries[i].name = new.file_name().unwrap_or_default().to_string_lossy().into_owned();
                f.entries[i].path = new.clone();
            }
            self.queue.retain(|q| !matches!(q, Queued::Store(p) | Queued::Save(p) if *p == path));
        }
        match store::load(&new) {
            Ok(on_disk) => {
                let on_disk = on_disk.unwrap_or_default();
                if on_disk.stack != p.edits.stack {
                    // Changed while it was saving: keep the newer stack in the sidecar.
                    self.enqueue(Queued::Store(new.clone()));
                }
                p.edits = PhotoEdits { stack: p.edits.stack, ..on_disk };
            }
            Err(e) => self.error(e.to_string()),
        }
        self.edits.insert(new, p);
        if let Some(i) = index {
            self.loader.invalidate(i);
            self.thumbs.invalidate(i);
            self.sizes.unset(i);
            self.displayed.remove(&i);
            if self.original_image.as_ref().is_some_and(|o| o.job.index == i) {
                self.original_image = None;
            }
        }
    }

    /// The original-decode the current photo needs: while it is held, or ahead of time for
    /// an edited photo once its own image is loaded. Call until `None`.
    pub fn next_original_job(&mut self) -> Option<OriginalJob> {
        if self.view != View::Photo || self.mode != Mode::Browse {
            return None;
        }
        let photo = self.current_path()?.to_path_buf();
        let edited = !self.stack().is_empty();
        if !edited || !(self.original || self.current().is_some()) {
            return None;
        }
        let path = self.edits.get(&photo).and_then(|p| p.edits.backup.clone()).unwrap_or(photo);
        let body = self.body_size();
        let side = ((body.width.max(body.height) * PREVIEW_SCALE) as u32).min(self.max_side);
        let job = OriginalJob { index: self.index, path, side };
        if self.original_image.as_ref().is_some_and(|o| o.job == job) {
            return None;
        }
        self.original_image = Some(Original { job: job.clone(), image: None });
        Some(job)
    }

    pub fn finish_original(&mut self, job: OriginalJob, result: Loaded) {
        if let Some(o) = self.original_image.as_mut().filter(|o| o.job == job) {
            if let Err(e) = &result {
                eprintln!("photo-cruiser: original of {}: {e}", job.path.display());
            }
            o.image = Some(result);
        }
    }

    /// The current photo's original, decoded (held or not).
    pub fn original_ready(&self) -> Option<&Handle> {
        match &self.original_image.as_ref().filter(|o| o.job.index == self.index)?.image {
            Some(Ok(p)) => Some(&p.handle),
            _ => None,
        }
    }

    /// The original, while it is held and decoded (an unedited photo shows as it is).
    pub fn original_handle(&self) -> Option<&Handle> {
        let o = self.original_image.as_ref().filter(|o| self.original && o.job.index == self.index)?;
        let edited = !self.stack().is_empty();
        match &o.image {
            Some(Ok(p)) if edited && self.view == View::Photo && self.mode == Mode::Browse => Some(&p.handle),
            _ => None,
        }
    }

    /// What photo `index` should show: its edits, but the whole original while it is
    /// being cropped.
    fn display_stack(&self, index: usize) -> EditStack {
        let Some(path) = self.folder.as_ref().and_then(|f| f.entries.get(index)).map(|e| &e.path) else {
            return EditStack::default();
        };
        let mut stack = self.stack_of(path);
        if index == self.index && self.mode == Mode::Crop {
            stack.set_crop(None);
        }
        stack
    }

    /// Reload the images whose edits (or crop mode) changed since they were rendered.
    /// The old image stays on screen until the new one is ready.
    fn sync_display(&mut self) {
        let changed: Vec<usize> =
            self.displayed.iter().filter(|(i, s)| self.display_stack(**i) != **s).map(|(i, _)| *i).collect();
        for i in changed {
            self.displayed.remove(&i);
            self.loader.invalidate(i);
        }
    }

    fn select(&mut self, index: usize) {
        if index != self.index {
            self.leave();
        }
        let now = Instant::now();
        self.selected_at = [Some(now), self.selected_at[0]];
        self.index = index;
        self.crop.clear();
        if let Some(path) = self.current_path().map(Path::to_path_buf) {
            self.load_edits(&path);
            if self.mode == Mode::Crop {
                self.crop.selection = self.crop_selection();
            }
        }
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
        self.displayed.clear();
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
                if let Some(path) = self.current_path().map(Path::to_path_buf) {
                    self.load_edits(&path);
                }
                // Edits left unsaved last time (the app was killed): save them now.
                let left = store::dirty_photos(&dir);
                if !left.is_empty() {
                    let n = left.len();
                    self.notice =
                        Some(format!("saving {n} photo{} edited last time", if n == 1 { "" } else { "s" }));
                    for path in left {
                        self.load_edits(&path);
                        self.queue_save(path);
                    }
                }
            }
            Err(e) => {
                let msg = format!("Cannot open folder: {e}");
                self.folder = Some(Folder { path: dir, entries: Vec::new() });
                self.folder_error = Some(msg.clone());
                self.error(msg);
            }
        }
    }

    /// Source size of the current photo (zero without one).
    fn image_size(&self) -> Size {
        self.picture().map(Picture::source_size).unwrap_or_default()
    }

    /// The crop's width / height on the current photo; `None` for a free crop.
    pub fn crop_ratio(&self) -> Option<f32> {
        self.crop.aspect.ratio(self.image_size(), self.crop.swapped)
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

    /// The next decode to start, with the file to decode (the original, for a full
    /// decode) and the edits to render, while a loader slot is free. Call until `None`.
    pub fn next_job(&mut self) -> Option<LoadJob> {
        let job = self.loader.next_job(self.index, self.len())?;
        if self.trace {
            self.job_started.push((job, Instant::now()));
        }
        let photo = self.folder.as_ref()?.entries.get(job.index)?.path.clone();
        self.load_edits(&photo);
        let stack = self.display_stack(job.index);
        let path = match (job.kind, &self.edits[&photo].edits.backup) {
            (Kind::Full, Some(backup)) => backup.clone(),
            _ => photo,
        };
        if job.kind == Kind::Full {
            self.displayed.insert(job.index, stack.clone());
        }
        Some(LoadJob { job, path, stack })
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
            self.sync_tool();
        }
        self.sync_under();
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
        if self.quitting {
            let n = self.saving().len();
            return format!("Saving {n} photo{}…", if n == 1 { "" } else { "s" });
        }
        if self.original_handle().is_some() {
            s += " · original, without edits";
        }
        if let Some(px) = self.crop_pixels() {
            s += &format!(" · crop {px}");
        } else if self.mode == Mode::Crop {
            s += " · crop: drag on the image (Esc to leave)";
        }
        if let Some(e) = self.current_path().and_then(|p| self.save_failed.get(p)) {
            s += &format!(" — not saved: {e}");
        } else if let Some(notice) = &self.notice {
            s += &format!(" — {notice}");
        } else if !self.saving().is_empty() {
            s += " — saving…";
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
            menu_focus: self.menu_focus,
            help: self.help,
            about: self.about,
            aspect: self.crop.aspect,
            aspect_swapped: self.crop.swapped,
            aspect_input: self.aspect_input.clone(),
            palette: self.palette.clone(),
            palette_items: match self.palette {
                Some(_) => palette::filtered(self).into_iter().map(|c| c.label).collect(),
                None => Vec::new(),
            },
            names: self.names,
            layout: self.layout,
            crop: self.crop_pixels(),
            title: self.title(),
            status: self.status(),
            loading: self.current_path().is_some() && self.current().is_none(),
            display: self.display(),
            edits: self.stack().ops,
            tool: self.tool.as_ref().map(|p| ToolDump {
                tool: p.tool,
                value: p.value(&self.stack()),
                opening: p.opening,
                dragging: p.drag.is_some(),
            }),
            preview: self.preview.as_ref().is_some_and(|p| p.index == self.index),
            underlay: self.underlay().is_some(),
            original: self.original_handle().is_some(),
            dirty: self.dirty(),
            saved: self
                .current_path()
                .and_then(|p| self.edits.get(p))
                .and_then(|p| p.edits.saved.clone())
                .map(|s| s.ops),
            saving: self.saving().iter().map(|p| p.display().to_string()).collect(),
            backup: self.backup().map(|b| b.display().to_string()),
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
    pub menu_focus: Option<usize>,
    pub help: bool,
    pub about: bool,
    /// The crop's aspect: `original`, `free`, `W:H`.
    pub aspect: Aspect,
    /// The aspect turned portrait ↔ landscape.
    pub aspect_swapped: bool,
    /// The custom aspect field's text, while it shows.
    pub aspect_input: Option<String>,
    /// The command palette, while it shows.
    pub palette: Option<Palette>,
    /// Its rows (the commands matching its query).
    pub palette_items: Vec<String>,
    pub names: bool,
    pub layout: grid::Layout,
    /// Integer source pixels.
    pub crop: Option<PixelRect>,
    pub title: String,
    pub status: String,
    /// The current image is still decoding.
    pub loading: bool,
    pub display: Display,
    /// The current photo's edit stack.
    pub edits: Vec<Op>,
    /// The open one-slider tool.
    pub tool: Option<ToolDump>,
    /// The image area shows the tool's live preview.
    pub preview: bool,
    /// The image the main one replaced still shows under it: the main one isn't
    /// confirmed on the GPU yet (no blank frames while it uploads).
    pub underlay: bool,
    /// The original shows, without edits (held: `show_original`).
    pub original: bool,
    /// Its edits differ from what its file was saved with.
    pub dirty: bool,
    /// The stack its file was saved with (`null`: never saved).
    pub saved: Option<Vec<Op>>,
    /// Photos queued for saving.
    pub saving: Vec<String>,
    /// The backup of its original, if it was saved.
    pub backup: Option<String>,
    pub errors: Vec<String>,
    /// Logical size of the window (from the screenshot or `window::size`).
    pub window: Option<WindowSize>,
    pub shot: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ToolDump {
    pub tool: Tool,
    /// What the slider shows.
    pub value: f32,
    /// The value it opened at (`null` while the photo loads).
    pub opening: Option<f32>,
    pub dragging: bool,
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
