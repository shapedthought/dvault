//! `dvault merge` — merge another branch into the current one using whole-file
//! resolution.
//!
//! Office documents are binary, so we don't blend file contents. Instead, for
//! each file we compare three versions — the common ancestor (base), ours, and
//! theirs:
//!
//! - only one side changed it → take that side automatically;
//! - both sides changed it → a conflict; the user picks ours or theirs (or
//!   views the readable diff first).
//!
//! Fast-forward and already-up-to-date cases are handled without a merge
//! commit. A genuine merge produces a commit with two parents.

use crate::config::Config;
use crate::db::{Commit, CommitFile, Db};
use crate::switch::{modified_files, restore_working_tree};
use crate::vault::Vault;
use crate::{diff, refs, store};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::collections::BTreeSet;
use std::io::Write;
use uuid::Uuid;

pub fn run(target: String) -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let mut db = Db::open(&vault.db_path())?;

    let current = refs::current_branch(&vault)?;
    if target == current {
        bail!("Cannot merge a branch into itself.");
    }
    let theirs_tip = refs::branch_tip(&vault, &target)?
        .with_context(|| format!("No such branch (or it has no commits): {target}"))?;

    let ours_tip = match refs::branch_tip(&vault, &current)? {
        // Current branch is unborn — adopt theirs wholesale.
        None => {
            refs::set_branch_tip(&vault, &current, &theirs_tip)?;
            restore_working_tree(&vault, &db, &config, &theirs_tip)?;
            let short: String = theirs_tip.chars().take(crate::db::SHORT_HASH_LEN).collect();
            println!("Fast-forwarded {current} to {short} (was empty).");
            return Ok(());
        }
        Some(t) => t,
    };

    // Refuse to merge over uncommitted changes.
    let modified = modified_files(&vault, &db, &config, Some(&ours_tip))?;
    if !modified.is_empty() {
        bail!(
            "You have uncommitted changes to: {}.\nCommit them before merging.",
            modified.join(", ")
        );
    }

    let base = db.lca(&ours_tip, &theirs_tip)?;

    // Already up to date: theirs is an ancestor of ours.
    if base.as_deref() == Some(theirs_tip.as_str()) {
        println!("Already up to date.");
        return Ok(());
    }

    // Fast-forward: ours is an ancestor of theirs, so just advance the ref.
    if base.as_deref() == Some(ours_tip.as_str()) {
        refs::set_branch_tip(&vault, &current, &theirs_tip)?;
        restore_working_tree(&vault, &db, &config, &theirs_tip)?;
        let short: String = theirs_tip.chars().take(crate::db::SHORT_HASH_LEN).collect();
        println!("Fast-forwarded {current} to {short}.");
        return Ok(());
    }

    // Divergent histories: resolve file by file.
    let resolution = resolve(
        &vault,
        &db,
        &config,
        base.as_deref(),
        &ours_tip,
        &theirs_tip,
    )?;

    // Snapshot only files whose merged result differs from our tip (the first
    // parent); kept-ours files are already correct in the commit and on disk.
    let snapshots: Vec<CommitFile> = resolution.take_theirs.clone();

    let identity = crate::identity::resolve(&config)?;
    let commit = Commit {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        message: format!("Merge branch '{target}' into {current}"),
        author_name: identity.name,
        author_email: identity.email,
    };
    db.insert_commit(&commit, Some(&ours_tip), Some(&theirs_tip), &snapshots)?;
    refs::set_branch_tip(&vault, &current, &commit.id)?;

    // Update the working tree for the files we took from theirs.
    for cf in &resolution.take_theirs {
        let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
        let working = vault.working_path(&cf.file_path);
        std::fs::write(&working, bytes)
            .with_context(|| format!("could not write {}", working.display()))?;
    }

    let short: String = commit.id.chars().take(crate::db::SHORT_HASH_LEN).collect();
    println!("Merged branch '{target}' into {current}  [{short}]");
    if resolution.took.is_empty() {
        println!("  (no file changes; branches joined)");
    }
    for line in &resolution.took {
        println!("  {line}");
    }
    Ok(())
}

