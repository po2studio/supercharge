//! CSV, TSV and spreadsheets as pipe tables: the header row first, which the chunker repeats in
//! every row group.

use super::{
    office,
    text::{decode, table},
    Converted,
};
use crate::doc;
use calamine::{Data, Reader, SheetVisible};
use std::{io::Cursor, time::Duration, time::UNIX_EPOCH};

/// The delimiter of a `.csv`: Excel in many locales (Iceland's too) writes `;`.
fn delimiter(text: &str) -> u8 {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    b",;\t|"
        .iter()
        .copied()
        .max_by_key(|d| first.bytes().filter(|b| b == d).count())
        .filter(|d| first.bytes().any(|b| b == *d))
        .unwrap_or(b',')
}

pub fn csv(bytes: &[u8], tsv: bool) -> Converted {
    let text = decode(bytes);
    let delim = if tsv { b'\t' } else { delimiter(&text) };
    let rows: Vec<Vec<String>> = csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes())
        .records()
        .flatten()
        .map(|r| r.iter().map(str::to_string).collect::<Vec<_>>())
        .filter(|r| r.iter().any(|c| !c.trim().is_empty()))
        .collect();
    Converted {
        text: table(&rows).trim_end().to_string(),
        title: None,
        date: None,
        prose: false,
        meta: Default::default(),
    }
}

/// An Excel serial date as `YYYY-MM-DD`, with the time when there is one.
fn excel_date(serial: f64) -> String {
    let secs = ((serial - 25569.0) * 86400.0).round();
    if !(0.0..4e11).contains(&secs) {
        return serial.to_string();
    }
    let secs = secs as u64;
    let date = doc::date_of(UNIX_EPOCH + Duration::from_secs(secs));
    match secs % 86400 {
        0 => date,
        t => format!("{date} {:02}:{:02}", t / 3600, t % 3600 / 60),
    }
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty | Data::Error(_) => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", *f as i64),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(d) if d.is_datetime() => excel_date(d.as_f64()),
        Data::DateTime(d) => d.as_f64().to_string(),
    }
}

/// `.xlsx`, `.xlsm`, `.xlsb` and `.xls`: each visible sheet is a section with a table.
pub fn spreadsheet(bytes: &[u8]) -> Result<Converted, String> {
    let mut book = calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| format!("cannot be read ({e}); it may be password-protected"))?;
    let mut out = String::new();
    let sheets = book.sheets_metadata().to_vec();
    for sheet in sheets {
        if sheet.visible != SheetVisible::Visible {
            continue;
        }
        let Ok(range) = book.worksheet_range(&sheet.name) else {
            continue;
        };
        let rows: Vec<Vec<String>> = range
            .rows()
            .map(|r| {
                let mut cells: Vec<String> = r.iter().map(cell_text).collect();
                while cells.last().is_some_and(|c| c.trim().is_empty()) {
                    cells.pop();
                }
                cells
            })
            .filter(|r| !r.is_empty())
            .collect();
        if rows.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}\n\n{}\n", sheet.name, table(&rows)));
    }
    let (title, date) = office::spreadsheet_properties(bytes);
    Ok(Converted {
        text: out.trim().to_string(),
        title,
        date,
        prose: true,
        meta: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_becomes_a_table_whatever_the_delimiter_and_encoding() {
        let c = csv(
            b"id;n\xe1fn;upph\xe6\xf0\n1;\"\xde\xf3r; Jonsson\";500\n\n2;Anna;7\n",
            false,
        );
        assert_eq!(
            c.text,
            "| id | náfn | upphæð |\n|---|---|---|\n| 1 | Þór; Jonsson | 500 |\n| 2 | Anna | 7 |"
        );
        assert_eq!(
            csv(b"a\tb\n1\t2\n", true).text,
            "| a | b |\n|---|---|\n| 1 | 2 |"
        );
        assert_eq!(
            csv(b"just one column\nand more\n", false)
                .text
                .lines()
                .count(),
            3
        );
    }

    #[test]
    fn excel_serials_are_dates() {
        assert_eq!(excel_date(45292.0), "2024-01-01");
        assert_eq!(excel_date(45292.5), "2024-01-01 12:00");
    }
}
