//! `dvault cat` — print the extracted readable text of a document version to
//! stdout (no file written), for quick inspection or piping.
//!
//!   dvault cat <file>            the working copy
//!   dvault cat <commit> <file>   a committed/tagged version

use crate::db::Db;
use crate::extract::extract_text;
use crate::revparse;
use crate::vault::Vault;
use anyhow::Result;

pub fn run(args: Vec<String>) -> Result<()> {
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let (rel, bytes) = revparse::file_bytes(&vault, &db, &args)?;
    for line in extract_text(&rel, &bytes)? {
        println!("{line}");
    }
    Ok(())
}
