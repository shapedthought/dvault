//! `dvault switch` — move `HEAD` to another branch and update the working
//! files to that branch's versions.

use crate::config::Config;
use crate::db::Db;
use crate::refs;
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;

pub fn run(branch: String, force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let db = Db::open(&vault.db_path())?;

    if !refs::branch_exists(&vault, &branch) {
        bail!("No such branch: {branch}. Create it with 'dvault branch {branch}'.");
    }
    let current = refs::current_branch(&vault)?;
    if current == branch {
        println!("Already on branch {branch}");
        return Ok(());
    }

    // Refuse to switch over uncommitted changes unless forced — otherwise the
    // restore below would silently overwrite them.
    if !force {
        let current_tip = refs::branch_tip(&vault, &current)?;
        let modified = modified_files(&vault, &db, &config, current_tip.as_deref())?;
        if !modified.is_empty() {
            bail!(
                "You have uncommitted changes to: {}.\nCommit them or re-run with --force to discard.",
                modified.join(", ")
            );
        }
    }

    let target_tip = refs::branch_tip(&vault, &branch)?;
    if let Some(tip) = &target_tip {
        restore_working_tree(&vault, &db, &config, tip)?;
    }

    refs::set_head(&vault, &branch)?;
    println!("Switched to branch {branch}");
    Ok(())
}

/// Tracked files whose working copy differs from its version at `tip`
/// (including tracked-but-never-committed "new" files).
pub fn modified_files(
    vault: &Vault,
    db: &Db,
    config: &Config,
    tip: Option<&str>,
) -> Result<Vec<String>> {
    let mut modified = Vec::new();
    for rel in &config.tracked.files {
        let working = vault.working_path(rel);
        if !working.is_file() {
            continue; // missing working file isn't an uncommitted edit
        }
        let bytes = std::fs::read(&working)
            .with_context(|| format!("could not read {}", working.display()))?;
        let hash = store::hash_bytes(&bytes);
        let committed = match tip {
            Some(t) => db.file_at(t, rel)?,
            None => None,
        };
        match committed {
            Some(cf) if cf.blob_hash == hash => {}
            _ => modified.push(rel.clone()),
        }
    }
    Ok(modified)
}

/// Write every file recorded in `tip`'s history back to the working tree at its
/// `tip` version. Files not present in `tip` are left untouched (never deleted).
pub fn restore_working_tree(vault: &Vault, db: &Db, config: &Config, tip: &str) -> Result<()> {
    // Union of currently-tracked files and everything in the target history.
    let mut files: BTreeSet<String> = config.tracked.files.iter().cloned().collect();
    files.extend(db.files_in_history(tip)?);

    for rel in files {
        if let Some(cf) = db.file_at(tip, &rel)? {
            let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
            let working = vault.working_path(&rel);
            if let Some(parent) = working.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&working, bytes)
                .with_context(|| format!("could not write {}", working.display()))?;
        }
    }
    Ok(())
}
