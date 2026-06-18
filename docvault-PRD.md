# PRD: `docvault` — Git-like Version Control for Office Documents

**Status:** Ready for implementation  
**Target builder:** Claude Code  
**Stack:** Rust (clap, serde, sha2, zip, rusqlite)

---

## Overview

`docvault` is a standalone CLI tool that brings Git-like version history to Word (`.docx`), Excel (`.xlsx`), PowerPoint (`.pptx`), and PDF files. It is self-contained — no Git dependency — and targets users who work with Office documents but are not developers.

The single guiding principle: **feel like Git, require no Git knowledge**.

---

## Problem

Office users have no good version control story outside of SharePoint or OneDrive. Existing tools are either Git extensions (developer-facing), GUI add-ins (Excel-only, Windows-only), or cloud-tied. There is no standalone CLI that:

- Works across Word, Excel, PowerPoint, and PDF
- Requires no Git installation or knowledge
- Produces meaningful human-readable diffs (not binary blob comparisons)
- Is a single binary with no runtime dependencies

---

## CLI Interface

All commands live under the `dvault` binary.

### Initialise a vault

```
dvault init
```

Creates a `.dvault/` directory in the current working directory. Fails with a clear message if one already exists. Writes an initial `config.toml`.

### Track a file

```
dvault add <file>
dvault add report.docx
dvault add budget.xlsx presentation.pptx
```

Marks one or more files as tracked. Does not commit — mirrors `git add`. Writes tracked file paths to `.dvault/config.toml`. Errors if the file does not exist or is not a supported type.

### Commit a snapshot

```
dvault commit -m "Q3 first draft"
dvault commit --message "Board approved version"
```

