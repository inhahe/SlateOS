//! The books a reader has opened, and where they were in each.
//!
//! Until 2026-09-27 the reader opened on five books that do not exist --
//! "The Clockwork Garden" by "Eleanor Voss" and four more -- and had no way to
//! open a real one. It now opens plain-text files (`read_book`), and keeps the
//! list of them, with each one's place, bookmarks and type size, in
//! `<config>/ebook/library.txt` (`settingsfile::config_dir`), so the library is
//! the same the next time.
//!
//! **The file**, one line a book after a header:
//!
//! ```text
//! # SlateOS ebook library, format 1
//! <path>\t<offset>\t<type size>\t<bookmark>,<bookmark>,...
//! ```
//!
//! The path is written with `pathcodec` (design-decisions.md §426), which
//! escapes every byte outside printable ASCII -- a tab or a line break in a
//! name included -- so a name that is not text is kept whole and cannot break
//! the line it is on. Offsets are byte offsets into the book's text, the same
//! unit `ReadingState` keeps, so a place survives a change of type size.

use std::path::{Path, PathBuf};

use crate::{FontSizeLevel, ReadingState};

/// The first line of the file.
const HEADER: &str = "# SlateOS ebook library, format 1";

/// The most of a book file that is read: a text file bigger than this is read
/// as far as this, and says so. Sixty-four MiB is some forty times the longest
/// novel in Project Gutenberg; a file past it is almost certainly not a book.
pub const MAX_BOOK_BYTES: usize = 64 << 20;

/// One book on the shelf: where it is, and where the reader was in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shelved {
    /// The file.
    pub path: PathBuf,
    /// The place, the bookmarks and the type size.
    pub state: ReadingState,
}

/// Where the shelf is kept: `None` when there is no home folder to keep it in.
#[must_use]
pub fn shelf_path() -> Option<PathBuf> {
    settingsfile::config_dir().map(|dir| dir.join("ebook").join("library.txt"))
}

/// The file's text.
#[must_use]
pub fn to_text(books: &[Shelved]) -> String {
    let mut out = String::from(HEADER);
    out.push('\n');
    for book in books {
        let marks: Vec<String> = book
            .state
            .bookmarks
            .iter()
            .map(ToString::to_string)
            .collect();
        out.push_str(&pathcodec::encode_path(&book.path));
        out.push('\t');
        out.push_str(&book.state.offset.to_string());
        out.push('\t');
        out.push_str(book.state.font_size.label());
        out.push('\t');
        out.push_str(&marks.join(","));
        out.push('\n');
    }
    out
}

/// Read the file's text.
///
/// # Errors
///
/// Says which line is not a book. The whole file is refused rather than read
/// in part: saving what was understood would throw the rest away.
pub fn parse(text: &str) -> Result<Vec<Shelved>, String> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err("it is not an ebook library this reader knows".to_string());
    }
    let mut books = Vec::new();
    for (n, line) in lines.enumerate() {
        if line.is_empty() {
            continue;
        }
        let bad = || format!("line {} is not a book", n.saturating_add(2));
        let mut fields = line.split('\t');
        let (Some(path), Some(offset), Some(font), Some(marks), None) = (
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
            fields.next(),
        ) else {
            return Err(bad());
        };
        if path.is_empty() {
            return Err(bad());
        }
        let offset = offset.parse::<usize>().map_err(|_| bad())?;
        let font_size = FontSizeLevel::from_label(font).ok_or_else(bad)?;
        let mut bookmarks = Vec::new();
        for mark in marks.split(',').filter(|m| !m.is_empty()) {
            bookmarks.push(mark.parse::<usize>().map_err(|_| bad())?);
        }
        bookmarks.sort_unstable();
        bookmarks.dedup();
        books.push(Shelved {
            path: pathcodec::decode_path(path),
            state: ReadingState {
                offset,
                bookmarks,
                font_size,
            },
        });
    }
    Ok(books)
}

