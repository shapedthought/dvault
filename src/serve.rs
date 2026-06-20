//! `dvault serve` — a small local web UI for browsing history and reviewing
//! diffs in a browser, plus basic actions (commit, restore, tag).
//!
//! Uses `tiny_http` (synchronous, single-threaded — fine for a local,
//! single-user UI). Binds to localhost by default; pass `--host 0.0.0.0` for
//! Docker and map the port host-locally (`-p 127.0.0.1:8080:8080`). Pages are
//! server-rendered HTML reusing the diff renderer; the only JavaScript is
//! `confirm()` on destructive actions.

use crate::config::Config;
use crate::db::{Db, SHORT_HASH_LEN};
use crate::log::format_timestamp;
use crate::report::{DIFF_CSS, counts, diff_body_html, esc};
use crate::vault::Vault;
use crate::{checkout, commit, diff, extract, handoff, refs, revparse, store, tag};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Cursor;
use tiny_http::{Header, Method, Request, Response, Server};

type Resp = Response<Cursor<Vec<u8>>>;

pub fn run(host: String, port: u16) -> Result<()> {
    // Fail fast if we're not in a vault.
    Vault::discover().context("run 'dvault serve' inside a vault (a folder with .dvault/)")?;

    let addr = format!("{host}:{port}");
    let server =
        Server::http(&addr).map_err(|e| anyhow::anyhow!("could not bind to {addr}: {e}"))?;
    println!("dvault UI running at http://{addr}  (Ctrl-C to stop)");

    for mut request in server.incoming_requests() {
        let resp = match handle(&mut request) {
            Ok(r) => r,
            Err(e) => html_resp(page(
                "Error",
                &format!(
                    "<p class=\"err\">{}</p><p><a href=\"/\">← back</a></p>",
                    esc(&e.to_string())
                ),
            ))
            .with_status_code(500),
        };
        let _ = request.respond(resp);
    }
    Ok(())
}

fn handle(req: &mut Request) -> Result<Resp> {
    let url = req.url().to_string();
    let method = req.method().clone();
    let (path, query) = split_query(&url);

    match (method, path.as_str()) {
        (Method::Get, "/") => home(),
        (Method::Get, "/file") => file_history(&query),
        (Method::Get, "/diff") => diff_view(&query),
        (Method::Get, "/show") => show_view(&query),
        (Method::Post, "/commit") => do_commit(&parse_pairs(&read_body(req)?)),
        (Method::Post, "/checkout") => do_checkout(&parse_pairs(&read_body(req)?)),
        (Method::Post, "/tag") => do_tag(&parse_pairs(&read_body(req)?)),
        _ => Ok(html_resp(page("Not found", "<p>Not found.</p>")).with_status_code(404)),
    }
}

// --- views -----------------------------------------------------------------

fn home() -> Result<Resp> {
    let vault = Vault::discover()?;
    let config = Config::load(&vault.config_path())?;
    let db = Db::open(&vault.db_path())?;
    let branch = refs::current_branch(&vault)?;
    let tip = refs::branch_tip(&vault, &branch)?;

    let project = vault
        .root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "vault".into());

    let mut b = String::new();
    b.push_str(&format!(
        "<h1>{} <span class=\"muted\">· branch {}</span></h1>",
        esc(&project),
        esc(&branch)
    ));

    b.push_str("<h2>Tracked files</h2>");
    if config.tracked.files.is_empty() {
        b.push_str("<p class=\"muted\">No files tracked yet.</p>");
    } else {
        b.push_str("<table>");
        for rel in &config.tracked.files {
            b.push_str(&format!(
                "<tr><td><a href=\"/file?path={p}\">{name}</a></td><td>{status}</td>\
                 <td><a href=\"/diff?file={p}&amp;to=working\">view changes</a></td></tr>",
                p = urlenc(rel),
                name = esc(rel),
                status = file_status(&vault, &db, tip.as_deref(), rel)?,
            ));
        }
        b.push_str("</table>");
    }

    b.push_str(
        r#"<h2>Commit current changes</h2>
<form method="post" action="/commit">
  <input type="text" name="message" placeholder="Commit message" size="44" required>
  <input type="submit" value="Commit">
</form>"#,
    );

    match &tip {
        Some(t) => {
            b.push_str("<h2>Recent commits</h2><table>");
            for c in db.reachable_commits(t, None)?.iter().take(20) {
                b.push_str(&commit_row(c));
            }
            b.push_str("</table>");
        }
        None => b.push_str("<p class=\"muted\">No commits yet.</p>"),
    }

    Ok(html_resp(page("dvault", &b)))
}

