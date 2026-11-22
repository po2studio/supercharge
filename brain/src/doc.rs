//! What a text file says about itself: frontmatter, title, date, and what it says about being
//! decided, replaced or amended.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// What a decision's frontmatter, or an ADR's status line, says about it. Empty for most files.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// `business`, `product` or `process`.
    pub kind: Option<String>,
    pub project: Option<String>,
    /// `proposed`, `accepted` or `superseded`.
    pub status: Option<String>,
    /// Names of the decisions this one replaces.
    pub supersedes: Vec<String>,
    /// The name of the decision that replaced this one.
    pub superseded_by: Option<String>,
    /// The ADRs that amend this one, from its status line.
    pub amended_by: Vec<String>,
}

impl Meta {
    pub fn is_empty(&self) -> bool {
        *self == Meta::default()
    }
}

pub struct Parsed<'a> {
    pub body: &'a str,
    pub title: Option<String>,
    pub date: Option<String>,
    pub meta: Meta,
}

/// Titles and headings go into every chunk's header; a line of megabytes must not.
pub const MAX_TITLE_CHARS: usize = 200;

pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// A frontmatter value: quotes removed (with their escapes: `\"` in double quotes, `''` in
/// single), and a trailing ` # comment` cut off, as YAML does.
fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        let Some(inner) = v.strip_prefix(q) else {
            continue;
        };
        let mut out = String::new();
        let mut chars = inner.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if q == '"' => match chars.next() {
                    Some(e @ ('"' | '\\')) => out.push(e),
                    Some(e) => {
                        out.push('\\');
                        out.push(e);
                    }
                    None => out.push('\\'),
                },
                c if c == q && q == '\'' && chars.peek() == Some(&q) => {
                    out.push(q);
                    chars.next();
                }
                c if c == q => return out,
                c => out.push(c),
            }
        }
        // Never closed: the text as written.
        return v.to_string();
    }
    let v = match v.find(" #") {
        Some(i) => &v[..i],
        None if v.starts_with('#') => "",
        None => v,
    };
    v.trim().to_string()
}

/// A frontmatter list: `[a, b]` on the line, or `- a` lines after an empty value.
fn list(inline: &str, following: &[&str]) -> Vec<String> {
    let inline = inline.trim();
    let items: Vec<&str> = if let Some(inner) = inline.strip_prefix('[') {
        inner.trim_end().trim_end_matches(']').split(',').collect()
    } else if inline.is_empty() {
        following
            .iter()
            .map_while(|l| l.trim_start().strip_prefix("- "))
            .collect()
    } else {
        vec![inline]
    };
    items
        .into_iter()
        .map(unquote)
        .filter(|i| !i.is_empty())
        .collect()
}

/// Splits a leading `---` frontmatter block off and reads `title`, `date` and what a decision
/// says about itself from it. Without frontmatter the title is the first `#` heading, if any.
pub fn parse(text: &str) -> Parsed<'_> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut title = None;
    let mut date = None;
    let mut meta = Meta::default();
    let mut body = text;
    if let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        let mut offset = 0;
        for line in rest.split_inclusive('\n') {
            if line.trim_end() == "---" {
                let front = &rest[..offset];
                body = &rest[offset + line.len()..];
                let lines: Vec<&str> = front.lines().collect();
                for (i, l) in lines.iter().enumerate() {
                    let Some((k, v)) = l.split_once(':') else {
                        continue;
                    };
                    let v = unquote(v);
                    let one = || (!v.is_empty()).then(|| v.clone());
                    match k.trim() {
                        "title" if !v.is_empty() => title = Some(clip(&v, MAX_TITLE_CHARS)),
                        "date" => date = valid_date(&v),
                        "kind" => {
                            meta.kind = one().map(|k| k.to_lowercase()).filter(|k| {
                                matches!(k.as_str(), "business" | "product" | "process")
                            })
                        }
                        "project" => meta.project = one(),
                        "status" => {
                            meta.status = one().map(|s| s.to_lowercase()).filter(|s| {
                                matches!(s.as_str(), "proposed" | "accepted" | "superseded")
                            })
                        }
                        "supersedes" => meta.supersedes = list(&v, &lines[i + 1..]),
                        "superseded-by" => meta.superseded_by = one(),
                        _ => {}
                    }
                }
                break;
            }
            offset += line.len();
        }
    }
    if title.is_none() {
        title = body
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .map(|t| clip(t.trim().trim_end_matches('#').trim(), MAX_TITLE_CHARS))
            .filter(|t| !t.is_empty());
    }
    Parsed {
        body,
        title,
        date,
        meta,
    }
}

