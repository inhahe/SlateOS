//! Reading the PDF file format: enough of it to say what is in a document.
//!
//! This crate drew PDFs before it could read one. The window had a document
//! model, a page tree, thumbnails and a search box, and the only thing that
//! ever filled them was `PdfDocument::create_sample`, which invents pages.
//! So "open a PDF" was the one thing a PDF viewer could not do.
//!
//! What is here is the spine of the format, in the order a reader has to walk
//! it:
//!
//! 1. **Objects.** Eight types, nested. [`Object`] and [`Lexer`].
//! 2. **The cross-reference table**, which maps an object number to a byte
//!    offset. A file is read back-to-front: `startxref` at the end gives the
//!    offset of the table, the table gives the offset of everything else.
//! 3. **The trailer**, whose `/Root` names the catalog.
//! 4. **The page tree**, a tree of `/Pages` nodes with `/Kids`, whose leaves
//!    are `/Page` objects carrying a `/MediaBox`.
//!
//! **What is deliberately not here yet.** Content streams -- the drawing
//! instructions inside a page -- are not interpreted, so this reports a
//! document's shape and not its text. Cross-reference *streams* (PDF 1.5's
//! compressed replacement for the table) are not read either, so a file using
//! them is refused by name rather than half-read; [`Error::XrefStream`] says
//! so. Refusing is the point: a viewer that opened such a file and showed
//! zero pages would be indistinguishable from an empty document.
//!
//! **Measured against files this did not write.** Every fixture in the tests
//! below is assembled here, which proves the parser agrees with itself and
//! nothing more, so it was also run over three PDFs produced by other
//! software: a 13.8 MB, 122-page manual (v1.4), a 3-page quick-start (v1.4)
//! and a 47-page guide (v1.5). All three came back with the right page counts
//! and real page sizes -- A4 at 595.28 x 841.89 points, and the quick-start
//! landscape at 841.89 x 595.28, which is a wide `/MediaBox` and not a
//! rotation. That run is not a test here, because it depends on files that
//! happen to be on one machine, and a test that passes by skipping is worse
//! than no test.
//!
//! **Hostile input is the normal case.** A PDF is a file from elsewhere, and
//! every length in it is a claim. Nothing here allocates on the strength of a
//! header, every offset is bounds-checked against the real file, the object
//! graph is walked with an explicit depth cap and a visited set (a `/Kids`
//! array can point at its own parent), and the same discipline
//! `gui/imagecodec` documents for PNG applies for the same reason.

use std::collections::{BTreeMap, BTreeSet};

/// How deep a page tree may nest before this gives up.
///
/// Real documents are two or three deep; the tree exists to keep the page
/// list from being one enormous array. A file claiming hundreds of levels is
/// not a document anyone made, and recursion on attacker-chosen depth is a
/// stack overflow with extra steps.
const MAX_TREE_DEPTH: usize = 64;

/// Why a file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No `%PDF-` header. Not a PDF at all.
    NotAPdf,
    /// No `startxref`, or it does not point at a table.
    NoXref,
    /// The cross-reference is a stream, which this does not read yet.
    XrefStream,
    /// The trailer has no `/Root`, or it does not resolve to a catalog.
    NoCatalog,
    /// The catalog has no page tree.
    NoPages,
    /// The file ends in the middle of something.
    Truncated,
    /// A number, name or delimiter was expected and something else was there.
    Malformed(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Self::NotAPdf => "not a PDF file (no %PDF- header)",
            Self::NoXref => "no cross-reference table",
            Self::XrefStream => "this PDF uses a cross-reference stream, which is not read yet",
            Self::NoCatalog => "no document catalog",
            Self::NoPages => "no page tree",
            Self::Truncated => "the file ends mid-object",
            Self::Malformed(what) => what,
        };
        f.write_str(text)
    }
}

/// One PDF object.
///
/// `Ref` is the indirect reference `12 0 R`, which is a pointer into the
/// cross-reference table rather than a value. Resolving it is the caller's
/// job because resolution needs the whole file; keeping it as a variant is
/// what lets a dictionary be parsed without one.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Null,
    Bool(bool),
    /// PDF does not distinguish integers from reals in most places, but the
    /// two are lexed differently and an object number must be whole.
    Int(i64),
    Real(f64),
    /// A literal or hex string, as bytes. **Not** a `String`: a PDF string is
    /// arbitrary bytes in an encoding the document declares elsewhere, and
    /// deciding it is UTF-8 here would be the lossy conversion this project
    /// forbids on data that came from outside.
    Str(Vec<u8>),
    /// A `/Name`, without the slash.
    Name(String),
    Array(Vec<Object>),
    Dict(BTreeMap<String, Object>),
    /// A stream: its dictionary, and where its bytes are in the file.
    ///
    /// The bytes are a range rather than a copy so that opening a large
    /// document does not duplicate it in memory before anything asks for a
    /// page.
    Stream {
        dict: BTreeMap<String, Object>,
        start: usize,
        len: usize,
    },
    /// `n g R`.
    Ref(u32, u16),
}

