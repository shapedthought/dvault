# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project state

The full CLI is built and working **for `.docx` only**. All commands (`init`, `add`, `remove`, `commit`, `log`, `status`, `diff`, `checkout`, `export`, `tag`, `branch`, `switch`, `merge`, `config`) function end-to-end. `docvault-PRD.md` is the original spec; a real `.docx` CV (gitignored, not committed) is used as the local test fixture.

**Branching and merging are implemented** (beyond the original PRD scope). History is a DAG; merges are whole-file (pick ours/theirs on conflict), not content blends — a deliberate choice for binary Office files.

`.xlsx`/`.pptx`/`.pdf` are **not yet supported** — `add` rejects them (they have no text extractor, so tracking them would only yield useless size-only diffs). The storage/history layers are format-agnostic, so adding a format = add `src/extract/<fmt>.rs`, then add the extension to `extract::SUPPORTED`, `extract::extract_text`, and `extract::can_diff`. Nothing else needs to change.

## What this is

`dvault` is a standalone Rust CLI that brings Git-like version history to Office documents (`.docx`, `.xlsx`, `.pptx`, `.pdf`). Guiding principle: **feel like Git, require no Git knowledge**. It is self-contained — no Git dependency, single binary, no runtime dependencies.

## Commands

```
cargo build              # debug build
cargo build --release    # release binary at target/release/dvault
cargo run -- <args>      # e.g. cargo run -- init
cargo test               # run all tests
cargo test <name>        # run a single test by name substring
cargo clippy             # lint
cargo fmt                # format
```

## Architecture

Each `dvault` subcommand maps to its own module in `src/` (`init.rs`, `add.rs`, `commit.rs`, `log.rs`, `diff.rs`, `changes.rs`, `checkout.rs`, `export.rs`, `tag.rs`, `status.rs`, `remove.rs`, `branch.rs`, `switch.rs`, `merge.rs`; the `config` command lives in `config_cmd.rs`). `main.rs` is a clap (derive) entrypoint that dispatches to them. `config.rs` (distinct from `config_cmd.rs`) is the config.toml model; `vault.rs` handles discovery and path relativization. Four shared layers underpin the commands:

