//! `.docx` readable-text extraction.
//!
//! A `.docx` is a ZIP archive. Body text lives in `word/document.xml`, but a
//! document's readable content is also spread across other parts: headers
//! (`word/headerN.xml`), footers (`word/footerN.xml`), footnotes/endnotes, and
//! comments. They all use the same WordprocessingML — paragraphs (`<w:p>`) of
//! runs (`<w:r>`) of text (`<w:t>`) — so one parser handles every part.
//!
//! We extract each part and concatenate them in a stable order, with the
//! non-body regions introduced by a `[Header]` / `[Footnotes]` / ... banner so
//! a diff makes clear *where* a change happened. (Text boxes live inside
//! `document.xml`, so they're already covered by the body extraction.)
//!
//! `<w:tab/>` and `<w:br/>` are preserved as a tab / space so that tab-separated
//! runs (e.g. table-cell label/value pairs) don't get glued into run-on words
//! like "EnglishNative".

use anyhow::{Context, Result, bail};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::{Cursor, Read};

type Archive = zip::ZipArchive<Cursor<Vec<u8>>>;

/// Extract the readable text of a `.docx`, one line per non-empty paragraph,
/// across the body and all supporting regions.
pub fn extract(bytes: &[u8]) -> Result<Vec<String>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))
        .context("file is not a valid .docx (zip) archive")?;
    let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();

    let mut lines = Vec::new();

    // Body — required. Includes text boxes (they live inside document.xml).
    if !names.iter().any(|n| n == "word/document.xml") {
        bail!("not a valid .docx: missing word/document.xml");
    }
    lines.extend(parse_paragraphs(&read_entry(
        &mut archive,
        "word/document.xml",
    )?)?);

    // Headers and footers can each span several files (default/even/first
    // page); merge them per type so the reader sees one section.
    let headers = matching(&names, "word/header", ".xml");
    append_section(
        &mut lines,
        "Header",
        read_and_parse(&mut archive, &headers)?,
    );
    let footers = matching(&names, "word/footer", ".xml");
    append_section(
        &mut lines,
        "Footer",
        read_and_parse(&mut archive, &footers)?,
    );

    // Single-file regions.
    for (file, label) in [
        ("word/footnotes.xml", "Footnotes"),
        ("word/endnotes.xml", "Endnotes"),
        ("word/comments.xml", "Comments"),
    ] {
        if names.iter().any(|n| n == file) {
            let section = parse_paragraphs(&read_entry(&mut archive, file)?)?;
            append_section(&mut lines, label, section);
        }
    }

    Ok(lines)
}

/// File names starting with `prefix` and ending with `suffix`, sorted for a
/// deterministic ordering (so diffs are stable across versions).
fn matching(names: &[String], prefix: &str, suffix: &str) -> Vec<String> {
    let mut v: Vec<String> = names
        .iter()
        .filter(|n| n.starts_with(prefix) && n.ends_with(suffix))
        .cloned()
        .collect();
    v.sort();
    v
}

fn read_entry(archive: &mut Archive, name: &str) -> Result<Vec<u8>> {
    let mut entry = archive
        .by_name(name)
        .with_context(|| format!("could not open {name} inside the .docx"))?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .with_context(|| format!("could not read {name}"))?;
    Ok(buf)
}

/// Parse and concatenate the paragraphs of several parts.
fn read_and_parse(archive: &mut Archive, files: &[String]) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for f in files {
        lines.extend(parse_paragraphs(&read_entry(archive, f)?)?);
    }
    Ok(lines)
}

/// Append a labeled section if it has any content.
fn append_section(lines: &mut Vec<String>, label: &str, section: Vec<String>) {
    if section.is_empty() {
        return;
    }
    lines.push(format!("[{label}]"));
    lines.extend(section);
}

/// The five predefined XML entities.
fn predefined_entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "apos" => Some('\''),
        "quot" => Some('"'),
        _ => None,
    }
}

