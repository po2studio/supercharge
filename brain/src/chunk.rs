//! Chunker v1: split at the strongest boundary available, falling back in order: headings, blank-line
//! paragraphs, lines, sentence ends, whitespace, a hard cut. About `target` tokens per chunk, about 10%
//! overlap when a section has to be cut, tiny pieces merged into their neighbours.

pub const VERSION: u32 = 1;

#[derive(Clone, Copy)]
pub struct Params {
    pub target: usize,
    pub overlap: usize,
    pub min: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            target: 400,
            overlap: 40,
            min: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Heading path, `A › B`, empty before the first heading.
    pub heading: String,
    pub text: String,
}

struct Piece {
    path: Vec<String>,
    text: String,
    tokens: usize,
}

/// Splits `text` (frontmatter already removed). `count` measures a string in model tokens.
pub fn chunk(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<Chunk> {
    let mut pieces = Vec::new();
    for (path, body) in sections(text) {
        for part in fit(&body, count, p) {
            let tokens = count(&part);
            pieces.push(Piece {
                path: path.clone(),
                text: part,
                tokens,
            });
        }
    }
    let mut merged: Vec<Piece> = Vec::new();
    for piece in pieces {
        if let Some(last) = merged.last_mut() {
            if (last.tokens < p.min || piece.tokens < p.min)
                && last.tokens + piece.tokens <= p.target
            {
                if piece.path != last.path {
                    if let Some(h) = piece.path.last() {
                        last.text.push_str("\n\n");
                        last.text.push_str(h);
                    }
                }
                last.text.push_str("\n\n");
                last.text.push_str(&piece.text);
                last.tokens += piece.tokens;
                continue;
            }
        }
        merged.push(piece);
    }
    merged
        .into_iter()
        .map(|m| Chunk {
            heading: m.path.join(" › "),
            text: m.text,
        })
        .collect()
}

/// Splits at ATX headings (outside code fences) into `(heading path, body)`, dropping empty bodies.
fn sections(text: &str) -> Vec<(Vec<String>, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut body = String::new();
    let mut fence: Option<String> = None;
    let flush =
        |stack: &[(usize, String)], body: &mut String, out: &mut Vec<(Vec<String>, String)>| {
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                out.push((
                    stack.iter().map(|(_, h)| h.clone()).collect(),
                    trimmed.to_string(),
                ));
            }
            body.clear();
        };
    for line in text.lines() {
        let stripped = line.trim_start();
        if let Some(marker) = &fence {
            if stripped.starts_with(marker.as_str()) {
                fence = None;
            }
        } else if stripped.starts_with("```") || stripped.starts_with("~~~") {
            fence = Some(stripped[..3].to_string());
        } else if let Some((level, title)) = heading(line) {
            flush(&stack, &mut body, &mut out);
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            stack.push((level, title));
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    flush(&stack, &mut body, &mut out);
    out
}

fn heading(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim();
    (!title.is_empty()).then(|| (hashes, title.to_string()))
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    Paragraph,
    Line,
    Sentence,
    Word,
    Cut,
}

impl Level {
    fn next(self) -> Option<Level> {
        match self {
            Level::Paragraph => Some(Level::Line),
            Level::Line => Some(Level::Sentence),
            Level::Sentence => Some(Level::Word),
            Level::Word => Some(Level::Cut),
            Level::Cut => None,
        }
    }

    fn separator(self) -> &'static str {
        match self {
            Level::Paragraph => "\n\n",
            Level::Line => "\n",
            _ => " ",
        }
    }
}

fn fit(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    split_at(text, Level::Paragraph, count, p)
}

fn split_at(text: &str, level: Level, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    if count(text) <= p.target {
        return vec![text.to_string()];
    }
    if level == Level::Cut {
        return hard_cut(text, count, p);
    }
    let parts = match level {
        Level::Paragraph => split_paragraphs(text),
        Level::Line => text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        Level::Sentence => sentences(text),
        _ => text.split_whitespace().map(String::from).collect(),
    };
    // One part means this boundary isn't there; try the next one down.
    if parts.len() <= 1 {
        return split_at(text, level.next().unwrap(), count, p);
    }
    let mut units = Vec::new();
    for part in parts {
        if count(&part) <= p.target {
            units.push(part);
        } else {
            units.extend(split_at(&part, level.next().unwrap(), count, p));
        }
    }
    pack(&units, level.separator(), count, p)
}

fn split_paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            if !cur.is_empty() {
                cur.push('\n');
            }
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        let end = matches!(c, '.' | '!' | '?' | '。' | '！' | '？');
        if end && chars.peek().is_none_or(|n| n.is_whitespace()) {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Greedy packing of units into chunks of at most `target` tokens. A chunk that follows another
/// starts with that chunk's last units, as far as `overlap` tokens allow.
fn pack(units: &[String], sep: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    let sizes: Vec<usize> = units.iter().map(|u| count(u)).collect();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < units.len() {
        let mut end = start;
        let mut total = 0;
        while end < units.len() && (end == start || total + sizes[end] <= p.target) {
            total += sizes[end];
            end += 1;
        }
        chunks.push(units[start..end].join(sep));
        if end >= units.len() {
            break;
        }
        // Back up over trailing units that fit the overlap, always moving forward overall.
        let mut next = end;
        let mut tail = 0;
        while next > start + 1
            && tail + sizes[next - 1] <= p.overlap
            && tail + sizes[next - 1] + sizes[end] <= p.target
        {
            tail += sizes[next - 1];
            next -= 1;
        }
        start = next;
    }
    chunks
}

/// A run with no usable boundary: cut by characters, in windows sized from its token density.
fn hard_cut(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let tokens = count(text).max(1);
    let mut window = (chars.len() * p.target / tokens).max(1);
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + window).min(chars.len());
        let mut piece: String = chars[start..end].iter().collect();
        while count(&piece) > p.target && end - start > 1 {
            window = (window * 9 / 10).max(1);
            end = (start + window).min(chars.len());
            piece = chars[start..end].iter().collect();
        }
        out.push(piece);
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> usize {
        s.split_whitespace().count()
    }

    fn quarter(s: &str) -> usize {
        s.chars().count().div_ceil(4)
    }

    fn small() -> Params {
        Params {
            target: 20,
            overlap: 4,
            min: 6,
        }
    }

    fn lorem(n: usize, tag: &str) -> String {
        (0..n)
            .map(|i| format!("{tag}{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(chunk("", &words, &small()).is_empty());
        assert!(chunk("  \n\n \n", &words, &small()).is_empty());
        assert!(chunk("# Only a heading\n", &words, &small()).is_empty());
    }

    #[test]
    fn a_short_document_is_one_chunk_under_its_heading_path() {
        let c = chunk(
            "# Policy\n\n## Pets\n\nNo dogs allowed in the office, ever, period.\n",
            &words,
            &small(),
        );
        assert_eq!(
            c,
            vec![Chunk {
                heading: "Policy › Pets".into(),
                text: "No dogs allowed in the office, ever, period.".into()
            }]
        );
    }

    #[test]
    fn headings_split_sections_that_are_big_enough() {
        let doc = format!("# A\n\n{}\n\n# B\n\n{}\n", lorem(10, "a"), lorem(10, "b"));
        let c = chunk(&doc, &words, &small());
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].heading.as_str(), c[1].heading.as_str()), ("A", "B"));
        assert!(c[1].text.starts_with("b0"));
    }

    #[test]
    fn sibling_headings_replace_each_other_in_the_path() {
        let doc = format!(
            "# A\n\n## One\n\n{}\n\n## Two\n\n{}\n",
            lorem(10, "x"),
            lorem(10, "y")
        );
        let c = chunk(&doc, &words, &small());
        assert_eq!(
            c.iter().map(|c| c.heading.as_str()).collect::<Vec<_>>(),
            ["A › One", "A › Two"]
        );
    }

    #[test]
    fn hashes_inside_code_fences_are_not_headings() {
        let doc = "# Setup\n\n```sh\n# install it\nmake install\n```\n\nThen run it twice to be sure it works fine.\n";
        let c = chunk(doc, &words, &small());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].heading, "Setup");
        assert!(c[0].text.contains("# install it"));
    }

    #[test]
    fn tiny_sections_merge_into_a_neighbour_and_keep_their_heading_text() {
        let doc = "# A\n\nshort one here\n\n# B\n\nshort two there\n";
        let c = chunk(doc, &words, &small());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].heading, "A");
        assert!(
            c[0].text.contains("B\n\nshort two there"),
            "{:?}",
            c[0].text
        );
    }

    #[test]
    fn paragraphs_pack_up_to_the_target_with_overlap() {
        let paras: Vec<String> = (0..6).map(|i| lorem(8, &format!("p{i}_"))).collect();
        let doc = paras.join("\n\n");
        let c = chunk(&doc, &words, &small());
        assert!(c.len() >= 3, "{c:?}");
        assert!(c.iter().all(|c| words(&c.text) <= 20));
        // Overlap: with 8-word paragraphs and a 4-token overlap nothing fits, so none repeats.
        let all = c
            .iter()
            .map(|c| c.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        for p in &paras {
            assert!(all.contains(p.as_str()));
        }
        // Small paragraphs do overlap by their last unit.
        let paras: Vec<String> = (0..12).map(|i| lorem(3, &format!("q{i}_"))).collect();
        let c = chunk(&paras.join("\n\n"), &words, &small());
        assert!(c.len() >= 2);
        let first_last = c[0].text.split("\n\n").last().unwrap().to_string();
        assert!(c[1].text.starts_with(&first_last), "{c:?}");
    }

    #[test]
    fn flat_text_with_no_boundaries_is_cut_into_pieces_that_fit() {
        let flat = lorem(100, "w");
        let c = chunk(&flat, &words, &small());
        assert!(c.len() >= 5);
        assert!(c.iter().all(|c| words(&c.text) <= 20), "{c:?}");
        let all = c
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for i in 0..100 {
            assert!(
                all.contains(&format!("w{i} ")) || all.ends_with(&format!("w{i}")),
                "lost w{i}"
            );
        }
    }

    #[test]
    fn sentences_are_the_next_boundary_after_lines() {
        let doc = (0..10)
            .map(|i| format!("Sentence number {i} says something. "))
            .collect::<String>();
        let c = chunk(&doc, &words, &small());
        assert!(c.len() >= 3);
        for chunk in &c {
            assert!(
                chunk.text.ends_with('.'),
                "cut mid-sentence: {:?}",
                chunk.text
            );
        }
    }

    #[test]
    fn a_run_with_no_whitespace_is_hard_cut() {
        let blob = "x".repeat(4000);
        let c = chunk(
            &blob,
            &quarter,
            &Params {
                target: 100,
                overlap: 10,
                min: 20,
            },
        );
        assert!(c.len() >= 10);
        assert!(c.iter().all(|c| quarter(&c.text) <= 100));
        assert_eq!(c.iter().map(|c| c.text.len()).sum::<usize>(), 4000);
    }

    #[test]
    fn every_chunk_fits_whatever_the_input() {
        let doc = format!(
            "# T\n\n{}\n\n## S\n\n{}\nline two {}\n\n{}",
            lorem(70, "a"),
            lorem(5, "b"),
            lorem(40, "c"),
            "z".repeat(300)
        );
        for c in chunk(
            &doc,
            &quarter,
            &Params {
                target: 50,
                overlap: 5,
                min: 10,
            },
        ) {
            assert!(
                quarter(&c.text) <= 50,
                "{} tokens: {:?}",
                quarter(&c.text),
                c.text
            );
        }
    }
}
