//! Pulling plain text out of PDFs, Word documents and slide decks.
//!
//! These are pure functions over bytes — no database, no HTTP, no AI, and in
//! increment 15 they are wired to **nothing**. That is deliberate: if
//! `pdf-extract` interleaves a two-column handout into nonsense, that is worth
//! discovering for the price of one module rather than after an upload path, a
//! staging table and a review screen have all been built on the assumption it
//! works.
//!
//! All three are pure Rust. `zip` is pinned to the `deflate` feature only,
//! because its default set drags in `zstd-sys` and `bzip2-sys` — native C that
//! would add a compiler toolchain to the Docker runtime for formats an Office
//! file never uses.

use std::io::{Cursor, Read};

use crate::error::DocumentError;

/// Documents larger than this are refused before parsing. A 50 MB PDF is either
/// a scan (no text layer, so nothing to extract) or a textbook, and neither
/// belongs in a single flashcard generation.
pub const MAX_DOCUMENT_BYTES: usize = 15 * 1024 * 1024;

/// Below this many characters, a "successful" extraction is almost certainly a
/// scanned document with no text layer. Saying so is far more useful than
/// handing the model three characters and charging for the privilege.
pub const MIN_USEFUL_CHARS: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Pdf,
    Docx,
    Pptx,
}

impl DocumentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentKind::Pdf => "pdf",
            DocumentKind::Docx => "docx",
            DocumentKind::Pptx => "pptx",
        }
    }

    /// Sniffs the format from the bytes, not the filename — an extension is a
    /// claim, and a mislabelled file should fail clearly rather than oddly.
    pub fn sniff(bytes: &[u8], filename: Option<&str>) -> Option<Self> {
        if bytes.starts_with(b"%PDF-") {
            return Some(DocumentKind::Pdf);
        }
        // DOCX and PPTX are both zips; the entry names tell them apart.
        if bytes.starts_with(b"PK\x03\x04") {
            if let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(bytes)) {
                let names: Vec<String> = (0..archive.len())
                    .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
                    .collect();
                if names.iter().any(|n| n.starts_with("word/")) {
                    return Some(DocumentKind::Docx);
                }
                if names.iter().any(|n| n.starts_with("ppt/")) {
                    return Some(DocumentKind::Pptx);
                }
            }
        }
        // Fall back to the extension only when the content was inconclusive.
        match filename?.rsplit('.').next()?.to_lowercase().as_str() {
            "pdf" => Some(DocumentKind::Pdf),
            "docx" => Some(DocumentKind::Docx),
            "pptx" => Some(DocumentKind::Pptx),
            _ => None,
        }
    }
}

/// Collapses the whitespace soup every one of these formats produces, while
/// keeping paragraph breaks — the model reads structure, and a single wall of
/// text loses the separation between one slide's bullet and the next.
fn tidy(raw: &str) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    for block in raw.split('\n') {
        let line = block.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            continue;
        }
        paragraphs.push(line);
    }
    paragraphs.join("\n")
}

fn guard_size(bytes: &[u8]) -> Result<(), DocumentError> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::Unreadable(format!(
            "that file is {} MB; the limit is {} MB",
            bytes.len() / (1024 * 1024),
            MAX_DOCUMENT_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

/// Reads every `<w:t>` run out of a DOCX's `word/document.xml`, treating
/// paragraph ends as line breaks.
pub fn extract_docx(bytes: &[u8]) -> Result<String, DocumentError> {
    guard_size(bytes)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| DocumentError::Unreadable(format!("not a readable .docx: {e}")))?;
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| DocumentError::Unreadable("that .docx has no word/document.xml".into()))?
        .read_to_string(&mut xml)
        .map_err(|e| DocumentError::Unreadable(e.to_string()))?;
    Ok(tidy(&text_from_office_xml(&xml, "w:t", "w:p")))
}

