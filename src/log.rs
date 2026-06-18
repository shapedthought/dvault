//! `dvault log` — commit history, newest first, optionally filtered by file.

use crate::db::{Db, SHORT_HASH_LEN};
use crate::refs;
use crate::tag;
use crate::vault::Vault;
use anyhow::Result;
use chrono::{DateTime, Utc};

/// Format a stored RFC 3339 timestamp as `YYYY-MM-DD HH:MM` (UTC).
pub fn format_timestamp(iso: &str) -> String {
    match DateTime::parse_from_rfc3339(iso) {
        Ok(dt) => dt.with_timezone(&Utc).format("%Y-%m-%d %H:%M").to_string(),
        Err(_) => iso.to_string(),
    }
}

pub fn run(file: Option<String>, show_tags: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    // If filtering by file, normalise to the stored relative form.
    let file_rel = match &file {
        Some(f) => Some(vault.relativize(f)?),
        None => None,
    };

    let branch = refs::current_branch(&vault)?;
    let commits = match refs::branch_tip(&vault, &branch)? {
        Some(tip) => db.reachable_commits(&tip, file_rel.as_deref())?,
        None => Vec::new(),
    };
    if commits.is_empty() {
        match &file_rel {
            Some(f) => println!("No commits found for {f} on branch {branch}."),
            None => println!("No commits yet on branch {branch}."),
        }
        return Ok(());
    }

    let tags = if show_tags {
        tag::load_tags_by_commit(&vault)?
    } else {
        Default::default()
    };

    for c in commits {
        let short: String = c.id.chars().take(SHORT_HASH_LEN).collect();
        let when = format_timestamp(&c.created_at);
        let mut line = format!("{short}  {when}  {:<28}  {}", c.message, c.author_name);
        if show_tags && let Some(names) = tags.get(&c.id) {
            line.push_str(&format!("  ({})", names.join(", ")));
        }
        println!("{line}");
    }
    Ok(())
}
