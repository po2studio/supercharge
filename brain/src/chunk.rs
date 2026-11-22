//! Chunker v2: split at the strongest boundary available, falling back in order: headings, blank-line
//! paragraphs, lines, sentence ends, whitespace, a hard cut. About `target` tokens per chunk, about 10%
//! overlap when a section has to be cut, tiny pieces merged into their neighbours.
//!
//! Some text has a boundary of its own that comes before the paragraph: a transcript splits by
//! speaker turn, a table by row groups with its header row repeated in each, and a fenced code
//! block is never cut at a blank line inside it.

pub const VERSION: u32 = 2;

#[derive(Clone, Copy)]
pub struct Params {
    pub target: usize,
    pub overlap: usize,
    pub min: usize,
    /// Whether `#` lines are headings: not in source code, YAML or logs.
    pub headings: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            target: 400,
            overlap: 40,
            min: 60,
            headings: true,
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
    for (path, body) in sections(text, p.headings) {
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
            // A heading that changes is written into the merged text, so it counts too.
            let heading = (piece.path != last.path)
                .then(|| piece.path.last())
                .flatten();
            let heading_tokens = heading.map_or(0, |h| count(h));
            if (last.tokens < p.min || piece.tokens < p.min)
                && last.tokens + piece.tokens + heading_tokens <= p.target
            {
                if let Some(h) = heading {
                    last.text.push_str("\n\n");
                    last.text.push_str(h);
                }
                last.text.push_str("\n\n");
                last.text.push_str(&piece.text);
                last.tokens += piece.tokens + heading_tokens;
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
/// Without `headings` the text is one section.
fn sections(text: &str, headings: bool) -> Vec<(Vec<String>, String)> {
    if !headings {
        let body = text.trim();
        return if body.is_empty() {
            Vec::new()
        } else {
            vec![(Vec::new(), body.to_string())]
        };
    }
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
    (!title.is_empty()).then(|| (hashes, crate::doc::clip(title, crate::doc::MAX_TITLE_CHARS)))
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Level {
    /// A speaker's turn in a transcript.
    Turn,
    Paragraph,
    Line,
    Sentence,
    Word,
    Cut,
}

impl Level {
    fn next(self) -> Option<Level> {
        match self {
            Level::Turn => Some(Level::Paragraph),
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
            Level::Turn | Level::Line => "\n",
            _ => " ",
        }
    }
}

/// No natural text has more than this many bytes to a token, so a longer string is over the
/// target without asking the tokenizer, whose time grows with what it is handed.
const MAX_BYTES_PER_TOKEN: usize = 16;

fn fits(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> bool {
    text.len() <= p.target * MAX_BYTES_PER_TOKEN && count(text) <= p.target
}

fn fit(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    let start = if is_transcript(text) {
        Level::Turn
    } else {
        Level::Paragraph
    };
    split_at(text, start, count, p)
}

fn split_at(text: &str, level: Level, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    if fits(text, count, p) {
        return vec![text.to_string()];
    }
    if level == Level::Cut {
        return hard_cut(text, count, p);
    }
    if level == Level::Paragraph && is_table(text) {
        if let Some(groups) = table_groups(text, count, p) {
            return groups;
        }
    }
    let parts = match level {
        Level::Turn => turns(text),
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
        if fits(&part, count, p) {
            units.push(part);
        } else if level == Level::Turn {
            units.extend(split_turn(&part, count, p));
        } else if level == Level::Paragraph && is_table(&part) {
            match table_groups(&part, count, p) {
                Some(groups) => units.extend(groups),
                None => units.extend(split_at(&part, Level::Line, count, p)),
            }
        } else {
            units.extend(split_at(&part, level.next().unwrap(), count, p));
        }
    }
    pack(&units, level.separator(), count, p)
}

/// Blank-line paragraphs; a fenced code block is one, whatever blank lines it has.
fn split_paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let stripped = line.trim_start();
        if fence.is_none() && line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
        match fence {
            Some(marker) if stripped.starts_with(marker) => fence = None,
            Some(_) => {}
            None if stripped.starts_with("```") => fence = Some("```"),
            None if stripped.starts_with("~~~") => fence = Some("~~~"),
            None => {}
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// A Markdown pipe table: at least three lines, each starting with `|`.
fn is_table(text: &str) -> bool {
    let mut lines = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        if !line.trim_start().starts_with('|') {
            return false;
        }
        lines += 1;
    }
    lines >= 3
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Row groups of a table that fill a chunk, each with the table's header row (and its rule)
/// repeated. `None` when the header alone would take too much of a chunk.
fn table_groups(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Option<Vec<String>> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let head = if lines.get(1).is_some_and(|l| is_rule(l)) {
        2
    } else {
        1
    };
    let header = lines[..head].join("\n");
    let budget = p.target.saturating_sub(count(&header));
    if budget < p.target / 2 || lines.len() <= head {
        return None;
    }
    let mut groups = Vec::new();
    let mut rows: Vec<&str> = Vec::new();
    let mut used = 0;
    let flush = |rows: &mut Vec<&str>, groups: &mut Vec<String>| {
        if !rows.is_empty() {
            groups.push(format!("{header}\n{}", rows.join("\n")));
            rows.clear();
        }
    };
    for row in &lines[head..] {
        let tokens = count(row);
        if tokens > budget {
            // One row too long for a chunk: its text is cut, and each piece keeps the header.
            flush(&mut rows, &mut groups);
            used = 0;
            let q = Params {
                target: budget,
                ..*p
            };
            for piece in split_at(row, Level::Sentence, count, &q) {
                groups.push(format!("{header}\n{piece}"));
            }
            continue;
        }
        if used + tokens > budget {
            flush(&mut rows, &mut groups);
            used = 0;
        }
        rows.push(row);
        used += tokens;
    }
    flush(&mut rows, &mut groups);
    Some(groups)
}

/// `Name:` at the start of a line of a transcript, with an optional timestamp before it and bold
/// markers around it: the label with its colon, as written.
fn speaker(line: &str) -> Option<&str> {
    let line = line.trim_start();
    let mut rest = line;
    if let Some(r) = rest.strip_prefix(['[', '(']) {
        let end = r.find([']', ')'])?;
        if r[..end]
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ':' | '.'))
        {
            rest = r[end + 1..].trim_start();
        }
    } else {
        let stamp = rest
            .find(|c: char| !(c.is_ascii_digit() || c == ':' || c == '.'))
            .unwrap_or(rest.len());
        if stamp >= 4 && rest[..stamp].contains(':') {
            rest = rest[stamp..].trim_start_matches([' ', '-']);
        }
    }
    let body = rest.trim_start_matches('*');
    let colon = body.find(':')?;
    let label = body[..colon].trim_end_matches('*').trim();
    let first = label.chars().next()?;
    let plausible = (first.is_uppercase() || first.is_ascii_digit())
        && label.chars().count() <= 40
        && label.split_whitespace().count() <= 4
        && label
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '\'' | '’' | '-' | '_'));
    let after = body[colon + 1..].trim_start_matches('*');
    let spaced = after.is_empty() || after.starts_with(char::is_whitespace);
    (plausible && spaced).then(|| &line[..line.len() - body.len() + colon + 1])
}

/// Whether the text is a transcript: many lines that start with a speaker's name, at least one
/// speaker talking more than once, and speakers making up much of the text.
fn is_transcript(text: &str) -> bool {
    let mut lines = 0;
    let mut turns = 0;
    let mut speakers: Vec<&str> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        lines += 1;
        if let Some(s) = speaker(line) {
            turns += 1;
            let name = s.trim_end_matches(':').trim_matches('*').trim();
            if !speakers.contains(&name) {
                speakers.push(name);
            }
        }
    }
    turns >= 4 && speakers.len() >= 2 && speakers.len() < turns && turns * 5 >= lines * 2
}

/// The turns of a transcript; what comes before the first speaker is a turn of its own.
fn turns(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        match out.last_mut() {
            Some(last) if speaker(line).is_none() => {
                last.push('\n');
                last.push_str(line.trim());
            }
            _ => out.push(line.trim().to_string()),
        }
    }
    out
}

