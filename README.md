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

**What you get:**

- **Readable, content-level diffs** of `.docx` — across the body *and* headers, footers, footnotes, and comments — with the specific changed words highlighted
- **Milestone history**: snapshots, `log` (with a `--graph`), `show`, word-count `stats`, and `tag`s you can use anywhere a commit is expected
- **Branching and merging** (whole-file conflict resolution — you pick a side)
- Surface Word's own **tracked changes**, and generate shareable **HTML / Markdown** diff reports
- **Multi-person collaboration** over a shared/synced folder — per-machine identity and an advisory lock, no server
- Runs as a **single binary or a Docker container** — no cloud, no Git, no install required

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

## Running with Docker

If you can't install binaries (e.g. company policy), run dvault as a container instead. It works identically to the native binary against a bind-mounted vault folder — including one that's OneDrive/SharePoint-synced.

```sh
docker build -t dvault .
```

Then add a shell alias so it feels native:

```sh
alias dvault='docker run --rm -it \
  -v "$PWD":/work -w /work \
  -u "$(id -u):$(id -g)" \
  -e DVAULT_USER_NAME -e DVAULT_USER_EMAIL \
  dvault'
```

…and set your identity once in your shell profile (this is how the container knows who you are — see the config precedence below):

```sh
export DVAULT_USER_NAME="Jane Smith"
export DVAULT_USER_EMAIL="jane@company.com"
```

Now `dvault status`, `dvault commit -m "…"`, etc. work as usual. Why those flags:

- **`-u "$(id -u):$(id -g)"`** — run as *you*, so files written into the (possibly synced) vault are owned by you, not `root`.
- **`-it`** — keeps stdin/TTY for the interactive prompts (`checkout` confirm, `merge` conflict resolution) and enables color. Without a TTY, color turns off (use `CLICOLOR_FORCE=1` to force it) and prompts safely default to "no".
- Mount the folder that **contains** `.dvault/` — vault discovery walks upward and can't climb above the mount point.

A team can mix freely: some people on the native binary, others via this image, all against the same synced vault.

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

### `dvault config <key> [value]` / `--global`
Get or set your identity. Identity is resolved with this precedence (highest first):

1. **Environment** — `DVAULT_USER_NAME` / `DVAULT_USER_EMAIL`
2. **Per-user global config** — `~/.config/dvault/config.toml` (or `$XDG_CONFIG_HOME/dvault/config.toml`)
3. **Per-vault config** — `.dvault/config.toml`
4. **OS username** (name only) as a last resort

```sh
dvault config user.name "Jane Smith"            # set in this vault
dvault config --global user.name "Jane Smith"   # set once per machine
dvault config user.name                         # prints the EFFECTIVE value and where it came from
```

This is what makes dvault usable by **multiple people on a shared/synced vault** and **inside a container**: each person sets their own identity via the environment or their global config, without ever editing the shared vault config. In a container, set `-e DVAULT_USER_NAME=… -e DVAULT_USER_EMAIL=…` (the env layer is highest precedence precisely so this works when the container's home directory and username aren't yours).

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
Shows readable changes for a `.docx`. Deletions are shown in **red**, additions in **green** (like `git diff`), and within a changed paragraph the **specific changed words are highlighted** (reverse video) so a one-word edit doesn't look like the whole paragraph changed. Each hunk is labelled with the **nearest heading**, so you know *which section* changed:
```
@@ -2,7 +2,7 @@  Q3 Results
-Revenue for Q3 was $4.2M.
+Revenue for Q3 was $4.8M.
```
(Heading detection uses Word's heading styles — `Heading 1`, `Title`, etc. Manually-bolded "headings" with no style can't be detected. This flows into `compare` and `report` too.)

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

