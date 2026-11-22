//! Word, PowerPoint and OpenDocument files: zip archives of XML. Text comes out as Markdown-ish
//! text: headings keep their `#`, list items their `-`, tables become pipe tables.

use super::{
    text::{table, unescape},
    xml::{attr, tokens, Tok},
    Converted,
};
use crate::doc;
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};
use zip::ZipArchive;

/// The most one XML part is read: a zip bomb must not fill memory.
const MAX_PART: u64 = 64 << 20;

type Zip<'a> = ZipArchive<Cursor<&'a [u8]>>;

pub fn open(bytes: &[u8]) -> Result<Zip<'_>, String> {
    if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        return Err("encrypted (password-protected), or an old binary Office format".into());
    }
    ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a readable archive ({e})"))
}

fn part(zip: &mut Zip, name: &str) -> Option<String> {
    let file = zip.by_name(name).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_PART).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The names of the parts like `<dir>/<stem>N.xml`, in the order of N.
fn numbered(zip: &Zip, dir: &str, stem: &str) -> Vec<(u32, String)> {
    let mut found: Vec<(u32, String)> = zip
        .file_names()
        .filter_map(|n| {
            let n = n.ok()?;
            let num = n
                .strip_prefix(dir)?
                .strip_prefix(stem)?
                .strip_suffix(".xml")?;
            Some((num.parse().ok()?, n.to_string()))
        })
        .collect();
    found.sort();
    found
}