fn file_history(q: &HashMap<String, String>) -> Result<Resp> {
    let rel = q.get("path").context("missing ?path")?;
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;
    let branch = refs::current_branch(&vault)?;
    let tip = refs::branch_tip(&vault, &branch)?;

    let mut b = String::new();
    b.push_str(&format!("<h1>{}</h1>", esc(rel)));
    b.push_str(&format!(
        "<p><a href=\"/diff?file={}&amp;to=working\">View working-copy changes (vs latest)</a></p>",
        urlenc(rel)
    ));

    let commits = match &tip {
        Some(t) => db.reachable_commits(t, Some(rel))?,
        None => Vec::new(),
    };

    b.push_str("<h2>Version history</h2><table>");
    b.push_str("<tr><th>Version</th><th>When</th><th>Message</th><th>Author</th><th></th></tr>");
    for (i, c) in commits.iter().enumerate() {
        let short: String = c.id.chars().take(SHORT_HASH_LEN).collect();
        // Diff this version against the one before it in history.
        let diff_link = match commits.get(i + 1) {
            Some(prev) => format!(
                "<a href=\"/diff?file={f}&amp;from={p}&amp;to={t}\">diff</a> ",
                f = urlenc(rel),
                p = urlenc(&prev.id),
                t = urlenc(&c.id)
            ),
            None => String::new(),
        };
        let restore = format!(
            "<form class=\"inline\" method=\"post\" action=\"/checkout\" \
             onsubmit=\"return confirm('Restore the working file to {short}? This overwrites it.')\">\
             <input type=\"hidden\" name=\"commit\" value=\"{cid}\">\
             <input type=\"hidden\" name=\"file\" value=\"{file}\"><button>restore</button></form>",
            short = esc(&short),
            cid = esc(&c.id),
            file = esc(rel),
        );
        b.push_str(&format!(
            "<tr><td><a class=\"hash\" href=\"/show?commit={cid}\">{short}</a></td>\
             <td>{when}</td><td>{msg}</td><td class=\"muted\">{who}</td><td>{diff_link}{restore}</td></tr>",
            cid = urlenc(&c.id),
            short = short,
            when = format_timestamp(&c.created_at),
            msg = esc(&c.message),
            who = esc(&c.author_name),
        ));
    }
    b.push_str("</table>");
    Ok(html_resp(page(rel, &b)))
}

fn diff_view(q: &HashMap<String, String>) -> Result<Resp> {
    let file = q.get("file").context("missing ?file")?;
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;

    let to_ref = q
        .get("to")
        .cloned()
        .unwrap_or_else(|| "working".to_string());
    // Default the "from" side to the file's latest committed version.
    let from_ref = match q.get("from") {
        Some(f) => f.clone(),
        None => {
            let branch = refs::current_branch(&vault)?;
            match refs::branch_tip(&vault, &branch)? {
                Some(t) => match db.file_at_commit(&t, file)? {
                    Some((c, _)) => c.id,
                    None => "working".to_string(),
                },
                None => "working".to_string(),
            }
        }
    };

    let (old_bytes, old_label) = version_bytes(&vault, &db, file, &from_ref)?;
    let (new_bytes, new_label) = version_bytes(&vault, &db, file, &to_ref)?;
    let old_lines = extract::extract_lines(file, &old_bytes)?;
    let new_lines = extract::extract_lines(file, &new_bytes)?;
    let (ins, del) = counts(&diff::join_lines(&old_lines), &diff::join_lines(&new_lines));
    let body_html = diff_body_html(&old_lines, &new_lines);

    let mut b = String::new();
    b.push_str(&format!(
        "<h1>{file} <span class=\"muted\">{old} → {new}</span></h1>",
        file = esc(file),
        old = esc(&old_label),
        new = esc(&new_label),
    ));
    if body_html.trim().is_empty() {
        b.push_str("<p class=\"muted\">No textual changes.</p>");
    } else {
        b.push_str(&format!(
            "<p class=\"summary\"><span class=\"ins\">{ins} insertions</span>, \
             <span class=\"del\">{del} deletions</span></p><div class=\"diff\">{body_html}</div>"
        ));
    }
    b.push_str(&format!(
        "<p><a href=\"/file?path={}\">← history</a></p>",
        urlenc(file)
    ));
    Ok(html_resp(page(file, &b)))
}

