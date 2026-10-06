//! The app model: folder, index, loader, crop, mode. Plain data + [`Action`] handling, no
//! iced runtime, so it can be driven directly in tests (decode synchronously with
//! [`decode_picture`] and feed results to [`State::finish`]).

use crate::action::Action;
use iced::widget::image::Handle;
use photo_cruiser_core::crop::{CropTool, MouseKind};
use photo_cruiser_core::geom::PixelRect;
use photo_cruiser_core::loader::{Job, Loader};
use photo_cruiser_core::{Folder, Point, Size};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Longest side of the displayed (GPU) image. Real GPUs take 8192+; 4096 keeps uploads
/// small. Crop coordinates are unaffected (always source pixels).
pub const DEFAULT_MAX_SIDE: u32 = 4096;

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

/// Decode `path` for display (blocking). Errors become strings for the UI.
pub fn decode_picture(path: &Path, max_side: u32) -> Loaded {
    let d = photo_cruiser_core::decode::decode_max_side(path, Some(max_side))
        .map_err(|e| e.to_string().trim_end().to_string())?;
    let (w, h) = d.image.dimensions();
    Ok(Picture { source: d.source_size(), handle: Handle::from_rgba(w, h, d.image.into_raw()) })
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
}

impl State {
    pub fn new(max_side: u32) -> Self {
        State {
            folder: None,
            folder_error: None,
            index: 0,
            loader: Loader::default(),
            crop: CropTool::default(),
            mode: Mode::Browse,
            max_side,
            errors: Vec::new(),
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

    /// Nothing decoding and current ± 1 loaded.
    pub fn is_idle(&self) -> bool {
        self.loader.is_idle(self.index, self.len())
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
        self.index = index;
        self.crop.clear();
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

    /// The next decode to start (at most one in flight), with the file to decode.
    pub fn next_job(&mut self) -> Option<(Job, PathBuf)> {
        let job = self.loader.next_job(self.index, self.len())?;
        let path = self.folder.as_ref()?.entries.get(job.index)?.path.clone();
        Some((job, path))
    }

    pub fn finish(&mut self, job: Job, result: Loaded) {
        self.loader.finish(job, result);
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
    pub errors: Vec<String>,
    /// Logical size of the window (from the screenshot or `window::size`).
    pub window: Option<WindowSize>,
    pub shot: Option<String>,
}
