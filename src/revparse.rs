//! Resolve a user-supplied commit reference to a full commit id.
//!
//! A reference may be a tag name or a (possibly abbreviated) commit hash. Tags
//! take precedence: if a tag with that exact name exists, it wins even if the
//! string also looks like a hash prefix — so a tag always resolves to its
//! target.

use crate::db::{Db, SHORT_HASH_LEN};
use crate::store;
use crate::tag;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};

pub fn resolve(vault: &Vault, db: &Db, reference: &str) -> Result<String> {
    if let Some(id) = tag::tag_commit(vault, reference)? {
        // Validate the tag still points at a real commit; otherwise fall
        // through to hash resolution so the error mentions the hash.
        if db.get_commit(&id).is_ok() {
            return Ok(id);
        }
    }
    db.resolve_commit(reference)
}

/// Resolve `diff`-style args into a file's vault-relative path and bytes.
/// Shared by commands that read one file version:
///
///   [file]            → the working copy
///   [commit, file]    → the file as snapshotted in that commit/tag
pub fn file_bytes(vault: &Vault, db: &Db, args: &[String]) -> Result<(String, Vec<u8>)> {
    match args {
        [file] => {
            let rel = vault.relativize(file)?;
            let working = vault.working_path(&rel);
            if !working.is_file() {
                bail!("File not found: {file}");
            }
            let bytes = std::fs::read(&working)
                .with_context(|| format!("could not read {}", working.display()))?;
            Ok((rel, bytes))
        }
        [commitish, file] => {
            let rel = vault.relativize(file)?;
            let commit_id = resolve(vault, db, commitish)?;
            let cf = db.get_commit_file(&commit_id, &rel)?.with_context(|| {
                let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
                format!("{rel} was not snapshotted in commit {short}")
            })?;
            let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
            Ok((rel, bytes))
        }
        _ => bail!("expected <file>  OR  <commit> <file>"),
    }
}
