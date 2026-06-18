//! Per-format readable-text extraction for diffing.
//!
//! v1 implements `.docx` only. `.xlsx`, `.pptx`, and `.pdf` are planned but
//! not yet supported: until each has a text extractor, `add` rejects them
//! rather than tracking files we can't meaningfully diff. To add a format,
//! write its `extract` module and add its extension here and in `extract_text`
//! / `can_diff`.

pub mod docx;

pub use docx::{ChangeKind, TrackedChange};

use anyhow::{Result, bail};
use std::path::Path;

/// File extensions dvault accepts at `add` time. Extend this as extractors
/// are implemented (planned: xlsx, pptx, pdf).
pub const SUPPORTED: &[&str] = &["docx"];

/// Lowercased extension of a path, if any.
pub fn extension(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

pub fn is_supported(path: &str) -> bool {
    matches!(extension(path), Some(ext) if SUPPORTED.contains(&ext.as_str()))
}

/// Whether we can produce a readable text diff for this file type.
pub fn can_diff(path: &str) -> bool {
    matches!(extension(path).as_deref(), Some("docx"))
}

/// Extract readable text (one logical line per element) from raw file bytes.
pub fn extract_text(path: &str, bytes: &[u8]) -> Result<Vec<String>> {
    match extension(path).as_deref() {
        Some("docx") => docx::extract(bytes),
        Some(other) => bail!("text diff not yet supported for .{other} files"),
        None => bail!("cannot determine file type for {path}"),
    }
}

/// Extract Word tracked changes (revision marks) from raw file bytes.
pub fn tracked_changes(path: &str, bytes: &[u8]) -> Result<Vec<TrackedChange>> {
    match extension(path).as_deref() {
        Some("docx") => docx::tracked_changes(bytes),
        Some(other) => bail!("tracked changes are only supported for .docx files, not .{other}"),
        None => bail!("cannot determine file type for {path}"),
    }
}