/// A turn too long for a chunk: cut at sentences, each piece starting with the speaker's name.
fn split_turn(turn: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    let Some(label) = speaker(turn) else {
        return split_at(turn, Level::Paragraph, count, p);
    };
    let rest = &turn[label.len()..];
    let q = Params {
        target: p.target.saturating_sub(count(label)).max(p.target / 2),
        ..*p
    };
    split_at(rest.trim(), Level::Sentence, count, &q)
        .into_iter()
        .map(|piece| format!("{label} {piece}"))
        .collect()
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

/// A run with no usable boundary: cut by characters, in windows sized from the token density of
/// its first `target` characters (never from the whole run, which may be megabytes long).
fn hard_cut(text: &str, count: &dyn Fn(&str) -> usize, p: &Params) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let sample_len = chars.len().min(p.target);
    let sample: String = chars[..sample_len].iter().collect();
    let tokens = count(&sample).max(1);
    let mut window = (sample_len * p.target / tokens).clamp(1, p.target * 8);
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
            headings: true,
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
    fn a_huge_heading_is_cut_in_the_heading_path() {
        let c = chunk(
            &format!("# {}\n\nbody text here\n", "h".repeat(9000)),
            &words,
            &small(),
        );
        assert_eq!(c[0].heading.chars().count(), crate::doc::MAX_TITLE_CHARS);
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
    fn a_merged_chunk_counts_the_heading_it_swallows() {
        // 19 words, then one word under a ten-word heading: merging would make 30 words.
        let doc = format!(
            "# A\n\n{}\n\n# {}\n\nlast\n",
            lorem(19, "a"),
            lorem(10, "h")
        );
        let c = chunk(&doc, &words, &small());
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c.iter().all(|c| words(&c.text) <= 20), "{c:?}");
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
                ..Params::default()
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
                ..Params::default()
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

    /// What the real tokenizer costs: time grows with the length of what it is handed, and it
    /// reports at most 512 tokens however long the text is.
    fn measured(
        metric: fn(&str) -> usize,
        biggest: &std::cell::Cell<usize>,
        total: &std::cell::Cell<usize>,
        s: &str,
    ) -> usize {
        biggest.set(biggest.get().max(s.len()));
        total.set(total.get() + s.len());
        metric(s).min(512)
    }

    #[test]
    fn a_huge_text_with_no_boundaries_is_never_measured_whole() {
        let (biggest, total) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
        let count = |s: &str| measured(quarter, &biggest, &total, s);
        let p = Params::default();
        let blob = "x".repeat(4_000_000);
        let c = chunk(&blob, &count, &p);
        assert!(c.iter().all(|c| quarter(&c.text) <= p.target));
        assert_eq!(c.iter().map(|c| c.text.len()).sum::<usize>(), 4_000_000);
        assert!(
            biggest.get() <= p.target * 16,
            "measured a {}-byte string",
            biggest.get()
        );
        assert!(
            total.get() <= 8 * 4_000_000,
            "measured {} bytes",
            total.get()
        );
    }

    #[test]
    fn a_huge_text_with_words_is_measured_in_small_pieces_too() {
        let (biggest, total) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
        let count = |s: &str| measured(words, &biggest, &total, s);
        let p = Params::default();
        let text = "word ".repeat(200_000);
        let c = chunk(&text, &count, &p);
        assert!(c.iter().all(|c| words(&c.text) <= p.target));
        assert!(biggest.get() <= p.target * 16, "{}", biggest.get());
    }

    #[test]
    fn a_table_is_cut_into_row_groups_that_each_repeat_the_header() {
        let mut table = String::from("| id | name |\n|---|---|\n");
        for i in 0..30 {
            table.push_str(&format!("| {i} | name{i} |\n"));
        }
        let c = chunk(&table, &words, &small());
        assert!(c.len() > 2, "{c:?}");
        for chunk in &c {
            assert!(
                chunk.text.starts_with("| id | name |\n|---|---|\n| "),
                "{:?}",
                chunk.text
            );
            assert!(words(&chunk.text) <= 20);
        }
        // Every row is somewhere, once.
        let all: String = c
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for i in 0..30 {
            assert_eq!(all.matches(&format!("| {i} | name{i} |")).count(), 1);
        }
    }

    #[test]
    fn a_table_that_fits_stays_whole_and_a_table_among_prose_is_cut_alone() {
        let c = chunk("| a | b |\n|---|---|\n| 1 | 2 |\n", &words, &small());
        assert_eq!(c.len(), 1);
        let doc = format!(
            "{}\n\n| a | b |\n|---|---|\n{}",
            lorem(15, "p"),
            "| 1 | 2 |\n".repeat(30)
        );
        let c = chunk(&doc, &words, &small());
        assert!(c[0].text.starts_with("p0 "));
        assert!(c.iter().skip(1).all(|c| c.text.contains("| a | b |")));
    }

    #[test]
    fn one_row_too_long_for_a_chunk_is_cut_and_each_piece_keeps_the_header() {
        let doc = format!(
            "| a | b |\n|---|---|\n| short | row |\n| {} | {} |\n",
            lorem(40, "x"),
            lorem(40, "y")
        );
        let c = chunk(&doc, &words, &small());
        assert!(c.len() >= 4, "{c:?}");
        assert!(
            c.iter()
                .all(|c| c.text.starts_with("| a | b |") && words(&c.text) <= 20),
            "{c:?}"
        );
    }

    const TALK: &str = "Anna: Welcome everyone to the weekly sync.\nBjorn: Thanks. The launch moves to May.\nAnna: Why May and not April?\nBjorn: The vendor contract is not signed until the second week of April, so we cannot start before then.\nCarla: I can confirm the contract is with legal.\nAnna: Fine, May it is.\n";

    #[test]
    fn a_transcript_is_cut_by_speaker_turn() {
        assert!(is_transcript(TALK));
        assert!(!is_transcript(
            "Note: one\nAction: two\nSome prose here.\nMore prose.\n"
        ));
        let c = chunk(
            TALK,
            &words,
            &Params {
                target: 25,
                overlap: 0,
                min: 5,
                ..Params::default()
            },
        );
        assert!(c.len() >= 2, "{c:?}");
        for chunk in &c {
            for line in chunk.text.lines() {
                assert!(
                    speaker(line).is_some(),
                    "a chunk starts or continues mid-turn: {line:?}"
                );
            }
        }
    }

    #[test]
    fn a_long_turn_is_cut_at_sentences_and_each_piece_names_the_speaker() {
        let long = (0..12)
            .map(|i| format!("Sentence number {i} says something."))
            .collect::<Vec<_>>()
            .join(" ");
        let doc = format!("Anna: Hello.\nBjorn: {long}\nAnna: Bye.\nBjorn: Ok.\nAnna: Done.\n");
        let c = chunk(&doc, &words, &small());
        let pieces: Vec<&Chunk> = c
            .iter()
            .filter(|c| c.text.contains("Sentence number"))
            .collect();
        assert!(pieces.len() >= 3, "{c:?}");
        assert!(
            pieces
                .iter()
                .all(|c| c.text.trim_start().starts_with("Bjorn:") || c.text.contains("\nBjorn:")),
            "{pieces:?}"
        );
    }

    #[test]
    fn speaker_labels_with_timestamps_and_bold_are_recognised() {
        assert_eq!(
            speaker("[00:01:23] Anna Jonsdottir: hi"),
            Some("[00:01:23] Anna Jonsdottir:")
        );
        assert_eq!(speaker("**Anna:** hi"), Some("**Anna:"));
        assert_eq!(speaker("12:30 Bjorn: hi"), Some("12:30 Bjorn:"));
        assert_eq!(speaker("http://example.com is up"), None);
        assert_eq!(speaker("see: below"), None);
        assert_eq!(
            speaker("A very long sentence that ends with a colon and goes on: yes"),
            None
        );
    }

    #[test]
    fn a_code_block_is_kept_whole_and_code_files_have_no_headings() {
        let code = "```python\nprint(1)\n\nprint(2)\n```";
        let doc = format!("{}\n\n{code}\n\n{}", lorem(14, "a"), lorem(14, "b"));
        let c = chunk(&doc, &words, &small());
        assert!(c.iter().any(|c| c.text.contains(code)), "{c:?}");
        let p = Params {
            headings: false,
            ..small()
        };
        let c = chunk("# not a heading\nx = 1\n", &words, &p);
        assert_eq!((c.len(), c[0].heading.as_str()), (1, ""));
        assert!(c[0].text.starts_with("# not a heading"));
    }
}