struct Resolution {
    /// Files whose merged result is theirs (need a snapshot + working write).
    take_theirs: Vec<CommitFile>,
    /// Human-readable per-file outcome lines.
    took: Vec<String>,
}

fn resolve(
    vault: &Vault,
    db: &Db,
    config: &Config,
    base: Option<&str>,
    ours: &str,
    theirs: &str,
) -> Result<Resolution> {
    // Consider every file known to either side.
    let mut files: BTreeSet<String> = config.tracked.files.iter().cloned().collect();
    files.extend(db.files_in_history(ours)?);
    files.extend(db.files_in_history(theirs)?);

    let mut take_theirs = Vec::new();
    let mut took = Vec::new();

    for rel in files {
        let base_cf = match base {
            Some(b) => db.file_at(b, &rel)?,
            None => None,
        };
        let ours_cf = db.file_at(ours, &rel)?;
        let theirs_cf = db.file_at(theirs, &rel)?;

        let base_h = base_cf.as_ref().map(|c| &c.blob_hash);
        let ours_h = ours_cf.as_ref().map(|c| &c.blob_hash);
        let theirs_h = theirs_cf.as_ref().map(|c| &c.blob_hash);

        if ours_h == theirs_h {
            // Identical on both sides (or absent on both) — nothing to do.
            continue;
        }
        if theirs_h == base_h {
            // Only our side changed it — keep ours.
            continue;
        }
        if ours_h == base_h {
            // Only their side changed it — take theirs.
            if let Some(cf) = theirs_cf {
                took.push(format!("{rel}  →  taking theirs"));
                take_theirs.push(cf);
            }
            continue;
        }

        // Both sides changed it → conflict.
        match prompt_conflict(vault, db, &rel, ours_cf.as_ref(), theirs_cf.as_ref())? {
            Choice::Ours => took.push(format!("{rel}  →  conflict, kept ours")),
            Choice::Theirs => {
                if let Some(cf) = theirs_cf {
                    took.push(format!("{rel}  →  conflict, took theirs"));
                    take_theirs.push(cf);
                }
            }
        }
    }

    Ok(Resolution { take_theirs, took })
}

enum Choice {
    Ours,
    Theirs,
}

fn prompt_conflict(
    vault: &Vault,
    db: &Db,
    rel: &str,
    ours: Option<&CommitFile>,
    theirs: Option<&CommitFile>,
) -> Result<Choice> {
    println!("\nConflict in {rel} — changed on both branches.");
    loop {
        print!("  Keep [o]urs, take [t]heirs, or show [d]iff? ");
        std::io::stdout().flush().ok();

        let mut answer = String::new();
        let read = std::io::stdin()
            .read_line(&mut answer)
            .context("could not read choice")?;
        if read == 0 {
            bail!(
                "merge aborted: {rel} conflicts and needs a choice — run in an interactive terminal"
            );
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "o" | "ours" => return Ok(Choice::Ours),
            "t" | "theirs" => return Ok(Choice::Theirs),
            "d" | "diff" => {
                let ours_bytes = read_side(vault, db, ours)?;
                let theirs_bytes = read_side(vault, db, theirs)?;
                diff::show_file_diff(
                    rel,
                    format!("{rel} (ours)"),
                    ours_bytes,
                    format!("{rel} (theirs)"),
                    theirs_bytes,
                )?;
            }
            other => println!("  Unrecognized choice: {other:?}. Enter o, t, or d."),
        }
    }
}

fn read_side(vault: &Vault, _db: &Db, cf: Option<&CommitFile>) -> Result<Vec<u8>> {
    match cf {
        Some(cf) => store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed),
        None => Ok(Vec::new()),
    }
}