fn show_view(q: &HashMap<String, String>) -> Result<Resp> {
    let reference = q.get("commit").context("missing ?commit")?;
    let vault = Vault::discover()?;
    let db = Db::open(&vault.db_path())?;
    let commit_id = revparse::resolve(&vault, &db, reference)?;
    let c = db.get_commit(&commit_id)?;
    let parents = db.parents_of(&commit_id)?;
    let files = db.commit_files(&commit_id)?;
    let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();

    let mut b = String::new();
    b.push_str(&format!(
        "<h1>Commit <span class=\"hash\">{}</span></h1>",
        short
    ));
    let email = c
        .author_email
        .as_ref()
        .map(|e| format!(" &lt;{}&gt;", esc(e)))
        .unwrap_or_default();
    b.push_str(&format!(
        "<p class=\"muted\">{}{} · {}</p>",
        esc(&c.author_name),
        email,
        format_timestamp(&c.created_at)
    ));
    if parents.len() >= 2 {
        let ps: Vec<String> = parents
            .iter()
            .map(|p| p.chars().take(SHORT_HASH_LEN).collect())
            .collect();
        b.push_str(&format!(
            "<p class=\"muted\">Merge of {}</p>",
            esc(&ps.join(" + "))
        ));
    }
    b.push_str(&format!("<p><strong>{}</strong></p>", esc(&c.message)));

    b.push_str("<h2>Files changed</h2><table>");
    for cf in &files {
        let link = match parents.first() {
            Some(p) => format!(
                "<a href=\"/diff?file={f}&amp;from={p}&amp;to={t}\">diff</a>",
                f = urlenc(&cf.file_path),
                p = urlenc(p),
                t = urlenc(&commit_id)
            ),
            None => String::new(),
        };
        b.push_str(&format!(
            "<tr><td>{name}</td><td class=\"muted\">{size}</td><td>{link}</td></tr>",
            name = esc(&cf.file_path),
            size = human_size(cf.file_size),
        ));
    }
    b.push_str("</table>");

    b.push_str(&format!(
        "<h2>Tag this commit</h2>\
         <form method=\"post\" action=\"/tag\"><input type=\"hidden\" name=\"commit\" value=\"{cid}\">\
         <input type=\"text\" name=\"name\" placeholder=\"tag name\"> <button>Tag</button></form>",
        cid = esc(&commit_id)
    ));

    Ok(html_resp(page("commit", &b)))
}

// --- actions ---------------------------------------------------------------

fn do_commit(p: &HashMap<String, String>) -> Result<Resp> {
    let msg = p.get("message").cloned().unwrap_or_default();
    if msg.trim().is_empty() {
        return Ok(html_resp(page(
            "Error",
            "<p class=\"err\">A commit message is required.</p><p><a href=\"/\">← back</a></p>",
        )));
    }
    commit::run(msg, false)?;
    Ok(redirect("/"))
}

fn do_checkout(p: &HashMap<String, String>) -> Result<Resp> {
    let commit_ref = p.get("commit").context("missing commit")?.clone();
    let file = p.get("file").context("missing file")?.clone();
    checkout::run(commit_ref, file.clone(), true)?;
    Ok(redirect(&format!("/file?path={}", urlenc(&file))))
}

fn do_tag(p: &HashMap<String, String>) -> Result<Resp> {
    let name = p.get("name").context("missing tag name")?.clone();
    if name.trim().is_empty() {
        return Ok(redirect("/"));
    }
    let commit_ref = p.get("commit").cloned();
    tag::run(Some(name), commit_ref, false)?;
    Ok(redirect("/"))
}

// --- helpers ---------------------------------------------------------------

fn file_status(vault: &Vault, db: &Db, tip: Option<&str>, rel: &str) -> Result<String> {
    if let Some(h) = handoff::outstanding(vault, rel)? {
        return Ok(format!(
            "<span class=\"badge out\">out for edit · {}</span>",
            esc(&h.handed_to)
        ));
    }
    let working = vault.working_path(rel);
    if !working.is_file() {
        return Ok("<span class=\"muted\">missing</span>".into());
    }
    let hash = store::hash_bytes(&std::fs::read(&working)?);
    let committed = tip.and_then(|t| db.file_at(t, rel).ok().flatten());
    Ok(match committed {
        None => "<span class=\"badge\">new</span>".into(),
        Some(cf) if cf.blob_hash == hash => "<span class=\"muted\">unchanged</span>".into(),
        Some(_) => "<span class=\"badge mod\">modified</span>".into(),
    })
}