impl Object {
    /// The dictionary inside this object, whether it is one or leads a stream.
    #[must_use]
    pub fn as_dict(&self) -> Option<&BTreeMap<String, Object>> {
        match self {
            Self::Dict(d) | Self::Stream { dict: d, .. } => Some(d),
            _ => None,
        }
    }

    /// This object as a number, whichever way it was written.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => {
                // `i64 as f64` is lossy above 2^53, and a coordinate that
                // large is not a coordinate. Refusing is better than rounding
                // silently.
                if i.unsigned_abs() <= (1u64 << 53) {
                    #[allow(clippy::cast_precision_loss, reason = "bounded above")]
                    Some(*i as f64)
                } else {
                    None
                }
            }
            Self::Real(r) => Some(*r),
            _ => None,
        }
    }
}

/// Whether `b` ends a token.
const fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Whether `b` is PDF whitespace. Note the NUL: the format says so.
const fn is_space(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

/// A reader positioned in a PDF file.
pub struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Start reading at `pos`, which is checked against the file's length.
    pub fn seek(&mut self, pos: usize) -> Result<(), Error> {
        if pos > self.data.len() {
            return Err(Error::Truncated);
        }
        self.pos = pos;
        Ok(())
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    /// Whitespace and comments. A `%` runs to the end of the line.
    fn skip_space(&mut self) {
        while let Some(b) = self.peek() {
            if is_space(b) {
                self.pos = self.pos.saturating_add(1);
            } else if b == b'%' {
                while let Some(c) = self.peek() {
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                    self.pos = self.pos.saturating_add(1);
                }
            } else {
                break;
            }
        }
    }

    /// The next bare token: a keyword, a number, anything not a delimiter.
    fn token(&mut self) -> &'a [u8] {
        self.skip_space();
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_space(b) || is_delimiter(b) {
                break;
            }
            self.pos = self.pos.saturating_add(1);
        }
        self.data.get(start..self.pos).unwrap_or_default()
    }

    /// Read `word` if it is next, and say whether it was.
    fn eat(&mut self, word: &[u8]) -> bool {
        self.skip_space();
        let end = self.pos.saturating_add(word.len());
        if self.data.get(self.pos..end) == Some(word) {
            self.pos = end;
            true
        } else {
            false
        }
    }

    /// One object, with its nesting.
    pub fn object(&mut self) -> Result<Object, Error> {
        self.object_at_depth(0)
    }

    fn object_at_depth(&mut self, depth: usize) -> Result<Object, Error> {
        if depth > MAX_TREE_DEPTH {
            return Err(Error::Malformed("object nesting is too deep"));
        }
        self.skip_space();
        let Some(b) = self.peek() else {
            return Err(Error::Truncated);
        };
        match b {
            b'/' => self.name(),
            b'(' => self.literal_string(),
            b'[' => {
                self.pos = self.pos.saturating_add(1);
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    match self.peek() {
                        None => return Err(Error::Truncated),
                        Some(b']') => {
                            self.pos = self.pos.saturating_add(1);
                            break;
                        }
                        Some(_) => items.push(self.object_at_depth(depth.saturating_add(1))?),
                    }
                }
                Ok(Object::Array(items))
            }
            b'<' => {
                if self.data.get(self.pos.saturating_add(1)) == Some(&b'<') {
                    self.dictionary(depth)
                } else {
                    self.hex_string()
                }
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number_or_ref(),
            _ => {
                let word = self.token();
                match word {
                    b"true" => Ok(Object::Bool(true)),
                    b"false" => Ok(Object::Bool(false)),
                    b"null" => Ok(Object::Null),
                    b"" => Err(Error::Truncated),
                    _ => Err(Error::Malformed("unknown keyword where an object was due")),
                }
            }
        }
    }

    fn name(&mut self) -> Result<Object, Error> {
        // Past the slash.
        self.pos = self.pos.saturating_add(1);
        let mut out = String::new();
        while let Some(b) = self.peek() {
            if is_space(b) || is_delimiter(b) {
                break;
            }
            self.pos = self.pos.saturating_add(1);
            if b == b'#' {
                // `#41` is `A`. Two hex digits, and a `#` without them is the
                // file's problem, not ours.
                let hi = self.peek().and_then(hex_val);
                let lo = self
                    .data
                    .get(self.pos.saturating_add(1))
                    .copied()
                    .and_then(hex_val);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    self.pos = self.pos.saturating_add(2);
                    out.push(char::from(hi.saturating_mul(16).saturating_add(lo)));
                    continue;
                }
                return Err(Error::Malformed("a # in a name without two hex digits"));
            }
            out.push(char::from(b));
        }
        Ok(Object::Name(out))
    }

    fn literal_string(&mut self) -> Result<Object, Error> {
        self.pos = self.pos.saturating_add(1);
        let mut out = Vec::new();
        let mut depth = 1usize;
        while let Some(b) = self.peek() {
            self.pos = self.pos.saturating_add(1);
            match b {
                b'\\' => {
                    let Some(esc) = self.peek() else {
                        return Err(Error::Truncated);
                    };
                    self.pos = self.pos.saturating_add(1);
                    match esc {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\n' => {}
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos = self.pos.saturating_add(1);
                            }
                        }
                        b'0'..=b'7' => {
                            // Up to three octal digits.
                            let mut val = u32::from(esc.saturating_sub(b'0'));
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        self.pos = self.pos.saturating_add(1);
                                        val = val
                                            .saturating_mul(8)
                                            .saturating_add(u32::from(d.saturating_sub(b'0')));
                                    }
                                    _ => break,
                                }
                            }
                            out.push(u8::try_from(val & 0xff).unwrap_or(0));
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth = depth.saturating_add(1);
                    out.push(b);
                }
                b')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Ok(Object::Str(out));
                    }
                    out.push(b);
                }
                _ => out.push(b),
            }
        }
        Err(Error::Truncated)
    }

    fn hex_string(&mut self) -> Result<Object, Error> {
        self.pos = self.pos.saturating_add(1);
        let mut out = Vec::new();
        let mut half: Option<u8> = None;
        while let Some(b) = self.peek() {
            self.pos = self.pos.saturating_add(1);
            if b == b'>' {
                // An odd digit count pads with zero, which the spec requires.
                if let Some(hi) = half {
                    out.push(hi.saturating_mul(16));
                }
                return Ok(Object::Str(out));
            }
            if is_space(b) {
                continue;
            }
            let Some(v) = hex_val(b) else {
                return Err(Error::Malformed("a non-hex digit inside <>"));
            };
            match half {
                None => half = Some(v),
                Some(hi) => {
                    out.push(hi.saturating_mul(16).saturating_add(v));
                    half = None;
                }
            }
        }
        Err(Error::Truncated)
    }

    fn dictionary(&mut self, depth: usize) -> Result<Object, Error> {
        self.pos = self.pos.saturating_add(2);
        let mut map = BTreeMap::new();
        loop {
            self.skip_space();
            if self.eat(b">>") {
                break;
            }
            let Some(b'/') = self.peek() else {
                return Err(Error::Malformed("a dictionary key that is not a name"));
            };
            let Object::Name(key) = self.name()? else {
                return Err(Error::Malformed("a dictionary key that is not a name"));
            };
            let value = self.object_at_depth(depth.saturating_add(1))?;
            map.insert(key, value);
        }
        // A stream's bytes follow its dictionary.
        let save = self.pos;
        self.skip_space();
        if self.eat(b"stream") {
            // Exactly one CRLF or LF, and nothing else, precedes the data.
            if self.peek() == Some(b'\r') {
                self.pos = self.pos.saturating_add(1);
            }
            if self.peek() == Some(b'\n') {
                self.pos = self.pos.saturating_add(1);
            }
            let start = self.pos;
            // `/Length` may be an indirect reference, which cannot be resolved
            // from here. When it is, the caller re-reads the stream once the
            // xref is built; until then the range is empty rather than wrong.
            let len = match map.get("Length").and_then(Object::as_f64) {
                Some(n) if n >= 0.0 => {
                    #[allow(clippy::cast_possible_truncation, reason = "checked below")]
                    #[allow(clippy::cast_sign_loss, reason = "checked non-negative")]
                    let n = n as usize;
                    if start.saturating_add(n) <= self.data.len() {
                        n
                    } else {
                        return Err(Error::Truncated);
                    }
                }
                _ => 0,
            };
            self.pos = start.saturating_add(len);
            return Ok(Object::Stream {
                dict: map,
                start,
                len,
            });
        }
        self.pos = save;
        Ok(Object::Dict(map))
    }

    /// A number, or the `n g R` that only looks like two numbers.
    fn number_or_ref(&mut self) -> Result<Object, Error> {
        let save = self.pos;
        let first = self.token();
        let Some(value) = parse_number(first) else {
            return Err(Error::Malformed("a number that will not parse"));
        };
        if let Object::Int(n) = value {
            if n >= 0 {
                let after_first = self.pos;
                let second = self.token();
                if let Some(Object::Int(g)) = parse_number(second) {
                    if g >= 0 && self.eat(b"R") {
                        let num = u32::try_from(n).unwrap_or(0);
                        let generation = u16::try_from(g).unwrap_or(0);
                        return Ok(Object::Ref(num, generation));
                    }
                }
                // Not a reference after all. The second token belongs to
                // whoever asked next.
                self.pos = after_first;
                return Ok(value);
            }
        }
        self.pos = save;
        let _ = self.token();
        Ok(value)
    }
}

