//! `dvault tag` — name a commit, list tags, or delete a tag. Tags are
//! plain-text files under `.dvault/refs/tags/<name>` containing the target
//! commit's id.

use crate::db::{Db, SHORT_HASH_LEN};
use crate::log::format_timestamp;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;

/// Tag names are restricted to a filesystem- and shell-safe character set.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Invalid tag name: {name}. Use letters, digits, hyphens and underscores.");
    }
    Ok(())
}

pub fn run(name: Option<String>, commit: Option<String>, delete: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    match name {
        None => {
            if delete {
                bail!("Specify a tag to delete: dvault tag -d <name>");
            }
            list_tags(&vault, &db)
        }
        Some(name) if delete => delete_tag(&vault, &name),
        Some(name) => create_tag(&vault, &db, &name, commit),
    }
}

fn create_tag(vault: &Vault, db: &Db, name: &str, commit: Option<String>) -> Result<()> {
    validate_name(name)?;

    // Resolve the target commit (an explicit ref/tag, or the current branch tip).
    let full_id = match commit {
        Some(reference) => crate::revparse::resolve(vault, db, &reference)?,
        None => crate::refs::head_tip(vault)?.context("No commits yet — nothing to tag.")?,
    };

    let tags_dir = vault.tags_dir();
    std::fs::create_dir_all(&tags_dir).context("could not create tags directory")?;
    let tag_path = tags_dir.join(name);
    if tag_path.exists() {
        bail!("Tag already exists: {name}");
    }
    std::fs::write(&tag_path, &full_id).context("could not write tag")?;

    let short: String = full_id.chars().take(SHORT_HASH_LEN).collect();
    println!("Tagged {short} as '{name}'");
    Ok(())
}

fn delete_tag(vault: &Vault, name: &str) -> Result<()> {
    let path = vault.tags_dir().join(name);
    if !path.is_file() {
        bail!("No such tag: {name}");
    }
    std::fs::remove_file(&path).with_context(|| format!("could not delete tag {name}"))?;
    println!("Deleted tag {name}");
    Ok(())
}

fn list_tags(vault: &Vault, db: &Db) -> Result<()> {
    let mut tags = all_tags(vault)?;
    if tags.is_empty() {
        println!("No tags yet. Create one with 'dvault tag <name>'.");
        return Ok(());
    }
    tags.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, commit_id) in tags {
        let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
        // Annotate with the commit's date + message when it still exists.
        match db.get_commit(&commit_id) {
            Ok(c) => println!(
                "{name:<20} {short}  {}  {}",
                format_timestamp(&c.created_at),
                c.message
            ),
            Err(_) => println!("{name:<20} {short}  (dangling)"),
        }
    }
    Ok(())
}

/// All tags as `(name, commit_id)` pairs.
pub fn all_tags(vault: &Vault) -> Result<Vec<(String, String)>> {
    let mut tags = Vec::new();
    let tags_dir = vault.tags_dir();
    if !tags_dir.is_dir() {
        return Ok(tags);
    }
    for entry in std::fs::read_dir(&tags_dir).context("could not read tags directory")? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let commit_id = std::fs::read_to_string(entry.path())?.trim().to_string();
        tags.push((name, commit_id));
    }
    Ok(tags)
}

/// The commit id a tag points at, if the tag exists.
pub fn tag_commit(vault: &Vault, name: &str) -> Result<Option<String>> {
    let path = vault.tags_dir().join(name);
    if !path.is_file() {
        return Ok(None);
    }
    let id = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read tag {name}"))?
        .trim()
        .to_string();
    Ok(if id.is_empty() { None } else { Some(id) })
}

/// Load all tags as a map of commit-id → list of tag names (for `log --tags`).
pub fn load_tags_by_commit(vault: &Vault) -> Result<HashMap<String, Vec<String>>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (name, commit_id) in all_tags(vault)? {
        map.entry(commit_id).or_default().push(name);
    }
    for names in map.values_mut() {
        names.sort();
    }
    Ok(map)
}
