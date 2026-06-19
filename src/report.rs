//! `dvault report` — render a diff as a standalone HTML or Markdown file that
//! can be shared with someone who doesn't have dvault.
//!
//!   dvault report <file>                     working copy vs last commit
//!   dvault report <from> <to> <file>         between two commits/tags
//!   dvault report ... --format md --out x.md
//!
//! HTML is the default and the richest output: deletions/additions are tinted
//! and the specific changed words are highlighted inline. Markdown emits a
//! GitHub-style fenced ```diff block, which renders with red/green almost
//! everywhere.

use crate::db::Db;
use crate::diff;
use crate::extract::{can_diff, extract_text};
use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use similar::{ChangeTag, TextDiff};
use std::path::{Path, PathBuf};

enum Format {
    Html,
    Markdown,
}

pub fn run(args: Vec<String>, format: String, out: Option<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let (old, new, file_rel) = diff::resolve_sides(&vault, &db, &args)?;
    if !can_diff(&file_rel) {
        bail!("reports are only available for .docx files");
    }
    let fmt = parse_format(&format)?;

    let old_text = diff::joined_text(&extract_text(&file_rel, &old.bytes)?);
    let new_text = diff::joined_text(&extract_text(&file_rel, &new.bytes)?);

    let content = match fmt {
        Format::Html => render_html(&file_rel, &old.label, &new.label, &old_text, &new_text),
        Format::Markdown => {
            render_markdown(&file_rel, &old.label, &new.label, &old_text, &new_text)
        }
    };

    let out_path = match out {
        Some(p) => PathBuf::from(p),
        None => default_out(&file_rel, &fmt),
    };
    std::fs::write(&out_path, content)
        .with_context(|| format!("could not write {}", out_path.display()))?;
    println!("Wrote report to {}", out_path.display());
    Ok(())
}

fn parse_format(format: &str) -> Result<Format> {
    match format.to_ascii_lowercase().as_str() {
        "html" | "htm" => Ok(Format::Html),
        "md" | "markdown" => Ok(Format::Markdown),
        other => bail!("Unknown report format: {other}. Use 'html' or 'md'."),
    }
}

