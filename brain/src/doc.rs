//! What a text file says about itself: frontmatter, title, date.

use std::time::SystemTime;

pub struct Parsed<'a> {
    pub body: &'a str,
    pub title: Option<String>,
    pub date: Option<String>,
}

/// Titles and headings go into every chunk's header; a line of megabytes must not.
pub const MAX_TITLE_CHARS: usize = 200;

pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Splits a leading `---` frontmatter block off and reads `title` and `date` from it.
/// Without frontmatter the title is the first `#` heading, if any.
pub fn parse(text: &str) -> Parsed<'_> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut title = None;
    let mut date = None;
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
                for l in front.lines() {
                    if let Some((k, v)) = l.split_once(':') {
                        let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
                        match k.trim() {
                            "title" if !v.is_empty() => title = Some(clip(v, MAX_TITLE_CHARS)),
                            "date" => date = valid_date(v),
                            _ => {}
                        }
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
    Parsed { body, title, date }
}

/// `YYYY-MM-DD` from the start of `s`, if it is one.
fn valid_date(s: &str) -> Option<String> {
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

/// `decision`, `note` or `raw`, from the top folder (see the settled rules for what lives where).
pub fn kind_of(path: &str) -> &'static str {
    match path.split('/').next() {
        Some("decisions") if path.contains('/') => "decision",
        Some("notes") if path.contains('/') => "note",
        _ => "raw",
    }
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
    fn kinds_come_from_the_top_folder() {
        assert_eq!(kind_of("decisions/2024-05-14-x.md"), "decision");
        assert_eq!(kind_of("notes/a.md"), "note");
        assert_eq!(kind_of("notes.md"), "raw");
        assert_eq!(kind_of("policies/decisions/x.md"), "raw");
    }
}