fn commit_row(c: &crate::db::Commit) -> String {
    let short: String = c.id.chars().take(SHORT_HASH_LEN).collect();
    format!(
        "<tr><td><a class=\"hash\" href=\"/show?commit={cid}\">{short}</a></td>\
         <td>{when}</td><td>{msg}</td><td class=\"muted\">{who}</td></tr>",
        cid = urlenc(&c.id),
        short = short,
        when = format_timestamp(&c.created_at),
        msg = esc(&c.message),
        who = esc(&c.author_name),
    )
}

/// Bytes + label for a version: `working` for the working copy, else a commit
/// or tag reference.
fn version_bytes(
    vault: &Vault,
    db: &Db,
    file_rel: &str,
    reference: &str,
) -> Result<(Vec<u8>, String)> {
    if reference == "working" {
        let working = vault.working_path(file_rel);
        let bytes = std::fs::read(&working)
            .with_context(|| format!("could not read {}", working.display()))?;
        Ok((bytes, "working copy".into()))
    } else {
        let commit_id = revparse::resolve(vault, db, reference)?;
        let cf = db
            .get_commit_file(&commit_id, file_rel)?
            .with_context(|| format!("{file_rel} was not in that commit"))?;
        let bytes = store::read_blob(&vault.objects_dir(), &cf.blob_hash, cf.compressed)?;
        let short: String = commit_id.chars().take(SHORT_HASH_LEN).collect();
        Ok((bytes, short))
    }
}

fn human_size(bytes: i64) -> String {
    let b = bytes.max(0) as u64;
    if b >= 1024 * 1024 {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{} KB", b / 1024)
    } else {
        format!("{b} B")
    }
}

fn html_resp(body: String) -> Resp {
    Response::from_string(body).with_header(
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
    )
}

fn redirect(to: &str) -> Resp {
    Response::from_string("")
        .with_status_code(303)
        .with_header(Header::from_bytes(&b"Location"[..], to.as_bytes()).unwrap())
}

fn read_body(req: &mut Request) -> Result<String> {
    let mut s = String::new();
    req.as_reader()
        .read_to_string(&mut s)
        .context("could not read request body")?;
    Ok(s)
}

fn split_query(url: &str) -> (String, HashMap<String, String>) {
    match url.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_pairs(q)),
        None => (url.to_string(), HashMap::new()),
    }
}

fn parse_pairs(s: &str) -> HashMap<String, String> {
    s.split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((percent_decode(k), percent_decode(v)))
        })
        .collect()
}

/// Decode `%XX` escapes and `+` (used in query strings and form bodies).
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a value for use in a URL (query value or path).
fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &byte in s.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head>
<meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{t} · dvault</title>
<style>
 body {{ font-family: -apple-system, Segoe UI, Roboto, sans-serif; max-width: 64rem; margin: 1.5rem auto; padding: 0 1rem; color: #1a1a1a; }}
 a {{ color: #2563eb; text-decoration: none; }} a:hover {{ text-decoration: underline; }}
 nav {{ margin-bottom: 1.5rem; padding-bottom: .5rem; border-bottom: 1px solid #eee; }}
 nav .brand {{ font-weight: 700; font-size: 1.1rem; }}
 h1 {{ font-size: 1.3rem; }} h2 {{ font-size: 1.05rem; margin-top: 1.6rem; }}
 table {{ border-collapse: collapse; width: 100%; font-size: .9rem; }}
 td, th {{ text-align: left; padding: .35rem .5rem; border-bottom: 1px solid #f0f0f0; vertical-align: top; }}
 .hash {{ font-family: ui-monospace, monospace; color: #b45309; }}
 .muted {{ color: #888; }}
 .summary {{ font-weight: 600; }} .summary .ins {{ color: #137333; }} .summary .del {{ color: #b3261e; }}
 .badge {{ font-size: .75rem; padding: .05rem .4rem; border-radius: 4px; background: #eee; }}
 .mod {{ background: #fef3c7; }} .out {{ background: #e0e7ff; }}
 form.inline {{ display: inline; }}
 button, input[type=submit] {{ font: inherit; padding: .2rem .55rem; border: 1px solid #ccc; border-radius: 5px; background: #f7f7f7; cursor: pointer; }}
 input[type=text] {{ font: inherit; padding: .25rem .5rem; border: 1px solid #ccc; border-radius: 5px; }}
 .err {{ color: #b3261e; }}
{diff_css}
</style></head><body>
<nav><a class="brand" href="/">⛁ dvault</a></nav>
{body}
</body></html>"#,
        t = esc(title),
        diff_css = DIFF_CSS,
        body = body,
    )
}
