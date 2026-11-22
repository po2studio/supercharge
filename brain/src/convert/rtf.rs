//! RTF to plain text: control words are dropped, escapes decoded, and the groups that hold fonts,
//! styles, pictures and the like skipped.

use super::Converted;

/// Groups that are not the document's text.
const SKIPPED: [&str; 17] = [
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "header",
    "footer",
    "headerl",
    "headerr",
    "footerl",
    "footerr",
    "generator",
    "listtable",
    "listoverridetable",
    "rsidtbl",
    "themedata",
    "object",
];

#[derive(Clone, Copy)]
struct State {
    skip: bool,
    /// How many characters follow a `\uN` as a fallback for readers without Unicode.
    uc: usize,
}

pub fn convert(bytes: &[u8]) -> Result<Converted, String> {
    if !bytes.starts_with(b"{\\rtf") {
        return Err("not an RTF file".into());
    }
    let mut out = String::new();
    let mut stack: Vec<State> = Vec::new();
    let mut state = State { skip: false, uc: 1 };
    let mut skip_chars = 0;
    let mut i = 0;
    // The first control word of a group says whether it is one to skip.
    let mut group_start = false;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            b'{' => {
                stack.push(state);
                group_start = true;
                continue;
            }
            b'}' => {
                state = stack.pop().unwrap_or(state);
            }
            b'\\' => {
                let Some(&next) = bytes.get(i) else { break };
                i += 1;
                match next {
                    b'\\' | b'{' | b'}' => {
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else if !state.skip {
                            out.push(next as char);
                        }
                    }
                    b'\'' => {
                        let hex = bytes
                            .get(i..i + 2)
                            .and_then(|h| std::str::from_utf8(h).ok());
                        i += 2;
                        if skip_chars > 0 {
                            skip_chars -= 1;
                        } else if let (false, Some(Ok(v))) =
                            (state.skip, hex.map(|h| u8::from_str_radix(h, 16)))
                        {
                            out.push(
                                encoding_rs::WINDOWS_1252
                                    .decode_without_bom_handling(&[v])
                                    .0
                                    .chars()
                                    .next()
                                    .unwrap_or('?'),
                            );
                        }
                    }
                    b'*' => state.skip = true,
                    b'~' => {
                        if !state.skip {
                            out.push('\u{a0}');
                        }
                    }
                    b'_' => {
                        if !state.skip {
                            out.push('-');
                        }
                    }
                    c if c.is_ascii_alphabetic() => {
                        let start = i - 1;
                        while bytes.get(i).is_some_and(u8::is_ascii_alphabetic) {
                            i += 1;
                        }
                        let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
                        let num_start = i;
                        if bytes.get(i) == Some(&b'-') {
                            i += 1;
                        }
                        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                            i += 1;
                        }
                        let num: Option<i32> = std::str::from_utf8(&bytes[num_start..i])
                            .ok()
                            .and_then(|n| n.parse().ok());
                        if bytes.get(i) == Some(&b' ') {
                            i += 1;
                        }
                        if group_start && SKIPPED.contains(&word) {
                            state.skip = true;
                        }
                        if state.skip {
                            group_start = false;
                            continue;
                        }
                        match word {
                            "par" | "sect" | "page" => out.push_str("\n\n"),
                            "line" => out.push('\n'),
                            "tab" => out.push(' '),
                            "cell" => out.push_str(" | "),
                            "row" => out.push('\n'),
                            "emdash" => out.push('\u{2014}'),
                            "endash" => out.push('\u{2013}'),
                            "bullet" => out.push('\u{2022}'),
                            "uc" => state.uc = num.unwrap_or(1).max(0) as usize,
                            "u" => {
                                if let Some(n) = num {
                                    // Negative numbers are the upper half of a 16-bit value.
                                    let code = if n < 0 { n + 65536 } else { n } as u32;
                                    out.push(char::from_u32(code).unwrap_or('?'));
                                    skip_chars = state.uc;
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => {}
            _ => {
                if skip_chars > 0 {
                    skip_chars -= 1;
                } else if !state.skip {
                    out.push(char::from(b));
                }
            }
        }
        group_start = false;
    }
    let mut text = String::new();
    let mut blanks = 0;
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            blanks += 1;
            continue;
        }
        if blanks > 0 && !text.is_empty() {
            text.push_str("\n\n");
        } else if !text.is_empty() {
            text.push('\n');
        }
        blanks = 0;
        text.push_str(line);
    }
    Ok(Converted {
        text,
        title: None,
        date: None,
        prose: true,
        meta: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_comes_out_and_fonts_and_escapes_are_handled() {
        let rtf = br"{\rtf1\ansi\ansicpg1252{\fonttbl{\f0 Arial;}}{\colortbl;\red0;}{\*\generator Word;}\pard Orlof \b er\b0  25 \'fe\'f3r dagar.\par Next \u8364? line\line two\par}";
        let c = convert(rtf).unwrap();
        assert_eq!(c.text, "Orlof er 25 þór dagar.\n\nNext € line\ntwo");
    }

    #[test]
    fn something_else_is_not_rtf() {
        assert!(convert(b"plain").is_err());
    }
}
