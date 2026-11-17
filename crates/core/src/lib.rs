//! Photo Cruiser core: everything about photos, nothing about GUIs.
//!
//! Synchronous and side-effect free where possible. Callers (the CLI, the iced app) own
//! concurrency; [`loader::Loader`] is the small helper they share for "one decode in
//! flight, always for the latest request".

pub mod adjust;
pub mod catalog;
pub mod colour;
pub mod contrast;
pub mod crop;
pub mod decode;
pub mod dehaze;
pub mod edit;
pub mod error;
pub mod export;
pub mod export_tree;
pub mod geom;
pub mod info;
pub mod justified;
pub mod levels;
pub mod loader;
pub mod local_contrast;
pub mod meta;
pub mod rating;
pub mod render;
pub mod sharpen;
pub mod smooth;
pub mod source;
pub mod store;
pub mod straighten;
pub mod tones;
pub mod vignette;
pub mod warmth;
pub mod zoom;

pub use adjust::Tool;
pub use catalog::{Entry, Folder};
pub use decode::{Decoded, Format, Probe, decode, embedded_preview, probe, thumbnail};
pub use edit::{EditStack, History, Op};
pub use error::{Error, Result};
pub use geom::{Point, Rect, Size};
pub use zoom::Zoom;
