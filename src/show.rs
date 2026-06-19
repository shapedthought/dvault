//! `dvault show` — a commit's metadata and the files it changed (git-show
//! style). With `--diff`, also shows the readable diff of each changed file
//! against the commit's first parent.
//!
//!   dvault show              the current branch tip
//!   dvault show <commit>     a specific commit (hash or tag)
//!   dvault show <commit> --diff

use crate::db::{Db, SHORT_HASH_LEN};
use crate::diff::{show_file_diff, use_color};
use crate::log::format_timestamp;
use crate::store;
use crate::vault::Vault;
use crate::{graph, refs, revparse};
use anyhow::{Context, Result};

const YELLOW: &str = "\x1b[33m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

fn human_size(bytes: i64) -> String {
    let b = bytes.max(0) as u64;
    if b >= 1024 * 1024 {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{} KB", b / 1024)
    } else {
        format!("{b} B")
    }
}

pub fn run(reference: Option<String>, show_diff: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let commit_id = match reference {
        Some(r) => revparse::resolve(&vault, &db, &r)?,
        None => refs::head_tip(&vault)?.context("No commits yet on this branch.")?,
    };

    let commit = db.get_commit(&commit_id)?;
    let parents = db.parents_of(&commit_id)?;
    let files = db.commit_files(&commit_id)?;
    let color = use_color();
    let deco = graph::decoration_for(&vault, &db, &commit_id, color)?;

    // Header — full hash, then author/date/message.
    if color {
        println!("{YELLOW}commit {commit_id}{RESET}{deco}");
    } else {
        println!("commit {commit_id}{deco}");
    }
    if parents.len() >= 2 {
        let shorts: Vec<String> = parents
            .iter()
            .map(|p| p.chars().take(SHORT_HASH_LEN).collect())
            .collect();
        println!("Merge:  {}", shorts.join(" "));
    }
    match &commit.author_email {
        Some(email) => println!("Author: {} <{email}>", commit.author_name),
        None => println!("Author: {}", commit.author_name),
    }
    println!("Date:   {}", format_timestamp(&commit.created_at));
    println!("\n    {}\n", commit.message);

    // Files changed in this commit.
    if files.is_empty() {
        println!("No file changes.");
    } else if color {
        println!("{BOLD}Files changed:{RESET}");
    } else {
        println!("Files changed:");
    }
    for cf in &files {
        println!("  {:<28} {}", cf.file_path, human_size(cf.file_size));
    }

    if show_diff {
        let parent = parents.first().map(String::as_str);
        for cf in &files {
            println!();
            show_file_change(&vault, &db, parent, cf)?;
        }
    }
    Ok(())
}

/// Readable diff of one changed file against the (first) parent's version.
fn show_file_change(
    vault: &Vault,
    db: &Db,
    parent: Option<&str>,
    cf: &crate::db::CommitFile,
) -> Result<()> {
    let new_bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
    let (old_bytes, old_label) = match parent.and_then(|p| db.file_at(p, &cf.file_path).ok()?) {
        Some(pcf) => (
            store::read_blob(&vault.objects_dir(), &pcf.blob_hash, pcf.compressed)?,
            format!("{} (parent)", cf.file_path),
        ),
        None => (Vec::new(), format!("{} (new file)", cf.file_path)),
    };
    show_file_diff(
        &cf.file_path,
        old_label,
        old_bytes,
        format!("{} (this commit)", cf.file_path),
        new_bytes,
    )
}
