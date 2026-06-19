# dvault

Git-like version control for Word documents — without Git, and without needing to know Git.

`dvault` is a single self-contained binary that gives you version history, snapshots, and **readable diffs** for your `.docx` files. Instead of comparing opaque binary blobs, it extracts the actual text and shows you what changed in plain language:

```diff
--- report.docx (b71d003)
+++ report.docx (working copy)
@@ -1,4 +1,4 @@
 Executive Summary
-Revenue for Q3 was $4.2M.
+Revenue for Q3 was $4.8M.
```

## Focus

`dvault` is **built for Word `.docx` documents** — that focus is deliberate, not a limitation on the way to something else. Word is where readable, content-level diffs shine: the body text lives in a structured XML part we can extract cleanly, so a one-line edit shows as a one-line change instead of an opaque binary blob.

`dvault add` only accepts `.docx`; other file types are rejected with a clear message. The storage and history layers happen to be format-agnostic, so other formats *could* be added later, but the tool is intentionally scoped to do one thing well.

## Installing

Requires a [Rust toolchain](https://rustup.rs/). From the project root:

```sh
cargo build --release
```

The binary lands at `target/release/dvault`. Copy it somewhere on your `PATH`, e.g.:

```sh
cp target/release/dvault ~/.local/bin/
```

Verify:

```sh
dvault --help
```

## Quick start

```sh
cd ~/Documents/my-project

dvault init                          # create the vault
dvault config user.name "Jane Smith"  # (optional) set your identity
dvault add report.docx               # start tracking a file
dvault commit -m "First draft"       # snapshot it

# ... edit report.docx in Word ...

dvault status                        # see that it's modified
dvault diff report.docx              # see exactly what changed
dvault commit -m "Revised Q3 figures"
dvault log                           # view history
```

## Commands

### `dvault init`
Creates a `.dvault/` vault in the current directory. Run this once per project folder.

### `dvault config <key> [value]`
Get or set your identity, stored per-vault. Without a value, prints the current setting.
```sh
dvault config user.name "Jane Smith"
dvault config user.email "jane@example.com"
dvault config user.name                  # prints the current value
```
If unset, commits fall back to your OS username.

### `dvault add <file>...`
Start tracking one or more files (like `git add`). Does **not** commit. Rejects unsupported file types (currently anything that isn't `.docx`).
```sh
dvault add report.docx
dvault add report.docx proposal.docx
```

### `dvault remove <file>`
Stop tracking a file. History and snapshots are preserved — only future commits stop including it.

### `dvault commit -m "<message>"`
Snapshots every tracked file that has changed since its last commit. Unchanged files are detected by content hash and skipped.
```sh
dvault commit -m "Board-approved version"
```
```
[a3f9c12] Board-approved version
  report.docx    →  snapshotted (43 KB)
  proposal.docx  →  no changes, skipped
```

### `dvault show [<commit>] [--diff]`
Shows a commit's metadata (hash, author, date, message, ref decorations, and `Merge:` parents for merge commits) and the files it changed. Defaults to the current branch tip. Add `--diff` to also show the readable diff of each changed file against its parent. Accepts a tag.
```sh
dvault show                    # the latest commit
dvault show approved --diff    # a tagged commit, with per-file diffs
```
```
commit a3f9c12… (HEAD -> main, tag: approved)
Author: Jane Smith <jane@example.com>
Date:   2026-06-18 14:32

    Revise revenue figure

Files changed:
  report.docx   43 KB
```

### `dvault status`
Lists tracked files and whether each is unchanged, modified, new (staged but never committed), or missing.

### `dvault log [file] [--tags] [--graph [--all]]`
Shows commit history, newest first. Pass a filename to see only commits touching that file. Pass `--tags` to show tags inline. Pass `--graph` to draw the branch/merge structure as a graph (add `--all` to include every branch, not just the current one).
```sh
dvault log
dvault log report.docx
dvault log --tags
dvault log --graph --all
```
```
a3f9c12  2026-06-18 14:32  Board-approved version   Jane Smith  (board-approved)
b71d003  2026-06-17 09:11  First draft              Jane Smith
```
With `--graph`, decorated with `HEAD ->`, branch, and tag refs:
```
⍟─╮ 9ed9830 (HEAD -> main, tag: approved) Merge branch 'draft' into main
● │ ce3c33e Update figures
│ ● 1208fc4 (draft) Polish draft
│ ● 5b13208 Rewrite intro
●─╯ 35b4b33 Fix typo
⊝ 24e684f Initial version
```

### `dvault diff <file>` / `dvault diff <from> <to> <file>`
Shows readable changes for a `.docx`. Deletions are shown in **red**, additions in **green** (like `git diff`), and within a changed paragraph the **specific changed words are highlighted** (reverse video) so a one-word edit doesn't look like the whole paragraph changed.

Diffs cover the whole document, not just the body: **headers, footers, footnotes, endnotes, and comments** are included, each introduced by a `[Header]` / `[Footnotes]` / … banner so you can see *where* a change happened. (Text boxes are part of the body and are covered automatically.)
- With just a filename: compares your **working copy** against its last commit.
- With two commit hashes: compares those two snapshots.
```sh
dvault diff report.docx
dvault diff b71d003 a3f9c12 report.docx
```
Color is applied only when writing to a terminal; piped or redirected output stays plain text. Set `NO_COLOR=1` to disable it, or `CLICOLOR_FORCE=1` to keep color when piping (e.g. `dvault diff report.docx | less -R`).

Add `--stat` for just a paragraph-level summary instead of the full diff:
```sh
dvault diff --stat report.docx
# report.docx: 3 changed, 1 added, 0 removed (paragraphs)
```

### `dvault cat <file>` / `dvault cat <commit> <file>`
Prints the extracted readable text of a document version to stdout — handy for a quick look or piping (e.g. into `grep` or `wc`).
```sh
dvault cat report.docx              # the working copy
dvault cat approved report.docx     # a committed/tagged version
```

### `dvault stats [file]`
Shows word counts and growth over time. With a file, lists the word count at each revision; without one, a one-line summary per tracked file.
```sh
dvault stats report.docx
dvault stats
```
```
Word count for report.docx (on main):

  b71d003  2026-06-17 09:11    1,200 words
  a3f9c12  2026-06-18 14:32    1,850 words  (+650)

Grew from 1,200 to 1,850 words across 2 revisions.
```

### `dvault changes <file>` / `dvault changes <commit> <file>`
Lists a document's **pending Word tracked changes** (its `<w:ins>`/`<w:del>` revision marks), with the author and date Word recorded. This is distinct from `dvault diff`: `diff` compares two *snapshots*, while `changes` surfaces the unaccepted edits already inside a single document — handy for reviewing a doc someone sent back with Track Changes on.
```sh
dvault changes contract.docx          # the working copy
dvault changes review contract.docx   # a committed/tagged version
```
```
Tracked changes in contract.docx (3):

  - deleted  John Doe, 2026-06-17 14:30
      "1 January"
  + inserted  Jane Smith, 2026-06-18 09:15
      "1 March"
  + inserted  Jane Smith, 2026-06-18 09:20
      "A new liability clause is added here."
```

### `dvault report [<from> <to>] <file> [--format html|md] [--out path]`
Writes a **standalone HTML or Markdown report** of the changes — something you can email or attach for someone who doesn't have dvault. Same version selection as `diff`. HTML (the default) is self-contained (inline CSS, no assets) and highlights the specific changed words; Markdown emits a GitHub-style fenced ` ```diff ` block.
```sh
dvault report report.docx                       # report.docx vs last commit → report-diff.html
dvault report v1 approved report.docx           # between two commits/tags
dvault report report.docx --format md --out changes.md
```

### `dvault checkout <commit> <file> [--force]`
Restores a file to a historic version, **overwriting** the working copy. Prompts for confirmation first; use `--force` to skip the prompt.
```sh
dvault checkout b71d003 report.docx
```

### `dvault export <commit> <file> [--out <path>]`
Writes a historic version to a **new** file without touching your working copy. Defaults to `<name>-<hash>.<ext>`.
```sh
dvault export b71d003 report.docx
dvault export b71d003 report.docx --out report-original.docx
```

### `dvault tag [name] [commit]` / `dvault tag -d <name>`
With no argument, lists all tags and the commit each points at. With a name, tags a commit (defaults to the current branch tip). With `-d`, deletes a tag. Tag names allow letters, digits, hyphens, and underscores.
```sh
dvault tag                       # list tags
dvault tag board-approved        # tag the latest commit
dvault tag board-approved a3f9c12   # tag a specific commit
dvault tag -d board-approved     # delete the tag
```
**A tag can be used anywhere a commit hash is accepted** — in `diff`, `checkout`, and `export`:
```sh
dvault diff board-approved latest-review report.docx
dvault checkout board-approved report.docx
dvault export board-approved report.docx
```
If a tag name happens to look like a commit hash, the tag wins. Deleting a tag only removes the label; the commit and its snapshots are untouched.

### `dvault branch [name]` / `dvault branch -d <name>`
With no argument, lists branches (the current one marked `*`). With a name, creates a new branch at the current commit. With `-d`, deletes a branch.
```sh
dvault branch                 # list
dvault branch draft-rewrite   # create
dvault branch -d draft-rewrite   # delete (refuses if it has unmerged commits)
dvault branch -D draft-rewrite   # force-delete even if unmerged
```
Deleting a branch only removes the label — committed snapshots are never deleted. You can't delete the branch you're currently on.

### `dvault switch <branch> [--force]`
Moves to another branch and updates your working files to that branch's versions. Refuses to switch if you have uncommitted changes (so they aren't lost); `--force` discards them.
```sh
dvault switch draft-rewrite
```

### `dvault merge <branch>`
Merges another branch into the current one. Because `.docx` files are binary archives, merging works **per whole file**, not by blending contents:

- If only one branch changed a file, that version is taken automatically.
- If **both** branches changed the same file, it's a conflict — you choose to keep **o**urs or take **t**heirs, and can show the readable **d**iff first.

Fast-forward and already-up-to-date cases are handled automatically; a genuine merge records a commit with two parents.
```sh
dvault switch main
dvault merge draft-rewrite
```
```
Conflict in report.docx — changed on both branches.
  Keep [o]urs, take [t]heirs, or show [d]iff? d
  ... diff shown ...
  Keep [o]urs, take [t]heirs, or show [d]iff? t
Merged branch 'draft-rewrite' into main  [a91f3c2]
  report.docx  →  conflict, took theirs
```

## Branching & merging in practice

```sh
dvault branch q3-revisions      # spin off a branch
dvault switch q3-revisions      # work on it
# ... edit report.docx, dvault commit -m "..."
dvault switch main              # back to main
dvault merge q3-revisions       # fold the work back in
```

## How it works

- **Commit hashes** are abbreviated to 7 characters in output, and any unique prefix is accepted wherever a commit is expected. **Tag names** work in those places too (and take precedence over hash prefixes).
- **Snapshots** are stored content-addressed in `.dvault/objects/` (deduplicated by SHA-256), with metadata in a local SQLite database. Files over 100 KB are compressed.
- Everything lives in the `.dvault/` directory in your project — no cloud, no external services, no Git.

## Vault layout

```
.dvault/
  config.toml          # tracked files + your identity
  db.sqlite            # commit history (a DAG) and file→snapshot mappings
  objects/             # content-addressed snapshots
  HEAD                 # the branch you're on
  refs/heads/          # one file per branch (its tip commit)
  refs/tags/           # one file per tag
```

## Scope

Intentionally **not** included: other file formats (Word `.docx` only, by design), remotes/sync (`push`/`pull`), rename tracking, and auto-commit-on-save. Merging is **whole-file** (you pick a side on conflict), not a content-level blend — an intentional choice for binary `.docx` files.

## Development

```sh
cargo test       # run the test suite
cargo clippy     # lint
cargo fmt        # format
```
