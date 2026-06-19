//! ASCII/Unicode commit-graph rendering for `dvault log --graph`.
//!
//! History is a DAG (`parent_id` + `second_parent_id`). The `renderdag` crate
//! (the lane-based renderer from Sapling) does the hard part — assigning lanes
//! and drawing the `│ ╮ ╯ ─` connectors — emitting exactly one graph line per
//! commit. We supply the commit ids + parents, then append each commit's label
//! (short hash, ref decorations, date, message) to its line.

use crate::db::{Db, GraphCommit, SHORT_HASH_LEN};
use crate::diff::use_color;
use crate::log::format_timestamp;
use crate::refs;
use crate::tag;
use crate::vault::Vault;
use anyhow::Result;
use renderdag::{GraphRenderer, Node, RenderConfig};
use std::collections::{HashMap, HashSet};

const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const HEAD: &str = "\x1b[1;36m"; // bold cyan

/// Ref decorations attached to a commit (branch tips, tags, HEAD).
#[derive(Default)]
struct Deco {
    head_branch: Option<String>,
    branches: Vec<String>,
    tags: Vec<String>,
}

pub fn run(vault: &Vault, db: &Db, all: bool) -> Result<()> {
    let current = refs::current_branch(vault)?;

    // Which branch tips anchor the graph: just the current branch, or all.
    let mut tips: Vec<String> = Vec::new();
    if all {
        for b in refs::list_branches(vault)? {
            if let Some(t) = refs::branch_tip(vault, &b)? {
                tips.push(t);
            }
        }
    } else if let Some(t) = refs::branch_tip(vault, &current)? {
        tips.push(t);
    }

    if tips.is_empty() {
        println!("No commits yet on branch {current}.");
        return Ok(());
    }

    // Commits reachable from the chosen tips, newest first.
    let mut reachable: HashSet<String> = HashSet::new();
    for t in &tips {
        reachable.extend(db.ancestors(t)?);
    }
    let commits: Vec<GraphCommit> = db
        .all_commits_with_parents()?
        .into_iter()
        .filter(|gc| reachable.contains(&gc.commit.id))
        .collect();

    // Render the lane glyphs — one line per commit, in order.
    let nodes: Vec<Node> = commits
        .iter()
        .map(|gc| Node::new(gc.commit.id.clone(), gc.parents.clone()))
        .collect();
    let rendered = GraphRenderer::new(RenderConfig::default()).render_to_string(&nodes);
    let glyph_lines: Vec<&str> = rendered.lines().collect();

    let decorations = build_decorations(vault, db, &current)?;
    let color = use_color();

    // For a connected history, renderdag emits one line per commit; pad
    // defensively if that ever doesn't hold.
    for (i, gc) in commits.iter().enumerate() {
        let glyphs = glyph_lines.get(i).copied().unwrap_or("*");
        let label = label_for(gc, decorations.get(&gc.commit.id), color);
        println!("{glyphs} {label}");
    }
    Ok(())
}

fn build_decorations(vault: &Vault, db: &Db, current: &str) -> Result<HashMap<String, Deco>> {
    let mut map: HashMap<String, Deco> = HashMap::new();

    for branch in refs::list_branches(vault)? {
        if let Some(tip) = refs::branch_tip(vault, &branch)? {
            let entry = map.entry(tip).or_default();
            if branch == current {
                entry.head_branch = Some(branch);
            } else {
                entry.branches.push(branch);
            }
        }
    }
    for (name, commit_id) in tag::all_tags(vault)? {
        // Only decorate tags whose commit still exists.
        if db.get_commit(&commit_id).is_ok() {
            map.entry(commit_id).or_default().tags.push(name);
        }
    }
    for deco in map.values_mut() {
        deco.branches.sort();
        deco.tags.sort();
    }
    Ok(map)
}

fn label_for(gc: &GraphCommit, deco: Option<&Deco>, color: bool) -> String {
    let short: String = gc.commit.id.chars().take(SHORT_HASH_LEN).collect();
    let when = format_timestamp(&gc.commit.created_at);
    let msg = &gc.commit.message;
    let refs = deco.map(|d| format_deco(d, color)).unwrap_or_default();

    if color {
        format!("{YELLOW}{short}{RESET}{refs} {DIM}{when}{RESET} {msg}")
    } else {
        format!("{short}{refs} {when} {msg}")
    }
}

/// Format the ` (HEAD -> main, draft, tag: approved)` decoration suffix.
fn format_deco(deco: &Deco, color: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = &deco.head_branch {
        parts.push(if color {
            format!("{HEAD}HEAD -> {b}{RESET}")
        } else {
            format!("HEAD -> {b}")
        });
    }
    for b in &deco.branches {
        parts.push(if color {
            format!("{GREEN}{b}{RESET}")
        } else {
            b.clone()
        });
    }
    for t in &deco.tags {
        parts.push(if color {
            format!("{YELLOW}tag: {t}{RESET}")
        } else {
            format!("tag: {t}")
        });
    }
    if parts.is_empty() {
        String::new()
    } else if color {
        format!(" {CYAN}({RESET}{}{CYAN}){RESET}", parts.join(", "))
    } else {
        format!(" ({})", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deco_orders_head_then_branches_then_tags() {
        let deco = Deco {
            head_branch: Some("main".into()),
            branches: vec!["draft".into()],
            tags: vec!["approved".into()],
        };
        assert_eq!(
            format_deco(&deco, false),
            " (HEAD -> main, draft, tag: approved)"
        );
    }

    #[test]
    fn no_decoration_is_empty() {
        assert_eq!(format_deco(&Deco::default(), false), "");
    }

    #[test]
    fn label_includes_short_hash_and_message() {
        let gc = GraphCommit {
            commit: crate::db::Commit {
                id: "abcdef1234567890".into(),
                created_at: "2026-06-18T10:00:00Z".into(),
                message: "Fix typo".into(),
                author_name: "Ed".into(),
                author_email: None,
            },
            parents: vec![],
        };
        let label = label_for(&gc, None, false);
        assert!(label.starts_with("abcdef1")); // 7-char short hash
        assert!(label.ends_with("Fix typo"));
    }
}
