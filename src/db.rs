//! SQLite access layer (`.dvault/db.sqlite`): commits, file-to-blob mappings,
//! lineage, and short-hash resolution.
//!
//! History is a DAG. Each commit has up to two parents: `parent_id` (the
//! first/primary parent — the branch you were on) and `second_parent_id` (the
//! branch merged in, NULL for ordinary commits). Branch-aware lookups walk this
//! DAG with a recursive CTE rather than ordering globally by time.

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;
use std::path::Path;

/// Length of a short hash, matching `git`'s default abbreviation.
pub const SHORT_HASH_LEN: usize = 7;

/// Recursive CTE yielding every commit reachable from `?1` (inclusive), by
/// following both parent links. Callers append a `SELECT ... ` that joins `anc`.
const ANCESTORS_CTE: &str = r#"
WITH RECURSIVE anc(id) AS (
    SELECT ?1
    UNION
    SELECT c.parent_id        FROM commits c JOIN anc a ON c.id = a.id WHERE c.parent_id IS NOT NULL
    UNION
    SELECT c.second_parent_id FROM commits c JOIN anc a ON c.id = a.id WHERE c.second_parent_id IS NOT NULL
)
"#;

pub struct Db {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub id: String,
    pub created_at: String,
    pub message: String,
    pub author_name: String,
    pub author_email: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommitFile {
    pub file_path: String,
    pub blob_hash: String,
    pub file_size: i64,
    pub compressed: bool,
}

/// A commit paired with its parent ids (for DAG/graph rendering).
#[derive(Debug, Clone)]
pub struct GraphCommit {
    pub commit: Commit,
    pub parents: Vec<String>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)
            .with_context(|| format!("could not open database: {}", path.display()))?;
        let db = Db { conn };
        db.init_schema()?;
        db.migrate()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS commits (
                id               TEXT PRIMARY KEY,
                parent_id        TEXT REFERENCES commits(id),
                second_parent_id TEXT REFERENCES commits(id),
                created_at       TEXT NOT NULL,
                message          TEXT NOT NULL,
                author_name      TEXT NOT NULL,
                author_email     TEXT
            );

            CREATE TABLE IF NOT EXISTS commit_files (
                commit_id  TEXT NOT NULL REFERENCES commits(id),
                file_path  TEXT NOT NULL,
                blob_hash  TEXT NOT NULL,
                file_size  INTEGER NOT NULL,
                compressed BOOLEAN NOT NULL DEFAULT 0,
                PRIMARY KEY (commit_id, file_path)
            );