const fn hex_val(b: u8) -> Option<u8> {
    // Saturating throughout: the ranges make each subtraction safe, but the
    // crate denies bare arithmetic so that the next edit to this function does
    // not have to be re-argued.
    match b {
        b'0'..=b'9' => Some(b.saturating_sub(b'0')),
        b'a'..=b'f' => Some(b.saturating_sub(b'a').saturating_add(10)),
        b'A'..=b'F' => Some(b.saturating_sub(b'A').saturating_add(10)),
        _ => None,
    }
}

/// A PDF number: integer or real, with an optional sign.
fn parse_number(token: &[u8]) -> Option<Object> {
    if token.is_empty() {
        return None;
    }
    let text = core::str::from_utf8(token).ok()?;
    if text.contains('.') {
        // "4." and ".5" are both legal PDF and both rejected by Rust's parser,
        // so they are padded rather than refused.
        let padded = if text.starts_with('.') {
            format!("0{text}")
        } else if text.ends_with('.') {
            format!("{text}0")
        } else {
            text.to_owned()
        };
        padded.parse::<f64>().ok().map(Object::Real)
    } else {
        text.parse::<i64>().ok().map(Object::Int)
    }
}

/// How many pages a document may declare before this stops following it.
///
/// The page tree is walked with a visited set, so a cycle cannot spin here;
/// this bounds the honest-but-absurd case instead.
const MAX_PAGES: usize = 100_000;

