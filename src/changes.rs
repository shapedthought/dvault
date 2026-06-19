//! `dvault changes` — list a document's *pending* Word tracked changes
//! (revision marks), as opposed to the snapshot-to-snapshot diffs `dvault diff`
//! produces.
//!
//!   dvault changes <file>            inspect the working copy
//!   dvault changes <commit> <file>   inspect a committed/ tagged version

use crate::db::Db;
use crate::diff::use_color;
use crate::extract::{ChangeKind, TrackedChange, tracked_changes};
use crate::log::format_timestamp;
use crate::revparse;
use crate::vault::Vault;
use anyhow::Result;

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

pub fn run(args: Vec<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let (rel, bytes) = revparse::file_bytes(&vault, &db, &args)?;

    let changes = tracked_changes(&rel, &bytes)?;
    if changes.is_empty() {
        println!("No tracked changes in {rel}.");
        return Ok(());
    }

    let color = use_color();
    println!("Tracked changes in {rel} ({}):\n", changes.len());
    for c in &changes {
        print_change(c, color);
    }
    Ok(())
}

fn print_change(c: &TrackedChange, color: bool) {
    let (sign, verb, tint) = match c.kind {
        ChangeKind::Insertion => ('+', "inserted", GREEN),
        ChangeKind::Deletion => ('-', "deleted", RED),
    };
    let author = c.author.as_deref().unwrap_or("unknown");
    let when = c
        .date
        .as_deref()
        .map(format_timestamp)
        .unwrap_or_else(|| "no date".to_string());
    // Whitespace-collapse the text so a multi-line edit shows on one line.
    let text: String = c.text.split_whitespace().collect::<Vec<_>>().join(" ");

    if color {
        println!("  {tint}{sign} {verb}{RESET}  {DIM}{author}, {when}{RESET}");
        println!("      {tint}\"{text}\"{RESET}");
    } else {
        println!("  {sign} {verb}  {author}, {when}");
        println!("      \"{text}\"");
    }
}
