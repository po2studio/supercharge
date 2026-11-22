//! HTML to Markdown-ish text: headings keep their `#`, lists their `-`, tables become pipe
//! tables, `<pre>` a fenced block. Scripts, styles and comments are dropped.

use super::{
    text::{decode, unescape},
    xml::{attr, tokens, Tok},
    Converted,
};
use crate::doc;

/// Elements whose content is not text for a reader.
const HIDDEN: [&str; 5] = ["script", "style", "noscript", "template", "svg"];
/// A blank line before and after.
const BLOCKS: [&str; 22] = [
    "p",
    "ul",
    "ol",
    "dl",
    "table",
    "blockquote",
    "section",
    "article",
    "aside",
    "header",
    "footer",
    "nav",
    "main",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "details",
    "hr",
    "address",
    "body",
    "html",
];
/// A line break before and after.
const LINES: [&str; 7] = ["div", "li", "dt", "dd", "tr", "br", "summary"];

struct Out {
    s: String,
    pre: bool,
    /// Inside a table cell, where every break is a space.
    cell: usize,
}

impl Out {
    fn newline(&mut self) {
        if self.cell > 0 {
            self.space();
            return;
        }
        let trimmed = self.s.trim_end_matches(' ').len();
        self.s.truncate(trimmed);
        if !self.s.is_empty() && !self.s.ends_with('\n') {
            self.s.push('\n');
        }
    }

    fn blank(&mut self) {
        if self.cell > 0 {
            self.space();
            return;
        }
        self.newline();
        if !self.s.is_empty() && !self.s.ends_with("\n\n") {
            self.s.push('\n');
        }
    }

    fn space(&mut self) {
        if !self.s.is_empty() && !self.s.ends_with([' ', '\n']) {
            self.s.push(' ');
        }
    }

    fn text(&mut self, t: &str) {
        if self.pre {
            self.s.push_str(t);
            return;
        }
        if t.starts_with(char::is_whitespace) {
            self.space();
        }
        for (i, word) in t.split_whitespace().enumerate() {
            if i > 0 {
                self.space();
            }
            if self.cell > 0 {
                self.s.push_str(&word.replace('|', "\\|"));
            } else {
                self.s.push_str(word);
            }
        }
        if t.ends_with(char::is_whitespace) {
            self.space();
        }
    }
}

/// The charset a page declares in its first bytes, if any.
fn declared(bytes: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_lowercase();
    let at = head.find("charset=")? + "charset=".len();
    let label: String = head[at..]
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
        .collect();
    encoding_rs::Encoding::for_label(label.as_bytes())
}

pub fn convert(bytes: &[u8]) -> Converted {
    let source = match declared(bytes) {
        Some(enc) if std::str::from_utf8(bytes).is_err() => {
            enc.decode_without_bom_handling(bytes).0
        }
        _ => decode(bytes),
    };
    to_text(&source)
}

