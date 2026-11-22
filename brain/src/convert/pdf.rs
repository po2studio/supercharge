//! PDF text layers. A PDF with none (a scan) has no text to give; OCR is out of scope.

use super::Converted;
use crate::doc;
use pdf_extract::{Dictionary, Document, Object};
use std::{sync::mpsc, time::Duration};

/// A malformed PDF can send a parser in circles; it is given this long.
const TIME_LIMIT: Duration = Duration::from_secs(120);

pub fn convert(bytes: &[u8]) -> Result<Converted, String> {
    if !bytes[..bytes.len().min(1024)]
        .windows(5)
        .any(|w| w == b"%PDF-")
    {
        return Err("not a PDF (it has no %PDF- header)".into());
    }
    let (tx, rx) = mpsc::channel();
    let owned = bytes.to_vec();
    // A deep page tree can overflow the default stack, and the parser panics on some inputs.
    let worker = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(move || {
            let _ = tx.send(extract(&owned));
        })
        .map_err(|e| format!("cannot be read ({e})"))?;
    match rx.recv_timeout(TIME_LIMIT) {
        Ok(result) => {
            let _ = worker.join();
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("took over {} s to read", TIME_LIMIT.as_secs()))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("cannot be read (the parser failed)".into())
        }
    }
}

fn extract(bytes: &[u8]) -> Result<Converted, String> {
    let read = |e: String| {
        let lower = e.to_lowercase();
        if ["password", "encrypt", "decrypt"]
            .iter()
            .any(|w| lower.contains(w))
        {
            "encrypted (password-protected)".to_string()
        } else {
            format!("cannot be read ({e})")
        }
    };
    let doc = Document::load_mem(bytes).map_err(|e| read(e.to_string()))?;
    if doc.is_encrypted() {
        return Err("encrypted (password-protected)".into());
    }
    let pages =
        pdf_extract::extract_text_from_mem_by_pages(bytes).map_err(|e| read(e.to_string()))?;
    let mut text = String::new();
    for page in pages {
        let page = tidy(&page);
        if !page.is_empty() {
            text.push_str(&page);
            text.push_str("\n\n");
        }
    }
    let info = info(&doc);
    let get = |key: &[u8]| info.as_ref().and_then(|i| i.get(key).ok()).and_then(string);
    Ok(Converted {
        text: if text.chars().any(char::is_alphanumeric) {
            text.trim().to_string()
        } else {
            String::new()
        },
        title: get(b"Title")
            .map(|t| doc::clip(t.trim(), doc::MAX_TITLE_CHARS))
            .filter(|t| !t.is_empty()),
        date: get(b"ModDate")
            .or_else(|| get(b"CreationDate"))
            .and_then(|d| pdf_date(&d)),
        prose: true,
    })
}

fn info(doc: &Document) -> Option<Dictionary> {
    let info = doc.trailer.get(b"Info").ok()?;
    match info {
        Object::Reference(id) => doc.get_dictionary(*id).ok().cloned(),
        Object::Dictionary(d) => Some(d.clone()),
        _ => None,
    }
}

/// A PDF string: UTF-16 with a byte order mark, or PDFDocEncoding (taken as Latin-1).
fn string(o: &Object) -> Option<String> {
    let Object::String(bytes, _) = o else {
        return None;
    };
    Some(if bytes.starts_with(&[0xfe, 0xff]) {
        let (pairs, _) = bytes[2..].as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|c| u16::from_be_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|b| char::from(*b)).collect()
    })
}

/// `D:20240514101500+00'00'` as `2024-05-14`.
fn pdf_date(s: &str) -> Option<String> {
    let d = s.strip_prefix("D:").unwrap_or(s).get(..8)?;
    doc::valid_date(&format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..8]))
}

/// Trailing spaces off, page-break characters out, no more than one blank line in a row.
fn tidy(page: &str) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    for line in page.replace('\u{c}', "\n").lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blanks += 1;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
            if blanks > 0 {
                out.push('\n');
            }
        }
        blanks = 0;
        out.push_str(line.trim_start());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_tidied() {
        assert_eq!(tidy("  a  \n\n\n\n b\n\u{c}"), "a\n\nb");
    }

    #[test]
    fn dates_and_strings() {
        assert_eq!(
            pdf_date("D:20240514101500+00'00'").as_deref(),
            Some("2024-05-14")
        );
        assert_eq!(pdf_date("garbage"), None);
        assert_eq!(
            string(&Object::string_literal(vec![
                0xfe, 0xff, 0x00, 0x41, 0x00, 0xde
            ]))
            .as_deref(),
            Some("AÞ")
        );
    }

    #[test]
    fn what_is_not_a_pdf_is_said_so() {
        assert!(convert(b"hello").unwrap_err().starts_with("not a PDF"));
        assert!(convert(b"%PDF-1.4\nnot really").is_err());
    }
}
