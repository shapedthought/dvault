//! `dvault init` — create a `.dvault/` vault in the current directory.

use crate::config::Config;
use crate::db::Db;
use crate::refs::{self, DEFAULT_BRANCH};
use crate::vault::{VAULT_DIR, Vault};
use anyhow::{Context, Result, bail};

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir().context("could not read current directory")?;
    let vault_dir = cwd.join(VAULT_DIR);

    if vault_dir.exists() {
        bail!(
            "A dvault repository already exists at {}",
            vault_dir.display()
        );
    }

    std::fs::create_dir_all(vault_dir.join("objects"))
        .context("could not create objects directory")?;
    std::fs::create_dir_all(vault_dir.join("refs").join("tags"))
        .context("could not create refs directory")?;

    // Initial empty config and database.
    Config::default().save(&vault_dir.join("config.toml"))?;
    Db::open(&vault_dir.join("db.sqlite"))?;

    // Point HEAD at the (unborn) default branch.
    let vault = Vault {
        root: cwd.clone(),
        dir: vault_dir.clone(),
    };
    refs::set_head(&vault, DEFAULT_BRANCH)?;

    println!(
        "Initialized empty dvault repository in {}/ (on branch {DEFAULT_BRANCH})",
        vault_dir.display()
    );
    Ok(())
}