            CREATE INDEX IF NOT EXISTS idx_commit_files_path
                ON commit_files(file_path);
            "#,
        )?;
        Ok(())
    }

    /// Bring older vaults up to the current schema/lineage model.
    fn migrate(&self) -> Result<()> {
        // 1. Add second_parent_id to vaults created before merge support.
        let has_second = self
            .conn
            .prepare("SELECT 1 FROM pragma_table_info('commits') WHERE name = 'second_parent_id'")?
            .exists([])?;
        if !has_second {
            self.conn
                .execute("ALTER TABLE commits ADD COLUMN second_parent_id TEXT", [])?;
        }

        // 2. Legacy vaults stored linear history with parent_id always NULL, so
        //    every commit is an island. Re-link them into a chain by time so the
        //    DAG walks work. Only NULL parents that have an earlier commit are
        //    linked; the true root stays NULL. Idempotent once linked.
        let chain: Vec<(String, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, parent_id FROM commits ORDER BY created_at ASC, id ASC")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?
        };
        for i in 1..chain.len() {
            let (id, parent) = &chain[i];
            if parent.is_none() {
                let prev = &chain[i - 1].0;
                self.conn.execute(
                    "UPDATE commits SET parent_id = ?1 WHERE id = ?2 AND parent_id IS NULL",
                    params![prev, id],
                )?;
            }
        }
        Ok(())
    }

    /// Insert a commit with up to two parents plus its file snapshots, in one
    /// transaction.
    pub fn insert_commit(
        &mut self,
        commit: &Commit,
        parent_id: Option<&str>,
        second_parent_id: Option<&str>,
        files: &[CommitFile],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO commits
               (id, parent_id, second_parent_id, created_at, message, author_name, author_email)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                commit.id,
                parent_id,
                second_parent_id,
                commit.created_at,
                commit.message,
                commit.author_name,
                commit.author_email,
            ],
        )?;
        for f in files {
            tx.execute(
                "INSERT INTO commit_files (commit_id, file_path, blob_hash, file_size, compressed)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    commit.id,
                    f.file_path,
                    f.blob_hash,
                    f.file_size,
                    f.compressed
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn map_commit(row: &rusqlite::Row) -> rusqlite::Result<Commit> {
        Ok(Commit {
            id: row.get(0)?,
            created_at: row.get(1)?,
            message: row.get(2)?,
            author_name: row.get(3)?,
            author_email: row.get(4)?,
        })
    }

    fn map_commit_file(row: &rusqlite::Row) -> rusqlite::Result<CommitFile> {
        Ok(CommitFile {
            file_path: row.get(0)?,
            blob_hash: row.get(1)?,
            file_size: row.get(2)?,
            compressed: row.get(3)?,
        })
    }

    /// Commits reachable from `tip`, newest first. If `file` is given, only
    /// those that snapshot that path. Used by `log`.
    pub fn reachable_commits(&self, tip: &str, file: Option<&str>) -> Result<Vec<Commit>> {
        let mut commits = Vec::new();
        match file {
            Some(path) => {
                let sql = format!(
                    "{ANCESTORS_CTE}
                     SELECT c.id, c.created_at, c.message, c.author_name, c.author_email
                     FROM commits c
                     JOIN anc ON anc.id = c.id
                     JOIN commit_files f ON f.commit_id = c.id
                     WHERE f.file_path = ?2
                     ORDER BY c.created_at DESC, c.id DESC"
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let rows = stmt.query_map(params![tip, path], Self::map_commit)?;
                for r in rows {
                    commits.push(r?);
                }
            }
            None => {
                let sql = format!(
                    "{ANCESTORS_CTE}
                     SELECT c.id, c.created_at, c.message, c.author_name, c.author_email
                     FROM commits c
                     JOIN anc ON anc.id = c.id
                     ORDER BY c.created_at DESC, c.id DESC"
                );
                let mut stmt = self.conn.prepare(&sql)?;
                let rows = stmt.query_map(params![tip], Self::map_commit)?;
                for r in rows {
                    commits.push(r?);
                }
            }
        }
        Ok(commits)
    }

    /// All commit ids reachable from `tip` (inclusive).
    pub fn ancestors(&self, tip: &str) -> Result<HashSet<String>> {
        let sql = format!("{ANCESTORS_CTE} SELECT id FROM anc");
        let mut stmt = self.conn.prepare(&sql)?;
        let ids = stmt
            .query_map(params![tip], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(ids)
    }

    /// Ancestor ids of `tip`, newest first (for lowest-common-ancestor search).
    fn ancestors_ordered(&self, tip: &str) -> Result<Vec<String>> {
        let sql = format!(
            "{ANCESTORS_CTE}
             SELECT c.id FROM commits c JOIN anc ON anc.id = c.id
             ORDER BY c.created_at DESC, c.id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let ids = stmt
            .query_map(params![tip], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    }

    /// The version of `file` as seen from `tip`: the most recent commit
    /// reachable from `tip` that snapshots it. None if never committed on this
    /// line of history.
    pub fn file_at(&self, tip: &str, file: &str) -> Result<Option<CommitFile>> {
        let sql = format!(
            "{ANCESTORS_CTE}
             SELECT f.file_path, f.blob_hash, f.file_size, f.compressed
             FROM commit_files f
             JOIN commits c ON c.id = f.commit_id
             JOIN anc ON anc.id = c.id
             WHERE f.file_path = ?2
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT 1"
        );
        let row = self
            .conn
            .query_row(&sql, params![tip, file], Self::map_commit_file)
            .optional()?;
        Ok(row)
    }

    /// Like [`file_at`](Self::file_at), but also returns the commit that
    /// snapshotted the file (for display in `status`).
    pub fn file_at_commit(&self, tip: &str, file: &str) -> Result<Option<(Commit, CommitFile)>> {
        let sql = format!(
            "{ANCESTORS_CTE}
             SELECT c.id, c.created_at, c.message, c.author_name, c.author_email,
                    f.file_path, f.blob_hash, f.file_size, f.compressed
             FROM commit_files f
             JOIN commits c ON c.id = f.commit_id
             JOIN anc ON anc.id = c.id
             WHERE f.file_path = ?2
             ORDER BY c.created_at DESC, c.id DESC
             LIMIT 1"
        );
        let row = self
            .conn
            .query_row(&sql, params![tip, file], |row| {
                let commit = Commit {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    message: row.get(2)?,
                    author_name: row.get(3)?,
                    author_email: row.get(4)?,
                };
                let file = CommitFile {
                    file_path: row.get(5)?,
                    blob_hash: row.get(6)?,
                    file_size: row.get(7)?,
                    compressed: row.get(8)?,
                };
                Ok((commit, file))
            })
            .optional()?;
        Ok(row)
    }

    /// Distinct file paths that appear anywhere in `tip`'s history.
    pub fn files_in_history(&self, tip: &str) -> Result<Vec<String>> {
        let sql = format!(
            "{ANCESTORS_CTE}
             SELECT DISTINCT f.file_path
             FROM commit_files f JOIN anc ON anc.id = f.commit_id
             ORDER BY f.file_path"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let paths = stmt
            .query_map(params![tip], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(paths)
    }

    /// Lowest common ancestor of two commits: the most recent commit reachable
    /// from both. None if they share no history.
    pub fn lca(&self, a: &str, b: &str) -> Result<Option<String>> {
        let set_a = self.ancestors(a)?;
        for id in self.ancestors_ordered(b)? {
            if set_a.contains(&id) {
                return Ok(Some(id));
            }
        }
        Ok(None)
    }

    /// Resolve a full or short commit hash to a unique commit id.
    pub fn resolve_commit(&self, prefix: &str) -> Result<String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM commits WHERE id LIKE ?1 || '%' ORDER BY id")?;
        let ids: Vec<String> = stmt
            .query_map(params![prefix], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<_>>()?;

        match ids.len() {
            0 => bail!("Unknown commit: {prefix}. Run 'dvault log' to see valid commits."),
            1 => Ok(ids.into_iter().next().unwrap()),
            _ => {
                let shown: Vec<String> = ids
                    .iter()
                    .map(|id| id.chars().take(SHORT_HASH_LEN).collect())
                    .collect();
                bail!(
                    "Ambiguous commit hash: {prefix}. Matches: {}. Use a longer prefix.",
                    shown.join(", ")
                );
            }
        }
    }

    pub fn get_commit(&self, id: &str) -> Result<Commit> {
        self.conn
            .query_row(
                "SELECT id, created_at, message, author_name, author_email
                 FROM commits WHERE id = ?1",
                params![id],
                Self::map_commit,
            )
            .optional()?
            .with_context(|| format!("commit not found: {id}"))
    }

    /// Every commit with its parent ids, newest first — for graph rendering.
    /// (Callers filter to the reachable set they want to display.)
    pub fn all_commits_with_parents(&self) -> Result<Vec<GraphCommit>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, created_at, message, author_name, author_email, parent_id, second_parent_id
             FROM commits ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let commit = Commit {
                id: row.get(0)?,
                created_at: row.get(1)?,
                message: row.get(2)?,
                author_name: row.get(3)?,
                author_email: row.get(4)?,
            };
            let mut parents = Vec::new();
            if let Some(p) = row.get::<_, Option<String>>(5)? {
                parents.push(p);
            }
            if let Some(p) = row.get::<_, Option<String>>(6)? {
                parents.push(p);
            }
            Ok(GraphCommit { commit, parents })
        })?;
        rows.collect::<rusqlite::Result<_>>().map_err(Into::into)
    }

    /// The snapshot of `file` recorded in commit `commit_id`, if any.
    pub fn get_commit_file(&self, commit_id: &str, file: &str) -> Result<Option<CommitFile>> {
        let row = self
            .conn
            .query_row(
                "SELECT file_path, blob_hash, file_size, compressed
                 FROM commit_files WHERE commit_id = ?1 AND file_path = ?2",
                params![commit_id, file],
                Self::map_commit_file,
            )
            .optional()?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn memory_db() -> Db {
        let db = Db {
            conn: Connection::open_in_memory().unwrap(),
        };
        db.init_schema().unwrap();
        db
    }

    /// Insert a commit `id` at time `t` with parents and an optional file blob.
    fn commit(
        db: &mut Db,
        id: &str,
        t: &str,
        parent: Option<&str>,
        second: Option<&str>,
        file: Option<(&str, &str)>, // (path, blob_hash)
    ) {
        let c = Commit {
            id: id.into(),
            created_at: t.into(),
            message: "m".into(),
            author_name: "a".into(),
            author_email: None,
        };
        let files: Vec<CommitFile> = file
            .into_iter()
            .map(|(p, h)| CommitFile {
                file_path: p.into(),
                blob_hash: h.into(),
                file_size: 1,
                compressed: false,
            })
            .collect();
        db.insert_commit(&c, parent, second, &files).unwrap();
    }

    // Diamond: c0 -> c1 (ours), c0 -> c2 (theirs), merge c3 = c1 + c2.
    fn diamond() -> Db {
        let mut db = memory_db();
        commit(
            &mut db,
            "c0",
            "2024-01-01T00:00:00Z",
            None,
            None,
            Some(("f", "h0")),
        );
        commit(
            &mut db,
            "c1",
            "2024-01-02T00:00:00Z",
            Some("c0"),
            None,
            Some(("f", "h1")),
        );
        commit(
            &mut db,
            "c2",
            "2024-01-03T00:00:00Z",
            Some("c0"),
            None,
            None,
        );
        commit(
            &mut db,
            "c3",
            "2024-01-04T00:00:00Z",
            Some("c1"),
            Some("c2"),
            None,
        );
        db
    }

    #[test]
    fn ancestors_follow_both_parents() {
        let db = diamond();
        let anc = db.ancestors("c3").unwrap();
        assert_eq!(
            anc,
            ["c0", "c1", "c2", "c3"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
        assert_eq!(
            db.ancestors("c1").unwrap(),
            ["c0", "c1"].iter().map(|s| s.to_string()).collect()
        );
    }

    #[test]
    fn lca_of_diverged_branches_is_fork_point() {
        let db = diamond();
        assert_eq!(db.lca("c1", "c2").unwrap().as_deref(), Some("c0"));
        // Linear: lca of an ancestor and descendant is the ancestor.
        assert_eq!(db.lca("c0", "c1").unwrap().as_deref(), Some("c0"));
    }

    #[test]
    fn file_at_picks_most_recent_on_the_line() {
        let db = diamond();
        // From c1, f was last set at c1 (h1); from c2, only c0 set it (h0).
        assert_eq!(db.file_at("c1", "f").unwrap().unwrap().blob_hash, "h1");
        assert_eq!(db.file_at("c2", "f").unwrap().unwrap().blob_hash, "h0");
        assert!(db.file_at("c0", "missing").unwrap().is_none());
    }

    #[test]
    fn migrate_links_legacy_null_parents() {
        let db = memory_db();
        // Simulate a pre-branching vault: commits with NULL parents.
        for (i, t) in ["2024-01-01", "2024-01-02", "2024-01-03"]
            .iter()
            .enumerate()
        {
            db.conn
                .execute(
                    "INSERT INTO commits (id, created_at, message, author_name) VALUES (?1, ?2, 'm', 'a')",
                    params![format!("c{i}"), format!("{t}T00:00:00Z")],
                )
                .unwrap();
        }
        db.migrate().unwrap();
        // The three islands should now form a chain reachable from the tip.
        assert_eq!(db.ancestors("c2").unwrap().len(), 3);
    }
}