/// One page's geometry, as the file declares it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageInfo {
    /// Width in points, after `/Rotate` has been applied.
    pub width: f32,
    /// Height in points, after `/Rotate` has been applied.
    pub height: f32,
    /// The page's rotation, normalised to 0, 90, 180 or 270.
    pub rotation: i32,
}

/// What a PDF file says it contains.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The version from the `%PDF-` header, as written: "1.4", "1.7".
    pub version: String,
    /// One entry per page, in reading order.
    pub pages: Vec<PageInfo>,
}

/// The default page box when a file declares none anywhere.
///
/// US Letter, which is what the specification names. A file with no
/// `/MediaBox` at any level is malformed; guessing Letter matches every other
/// reader and is better than refusing a document over a missing default.
const DEFAULT_BOX: (f32, f32) = (612.0, 792.0);

/// Values a page inherits from its ancestors in the tree.
#[derive(Debug, Clone, Copy)]
struct Inherited {
    box_pt: (f32, f32),
    rotation: i32,
}

/// Read `bytes` as a PDF document.
///
/// # Errors
///
/// Returns [`Error`] describing what about the file could not be read. Every
/// one of them is a refusal rather than a guess: a viewer that opens a file it
/// did not understand and shows nothing has told the reader the document is
/// empty, which is a claim about the document.
pub fn read(bytes: &[u8]) -> Result<Document, Error> {
    let version = header_version(bytes)?;
    let start = startxref_offset(bytes)?;

    let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
    let mut trailer: Option<BTreeMap<String, Object>> = None;
    let mut seen_sections: BTreeSet<usize> = BTreeSet::new();
    let mut next = Some(start);
    while let Some(pos) = next {
        // An incremental update chains back through `/Prev`, and a file can
        // point one at itself. The set is what makes that terminate.
        if !seen_sections.insert(pos) {
            break;
        }
        let (dict, prev) = read_xref_section(bytes, pos, &mut offsets)?;
        if trailer.is_none() {
            trailer = Some(dict);
        }
        next = prev;
    }

    let trailer = trailer.ok_or(Error::NoXref)?;
    let root = trailer.get("Root").ok_or(Error::NoCatalog)?;
    let catalog = resolve(bytes, &offsets, root, 0)?;
    let pages_ref = catalog
        .as_dict()
        .and_then(|d| d.get("Pages"))
        .ok_or(Error::NoPages)?
        .clone();

    let mut pages = Vec::new();
    let mut seen_nodes = BTreeSet::new();
    let inherited = Inherited {
        box_pt: DEFAULT_BOX,
        rotation: 0,
    };
    walk_pages(
        bytes,
        &offsets,
        &pages_ref,
        inherited,
        0,
        &mut seen_nodes,
        &mut pages,
    )?;
    if pages.is_empty() {
        return Err(Error::NoPages);
    }
    Ok(Document { version, pages })
}

