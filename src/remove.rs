//! `dvault remove` — stop tracking a file. History and blobs are preserved.

use crate::config::Config;
use crate::vault::Vault;
use anyhow::{Result, bail};

pub fn run(file: String) -> Result<()> {
    let vault = Vault::discover()?;
    let config_path = vault.config_path();
    let mut config = Config::load(&config_path)?;

    let rel = vault.relativize(&file)?;
    if !config.is_tracked(&rel) {
        bail!("Not a tracked file: {file}");
    }

    config.tracked.files.retain(|f| f != &rel);
    config.save(&config_path)?;

    println!("Stopped tracking {rel} (history preserved)");
    Ok(())
}
