//! `dvault branch` — list branches, create one at the current commit, or
//! delete one.

use crate::db::Db;
use crate::refs;
use crate::vault::Vault;
use anyhow::{Result, bail};

pub fn run(name: Option<String>, delete: bool, force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    // -D (force) implies deletion, matching `git branch -D`.
    let delete = delete || force;

    match name {
        // List branches, marking the current one.
        None => {
            if delete {
                bail!("Specify a branch to delete: dvault branch -d <name>");
            }
            let current = refs::current_branch(&vault)?;
            for b in refs::list_branches(&vault)? {
                let mark = if b == current { "*" } else { " " };
                println!("{mark} {b}");
            }
        }
        Some(name) if delete => delete_branch(&vault, &name, force)?,
        // Create a new branch at the current branch tip.
        Some(name) => {
            refs::validate_branch_name(&name)?;
            if refs::branch_exists(&vault, &name) {
                bail!("Branch already exists: {name}");
            }
            let current = refs::current_branch(&vault)?;
            let tip = refs::branch_tip(&vault, &current)?.ok_or_else(|| {
                anyhow::anyhow!("Cannot branch before the first commit. Commit something first.")
            })?;
            refs::set_branch_tip(&vault, &name, &tip)?;
            let short: String = tip.chars().take(crate::db::SHORT_HASH_LEN).collect();
            println!("Created branch {name} at {short}");
            println!("Switch to it with: dvault switch {name}");
        }
    }
    Ok(())
}

fn delete_branch(vault: &Vault, name: &str, force: bool) -> Result<()> {
    if !refs::branch_exists(vault, name) {
        bail!("No such branch: {name}");
    }
    if refs::current_branch(vault)? == name {
        bail!("Cannot delete the current branch. Switch to another branch first.");
    }

    // Safety: refuse to delete a branch whose tip isn't reachable from any other
    // branch (its commits would become unreachable), unless forced.
    if !force
        && let Some(tip) = refs::branch_tip(vault, name)?
        && !is_merged(vault, name, &tip)?
    {
        bail!(
            "Branch {name} is not merged into another branch; its commits would be orphaned.\n\
             Re-run with -D / --force to delete it anyway."
        );
    }

    refs::delete_branch(vault, name)?;
    println!("Deleted branch {name}");
    Ok(())
}

/// Whether `tip` is reachable from some branch other than `name`.
fn is_merged(vault: &Vault, name: &str, tip: &str) -> Result<bool> {
    let db = Db::open(&vault.db_path())?;
    for other in refs::list_branches(vault)? {
        if other == name {
            continue;
        }
        if let Some(other_tip) = refs::branch_tip(vault, &other)?
            && db.ancestors(&other_tip)?.contains(tip)
        {
            return Ok(true);
        }
    }
    Ok(false)
}
