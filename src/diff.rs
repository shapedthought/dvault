//! `dvault diff` — readable changes for a file.
//!
//!   dvault diff <file>               working copy vs its last commit
//!   dvault diff <from> <to> <file>   between two commits
//!
//! For `.docx` we diff extracted readable text (unified, 3 lines of context).
//! For formats without an extractor we fall back to a size-delta summary.

use crate::db::{Db, SHORT_HASH_LEN};
use crate::extract::{can_diff, extract_text};
use crate::refs;
use crate::store;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use similar::TextDiff;

/// One side of a diff: a labelled set of bytes.
struct Side {
    label: String,
    bytes: Vec<u8>,
}

pub fn run(args: Vec<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;
    let objects = vault.objects_dir();

    let (old, new, file_rel) = match args.as_slice() {
        [file] => {
            let rel = vault.relativize(file)?;
            let tip = refs::head_tip(&vault)?
                .context("No commits yet on this branch. Nothing to diff against.")?;
            let (commit, cf) = db
                .file_at_commit(&tip, &rel)?
                .with_context(|| format!("No commits yet for {rel}. Nothing to diff against."))?;
            let old_bytes = store::read_blob(&objects, &cf.blob_hash, cf.compressed)?;
            let working = vault.working_path(&rel);
            if !working.is_file() {
                bail!("File not found: {file}");
            }
            let new_bytes = std::fs::read(&working)
                .with_context(|| format!("could not read {}", working.display()))?;
            let short: String = commit.id.chars().take(SHORT_HASH_LEN).collect();
            (
                Side {
                    label: format!("{rel} ({short})"),
                    bytes: old_bytes,
                },
                Side {
                    label: format!("{rel} (working copy)"),
                    bytes: new_bytes,
                },
                rel,
            )
        }
        [from, to, file] => {
            let rel = vault.relativize(file)?;
            let from_id = crate::revparse::resolve(&vault, &db, from)?;
            let to_id = crate::revparse::resolve(&vault, &db, to)?;
            let old = load_commit_side(&db, &objects, &from_id, &rel)?;
            let new = load_commit_side(&db, &objects, &to_id, &rel)?;
            (old, new, rel)
        }
        _ => bail!("Usage: dvault diff <file>  OR  dvault diff <from> <to> <file>"),
    };

    if can_diff(&file_rel) {
        print_text_diff(&file_rel, &old, &new)?;
    } else {
        print_size_diff(&old, &new);
    }
    Ok(())
}

/// Render a readable diff between two in-memory versions of a file (used by
/// `merge` to show a conflict). Colored when writing to a terminal.
pub fn show_file_diff(
    file_rel: &str,
    old_label: String,
    old_bytes: Vec<u8>,
    new_label: String,
    new_bytes: Vec<u8>,
) -> Result<()> {
    let old = Side {
        label: old_label,
        bytes: old_bytes,
    };
    let new = Side {
        label: new_label,
        bytes: new_bytes,
    };
    if can_diff(file_rel) {
        print_text_diff(file_rel, &old, &new)
    } else {
        print_size_diff(&old, &new);
        Ok(())
    }
}

fn load_commit_side(
    db: &Db,
    objects: &std::path::Path,
    commit_id: &str,
    rel: &str,
) -> Result<Side> {
    let cf = db.get_commit_file(commit_id, rel)?.with_context(|| {
        let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
        format!("{rel} was not snapshotted in commit {short}")
    })?;
    let bytes = store::read_blob(objects, &cf.blob_hash, cf.compressed)?;
    let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
    Ok(Side {
        label: format!("{rel} ({short})"),
        bytes,
    })
}

fn print_text_diff(file_rel: &str, old: &Side, new: &Side) -> Result<()> {
    let old_lines = extract_text(file_rel, &old.bytes)?;
    let new_lines = extract_text(file_rel, &new.bytes)?;
    let old_text = old_lines.join("\n");
    let new_text = new_lines.join("\n");

    if old_text == new_text {
        println!("No textual changes in {file_rel}.");
        return Ok(());
    }

    let diff = TextDiff::from_lines(&old_text, &new_text);
    let rendered = diff
        .unified_diff()
        .context_radius(3)
        .header(&old.label, &new.label)
        .to_string();

    if use_color() {
        print!("{}", colorize(&rendered));
    } else {
        print!("{rendered}");
    }
    Ok(())
}

// ANSI styling for diff output: red deletions, green additions, cyan hunk
// headers, bold file headers — matching the familiar `git diff` palette.
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Colorize only when stdout is an interactive terminal and the user hasn't
/// opted out via `NO_COLOR`, so piped/redirected output stays plain text.
fn use_color() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

/// Wrap each line of a unified diff in ANSI colors based on its leading marker.
fn colorize(diff: &str) -> String {
    let mut out = String::with_capacity(diff.len() + diff.len() / 8);
    for segment in diff.split_inclusive('\n') {
        let (body, newline) = match segment.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (segment, ""),
        };
        // Order matters: `+++`/`---` file headers must be checked before the
        // single-char `+`/`-` content markers.
        let color = if body.starts_with("@@") {
            CYAN
        } else if body.starts_with("+++") || body.starts_with("---") {
            BOLD
        } else if body.starts_with('+') {
            GREEN
        } else if body.starts_with('-') {
            RED
        } else {
            "" // context line: no styling
        };

        if color.is_empty() {
            out.push_str(body);
        } else {
            out.push_str(color);
            out.push_str(body);
            out.push_str(RESET);
        }
        out.push_str(newline);
    }
    out
}

fn print_size_diff(old: &Side, new: &Side) {
    eprintln!("warning: no readable text diff available for this file type; showing size only");
    println!("  {}: {}", old.label, human_size(old.bytes.len() as u64));
    println!("  {}: {}", new.label, human_size(new.bytes.len() as u64));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorize_marks_each_line_role() {
        let diff = "--- a (111)\n+++ b (222)\n@@ -1,2 +1,2 @@\n context\n-gone\n+added\n";
        let out = colorize(diff);
        assert!(out.contains(&format!("{RED}-gone{RESET}")));
        assert!(out.contains(&format!("{GREEN}+added{RESET}")));
        assert!(out.contains(&format!("{CYAN}@@ -1,2 +1,2 @@{RESET}")));
        assert!(out.contains(&format!("{BOLD}--- a (111){RESET}")));
        assert!(out.contains(&format!("{BOLD}+++ b (222){RESET}")));
        // context lines are left untouched
        assert!(out.contains(" context\n"));
    }

    #[test]
    fn colorize_preserves_text_when_stripped_of_codes() {
        let diff = "@@ -1 +1 @@\n-old\n+new\n";
        let stripped = colorize(diff)
            .replace(RED, "")
            .replace(GREEN, "")
            .replace(CYAN, "")
            .replace(BOLD, "")
            .replace(RESET, "");
        assert_eq!(stripped, diff);
    }
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}
