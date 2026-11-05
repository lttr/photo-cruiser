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
    /// The photo's folder can't be written (Save and the star need it).
    #[error("{path}: the folder is read-only, nothing can be written there")]
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

    /// An `image` error: its I/O failures are [`Error::Io`], the rest [`Error::Decode`].
    pub(crate) fn image(path: impl Into<PathBuf>, e: image::ImageError) -> Self {
        match e {
            image::ImageError::IoError(source) => Error::io(path, source),
            e => Error::decode(path, e),
        }
    }

    /// An I/O failure that may succeed if tried again (EIO, timeouts, interrupted or
    /// would-block reads), as network shares produce. Missing files and denied access are
    /// not transient.
    pub fn is_transient(&self) -> bool {
        use std::io::ErrorKind::*;
        let Error::Io { source, .. } = self else { return false };
        source.raw_os_error() == Some(5) || matches!(source.kind(), TimedOut | Interrupted | WouldBlock)
    }

    pub(crate) fn encode(path: impl Into<PathBuf>, message: impl ToString) -> Self {
        Error::Encode { path: path.into(), message: message.to_string().trim().to_string() }
    }

    /// Why it failed, for people: one sentence without the path (the UI shows the file
    /// name elsewhere) and without decoder internals (those stay in [`Display`]).
    ///
    /// [`Display`]: std::fmt::Display
    pub fn reason(&self) -> String {
        use std::io::ErrorKind::*;
        match self {
            Error::Io { source, .. } => match source.kind() {
                NotFound => "The file is gone: it was moved or deleted.".into(),
                PermissionDenied => "You don't have permission to read this file.".into(),
                _ if self.is_transient() => "The file couldn't be read. Try again in a moment.".into(),
                _ => format!("The file couldn't be read ({source})."),
            },
            Error::Unsupported { .. } => "Photo Cruiser can't open this file format.".into(),
            Error::Decode { .. } => "The file is damaged or incomplete.".into(),
            e => {
                let s = e.to_string();
                let s = match e.path() {
                    Some(p) => s.strip_prefix(&format!("{}: ", p.display())).unwrap_or(&s).to_string(),
                    None => s,
                };
                let mut c = s.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect::<String>() + ".").unwrap_or_default()
            }
        }
    }

    /// The file the error is about, if any.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            Error::Io { path, .. }
            | Error::Unsupported { path }
            | Error::UnsupportedOutput { path }
            | Error::Decode { path, .. }
            | Error::Encode { path, .. }
            | Error::ReadOnly { path }
            | Error::NameTaken { path }
            | Error::Animated { path }
            | Error::HighBitDepth { path }
            | Error::StaleEdits { path } => Some(path),
            Error::Edits(_) | Error::EmptyCrop(_) => None,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_has_no_path_or_decoder_detail() {
        let e = Error::decode("/x/a.jpg", "Format error decoding Jpeg: Not enough bytes");
        assert_eq!(e.reason(), "The file is damaged or incomplete.");
        let e = Error::io("/x/a.jpg", std::io::ErrorKind::NotFound.into());
        assert_eq!(e.reason(), "The file is gone: it was moved or deleted.");
        assert_eq!(Error::Animated { path: "/x/a.gif".into() }.reason(), "Animated images can't be edited.");
        assert_eq!(Error::Edits("bad".into()).reason(), "Invalid edits: bad.");
    }
}
