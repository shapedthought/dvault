//! `.docx` readable-text extraction.
//!
//! A `.docx` is a ZIP archive; the body text lives in `word/document.xml`. We
//! walk the XML and collect the text of `<w:t>` (text run) elements, grouped by
//! their containing `<w:p>` (paragraph) so each paragraph becomes one logical
//! line. `<w:tab/>` and `<w:br/>` are preserved as a tab / space so that
//! tab-separated runs (e.g. table-cell label/value pairs) don't get glued into
//! run-on words like "EnglishNative".

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::io::Cursor;

/// Extract the readable text of a `.docx`, one line per non-empty paragraph.
pub fn extract(bytes: &[u8]) -> Result<Vec<String>> {
    let document_xml = read_document_xml(bytes)?;
    parse_document(&document_xml)
}

/// Pull `word/document.xml` out of the docx ZIP container.
fn read_document_xml(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .context("file is not a valid .docx (zip) archive")?;
    let mut entry = archive
        .by_name("word/document.xml")
        .context("not a valid .docx: missing word/document.xml")?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut buf).context("could not read word/document.xml")?;
    Ok(buf)
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

fn parse_document(xml: &[u8]) -> Result<Vec<String>> {
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
            .context("malformed XML in word/document.xml")?
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

    /// Build a minimal .docx (zip with word/document.xml) from a body fragment.
    fn make_docx(body: &str) -> Vec<u8> {
        let xml = format!(
            r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        );
        let mut buf = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            zip.start_file::<_, ()>("word/document.xml", zip::write::FileOptions::default())
                .unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        buf
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
}
