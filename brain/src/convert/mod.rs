//! Any file to text. What a person drops into a source is read by what it is: Markdown and plain
//! text as they are, source code and JSON without Markdown's headings, and the rest converted
//! (HTML, PDF, Word, Excel, PowerPoint, OpenDocument, RTF, email, CSV). A file that cannot be
//! converted (a scan, an image, a corrupt or encrypted file) is reported with the reason, never
//! an error. Conversions of the formats that cost time are cached by content hash beside the
//! index, never in the source.

mod html;
mod mail;
mod office;
mod pdf;
mod rtf;
mod table;
pub mod text;
mod xml;

use crate::doc;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
};

/// Bumped when a converter's output changes, so cached conversions are made again.
pub const VERSION: u32 = 1;

/// The largest text file, or converted text, that is indexed: logs and dumps are left out, and
/// the time to chunk and embed a file grows with its size.
pub const MAX_TEXT_BYTES: u64 = 1 << 20;
/// The largest file of a format that is converted from a container, such as a PDF or a Word file.
pub const MAX_CONTAINER_BYTES: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Converted {
    pub text: String,
    /// The document's own title, if it has one; else the file's name is used.
    pub title: Option<String>,
    /// The document's own date (`YYYY-MM-DD`), if it says one.
    pub date: Option<String>,
    /// Whether `#` lines are headings: not in source code, YAML or logs.
    pub prose: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Text,
    /// Source code, configuration and other text that is not prose.
    Code,
    Json,
    Csv,
    Tsv,
    Html,
    Rtf,
    Pdf,
    Docx,
    Pptx,
    Spreadsheet,
    Odf,
    Email,
    Mbox,
    /// Cannot be converted, whatever is in it; the reason.
    Unconvertible(&'static str),
    /// An extension it does not know: read as text if it is text.
    Unknown,
}

const CODE: &[&str] = &[
    "rs",
    "py",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "go",
    "java",
    "kt",
    "kts",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "cs",
    "rb",
    "php",
    "swift",
    "scala",
    "sh",
    "bash",
    "zsh",
    "fish",
    "ps1",
    "bat",
    "sql",
    "lua",
    "pl",
    "pm",
    "r",
    "jl",
    "dart",
    "ex",
    "exs",
    "erl",
    "hs",
    "clj",
    "ml",
    "vue",
    "svelte",
    "css",
    "scss",
    "sass",
    "less",
    "toml",
    "ini",
    "cfg",
    "conf",
    "properties",
    "xml",
    "svg",
    "yaml",
    "yml",
    "ndjson",
    "jsonl",
    "tex",
    "rst",
    "adoc",
    "org",
    "log",
    "tf",
    "proto",
    "graphql",
    "gradle",
    "zig",
    "nim",
    "diff",
    "patch",
];

impl Format {
    pub fn of(path: &Path) -> Format {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        match ext.as_str() {
            "md" | "markdown" => Format::Markdown,
            "txt" | "text" => Format::Text,
            "json" => Format::Json,
            "csv" => Format::Csv,
            "tsv" => Format::Tsv,
            "html" | "htm" | "xhtml" => Format::Html,
            "rtf" => Format::Rtf,
            "pdf" => Format::Pdf,
            "docx" | "docm" | "dotx" => Format::Docx,
            "pptx" | "pptm" | "ppsx" => Format::Pptx,
            "xlsx" | "xlsm" | "xlsb" | "xls" => Format::Spreadsheet,
            "odt" | "ods" | "odp" => Format::Odf,
            "eml" => Format::Email,
            "mbox" => Format::Mbox,
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "heif"
            | "ico" | "psd" => Format::Unconvertible("image (OCR is not supported)"),
            "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wma" => {
                Format::Unconvertible("audio (transcription is not supported)")
            }
            "mp4" | "mov" | "avi" | "mkv" | "webm" | "wmv" | "m4v" => {
                Format::Unconvertible("video (transcription is not supported)")
            }
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => {
                Format::Unconvertible("archive (unpack it into the source to index what is in it)")
            }
            "doc" | "ppt" | "dot" | "pps" => {
                Format::Unconvertible("old binary Office format (save it as .docx or .pptx)")
            }
            e if CODE.contains(&e) => Format::Code,
            _ => Format::Unknown,
        }
    }

    /// Files larger than this are left out without being read.
    pub fn max_bytes(self) -> u64 {
        match self {
            Format::Pdf
            | Format::Docx
            | Format::Pptx
            | Format::Spreadsheet
            | Format::Odf
            | Format::Email
            | Format::Mbox => MAX_CONTAINER_BYTES,
            _ => MAX_TEXT_BYTES,
        }
    }

    /// Whether converting costs enough to keep the result.
    pub fn cached(self) -> bool {
        self.max_bytes() == MAX_CONTAINER_BYTES
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Text(Converted),
    /// Why this file has no text to index.
    Unconverted(String),
}

fn plain(text: &str, prose: bool) -> Converted {
    Converted {
        text: text.to_string(),
        title: None,
        date: None,
        prose,
    }
}

fn run(format: Format, bytes: &[u8]) -> Result<Converted, String> {
    Ok(match format {
        Format::Markdown | Format::Text => {
            let decoded = text::decode(bytes);
            let parsed = doc::parse(&decoded);
            Converted {
                text: parsed.body.to_string(),
                title: parsed.title,
                date: parsed.date,
                prose: true,
            }
        }
        Format::Code => plain(&text::decode(bytes), false),
        Format::Json => {
            let decoded = text::decode(bytes);
            let pretty = serde_json::from_str::<serde_json::Value>(&decoded)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or_else(|_| decoded.into_owned());
            plain(&pretty, false)
        }
        Format::Csv => table::csv(bytes, false),
        Format::Tsv => table::csv(bytes, true),
        Format::Html => html::convert(bytes),
        Format::Rtf => rtf::convert(bytes)?,
        Format::Pdf => pdf::convert(bytes)?,
        Format::Docx => office::docx(bytes)?,
        Format::Pptx => office::pptx(bytes)?,
        Format::Spreadsheet => table::spreadsheet(bytes)?,
        Format::Odf => office::odf(bytes)?,
        Format::Email => mail::eml(bytes)?,
        Format::Mbox => mail::mbox(bytes)?,
        Format::Unconvertible(reason) => return Err(reason.to_string()),
        Format::Unknown => {
            let utf16 = encoding_rs::Encoding::for_bom(bytes).is_some();
            if !utf16 && bytes[..bytes.len().min(8192)].contains(&0) {
                return Err("a binary file in a format it does not read".into());
            }
            plain(&text::decode(bytes), false)
        }
    })
}

/// The text of a file, or why it has none. Never fails and never panics: a converter that does
/// is a corrupt file.
pub fn convert(format: Format, bytes: &[u8]) -> Outcome {
    let converted = match catch_unwind(AssertUnwindSafe(|| run(format, bytes))) {
        Ok(Ok(c)) => c,
        Ok(Err(reason)) => return Outcome::Unconverted(reason),
        Err(_) => return Outcome::Unconverted("cannot be read (the converter failed)".into()),
    };
    // A format whose whole point is its text has nothing to index without any.
    let must_have_text = !matches!(
        format,
        Format::Markdown | Format::Text | Format::Code | Format::Json | Format::Csv | Format::Tsv
    ) && !matches!(format, Format::Unknown);
    if must_have_text && !converted.text.chars().any(char::is_alphanumeric) {
        return Outcome::Unconverted(
            if format == Format::Pdf {
                "no text layer (a scan or an image-only PDF; OCR is not supported)"
            } else {
                "no text in it"
            }
            .into(),
        );
    }
    if format.cached() && converted.text.len() as u64 > MAX_TEXT_BYTES {
        return Outcome::Unconverted(format!(
            "converted text over {} MiB (left out)",
            MAX_TEXT_BYTES >> 20
        ));
    }
    Outcome::Text(converted)
}

#[derive(Serialize, Deserialize)]
struct Cached {
    version: u32,
    #[serde(flatten)]
    converted: Converted,
}

fn cache_file(dir: &Path, hash: &str) -> std::path::PathBuf {
    dir.join(format!("{hash}.json"))
}

/// A cached conversion of the file with this content hash, if one was made by this version.
pub fn cache_get(dir: &Path, hash: &str) -> Option<Converted> {
    let cached: Cached = serde_json::from_slice(&fs::read(cache_file(dir, hash)).ok()?).ok()?;
    (cached.version == VERSION).then_some(cached.converted)
}

/// Keeps a conversion for next time. A cache that cannot be written is only a slower next run.
pub fn cache_put(dir: &Path, hash: &str, converted: &Converted) {
    let cached = Cached {
        version: VERSION,
        converted: converted.clone(),
    };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec(&cached) {
        let tmp = dir.join(format!("{hash}.tmp"));
        if fs::write(&tmp, bytes).is_ok() {
            let _ = fs::rename(&tmp, cache_file(dir, hash));
        }
    }
}

/// Removes the cached conversions of files that are no longer in the source.
pub fn cache_prune(dir: &Path, keep: &HashSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let stale = match name.strip_suffix(".json") {
            Some(hash) => !keep.contains(hash),
            None => true,
        };
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(name: &str) -> Format {
        Format::of(Path::new(name))
    }

    #[test]
    fn formats_come_from_the_extension() {
        assert_eq!(fmt("a/B.PDF"), Format::Pdf);
        assert_eq!(fmt("x.docx"), Format::Docx);
        assert_eq!(fmt("x.yaml"), Format::Code);
        assert_eq!(fmt("Makefile"), Format::Unknown);
        assert!(matches!(fmt("scan.JPG"), Format::Unconvertible(_)));
        assert!(matches!(fmt("old.doc"), Format::Unconvertible(_)));
    }

    #[test]
    fn markdown_keeps_its_frontmatter_title_and_date_and_code_has_no_headings() {
        let Outcome::Text(c) = convert(
            Format::Markdown,
            b"---\ntitle: T\ndate: 2024-05-14\n---\nbody",
        ) else {
            panic!()
        };
        assert_eq!(
            (c.title.as_deref(), c.date.as_deref(), c.text.as_str()),
            (Some("T"), Some("2024-05-14"), "body")
        );
        let Outcome::Text(c) = convert(Format::Code, b"# a comment\nx = 1") else {
            panic!()
        };
        assert!(!c.prose);
    }

    #[test]
    fn json_is_laid_out_one_value_to_a_line() {
        let Outcome::Text(c) = convert(Format::Json, br#"{"a":1,"b":[2]}"#) else {
            panic!()
        };
        assert!(c.text.lines().count() > 3, "{}", c.text);
    }

    #[test]
    fn unknown_extensions_are_read_when_text_and_reported_when_binary() {
        assert!(matches!(
            convert(Format::Unknown, b"all: build\n"),
            Outcome::Text(_)
        ));
        assert!(matches!(
            convert(Format::Unknown, b"\x7fELF\0\0\0"),
            Outcome::Unconverted(_)
        ));
    }

    #[test]
    fn a_format_with_no_text_in_it_is_unconverted_and_a_crash_is_a_corrupt_file() {
        assert!(matches!(
            convert(Format::Html, b"<img src=x>"),
            Outcome::Unconverted(_)
        ));
        assert!(matches!(
            convert(Format::Docx, b"PK\x03\x04junk"),
            Outcome::Unconverted(_)
        ));
        assert!(matches!(
            convert(Format::Pdf, b"%PDF-1.7 \x00\xff"),
            Outcome::Unconverted(_)
        ));
    }

    #[test]
    fn the_cache_keeps_conversions_of_files_still_there_and_this_versions_only() {
        let dir = tempfile::tempdir().unwrap();
        let c = Converted {
            text: "t".into(),
            title: Some("T".into()),
            date: None,
            prose: true,
        };
        cache_put(dir.path(), "aa", &c);
        cache_put(dir.path(), "bb", &c);
        assert_eq!(cache_get(dir.path(), "aa"), Some(c.clone()));
        fs::write(
            dir.path().join("cc.json"),
            r#"{"version":0,"text":"","title":null,"date":null,"prose":true}"#,
        )
        .unwrap();
        assert_eq!(cache_get(dir.path(), "cc"), None);
        cache_prune(dir.path(), &HashSet::from(["aa".to_string()]));
        assert!(cache_get(dir.path(), "aa").is_some() && cache_get(dir.path(), "bb").is_none());
        assert!(!dir.path().join("cc.json").exists());
    }
}