fn default_out(file_rel: &str, fmt: &Format) -> PathBuf {
    let stem = Path::new(file_rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("report");
    let ext = match fmt {
        Format::Html => "html",
        Format::Markdown => "md",
    };
    PathBuf::from(format!("{stem}-diff.{ext}"))
}

/// Count line-level insertions and deletions for the report summary.
fn counts(old_text: &str, new_text: &str) -> (usize, usize) {
    let diff = TextDiff::from_lines(old_text, new_text);
    let mut ins = 0;
    let mut del = 0;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => ins += 1,
            ChangeTag::Delete => del += 1,
            ChangeTag::Equal => {}
        }
    }
    (ins, del)
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_html(file_rel: &str, old_label: &str, new_label: &str, old: &str, new: &str) -> String {
    let (ins, del) = counts(old, new);
    let diff = TextDiff::from_lines(old, new);

    let mut body = String::new();
    let mut first = true;
    for group in diff.grouped_ops(3) {
        if !first {
            body.push_str("    <div class=\"gap\">⋯</div>\n");
        }
        first = false;
        for op in &group {
            for change in diff.iter_inline_changes(op) {
                let (cls, sign) = match change.tag() {
                    ChangeTag::Delete => ("del", '-'),
                    ChangeTag::Insert => ("ins", '+'),
                    ChangeTag::Equal => ("ctx", ' '),
                };
                body.push_str(&format!(
                    "    <div class=\"row {cls}\"><span class=\"s\">{sign}</span>"
                ));
                for (emph, text) in change.iter_strings_lossy() {
                    let t = esc(text.trim_end_matches('\n'));
                    if emph && change.tag() != ChangeTag::Equal {
                        body.push_str(&format!("<span class=\"w\">{t}</span>"));
                    } else {
                        body.push_str(&t);
                    }
                }
                body.push_str("</div>\n");
            }
        }
    }

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Changes to {title}</title>
<style>
  body {{ font-family: -apple-system, Segoe UI, Roboto, sans-serif; max-width: 60rem; margin: 2rem auto; padding: 0 1rem; color: #1a1a1a; }}
  h1 {{ font-size: 1.4rem; }}
  .meta {{ color: #555; }}
  .meta code {{ background: #f0f0f0; padding: .1rem .3rem; border-radius: 3px; }}
  .summary {{ font-weight: 600; margin: .5rem 0 1.5rem; }}
  .summary .ins {{ color: #137333; }} .summary .del {{ color: #b3261e; }}
  .diff {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .85rem;
           border: 1px solid #ddd; border-radius: 6px; overflow: hidden; white-space: pre-wrap; }}
  .row {{ padding: .1rem .6rem; }}
  .row .s {{ display: inline-block; width: 1ch; color: #999; user-select: none; }}
  .ctx {{ color: #444; }}
  .del {{ background: #fbe9e7; }} .ins {{ background: #e6f4ea; }}
  .del .w {{ background: #f4b9b1; border-radius: 2px; }}
  .ins .w {{ background: #a8e0bb; border-radius: 2px; }}
  .gap {{ text-align: center; color: #bbb; padding: .2rem; }}
  footer {{ margin-top: 1.5rem; color: #999; font-size: .8rem; }}
</style>
</head>
<body>
<h1>Changes to {title}</h1>
<p class="meta">Comparing <code>{old}</code> &rarr; <code>{new}</code></p>
<p class="summary"><span class="ins">{ins_s}</span>, <span class="del">{del_s}</span></p>
<div class="diff">
{body}</div>
<footer>Generated by dvault</footer>
</body>
</html>
"#,
        title = esc(file_rel),
        old = esc(old_label),
        new = esc(new_label),
        ins_s = plural(ins, "insertion"),
        del_s = plural(del, "deletion"),
        body = body,
    )
}

fn render_markdown(
    file_rel: &str,
    old_label: &str,
    new_label: &str,
    old: &str,
    new: &str,
) -> String {
    let (ins, del) = counts(old, new);
    // Reuse the plain unified-diff renderer for a fenced ```diff block, which
    // renders red/green in most Markdown viewers.
    let unified = diff::render(old_label, new_label, old, new, false);
    format!(
        "# Changes to {file_rel}\n\n\
         Comparing `{old_label}` → `{new_label}`\n\n\
         **{ins_s}, {del_s}**\n\n\
         ```diff\n{unified}```\n\n\
         _Generated by dvault_\n",
        ins_s = plural(ins, "insertion"),
        del_s = plural(del, "deletion"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_parsing_and_defaults() {
        assert!(matches!(parse_format("html").unwrap(), Format::Html));
        assert!(matches!(parse_format("MD").unwrap(), Format::Markdown));
        assert!(matches!(
            parse_format("markdown").unwrap(),
            Format::Markdown
        ));
        assert!(parse_format("pdf").is_err());
        assert_eq!(
            default_out("report.docx", &Format::Html),
            PathBuf::from("report-diff.html")
        );
        assert_eq!(
            default_out("report.docx", &Format::Markdown),
            PathBuf::from("report-diff.md")
        );
    }

    #[test]
    fn counts_are_line_level() {
        // One changed paragraph = one deletion + one insertion.
        let (ins, del) = counts("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!((ins, del), (1, 1));
    }

    #[test]
    fn html_escapes_and_emphasizes_changed_word() {
        let html = render_html(
            "f.docx",
            "v1",
            "v2",
            "Revenue was 4.2M\n",
            "Revenue was 4.8M\n",
        );
        // Self-contained document with a summary.
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("1 insertion"));
        assert!(html.contains("1 deletion"));
        // Only the changed token is wrapped for emphasis.
        assert!(html.contains(r#"<span class="w">4.2M</span>"#));
        assert!(html.contains(r#"<span class="w">4.8M</span>"#));
    }

    #[test]
    fn html_escapes_special_characters() {
        let html = render_html("f.docx", "v1", "v2", "a < b & c\n", "a < b & d\n");
        assert!(html.contains("&lt;"));
        assert!(html.contains("&amp;"));
        assert!(!html.contains("a < b & c")); // raw angle/amp not present unescaped
    }

    #[test]
    fn markdown_has_fenced_diff_block() {
        let md = render_markdown("f.docx", "v1", "v2", "old line\n", "new line\n");
        assert!(md.starts_with("# Changes to f.docx"));
        assert!(md.contains("```diff"));
        assert!(md.contains("-old line"));
        assert!(md.contains("+new line"));
    }
}
