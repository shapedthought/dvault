//! `.dvault/config.toml` read/write: user identity and the tracked-file list.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "User::is_empty")]
    pub user: User,
    // Skipped when empty so the per-user global config file (which only carries
    // identity) doesn't grow a spurious `[tracked]` section.
    #[serde(default, skip_serializing_if = "Tracked::is_empty")]
    pub tracked: Tracked,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct User {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl User {
    fn is_empty(&self) -> bool {
        self.name.is_none() && self.email.is_none()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Tracked {
    #[serde(default)]
    pub files: Vec<String>,
}

impl Tracked {
    fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
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

    pub fn is_tracked(&self, rel: &str) -> bool {
        self.tracked.files.iter().any(|f| f == rel)
    }
}
