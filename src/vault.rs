//! Vault discovery and path helpers.
//!
//! A vault is a `.dvault/` directory. We locate it by walking up the directory
//! tree from the current working directory, mirroring how Git finds `.git/`.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub const VAULT_DIR: &str = ".dvault";

/// A located, existing vault.
pub struct Vault {
    /// The directory containing `.dvault/` (the working root).
    pub root: PathBuf,
    /// The `.dvault/` directory itself.
    pub dir: PathBuf,
}

impl Vault {
    /// Walk up from `start` until a `.dvault/` directory is found.
    pub fn discover_from(start: &Path) -> Result<Vault> {
        let mut cur = Some(start);
        while let Some(dir) = cur {
            let candidate = dir.join(VAULT_DIR);
            if candidate.is_dir() {
                return Ok(Vault {
                    root: dir.to_path_buf(),
                    dir: candidate,
                });
            }
            cur = dir.parent();
        }
        bail!("Not a dvault repository. Run 'dvault init' first.");
    }

    /// Discover a vault starting from the current working directory.
    pub fn discover() -> Result<Vault> {
        let cwd = std::env::current_dir().context("could not read current directory")?;
        Self::discover_from(&cwd)
    }

    pub fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    pub fn db_path(&self) -> PathBuf {
        self.dir.join("db.sqlite")
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.dir.join("objects")
    }

    pub fn tags_dir(&self) -> PathBuf {
        self.dir.join("refs").join("tags")
    }

    /// Resolve a tracked-file path (stored relative to `root`) to an absolute
    /// working-copy path.
    pub fn working_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Convert a user-supplied path (relative to CWD, or absolute) into the
    /// canonical form stored in config: a path relative to the vault root,
    /// using forward slashes. The file need not exist.
    pub fn relativize(&self, input: &str) -> Result<String> {
        let cwd = std::env::current_dir().context("could not read current directory")?;
        let abs = if Path::new(input).is_absolute() {
            PathBuf::from(input)
        } else {
            cwd.join(input)
        };
        let abs = normalize(&abs);
        let rel = abs.strip_prefix(&self.root).map_err(|_| {
            anyhow::anyhow!("{} is outside the vault at {}", input, self.root.display())
        })?;
        Ok(rel.to_string_lossy().replace('\\', "/"))
    }
}

/// Lexically normalise a path (resolve `.` and `..`) without touching the
/// filesystem, so it works for files that don't exist yet.
fn normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}
