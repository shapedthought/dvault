//! `dvault compare` — readable diff between two loose `.docx` files on disk,
//! with **no vault required**.
//!
//! For the "Save As" reality: a folder of near-identical version files where
//! you just want to see what changed between two of them. Reuses the same
//! extraction + diff renderer as `dvault diff`, so the output (colored, with
//! inline word highlighting) is identical — it just operates on two file paths
//! instead of commits.

use crate::diff::show_file_diff;
use anyhow::{Context, Result};

pub fn run(old: String, new: String) -> Result<()> {
    let old_bytes = std::fs::read(&old).with_context(|| format!("could not read {old}"))?;
    let new_bytes = std::fs::read(&new).with_context(|| format!("could not read {new}"))?;
    // `new` provides the file type for extraction/diffability; the paths are
    // used as the diff headers.
    show_file_diff(&new, old, old_bytes, new.clone(), new_bytes)
}
