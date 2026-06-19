//! `dvault stats` — word counts and growth over time.
//!
//!   dvault stats <file>   word count at each revision of that file
//!   dvault stats          a one-line summary per tracked file

use crate::config::Config;
use crate::db::{Db, SHORT_HASH_LEN};
use crate::extract::extract_text;
use crate::log::format_timestamp;
use crate::refs;
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result};

pub fn run(file: Option<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;
    let branch = refs::current_branch(&vault)?;
    let tip = refs::branch_tip(&vault, &branch)?;

    match file {
        Some(f) => stats_for_file(&vault, &db, &branch, tip.as_deref(), &f),
        None => stats_all(&vault, &db, tip.as_deref()),
    }
}

/// Word count of a file's readable text, ignoring `[Section]` banner lines.
fn word_count(rel: &str, bytes: &[u8]) -> Result<usize> {
    let lines = extract_text(rel, bytes)?;
    Ok(lines
        .iter()
        .filter(|l| !is_banner(l))
        .flat_map(|l| l.split_whitespace())
        .count())
}

fn is_banner(line: &str) -> bool {
    line.starts_with('[')
        && line.ends_with(']')
        && line.len() > 2
        && line[1..line.len() - 1]
            .chars()
            .all(|c| c.is_ascii_alphabetic())
}

/// Insert thousands separators: 1850 -> "1,850".
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn stats_for_file(
    vault: &Vault,
    db: &Db,
    branch: &str,
    tip: Option<&str>,
    file: &str,
) -> Result<()> {
    let rel = vault.relativize(file)?;
    let Some(tip) = tip else {
        println!("No commits yet on branch {branch}.");
        return Ok(());
    };

    // Oldest-first, only commits that snapshot this file.
    let mut commits = db.reachable_commits(tip, Some(&rel))?;
    commits.reverse();
    if commits.is_empty() {
        println!("No history for {rel} on branch {branch}.");
        return Ok(());
    }

    println!("Word count for {rel} (on {branch}):\n");
    let mut prev: Option<usize> = None;
    let (mut first, mut last) = (0usize, 0usize);
    for (i, c) in commits.iter().enumerate() {
        let cf = db
            .get_commit_file(&c.id, &rel)?
            .context("snapshot vanished mid-read")?;
        let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
        let words = word_count(&rel, &bytes)?;
        if i == 0 {
            first = words;
        }
        last = words;

        let short: String = c.id.chars().take(SHORT_HASH_LEN).collect();
        let when = format_timestamp(&c.created_at);
        let delta = match prev {
            Some(p) if words >= p => format!("  (+{})", thousands(words - p)),
            Some(p) => format!("  (-{})", thousands(p - words)),
            None => String::new(),
        };
        println!("  {short}  {when}  {:>7} words{delta}", thousands(words));
        prev = Some(words);
    }

    let n = commits.len();
    let revs = if n == 1 { "revision" } else { "revisions" };
    if first == last {
        println!("\n{} words across {n} {revs}.", thousands(last));
    } else {
        println!(
            "\nGrew from {} to {} words across {n} {revs}.",
            thousands(first),
            thousands(last)
        );
    }
    Ok(())
}

fn stats_all(vault: &Vault, db: &Db, tip: Option<&str>) -> Result<()> {
    let config = Config::load(&vault.config_path())?;
    if config.tracked.files.is_empty() {
        println!("No files are tracked.");
        return Ok(());
    }

    println!("Tracked files:");
    for rel in &config.tracked.files {
        let (words, revs) = match tip {
            Some(t) => match db.file_at(t, rel)? {
                Some(cf) => {
                    let bytes =
                        store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
                    let n = db.reachable_commits(t, Some(rel))?.len();
                    (Some(word_count(rel, &bytes)?), n)
                }
                None => (None, 0),
            },
            None => (None, 0),
        };
        match words {
            Some(w) => {
                let r = if revs == 1 { "revision" } else { "revisions" };
                println!("  {rel:<28} {:>7} words   {revs} {r}", thousands(w));
            }
            None => println!("  {rel:<28} (not yet committed)"),
        }
    }
    Ok(())
}