/// Read the shelf at `path`: none there is an empty shelf.
///
/// # Errors
///
/// The file is there and does not read, or does not parse.
pub fn load(path: &Path) -> Result<Vec<Shelved>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Write the shelf to `path`, whole or not at all.
///
/// # Errors
///
/// Its folder cannot be made, or the file cannot be written.
pub fn save(path: &Path, books: &[Shelved]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    safeio::write_str_atomically(path, &to_text(books))
}

/// A book file read: its text, and anything worth saying about reading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadText {
    /// The text, with its line breaks as `\n`.
    pub text: String,
    /// The title the text gives itself, if it gives one.
    pub title: Option<String>,
    /// The author it names, if it names one.
    pub author: Option<String>,
    /// What the reader should know: a text read in part, or read as
    /// Windows-1252 because it is not UTF-8.
    pub notes: Vec<String>,
}

/// Read a plain-text book, at most `cap` bytes of it.
///
/// UTF-8 is read as it is (a byte-order mark dropped). UTF-16 with a
/// byte-order mark is read as UTF-16. Anything else is read as Windows-1252,
/// in which every byte is a character -- the encoding most older plain-text
/// books that are not UTF-8 are in -- and a note says so, so a book that comes
/// out wrong says why. Never a lossy decode, which would turn every such
/// character into the same replacement mark without a word.
///
/// # Errors
///
/// The file cannot be read, or says it is UTF-8 or UTF-16 and is not.
pub fn read_book(path: &Path, cap: usize) -> Result<ReadText, String> {
    let read = safeio::read_capped(path, cap).map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    if read.truncated {
        notes.push(format!(
            "Only the first {} of the file were read.",
            amount(cap)
        ));
    }
    let text = decode(read.bytes, read.truncated, &mut notes)?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let (title, author) = header_fields(&text);
    Ok(ReadText {
        text,
        title,
        author,
        notes,
    })
}

/// `cap` as a person would say it.
fn amount(cap: usize) -> String {
    if cap >= 1 << 20 {
        format!("{} MiB", cap >> 20)
    } else {
        format!("{cap} bytes")
    }
}

/// The bytes of a book as text, by what they are.
///
/// `truncated` says the bytes stop where a cap stopped them rather than where
/// the file ends, so a character the cap cut in two is dropped rather than
/// taken as proof that the text is not what it says.
fn decode(bytes: Vec<u8>, truncated: bool, notes: &mut Vec<String>) -> Result<String, String> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return utf8(rest.to_vec(), truncated)
            .map_err(|_| "the file says it is UTF-8 and is not".to_string());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes, truncated);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes, truncated);
    }
    match utf8(bytes, truncated) {
        Ok(text) => Ok(text),
        Err(bytes) => {
            notes.push("Read as Windows-1252: the file is not UTF-8.".to_string());
            Ok(bytes.iter().map(|&b| windows_1252(b)).collect())
        }
    }
}

/// `bytes` as UTF-8, or the bytes back if they are not -- except that a
/// character cut in two at the very end of a truncated read is dropped.
fn utf8(bytes: Vec<u8>, truncated: bool) -> Result<String, Vec<u8>> {
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(e) if truncated && e.utf8_error().error_len().is_none() => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            // Everything before `valid_up_to` is UTF-8 by its definition, so
            // this cannot fail; were it to, the bytes go back to the caller.
            String::from_utf8(bytes).map_err(std::string::FromUtf8Error::into_bytes)
        }
        Err(e) => Err(e.into_bytes()),
    }
}

/// UTF-16 in the byte order `unit` reads.
fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16, truncated: bool) -> Result<String, String> {
    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .filter_map(|pair| <[u8; 2]>::try_from(pair).ok())
        .map(unit)
        .collect();
    if truncated && units.last().is_some_and(|u| (0xD800..0xDC00).contains(u)) {
        // The cap cut a surrogate pair in two.
        units.pop();
    }
    String::from_utf16(&units).map_err(|_| "the file says it is UTF-16 and is not".to_string())
}

