//! Branch references and `HEAD`, stored as plain files under `.dvault/`
//! (Git-style):
//!
//! - `HEAD` contains `ref: refs/heads/<branch>` — the branch you're on.
//! - `refs/heads/<branch>` contains the branch's tip commit id. The file is
//!   absent until the branch's first commit (an "unborn" branch).

use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

pub const DEFAULT_BRANCH: &str = "main";
const HEAD_PREFIX: &str = "ref: refs/heads/";

fn head_path(vault: &Vault) -> PathBuf {
    vault.dir.join("HEAD")
}

fn heads_dir(vault: &Vault) -> PathBuf {
    vault.dir.join("refs").join("heads")
}

fn branch_path(vault: &Vault, branch: &str) -> PathBuf {
    heads_dir(vault).join(branch)
}

/// Validate a branch name (same rules as tags: alphanumerics, `-`, `_`).
pub fn validate_branch_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Invalid branch name: {name}. Use letters, digits, hyphens and underscores.");
    }
    Ok(())
}

/// The branch `HEAD` currently points at. Defaults to `main` if `HEAD` is
/// missing (older vaults predating branch support).
pub fn current_branch(vault: &Vault) -> Result<String> {
    let path = head_path(vault);
    if !path.exists() {
        return Ok(DEFAULT_BRANCH.to_string());
    }
    let content = std::fs::read_to_string(&path).context("could not read HEAD")?;
    let line = content.trim();
    match line.strip_prefix(HEAD_PREFIX) {
        Some(branch) if !branch.is_empty() => Ok(branch.to_string()),
        _ => bail!("HEAD is malformed: {line:?}"),
    }
}

/// Point `HEAD` at `branch`.
pub fn set_head(vault: &Vault, branch: &str) -> Result<()> {
    std::fs::write(head_path(vault), format!("{HEAD_PREFIX}{branch}\n"))
        .context("could not write HEAD")
}

/// The tip commit id of `branch`, or None if the branch is unborn.
pub fn branch_tip(vault: &Vault, branch: &str) -> Result<Option<String>> {
    let path = branch_path(vault, branch);
    if !path.exists() {
        return Ok(None);
    }
    let id = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read branch {branch}"))?
        .trim()
        .to_string();
    Ok(if id.is_empty() { None } else { Some(id) })
}

/// Move `branch` to point at `commit_id`.
pub fn set_branch_tip(vault: &Vault, branch: &str, commit_id: &str) -> Result<()> {
    let dir = heads_dir(vault);
    std::fs::create_dir_all(&dir).context("could not create refs/heads")?;
    std::fs::write(branch_path(vault, branch), format!("{commit_id}\n"))
        .with_context(|| format!("could not update branch {branch}"))
}

pub fn branch_exists(vault: &Vault, branch: &str) -> bool {
    branch_path(vault, branch).exists()
}

/// Delete a branch ref. History (commits, blobs) is untouched.
pub fn delete_branch(vault: &Vault, branch: &str) -> Result<()> {
    std::fs::remove_file(branch_path(vault, branch))
        .with_context(|| format!("could not delete branch {branch}"))
}

/// All branch names, sorted.
pub fn list_branches(vault: &Vault) -> Result<Vec<String>> {
    let dir = heads_dir(vault);
    let mut names = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(&dir).context("could not read refs/heads")? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                names.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    // The current branch may be unborn (no ref file yet) — include it so it
    // shows in listings immediately after `init`.
    let current = current_branch(vault)?;
    if !names.contains(&current) {
        names.push(current);
    }
    names.sort();
    Ok(names)
}

/// The tip commit of the branch `HEAD` points at, if any.
pub fn head_tip(vault: &Vault) -> Result<Option<String>> {
    let branch = current_branch(vault)?;
    branch_tip(vault, &branch)
}
