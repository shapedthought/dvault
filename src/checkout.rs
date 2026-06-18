//! `dvault checkout` — restore a file to a historic version, overwriting the
//! working copy. Prompts before overwriting unless `--force`.

use crate::db::{Db, SHORT_HASH_LEN};
use crate::log::format_timestamp;
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use std::io::Write;

pub fn run(commit: String, file: String, force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let rel = vault.relativize(&file)?;
    let commit_id = crate::revparse::resolve(&vault, &db, &commit)?;
    let meta = db.get_commit(&commit_id)?;
    let cf = db.get_commit_file(&commit_id, &rel)?.with_context(|| {
        let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
        format!("{rel} was not snapshotted in commit {short}")
    })?;

    let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
    let when = format_timestamp(&meta.created_at);

    let working = vault.working_path(&rel);
    if working.exists() && !force && !confirm_overwrite(&rel, &short, &when, &meta.message)? {
        bail!("Aborted. {rel} was not changed.");
    }

    let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
    std::fs::write(&working, bytes)
        .with_context(|| format!("could not write {}", working.display()))?;

    println!("Restored {rel} to version {short}");
    Ok(())
}

/// Prompt `[y/N]` before overwriting. Returns whether the user confirmed.
/// Reads a line from stdin so it works both interactively and when piped; an
/// empty answer or EOF (no input) defaults to "no".
fn confirm_overwrite(rel: &str, short: &str, when: &str, message: &str) -> Result<bool> {
    println!("Restore {rel} to version {short} ({when} \"{message}\")?");
    print!("This will overwrite the current file. [y/N] ");
    std::io::stdout().flush().ok();

    let mut answer = String::new();
    let read = std::io::stdin()
        .read_line(&mut answer)
        .context("could not read confirmation")?;
    if read == 0 {
        return Ok(false); // EOF / no TTY: default to no
    }
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}
