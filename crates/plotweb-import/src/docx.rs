use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::{DetectedChapter, ImportError};

/// Extract text from a .docx file and split into chapters.
///
/// Docx files are ZIP archives containing `word/document.xml`.
/// We parse the XML to extract paragraphs, detecting heading styles
/// (Heading1/Heading2) as chapter boundaries.
pub fn split_chapters(data: &[u8]) -> Result<Vec<DetectedChapter>, ImportError> {
    let cursor = Cursor::new(data);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| ImportError::DocxError(e.to_string()))?;

    let xml = {
        let mut file = archive
            .by_name("word/document.xml")
            .map_err(|e| ImportError::DocxError(format!("missing word/document.xml: {}", e)))?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut buf)
            .map_err(|e| ImportError::DocxError(e.to_string()))?;
        buf
    };

    // Also try to load styles.xml for style name mapping
    let style_map = load_style_map(&mut archive);

    let paragraphs = parse_document_xml(&xml, &style_map)?;

    if paragraphs.is_empty() {
        return Ok(Vec::new());
    }

    // Split paragraphs into chapters based on heading paragraphs
    let mut chapters: Vec<DetectedChapter> = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for para in &paragraphs {
        if para.is_heading {
            // Flush previous chapter
            if current_title.is_some() || !current_lines.is_empty() {
                let title = current_title.take().unwrap_or_else(|| "Preamble".to_string());
                let content = current_lines.join("\n\n").trim().to_string();
                chapters.push(DetectedChapter { title, content });
                current_lines.clear();
            }
            current_title = Some(para.text.clone());
        } else if !para.text.is_empty() {
            if let Some(ref align) = para.alignment {
                current_lines.push(format!("{{align:{}}}\n{}", align, para.text));
            } else {
                current_lines.push(para.text.clone());
            }
        }
    }

    // Flush last chapter
    if current_title.is_some() || !current_lines.is_empty() {
        let title = current_title.unwrap_or_else(|| "Chapter 1".to_string());
        let content = current_lines.join("\n\n").trim().to_string();
        chapters.push(DetectedChapter { title, content });
    }

    // If no headings were found, try the text-based heuristics (same as markdown)
    if chapters.len() <= 1 {
        let full_text: String = paragraphs
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let md_chapters = crate::markdown::split_chapters(&full_text);
        if md_chapters.len() > 1 {
            return Ok(md_chapters);
        }
    }

    Ok(chapters)
}

struct Paragraph {
    text: String,
    is_heading: bool,
    alignment: Option<String>,
}

/// Map from style ID -> whether it's a heading style.
type StyleMap = std::collections::HashMap<String, bool>;

