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
        }
    }
}