pub fn to_text(html: &str) -> Converted {
    let mut out = Out {
        s: String::new(),
        pre: false,
        cell: 0,
    };
    let mut title = None;
    let mut date = None;
    let mut hidden: Option<String> = None;
    let mut in_title = false;
    let mut raw_title = String::new();
    let mut row_open = false;
    let mut first_heading: Option<String> = None;
    let mut heading_at: Option<usize> = None;
    for tok in tokens(html) {
        match tok {
            Tok::Open { name, attrs, empty } => {
                let name = name.to_ascii_lowercase();
                if hidden.is_some() {
                    continue;
                }
                if name == "title" {
                    in_title = !empty;
                    continue;
                }
                if name == "meta" {
                    let key = attr(attrs, "name")
                        .or_else(|| attr(attrs, "property"))
                        .unwrap_or_default()
                        .to_lowercase();
                    if date.is_none()
                        && matches!(
                            key.as_str(),
                            "date"
                                | "dcterms.date"
                                | "dc.date"
                                | "article:modified_time"
                                | "article:published_time"
                                | "last-modified"
                        )
                    {
                        date = attr(attrs, "content").and_then(|c| doc::valid_date(c.trim()));
                    }
                    continue;
                }
                if HIDDEN.contains(&name.as_str()) {
                    if !empty {
                        hidden = Some(name);
                    }
                    continue;
                }
                match name.as_str() {
                    "pre" => {
                        out.blank();
                        out.s.push_str("```\n");
                        out.pre = true;
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        out.blank();
                        let level = usize::from(name.as_bytes()[1] - b'0');
                        out.s.push_str(&"#".repeat(level));
                        out.s.push(' ');
                        heading_at = Some(out.s.len());
                    }
                    "li" => {
                        out.newline();
                        out.s.push_str("- ");
                    }
                    "tr" => {
                        out.newline();
                        out.s.push('|');
                        row_open = true;
                    }
                    "td" | "th" => {
                        if row_open {
                            out.s.push(' ');
                            out.cell += 1;
                        }
                    }
                    "img" => {
                        if let Some(alt) = attr(attrs, "alt").filter(|a| !a.trim().is_empty()) {
                            out.text(&format!(" {alt} "));
                        }
                    }
                    _ if BLOCKS.contains(&name.as_str()) => out.blank(),
                    _ if LINES.contains(&name.as_str()) => out.newline(),
                    _ => {}
                }
            }
            Tok::Close(name) => {
                let name = name.to_ascii_lowercase();
                if let Some(h) = &hidden {
                    if *h == name {
                        hidden = None;
                    }
                    continue;
                }
                if name == "title" {
                    in_title = false;
                    continue;
                }
                match name.as_str() {
                    "pre" => {
                        out.pre = false;
                        out.newline();
                        out.s.push_str("```");
                        out.blank();
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        if let (None, Some(at)) = (&first_heading, heading_at) {
                            first_heading = Some(out.s[at..].trim().to_string());
                        }
                        heading_at = None;
                        out.blank();
                    }
                    "td" | "th" => {
                        if row_open && out.cell > 0 {
                            out.cell -= 1;
                            let trimmed = out.s.trim_end().len();
                            out.s.truncate(trimmed);
                            out.s.push_str(" |");
                        }
                    }
                    "tr" => row_open = false,
                    _ if BLOCKS.contains(&name.as_str()) => out.blank(),
                    _ if LINES.contains(&name.as_str()) => out.newline(),
                    _ => {}
                }
            }
            Tok::Text(t) => {
                if hidden.is_some() {
                    continue;
                }
                let t = unescape(t);
                if in_title {
                    raw_title.push_str(&t);
                } else {
                    out.text(&t);
                }
            }
        }
    }
    if out.pre {
        out.s.push_str("\n```");
    }
    if !raw_title.trim().is_empty() {
        title = Some(doc::clip(
            &raw_title.split_whitespace().collect::<Vec<_>>().join(" "),
            doc::MAX_TITLE_CHARS,
        ));
    }
    let title = title.or_else(|| first_heading.map(|h| doc::clip(&h, doc::MAX_TITLE_CHARS)));
    Converted {
        text: out.s.trim().to_string(),
        title: title.filter(|t| !t.is_empty()),
        date,
        prose: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_keeps_its_structure_and_drops_scripts_and_styles() {
        let page = "<html><head><title>Sala &amp; skil</title><meta name=\"date\" content=\"2024-05-14T10:00:00Z\"><style>p{}</style></head>\
            <body><h1>Skil</h1><p>First   paragraph\nwith <b>bold</b> text.</p><script>var x = '<p>';</script>\
            <ul><li>one</li><li>two</li></ul><pre>a\n  b</pre><p>&thorn;&oacute;r &#8211; end</p></body></html>";
        let c = to_text(page);
        assert_eq!(c.title.as_deref(), Some("Sala & skil"));
        assert_eq!(c.date.as_deref(), Some("2024-05-14"));
        assert_eq!(
            c.text,
            "# Skil\n\nFirst paragraph with bold text.\n\n- one\n- two\n\n```\na\n  b\n```\n\nþór – end"
        );
    }

    #[test]
    fn tables_become_pipe_rows_with_every_cell_on_its_line() {
        let c = to_text(
            "<table><tr><th>Name</th><th>Role</th></tr><tr><td>Anna</td><td><p>Chair</p><p>CFO</p></td></tr></table>",
        );
        assert_eq!(c.text, "| Name | Role |\n| Anna | Chair CFO |");
    }

    #[test]
    fn a_declared_charset_is_honoured() {
        let bytes = b"<meta charset=\"iso-8859-1\"><p>\xfe\xf3r</p>";
        assert_eq!(convert(bytes).text, "þór");
    }

    #[test]
    fn the_first_heading_is_the_title_when_there_is_none() {
        assert_eq!(to_text("<h2>Only</h2>x").title.as_deref(), Some("Only"));
    }
}