/// The version in the `%PDF-` header.
fn header_version(bytes: &[u8]) -> Result<String, Error> {
    // Within the first kilobyte: a file is allowed leading junk, and readers
    // are expected to tolerate it.
    let window_end = bytes.len().min(1024);
    let window = bytes.get(..window_end).unwrap_or_default();
    let at = find(window, b"%PDF-").ok_or(Error::NotAPdf)?;
    let rest = window.get(at.saturating_add(5)..).unwrap_or_default();
    let mut version = String::new();
    for &b in rest {
        if b.is_ascii_digit() || b == b'.' {
            version.push(char::from(b));
        } else {
            break;
        }
    }
    if version.is_empty() {
        return Err(Error::NotAPdf);
    }
    Ok(version)
}

/// Where `startxref` says the cross-reference table is.
fn startxref_offset(bytes: &[u8]) -> Result<usize, Error> {
    // From the end: the last one wins, because an incrementally updated file
    // appends a whole new trailer.
    let tail_start = bytes.len().saturating_sub(2048);
    let tail = bytes.get(tail_start..).unwrap_or_default();
    let at = rfind(tail, b"startxref").ok_or(Error::NoXref)?;
    let mut lex = Lexer::new(bytes);
    lex.seek(tail_start.saturating_add(at).saturating_add(9))?;
    match lex.object()? {
        Object::Int(n) if n >= 0 => {
            let off = usize::try_from(n).map_err(|_| Error::NoXref)?;
            if off >= bytes.len() {
                return Err(Error::NoXref);
            }
            Ok(off)
        }
        _ => Err(Error::NoXref),
    }
}

/// One `xref` section and its trailer, adding offsets to `offsets`.
///
/// Returns the trailer dictionary and the `/Prev` offset, if any.
fn read_xref_section(
    data: &[u8],
    pos: usize,
    offsets: &mut BTreeMap<u32, usize>,
) -> Result<(BTreeMap<String, Object>, Option<usize>), Error> {
    let mut lex = Lexer::new(data);
    lex.seek(pos)?;
    if !lex.eat(b"xref") {
        // PDF 1.5 replaced the table with a compressed stream. Saying so by
        // name beats reporting a document with no pages.
        return Err(Error::XrefStream);
    }
    loop {
        if lex.eat(b"trailer") {
            break;
        }
        let first = parse_number(lex.token());
        let Some(Object::Int(start)) = first else {
            return Err(Error::Malformed(
                "an xref subsection that is not two numbers",
            ));
        };
        let second = parse_number(lex.token());
        let Some(Object::Int(count)) = second else {
            return Err(Error::Malformed(
                "an xref subsection that is not two numbers",
            ));
        };
        if start < 0 || count < 0 {
            return Err(Error::Malformed("a negative xref subsection"));
        }
        let count = usize::try_from(count).unwrap_or(0);
        for i in 0..count {
            let off = parse_number(lex.token());
            let Some(Object::Int(off)) = off else {
                return Err(Error::Malformed("an xref entry without an offset"));
            };
            // The generation is read and discarded: this resolves by object
            // number, and a file that reuses a number with a higher generation
            // has superseded the old one through the /Prev chain already.
            let _generation = lex.token();
            let kind = lex.token();
            if kind == b"n" && off >= 0 {
                let Ok(num) =
                    u32::try_from(start.saturating_add(i64::try_from(i).unwrap_or(i64::MAX)))
                else {
                    continue;
                };
                let Ok(off) = usize::try_from(off) else {
                    continue;
                };
                if off < data.len() {
                    // The newest section is read first, so an entry already
                    // here came from a later update and wins.
                    offsets.entry(num).or_insert(off);
                }
            }
        }
    }
    let Object::Dict(dict) = lex.object()? else {
        return Err(Error::Malformed("a trailer that is not a dictionary"));
    };
    let prev = dict
        .get("Prev")
        .and_then(Object::as_f64)
        .filter(|p| *p >= 0.0)
        .and_then(|p| {
            let clamped = p.min(4_294_967_295.0);
            let as_usize = usize::try_from(clamped as u64).ok()?;
            (as_usize < data.len()).then_some(as_usize)
        });
    Ok((dict, prev))
}

/// The object at `offset`, past its `n g obj` header.
fn object_at(data: &[u8], offset: usize) -> Result<Object, Error> {
    let mut lex = Lexer::new(data);
    lex.seek(offset)?;
    let _number = lex.token();
    let _generation = lex.token();
    if !lex.eat(b"obj") {
        return Err(Error::Malformed("an xref entry not pointing at an object"));
    }
    lex.object()
}