- **`store.rs`** — content-addressed blob storage in `.dvault/objects/`, using Git's layout (first two hex chars of the SHA-256 as subdirectory, remainder as filename). Stores raw bytes; zlib-compresses blobs over 100 KB and records a `compressed` flag in the DB so retrieval knows whether to decompress.
- **`db.rs`** — SQLite access layer (`.dvault/db.sqlite`, via `rusqlite` bundled). `commits` (UUID v4 id, `parent_id` + `second_parent_id` for the DAG, ISO 8601 UTC timestamp, message, author) and `commit_files` (commit_id → file_path → blob_hash mapping). History is a DAG walked via a recursive-CTE (`ANCESTORS_CTE`): `ancestors`, `file_at(tip, file)`, `reachable_commits`, `lca`, `files_in_history`. `migrate()` adds `second_parent_id` to old vaults and re-links legacy NULL parents into a chain.
- **`refs.rs`** — branch refs and `HEAD` as plain files (`.dvault/HEAD` → `ref: refs/heads/<branch>`; `.dvault/refs/heads/<branch>` → tip commit id). A branch is "unborn" until its first commit (no ref file yet). `head_tip`, `branch_tip`, `set_branch_tip`, `current_branch`, `set_head`, `list_branches`, `delete_branch`. `branch -d` refuses to delete an unmerged branch (tip not reachable from another branch's tip) without `-D`/`--force`, and never deletes the current branch.
- **`extract/`** — per-format text extractors that turn binary Office files into readable text for diffing. This is the core of the product: diffs are **content-level, not binary**. Only `docx.rs` exists today; `mod.rs` holds the supported-types list, the `add`-time gate, and `can_diff`/`extract_text` dispatch.

### Vault layout (`.dvault/`)

`config.toml` (tracked files + user identity), `db.sqlite` (commit DAG, file mappings), `objects/` (blob store), `HEAD` (current branch), `refs/heads/<branch>` and `refs/tags/<name>` (plain-text files containing a commit id).

### Key cross-cutting behaviors

- **Vault discovery:** walk parent directories from CWD until `.dvault/` is found (mirrors Git). Error if none found.
- **Branch-aware "latest":** there is no global "latest commit" — `commit`/`status`/`diff`/`log` resolve the current branch via `refs`, then walk the DAG from its tip (`db.file_at`, `db.reachable_commits`). Anything that used to mean "newest by time" must go through the branch tip instead.
- **Reference resolution:** commands taking a commit (`diff`, `checkout`, `export`, `tag`) accept a tag name OR a short/full hash, via `revparse::resolve`. It tries the tag first (`tag::tag_commit`), then falls back to `db.resolve_commit` (hash prefix `LIKE ?1 || '%'`; error on zero/ambiguous matches). Tags win over hash-lookalikes. Short hashes are the first 7 chars.
- **Merge model:** `merge.rs` computes the `lca` of the two tips, then resolves whole files (only-one-side-changed → auto; both-changed → interactive ours/theirs/diff prompt). Fast-forward and up-to-date are short-circuited. A real merge writes a commit with both parents and only snapshots take-theirs files. `switch`/`merge` refuse to run over uncommitted changes (`switch::modified_files`).
- **Change detection:** `commit` and `status` compare a freshly computed SHA-256 of the working file against its `blob_hash` at the branch tip (`db.file_at`); identical hash means unchanged/skipped.
- **Diffs:** `diff.rs::render` builds a unified diff (3 lines of context) from `similar`'s `grouped_ops` + `iter_inline_changes` (the `inline` feature must stay enabled in Cargo.toml). With color on, changed lines are red/green and the genuinely changed *words within* a line get reverse video (`\x1b[7;3Xm`); context lines are unstyled. Color gating in `use_color()`: `NO_COLOR` disables, `CLICOLOR_FORCE` forces on, else TTY detection — so piped output is plain. docx extraction (`extract/docx.rs`) collects text from `<w:t>` elements via `quick-xml`, one line per `<w:p>` paragraph. It parses not just `word/document.xml` (the body, which includes text boxes) but also headers (`word/headerN.xml`), footers, footnotes, endnotes, and comments — all the same WordprocessingML, so `parse_paragraphs` handles each. Non-body regions are concatenated in a stable order, each introduced by a `[Header]`/`[Footnotes]`/… banner line via `append_section`. Separately, `docx::tracked_changes` parses Word revision marks (`<w:ins>`/`<w:del>`, deletions carry text in `<w:delText>`) into `TrackedChange { kind, author, date, text }` for the `changes` command — distinct from snapshot diffing. Two quick-xml 0.40 gotchas the code already handles: `<w:tab/>` is emitted as a tab so adjacent runs don't glue into run-on words (the "EnglishNative" bug), and XML entities arrive as separate `Event::GeneralRef` events (not inside `Event::Text`), so they're resolved explicitly. For unsupported types, `diff` warns and shows only a file-size delta.

## Conventions specific to this project

- **No panics in user-facing paths.** Every error prints a human-readable message to stderr and exits non-zero. The PRD's Error Handling table specifies exact wording for common cases (no vault, file not found, unsupported type, nothing to commit, unknown/ambiguous hash) — match those messages.
- **Reject unsupported file types at `add` time**, not at commit/diff time. The accepted set is `extract::SUPPORTED` — currently `docx` only.
- **Destructive commands prompt before overwriting.** `checkout` confirms `[y/N]` before overwriting unless `--force`. The prompt is a manual stdin read (not `dialoguer`) so it behaves under piped/non-interactive stdin — EOF or empty input defaults to "no".
- **History is append-only.** `remove` untracks a file but never deletes its snapshots or blobs.
- **Out of scope:** remotes/sync, rename tracking, watch mode, global config. Merging is whole-file, not a content blend (deliberate for binary docs).

## Dependencies & build notes

Edition 2024. clap 4 (derive), serde 1, toml 1, rusqlite 0.37 (bundled), sha2 0.11, hex, uuid 1 (v4), zip 8, quick-xml 0.40, chrono 0.4, anyhow 1, similar 3, flate2, whoami 2. These are newer than the PRD's suggested pins and several APIs differ from the PRD examples (e.g. `whoami::username()` returns `Result`; quick-xml uses `decode()` + `GeneralRef` rather than `unescape()`).

**`rusqlite` is pinned to 0.37 deliberately.** 0.38+ pulls `libsqlite3-sys` 0.38, whose build script uses the unstable `cfg_select!` feature and fails to compile on the current stable toolchain. Do not bump it without confirming `libsqlite3-sys` builds.