/// Reads a PPTX slide by slide. Slides are numbered entries, and a deck read
/// out of order would scramble a lecture's argument, so they are sorted
/// numerically rather than lexically — `slide10` must not land between
/// `slide1` and `slide2`.
pub fn extract_pptx(bytes: &[u8]) -> Result<String, DocumentError> {
    guard_size(bytes)?;
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| DocumentError::Unreadable(format!("not a readable .pptx: {e}")))?;

    let mut slides: Vec<(usize, String)> = Vec::new();
    for i in 0..archive.len() {
        let Ok(mut file) = archive.by_index(i) else { continue };
        let name = file.name().to_string();
        if !(name.starts_with("ppt/slides/slide") && name.ends_with(".xml")) {
            continue;
        }
        let number: usize = name
            .trim_start_matches("ppt/slides/slide")
            .trim_end_matches(".xml")
            .parse()
            .unwrap_or(usize::MAX);
        let mut xml = String::new();
        if file.read_to_string(&mut xml).is_ok() {
            slides.push((number, text_from_office_xml(&xml, "a:t", "a:p")));
        }
    }
    slides.sort_by_key(|(n, _)| *n);

    let body = slides
        .into_iter()
        .enumerate()
        .map(|(i, (_, text))| format!("--- Slide {} ---\n{}", i + 1, tidy(&text)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(body)
}

/// Pulls the text nodes out of an Office XML part.
///
/// Deliberately a streaming pass over the events rather than a DOM walk: these
/// parts are deeply nested and the only thing wanted is the ordered text runs.
fn text_from_office_xml(xml: &str, text_tag: &str, paragraph_tag: &str) -> String {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if e.name().as_ref() == text_tag {
                    in_text = true;
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let name = name.as_ref();
                if name == text_tag {
                    in_text = false;
                } else if name == paragraph_tag {
                    out.push('\n');
                }
            }
            Ok(Event::Text(e)) if in_text => {
                // `xml10_content` normalises line endings; `escape::unescape`
                // turns &amp; and friends back into characters. A run that
                // fails to unescape falls back to its raw text rather than
                // aborting the document — losing an ampersand beats losing the
                // chapter.
                let raw = e.xml10_content();
                match quick_xml::escape::unescape(&raw) {
                    Ok(text) => out.push_str(&text),
                    Err(_) => out.push_str(&raw),
                }
            }
            Ok(Event::Eof) => break,
            // A malformed part costs that part, not the whole document.
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Extracts a PDF's text layer. Returns an error for a scan, which has none.
pub fn extract_pdf(bytes: &[u8]) -> Result<String, DocumentError> {
    guard_size(bytes)?;
    // pdf-extract panics on some malformed files rather than returning an
    // error, and this process runs with `panic = "abort"` — so a bad upload
    // would take the whole server down. Isolating it is not optional.
    let bytes = bytes.to_vec();
    let extracted = std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&bytes))
        .map_err(|_| DocumentError::Unreadable("that PDF could not be read — it may be corrupt".into()))?
        .map_err(|e| DocumentError::Unreadable(format!("that PDF could not be read: {e}")))?;
    Ok(tidy(&extracted))
}

