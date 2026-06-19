//! dvault — Git-like version control for Office documents.
//!
//! `main` is a thin clap dispatcher; each subcommand lives in its own module.
//! Errors bubble up as `anyhow::Error` and are printed to stderr with a
//! non-zero exit code — no panics on user-facing paths.

mod add;
mod branch;
mod cat;
mod changes;
mod checkout;
mod commit;
mod config;
mod config_cmd;
mod db;
mod diff;
mod export;
mod extract;
mod graph;
mod init;
mod log;
mod merge;
mod refs;
mod remove;
mod report;
mod revparse;
mod show;
mod stats;
mod status;
mod store;
mod switch;
mod tag;
mod vault;

use clap::{Parser, Subcommand};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "dvault",
    version,
    about = "Git-like version control for Office documents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a vault (.dvault/) in the current directory
    Init,

    /// Track one or more files (mirrors `git add`; does not commit)
    Add {
        /// Files to start tracking
        #[arg(required = true)]
        files: Vec<String>,
    },

    /// Stop tracking a file (history and snapshots are preserved)
    Remove { file: String },

    /// Snapshot all tracked files that have changed
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
    },

    /// Show commit history, optionally for a single file
    Log {
        /// Limit history to commits touching this file
        file: Option<String>,
        /// Show tags inline
        #[arg(long)]
        tags: bool,
        /// Draw the commit graph (branches and merges)
        #[arg(long)]
        graph: bool,
        /// With --graph, include all branches (not just the current one)
        #[arg(long)]
        all: bool,
    },

    /// Show tracked files and whether they are modified
    Status,

    /// Show readable changes for a file
    ///
    /// `dvault diff <file>` compares the working copy to the last commit;
    /// `dvault diff <from> <to> <file>` compares two commits.
    Diff {
        #[arg(required = true)]
        args: Vec<String>,
        /// Show only a paragraph-level summary, not the full diff
        #[arg(long)]
        stat: bool,
    },

    /// Print the readable text of a document version to stdout
    ///
    /// `dvault cat <file>` or `dvault cat <commit> <file>`.
    Cat {
        #[arg(required = true)]
        args: Vec<String>,
    },

    /// Show word counts and growth over time
    ///
    /// `dvault stats <file>` for one file's history, or `dvault stats` for all.
    Stats { file: Option<String> },

    /// Show a commit's metadata and the files it changed
    ///
    /// Defaults to the current branch tip. With `--diff`, also shows the
    /// readable diff of each changed file against its parent.
    Show {
        /// Commit hash or tag (defaults to the current branch tip)
        reference: Option<String>,
        /// Also show the readable diff of each changed file
        #[arg(long)]
        diff: bool,
    },

    /// List a document's pending Word tracked changes (revision marks)
    ///
    /// `dvault changes <file>` inspects the working copy;
    /// `dvault changes <commit> <file>` inspects a committed version.
    Changes {
        #[arg(required = true)]
        args: Vec<String>,
    },

    /// Write a shareable HTML or Markdown report of the changes
    ///
    /// Same version selection as `diff`: `<file>` or `<from> <to> <file>`.
    Report {
        #[arg(required = true)]
        args: Vec<String>,
        /// Output format: html (default) or md
        #[arg(long, default_value = "html")]
        format: String,
        /// Output file path (default: <name>-diff.<ext>)
        #[arg(long)]
        out: Option<String>,
    },

    /// Restore a file to a historic version, overwriting the working copy
    Checkout {
        commit: String,
        file: String,
        /// Skip the overwrite confirmation prompt
        #[arg(long)]
        force: bool,
    },

    /// Write a historic version to a new file without touching the working copy
    Export {
        commit: String,
        file: String,
        /// Output path (default: <stem>-<shorthash><ext>)
        #[arg(long)]
        out: Option<String>,
    },

    /// Tag a commit, list tags, or delete a tag
    Tag {
        /// Tag name to create or delete; omit to list tags
        name: Option<String>,
        /// Commit (or tag) to point at; defaults to the current branch tip
        commit: Option<String>,
        /// Delete the named tag
        #[arg(short = 'd', long)]
        delete: bool,
    },

    /// List branches, create a new one, or delete one
    Branch {
        /// Branch name to create or delete; omit to list branches
        name: Option<String>,
        /// Delete the named branch (refuses if unmerged)
        #[arg(short = 'd', long)]
        delete: bool,
        /// Force deletion of an unmerged branch
        #[arg(short = 'D', long)]
        force: bool,
    },

    /// Switch to another branch, updating the working files
    Switch {
        branch: String,
        /// Discard uncommitted changes instead of refusing to switch
        #[arg(long)]
        force: bool,
    },

    /// Merge another branch into the current one
    Merge {
        /// Branch to merge into the current branch
        branch: String,
    },

    /// Get or set vault configuration (e.g. user.name, user.email)
    Config {
        /// Config key, e.g. user.name
        key: String,
        /// New value; omit to read the current value
        value: Option<String>,
    },
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Init => init::run(),
        Command::Add { files } => add::run(files),
        Command::Remove { file } => remove::run(file),
        Command::Commit { message } => commit::run(message),
        Command::Log {
            file,
            tags,
            graph,
            all,
        } => log::run(file, tags, graph, all),
        Command::Status => status::run(),
        Command::Diff { args, stat } => diff::run(args, stat),
        Command::Cat { args } => cat::run(args),
        Command::Stats { file } => stats::run(file),
        Command::Show { reference, diff } => show::run(reference, diff),
        Command::Changes { args } => changes::run(args),
        Command::Report { args, format, out } => report::run(args, format, out),
        Command::Checkout {
            commit,
            file,
            force,
        } => checkout::run(commit, file, force),
        Command::Export { commit, file, out } => export::run(commit, file, out),
        Command::Tag {
            name,
            commit,
            delete,
        } => tag::run(name, commit, delete),
        Command::Branch {
            name,
            delete,
            force,
        } => branch::run(name, delete, force),
        Command::Switch { branch, force } => switch::run(branch, force),
        Command::Merge { branch } => merge::run(branch),
        Command::Config { key, value } => config_cmd::run(key, value),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