/// What an ADR's status line says, in the first lines of its body: `Status: accepted`,
/// `Status: superseded by 0005`, `Status: accepted; amended by [0004](0004-x.md), 0007`.
/// `amended by` alone means accepted.
pub fn adr_status(body: &str) -> Meta {
    let mut meta = Meta::default();
    let Some(line) = body.lines().take(30).find_map(|l| {
        let l = l.trim().trim_start_matches(['*', '_']);
        let (key, rest) = l.split_at_checked(6)?;
        let rest = rest.trim_start_matches(['*', '_']);
        (key.eq_ignore_ascii_case("status") && rest.starts_with(':'))
            .then(|| rest.trim_start_matches(['*', '_', ':', ' ']).trim())
    }) else {
        return meta;
    };
    for part in line.split(';') {
        let part = part.trim();
        let lower = part.to_lowercase();
        if lower.starts_with("superseded by") {
            meta.status = Some("superseded".into());
            meta.superseded_by = refs(part.get("superseded by".len()..).unwrap_or(""))
                .into_iter()
                .next();
        } else if lower.starts_with("amended by") {
            meta.status.get_or_insert_with(|| "accepted".into());
            meta.amended_by = refs(part.get("amended by".len()..).unwrap_or(""));
        } else {
            let word = lower
                .split(|c: char| !c.is_alphabetic())
                .next()
                .unwrap_or("");
            if matches!(word, "proposed" | "accepted" | "superseded") {
                meta.status = Some(word.to_string());
            }
        }
    }
    meta
}