/// Extracts text from whichever of the three formats this is.
pub fn extract(bytes: &[u8], filename: Option<&str>) -> Result<(DocumentKind, String), DocumentError> {
    let kind = DocumentKind::sniff(bytes, filename)
        .ok_or_else(|| DocumentError::Unreadable("that file is not a PDF, Word document or slide deck".into()))?;
    let text = match kind {
        DocumentKind::Pdf => extract_pdf(bytes)?,
        DocumentKind::Docx => extract_docx(bytes)?,
        DocumentKind::Pptx => extract_pptx(bytes)?,
    };
    if text.chars().filter(|c| !c.is_whitespace()).count() < MIN_USEFUL_CHARS {
        return Err(DocumentError::EmptyGeneration(format!(
            "that {} has almost no selectable text — if it is a scan, there is no text layer to read",
            kind.as_str()
        )));
    }
    Ok((kind, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Builds a minimal Office-shaped zip in memory, so the parsers are tested
    /// against the real container format rather than a mock.
    fn office_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(body.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn docx(paragraphs: &[&str]) -> Vec<u8> {
        let body: String = paragraphs.iter().map(|p| format!("<w:p><w:r><w:t>{p}</w:t></w:r></w:p>")).collect();
        office_zip(&[("word/document.xml", &format!("<?xml version=\"1.0\"?><w:document><w:body>{body}</w:body></w:document>"))])
    }

    #[test]
    fn extracts_paragraphs_from_a_docx() {
        let bytes = docx(&["A mole is 6.022e23 particles.", "Molar mass is grams per mole."]);
        let text = extract_docx(&bytes).unwrap();
        assert!(text.contains("A mole is 6.022e23 particles."));
        assert!(text.contains("Molar mass is grams per mole."));
        // Paragraphs stay separated, so the model can see structure.
        assert!(text.contains('\n'));
    }

    #[test]
    fn joins_runs_split_mid_sentence() {
        // Word splits a sentence across runs constantly (spellcheck, formatting).
        let xml = "<?xml version=\"1.0\"?><w:document><w:body><w:p><w:r><w:t>A mole is </w:t></w:r><w:r><w:t>6.022e23</w:t></w:r></w:p></w:body></w:document>";
        let bytes = office_zip(&[("word/document.xml", xml)]);
        assert!(extract_docx(&bytes).unwrap().contains("A mole is 6.022e23"));
    }

    #[test]
    fn a_zip_without_a_document_part_is_a_clear_error() {
        let bytes = office_zip(&[("word/styles.xml", "<x/>")]);
        let err = extract_docx(&bytes).unwrap_err().to_string();
        assert!(err.contains("word/document.xml"), "{err}");
    }

    #[test]
    fn extracts_slides_in_numeric_order() {
        // Deliberately out of order, and including slide10 — which sorts before
        // slide2 lexically and would scramble a lecture.
        let slide = |t: &str| format!("<?xml version=\"1.0\"?><p:sld><p:cSld><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:cSld></p:sld>");
        let bytes = office_zip(&[
            ("ppt/slides/slide10.xml", &slide("Tenth slide")),
            ("ppt/slides/slide2.xml", &slide("Second slide")),
            ("ppt/slides/slide1.xml", &slide("First slide")),
        ]);
        let text = extract_pptx(&bytes).unwrap();
        let first = text.find("First slide").unwrap();
        let second = text.find("Second slide").unwrap();
        let tenth = text.find("Tenth slide").unwrap();
        assert!(first < second && second < tenth, "slides came out in the wrong order:\n{text}");
        assert!(text.contains("--- Slide 1 ---"));
        assert!(text.contains("--- Slide 3 ---"), "slides are renumbered sequentially for the reader");
    }

    #[test]
    fn sniffing_prefers_content_over_the_extension() {
        assert_eq!(DocumentKind::sniff(b"%PDF-1.7\nrest", Some("notes.docx")), Some(DocumentKind::Pdf));
        assert_eq!(DocumentKind::sniff(&docx(&["x"]), Some("whatever.pdf")), Some(DocumentKind::Docx));
        let pptx = office_zip(&[("ppt/slides/slide1.xml", "<p:sld/>")]);
        assert_eq!(DocumentKind::sniff(&pptx, None), Some(DocumentKind::Pptx));
    }

    #[test]
    fn an_unrecognisable_file_is_refused() {
        assert_eq!(DocumentKind::sniff(b"just some text", Some("notes.txt")), None);
        assert_eq!(DocumentKind::sniff(b"", None), None);
        assert!(extract(b"just some text", Some("notes.txt")).is_err());
    }

    #[test]
    fn an_oversized_document_is_refused_before_parsing() {
        let huge = vec![0u8; MAX_DOCUMENT_BYTES + 1];
        for result in [extract_pdf(&huge), extract_docx(&huge), extract_pptx(&huge)] {
            assert!(result.unwrap_err().to_string().contains("limit is"));
        }
    }

    /// A scanned PDF extracts almost nothing; saying so beats charging for a
    /// generation from three characters of noise.
    #[test]
    fn a_document_with_almost_no_text_is_reported_as_such() {
        let bytes = docx(&["hi"]);
        let err = extract(&bytes, Some("scan.docx")).unwrap_err().to_string();
        assert!(err.contains("almost no selectable text"), "{err}");
    }

    #[test]
    fn malformed_archives_error_rather_than_panicking() {
        for bad in [b"PK\x03\x04garbage".to_vec(), b"PK\x03\x04".to_vec(), vec![0u8; 10]] {
            let _ = extract_docx(&bad);
            let _ = extract_pptx(&bad);
            let _ = extract(&bad, Some("x.docx"));
        }
    }

    #[test]
    fn malformed_pdf_bytes_error_rather_than_aborting_the_process() {
        // pdf-extract panics on some inputs, and this binary aborts on panic,
        // so the catch_unwind in extract_pdf is load-bearing.
        for bad in [b"%PDF-1.4\ngarbage".to_vec(), b"%PDF-".to_vec()] {
            assert!(extract_pdf(&bad).is_err());
        }
    }

    #[test]
    fn tidy_collapses_whitespace_but_keeps_paragraphs() {
        assert_eq!(tidy("  a   b  \n\n  c  \n"), "a b\nc");
        assert_eq!(tidy(""), "");
        assert_eq!(tidy("   \n  \n"), "");
    }
}
