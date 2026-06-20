//! `dvault handoff` / `dvault receive` — lightweight document handoff for people
//! who don't share a synced vault (external/non-technical collaborators).
//!
//! `handoff` writes a small JSON "slip" recording which committed version the
//! document is based on, who it's going to, and a content hash. You email the
//! `.docx` (a normal file they open in Word) plus the slip. When it comes back,
//! `receive` drops the edited file into place and commits it **attributed to
//! the recipient** — they never need dvault. The slip also lets `receive`
//! detect if the document moved on locally while it was out (divergence).

use crate::config::Config;
use crate::db::{Commit, CommitFile, Db, SHORT_HASH_LEN};
use crate::log::format_timestamp;
use crate::store;
use crate::vault::Vault;
use crate::{identity, refs};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handoff {
    /// Vault-relative path of the document.
    pub file: String,
    /// Commit the handed-off document is based on.
    pub base_commit: String,
    /// Content hash of the document at handoff (its blob hash).
    pub base_hash: String,
    pub handed_to: String,
    pub handed_by: String,
    /// Handoff time, RFC 3339.
    pub at: String,
}

impl Handoff {
    pub fn describe(&self) -> String {
        format!(
            "out for edit with {} since {}",
            self.handed_to,
            format_timestamp(&self.at)
        )
    }
}

/// Internal record path for an outstanding handoff of `rel`.
fn record_path(vault: &Vault, rel: &str) -> PathBuf {
    vault
        .handoffs_dir()
        .join(format!("{}.json", rel.replace(['/', '\\'], "_")))
}

/// Read the outstanding handoff for `rel`, if any.
pub fn outstanding(vault: &Vault, rel: &str) -> Result<Option<Handoff>> {
    let path = record_path(vault, rel);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).context("could not read handoff record")?;
    Ok(Some(
        serde_json::from_str(&text).context("handoff record is malformed")?,
    ))
}

pub fn run_handoff(file: String, to: Option<String>, cancel: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let db = Db::open(&vault.db_path())?;
    let rel = vault.relativize(&file)?;

    if cancel {
        let path = record_path(&vault, &rel);
        if path.is_file() {
            std::fs::remove_file(&path).context("could not clear handoff")?;
            println!("Cancelled the handoff of {rel}.");
        } else {
            println!("{rel} is not currently handed off.");
        }
        return Ok(());
    }

    let to = to.context("specify who you're handing it to: --to \"Name\"")?;
    if !config.is_tracked(&rel) {
        bail!("Not a tracked file: {file}. Run 'dvault add {file}' first.");
    }
    if outstanding(&vault, &rel)?.is_some() {
        bail!("{rel} is already handed off. Use 'dvault handoff {rel} --cancel' or receive it.");
    }

    // Anchor to the committed version, and require a clean working copy so the
    // file we send matches a known commit.
    let branch = refs::current_branch(&vault)?;
    let tip =
        refs::branch_tip(&vault, &branch)?.context("commit the document before handing it off")?;
    let cf = db
        .file_at(&tip, &rel)?
        .with_context(|| format!("{rel} hasn't been committed yet; commit it first"))?;

    let working = vault.working_path(&rel);
    if !working.is_file() {
        bail!("File not found: {file}");
    }
    let working_hash = store::hash_bytes(&std::fs::read(&working)?);
    if working_hash != cf.blob_hash {
        bail!("{rel} has uncommitted changes; commit them before handing it off.");
    }

    let record = Handoff {
        file: rel.clone(),
        base_commit: tip,
        base_hash: cf.blob_hash,
        handed_to: to.clone(),
        handed_by: identity::resolve(&config)?.name,
        at: Utc::now().to_rfc3339(),
    };

    // Track it internally (so `status` shows it) and write a portable slip.
    std::fs::create_dir_all(vault.handoffs_dir()).context("could not create handoffs dir")?;
    let json = serde_json::to_string_pretty(&record)?;
    std::fs::write(record_path(&vault, &rel), &json).context("could not write handoff record")?;

    let slip = slip_path(&working, &rel);
    std::fs::write(&slip, &json).context("could not write handoff slip")?;

    let slip_name = slip.file_name().unwrap().to_string_lossy();
    println!("Handed off {rel} to {to}.");
    println!("Send both files: {rel} and {slip_name}");
    Ok(())
}

