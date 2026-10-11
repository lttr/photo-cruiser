use crate::action::Action;
use crate::script;
use crate::state::Loaded;
use photo_cruiser_core::Point;
use photo_cruiser_core::crop::MouseKind;
use photo_cruiser_core::loader::{Job, SizeJob, ThumbJob};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Message {
    /// Everything user-visible goes through an [`Action`].
    Action(Action),
    /// `Ctrl+O` / menu: show the folder picker (interactive only, not an `Action`).
    OpenDialog,
    FolderPicked(Option<PathBuf>),
    /// Redraw tick while a fade runs.
    Frame,
    /// The window was resized (maybe into or out of full screen).
    Resized(iced::window::Id),
    /// The window's real mode after a resize: full screen or not.
    WindowMode(bool),
    /// A background decode finished.
    Decoded(Job, Loaded),
    /// A grid thumbnail finished.
    Thumb(ThumbJob, Loaded),
    /// Photo sizes probed (`None`: unreadable, or the probe thread died).
    Sizes(SizeJob, Vec<Option<(u32, u32)>>),
    /// The image / grid area has a new size (reported by the overlay when it draws).
    Body(iced::Size),
    /// The grid was scrolled to this vertical offset.
    GridScrolled(f32),
    /// Mouse press/move/release over the image: source pixel (unclamped), grab tolerance
    /// in source pixels.
    Crop(MouseKind, Point, f32),
    /// The custom aspect field was edited.
    AspectInput(String),
    /// Script-mode internal steps.
    Script(script::Event),
}

impl From<Action> for Message {
    fn from(a: Action) -> Self {
        Message::Action(a)
    }
}
