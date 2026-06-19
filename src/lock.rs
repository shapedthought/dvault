//! `dvault lock` / `dvault unlock` — advisory coordination for vaults shared
//! over a cloud-synced folder.
//!
//! This is **advisory, not enforced**: a synced filesystem can't provide atomic
//! locks, and the lock file itself syncs (so it can briefly conflict). It's a
//! "I've got it" convention to avoid two people committing simultaneously and
//! forking the history — not a hard guarantee. `commit` checks it (overridable
//! with `--force`); `status` shows it.

use crate::config::Config;
use crate::identity;
use crate::log::format_timestamp;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Lock {
    pub holder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub locked_at: String,
}

impl Lock {
    /// Human-readable "by X since <time>".
    pub fn describe(&self) -> String {
        format!(
            "by {} since {}",
            self.holder,
            format_timestamp(&self.locked_at)
        )
    }
}

fn lock_path(vault: &Vault) -> PathBuf {
    vault.dir.join("lock")
}

/// Read the current lock, if any.
pub fn read(vault: &Vault) -> Result<Option<Lock>> {
    let path = lock_path(vault);
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).context("could not read lock")?;
    Ok(Some(
        toml::from_str(&text).context("lock file is malformed")?,
    ))
}

/// Whether `lock` is held by the given identity (compared by name).
fn held_by(lock: &Lock, name: &str) -> bool {
    lock.holder == name
}

pub fn run_lock(force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let me = identity::resolve(&config)?.name;

    if let Some(existing) = read(&vault)? {
        if held_by(&existing, &me) {
            // Refresh the timestamp; it's already yours.
            write(&vault, &me, &config)?;
            println!("You already hold the lock (timestamp refreshed).");
            return Ok(());
        }
        if !force {
            bail!(
                "Vault is locked {}.\nCoordinate, or override with 'dvault lock --force'.",
                existing.describe()
            );
        }
        println!("Overriding lock held {}.", existing.describe());
    }

    write(&vault, &me, &config)?;
    println!("Locked by {me}.");
    Ok(())
}

pub fn run_unlock(force: bool) -> Result<()> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let me = identity::resolve(&config)?.name;

    match read(&vault)? {
        None => println!("No lock is held."),
        Some(existing) => {
            if !held_by(&existing, &me) && !force {
                bail!(
                    "Vault is locked {}.\nRemove it anyway with 'dvault unlock --force'.",
                    existing.describe()
                );
            }
            std::fs::remove_file(lock_path(&vault)).context("could not remove lock")?;
            println!("Unlocked.");
        }
    }
    Ok(())
}

fn write(vault: &Vault, name: &str, config: &Config) -> Result<()> {
    let lock = Lock {
        holder: name.to_string(),
        email: identity::resolve(config)?.email,
        locked_at: Utc::now().to_rfc3339(),
    };
    let text = toml::to_string_pretty(&lock).context("could not serialize lock")?;
    std::fs::write(lock_path(vault), text).context("could not write lock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_toml() {
        let lock = Lock {
            holder: "Ed".into(),
            email: Some("ed@x.com".into()),
            locked_at: "2026-06-19T14:00:00Z".into(),
        };
        let text = toml::to_string_pretty(&lock).unwrap();
        let back: Lock = toml::from_str(&text).unwrap();
        assert_eq!(back.holder, "Ed");
        assert_eq!(back.email.as_deref(), Some("ed@x.com"));
    }

    #[test]
    fn held_by_compares_holder_name() {
        let lock = Lock {
            holder: "Alice".into(),
            email: None,
            locked_at: "2026-06-19T14:00:00Z".into(),
        };
        assert!(held_by(&lock, "Alice"));
        assert!(!held_by(&lock, "Bob"));
    }
}