pub fn run_receive(slip: String, file: String, force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let mut db = Db::open(&vault.db_path())?;

    let text = std::fs::read_to_string(&slip)
        .with_context(|| format!("could not read handoff slip: {slip}"))?;
    let record: Handoff = serde_json::from_str(&text).context("handoff slip is malformed")?;

    // The slip must reference a commit we know.
    db.get_commit(&record.base_commit).with_context(|| {
        format!(
            "this slip references an unknown commit ({})",
            short(&record.base_commit)
        )
    })?;

    let branch = refs::current_branch(&vault)?;
    let tip = refs::branch_tip(&vault, &branch)?.context("no commits on this branch")?;

    // Divergence: did the document change here since it was handed off?
    if let Some(current) = db.file_at(&tip, &record.file)?
        && current.blob_hash != record.base_hash
        && !force
    {
        bail!(
            "{} has changed locally since it was handed off (it was based on {}).\n\
             The returned edits are based on the older version. Reconcile manually, or re-run\n\
             with --force to commit the returned file on top of the current version.",
            record.file,
            short(&record.base_commit)
        );
    }

    let bytes = std::fs::read(&file).with_context(|| format!("could not read {file}"))?;

    if store::hash_bytes(&bytes) == record.base_hash {
        clear_record(&vault, &record.file)?;
        println!(
            "{} came back unchanged — {} made no edits. Cleared the handoff.",
            record.file, record.handed_to
        );
        return Ok(());
    }

    // Write into the working tree and commit it, authored by the recipient.
    let working = vault.working_path(&record.file);
    std::fs::write(&working, &bytes)
        .with_context(|| format!("could not write {}", working.display()))?;

    let (blob_hash, compressed) = store::write_blob(&vault.objects_dir(), &bytes)?;
    let snapshot = CommitFile {
        file_path: record.file.clone(),
        blob_hash,
        file_size: bytes.len() as i64,
        compressed,
    };
    let commit = Commit {
        id: Uuid::new_v4().to_string(),
        created_at: Utc::now().to_rfc3339(),
        message: format!("Edits from {} (handoff)", record.handed_to),
        author_name: record.handed_to.clone(),
        author_email: None,
    };
    db.insert_commit(&commit, Some(&tip), None, &[snapshot])?;
    refs::set_branch_tip(&vault, &branch, &commit.id)?;
    clear_record(&vault, &record.file)?;

    println!(
        "Received {} from {} — committed as {} (authored by {}).",
        record.file,
        record.handed_to,
        short(&commit.id),
        record.handed_to
    );
    Ok(())
}

fn clear_record(vault: &Vault, rel: &str) -> Result<()> {
    let path = record_path(vault, rel);
    if path.is_file() {
        std::fs::remove_file(path).context("could not clear handoff record")?;
    }
    Ok(())
}

/// `report.docx` next to the working file -> `report.handoff.json` beside it.
fn slip_path(working: &Path, rel: &str) -> PathBuf {
    let stem = Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document");
    let name = format!("{stem}.handoff.json");
    match working.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

fn short(id: &str) -> String {
    id.chars().take(SHORT_HASH_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Handoff {
        Handoff {
            file: "sub/report.docx".into(),
            base_commit: "abcdef1234".into(),
            base_hash: "deadbeef".into(),
            handed_to: "Bob".into(),
            handed_by: "Alice".into(),
            at: "2026-06-20T10:00:00Z".into(),
        }
    }

    #[test]
    fn slip_roundtrips_through_json() {
        let json = serde_json::to_string_pretty(&sample()).unwrap();
        let back: Handoff = serde_json::from_str(&json).unwrap();
        assert_eq!(back.file, "sub/report.docx");
        assert_eq!(back.handed_to, "Bob");
        assert_eq!(back.base_commit, "abcdef1234");
    }

    #[test]
    fn describe_names_recipient() {
        assert!(sample().describe().contains("with Bob"));
    }

    #[test]
    fn slip_filename_is_beside_the_document() {
        let p = slip_path(Path::new("/work/sub/report.docx"), "sub/report.docx");
        assert_eq!(p, PathBuf::from("/work/sub/report.handoff.json"));
    }
}
