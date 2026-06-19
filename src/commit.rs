//! `dvault commit` — snapshot tracked files that have changed since last commit.

use crate::config::Config;
use crate::db::{Commit, CommitFile, Db};
use crate::refs;
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

/// Human-readable byte size, e.g. "42 KB".
fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

pub fn run(message: String, force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let mut db = Db::open(&vault.db_path())?;
    let objects = vault.objects_dir();

    let identity = crate::identity::resolve(&config)?;

    // Advisory lock check: refuse if someone else holds the lock, unless forced.
    if let Some(lock) = crate::lock::read(&vault)?
        && lock.holder != identity.name
        && !force
    {
        anyhow::bail!(
            "Vault is locked {}.\nCommit anyway with --force, or coordinate (see 'dvault status').",
            lock.describe()
        );
    }

    if config.tracked.files.is_empty() {
        println!("No files are tracked. Use 'dvault add <file>' first.");
        return Ok(());
    }

    let branch = refs::current_branch(&vault)?;
    let parent = refs::branch_tip(&vault, &branch)?;

    let mut snapshots: Vec<CommitFile> = Vec::new();
    // Buffered report lines so nothing prints if the commit ends up empty.
    let mut report: Vec<String> = Vec::new();

    for rel in &config.tracked.files {
        let working = vault.working_path(rel);
        if !working.is_file() {
            report.push(format!("  {rel}  →  missing from working copy, skipped"));
            continue;
        }
        let bytes = std::fs::read(&working)
            .with_context(|| format!("could not read {}", working.display()))?;
        let hash = store::hash_bytes(&bytes);

        // Skip unchanged files: hash matches this branch's last snapshot.
        if let Some(tip) = &parent
            && let Some(last) = db.file_at(tip, rel)?
            && last.blob_hash == hash
        {
            report.push(format!("  {rel}  →  no changes, skipped"));
            continue;
        }

        // `hash` and the stored blob hash are the same content hash.
        let (blob_hash, compressed) = store::write_blob(&objects, &bytes)?;
        debug_assert_eq!(blob_hash, hash);
        report.push(format!(
            "  {rel}  →  snapshotted ({})",
            human_size(bytes.len() as u64)
        ));
        snapshots.push(CommitFile {
            file_path: rel.clone(),
            blob_hash,
            file_size: bytes.len() as i64,
            compressed,
        });
    }

    if snapshots.is_empty() {
        println!("Nothing to commit. All tracked files are unchanged.");
        return Ok(());
    }

    let commit = Commit {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        message: message.clone(),
        author_name: identity.name,
        author_email: identity.email,
    };
    db.insert_commit(&commit, parent.as_deref(), None, &snapshots)?;
    refs::set_branch_tip(&vault, &branch, &commit.id)?;

    let short: String = commit.id.chars().take(crate::db::SHORT_HASH_LEN).collect();
    println!("[{branch} {short}] {message}");
    for line in report {
        println!("{line}");
    }
    Ok(())
}
