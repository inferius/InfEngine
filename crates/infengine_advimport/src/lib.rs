//! Offline importer for adventure-game data files.
//!
//! This crate converts data from other engines into InfEngine's own room
//! format, once, ahead of time. The engine never reads a foreign format
//! at runtime - it only ever loads the `.ron` rooms and `.png` images
//! this tool writes. That keeps the shipped game free of decoder code
//! and free of anyone else's data.
//!
//! ```text
//!   original data  ->  adv-import  ->  rooms/*.ron + images/*.png  ->  engine
//!   (your own copy)                    (our format)
//! ```
//!
//! Backends live in their own modules and share the [`report`] types, so
//! adding a game means adding a backend rather than touching the engine.
//! [`survey`] is the reconnaissance pass used to work out what an
//! unfamiliar directory contains before writing that backend.

#![forbid(unsafe_code)]

#[cfg(feature = "mt32")]
pub mod ogg;
pub mod wav;
pub mod bm;
pub mod bm1;
pub mod encoding;
pub mod kyra;
pub mod report;
pub mod survey;

use std::fmt;

pub use report::{ImportReport, Skipped};

/// Anything that can stop an import.
#[derive(Debug)]
pub enum ImportError {
    ReadDir {
        path: String,
        source: std::io::Error,
    },
    ReadFile {
        path: String,
        source: std::io::Error,
    },
    WriteFile {
        path: String,
        source: std::io::Error,
    },
    /// The source directory holds nothing this backend recognises.
    NothingToImport {
        path: String,
        looking_for: String,
    },
    /// A file was readable but not what its extension claimed.
    Format {
        path: String,
        detail: String,
    },
    EncodeImage {
        path: String,
        detail: String,
    },
    WriteRoom {
        path: String,
        detail: String,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadDir { path, source } => write!(f, "cannot read directory {path}: {source}"),
            Self::ReadFile { path, source } => write!(f, "cannot read {path}: {source}"),
            Self::WriteFile { path, source } => write!(f, "cannot write {path}: {source}"),
            Self::NothingToImport { path, looking_for } => {
                write!(f, "found no {looking_for} in {path}")
            }
            Self::Format { path, detail } => write!(f, "{path} is malformed: {detail}"),
            Self::EncodeImage { path, detail } => write!(f, "cannot encode {path}: {detail}"),
            Self::WriteRoom { path, detail } => write!(f, "cannot write room {path}: {detail}"),
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ReadDir { source, .. }
            | Self::ReadFile { source, .. }
            | Self::WriteFile { source, .. } => Some(source),
            _ => None,
        }
    }
}
