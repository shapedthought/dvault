//! `dvault export` — write a historic version to a new file, leaving the
//! working copy untouched.

use crate::db::{Db, SHORT_HASH_LEN};
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub fn run(commit: String, file: String, out: Option<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let rel = vault.relativize(&file)?;
    let commit_id = crate::revparse::resolve(&vault, &db, &commit)?;
    let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
    let cf = db
        .get_commit_file(&commit_id, &rel)?
        .with_context(|| format!("{rel} was not snapshotted in commit {short}"))?;

    let out_path = match out {
        Some(p) => PathBuf::from(p),
        None => default_out_name(&rel, &short),
    };

    let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
    std::fs::write(&out_path, bytes)
        .with_context(|| format!("could not write {}", out_path.display()))?;

    println!("Exported to {}", out_path.display());
    Ok(())
}

/// `report.docx` + `a3f9c12` -> `report-a3f9c12.docx`.
fn default_out_name(rel: &str, short: &str) -> PathBuf {
    let path = Path::new(rel);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("export");
    let ext = path.extension().and_then(|s| s.to_str());
    let name = match ext {
        Some(ext) => format!("{stem}-{short}.{ext}"),
        None => format!("{stem}-{short}"),
    };
    // Write into the same directory as the original.
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}