fn load_style_map(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> StyleMap {
    let mut map = StyleMap::new();

    let xml = match archive.by_name("word/styles.xml") {
        Ok(mut file) => {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut file, &mut buf).ok();
            buf
        }
        Err(_) => return map,
    };

    let mut reader = Reader::from_reader(xml.as_slice());
    let mut buf = Vec::new();
    let mut current_style_id = String::new();
    let mut in_style = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref().to_vec();
                let local = local_name(&name_bytes);
                if local == b"style" {
                    current_style_id.clear();
                    in_style = true;
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.as_ref()) == b"styleId" {
                            current_style_id =
                                String::from_utf8_lossy(&attr.value).to_string();
                        }
                    }
                }
                if in_style && local == b"name" {
                    for attr in e.attributes().flatten() {
                        if local_name(attr.key.as_ref()) == b"val" {
                            let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                            if val.starts_with("heading") {
                                let is_chapter_heading = val == "heading 1"
                                    || val == "heading 2"
                                    || val == "heading1"
                                    || val == "heading2";
                                map.insert(
                                    current_style_id.clone(),
                                    is_chapter_heading,
                                );
                            }
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref().to_vec();
                if local_name(&name_bytes) == b"style" {
                    in_style = false;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    map
}

fn parse_document_xml(xml: &[u8], style_map: &StyleMap) -> Result<Vec<Paragraph>, ImportError> {
    let mut reader = Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut paragraphs = Vec::new();

    let mut in_paragraph = false;
    let mut in_run = false;
    let mut in_rpr = false;
    let mut para_text = String::new();
    let mut is_heading = false;
    let mut para_alignment: Option<String> = None;

    // Run-level formatting flags (reset per run)
    let mut run_bold = false;
    let mut run_italic = false;
    let mut run_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref().to_vec();
                let local = local_name(&name_bytes);
                match local {
                    b"p" => {
                        in_paragraph = true;
                        para_text.clear();
                        is_heading = false;
                        para_alignment = None;
                    }
                    b"pStyle" if in_paragraph => {
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.as_ref()) == b"val" {
                                let val = String::from_utf8_lossy(&attr.value);
                                let val_str = val.to_string();
                                let val_lower = val.to_lowercase();

                                if val_lower.starts_with("heading") || val_lower.starts_with("titre") {
                                    if val_lower.contains('1') || val_lower.contains('2') {
                                        is_heading = true;
                                    }
                                }

                                if let Some(&is_ch) = style_map.get(&val_str) {
                                    if is_ch {
                                        is_heading = true;
                                    }
                                }
                            }
                        }
                    }
                    b"jc" if in_paragraph => {
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.as_ref()) == b"val" {
                                let val = String::from_utf8_lossy(&attr.value).to_lowercase();
                                match val.as_str() {
                                    "center" => para_alignment = Some("center".into()),
                                    "right" | "end" => para_alignment = Some("right".into()),
                                    "both" | "distribute" => para_alignment = Some("justify".into()),
                                    _ => {} // "left"/"start" is the default
                                }
                            }
                        }
                    }
                    b"r" if in_paragraph => {
                        in_run = true;
                        run_bold = false;
                        run_italic = false;
                        run_text.clear();
                    }
                    b"rPr" if in_run => {
                        in_rpr = true;
                    }
                    b"b" if in_rpr => {
                        // <b/> or <b w:val="true"/> means bold; <b w:val="false"/> means not bold
                        let mut is_off = false;
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.as_ref()) == b"val" {
                                let v = String::from_utf8_lossy(&attr.value).to_lowercase();
                                if v == "false" || v == "0" {
                                    is_off = true;
                                }
                            }
                        }
                        if !is_off {
                            run_bold = true;
                        }
                    }
                    b"i" if in_rpr => {
                        let mut is_off = false;
                        for attr in e.attributes().flatten() {
                            if local_name(attr.key.as_ref()) == b"val" {
                                let v = String::from_utf8_lossy(&attr.value).to_lowercase();
                                if v == "false" || v == "0" {
                                    is_off = true;
                                }
                            }
                        }
                        if !is_off {
                            run_italic = true;
                        }
                    }
                    b"br" if in_run => {
                        para_text.push('\n');
                    }
                    b"tab" if in_run => {
                        para_text.push('\t');
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let name = e.name();
                let name_bytes = name.as_ref().to_vec();
                let local = local_name(&name_bytes);
                match local {
                    b"p" => {
                        in_paragraph = false;
                        let text = para_text.trim().to_string();
                        paragraphs.push(Paragraph { text, is_heading, alignment: para_alignment.take() });
                    }
                    b"r" => {
                        // Flush run text with formatting
                        if !run_text.is_empty() {
                            let formatted = wrap_formatting(&run_text, run_bold, run_italic);
                            para_text.push_str(&formatted);
                        }
                        in_run = false;
                    }
                    b"rPr" => {
                        in_rpr = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) if in_run => {
                if let Ok(text) = e.unescape() {
                    run_text.push_str(&text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(ImportError::DocxError(format!("XML parse error: {}", e))),
            _ => {}
        }
        buf.clear();
    }

    Ok(paragraphs)
}

/// Wrap text in markdown bold/italic markers.
fn wrap_formatting(text: &str, bold: bool, italic: bool) -> String {
    match (bold, italic) {
        (true, true) => format!("***{}***", text),
        (true, false) => format!("**{}**", text),
        (false, true) => format!("*{}*", text),
        (false, false) => text.to_string(),
    }
}

/// Get the local name of an XML tag, stripping the namespace prefix.
fn local_name(full: &[u8]) -> &[u8] {
    if let Some(pos) = full.iter().rposition(|&b| b == b':') {
        &full[pos + 1..]
    } else {
        full
    }
}
