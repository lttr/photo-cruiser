//! Photo Cruiser core: everything about photos, nothing about GUIs.
//!
//! Synchronous and side-effect free where possible. Callers (the CLI, the iced app) own
//! concurrency; [`loader::Loader`] is the small helper they share for "one decode in
//! flight, always for the latest request".

pub mod catalog;
pub mod crop;
pub mod decode;
pub mod edit;
pub mod error;
pub mod export;
pub mod geom;
pub mod justified;
pub mod loader;
pub mod render;
pub mod source;

pub use catalog::{Entry, Folder};
pub use decode::{Decoded, Format, Probe, decode, embedded_preview, probe, thumbnail};
pub use edit::{EditStack, Op};
pub use error::{Error, Result};
pub use geom::{Point, Rect, Size};
