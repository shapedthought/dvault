//! `dvault config` — get or set identity, in the per-vault config or the
//! per-user global config.
//!
//!   dvault config user.name "Ed Howard"            set in the vault config
//!   dvault config --global user.name "Ed Howard"   set in the global config
//!   dvault config user.name                        report the *effective* value
//!   dvault config --global user.name               read the global config value

use crate::config::Config;
use crate::identity;
use crate::vault::Vault;
use anyhow::{Context, Result, bail};

pub fn run(key: String, value: Option<String>, global: bool) -> Result<()> {
    if global {
        run_global(&key, value)
    } else {
        run_vault(&key, value)
    }
}

fn run_vault(key: &str, value: Option<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let config_path = vault.config_path();
    let mut config = Config::load(&config_path)?;

    match value {
        Some(val) => {
            set_key(&mut config, key, &val)?;
            config.save(&config_path)?;
            println!("Set {key} = {val}  (vault config)");
        }
        // A bare get reports who you'd actually commit as, and from where.
        None => report_effective(&config, key)?,
    }
    Ok(())
}

fn run_global(key: &str, value: Option<String>) -> Result<()> {
    let path = identity::global_config_path().context(
        "could not determine a config/home directory; \
         set DVAULT_USER_NAME / DVAULT_USER_EMAIL via the environment instead",
    )?;

    let mut config = if path.is_file() {
        Config::load(&path)?
    } else {
        Config::default()
    };

    match value {
        Some(val) => {
            set_key(&mut config, key, &val)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("could not create {}", parent.display()))?;
            }
            config.save(&path)?;
            println!("Set {key} = {val}  (global config: {})", path.display());
        }
        None => match get_key(&config, key)? {
            Some(v) => println!("{v}"),
            None => println!("(unset)"),
        },
    }
    Ok(())
}

fn report_effective(vault_config: &Config, key: &str) -> Result<()> {
    let id = identity::resolve(vault_config)?;
    match key {
        "user.name" => println!("{}  (from {})", id.name, id.name_source.label()),
        "user.email" => match (id.email, id.email_source) {
            (Some(email), Some(src)) => println!("{email}  (from {})", src.label()),
            _ => println!("(unset)"),
        },
        other => bail!("Unknown config key: {other}. Known keys: user.name, user.email"),
    }
    Ok(())
}

fn set_key(config: &mut Config, key: &str, val: &str) -> Result<()> {
    match key {
        "user.name" => config.user.name = Some(val.to_string()),
        "user.email" => config.user.email = Some(val.to_string()),
        other => bail!("Unknown config key: {other}. Known keys: user.name, user.email"),
    }
    Ok(())
}

fn get_key(config: &Config, key: &str) -> Result<Option<String>> {
    Ok(match key {
        "user.name" => config.user.name.clone(),
        "user.email" => config.user.email.clone(),
        other => bail!("Unknown config key: {other}. Known keys: user.name, user.email"),
    })
}
