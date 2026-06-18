//! Resolve a user-supplied commit reference to a full commit id.
//!
//! A reference may be a tag name or a (possibly abbreviated) commit hash. Tags
//! take precedence: if a tag with that exact name exists, it wins even if the
//! string also looks like a hash prefix — so a tag always resolves to its
//! target.

use crate::db::Db;
use crate::tag;
use crate::vault::Vault;
use anyhow::Result;

pub fn resolve(vault: &Vault, db: &Db, reference: &str) -> Result<String> {
    if let Some(id) = tag::tag_commit(vault, reference)? {
        // Validate the tag still points at a real commit; otherwise fall
        // through to hash resolution so the error mentions the hash.
        if db.get_commit(&id).is_ok() {
            return Ok(id);
        }
    }
    db.resolve_commit(reference)
}
