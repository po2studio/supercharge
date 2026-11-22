//! Email: `.eml` and `.mbox`. The headers that say who and when go first as metadata lines, the
//! body follows with quoted replies stripped, and the subject is the title.

use super::{html, Converted};
use crate::doc;
use mail_parser::{mailbox::mbox::MessageIterator, AddressList, Message, MessageParser};

/// One message as `From:`, `To:`, `Cc:`, `Date:` and `Attachments:` lines, then its text.
struct Rendered {
    subject: Option<String>,
    date: Option<String>,
    text: String,
}

fn people(list: Option<AddressList<'_>>) -> String {
    let Some(list) = list else {
        return String::new();
    };
    list.mailboxes()
        .map(
            |m| match (m.name().filter(|n| !n.is_empty()), m.address()) {
                (Some(n), Some(a)) => format!("{n} <{a}>"),
                (Some(n), None) => n.to_string(),
                (None, Some(a)) => a.to_string(),
                (None, None) => String::new(),
            },
        )
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A reply's body without what it quotes: `>` lines, the "On … wrote:" line that introduces them,
/// and everything below an "Original Message" rule.
pub fn strip_quoted(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with('>') {
            continue;
        }
        let lower = t.to_lowercase();
        if t.starts_with("-----")
            && (lower.contains("original message") || lower.contains("forwarded message"))
        {
            break;
        }
        if t.len() > 20 && t.chars().all(|c| c == '_') {
            // Outlook's rule above the quoted headers.
            let next = lines.get(i + 1).map_or("", |l| l.trim());
            if ["from:", "frá:", "von:", "de :", "de:"]
                .iter()
                .any(|h| next.to_lowercase().starts_with(h))
            {
                break;
            }
        }
        let attribution = lower.ends_with("wrote:")
            || (t.ends_with(':')
                && lines[i + 1..]
                    .iter()
                    .map(|l| l.trim())
                    .find(|l| !l.is_empty())
                    .is_some_and(|l| l.starts_with('>')));
        if attribution {
            // Gmail may break the line before "wrote:".
            if lower == "wrote:"
                && out
                    .last()
                    .is_some_and(|l| l.trim_start().starts_with("On "))
            {
                out.pop();
            }
            continue;
        }
        out.push(line);
    }
    let mut text = out.join("\n");
    text = text.trim().to_string();
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text
}

fn render(message: &Message) -> Rendered {
    let subject = message
        .subject()
        .map(|s| doc::clip(s.trim(), doc::MAX_TITLE_CHARS))
        .filter(|s| !s.is_empty());
    let date = message
        .date()
        .and_then(|d| doc::valid_date(&d.to_rfc3339()));
    let mut text = String::new();
    for (label, value) in [
        ("From", people(message.from())),
        ("To", people(message.to())),
        ("Cc", people(message.cc())),
        ("Date", date.clone().unwrap_or_default()),
        (
            "Attachments",
            message
                .attachments()
                .filter_map(|a| a.attachment_name().map(str::to_string))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ] {
        if !value.is_empty() {
            text.push_str(&format!("{label}: {value}\n"));
        }
    }
    let body = match message.body_text(0) {
        Some(t) => t.into_owned(),
        None => message
            .body_html(0)
            .map(|h| html::to_text(&h).text)
            .unwrap_or_default(),
    };
    text.push('\n');
    text.push_str(&strip_quoted(&body));
    Rendered {
        subject,
        date,
        text: text.trim().to_string(),
    }
}

pub fn eml(bytes: &[u8]) -> Result<Converted, String> {
    let message = MessageParser::new()
        .parse(bytes)
        .filter(|m| m.subject().is_some() || m.from().is_some() || m.body_text(0).is_some())
        .ok_or("not an email message")?;
    let r = render(&message);
    Ok(Converted {
        text: r.text,
        title: r.subject,
        date: r.date,
        prose: true,
        meta: Default::default(),
    })
}

/// A mailbox: one section per message, headed by its subject, dated by its newest message.
pub fn mbox(bytes: &[u8]) -> Result<Converted, String> {
    let mut text = String::new();
    let mut date: Option<String> = None;
    let mut count = 0;
    for entry in MessageIterator::new(bytes) {
        let Ok(entry) = entry else { continue };
        let Some(message) = MessageParser::new().parse(entry.contents()) else {
            continue;
        };
        let r = render(&message);
        count += 1;
        text.push_str(&format!(
            "# {}\n\n{}\n\n",
            r.subject.as_deref().unwrap_or("(no subject)"),
            r.text
        ));
        date = date.max(r.date);
    }
    if count == 0 {
        return Err("no messages: not a mailbox".into());
    }
    Ok(Converted {
        text: text.trim().to_string(),
        title: None,
        date,
        prose: true,
        meta: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = "From: Anna Jonsdottir <anna@example.is>\r\nTo: Bjorn <bjorn@example.is>\r\nSubject: =?utf-8?q?Fundur_=C3=A1_f=C3=B6studag?=\r\nDate: Tue, 14 May 2024 10:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nThe meeting moves to room =C3=9Eingholt.\r\n\r\nOn Mon, 13 May 2024 at 09:00, Bjorn <bjorn@example.is> wrote:\r\n> Where is the meeting?\r\n> Thanks\r\n";

    #[test]
    fn headers_are_metadata_and_the_quoted_reply_is_gone() {
        let c = eml(REPLY.as_bytes()).unwrap();
        assert_eq!(c.title.as_deref(), Some("Fundur á föstudag"));
        assert_eq!(c.date.as_deref(), Some("2024-05-14"));
        assert_eq!(
            c.text,
            "From: Anna Jonsdottir <anna@example.is>\nTo: Bjorn <bjorn@example.is>\nDate: 2024-05-14\n\nThe meeting moves to room Þingholt."
        );
    }

    #[test]
    fn quoting_styles() {
        assert_eq!(
            strip_quoted("Yes.\n\n-----Original Message-----\nFrom: x\nno"),
            "Yes."
        );
        assert_eq!(
            strip_quoted("Yes.\n\nOn Mon, 13 May 2024 at 09:00, Bjorn\nwrote:\n> hi\n"),
            "Yes."
        );
        assert_eq!(
            strip_quoted("Yes.\n\nÞann 13. maí skrifaði Björn:\n> hæ\n"),
            "Yes."
        );
        assert_eq!(
            strip_quoted("Note:\nnothing quoted here"),
            "Note:\nnothing quoted here"
        );
        assert_eq!(
            strip_quoted("Yes.\n________________________________\nFrom: Bjorn\nSent: Monday"),
            "Yes."
        );
    }

    #[test]
    fn a_mailbox_has_a_section_per_message() {
        let mbox_text = "From a@x Mon May 13 09:00:00 2024\nSubject: One\nFrom: a@x\nDate: Mon, 13 May 2024 09:00:00 +0000\n\nfirst body\n\nFrom b@x Tue May 14 09:00:00 2024\nSubject: Two\nFrom: b@x\nDate: Tue, 14 May 2024 09:00:00 +0000\n\nsecond body\n";
        let c = mbox(mbox_text.as_bytes()).unwrap();
        assert!(c.text.starts_with("# One\n\nFrom: a@x"), "{}", c.text);
        assert!(c.text.contains("# Two\n") && c.text.contains("second body"));
        assert_eq!(c.date.as_deref(), Some("2024-05-14"));
        assert!(mbox(b"just text").is_err());
    }

    #[test]
    fn something_else_is_not_an_email() {
        assert!(eml(b"\x00\x01\x02 binary").is_err());
    }
}