Snapshots all tracked files that have changed since the last commit. Computes a SHA-256 of the current file content and skips the file if the hash matches the last committed hash (i.e. no changes). Stores the snapshot blob in `.dvault/objects/` using content-addressed storage (first two hex chars as directory, remainder as filename — identical to Git's layout). Writes a commit record to `.dvault/db.sqlite` containing: commit ID (UUID v4), timestamp (UTC ISO 8601), message, author (from `dvault config` or OS username fallback), and a mapping of file paths to blob hashes.

Prints a short confirmation:

```
[a3f9c12] Q3 first draft
  report.docx  →  snapshotted
  budget.xlsx  →  no changes, skipped
```

### Show commit history

```
dvault log
dvault log report.docx
```

Without a filename, shows all commits across all tracked files in reverse chronological order. With a filename, shows only commits that include a snapshot of that file.

Output format (monospace, no colour required):

```
a3f9c12  2025-06-18 14:32  Q3 first draft          Jane Smith
b71d003  2025-06-17 09:11  Initial version         Jane Smith
```

### Show what changed

```
dvault diff report.docx
dvault diff a3f9c12 b71d003 report.docx
```

Without commit IDs, diffs the working file against the last committed snapshot. With two commit IDs, diffs between those two snapshots.

The diff is content-level, not binary. Implementation:

- **`.docx` / `.pptx`:** Unzip the archive, extract `word/document.xml` (for docx) or `ppt/slides/slide*.xml` (for pptx), strip XML tags, and produce a unified text diff of the readable content.
- **`.xlsx`:** Unzip the archive, parse `xl/sharedStrings.xml` and `xl/worksheets/sheet*.xml`, produce a flat text representation (`SheetName!A1: value`) and diff that.
- **`.pdf`:** Extract text using `pdf-extract` or fallback to raw byte comparison with a note that PDF text extraction may be limited.

Output is unified diff format, printed to stdout. If no meaningful text can be extracted, print a warning and show file size delta only.

### Restore a version

```
dvault checkout a3f9c12 report.docx
```

Copies the blob for `report.docx` at commit `a3f9c12` to the working directory, overwriting the current file. Prompts for confirmation before overwriting:

```
Restore report.docx to version a3f9c12 (2025-06-17 09:11 "Initial version")?
This will overwrite the current file. [y/N]
```

Bypassed with `--force`.

### Export a historic version

```
dvault export a3f9c12 report.docx
dvault export a3f9c12 report.docx --out report-initial.docx
```

Writes the snapshot to a new file without touching the working copy. Default output filename: `<original-stem>-<short-hash><ext>` (e.g. `report-a3f9c12.docx`).

### Tag a commit

```
dvault tag board-approved
dvault tag board-approved a3f9c12
```

Without a commit ID, tags the latest commit. Tags are stored in `.dvault/refs/tags/<name>` as a plain text file containing the commit ID. Tag names must be alphanumeric plus hyphens and underscores.

```
dvault log --tags
```

Shows tags inline in log output.

### Show status

```
dvault status
```

Lists all tracked files and their state:

```
Tracked files:
  report.docx     modified   (last committed: a3f9c12, 2025-06-18 14:32)
  budget.xlsx     unchanged  (last committed: a3f9c12, 2025-06-18 14:32)
  notes.docx      new file   (staged, not yet committed)
```

### Configure identity

```
dvault config user.name "Jane Smith"
dvault config user.email "ed@example.com"
```

Writes to `.dvault/config.toml`. Falls back to OS username if not set. No global config in v1 — per-vault only.

### Untrack a file

```
dvault remove report.docx
```

Removes the file from tracking. Does not delete existing snapshots or blobs (history is preserved). Prints a confirmation.

---

## Storage Layout

```
.dvault/
  config.toml          # tracked files, user identity
  db.sqlite            # commit records, file-to-blob mappings, tags
  objects/             # content-addressed blob store
    a3/
      f9c12abc...      # raw file bytes (optionally zlib-compressed)
  refs/
    tags/
      board-approved   # plain text file: commit UUID
```

### `config.toml` schema

```toml
[user]
name = "Jane Smith"
email = "ed@example.com"

[tracked]
files = [
  "report.docx",
  "budget.xlsx"
]
```

### SQLite schema

```sql
CREATE TABLE commits (
  id          TEXT PRIMARY KEY,   -- UUID v4
  created_at  TEXT NOT NULL,      -- ISO 8601 UTC
  message     TEXT NOT NULL,
  author_name TEXT NOT NULL,
  author_email TEXT
);

CREATE TABLE commit_files (
  commit_id   TEXT NOT NULL REFERENCES commits(id),
  file_path   TEXT NOT NULL,
  blob_hash   TEXT NOT NULL,      -- SHA-256 hex
  file_size   INTEGER NOT NULL,
  PRIMARY KEY (commit_id, file_path)
);
```

---

## Error Handling

All errors must print a human-readable message to stderr and exit with a non-zero code. No panics in user-facing paths.

| Situation | Message |
|---|---|
| No `.dvault/` found | `Not a dvault repository. Run 'dvault init' first.` |
| File not found | `File not found: report.docx` |
| Unsupported file type | `Unsupported file type: .csv. Supported types: docx, xlsx, pptx, pdf` |
| Commit with no changes | `Nothing to commit. All tracked files are unchanged.` |
| Unknown commit hash | `Unknown commit: xyz1234. Run 'dvault log' to see valid commits.` |
| Ambiguous short hash | `Ambiguous commit hash: ab1. Matches: ab1f3c2, ab1a009. Use a longer prefix.` |

Short hashes (first 7 chars) are accepted everywhere a commit ID is expected. The tool resolves them from the database.

---

## Supported File Types

| Extension | Diff strategy |
|---|---|
| `.docx` | Extract `word/document.xml`, strip tags, unified text diff |
| `.xlsx` | Parse shared strings + sheet XML, flat cell representation, unified text diff |
| `.pptx` | Extract `ppt/slides/slide*.xml`, strip tags, unified text diff |
| `.pdf` | Best-effort text extraction; warn if extraction fails |

Any other extension is rejected at `dvault add` time with a clear error.

---

## Out of Scope for v1

- Branching or merging
- Remote/sync (no `dvault push/pull`)
- File rename tracking
- Conflict resolution
- Watch mode (auto-commit on save)
- Global config (`~/.dvaultconfig`)
- TUI
- Windows line ending normalisation

These are explicitly deferred, not forgotten. Design the storage layer so branching could be added later (commits have a `parent_id` field stubbed as `NULL` in the schema).

Add a `parent_id TEXT REFERENCES commits(id)` column to the `commits` table, always `NULL` in v1, to leave the door open.

---

## Implementation Notes for Claude Code

**Recommended crate dependencies:**

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
rusqlite = { version = "0.31", features = ["bundled"] }
sha2 = "0.10"
uuid = { version = "1", features = ["v4"] }
zip = "0.6"
quick-xml = "0.31"
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
similar = "2"          # for unified diff output
dialoguer = "0.11"     # for confirmation prompts
```

**Binary name:** `dvault`

**Module structure:**

```
src/
  main.rs          # clap entrypoint, command dispatch
  init.rs          # dvault init
  add.rs           # dvault add
  commit.rs        # dvault commit
  log.rs           # dvault log
  diff.rs          # dvault diff (+ per-format extractors)
  checkout.rs      # dvault checkout
  export.rs        # dvault export
  tag.rs           # dvault tag
  status.rs        # dvault status
  config.rs        # dvault config + config.toml read/write
  remove.rs        # dvault remove
  store.rs         # blob storage (objects/ directory)
  db.rs            # SQLite access layer
  extract/
    mod.rs
    docx.rs
    xlsx.rs
    pptx.rs
    pdf.rs
```

**Vault discovery:** Walk parent directories from CWD until a `.dvault/` directory is found, matching Git's behaviour. Return an error if none is found.

**Short hash resolution:** `SELECT id FROM commits WHERE id LIKE ?1 || '%'` — return an error if zero or more than one row matches.

**Blob storage:** Store raw file bytes. Apply `zlib` compression if the file is over 100 KB. Store a flag in `commit_files` (`compressed BOOLEAN`) so retrieval knows whether to decompress.

**Diff output:** Use the `similar` crate's `unified_diff` function. Target 3 lines of context (matching `git diff` default). Print to stdout so output can be piped.

**XML text extraction (docx/pptx):** Use `quick-xml` to walk the XML tree, collecting text content of `<w:t>` elements (docx) or `<a:t>` elements (pptx). Join with newlines. Strip all other tags.

**Excel extraction:** Parse `xl/sharedStrings.xml` to build a lookup table, then walk `xl/worksheets/sheet*.xml`. For each `<c>` element, resolve the cell reference and value, emit `SheetName!A1: resolved_value\n`.

---

## Example Session

```
$ dvault init
Initialized empty dvault repository in /Users/jane/docs/.dvault/

$ dvault add report.docx budget.xlsx
Tracking report.docx
Tracking budget.xlsx

$ dvault commit -m "Initial version"
[b71d003] Initial version
  report.docx  →  snapshotted (42 KB)
  budget.xlsx  →  snapshotted (18 KB)

# ... user edits report.docx ...

$ dvault status
Tracked files:
  report.docx     modified   (last committed: b71d003, 2025-06-17 09:11)
  budget.xlsx     unchanged  (last committed: b71d003, 2025-06-17 09:11)

$ dvault diff report.docx
--- report.docx (b71d003)
+++ report.docx (working copy)
@@ -1,4 +1,5 @@
 Executive Summary
-Revenue for Q3 was $4.2M.
+Revenue for Q3 was $4.8M.
+Growth was driven by EMEA expansion.

$ dvault commit -m "Updated Q3 revenue figure"
[a3f9c12] Updated Q3 revenue figure
  report.docx  →  snapshotted (43 KB)
  budget.xlsx  →  unchanged, skipped

$ dvault log
a3f9c12  2025-06-18 14:32  Updated Q3 revenue figure   Jane Smith
b71d003  2025-06-17 09:11  Initial version              Jane Smith

$ dvault tag board-approved
Tagged a3f9c12 as 'board-approved'

$ dvault export b71d003 report.docx
Exported to report-b71d003.docx
```

---

## Acceptance Criteria

- [ ] `dvault init` creates `.dvault/` with correct structure
- [ ] `dvault add` rejects unsupported file types with a clear error
- [ ] `dvault commit` skips unchanged files and reports which were skipped
- [ ] `dvault log` output is reverse chronological, filterable by file
- [ ] `dvault diff` produces readable text diff for `.docx` and `.xlsx` files
- [ ] `dvault checkout` prompts before overwriting and respects `--force`
- [ ] `dvault export` writes a copy without touching the working file
- [ ] `dvault status` correctly identifies modified vs unchanged tracked files
- [ ] Short hashes (7 chars) are accepted everywhere commit IDs are used
- [ ] No panics in any user-facing path; all errors go to stderr with a human message
- [ ] Single binary, no runtime dependencies
- [ ] Compiles and runs on macOS and Linux (Windows stretch goal)
