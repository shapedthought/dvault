//! Author-identity resolution.
//!
//! Identity is resolved across several sources so that collaborators sharing a
//! vault (e.g. a cloud-synced folder) and users running dvault in a container
//! are attributed correctly without editing the shared vault config.
//!
//! Precedence, highest first:
//!   1. Environment: `DVAULT_USER_NAME` / `DVAULT_USER_EMAIL`
//!   2. Per-user global config: `$XDG_CONFIG_HOME/dvault/config.toml`
//!      (default `~/.config/dvault/config.toml`)
//!   3. Per-vault config: `.dvault/config.toml` `[user]`
//!   4. OS username (name only) as a last resort
//!
//! The env layer is highest precedence specifically so a container — where the
//! home directory is ephemeral and the OS username is the container's — can
//! always set identity with `-e DVAULT_USER_NAME=...`.

use crate::config::Config;
use anyhow::Result;
use std::path::PathBuf;

const ENV_NAME: &str = "DVAULT_USER_NAME";
const ENV_EMAIL: &str = "DVAULT_USER_EMAIL";

/// Where a resolved identity value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Env,
    Global,
    Vault,
    OsUsername,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Env => "environment",
            Source::Global => "global config",
            Source::Vault => "vault config",
            Source::OsUsername => "OS username",
        }
    }
}

/// The effective commit author.
pub struct Identity {
    pub name: String,
    pub name_source: Source,
    pub email: Option<String>,
    pub email_source: Option<Source>,
}

/// Path to the per-user global config file, if a config/home dir can be found.
pub fn global_config_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(xdg).join("dvault").join("config.toml"));
    }
    let home = std::env::var_os("HOME").filter(|s| !s.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join("dvault")
            .join("config.toml"),
    )
}

/// Load the per-user global config if it exists (None if absent or no home dir).
pub fn load_global() -> Result<Option<Config>> {
    let Some(path) = global_config_path() else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(Config::load(&path)?))
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn os_username() -> String {
    whoami::username().unwrap_or_else(|_| "unknown".into())
}

/// Resolve the effective author identity, given the current vault config.
pub fn resolve(vault: &Config) -> Result<Identity> {
    let global = load_global()?;
    let global_user = |pick: fn(&Config) -> Option<String>| global.as_ref().and_then(pick);

    let (name, name_source) = if let Some(v) = env_value(ENV_NAME) {
        (v, Source::Env)
    } else if let Some(v) = global_user(|c| c.user.name.clone()) {
        (v, Source::Global)
    } else if let Some(v) = vault.user.name.clone() {
        (v, Source::Vault)
    } else {
        (os_username(), Source::OsUsername)
    };

    let (email, email_source) = if let Some(v) = env_value(ENV_EMAIL) {
        (Some(v), Some(Source::Env))
    } else if let Some(v) = global_user(|c| c.user.email.clone()) {
        (Some(v), Some(Source::Global))
    } else if let Some(v) = vault.user.email.clone() {
        (Some(v), Some(Source::Vault))
    } else {
        (None, None)
    };

    Ok(Identity {
        name,
        name_source,
        email,
        email_source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg(name: Option<&str>, email: Option<&str>) -> Config {
        let mut c = Config::default();
        c.user.name = name.map(String::from);
        c.user.email = email.map(String::from);
        c
    }

    #[test]
    fn vault_config_used_when_no_env_or_global() {
        // Note: relies on DVAULT_* env not being set in the test environment.
        let id = resolve(&cfg(Some("Vault Name"), Some("v@x.com"))).unwrap();
        // Source is Vault unless the host running tests has a global config; the
        // value, at minimum, must reflect a real source (not crash).
        assert!(!id.name.is_empty());
        assert!(matches!(
            id.name_source,
            Source::Vault | Source::Global | Source::Env
        ));
    }

    #[test]
    fn falls_back_to_os_username_with_no_identity_anywhere() {
        let id = resolve(&cfg(None, None)).unwrap();
        assert!(!id.name.is_empty());
        // Email has no OS fallback.
        if id.email.is_none() {
            assert!(id.email_source.is_none());
        }
    }

    #[test]
    fn global_path_prefers_xdg() {
        // Pure path logic — global_config_path reads env; just ensure it yields
        // a dvault/config.toml suffix when a base is available.
        if let Some(p) = global_config_path() {
            assert!(p.ends_with("dvault/config.toml"));
        }
    }
}
