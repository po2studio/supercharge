//! A forgiving XML tokenizer for the parts of Office and OpenDocument files that hold text. Names
//! keep their prefix (`w:p`, `text:h`): those formats use the same prefixes everywhere.

use super::text::unescape;

#[derive(Debug, PartialEq)]
pub enum Tok<'a> {
    /// `<name attrs>`; `empty` for `<name attrs/>`.
    Open {
        name: &'a str,
        attrs: &'a str,
        empty: bool,
    },
    Close(&'a str),
    /// Text between tags, references not yet decoded.
    Text(&'a str),
}

pub struct Tokens<'a> {
    rest: &'a str,
}

pub fn tokens(xml: &str) -> Tokens<'_> {
    Tokens { rest: xml }
}

/// The end of the tag at the start of `s` (which begins with `<`): the index of its `>`, skipping
/// any `>` inside quoted attribute values.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Tok<'a>;

    fn next(&mut self) -> Option<Tok<'a>> {
        loop {
            if self.rest.is_empty() {
                return None;
            }
            let Some(rest) = self.rest.strip_prefix('<') else {
                let end = self.rest.find('<').unwrap_or(self.rest.len());
                let (text, tail) = self.rest.split_at(end);
                self.rest = tail;
                return Some(Tok::Text(text));
            };
            // A `<` that no tag starts with (`a < b`) is text.
            if !rest.starts_with(|c: char| c.is_alphabetic() || matches!(c, '/' | '!' | '?')) {
                let end = 1 + self.rest[1..].find('<').unwrap_or(self.rest.len() - 1);
                let (text, tail) = self.rest.split_at(end);
                self.rest = tail;
                return Some(Tok::Text(text));
            }
            if let Some(after) = rest.strip_prefix("!--") {
                self.rest = after.find("-->").map_or("", |i| &after[i + 3..]);
                continue;
            }
            if let Some(after) = rest.strip_prefix("![CDATA[") {
                let end = after.find("]]>").unwrap_or(after.len());
                let text = &after[..end];
                self.rest = after.get(end + 3..).unwrap_or("");
                return Some(Tok::Text(text));
            }
            let Some(end) = tag_end(rest) else {
                self.rest = "";
                return None;
            };
            let inner = &rest[..end];
            self.rest = &rest[end + 1..];
            if inner.starts_with('?') || inner.starts_with('!') {
                continue;
            }
            if let Some(name) = inner.strip_prefix('/') {
                return Some(Tok::Close(name.trim()));
            }
            let empty = inner.ends_with('/');
            let inner = inner.strip_suffix('/').unwrap_or(inner);
            let split = inner
                .find(|c: char| c.is_whitespace())
                .unwrap_or(inner.len());
            return Some(Tok::Open {
                name: &inner[..split],
                attrs: &inner[split..],
                empty,
            });
        }
    }
}

/// The value of attribute `name` in the attribute text of a tag.
pub fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = attrs[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = attrs[..at].chars().last().is_none_or(char::is_whitespace);
        let after = attrs[from..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        if !before_ok {
            continue;
        }
        let value = &after[1..];
        let end = value.find(quote)?;
        return Some(unescape(&value[..end]).into_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_text_and_attributes() {
        let toks: Vec<Tok> =
            tokens("<?xml version=\"1.0\"?><a x=\"1>2\" y='z'><!-- c --><b/>t&amp;u</a>").collect();
        assert_eq!(
            toks,
            vec![
                Tok::Open {
                    name: "a",
                    attrs: " x=\"1>2\" y='z'",
                    empty: false
                },
                Tok::Open {
                    name: "b",
                    attrs: "",
                    empty: true
                },
                Tok::Text("t&amp;u"),
                Tok::Close("a"),
            ]
        );
        assert_eq!(attr(" x=\"1>2\" y='z'", "y").as_deref(), Some("z"));
        assert_eq!(attr(" xy=\"1\" y=\"2\"", "y").as_deref(), Some("2"));
        assert_eq!(attr(" a=\"&lt;\"", "a").as_deref(), Some("<"));
        assert_eq!(attr(" a=\"1\"", "b"), None);
    }

    #[test]
    fn a_less_than_sign_that_starts_no_tag_is_text() {
        let toks: Vec<Tok> = tokens("<p>a < b and 2<3</p>").collect();
        assert_eq!(
            toks,
            vec![
                Tok::Open {
                    name: "p",
                    attrs: "",
                    empty: false
                },
                Tok::Text("a "),
                Tok::Text("< b and 2"),
                Tok::Text("<3"),
                Tok::Close("p"),
            ]
        );
    }

    #[test]
    fn a_truncated_document_ends_quietly() {
        assert_eq!(tokens("<a><b").count(), 1);
        assert_eq!(tokens("text only").count(), 1);
    }
}