/// Title and date from `docProps/core.xml` (Office) or `meta.xml` (OpenDocument).
fn properties(zip: &mut Zip) -> (Option<String>, Option<String>) {
    let mut title = None;
    let (mut modified, mut created) = (None, None);
    for name in ["docProps/core.xml", "meta.xml"] {
        let Some(xml) = part(zip, name) else {
            continue;
        };
        let mut current = "";
        for tok in tokens(&xml) {
            match tok {
                Tok::Open { name, empty, .. } if !empty => current = name,
                Tok::Close(_) => current = "",
                Tok::Text(t) => {
                    let t = unescape(t);
                    match current {
                        "dc:title" if !t.trim().is_empty() => {
                            title = Some(doc::clip(t.trim(), doc::MAX_TITLE_CHARS));
                        }
                        "dcterms:modified" | "dc:date" => modified = doc::valid_date(t.trim()),
                        "dcterms:created" | "meta:creation-date" => {
                            created = doc::valid_date(t.trim());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    (title, modified.or(created))
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
    row: Vec<String>,
}

/// Collects blocks of text in reading order; a table's paragraphs go to its cells.
#[derive(Default)]
struct Build {
    out: String,
    cells: Vec<String>,
    tables: Vec<Table>,
    first_heading: Option<String>,
    /// The last block written was a list item.
    in_list: bool,
}

impl Build {
    fn para(&mut self, text: &str, heading: Option<usize>, list: bool) {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }
        if let Some(cell) = self.cells.last_mut() {
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(&text);
            return;
        }
        if let Some(level) = heading {
            self.heading(&text, level);
        } else if list {
            if !self.in_list {
                self.block_gap();
            }
            self.out.push_str("- ");
            self.out.push_str(&text);
            self.out.push('\n');
            self.in_list = true;
        } else {
            self.block_gap();
            self.out.push_str(&text);
            self.out.push_str("\n\n");
        }
    }

    fn block_gap(&mut self) {
        self.in_list = false;
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    fn heading(&mut self, text: &str, level: usize) {
        self.block_gap();
        if self.first_heading.is_none() {
            self.first_heading = Some(doc::clip(text, doc::MAX_TITLE_CHARS));
        }
        self.out.push_str(&"#".repeat(level.clamp(1, 6)));
        self.out.push(' ');
        self.out.push_str(text);
        self.out.push_str("\n\n");
    }

    fn start_table(&mut self) {
        self.tables.push(Table::default());
    }

    fn start_cell(&mut self) {
        self.cells.push(String::new());
    }

    fn end_cell(&mut self) {
        if let (Some(text), Some(t)) = (self.cells.pop(), self.tables.last_mut()) {
            t.row.push(text);
        }
    }

    fn end_row(&mut self) {
        if let Some(t) = self.tables.last_mut() {
            let mut row = std::mem::take(&mut t.row);
            while row.last().is_some_and(|c| c.trim().is_empty()) {
                row.pop();
            }
            if !row.is_empty() {
                t.rows.push(row);
            }
        }
    }

    fn end_table(&mut self) {
        let Some(t) = self.tables.pop() else {
            return;
        };
        if t.rows.is_empty() {
            return;
        }
        if let Some(cell) = self.cells.last_mut() {
            // A table inside a cell is flattened into it.
            for row in &t.rows {
                if !cell.is_empty() {
                    cell.push_str("; ");
                }
                cell.push_str(&row.join(", "));
            }
            return;
        }
        self.block_gap();
        self.out.push_str(&table(&t.rows));
        self.out.push('\n');
    }

    fn finish(self) -> String {
        self.out.trim().to_string()
    }
}

fn heading_level(style: &str) -> Option<usize> {
    let s = style.to_lowercase();
    if s == "title" {
        return Some(1);
    }
    s.strip_prefix("heading")?.trim().parse().ok()
}

/// Style ID to the style's name, from `word/styles.xml`: a document in another language calls its
/// headings by names of its own, but the built-in styles are named `heading 1` inside the file.
fn style_names(zip: &mut Zip) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Some(xml) = part(zip, "word/styles.xml") else {
        return names;
    };
    let mut id = None;
    for tok in tokens(&xml) {
        if let Tok::Open { name, attrs, .. } = tok {
            match name {
                "w:style" => id = attr(attrs, "w:styleId"),
                "w:name" => {
                    if let (Some(id), Some(v)) = (&id, attr(attrs, "w:val")) {
                        names.insert(id.clone(), v);
                    }
                }
                _ => {}
            }
        }
    }
    names
}

pub fn docx(bytes: &[u8]) -> Result<Converted, String> {
    let mut zip = open(bytes)?;
    let styles = style_names(&mut zip);
    let xml = part(&mut zip, "word/document.xml").ok_or("no word/document.xml: not a Word file")?;
    let mut b = Build::default();
    let (mut para, mut heading, mut list, mut in_t) = (String::new(), None, false, false);
    for tok in tokens(&xml) {
        match tok {
            Tok::Open { name, attrs, empty } => match name {
                "w:p" => {
                    para.clear();
                    heading = None;
                    list = false;
                }
                "w:pStyle" => {
                    let id = attr(attrs, "w:val").unwrap_or_default();
                    let name = styles.get(&id).map_or(id.as_str(), String::as_str);
                    heading = heading_level(name);
                    list |= name.to_lowercase().starts_with("list");
                }
                "w:numPr" => list = true,
                "w:t" => in_t = !empty,
                "w:tab" => para.push(' '),
                "w:br" | "w:cr" => para.push(' '),
                "w:tbl" => b.start_table(),
                "w:tc" => b.start_cell(),
                _ => {}
            },
            Tok::Close(name) => match name {
                "w:t" => in_t = false,
                "w:p" => {
                    b.para(&para, heading, list);
                    para.clear();
                }
                "w:tc" => b.end_cell(),
                "w:tr" => b.end_row(),
                "w:tbl" => b.end_table(),
                _ => {}
            },
            Tok::Text(t) if in_t => para.push_str(&unescape(t)),
            Tok::Text(_) => {}
        }
    }
    let (title, date) = properties(&mut zip);
    let first = b.first_heading.clone();
    Ok(Converted {
        text: b.finish(),
        title: title.or(first),
        date,
        prose: true,
    })
}

/// A slide's title and body, or a notes page's body.
fn slide(xml: &str, notes: bool) -> (Option<String>, String) {
    let mut b = Build::default();
    let (mut para, mut in_t) = (String::new(), false);
    let mut shape = String::new();
    let mut title: Vec<String> = Vec::new();
    for tok in tokens(xml) {
        match tok {
            Tok::Open { name, attrs, empty } => match name {
                "p:sp" | "p:graphicFrame" => shape.clear(),
                "p:ph" => shape = attr(attrs, "type").unwrap_or_default(),
                "a:p" => para.clear(),
                "a:t" => in_t = !empty,
                "a:br" => para.push(' '),
                "a:tbl" => b.start_table(),
                "a:tc" => b.start_cell(),
                _ => {}
            },
            Tok::Close(name) => match name {
                "a:t" => in_t = false,
                "a:p" => {
                    let wanted = if notes {
                        shape == "body"
                    } else {
                        !matches!(shape.as_str(), "sldNum" | "dt" | "ftr" | "hdr")
                    };
                    if !wanted && b.cells.is_empty() {
                        continue;
                    }
                    if !notes && matches!(shape.as_str(), "title" | "ctrTitle") {
                        title.push(para.split_whitespace().collect::<Vec<_>>().join(" "));
                    } else {
                        b.para(&para, None, false);
                    }
                }
                "a:tc" => b.end_cell(),
                "a:tr" => b.end_row(),
                "a:tbl" => b.end_table(),
                _ => {}
            },
            Tok::Text(t) if in_t => para.push_str(&unescape(t)),
            Tok::Text(_) => {}
        }
    }
    let title = Some(title.join(" ")).filter(|t| !t.trim().is_empty());
    (title, b.finish())
}

pub fn pptx(bytes: &[u8]) -> Result<Converted, String> {
    let mut zip = open(bytes)?;
    let slides = numbered(&zip, "ppt/slides/", "slide");
    if slides.is_empty() {
        return Err("no slides: not a PowerPoint file".into());
    }
    // Which slide a notes page belongs to, from the notes page's relationships.
    let mut notes: HashMap<u32, String> = HashMap::new();
    for (n, name) in numbered(&zip, "ppt/notesSlides/", "notesSlide") {
        let rels = part(
            &mut zip,
            &format!("ppt/notesSlides/_rels/notesSlide{n}.xml.rels"),
        )
        .unwrap_or_default();
        let target = rels
            .split("slides/slide")
            .nth(1)
            .and_then(|r| r.split(".xml").next())
            .and_then(|d| d.parse().ok())
            .unwrap_or(n);
        if let Some(xml) = part(&mut zip, &name) {
            let (_, body) = slide(&xml, true);
            if !body.is_empty() {
                notes.insert(target, body);
            }
        }
    }
    let mut out = String::new();
    let mut deck_title = None;
    for (n, name) in slides {
        let xml = part(&mut zip, &name).unwrap_or_default();
        let (title, body) = slide(&xml, false);
        deck_title = deck_title.or_else(|| title.clone());
        out.push_str(&format!(
            "## {}\n\n",
            title.unwrap_or_else(|| format!("Slide {n}"))
        ));
        if !body.is_empty() {
            out.push_str(&body);
            out.push_str("\n\n");
        }
        if let Some(note) = notes.get(&n) {
            out.push_str("Notes: ");
            out.push_str(note);
            out.push_str("\n\n");
        }
    }
    let (title, date) = properties(&mut zip);
    Ok(Converted {
        text: out.trim().to_string(),
        title: title.or(deck_title),
        date,
        prose: true,
    })
}

/// `.odt`, `.ods` and `.odp`.
pub fn odf(bytes: &[u8]) -> Result<Converted, String> {
    let mut zip = open(bytes)?;
    let xml = part(&mut zip, "content.xml").ok_or("no content.xml: not an OpenDocument file")?;
    let mut b = Build::default();
    let (mut para, mut heading, mut in_para) = (String::new(), None, false);
    let (mut list, mut sheets, mut slides) = (0usize, false, false);
    let mut slide_no = 0;
    for tok in tokens(&xml) {
        match tok {
            Tok::Open { name, attrs, empty } => match name {
                "office:spreadsheet" => sheets = true,
                "office:presentation" => slides = true,
                "text:h" => {
                    para.clear();
                    in_para = !empty;
                    heading = Some(
                        attr(attrs, "text:outline-level")
                            .and_then(|l| l.parse().ok())
                            .unwrap_or(1),
                    );
                }
                "text:p" => {
                    para.clear();
                    in_para = !empty;
                    heading = None;
                }
                "text:s" => {
                    let n: usize = attr(attrs, "text:c")
                        .and_then(|c| c.parse().ok())
                        .unwrap_or(1);
                    para.push_str(&" ".repeat(n.min(64)));
                }
                "text:tab" | "text:line-break" => para.push(' '),
                "text:list-item" => list += 1,
                "table:table" => {
                    if sheets {
                        if let Some(n) = attr(attrs, "table:name") {
                            b.heading(&n, 2);
                        }
                    }
                    b.start_table();
                }
                "table:table-cell" => {
                    b.start_cell();
                    if empty {
                        b.end_cell();
                    }
                }
                "table:covered-table-cell" => {
                    b.start_cell();
                    b.end_cell();
                }
                "draw:page" if slides => {
                    slide_no += 1;
                    b.heading(&format!("Slide {slide_no}"), 2);
                }
                _ => {}
            },
            Tok::Close(name) => match name {
                "text:h" | "text:p" => {
                    in_para = false;
                    b.para(&para, heading, list > 0);
                    para.clear();
                }
                "text:list-item" => list = list.saturating_sub(1),
                "table:table-cell" => b.end_cell(),
                "table:table-row" => b.end_row(),
                "table:table" => b.end_table(),
                _ => {}
            },
            Tok::Text(t) if in_para => para.push_str(&unescape(t)),
            Tok::Text(_) => {}
        }
    }
    let (title, date) = properties(&mut zip);
    let first = b.first_heading.clone().filter(|_| !sheets && !slides);
    Ok(Converted {
        text: b.finish(),
        title: title.or(first),
        date,
        prose: true,
    })
}

/// Title and date of a spreadsheet, whose cells another reader gives.
pub fn spreadsheet_properties(bytes: &[u8]) -> (Option<String>, Option<String>) {
    open(bytes).map_or((None, None), |mut zip| properties(&mut zip))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A zip of `parts`, as a minimal Office or OpenDocument file is.
    pub fn zipped(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut z = zip::ZipWriter::new(Cursor::new(&mut out));
            for (name, body) in parts {
                z.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        out
    }

    #[test]
    fn a_word_file_gives_headings_lists_tables_and_properties() {
        let doc = zipped(&[
            (
                "word/styles.xml",
                r#"<w:styles><w:style w:styleId="Fyrirsogn1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document><w:body>
                <w:p><w:pPr><w:pStyle w:val="Fyrirsogn1"/></w:pPr><w:r><w:t>Leave policy</w:t></w:r></w:p>
                <w:p><w:r><w:t xml:space="preserve">Staff get </w:t></w:r><w:r><w:t>25 days &amp; more.</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>one</w:t></w:r></w:p>
                <w:p><w:pPr><w:numPr/></w:pPr><w:r><w:t>two</w:t></w:r></w:p>
                <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Name</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Days</w:t></w:r></w:p></w:tc></w:tr>
                <w:tr><w:tc><w:p><w:r><w:t>Anna</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>30</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
                <w:p><w:r><w:delText>gone</w:delText></w:r></w:p>
                </w:body></w:document>"#,
            ),
            (
                "docProps/core.xml",
                "<cp:coreProperties><dc:title>Orlof</dc:title><dcterms:created>2023-01-02T10:00:00Z</dcterms:created><dcterms:modified>2024-05-14T10:00:00Z</dcterms:modified></cp:coreProperties>",
            ),
        ]);
        let c = docx(&doc).unwrap();
        assert_eq!(c.title.as_deref(), Some("Orlof"));
        assert_eq!(c.date.as_deref(), Some("2024-05-14"));
        assert_eq!(
            c.text,
            "# Leave policy\n\nStaff get 25 days & more.\n\n- one\n- two\n\n| Name | Days |\n|---|---|\n| Anna | 30 |"
        );
    }

    #[test]
    fn slides_have_their_title_text_and_notes() {
        let deck = zipped(&[
            (
                "ppt/slides/slide2.xml",
                r#"<p:sld><p:sp><p:ph type="title"/><a:p><a:r><a:t>Roadmap</a:t></a:r></a:p></p:sp>
                <p:sp><a:p><a:r><a:t>Ship the brain</a:t></a:r></a:p></p:sp></p:sld>"#,
            ),
            (
                "ppt/slides/slide10.xml",
                r#"<p:sld><p:sp><a:p><a:r><a:t>Last words</a:t></a:r></a:p></p:sp><p:sp><p:ph type="sldNum"/><a:p><a:r><a:t>10</a:t></a:r></a:p></p:sp></p:sld>"#,
            ),
            (
                "ppt/notesSlides/notesSlide1.xml",
                r#"<p:notes><p:sp><p:ph type="body"/><a:p><a:r><a:t>Say it slowly</a:t></a:r></a:p></p:sp></p:notes>"#,
            ),
            (
                "ppt/notesSlides/_rels/notesSlide1.xml.rels",
                r#"<Relationships><Relationship Target="../slides/slide2.xml"/></Relationships>"#,
            ),
        ]);
        let c = pptx(&deck).unwrap();
        assert_eq!(
            c.text,
            "## Roadmap\n\nShip the brain\n\nNotes: Say it slowly\n\n## Slide 10\n\nLast words"
        );
        assert_eq!(c.title.as_deref(), Some("Roadmap"));
    }

    #[test]
    fn an_opendocument_text_has_headings_and_tables_and_a_spreadsheet_its_sheets() {
        let odt = zipped(&[(
            "content.xml",
            r#"<office:document-content><office:body><office:text>
            <text:h text:outline-level="2">Rules</text:h>
            <text:p>Dogs<text:s text:c="2"/>are <text:span>welcome</text:span>.</text:p>
            <table:table table:name="T"><table:table-row><table:table-cell><text:p>a</text:p></table:table-cell><table:table-cell><text:p>b</text:p></table:table-cell></table:table-row></table:table>
            </office:text></office:body></office:document-content>"#,
        )]);
        assert_eq!(
            odf(&odt).unwrap().text,
            "## Rules\n\nDogs are welcome.\n\n| a | b |\n|---|---|"
        );
        let ods = zipped(&[(
            "content.xml",
            r#"<office:document-content><office:spreadsheet>
            <table:table table:name="Prices"><table:table-row><table:table-cell><text:p>item</text:p></table:table-cell><table:table-cell><text:p>eur</text:p></table:table-cell></table:table-row>
            <table:table-row><table:table-cell><text:p>kayak</text:p></table:table-cell><table:table-cell table:number-columns-repeated="3"/></table:table-row></table:table>
            </office:spreadsheet></office:document-content>"#,
        )]);
        assert_eq!(
            odf(&ods).unwrap().text,
            "## Prices\n\n| item | eur |\n|---|---|\n| kayak |  |"
        );
    }

    #[test]
    fn what_is_not_an_archive_is_said_so() {
        assert!(docx(b"PK garbage").unwrap_err().contains("not a readable"));
        assert!(docx(&[0xd0, 0xcf, 0x11, 0xe0, 0, 0])
            .unwrap_err()
            .contains("encrypted"));
        let empty = zipped(&[("other.xml", "<a/>")]);
        assert!(pptx(&empty).unwrap_err().contains("not a PowerPoint"));
    }
}