/// Follow `object` through the cross-reference table until it is a value.
fn resolve(
    data: &[u8],
    offsets: &BTreeMap<u32, usize>,
    object: &Object,
    depth: usize,
) -> Result<Object, Error> {
    if depth > MAX_TREE_DEPTH {
        return Err(Error::Malformed("a reference chain that does not end"));
    }
    match object {
        Object::Ref(number, _) => {
            // A dangling reference is `null` by the specification, not an
            // error: a file may point at an object it did not include.
            let Some(offset) = offsets.get(number) else {
                return Ok(Object::Null);
            };
            let target = object_at(data, *offset)?;
            resolve(data, offsets, &target, depth.saturating_add(1))
        }
        other => Ok(other.clone()),
    }
}

/// Walk the page tree, appending a [`PageInfo`] per leaf.
fn walk_pages(
    data: &[u8],
    offsets: &BTreeMap<u32, usize>,
    node_ref: &Object,
    inherited: Inherited,
    depth: usize,
    seen: &mut BTreeSet<u32>,
    out: &mut Vec<PageInfo>,
) -> Result<(), Error> {
    if depth > MAX_TREE_DEPTH || out.len() >= MAX_PAGES {
        return Ok(());
    }
    // A /Kids array may name its own parent, which is a cycle and a hang.
    if let Object::Ref(number, _) = node_ref {
        if !seen.insert(*number) {
            return Ok(());
        }
    }
    let node = resolve(data, offsets, node_ref, 0)?;
    let Some(dict) = node.as_dict() else {
        return Ok(());
    };

    // /MediaBox and /Rotate are inheritable: a leaf without one uses its
    // nearest ancestor's, which is why they are carried down rather than read
    // at the leaf.
    let mut here = inherited;
    if let Some(found) = media_box(data, offsets, dict) {
        here.box_pt = found;
    }
    if let Some(rotation) = dict.get("Rotate").and_then(Object::as_f64) {
        here.rotation = normalise_rotation(rotation);
    }

    let is_leaf = matches!(dict.get("Type"), Some(Object::Name(t)) if t == "Page");
    if is_leaf {
        let (mut width, mut height) = here.box_pt;
        if here.rotation == 90 || here.rotation == 270 {
            core::mem::swap(&mut width, &mut height);
        }
        out.push(PageInfo {
            width,
            height,
            rotation: here.rotation,
        });
        return Ok(());
    }

    let kids = dict.get("Kids").cloned().unwrap_or(Object::Null);
    let kids = resolve(data, offsets, &kids, 0)?;
    let Object::Array(kids) = kids else {
        return Ok(());
    };
    for kid in &kids {
        walk_pages(data, offsets, kid, here, depth.saturating_add(1), seen, out)?;
    }
    Ok(())
}

/// A `/MediaBox` as width and height in points.
fn media_box(
    data: &[u8],
    offsets: &BTreeMap<u32, usize>,
    dict: &BTreeMap<String, Object>,
) -> Option<(f32, f32)> {
    let raw = dict.get("MediaBox")?;
    let resolved = resolve(data, offsets, raw, 0).ok()?;
    let Object::Array(items) = resolved else {
        return None;
    };
    let mut corners = [0.0f64; 4];
    if items.len() != 4 {
        return None;
    }
    for (slot, item) in corners.iter_mut().zip(items.iter()) {
        // A corner may itself be an indirect reference.
        let value = resolve(data, offsets, item, 0).ok()?;
        *slot = value.as_f64()?;
    }
    let width = (corners.get(2)? - corners.first()?).abs();
    let height = (corners.get(3)? - corners.get(1)?).abs();
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, reason = "page points fit f32")]
    Some((width as f32, height as f32))
}

/// `/Rotate` as one of 0, 90, 180, 270.
fn normalise_rotation(value: f64) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "checked finite, then reduced"
    )]
    let whole = (value as i64).rem_euclid(360);
    let step = whole.saturating_div(90).saturating_mul(90);
    i32::try_from(step).unwrap_or(0)
}

/// The first occurrence of `needle`.
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len().saturating_sub(needle.len()))
        .find(|&i| hay.get(i..i.saturating_add(needle.len())) == Some(needle))
}

/// The last occurrence of `needle`.
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len().saturating_sub(needle.len()))
        .rev()
        .find(|&i| hay.get(i..i.saturating_add(needle.len())) == Some(needle))
}

