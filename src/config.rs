//! `.dvault/config.toml` read/write: user identity and the tracked-file list.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub user: User,
    #[serde(default)]
    pub tracked: Tracked,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct User {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Tracked {
    #[serde(default)]
    pub files: Vec<String>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("could not read config: {}", path.display()))?;
        toml::from_str(&text).context("config.toml is malformed")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).context("could not serialize config")?;
        std::fs::write(path, text)
            .with_context(|| format!("could not write config: {}", path.display()))
    }

    /// Resolve the commit author name: configured value, else OS username.
    pub fn author_name(&self) -> String {
        self.user
            .name
            .clone()
            .unwrap_or_else(|| whoami::username().unwrap_or_else(|_| "unknown".into()))
    }

    pub fn author_email(&self) -> Option<String> {
        self.user.email.clone()
    }

    pub fn is_tracked(&self, rel: &str) -> bool {
        self.tracked.files.iter().any(|f| f == rel)
    }
}
