//! `dvault add` — start tracking one or more files. Does not commit.

use crate::config::Config;
use crate::extract::{SUPPORTED, extension, is_supported};
use crate::vault::Vault;
use anyhow::{Result, bail};

pub fn run(files: Vec<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let config_path = vault.config_path();
    let mut config = Config::load(&config_path)?;

    for input in &files {
        let rel = vault.relativize(input)?;
        let working = vault.working_path(&rel);

        if !working.is_file() {
            bail!("File not found: {input}");
        }
        if !is_supported(&rel) {
            let ext = extension(&rel).unwrap_or_default();
            bail!(
                "Unsupported file type: .{ext}. Supported types: {}",
                SUPPORTED.join(", ")
            );
        }
        if config.is_tracked(&rel) {
            println!("Already tracking {rel}");
            continue;
        }

        config.tracked.files.push(rel.clone());
        println!("Tracking {rel}");
    }

    config.save(&config_path)?;
    Ok(())
}
