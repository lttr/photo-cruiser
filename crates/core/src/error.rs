use crate::Rect;
use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The single error type of `core`. Bad input files produce errors, never panics.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("{path}: unsupported format")]
    Unsupported { path: PathBuf },
    #[error("{path}: unsupported output format (use .png or .jpg)")]
    UnsupportedOutput { path: PathBuf },
    #[error("{path}: decode failed: {message}")]
    Decode { path: PathBuf, message: String },
    #[error("{path}: encode failed: {message}")]
    Encode { path: PathBuf, message: String },
    #[error("invalid edits: {0}")]
    Edits(String),
    /// A crop with no pixels left after clamping to the image.
    #[error("crop {},{},{},{} is empty after clamping to the image", .0.x, .0.y, .0.w, .0.h)]
    EmptyCrop(Rect),
    /// The photo's folder can't be written (Save needs it).
    #[error("{path}: the folder is read-only, the photo can't be saved")]
    ReadOnly { path: PathBuf },
    /// Save would convert to a file name another photo already has.
    #[error("{path}: already exists, the photo can't be saved under that name")]
    NameTaken { path: PathBuf },
    #[error("{path}: animated images can't be edited")]
    Animated { path: PathBuf },
    #[error("{path}: 16-bit images can't be saved yet")]
    HighBitDepth { path: PathBuf },
    /// The photo was changed by another program after it was saved; its edits are ignored.
    /// A warning, not a failure.
    #[error("{path}: changed outside Photo Cruiser, its saved edits are ignored")]
    StaleEdits { path: PathBuf },
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }

    pub(crate) fn decode(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Decode { path: path.into(), message: message.to_string().trim().to_string() }
    }

    pub(crate) fn encode(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Encode { path: path.into(), message: message.to_string().trim().to_string() }
    }

    /// Stable machine-readable kind, used in JSON output.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io { .. } => "io",
            Error::Unsupported { .. } => "unsupported",
            Error::UnsupportedOutput { .. } => "unsupported_output",
            Error::Decode { .. } => "decode",
            Error::Encode { .. } => "encode",
            Error::Edits(_) => "edits",
            Error::EmptyCrop(_) => "empty_crop",
            Error::ReadOnly { .. } => "read_only",
            Error::NameTaken { .. } => "name_taken",
            Error::Animated { .. } => "animated",
            Error::HighBitDepth { .. } => "high_bit_depth",
            Error::StaleEdits { .. } => "stale_edits",
        }
    }
}
