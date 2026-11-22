//! Bytes to text, and the character references of HTML and XML.

use std::borrow::Cow;

/// Text from the bytes of a file: UTF-8, UTF-16 when a byte order mark says so, and otherwise
/// Windows-1252, the encoding an old export of Icelandic or French text most likely has.
pub fn decode(bytes: &[u8]) -> Cow<'_, str> {
    if let Some((encoding, bom)) = encoding_rs::Encoding::for_bom(bytes) {
        return Cow::Owned(
            encoding
                .decode_without_bom_handling(&bytes[bom..])
                .0
                .into_owned(),
        );
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => Cow::Owned(
            encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(bytes)
                .0
                .into_owned(),
        ),
    }
}

/// The HTML 4 Latin-1 entities, in the order of the characters from U+00A0.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
    "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute",
    "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve", "Oacute",
    "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute",
    "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil",
    "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde",
    "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc",
    "uuml", "yacute", "thorn", "yuml",
];

fn named(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "ndash" => Some('\u{2013}'),
        "mdash" => Some('\u{2014}'),
        "lsquo" => Some('\u{2018}'),
        "rsquo" => Some('\u{2019}'),
        "ldquo" => Some('\u{201c}'),
        "rdquo" => Some('\u{201d}'),
        "hellip" => Some('\u{2026}'),
        "bull" => Some('\u{2022}'),
        "euro" => Some('\u{20ac}'),
        "trade" => Some('\u{2122}'),
        "larr" => Some('\u{2190}'),
        "rarr" => Some('\u{2192}'),
        _ => LATIN1
            .iter()
            .position(|n| *n == name)
            .and_then(|i| char::from_u32(0xa0 + i as u32)),
    }
}

/// `&amp;`, `&eacute;`, `&#233;` and `&#xE9;` as the characters they stand for; anything else
/// stays as written.
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        // A reference is short; a lone `&` followed by a long run is just an ampersand.
        let end = rest
            .char_indices()
            .take(12)
            .find(|(_, c)| *c == ';')
            .map(|(i, _)| i);
        let decoded = end.and_then(|end| {
            let name = &rest[1..end];
            let c = match name.strip_prefix('#') {
                Some(num) => {
                    let code = match num.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok(),
                        None => num.parse().ok(),
                    };
                    code.and_then(char::from_u32)
                }
                None => named(name),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// A table cell as one line of a Markdown table row.
pub fn cell(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.replace('|', "\\|")
}

/// A Markdown table: the first row is the header.
pub fn table(rows: &[Vec<String>]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let line = |row: &[String]| {
        let mut s = String::from("|");
        for i in 0..width {
            s.push(' ');
            s.push_str(&cell(row.get(i).map_or("", String::as_str)));
            s.push_str(" |");
        }
        s
    };
    let mut out = line(&rows[0]);
    out.push('\n');
    out.push('|');
    out.push_str(&"---|".repeat(width));
    for row in &rows[1..] {
        out.push('\n');
        out.push_str(&line(row));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_utf16_and_latin1_are_all_read() {
        assert_eq!(decode("þórður".as_bytes()), "þórður");
        assert_eq!(decode(b"caf\xe9 \xfe\xf3r"), "café þór");
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain("Orlof".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode(&utf16), "Orlof");
    }

    #[test]
    fn references_become_characters_and_junk_stays() {
        assert_eq!(
            unescape("a &amp; b &eacute;&thorn;&#243;&#xF0; &nbsp;x"),
            "a & b éþóð \u{a0}x"
        );
        assert_eq!(
            unescape("AT&T and &unknown; & &#xZZ;"),
            "AT&T and &unknown; & &#xZZ;"
        );
    }

    #[test]
    fn a_table_has_a_header_a_rule_and_pipes_escaped() {
        let rows = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["x|y".to_string(), "line\nbreak".to_string()],
        ];
        assert_eq!(
            table(&rows),
            "| a | b |\n|---|---|\n| x\\|y | line break |\n"
        );
    }
}