#[cfg(test)]
mod tests {
    // The same reasoning as the crate's other test module: a test that indexes
    // out of range should fail loudly at the line that did it. The defensive
    // lints keep panics out of code that runs on a user's file, which the
    // fixtures below are not.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    /// A PDF with real offsets, built rather than written out by hand.
    ///
    /// The offsets in a cross-reference table are byte positions, so a fixture
    /// typed as a string literal is wrong the moment anything above it changes
    /// length -- and a reader that ignored the table would still pass against
    /// it. Assembling it here means the table is right by construction and the
    /// test exercises the path a real file takes.
    fn build_pdf(sizes: &[(i32, i32)], rotate: Option<i32>) -> Vec<u8> {
        const NL: u8 = 10;
        let mut out: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        out.extend_from_slice(b"%PDF-1.4");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj");
        out.push(NL);

        offsets.push(out.len());
        let mut kids = String::new();
        for i in 0..sizes.len() {
            kids.push_str(&format!("{} 0 R ", i + 3));
        }
        let rot = rotate.map_or(String::new(), |r| format!(" /Rotate {r}"));
        out.extend_from_slice(
            format!(
                "2 0 obj << /Type /Pages /Count {}{} /Kids [{}] >> endobj",
                sizes.len(),
                rot,
                kids.trim_end()
            )
            .as_bytes(),
        );
        out.push(NL);

        for (i, (w, h)) in sizes.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(
                format!(
                    "{} 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] >> endobj",
                    i + 3
                )
                .as_bytes(),
            );
            out.push(NL);
        }

