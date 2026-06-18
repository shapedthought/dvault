//! `dvault config` — get or set per-vault configuration.
//!
//!   dvault config user.name "Ed Howard"   set
//!   dvault config user.name                get

use crate::config::Config;
use crate::vault::Vault;
use anyhow::{Result, bail};

pub fn run(key: String, value: Option<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let config_path = vault.config_path();
    let mut config = Config::load(&config_path)?;

    match value {
        // Set
        Some(val) => {
            match key.as_str() {
                "user.name" => config.user.name = Some(val.clone()),
                "user.email" => config.user.email = Some(val.clone()),
                other => bail!("Unknown config key: {other}. Known keys: user.name, user.email"),
            }
            config.save(&config_path)?;
            println!("Set {key} = {val}");
        }
        // Get
        None => {
            let current = match key.as_str() {
                "user.name" => config.user.name.clone(),
                "user.email" => config.user.email.clone(),
                other => bail!("Unknown config key: {other}. Known keys: user.name, user.email"),
            };
            match current {
                Some(v) => println!("{v}"),
                None => println!("(unset)"),
            }
        }
    }
    Ok(())
}
