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
pub(crate) struct Side {
    pub label: String,
    pub bytes: Vec<u8>,
}

pub fn run(args: Vec<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let (old, new, file_rel) = resolve_sides(&vault, &db, &args)?;

    if can_diff(&file_rel) {
        print_text_diff(&file_rel, &old, &new)?;
    } else {
        print_size_diff(&old, &new);
    }
    Ok(())
}

/// Resolve `diff`-style args into the two sides being compared and the
/// vault-relative file path. Shared by `diff` and `report`.
///
///   [file]            → working copy vs the file's last commit
///   [from, to, file]  → between two commits/tags
pub(crate) fn resolve_sides(
    vault: &Vault,
    db: &Db,
    args: &[String],
) -> Result<(Side, Side, String)> {
    let objects = vault.objects_dir();

    let result = match args {
        [file] => {
            let rel = vault.relativize(file)?;
            let tip = refs::head_tip(vault)?
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
            let from_id = crate::revparse::resolve(vault, db, from)?;
            let to_id = crate::revparse::resolve(vault, db, to)?;
            let old = load_commit_side(db, &objects, &from_id, &rel)?;
            let new = load_commit_side(db, &objects, &to_id, &rel)?;
            (old, new, rel)
        }
        _ => bail!("Usage: dvault diff <file>  OR  dvault diff <from> <to> <file>"),
    };

    Ok(result)
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

    print!(
        "{}",
        render(&old.label, &new.label, &old_text, &new_text, use_color())
    );
    Ok(())
}

// ANSI styling for diff output — the familiar `git diff` palette. Changed words
// *within* a line get reverse video so they stand out from the rest of the line.
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const RED_EMPH: &str = "\x1b[7;31m"; // reverse + red
const GREEN_EMPH: &str = "\x1b[7;32m"; // reverse + green
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

/// Whether to emit ANSI color. Off when piped/redirected so output stays plain,
/// with the usual overrides: `NO_COLOR` always disables; `CLICOLOR_FORCE`
/// forces color on (handy when piping into a pager like `less -R`).
pub fn use_color() -> bool {
    use std::io::IsTerminal;
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("CLICOLOR_FORCE").is_some() {
        return true;
    }
    std::io::stdout().is_terminal()
}

/// Render a unified diff between two texts. With `color`, deleted/added lines
/// are red/green and the specific changed words are emphasized (reverse video)
/// via `similar`'s inline diffing; without it, plain unified-diff text.
pub(crate) fn render(
    old_label: &str,
    new_label: &str,
    old_text: &str,
    new_text: &str,
    color: bool,
) -> String {
    use similar::ChangeTag;

    let diff = TextDiff::from_lines(old_text, new_text);
    let mut out = String::new();

    if color {
        out.push_str(&format!("{BOLD}--- {old_label}{RESET}\n"));
        out.push_str(&format!("{BOLD}+++ {new_label}{RESET}\n"));
    } else {
        out.push_str(&format!("--- {old_label}\n+++ {new_label}\n"));
    }

    for group in diff.grouped_ops(3) {
        let first = group.first().unwrap();
        let last = group.last().unwrap();
        let (os, oe) = (first.old_range().start, last.old_range().end);
        let (ns, ne) = (first.new_range().start, last.new_range().end);
        let header = format!("@@ -{},{} +{},{} @@", os + 1, oe - os, ns + 1, ne - ns);
        if color {
            out.push_str(&format!("{CYAN}{header}{RESET}\n"));
        } else {
            out.push_str(&header);
            out.push('\n');
        }

        for op in &group {
            for change in diff.iter_inline_changes(op) {
                let (sign, base, emph) = match change.tag() {
                    ChangeTag::Delete => ('-', RED, RED_EMPH),
                    ChangeTag::Insert => ('+', GREEN, GREEN_EMPH),
                    ChangeTag::Equal => (' ', "", ""),
                };

                if !color || change.tag() == ChangeTag::Equal {
                    out.push(sign);
                    for (_emph, text) in change.iter_strings_lossy() {
                        out.push_str(text.trim_end_matches('\n'));
                    }
                } else {
                    // Colored changed line: tinted sign, then each token tinted,
                    // with the genuinely changed tokens in reverse video.
                    out.push_str(base);
                    out.push(sign);
                    out.push_str(RESET);
                    for (emphasized, text) in change.iter_strings_lossy() {
                        let token = text.trim_end_matches('\n');
                        if token.is_empty() {
                            continue;
                        }
                        let style = if emphasized { emph } else { base };
                        out.push_str(style);
                        out.push_str(token);
                        out.push_str(RESET);
                    }
                }
                out.push('\n');
            }
        }
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

    /// Strip all ANSI SGR codes so we can assert on the underlying text.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // consume up to and including the terminating 'm'
                for d in chars.by_ref() {
                    if d == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn plain_render_has_no_escape_codes_and_is_unified() {
        let out = render("a (111)", "b (222)", "one\ntwo\n", "one\nTWO\n", false);
        assert!(
            !out.contains('\x1b'),
            "plain output must have no ANSI codes"
        );
        assert!(out.contains("--- a (111)\n+++ b (222)\n"));
        assert!(out.contains("@@"));
        assert!(out.contains("-two\n"));
        assert!(out.contains("+TWO\n"));
        assert!(out.contains(" one\n")); // context line, space-prefixed
    }

    #[test]
    fn colored_render_emphasizes_only_the_changed_word() {
        // Only "4.2M" → "4.8M" changed; the rest of the line should not be
        // reverse-video emphasized.
        let old = "Revenue was 4.2M today\n";
        let new = "Revenue was 4.8M today\n";
        let out = render("a", "b", old, new, true);

        // The changed tokens are wrapped in the reverse-video emphasis codes.
        assert!(out.contains(&format!("{RED_EMPH}4.2M{RESET}")));
        assert!(out.contains(&format!("{GREEN_EMPH}4.8M{RESET}")));
        // Unchanged words on the changed line are tinted but NOT emphasized.
        assert!(!out.contains(&format!("{RED_EMPH}Revenue")));
        assert!(!out.contains(&format!("{GREEN_EMPH}today")));
        // Stripping ANSI yields a clean unified diff with both lines intact.
        let plain = strip_ansi(&out);
        assert!(plain.contains("-Revenue was 4.2M today\n"));
        assert!(plain.contains("+Revenue was 4.8M today\n"));
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