/// One Windows-1252 byte as the character it stands for.
///
/// The printable ASCII and Latin-1 ranges are themselves; 0x80..=0x9F are the
/// typographer's characters Windows put there -- quotes, dashes, the euro.
/// The five bytes Windows-1252 leaves unassigned are shown as themselves (C1
/// controls), as a browser would.
fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}',
        '\u{017D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}',
        '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}',
        '\u{9D}', '\u{017E}', '\u{0178}',
    ];
    b.checked_sub(0x80)
        .and_then(|i| HIGH.get(usize::from(i)))
        .copied()
        .unwrap_or(char::from(b))
}

/// The `Title:` and `Author:` a text gives itself in its first lines, as
/// Project Gutenberg's books do.
fn header_fields(text: &str) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut author = None;
    for line in text.lines().take(60) {
        let line = line.trim();
        if title.is_none()
            && let Some(rest) = line.strip_prefix("Title:")
            && !rest.trim().is_empty()
        {
            title = Some(rest.trim().to_string());
        } else if author.is_none()
            && let Some(rest) = line.strip_prefix("Author:")
            && !rest.trim().is_empty()
        {
            author = Some(rest.trim().to_string());
        }
    }
    (title, author)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;

    fn state(offset: usize, bookmarks: &[usize], font_size: FontSizeLevel) -> ReadingState {
        ReadingState {
            offset,
            bookmarks: bookmarks.to_vec(),
            font_size,
        }
    }

    #[test]
    fn a_library_is_read_back_as_it_was_written() {
        let books = vec![
            Shelved {
                path: PathBuf::from("/home/ann/books/Pride and Prejudice.txt"),
                state: state(1234, &[10, 900], FontSizeLevel::Large),
            },
            Shelved {
                // A tab, a line break and a percent sign in a name: each
                // would break the line or the escape if written as it is.
                path: PathBuf::from("/home/ann/odd\tname\nhere 100%.txt"),
                state: state(0, &[], FontSizeLevel::Small),
            },
        ];
        let text = to_text(&books);
        assert_eq!(
            text.lines().count(),
            3,
            "a header and one line a book: {text:?}"
        );
        assert_eq!(parse(&text).unwrap(), books);
    }

    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_text_is_kept_whole() {
        use std::os::unix::ffi::OsStrExt;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(b"/books/caf\xE9.txt"));
        let books = vec![Shelved {
            path: path.clone(),
            state: state(5, &[], FontSizeLevel::Medium),
        }];
        assert_eq!(parse(&to_text(&books)).unwrap()[0].path, path);
    }

    #[test]
    fn a_file_that_is_not_a_library_is_refused_whole() {
        assert!(parse("").is_err(), "an empty file has no header");
        assert!(parse("some notes\n").is_err());
        let good = format!("{HEADER}\n/a.txt\t0\tMedium\t\n");
        assert!(parse(&good).is_ok());
        for bad in [
            "/a.txt\t0\tMedium",
            "/a.txt\t0\tMedium\t\textra",
            "/a.txt\tten\tMedium\t",
            "/a.txt\t0\tHuge\t",
            "/a.txt\t0\tMedium\t1,x",
            "\t0\tMedium\t",
        ] {
            let err = parse(&format!("{good}{bad}\n")).unwrap_err();
            assert!(err.contains("line 3"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn bookmarks_are_read_in_book_order_once_each() {
        let text = format!("{HEADER}\n/a.txt\t0\tMedium\t30,10,30,20\n");
        assert_eq!(parse(&text).unwrap()[0].state.bookmarks, vec![10, 20, 30]);
    }

    #[test]
    fn no_file_is_an_empty_library_and_a_damaged_one_is_an_error() {
        let scratch = scratchdir::ScratchDir::new("ebook_shelf_load");
        let path = scratch.dir().join("ebook").join("library.txt");
        assert_eq!(load(&path).unwrap(), Vec::new());
        save(&path, &[]).unwrap();
        assert_eq!(load(&path).unwrap(), Vec::new());
        std::fs::write(&path, "not a library").unwrap();
        assert!(load(&path).is_err());
    }

    fn read(bytes: &[u8]) -> ReadText {
        let scratch = scratchdir::ScratchDir::new("ebook_read_book");
        let path = scratch.dir().join("book.txt");
        std::fs::write(&path, bytes).unwrap();
        read_book(&path, MAX_BOOK_BYTES).unwrap()
    }

    #[test]
    fn utf8_is_read_as_it_is_and_says_nothing() {
        let got = read("Caf\u{e9} \u{201C}quoted\u{201D}".as_bytes());
        assert_eq!(got.text, "Caf\u{e9} \u{201C}quoted\u{201D}");
        assert!(got.notes.is_empty(), "{:?}", got.notes);
    }

    #[test]
    fn a_byte_order_mark_is_not_part_of_the_text() {
        assert_eq!(read(b"\xEF\xBB\xBFHello").text, "Hello");
    }

    #[test]
    fn utf16_with_a_byte_order_mark_is_read_either_way_round() {
        assert_eq!(read(b"\xFF\xFEH\x00i\x00").text, "Hi");
        assert_eq!(read(b"\xFE\xFF\x00H\x00i").text, "Hi");
    }

    #[test]
    fn text_that_is_not_utf8_is_read_as_windows_1252_and_says_so() {
        let got = read(b"\x93Caf\xE9\x94 \x96 \x80");
        assert_eq!(got.text, "\u{201C}Caf\u{e9}\u{201D} \u{2013} \u{20AC}");
        assert_eq!(got.notes.len(), 1, "{:?}", got.notes);
        assert!(got.notes[0].contains("Windows-1252"), "{:?}", got.notes);
    }

    #[test]
    fn line_breaks_of_every_kind_become_one() {
        assert_eq!(read(b"a\r\nb\rc\nd").text, "a\nb\nc\nd");
    }

    #[test]
    fn a_title_and_author_the_text_gives_are_read() {
        let got = read(b"The Project Gutenberg eBook\r\n\r\nTitle: Emma\r\n\r\nAuthor: Jane Austen\r\n\r\nChapter I");
        assert_eq!(got.title.as_deref(), Some("Emma"));
        assert_eq!(got.author.as_deref(), Some("Jane Austen"));
        assert_eq!(read(b"Just a story.").title, None);
    }

    #[test]
    fn a_book_past_the_cap_is_read_in_part_and_says_so() {
        let scratch = scratchdir::ScratchDir::new("ebook_read_cap");
        let path = scratch.dir().join("long.txt");
        // "é" is two bytes; a cap of 5 cuts the third one in two.
        std::fs::write(&path, "\u{e9}\u{e9}\u{e9}\u{e9}".as_bytes()).unwrap();
        let got = read_book(&path, 5).unwrap();
        assert_eq!(
            got.text, "\u{e9}\u{e9}",
            "a character cut in two is dropped, not misread"
        );
        assert_eq!(got.notes.len(), 1, "{:?}", got.notes);
        assert!(got.notes[0].contains("first 5 bytes"), "{:?}", got.notes);
    }

    #[test]
    fn a_missing_file_is_an_error_not_an_empty_book() {
        let scratch = scratchdir::ScratchDir::new("ebook_read_missing");
        assert!(read_book(&scratch.dir().join("gone.txt"), MAX_BOOK_BYTES).is_err());
    }

    #[test]
    fn windows_1252_leaves_ascii_and_latin_1_alone() {
        assert_eq!(windows_1252(b'A'), 'A');
        assert_eq!(windows_1252(0xE9), '\u{e9}');
        assert_eq!(windows_1252(0x80), '\u{20AC}');
        assert_eq!(windows_1252(0x9F), '\u{0178}');
        assert_eq!(windows_1252(0x81), '\u{81}');
    }
}