### `dvault compare <old.docx> <new.docx>`
Readable diff between **two loose files on disk** — **no vault, no `init`, no commit**. For the "Save As" reality: when you have a pile of near-identical version files (`report_v1.docx`, `report_v2.docx`) and just want to see what changed between two of them. Same colored, inline-highlighted output as `dvault diff`.
```sh
dvault compare report_v1.docx report_v2.docx
```
(If you find yourself doing this often, consider tracking *one* file and committing milestones instead — then `dvault log` and `dvault diff` give you the whole version history.)

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
Deleting a branch only removes the label — committed snapshots are never deleted. You can't delete the branch you're currently on. `dvault branch --show-current` prints just the current branch name (for shell prompts — see [Shell prompt](#shell-prompt)).

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

### `dvault lock` / `dvault unlock [--force]`
Take or release the **advisory** lock for a shared/synced vault (see Collaboration below). While someone else holds it, `dvault commit` refuses unless you pass `--force`. `dvault status` shows the current holder.
```sh
dvault lock
dvault unlock
```

### `dvault handoff <file> --to "Name"` / `dvault receive <slip> <file>`
Hand a document to someone who **doesn't share your vault** (see Collaboration). `handoff` writes a small `*.handoff.json` slip and marks the file "out for edit"; you email them the document + the slip. `receive` takes the edited file back and commits it **attributed to the recipient** — they never need dvault.
```sh
dvault handoff report.docx --to "Bob Smith"   # email report.docx + report.handoff.json
# ... Bob edits in Word and emails the file back ...
dvault receive report.handoff.json from-bob.docx
dvault handoff report.docx --cancel           # if it never comes back
```

## Branching & merging in practice

```sh
dvault branch q3-revisions      # spin off a branch
dvault switch q3-revisions      # work on it
# ... edit report.docx, dvault commit -m "..."
dvault switch main              # back to main
dvault merge q3-revisions       # fold the work back in
```

## Shell prompt

You can show the current dvault branch in your prompt, the same way shells show the git branch. `dvault branch --show-current` prints just the branch name (and nothing — no error — outside a vault), so it drops straight into a prompt hook.

**bash** — add to `~/.bashrc`:
```bash
dvault_ps1() { local b; b=$(dvault branch --show-current 2>/dev/null); [ -n "$b" ] && printf ' (dv:%s)' "$b"; }
PS1='\w$(dvault_ps1)\$ '
```

**zsh** — add to `~/.zshrc`:
```zsh
setopt PROMPT_SUBST
dvault_ps1() { local b; b=$(dvault branch --show-current 2>/dev/null); [ -n "$b" ] && printf ' (dv:%s)' "$b"; }
PROMPT='%~$(dvault_ps1)%# '
```

**Faster / Docker-friendly alternative.** The above runs `dvault` on every prompt. If dvault is slow to start for you — especially if you run it *via the Docker alias*, where each call spawns a container — use this pure-shell version instead. It reads `.dvault/HEAD` directly (walking up from any subdirectory), with no process spawn:
```bash
dvault_ps1() {
  local d=$PWD ref
  while :; do
    if [ -r "$d/.dvault/HEAD" ]; then
      IFS= read -r ref < "$d/.dvault/HEAD"
      printf ' (dv:%s)' "${ref#ref: refs/heads/}"
      return
    fi
    [ "$d" = "/" ] && return
    d=${d%/*}; [ -z "$d" ] && d=/
  done
}
```

**[Starship](https://starship.rs/)** — add a custom module to `~/.config/starship.toml`:
```toml
[custom.dvault]
command = "dvault branch --show-current"
when = "dvault branch --show-current | grep -q ."
format = "[ dv:$output]($style) "
style = "purple"
```

## Collaboration

dvault has no server. To work with others, **put the vault folder in a shared, synced location you already trust** — typically OneDrive or SharePoint, where the documents probably already live (so there's no new place for sensitive data to go). The cloud sync becomes the transport: one person commits, it syncs, the next person picks it up.

Set your identity per machine so commits are attributed correctly without editing the shared vault config — via the environment or your global config (see `dvault config` above). A team can mix native and Docker users freely on the same vault.

**Sequential handoff** (one editor at a time) is the sweet spot and works cleanly. For the occasional case where two people might edit at once, use the advisory lock:

```sh
dvault lock       # "I've got it" — others see it in `dvault status`
# ... edit and commit ...
dvault unlock     # hand it back
```

While a lock is held by someone else, `dvault commit` refuses (override with `--force`).

**An honest caveat:** the lock is *advisory*, not enforced — a cloud-synced filesystem can't provide atomic locks. The blob store is append-only and safe under sync, but the commit database (`db.sqlite`) and branch refs are synced as whole files, so two people committing at the *exact same time* can create a "conflicted copy" and fork the history. For sequential handoff this never happens; the lock is there to coordinate the rare overlap. (A future option — storing commits as individual append-only files — would make true concurrency safe, but isn't needed for milestone workflows.)

### Handing off to someone without a shared vault

When a collaborator is external (or just doesn't have access to your synced folder), use a **handoff slip**. It travels with the document by email and makes the round trip safe and attributed — the recipient only needs Word:

```sh
dvault handoff report.docx --to "Bob Smith"
# → email report.docx and report.handoff.json to Bob
# ... Bob edits in Word, emails the file back ...
dvault receive report.handoff.json from-bob.docx
# → commits Bob's edits, authored by Bob
```

The slip records which committed version the document was based on, so `receive` will **stop you** if the document changed locally while it was out (rather than silently clobbering) — re-run with `--force` to commit the returned file on top. While a document is out, `dvault status` shows it as *out for edit*, and `dvault handoff <file> --cancel` clears it if it never comes back.

## How it works

- **Commit hashes** are abbreviated to 7 characters in output, and any unique prefix is accepted wherever a commit is expected. **Tag names** work in those places too (and take precedence over hash prefixes).
- **Snapshots** are stored content-addressed in `.dvault/objects/` (deduplicated by SHA-256), with metadata in a local SQLite database. Files over 100 KB are compressed.
- **Identity** is resolved from environment → per-user global config → vault config → OS username, so shared/synced vaults and containers attribute commits to the right person.
- Everything lives in the `.dvault/` directory in your project — no cloud, no external services, no Git.

## Vault layout

```
.dvault/
  config.toml          # tracked files + (optional) per-vault identity
  db.sqlite            # commit history (a DAG) and file→snapshot mappings
  objects/             # content-addressed snapshots
  HEAD                 # the branch you're on
  refs/heads/          # one file per branch (its tip commit)
  refs/tags/           # one file per tag
  lock                 # present only while the advisory lock is held
```

Per-user identity (when set with `dvault config --global`) lives outside the vault at `~/.config/dvault/config.toml`.

## Scope

Intentionally **not** included: other file formats (Word `.docx` only, by design); a sync server or `push`/`pull` — collaboration is via a shared, cloud-synced folder plus an advisory lock instead (see [Collaboration](#collaboration)); rename tracking; and auto-commit-on-save (it would fight the deliberate *milestone* model). Merging is **whole-file** (you pick a side on conflict), not a content-level blend — an intentional choice for binary `.docx` files.

## Development

```sh
cargo test       # run the test suite
cargo clippy     # lint
cargo fmt        # format
```