/// Walk a WordprocessingML part and return one line per non-empty paragraph.
fn parse_paragraphs(xml: &[u8]) -> Result<Vec<String>> {
    let mut reader = Reader::from_reader(xml);
    let config = reader.config_mut();
    config.trim_text(false);

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    // True while we are inside a <w:t> element and should capture text events.
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .context("malformed XML in a .docx part")?
        {
            Event::Start(e) if e.name().as_ref() == b"w:t" => in_text = true,
            Event::End(e) => match e.name().as_ref() {
                b"w:t" => in_text = false,
                b"w:p" => {
                    let line = current.trim().to_string();
                    if !line.is_empty() {
                        lines.push(line);
                    }
                    current.clear();
                }
                _ => {}
            },
            Event::Empty(e) => match e.name().as_ref() {
                b"w:tab" => current.push('\t'),
                b"w:br" | b"w:cr" => current.push(' '),
                _ => {}
            },
            Event::Text(e) if in_text => {
                let decoded = e.decode().context("could not decode text run")?;
                current.push_str(&decoded);
            }
            // quick-xml emits entity/character references as their own events
            // rather than folding them into the surrounding Text. Resolve them
            // so "R&amp;D" comes back as "R&D" instead of "RD".
            Event::GeneralRef(e) if in_text => {
                if let Some(c) = e.resolve_char_ref().context("bad character reference")? {
                    current.push(c);
                } else {
                    let name = e.decode().context("could not decode entity")?;
                    if let Some(c) = predefined_entity(&name) {
                        current.push(c);
                    }
                    // Unknown named entities are dropped (none occur in docx body text).
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    // Flush a trailing paragraph that wasn't closed (defensive).
    let tail = current.trim();
    if !tail.is_empty() {
        lines.push(tail.to_string());
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    /// Build a minimal .docx (zip with word/document.xml) from a body fragment.
    fn make_docx(body: &str) -> Vec<u8> {
        make_docx_parts(body, &[])
    }

    /// Build a .docx with a body plus arbitrary extra parts, each given as
    /// `(zip_path, raw_xml)`.
    fn make_docx_parts(body: &str, extra: &[(&str, String)]) -> Vec<u8> {
        let document = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="{NS}"><w:body>{body}</w:body></w:document>"#
        );
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::FileOptions::<()>::default();
            zip.start_file("word/document.xml", opts).unwrap();
            zip.write_all(document.as_bytes()).unwrap();
            for (path, xml) in extra {
                zip.start_file(*path, opts).unwrap();
                zip.write_all(xml.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    /// Wrap paragraph fragments in a WordprocessingML root element.
    fn part(root: &str, inner: &str) -> String {
        format!(r#"<?xml version="1.0"?><w:{root} xmlns:w="{NS}">{inner}</w:{root}>"#)
    }

    fn para(text: &str) -> String {
        format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
    }

    #[test]
    fn extracts_one_line_per_paragraph() {
        let docx = make_docx(
            r#"<w:p><w:r><w:t>First line</w:t></w:r></w:p>
               <w:p><w:r><w:t>Second line</w:t></w:r></w:p>"#,
        );
        assert_eq!(extract(&docx).unwrap(), vec!["First line", "Second line"]);
    }

    #[test]
    fn joins_multiple_runs_in_a_paragraph() {
        let docx = make_docx(
            r#"<w:p><w:r><w:t>Revenue was </w:t></w:r><w:r><w:t>$4.8M</w:t></w:r></w:p>"#,
        );
        assert_eq!(extract(&docx).unwrap(), vec!["Revenue was $4.8M"]);
    }

    #[test]
    fn tab_separates_runs_instead_of_gluing_them() {
        // Regression: table-cell label/value pairs must not become "EnglishNative".
        let docx = make_docx(
            r#"<w:p><w:r><w:t>English</w:t></w:r><w:r><w:tab/><w:t>Native</w:t></w:r></w:p>"#,
        );
        assert_eq!(extract(&docx).unwrap(), vec!["English\tNative"]);
    }

    #[test]
    fn skips_empty_paragraphs() {
        let docx = make_docx(r#"<w:p></w:p><w:p><w:r><w:t>Only line</w:t></w:r></w:p><w:p></w:p>"#);
        assert_eq!(extract(&docx).unwrap(), vec!["Only line"]);
    }

    #[test]
    fn resolves_xml_entities() {
        let docx = make_docx(r#"<w:p><w:r><w:t>R&amp;D &lt;tag&gt;</w:t></w:r></w:p>"#);
        assert_eq!(extract(&docx).unwrap(), vec!["R&D <tag>"]);
    }

    #[test]
    fn rejects_non_docx_bytes() {
        assert!(extract(b"not a zip file").is_err());
    }

    #[test]
    fn extracts_headers_footers_footnotes_with_banners() {
        let docx = make_docx_parts(
            &para("Body paragraph"),
            &[
                ("word/header1.xml", part("hdr", &para("Confidential"))),
                ("word/footer1.xml", part("ftr", &para("Page 1"))),
                (
                    "word/footnotes.xml",
                    part(
                        "footnotes",
                        &format!("<w:footnote>{}</w:footnote>", para("See appendix A")),
                    ),
                ),
            ],
        );
        assert_eq!(
            extract(&docx).unwrap(),
            vec![
                "Body paragraph",
                "[Header]",
                "Confidential",
                "[Footer]",
                "Page 1",
                "[Footnotes]",
                "See appendix A",
            ]
        );
    }

    #[test]
    fn merges_multiple_header_files_under_one_banner() {
        let docx = make_docx_parts(
            &para("Body"),
            &[
                ("word/header2.xml", part("hdr", &para("Even page"))),
                ("word/header1.xml", part("hdr", &para("Default page"))),
            ],
        );
        // Sorted by filename: header1 before header2, both under one [Header].
        assert_eq!(
            extract(&docx).unwrap(),
            vec!["Body", "[Header]", "Default page", "Even page"]
        );
    }

    #[test]
    fn omits_banner_for_empty_regions() {
        // A header part with no real text (e.g. only a separator) adds nothing.
        let docx = make_docx_parts(
            &para("Body"),
            &[("word/header1.xml", part("hdr", "<w:p></w:p>"))],
        );
        assert_eq!(extract(&docx).unwrap(), vec!["Body"]);
    }

    #[test]
    fn body_only_docx_is_unchanged() {
        // No extra parts → output is exactly the body, no banners.
        let docx = make_docx(&para("Just the body"));
        assert_eq!(extract(&docx).unwrap(), vec!["Just the body"]);
    }
}