        let xref_at = out.len();
        out.extend_from_slice(b"xref");
        out.push(NL);
        out.extend_from_slice(format!("0 {}", offsets.len() + 1).as_bytes());
        out.push(NL);
        out.extend_from_slice(b"0000000000 65535 f");
        out.push(NL);
        for off in &offsets {
            out.extend_from_slice(format!("{off:010} 00000 n").as_bytes());
            out.push(NL);
        }
        out.extend_from_slice(
            format!("trailer << /Size {} /Root 1 0 R >>", offsets.len() + 1).as_bytes(),
        );
        out.push(NL);
        out.extend_from_slice(b"startxref");
        out.push(NL);
        out.extend_from_slice(format!("{xref_at}").as_bytes());
        out.push(NL);
        out.extend_from_slice(b"%%EOF");
        out
    }

    #[test]
    fn a_document_reports_its_pages_and_their_sizes() {
        let file = build_pdf(&[(612, 792), (595, 842)], None);
        let doc = read(&file).expect("reads");
        assert_eq!(doc.version, "1.4");
        assert_eq!(doc.pages.len(), 2);
        let first = doc.pages.first().expect("a first page");
        assert!((first.width - 612.0).abs() < 0.01, "US Letter width");
        assert!((first.height - 792.0).abs() < 0.01);
        let second = doc.pages.get(1).expect("a second page");
        assert!((second.width - 595.0).abs() < 0.01, "A4 width");
        assert!((second.height - 842.0).abs() < 0.01);
    }

    /// Rotation is inherited from the parent and swaps the page's sides.
    ///
    /// Both halves matter: `/Rotate` is set on the `/Pages` node here and no
    /// leaf carries one, so a reader that only looked at the leaf would report
    /// no rotation and an upright page.
    #[test]
    fn a_quarter_turn_is_inherited_and_swaps_the_sides() {
        let file = build_pdf(&[(612, 792)], Some(90));
        let doc = read(&file).expect("reads");
        let page = doc.pages.first().expect("a page");
        assert_eq!(page.rotation, 90);
        assert!((page.width - 792.0).abs() < 0.01, "sides swap when turned");
        assert!((page.height - 612.0).abs() < 0.01);
    }

    #[test]
    fn a_file_that_is_not_a_pdf_is_refused() {
        assert_eq!(read(b"just some bytes"), Err(Error::NotAPdf));
    }

    /// A cross-reference stream is named, not half-read.
    ///
    /// The danger is silence: this file has a catalog and a page, so a reader
    /// that shrugged at the table and found nothing would report a document of
    /// zero pages, which is indistinguishable from an empty one.
    #[test]
    fn a_cross_reference_stream_is_refused_by_name() {
        let mut file = build_pdf(&[(612, 792)], None);
        // Point startxref at the catalog instead of the table, which is what a
        // 1.5 file does: the offset leads to an object, not the word `xref`.
        let at = find(&file, b"startxref").expect("built with one");
        file.truncate(at);
        file.extend_from_slice(b"startxref");
        file.push(10);
        file.extend_from_slice(b"9");
        file.push(10);
        file.extend_from_slice(b"%%EOF");
        assert_eq!(read(&file), Err(Error::XrefStream));
    }

    /// A trailer chain that points at itself terminates.
    #[test]
    fn a_prev_pointing_at_its_own_section_terminates() {
        let file = build_pdf(&[(612, 792)], None);
        let at = find(&file, b"xref").expect("built with one");
        let patched = String::from_utf8_lossy(&file).replace(
            "/Size 4 /Root 1 0 R",
            &format!("/Size 4 /Root 1 0 R /Prev {at}"),
        );
        let doc = read(patched.as_bytes()).expect("still reads");
        assert_eq!(doc.pages.len(), 1, "the self-reference did not loop");
    }

    #[test]
    fn a_page_with_no_media_box_anywhere_falls_back_to_letter() {
        let file = build_pdf(&[(612, 792)], None);
        let patched = String::from_utf8_lossy(&file).replace("/MediaBox [0 0 612 792] ", "");
        // The offsets are now wrong by construction, so this asserts only that
        // a mangled file is refused rather than guessed at.
        assert!(read(patched.as_bytes()).is_err());
    }

    fn obj(src: &[u8]) -> Object {
        Lexer::new(src).object().expect("parses")
    }

    #[test]
    fn numbers_come_back_as_written() {
        assert_eq!(obj(b"42"), Object::Int(42));
        assert_eq!(obj(b"-17"), Object::Int(-17));
        assert_eq!(obj(b"3.5"), Object::Real(3.5));
        // Both of these are legal PDF and neither parses in Rust unaided.
        assert_eq!(obj(b"4."), Object::Real(4.0));
        assert_eq!(obj(b".5"), Object::Real(0.5));
    }

    #[test]
    fn a_reference_is_not_two_numbers() {
        assert_eq!(obj(b"12 0 R"), Object::Ref(12, 0));
        // The trap: the same three tokens without the R are a number followed
        // by things that are none of this object's business.
        let mut lex = Lexer::new(b"12 0 foo");
        assert_eq!(lex.object().expect("parses"), Object::Int(12));
        assert_eq!(lex.object().expect("parses"), Object::Int(0));
    }

    #[test]
    fn a_name_decodes_its_hex_escapes() {
        assert_eq!(obj(b"/Type"), Object::Name("Type".to_owned()));
        // `#20` is a space: a name may contain one, spelled this way.
        assert_eq!(obj(b"/A#20B"), Object::Name("A B".to_owned()));
    }

    #[test]
    fn strings_keep_their_bytes() {
        assert_eq!(obj(b"(hello)"), Object::Str(b"hello".to_vec()));
        // Nested parentheses do not end the string, and escapes are undone.
        assert_eq!(obj(b"(a(b)c)"), Object::Str(b"a(b)c".to_vec()));
        assert_eq!(obj(br"(tab\there)"), Object::Str(b"tab\there".to_vec()));
        // Octal, and a byte that is not text at all. A `String` here would
        // have to invent a replacement character for it.
        assert_eq!(obj(br"(\101\000)"), Object::Str(vec![b'A', 0]));
        assert_eq!(obj(b"<48656C6C6F>"), Object::Str(b"Hello".to_vec()));
        // An odd number of hex digits pads with zero rather than dropping one.
        assert_eq!(obj(b"<414>"), Object::Str(vec![b'A', 0x40]));
    }

    #[test]
    fn comments_are_not_content() {
        assert_eq!(obj(b"% a comment\n42"), Object::Int(42));
    }

    #[test]
    fn a_dictionary_nests() {
        let Object::Dict(d) = obj(b"<< /A 1 /B << /C [1 2] >> >>") else {
            panic!("not a dictionary");
        };
        assert_eq!(d.get("A"), Some(&Object::Int(1)));
        let Some(Object::Dict(inner)) = d.get("B") else {
            panic!("no nested dictionary");
        };
        assert_eq!(
            inner.get("C"),
            Some(&Object::Array(vec![Object::Int(1), Object::Int(2)]))
        );
    }

    #[test]
    fn a_stream_records_where_its_bytes_are_rather_than_copying_them() {
        let src = b"<< /Length 5 >>\nstream\nHELLO\nendstream";
        let Object::Stream { start, len, .. } = obj(src) else {
            panic!("not a stream");
        };
        assert_eq!(len, 5);
        assert_eq!(src.get(start..start + len), Some(&b"HELLO"[..]));
    }

    /// A length running past the end of the file is refused, not clamped.
    ///
    /// The number is a claim by whoever wrote the file. Clamping would hand
    /// the caller whatever bytes happened to be there.
    #[test]
    fn a_stream_longer_than_the_file_is_refused() {
        let src = b"<< /Length 9999 >>\nstream\nHELLO\nendstream";
        assert_eq!(Lexer::new(src).object(), Err(Error::Truncated));
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = b"[".repeat(MAX_TREE_DEPTH + 2);
        assert!(matches!(
            Lexer::new(&deep).object(),
            Err(Error::Malformed(_))
        ));
    }

    #[test]
    fn an_unterminated_string_ends_the_read() {
        assert_eq!(Lexer::new(b"(no end").object(), Err(Error::Truncated));
        assert_eq!(Lexer::new(b"<4142").object(), Err(Error::Truncated));
    }
}
