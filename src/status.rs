//! `dvault status` — tracked files and whether each is modified.

use crate::config::Config;
use crate::db::{Db, SHORT_HASH_LEN};
use crate::log::format_timestamp;
use crate::refs;
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result};

pub fn run() -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let db = Db::open(&vault.db_path())?;

    let branch = refs::current_branch(&vault)?;
    let tip = refs::branch_tip(&vault, &branch)?;
    println!("On branch {branch}");

    if config.tracked.files.is_empty() {
        println!("No files are tracked. Use 'dvault add <file>' to start.");
        return Ok(());
    }

    println!("Tracked files:");
    for rel in &config.tracked.files {
        let working = vault.working_path(rel);
        let last = match &tip {
            Some(t) => db.file_at_commit(t, rel)?,
            None => None,
        };

        let state = if !working.is_file() {
            "missing   (file not found in working copy)".to_string()
        } else {
            let bytes = std::fs::read(&working)
                .with_context(|| format!("could not read {}", working.display()))?;
            let hash = store::hash_bytes(&bytes);
            match &last {
                None => "new file   (staged, not yet committed)".to_string(),
                Some((commit, cf)) => {
                    let short: String = commit.id.chars().take(SHORT_HASH_LEN).collect();
                    let when = format_timestamp(&commit.created_at);
                    let label = if cf.blob_hash == hash {
                        "unchanged"
                    } else {
                        "modified "
                    };
                    format!("{label}  (last committed: {short}, {when})")
                }
            }
        };
        println!("  {rel:<28} {state}");
    }
    Ok(())
}