/// What an `amended by` or `superseded by` names: a comma- or `and`-separated list whose items
/// are Markdown links (their targets) or plain names.
fn refs(text: &str) -> Vec<String> {
    text.replace(" and ", ",")
        .replace('&', ",")
        .split(',')
        .filter_map(|item| {
            let name = match item.find("](") {
                Some(i) => item[i + 2..].split(')').next().unwrap_or("").trim(),
                None => item
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_matches(|c: char| !c.is_alphanumeric()),
            };
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

/// `YYYY-MM-DD` from the start of `s`, if it is one.
pub fn valid_date(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    let b = d.as_bytes();
    let ok = b.iter().enumerate().all(|(i, c)| {
        if i == 4 || i == 7 {
            *c == b'-'
        } else {
            c.is_ascii_digit()
        }
    });
    // Only an all-ASCII shape may be sliced: a multi-byte character would split on a boundary.
    if !ok {
        return None;
    }
    let (m, day) = (&d[5..7], &d[8..10]);
    (("01"..="12").contains(&m) && ("01"..="31").contains(&day)).then(|| d.to_string())
}

pub fn date_of(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// What kind of entry a file of an outside source is, from its top folder and its frontmatter
/// (see the settled rules for what lives where): a decision is `business`, `product` or
/// `process`, or just `decision` when its frontmatter doesn't say; then `note`, else `raw`.
pub fn kind_of(path: &str, meta: &Meta) -> String {
    match path.split('/').next() {
        Some("decisions") if path.contains('/') => {
            meta.kind.clone().unwrap_or_else(|| "decision".into())
        }
        Some("notes") if path.contains('/') => "note".into(),
        _ => "raw".into(),
    }
}

/// Whether `path` (relative to the project) is an ADR: a file in `adr/` under the docs folder.
pub fn is_adr(path: &str, docs: &str) -> bool {
    let adr = if docs.is_empty() {
        "adr/".to_string()
    } else {
        format!("{docs}/adr/")
    };
    path.starts_with(&adr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_gives_title_and_date_and_is_not_part_of_the_body() {
        let p = parse("---\ntitle: \"Second phone field\"\ndate: 2024-05-14\nkind: product\n---\n# Heading\n\nBody.\n");
        assert_eq!(p.title.as_deref(), Some("Second phone field"));
        assert_eq!(p.date.as_deref(), Some("2024-05-14"));
        assert_eq!(p.body, "# Heading\n\nBody.\n");
    }

    #[test]
    fn without_frontmatter_the_first_h1_is_the_title() {
        let p = parse("intro\n\n# The Title #\n\n## Sub\n");
        assert_eq!(p.title.as_deref(), Some("The Title"));
        assert_eq!(p.date, None);
        assert!(p.body.starts_with("intro"));
    }

    #[test]
    fn an_unclosed_or_garbled_frontmatter_is_just_text() {
        let p = parse("---\ntitle: x\nno end\n");
        assert_eq!(p.body, "---\ntitle: x\nno end\n");
        assert_eq!(parse("---\ndate: soon\n---\nx").date, None);
        assert_eq!(parse("---\ndate: 2024-13-45\n---\nx").date, None);
        assert_eq!(
            parse("---\ndate: 2024-05-14T10:00:00Z\n---\nx")
                .date
                .as_deref(),
            Some("2024-05-14")
        );
    }

    #[test]
    fn a_date_with_multibyte_characters_is_not_a_date_and_does_not_panic() {
        assert_eq!(parse("---\ndate: abcdéxxxx\n---\nx").date, None);
        assert_eq!(parse("---\ndate: 2024-é5-14\n---\nx").date, None);
        assert_eq!(parse("---\ndate: 日本語日本語日本語\n---\nx").date, None);
    }

    #[test]
    fn a_title_is_cut_to_a_sane_length() {
        let long = "w".repeat(5000);
        let framed = format!("---\ntitle: {long}\n---\nx");
        assert_eq!(
            parse(&framed).title.unwrap().chars().count(),
            MAX_TITLE_CHARS
        );
        let headed = format!("# {long}\n\nbody");
        assert_eq!(
            parse(&headed).title.unwrap().chars().count(),
            MAX_TITLE_CHARS
        );
    }

    #[test]
    fn dates_from_times() {
        let t = |s| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s);
        assert_eq!(date_of(t(0)), "1970-01-01");
        assert_eq!(date_of(t(1_709_424_000)), "2024-03-03");
        assert_eq!(date_of(t(1_709_251_200)), "2024-03-01");
        assert_eq!(date_of(t(951_782_400)), "2000-02-29");
    }

    #[test]
    fn kinds_come_from_the_top_folder_and_a_decisions_frontmatter() {
        let none = Meta::default();
        let product = Meta {
            kind: Some("product".into()),
            ..Default::default()
        };
        assert_eq!(kind_of("decisions/2024-05-14-x.md", &none), "decision");
        assert_eq!(kind_of("decisions/2024-05-14-x.md", &product), "product");
        assert_eq!(kind_of("notes/a.md", &product), "note");
        assert_eq!(kind_of("notes.md", &none), "raw");
        assert_eq!(kind_of("policies/decisions/x.md", &product), "raw");
    }

    #[test]
    fn adrs_are_the_files_in_adr_under_the_docs_folder() {
        assert!(is_adr("docs/adr/0001-x.md", "docs"));
        assert!(is_adr("adr/0001-x.md", ""));
        assert!(is_adr("handbook/adr/0001-x.md", "handbook"));
        assert!(!is_adr("docs/PRODUCT.md", "docs"));
        assert!(!is_adr("src/adr/0001-x.md", "docs"));
    }

    #[test]
    fn a_decisions_frontmatter_says_what_it_decided_and_replaced() {
        let p = parse(
            "---\ntitle: T\nkind: Product\nproject: courier-app\nstatus: superseded\nsupersedes: [a, \"b\"]\nsuperseded-by: later\ndecided-by: owner\n---\nx",
        );
        assert_eq!(
            p.meta,
            Meta {
                kind: Some("product".into()),
                project: Some("courier-app".into()),
                status: Some("superseded".into()),
                supersedes: vec!["a".into(), "b".into()],
                superseded_by: Some("later".into()),
                amended_by: vec![],
            }
        );
        let block = parse("---\nsupersedes:\n  - one\n  - two\nkind: process\n---\nx").meta;
        assert_eq!(block.supersedes, ["one", "two"]);
        assert_eq!(block.kind.as_deref(), Some("process"));
        // A trailing comment is not part of the value, as in the settled example.
        let commented = parse(
            "---\nkind: product   # business | product | process\nproject: courier-app          # optional\nstatus: superseded # replaced\nsupersedes: [a, b]   # earlier ones\ntitle: \"Use # in titles\"\n---\nx",
        );
        assert_eq!(commented.title.as_deref(), Some("Use # in titles"));
        let escaped =
            parse("---\ntitle: \"Use \\\"manual\\\" mode\"  # c\nproject: 'it''s'\n---\nx");
        assert_eq!(escaped.title.as_deref(), Some("Use \"manual\" mode"));
        assert_eq!(escaped.meta.project.as_deref(), Some("it's"));
        assert_eq!(commented.meta.kind.as_deref(), Some("product"));
        assert_eq!(commented.meta.project.as_deref(), Some("courier-app"));
        assert_eq!(commented.meta.status.as_deref(), Some("superseded"));
        assert_eq!(commented.meta.supersedes, ["a", "b"]);
        // What isn't one of the allowed values says nothing.
        let odd = parse("---\nkind: cosmic\nstatus: maybe\n---\nx").meta;
        assert!(odd.is_empty(), "{odd:?}");
        assert!(parse("# Plain\n\nbody").meta.is_empty());
    }

    #[test]
    fn an_adrs_status_line_says_accepted_superseded_or_amended() {
        let s = |t: &str| adr_status(&format!("# 0001: T\n\n{t}\n\n## Context\n"));
        assert_eq!(s("Status: accepted").status.as_deref(), Some("accepted"));
        assert_eq!(
            s("**Status:** Proposed").status.as_deref(),
            Some("proposed")
        );
        let sup = s("Status: superseded by [0005](0005-x.md)");
        assert_eq!(sup.status.as_deref(), Some("superseded"));
        assert_eq!(sup.superseded_by.as_deref(), Some("0005-x.md"));
        let plain = s("Status: superseded by 0005");
        assert_eq!(plain.superseded_by.as_deref(), Some("0005"));
        let am = s("Status: accepted; amended by [0004](0004-a.md), [0007](0007-b.md)");
        assert_eq!(am.status.as_deref(), Some("accepted"));
        assert_eq!(am.amended_by, ["0004-a.md", "0007-b.md"]);
        let alone = s("Status: amended by 0004 and 0007");
        assert_eq!(alone.status.as_deref(), Some("accepted"));
        assert_eq!(alone.amended_by, ["0004", "0007"]);
        let mixed = s("Status: accepted; amended by [0004](0004-a.md), 0007 and [0009](0009-c.md)");
        assert_eq!(mixed.amended_by, ["0004-a.md", "0007", "0009-c.md"]);
        assert!(adr_status("# No status here\n\nStatuses vary.\n").is_empty());
        assert!(adr_status("Status: wat").is_empty());
        // Non-ASCII text must not trip the slicing.
        assert!(adr_status("Status: İİİİİİ superseded by x")
            .status
            .is_none());
    }
}
