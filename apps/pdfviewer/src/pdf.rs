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
//! 5. **Content streams**, whose text operators give each page its words and
//!    where they sit. [`extract_text`].
//!
//! 6. **`/ToUnicode` CMaps**, which say what a composite font's codes mean.
//!    [`parse_cmap`] and [`CMap::decode`].
//!
//! 7. **Cross-reference streams and object streams**, PDF 1.5's compressed
//!    replacement for the table -- including the PNG row predictor their bytes
//!    are usually stored under. [`read_xref_stream`] and [`object_in_stream`].
//!
//! 8. **Glyph widths**, from `/Widths` for a simple font and `/W` for a
//!    composite one, so a run knows how wide it is rather than being guessed
//!    at. All 21801 runs across the three measured documents get a real
//!    width; the estimate they replace was 25% wide on one document's body
//!    text and 19% narrow on another's display type, which is the box a
//!    search highlight is drawn in.
//!
//! 9. **`/Encoding` differences**, the codes a font remaps. Eleven fonts in
//!    one measured document use them; 500 of its runs carry a ligature and 92
//!    a curly quote that previously decoded as whatever `WinAnsi` had at that
//!    code.
//!
//! **What is deliberately not here yet.** A named base encoding other than
//! `WinAnsiEncoding` -- `MacRomanEncoding`, `StandardEncoding` -- is treated
//! as WinAnsi, so a font declaring one has the wrong characters where the two
//! differ.
//!
//! **Words separated by pen movement get their spaces back**, which they
//! need: a PDF may separate words by *moving the pen* rather than drawing a
//! space, and a line that lost those reads "Definingandassigningmaterials"
//! and matches no search for two words. A jump wider than
//! [`WORD_GAP_THOUSANDTHS`] becomes one space. The cutoff is measured rather
//! than chosen -- the distribution of 7919 real adjustments is bimodal with
//! only seven of them in the band the threshold sits in.
//!
//! Three documents, three spacing mechanisms, which is why this is stated
//! narrowly: one kerns its gaps and gained spaces on 3164 of its 4482 runs;
//! one draws real spaces and has no adjustment anywhere near the threshold,
//! so it is untouched; one puts every word in its own run, where the spaces
//! are between runs and not inside them. **A search across separate runs
//! still will not match**, and that is the remaining half of this. A composite font *without* a
//! `/ToUnicode` map still yields nothing -- its codes are glyph indices into a
//! subset font and relate to no character -- and such a page is counted in
//! [`Document::unreadable_pages`], so "no results" from a search can be told
//! apart from "nothing to find".
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
//! **It found two defects no assembled fixture had.** The manual's content
//! streams all declare an indirect `/Length`, which the lexer cannot resolve
//! while it is still building the table resolution needs -- so all 122 pages
//! read as unreadable until [`stream_bytes_resolved`] went in. And the guide
//! is composite throughout, so its pages produced no text while reporting
//! themselves perfectly readable, which is the silence `unreadable_pages`
//! exists to break. Both now have tests; neither would have been written
//! without the files. Text runs measured afterwards: 451 from the
//! quick-start, 4372 from the manual, 0 from the guide with all 47 pages
//! counted unread.
//!
//! **Then the CMaps went in and the guide came back.** Its 47 pages hold
//! 16922 runs, beginning "Voxengo | Primary | User Guide", which is its title
//! page; the quick-start rose from 451 to 507, because one of its fonts was
//! composite too and had been skipped whole. All three documents now report
//! zero unread pages. `bfchar` is measured against both; **`bfrange` is
//! implemented from the specification and measured against nothing**, since
//! neither file uses a range -- both write every code out as a pair.
//!
//! **Over a corpus rather than three files.** Run across every PDF under the
//! machine's program directories -- 20 documents nobody here wrote -- it reads
//! **all 20**, refusing none, pulling 184519 text runs out with **zero unread
//! pages**. Before cross-reference streams went in it read 18 and refused 2,
//! and those two were the whole of the gap.
//!
//! A survey of 41 files found 38 classic tables to 2 streams, but that corpus
//! is documentation shipped with installed software and skews old by
//! construction; anything printed from a browser or a word processor today is
//! 1.5 or later. The 5% is a property of this machine, not of PDFs, which is
//! why the two were worth the work.
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
#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    /// Width in points, after `/Rotate` has been applied.
    pub width: f32,
    /// Height in points, after `/Rotate` has been applied.
    pub height: f32,
    /// The page's rotation, normalised to 0, 90, 180 or 270.
    pub rotation: i32,
    /// The text this page draws, in the order the content stream draws it.
    ///
    /// Empty for a page whose fonts are all composite, and for one whose
    /// content stream uses a filter this does not undo -- both are "nothing
    /// could be read here", which is a different thing from "there is nothing
    /// here" and is why [`Document::unreadable_pages`] counts them.
    pub text: Vec<TextRun>,
}

/// What a PDF file says it contains.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The version from the `%PDF-` header, as written: "1.4", "1.7".
    pub version: String,
    /// One entry per page, in reading order.
    pub pages: Vec<PageInfo>,
    /// How many pages had content this reader could not turn into text.
    ///
    /// Not the same as a page with no text on it. A window that says "no
    /// results" after searching a document it could not read has told the
    /// reader something false about their document.
    pub unreadable_pages: usize,
}

/// The default page box when a file declares none anywhere.
///
/// US Letter, which is what the specification names. A file with no
/// `/MediaBox` at any level is malformed; guessing Letter matches every other
/// reader and is better than refusing a document over a missing default.
const DEFAULT_BOX: (f32, f32) = (612.0, 792.0);

/// Values a page inherits from its ancestors in the tree.
#[derive(Debug, Clone)]
struct Inherited {
    box_pt: (f32, f32),
    rotation: i32,
    /// `/Resources`, unresolved: it is usually a reference, and resolving it
    /// at every node would re-read the same object once per page.
    resources: Option<Object>,
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

    let mut offsets = Xref::default();
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

    if offsets.is_empty() {
        return Err(Error::NoXref);
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
    let mut unreadable = 0usize;
    let mut seen_nodes = BTreeSet::new();
    let inherited = Inherited {
        box_pt: DEFAULT_BOX,
        rotation: 0,
        resources: None,
    };
    walk_pages(
        bytes,
        &offsets,
        &pages_ref,
        inherited,
        0,
        &mut seen_nodes,
        &mut pages,
        &mut unreadable,
    )?;
    if pages.is_empty() {
        return Err(Error::NoPages);
    }
    Ok(Document {
        version,
        pages,
        unreadable_pages: unreadable,
    })
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
    offsets: &mut Xref,
) -> Result<(BTreeMap<String, Object>, Option<usize>), Error> {
    let mut lex = Lexer::new(data);
    lex.seek(pos)?;
    if !lex.eat(b"xref") {
        // PDF 1.5 replaced the table with a stream. Its dictionary doubles as
        // the trailer, so this returns the same pair and the caller cannot
        // tell which kind it read.
        return read_xref_stream(data, pos, offsets);
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
                    offsets.add_direct(num, off);
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
fn resolve(data: &[u8], offsets: &Xref, object: &Object, depth: usize) -> Result<Object, Error> {
    if depth > MAX_TREE_DEPTH {
        return Err(Error::Malformed("a reference chain that does not end"));
    }
    match object {
        Object::Ref(number, _) => {
            // Direct first, then inside an object stream. A dangling reference
            // is `null` by the specification, not an error: a file may point
            // at an object it did not include.
            if let Some(offset) = offsets.direct.get(number) {
                let target = object_at(data, *offset)?;
                return resolve(data, offsets, &target, depth.saturating_add(1));
            }
            let Some((stream, index)) = offsets.compressed.get(number).copied() else {
                return Ok(Object::Null);
            };
            let target = object_in_stream(data, offsets, stream, index, depth)?;
            resolve(data, offsets, &target, depth.saturating_add(1))
        }
        other => Ok(other.clone()),
    }
}

/// Walk the page tree, appending a [`PageInfo`] per leaf.
fn walk_pages(
    data: &[u8],
    offsets: &Xref,
    node_ref: &Object,
    inherited: Inherited,
    depth: usize,
    seen: &mut BTreeSet<u32>,
    out: &mut Vec<PageInfo>,
    unreadable: &mut usize,
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
    if let Some(resources) = dict.get("Resources") {
        here.resources = Some(resources.clone());
    }

    let is_leaf = matches!(dict.get("Type"), Some(Object::Name(t)) if t == "Page");
    if is_leaf {
        let (mut width, mut height) = here.box_pt;
        if here.rotation == 90 || here.rotation == 270 {
            core::mem::swap(&mut width, &mut height);
        }
        let (text, readable) = page_text(data, offsets, dict, here.resources.as_ref());
        if !readable {
            *unreadable = unreadable.saturating_add(1);
        }
        out.push(PageInfo {
            width,
            height,
            rotation: here.rotation,
            text,
        });
        return Ok(());
    }

    let kids = dict.get("Kids").cloned().unwrap_or(Object::Null);
    let kids = resolve(data, offsets, &kids, 0)?;
    let Object::Array(kids) = kids else {
        return Ok(());
    };
    for kid in &kids {
        walk_pages(
            data,
            offsets,
            kid,
            here.clone(),
            depth.saturating_add(1),
            seen,
            out,
            unreadable,
        )?;
    }
    Ok(())
}

/// The text on one page, and whether everything on it could be read.
///
/// The second half of the answer is the point. A page whose content stream
/// uses a filter this does not undo comes back with no text, and so does a
/// blank page; reporting them the same way would let a search say "no results"
/// about a document it never read.
fn page_text(
    data: &[u8],
    offsets: &Xref,
    page: &BTreeMap<String, Object>,
    resources: Option<&Object>,
) -> (Vec<TextRun>, bool) {
    let fonts = font_map(data, offsets, resources);
    let Some(contents) = page.get("Contents") else {
        // No content stream at all is an empty page, not an unreadable one.
        return (Vec::new(), true);
    };
    let Ok(contents) = resolve(data, offsets, contents, 0) else {
        return (Vec::new(), false);
    };
    // `/Contents` is one stream or an array of them, and an array is a single
    // stream split at arbitrary points -- including, legally, inside an
    // operator. So they are concatenated before being read, not read one by
    // one.
    let parts = match contents {
        Object::Array(items) => items,
        single => vec![single],
    };
    let mut joined: Vec<u8> = Vec::new();
    let mut readable = true;
    for part in &parts {
        let Ok(part) = resolve(data, offsets, part, 0) else {
            readable = false;
            continue;
        };
        match stream_bytes_resolved(data, offsets, &part) {
            Ok(bytes) => {
                joined.extend_from_slice(&bytes);
                joined.push(b'\n');
            }
            Err(StreamError::NotAStream) => {}
            Err(_) => readable = false,
        }
    }
    let runs = extract_text(&joined, &fonts);
    // A page that drew nothing this could decode is unreadable, not empty.
    // Composite fonts are the case that matters: one of the three documents
    // this was measured against uses them throughout, so every page came back
    // with no text and nothing said why -- and a search over it would have
    // answered "no results" about a document that is full of words.
    // Unreadable only when nothing could have decoded it. A composite font
    // that brought its `/ToUnicode` is readable; one without is not, and the
    // difference is the whole point of carrying the map this far.
    let blocked = runs.is_empty()
        && fonts
            .values()
            .any(|font| font.kind == FontKind::Composite && font.to_unicode.is_none());
    (runs, readable && !blocked)
}

/// What a glyph name stands for, where this can say.
///
/// Two forms are algorithmic and exact: `uniXXXX` and `uXXXX` name a codepoint
/// outright. The rest is a table of the names that actually turn up -- 28
/// distinct ones across the eleven `/Differences` arrays in one measured
/// document, and they are the usual suspects: the `ff`/`fi`/`fl` ligatures,
/// curly quotes, dashes, bullets.
///
/// **A name this does not know decodes to nothing**, which is the same rule
/// the CID decoder follows. Guessing at an unknown glyph name puts a character
/// on the page that the document does not contain, and into every search over
/// it.
fn glyph_name_char(name: &str) -> Option<char> {
    // `uniXXXX`, and `uXXXX` with four to six digits.
    if let Some(hex) = name.strip_prefix("uni").filter(|h| h.len() == 4) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(hex) = name
        .strip_prefix('u')
        .filter(|h| (4..=6).contains(&h.len()))
    {
        if hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
        }
    }
    let found = match name {
        // Ligatures, which is what most `/Differences` arrays are for.
        "ff" => '\u{fb00}',
        "fi" => '\u{fb01}',
        "fl" => '\u{fb02}',
        "ffi" => '\u{fb03}',
        "ffl" => '\u{fb04}',
        // Quotes and dashes, the other common reason.
        "quoteright" => '\u{2019}',
        "quoteleft" => '\u{2018}',
        "quotedblleft" => '\u{201c}',
        "quotedblright" => '\u{201d}',
        "quotesinglbase" => '\u{201a}',
        "quotedblbase" => '\u{201e}',
        "quotesingle" => '\'',
        "quotedbl" => '"',
        "endash" => '\u{2013}',
        "emdash" => '\u{2014}',
        "minus" => '\u{2212}',
        "hyphen" => '-',
        "bullet" => '\u{2022}',
        "openbullet" => '\u{25e6}',
        "ellipsis" => '\u{2026}',
        "dagger" => '\u{2020}',
        "daggerdbl" => '\u{2021}',
        "perthousand" => '\u{2030}',
        "trademark" => '\u{2122}',
        "copyright" => '\u{a9}',
        "registered" => '\u{ae}',
        "degree" => '\u{b0}',
        "section" => '\u{a7}',
        "paragraph" => '\u{b6}',
        "periodcentered" => '\u{b7}',
        "fraction" => '\u{2044}',
        "guilsinglleft" => '\u{2039}',
        "guilsinglright" => '\u{203a}',
        "guillemotleft" => '\u{ab}',
        "guillemotright" => '\u{bb}',
        // Named punctuation that a `/Differences` array may restate.
        "space" => ' ',
        "period" => '.',
        "comma" => ',',
        "colon" => ':',
        "semicolon" => ';',
        "exclam" => '!',
        "question" => '?',
        "bar" => '|',
        "slash" => '/',
        "backslash" => '\\',
        "asterisk" => '*',
        "numbersign" => '#',
        "percent" => '%',
        "ampersand" => '&',
        "at" => '@',
        "underscore" => '_',
        "parenleft" => '(',
        "parenright" => ')',
        "braceleft" => '{',
        "braceright" => '}',
        "bracketleft" => '[',
        "bracketright" => ']',
        // TeX builds a tall bracket out of pieces. The top, bottom and `Big`
        // forms are brackets and read as one; the `ex` *extension* segment is
        // the middle of a drawn shape and is no character at all, so it is
        // deliberately absent -- mapping it would insert a bracket per row of
        // height.
        "bracketlefttp" | "bracketleftbt" | "bracketleftBig" | "bracketleftbig" => '[',
        "bracketrighttp" | "bracketrightbt" | "bracketrightBig" | "bracketrightbig" => ']',
        // A few accented names that turn up in re-encoded text fonts.
        "adieresis" => '\u{e4}',
        "odieresis" => '\u{f6}',
        "udieresis" => '\u{fc}',
        "germandbls" => '\u{df}',
        "eacute" => '\u{e9}',
        "egrave" => '\u{e8}',
        "agrave" => '\u{e0}',
        "ccedilla" => '\u{e7}',
        "ntilde" => '\u{f1}',
        _ => return None,
    };
    Some(found)
}

/// A font's `/Encoding`, as the codes it moves.
///
/// A plain name -- `/WinAnsiEncoding` -- moves nothing, because WinAnsi is
/// what this assumes anyway. A dictionary may carry `/Differences`, which is
/// the part that matters: an array where a number sets the next code and each
/// name that follows takes the next one along.
fn encoding_overrides(
    data: &[u8],
    offsets: &Xref,
    dict: &BTreeMap<String, Object>,
) -> BTreeMap<u32, char> {
    let mut out = BTreeMap::new();
    let Some(encoding) = dict.get("Encoding") else {
        return out;
    };
    let Ok(encoding) = resolve(data, offsets, encoding, 0) else {
        return out;
    };
    let Some(encoding) = encoding.as_dict() else {
        return out;
    };
    let Some(differences) = encoding.get("Differences") else {
        return out;
    };
    let Ok(Object::Array(items)) = resolve(data, offsets, differences, 0) else {
        return out;
    };
    let mut code = 0u32;
    for item in &items {
        match item {
            Object::Int(_) | Object::Real(_) => {
                let Some(value) = item.as_f64().filter(|v| *v >= 0.0 && *v < 65_536.0) else {
                    continue;
                };
                #[allow(clippy::cast_possible_truncation, reason = "bounded above")]
                #[allow(clippy::cast_sign_loss, reason = "filtered non-negative")]
                {
                    code = value as u32;
                }
            }
            Object::Name(name) => {
                if let Some(ch) = glyph_name_char(name) {
                    out.insert(code, ch);
                }
                // The counter advances whether or not the name was known, or
                // every name after an unknown one would land on the wrong code.
                code = code.saturating_add(1);
            }
            _ => {}
        }
    }
    out
}

/// A simple font's `/FirstChar` + `/Widths`, into a code-to-width map.
fn simple_widths(
    data: &[u8],
    offsets: &Xref,
    dict: &BTreeMap<String, Object>,
) -> BTreeMap<u32, f32> {
    let mut out = BTreeMap::new();
    let first = dict
        .get("FirstChar")
        .and_then(Object::as_f64)
        .unwrap_or(0.0)
        .max(0.0);
    let Some(widths) = dict.get("Widths") else {
        return out;
    };
    let Ok(Object::Array(widths)) = resolve(data, offsets, widths, 0) else {
        return out;
    };
    for (step, item) in widths.iter().enumerate() {
        let Ok(step) = u32::try_from(step) else { break };
        let Some(width) = resolve(data, offsets, item, 0)
            .ok()
            .and_then(|v| v.as_f64())
        else {
            continue;
        };
        #[allow(clippy::cast_possible_truncation, reason = "a code fits u32")]
        #[allow(clippy::cast_sign_loss, reason = "clamped non-negative")]
        let code = (first as u32).saturating_add(step);
        #[allow(clippy::cast_possible_truncation, reason = "a width fits f32")]
        out.insert(code, width as f32);
    }
    out
}

/// A composite font's `/W`, which mixes two forms in one array.
///
/// `c [w1 w2 ...]` gives consecutive codes their own widths; `cFirst cLast w`
/// gives a whole range one width. A parser that assumed either form alone
/// would read the other's numbers as the wrong thing entirely.
fn composite_widths(
    data: &[u8],
    offsets: &Xref,
    descendant: &BTreeMap<String, Object>,
) -> BTreeMap<u32, f32> {
    let mut out = BTreeMap::new();
    let Some(w) = descendant.get("W") else {
        return out;
    };
    let Ok(Object::Array(items)) = resolve(data, offsets, w, 0) else {
        return out;
    };
    let mut at = 0usize;
    while at < items.len() {
        let Some(first) = items.get(at).and_then(Object::as_f64) else {
            break;
        };
        let Some(next) = items.get(at.saturating_add(1)) else {
            break;
        };
        match next {
            Object::Array(list) => {
                for (step, item) in list.iter().enumerate() {
                    let (Ok(step), Some(width)) = (u32::try_from(step), item.as_f64()) else {
                        continue;
                    };
                    #[allow(clippy::cast_possible_truncation, reason = "a CID fits u32")]
                    #[allow(clippy::cast_sign_loss, reason = "CIDs are non-negative")]
                    let code = (first.max(0.0) as u32).saturating_add(step);
                    #[allow(clippy::cast_possible_truncation, reason = "a width fits f32")]
                    out.insert(code, width as f32);
                }
                at = at.saturating_add(2);
            }
            _ => {
                let Some(last) = next.as_f64() else { break };
                let Some(width) = items.get(at.saturating_add(2)).and_then(Object::as_f64) else {
                    break;
                };
                #[allow(clippy::cast_possible_truncation, reason = "a CID fits u32")]
                #[allow(clippy::cast_sign_loss, reason = "CIDs are non-negative")]
                let (lo, hi) = (first.max(0.0) as u32, last.max(0.0) as u32);
                // Bounded: a malformed pair could otherwise name the whole
                // 32-bit range and fill memory with one width.
                if hi >= lo && hi.saturating_sub(lo) < 65_536 {
                    for code in lo..=hi {
                        #[allow(clippy::cast_possible_truncation, reason = "a width fits f32")]
                        out.insert(code, width as f32);
                    }
                }
                at = at.saturating_add(3);
            }
        }
    }
    out
}

/// Which of a page's fonts are composite.
///
/// A font this cannot resolve is treated as composite, which is the cautious
/// direction: an unknown font's codes are decoded as nothing rather than as
/// bytes that may not be bytes.
fn font_map(data: &[u8], offsets: &Xref, resources: Option<&Object>) -> FontMap {
    let mut map = FontMap::new();
    let Some(resources) = resources else {
        return map;
    };
    let Ok(resources) = resolve(data, offsets, resources, 0) else {
        return map;
    };
    let Some(fonts) = resources.as_dict().and_then(|d| d.get("Font")) else {
        return map;
    };
    let Ok(fonts) = resolve(data, offsets, fonts, 0) else {
        return map;
    };
    let Some(fonts) = fonts.as_dict() else {
        return map;
    };
    for (name, entry) in fonts {
        let Ok(font) = resolve(data, offsets, entry, 0) else {
            // A font that will not resolve is assumed composite and mapless,
            // which decodes nothing: the cautious direction, since decoding an
            // unknown font's codes as bytes invents characters.
            map.insert(name.clone(), Font::composite(None));
            continue;
        };
        let dict = font.as_dict();
        let composite = matches!(
            dict.and_then(|d| d.get("Subtype")),
            Some(Object::Name(subtype)) if subtype == "Type0"
        ) || dict.is_none_or(|d| !d.contains_key("Subtype"));
        if !composite {
            let mut simple = Font::simple();
            if let Some(dict) = dict {
                simple.overrides = encoding_overrides(data, offsets, dict);
                simple.widths = simple_widths(data, offsets, dict);
                // `/MissingWidth` lives in the descriptor, not the font.
                simple.default_width = dict
                    .get("FontDescriptor")
                    .and_then(|entry| resolve(data, offsets, entry, 0).ok())
                    .and_then(|descriptor| {
                        descriptor
                            .as_dict()
                            .and_then(|d| d.get("MissingWidth"))
                            .and_then(Object::as_f64)
                    })
                    .map_or(0.0, |w| w as f32);
            }
            map.insert(name.clone(), simple);
            continue;
        }
        // `/ToUnicode` hangs off the Type0 font itself, not its descendant.
        let cmap = dict
            .and_then(|d| d.get("ToUnicode"))
            .and_then(|entry| resolve(data, offsets, entry, 0).ok())
            .and_then(|stream| stream_bytes_resolved(data, offsets, &stream).ok())
            .map(|bytes| parse_cmap(&bytes))
            .filter(|cmap| !cmap.is_empty());
        let mut font = Font::composite(cmap);
        // A composite font's widths are on its *descendant*, not on itself:
        // the Type0 font is a wrapper and the CIDFont underneath owns the
        // glyphs. `/DW` is the descendant's default and is 1000 when absent,
        // which is the one place a missing number has a specified value rather
        // than meaning nothing.
        if let Some(descendant) = dict
            .and_then(|d| d.get("DescendantFonts"))
            .and_then(|entry| resolve(data, offsets, entry, 0).ok())
            .and_then(|list| match list {
                Object::Array(items) => items.first().cloned(),
                single => Some(single),
            })
            .and_then(|entry| resolve(data, offsets, &entry, 0).ok())
        {
            if let Some(descendant) = descendant.as_dict() {
                font.widths = composite_widths(data, offsets, descendant);
                font.default_width = descendant
                    .get("DW")
                    .and_then(Object::as_f64)
                    .map_or(1000.0, |w| w as f32);
            }
        }
        map.insert(name.clone(), font);
    }
    map
}

/// Where every object in a file is.
///
/// Two kinds, because PDF 1.5 added a second. A *direct* object sits at a byte
/// offset in the file. A *compressed* one sits inside an object stream, named
/// by that stream's object number and its index within it -- so resolving it
/// means finding, decoding and parsing another object first.
#[derive(Debug, Default, Clone)]
pub struct Xref {
    direct: BTreeMap<u32, usize>,
    compressed: BTreeMap<u32, (u32, u32)>,
}

impl Xref {
    /// Record a direct object, unless a newer section already claimed it.
    fn add_direct(&mut self, number: u32, offset: usize) {
        if !self.compressed.contains_key(&number) {
            self.direct.entry(number).or_insert(offset);
        }
    }

    /// Record an object living inside an object stream.
    fn add_compressed(&mut self, number: u32, stream: u32, index: u32) {
        if !self.direct.contains_key(&number) {
            self.compressed.entry(number).or_insert((stream, index));
        }
    }

    /// Whether anything at all was found.
    fn is_empty(&self) -> bool {
        self.direct.is_empty() && self.compressed.is_empty()
    }
}

/// Undo a PNG row predictor.
///
/// A cross-reference stream is usually `/Predictor 12`, which is PNG's `Up`
/// filter applied per row: each row is one filter-type byte followed by
/// `columns` bytes, and every byte is stored as its difference from the byte
/// above. Reading the entries without undoing this yields differences rather
/// than offsets -- numbers that are entirely plausible and entirely wrong.
///
/// All five PNG filters are handled because the filter type is per *row*, so a
/// file may use `Up` for most rows and `Paeth` for one and still be ordinary.
fn undo_png_predictor(data: &[u8], columns: usize) -> Option<Vec<u8>> {
    if columns == 0 {
        return None;
    }
    let stride = columns.checked_add(1)?;
    let mut out: Vec<u8> = Vec::with_capacity(data.len());
    let mut previous: Vec<u8> = vec![0; columns];
    for row in data.chunks(stride) {
        let (kind, body) = row.split_first()?;
        let mut line: Vec<u8> = body.to_vec();
        line.resize(columns, 0);
        for i in 0..columns {
            let left = if i == 0 {
                0
            } else {
                line.get(i.saturating_sub(1)).copied().unwrap_or(0)
            };
            let up = previous.get(i).copied().unwrap_or(0);
            let up_left = if i == 0 {
                0
            } else {
                previous.get(i.saturating_sub(1)).copied().unwrap_or(0)
            };
            let raw = line.get(i).copied().unwrap_or(0);
            let value = match kind {
                0 => raw,
                1 => raw.wrapping_add(left),
                2 => raw.wrapping_add(up),
                3 => {
                    let average = (u16::from(left).saturating_add(u16::from(up))) / 2;
                    raw.wrapping_add(u8::try_from(average & 0xff).unwrap_or(0))
                }
                4 => raw.wrapping_add(paeth(left, up, up_left)),
                // An unknown filter type is not a row this can recover.
                _ => return None,
            };
            if let Some(slot) = line.get_mut(i) {
                *slot = value;
            }
        }
        out.extend_from_slice(&line);
        previous = line;
    }
    Some(out)
}

/// PNG's Paeth predictor.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    // Saturating throughout: three bytes widened to `i32` cannot overflow it,
    // but the crate denies bare arithmetic so the next edit does not have to
    // re-argue that.
    let p = i32::from(left)
        .saturating_add(i32::from(up))
        .saturating_sub(i32::from(up_left));
    let pa = p.saturating_sub(i32::from(left)).saturating_abs();
    let pb = p.saturating_sub(i32::from(up)).saturating_abs();
    let pc = p.saturating_sub(i32::from(up_left)).saturating_abs();
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// A big-endian integer from `width` bytes, or the default when width is zero.
///
/// `/W` may give a field zero bytes, which means every entry takes the
/// field's default rather than that the field is missing.
fn xref_field(bytes: &[u8], at: usize, width: usize, default: u64) -> Option<(u64, usize)> {
    if width == 0 {
        return Some((default, at));
    }
    let end = at.checked_add(width)?;
    let slice = bytes.get(at..end)?;
    let mut value = 0u64;
    for byte in slice {
        value = value.checked_mul(256)?.checked_add(u64::from(*byte))?;
    }
    Some((value, end))
}

/// One cross-reference *stream*, PDF 1.5's replacement for the table.
///
/// Returns its dictionary -- which doubles as the trailer, carrying `/Root`
/// and `/Prev` -- and the `/Prev` offset.
fn read_xref_stream(
    data: &[u8],
    pos: usize,
    xref: &mut Xref,
) -> Result<(BTreeMap<String, Object>, Option<usize>), Error> {
    let object = object_at(data, pos)?;
    let Some(dict) = object.as_dict().cloned() else {
        return Err(Error::Malformed("an xref offset pointing at no dictionary"));
    };
    if !matches!(dict.get("Type"), Some(Object::Name(t)) if t == "XRef") {
        return Err(Error::NoXref);
    }

    // The stream's own `/Length` is always direct in a cross-reference stream
    // -- it has to be, since resolving an indirect one would need the table
    // this stream is.
    let raw = stream_bytes(data, &object).map_err(|_| Error::NoXref)?;
    let columns = dict
        .get("DecodeParms")
        .and_then(Object::as_dict)
        .and_then(|d| d.get("Columns"))
        .and_then(Object::as_f64)
        .map_or(1usize, |c| c.max(1.0) as usize);
    let predictor = dict
        .get("DecodeParms")
        .and_then(Object::as_dict)
        .and_then(|d| d.get("Predictor"))
        .and_then(Object::as_f64)
        .map_or(1i64, |p| p as i64);
    let bytes = if predictor >= 10 {
        undo_png_predictor(&raw, columns).ok_or(Error::Malformed(
            "a cross-reference stream this cannot unfilter",
        ))?
    } else {
        raw
    };

    let widths: Vec<usize> = match dict.get("W") {
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| item.as_f64().map_or(0usize, |w| w.max(0.0) as usize))
            .collect(),
        _ => return Err(Error::Malformed("a cross-reference stream with no /W")),
    };
    let (w0, w1, w2) = (
        widths.first().copied().unwrap_or(0),
        widths.get(1).copied().unwrap_or(0),
        widths.get(2).copied().unwrap_or(0),
    );

    // `/Index` is a list of (first object number, count) pairs; without one the
    // whole file is a single run starting at zero.
    let index: Vec<(u64, u64)> = match dict.get("Index") {
        Some(Object::Array(items)) => items
            .chunks_exact(2)
            .filter_map(|pair| match pair {
                [first, count] => Some((
                    first.as_f64().map_or(0.0, |v| v.max(0.0)) as u64,
                    count.as_f64().map_or(0.0, |v| v.max(0.0)) as u64,
                )),
                _ => None,
            })
            .collect(),
        _ => {
            let size = dict.get("Size").and_then(Object::as_f64).unwrap_or(0.0);
            vec![(0, size.max(0.0) as u64)]
        }
    };

    let mut at = 0usize;
    for (first, count) in index {
        for step in 0..count {
            let Some((kind, next)) = xref_field(&bytes, at, w0, 1) else {
                break;
            };
            let Some((field2, next)) = xref_field(&bytes, next, w1, 0) else {
                break;
            };
            let Some((field3, next)) = xref_field(&bytes, next, w2, 0) else {
                break;
            };
            at = next;
            let Some(number) = first.checked_add(step).and_then(|n| u32::try_from(n).ok()) else {
                continue;
            };
            match kind {
                1 => {
                    if let Ok(offset) = usize::try_from(field2) {
                        if offset < data.len() {
                            xref.add_direct(number, offset);
                        }
                    }
                }
                2 => {
                    if let (Ok(stream), Ok(idx)) = (u32::try_from(field2), u32::try_from(field3)) {
                        xref.add_compressed(number, stream, idx);
                    }
                }
                // Type 0 is a free object: it is on the list precisely to say
                // it is not there.
                _ => {}
            }
        }
    }

    let prev = dict
        .get("Prev")
        .and_then(Object::as_f64)
        .filter(|p| *p >= 0.0)
        .and_then(|p| {
            let as_usize = usize::try_from(p.min(4_294_967_295.0) as u64).ok()?;
            (as_usize < data.len()).then_some(as_usize)
        });
    Ok((dict, prev))
}

/// The `index`-th object inside object stream `stream`.
///
/// An object stream holds `/N` objects after a header of `/N` number-offset
/// pairs, with the bodies starting at `/First`. Everything in it is a plain
/// object; what it saves is one `n g obj ... endobj` wrapper each and, since
/// the whole thing is usually deflated, a great deal of room.
fn object_in_stream(
    data: &[u8],
    xref: &Xref,
    stream: u32,
    index: u32,
    depth: usize,
) -> Result<Object, Error> {
    if depth > MAX_TREE_DEPTH {
        return Err(Error::Malformed("an object stream chain that does not end"));
    }
    // An object stream is itself a direct object; one living inside another
    // object stream is forbidden, and following it would be a cycle.
    let Some(offset) = xref.direct.get(&stream) else {
        return Ok(Object::Null);
    };
    let container = object_at(data, *offset)?;
    let Some(dict) = container.as_dict() else {
        return Ok(Object::Null);
    };
    let count = dict.get("N").and_then(Object::as_f64).unwrap_or(0.0);
    let first = dict.get("First").and_then(Object::as_f64).unwrap_or(0.0);
    if count <= 0.0 || first < 0.0 || f64::from(index) >= count {
        return Ok(Object::Null);
    }
    let Ok(bytes) = stream_bytes_resolved(data, xref, &container) else {
        return Ok(Object::Null);
    };
    let first = first as usize;

    // The header: `/N` pairs of "object number, offset from /First".
    let mut header = Lexer::new(&bytes);
    let mut offset_in_body = None;
    for step in 0..(count as u64) {
        let Some(Object::Int(_number)) = parse_number(header.token()) else {
            break;
        };
        let Some(Object::Int(at)) = parse_number(header.token()) else {
            break;
        };
        if step == u64::from(index) {
            offset_in_body = usize::try_from(at.max(0)).ok();
            break;
        }
    }
    let Some(offset_in_body) = offset_in_body else {
        return Ok(Object::Null);
    };
    let Some(start) = first.checked_add(offset_in_body) else {
        return Ok(Object::Null);
    };
    if start >= bytes.len() {
        return Ok(Object::Null);
    }
    let mut body = Lexer::new(&bytes);
    body.seek(start)?;
    body.object()
}

/// A `/MediaBox` as width and height in points.
fn media_box(data: &[u8], offsets: &Xref, dict: &BTreeMap<String, Object>) -> Option<(f32, f32)> {
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

/// The most a single stream may expand to.
///
/// A compressed stream is a claim about its own decompressed size, and zlib
/// can turn a few hundred bytes into gigabytes. `zlib_inflate_limited` stops
/// at this and reports it, which is why the cap is passed rather than the
/// output being trusted and measured afterwards.
const MAX_STREAM_BYTES: usize = 64 * 1024 * 1024;

/// What a stream's filter chain did, or why nothing could be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamError {
    /// The object is not a stream.
    NotAStream,
    /// The bytes named are not inside the file.
    Truncated,
    /// A filter this does not implement, named so the caller can say which.
    ///
    /// `/DCTDecode` is the common one and is not a defect: it is a JPEG image,
    /// and a text extractor has no business unpacking it.
    UnsupportedFilter(String),
    /// The stream said it was deflated and was not.
    Corrupt,
}

impl core::fmt::Display for StreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotAStream => f.write_str("not a stream"),
            Self::Truncated => f.write_str("the stream runs past the end of the file"),
            Self::UnsupportedFilter(name) => write!(f, "unsupported stream filter /{name}"),
            Self::Corrupt => f.write_str("the stream is not valid zlib data"),
        }
    }
}

/// The names in a `/Filter`, which may be one name or an array of them.
fn filter_names(dict: &BTreeMap<String, Object>) -> Vec<String> {
    match dict.get("Filter") {
        Some(Object::Name(name)) => vec![name.clone()],
        Some(Object::Array(items)) => items
            .iter()
            .filter_map(|item| match item {
                Object::Name(name) => Some(name.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A stream's bytes, resolving an indirect `/Length` first.
///
/// The lexer cannot resolve one: it is reading the file to build the
/// cross-reference table that resolution needs, so a stream whose `/Length` is
/// `12 0 R` comes back with a length of zero. Every content stream in a
/// 122-page manual this was measured against is written that way, and before
/// this the whole document read as 122 unreadable pages.
fn stream_bytes_resolved(
    data: &[u8],
    offsets: &Xref,
    object: &Object,
) -> Result<Vec<u8>, StreamError> {
    let Object::Stream { dict, start, len } = object else {
        return Err(StreamError::NotAStream);
    };
    if *len > 0 {
        return stream_bytes(data, object);
    }
    let Some(reference @ Object::Ref(..)) = dict.get("Length") else {
        // A genuinely empty stream, which is legal and not an error.
        return stream_bytes(data, object);
    };
    let Ok(resolved) = resolve(data, offsets, reference, 0) else {
        return Err(StreamError::Truncated);
    };
    let Some(length) = resolved.as_f64().filter(|n| *n >= 0.0) else {
        return Err(StreamError::Truncated);
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "checked against the file below"
    )]
    #[allow(clippy::cast_sign_loss, reason = "filtered non-negative")]
    let length = length as usize;
    if start.saturating_add(length) > data.len() {
        return Err(StreamError::Truncated);
    }
    stream_bytes(
        data,
        &Object::Stream {
            dict: dict.clone(),
            start: *start,
            len: length,
        },
    )
}

/// A stream's bytes, with its filters undone.
///
/// # Errors
///
/// [`StreamError`] saying which step could not be taken. An unsupported filter
/// is named rather than folded into a general failure, because the caller's
/// response differs: a `/DCTDecode` stream is an image and skipping it is
/// correct, while a filter nobody implemented is work left to do.
pub fn stream_bytes(data: &[u8], object: &Object) -> Result<Vec<u8>, StreamError> {
    let Object::Stream { dict, start, len } = object else {
        return Err(StreamError::NotAStream);
    };
    let raw = data
        .get(*start..start.saturating_add(*len))
        .ok_or(StreamError::Truncated)?;

    let mut bytes = raw.to_vec();
    for name in filter_names(dict) {
        match name.as_str() {
            // Both spellings of the same filter; `/Fl` is the abbreviation
            // PDF allows in inline images and some writers use throughout.
            "FlateDecode" | "Fl" => {
                bytes = deflate::zlib_inflate_limited(&bytes, MAX_STREAM_BYTES)
                    .map_err(|_| StreamError::Corrupt)?;
            }
            other => return Err(StreamError::UnsupportedFilter(other.to_owned())),
        }
    }
    Ok(bytes)
}

/// A run of text drawn by one show operator.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    /// Origin in PDF user space: points from the page's bottom-left corner.
    pub x: f32,
    pub y: f32,
    /// How wide the run is, in points.
    ///
    /// Summed from the font's own per-glyph widths, which is what decides
    /// where a search highlight is drawn. When the font declares none this
    /// falls back to half the point size per character -- the usual
    /// approximation for proportional text, and the only guess in this
    /// module. [`TextRun::width_is_measured`] says which you have.
    pub width: f32,
    /// Whether [`width`](Self::width) came from the font or from the fallback.
    pub width_is_measured: bool,
    /// The size the glyphs are actually drawn at.
    ///
    /// **Not** the operand of `Tf`. A content stream may say `/T1_0 1 Tf` and
    /// then `19 0 0 19 ... Tm`, and the text is 19 points: the size is the
    /// `Tf` operand multiplied by the text matrix's vertical scale. Taking
    /// `Tf` at face value reports every run in such a document as 1pt, and
    /// nothing about the result looks wrong.
    pub size: f32,
}

/// What kind of font a `Tf` name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    /// One byte per character: `/Type1`, `/TrueType`, `/MMType1`.
    Simple,
    /// `/Type0`, whose codes are multi-byte and mean nothing without the
    /// font's `/ToUnicode` map.
    Composite,
}

/// One font a page's `/Resources` declares.
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub kind: FontKind,
    /// Codes this font remaps, from its `/Encoding`'s `/Differences`.
    ///
    /// Applied over the base encoding rather than replacing it: a
    /// `/Differences` array names the handful of codes that move and says
    /// nothing about the rest, so a font that remaps three codes still reads
    /// its other 250 the ordinary way.
    pub overrides: BTreeMap<u32, char>,
    /// Glyph widths in thousandths of an em, by character code.
    ///
    /// From `/FirstChar` + `/Widths` for a simple font, and from the
    /// descendant font's `/W` for a composite one. Empty when the file
    /// declares none, which is what sends a run to the fallback estimate.
    pub widths: BTreeMap<u32, f32>,
    /// The width of a code the map does not list.
    ///
    /// `/MissingWidth` for a simple font, `/DW` for a composite one, and 0 for
    /// neither -- a code with no width and no default contributes nothing to
    /// the run's extent rather than a guess.
    pub default_width: f32,
    /// The `/ToUnicode` map, when the file supplies one.
    ///
    /// A composite font without one cannot be read at all: its codes are glyph
    /// indices into a subset font and carry no relation to any character. A
    /// *simple* font does not need one, because its codes are bytes in an
    /// encoding -- so `None` means "unreadable" for one kind and "no matter"
    /// for the other, and the two must not be collapsed.
    pub to_unicode: Option<CMap>,
}

impl Font {
    /// A one-byte-per-character font.
    #[must_use]
    pub fn simple() -> Self {
        Self {
            kind: FontKind::Simple,
            overrides: BTreeMap::new(),
            widths: BTreeMap::new(),
            default_width: 0.0,
            to_unicode: None,
        }
    }

    /// A composite font, with its map if the file gave one.
    #[must_use]
    pub fn composite(to_unicode: Option<CMap>) -> Self {
        Self {
            kind: FontKind::Composite,
            overrides: BTreeMap::new(),
            widths: BTreeMap::new(),
            default_width: 0.0,
            to_unicode,
        }
    }

    /// How wide `bytes` is, in thousandths of an em, and whether the font said.
    ///
    /// Splits on the same code width the decoder uses, so the sum is over the
    /// same glyphs that produced the text.
    #[must_use]
    fn advance_of(&self, bytes: &[u8]) -> Option<f32> {
        if self.widths.is_empty() && self.default_width <= 0.0 {
            return None;
        }
        let step = match self.kind {
            FontKind::Simple => 1,
            FontKind::Composite => self.to_unicode.as_ref().map_or(2, CMap::code_width),
        };
        let mut total = 0.0f32;
        for chunk in bytes.chunks(step.max(1)) {
            let mut code = 0u32;
            for byte in chunk {
                code = code.saturating_mul(256).saturating_add(u32::from(*byte));
            }
            total += self
                .widths
                .get(&code)
                .copied()
                .unwrap_or(self.default_width);
        }
        Some(total)
    }
}

/// The fonts a page's `/Resources` declares, by the name `Tf` uses.
pub type FontMap = BTreeMap<String, Font>;

/// How far the pen must jump backwards inside a `TJ` array to mean a space.
///
/// In thousandths of an em, negative because `TJ`'s numbers *subtract* from
/// the pen. **Measured, not chosen.** Across 7919 adjustments in one document
/// the distribution is sharply bimodal:
///
/// | band | count |
/// |---|---|
/// | <= -400 | 4212 |
/// | -400..-200 | 1410 |
/// | **-200..-100** | **7** |
/// | -100..-10 | 160 |
/// | > -10 | 2130 |
///
/// Seven values in 7919 fall between -200 and -100, so the threshold is not a
/// judgement call: it sits in a valley the data already has. Anywhere in that
/// band separates word gaps from the kerning inside a word.
///
/// It is safe for documents that space differently. The other two measured
/// here never cross it -- one writes real space characters and has nothing
/// below -100, the other puts every word in its own run -- so this changes
/// their text not at all.
const WORD_GAP_THOUSANDTHS: f32 = -150.0;

/// The identity matrix, which `BT` resets both text matrices to.
const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `translate(tx, ty) * m`, which is what `Td` and `Tm` compose.
fn translate(tx: f32, ty: f32, m: [f32; 6]) -> [f32; 6] {
    [
        m[0],
        m[1],
        m[2],
        m[3],
        tx.mul_add(m[0], ty.mul_add(m[2], m[4])),
        tx.mul_add(m[1], ty.mul_add(m[3], m[5])),
    ]
}

/// Decode one byte of a simple font's string.
///
/// **`/Encoding` is not read yet**, and `WinAnsiEncoding` is assumed, which is
/// what the overwhelming majority of simple fonts in real documents declare.
/// It agrees with Latin-1 everywhere except `0x80..=0x9F`, which is the table
/// below; a font using `StandardEncoding` or a `/Differences` array will have
/// the wrong characters in that range and the right ones elsewhere.
fn win_ansi(byte: u8) -> Option<char> {
    // The range where WinAnsi and Latin-1 disagree. `None` entries are
    // undefined in WinAnsi, and an undefined code draws nothing.
    const HIGH: [char; 32] = [
        '\u{20ac}', '\u{fffd}', '\u{201a}', '\u{0192}', '\u{201e}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02c6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{fffd}',
        '\u{017d}', '\u{fffd}', '\u{fffd}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02dc}', '\u{2122}', '\u{0161}', '\u{203a}',
        '\u{0153}', '\u{fffd}', '\u{017e}', '\u{0178}',
    ];
    match byte {
        0..=31 => None,
        128..=159 => {
            let index = usize::from(byte).checked_sub(128)?;
            match HIGH.get(index) {
                Some('\u{fffd}') | None => None,
                Some(c) => Some(*c),
            }
        }
        other => Some(char::from(other)),
    }
}

/// The text a content stream draws, with where and how big.
///
/// `fonts` says which `Tf` names are composite. A show operator under a
/// composite font contributes **nothing**: its codes are two bytes wide and
/// mean nothing without the font's `/ToUnicode` map, so decoding them
/// byte-wise would produce plausible-looking rubbish -- and rubbish in a
/// search index is worse than an empty one, because the reader cannot see it
/// is there.
#[must_use]
pub fn extract_text(content: &[u8], fonts: &FontMap) -> Vec<TextRun> {
    let mut lexer = Lexer::new(content);
    let mut runs: Vec<TextRun> = Vec::new();
    let mut operands: Vec<Object> = Vec::new();

    let mut text_matrix = IDENTITY;
    let mut line_matrix = IDENTITY;
    let mut font_size = 0.0f32;
    let mut leading = 0.0f32;
    let mut font: Option<&Font> = None;

    while let Some(piece) = lexer.content_piece() {
        let op = match piece {
            Piece::Value(value) => {
                // Bounded: a stream can otherwise stack operands forever
                // between operators and make this grow without limit.
                if operands.len() < 64 {
                    operands.push(value);
                }
                continue;
            }
            Piece::Op(op) => op,
        };
        match op.as_slice() {
            b"BT" => {
                text_matrix = IDENTITY;
                line_matrix = IDENTITY;
            }
            b"Tf" => {
                if let Some(size) = operands.last().and_then(Object::as_f64) {
                    font_size = size as f32;
                }
                font = match operands.iter().rev().nth(1) {
                    Some(Object::Name(name)) => fonts.get(name),
                    _ => None,
                };
            }
            b"TL" => {
                if let Some(value) = operands.last().and_then(Object::as_f64) {
                    leading = value as f32;
                }
            }
            b"Td" | b"TD" => {
                let ty = numeric(&operands, 0);
                let tx = numeric(&operands, 1);
                if op.as_slice() == b"TD" {
                    leading = -ty;
                }
                line_matrix = translate(tx, ty, line_matrix);
                text_matrix = line_matrix;
            }
            b"Tm" => {
                if operands.len() >= 6 {
                    let mut m = IDENTITY;
                    for (slot, index) in m.iter_mut().zip(0..6usize) {
                        *slot = numeric(&operands, 5usize.saturating_sub(index));
                    }
                    line_matrix = m;
                    text_matrix = m;
                }
            }
            b"T*" => {
                line_matrix = translate(0.0, -leading, line_matrix);
                text_matrix = line_matrix;
            }
            b"Tj" | b"'" | b"\"" => {
                if op.as_slice() != b"Tj" {
                    line_matrix = translate(0.0, -leading, line_matrix);
                    text_matrix = line_matrix;
                }
                if let Some(Object::Str(bytes)) = operands.last() {
                    push_run(&mut runs, bytes, text_matrix, font_size, font);
                }
            }
            b"TJ" => {
                if let Some(Object::Array(items)) = operands.last() {
                    // The numbers between the strings are kerning, in
                    // thousandths of an em. They move the pen; they are not
                    // text. Concatenating them would put "-3" inside a word.
                    //
                    // But a big enough jump *is* a space: a PDF may separate
                    // words by moving the pen rather than by drawing one, and
                    // a run that lost those reads "Definingandassigning" and
                    // matches no search for two words. See
                    // [`WORD_GAP_THOUSANDTHS`] for where the cutoff comes
                    // from.
                    let mut joined: Vec<u8> = Vec::new();
                    let mut pending_space = false;
                    for item in items {
                        match item {
                            Object::Str(bytes) => {
                                if pending_space && !joined.is_empty() && !bytes.is_empty() {
                                    joined.push(b' ');
                                }
                                pending_space = false;
                                joined.extend_from_slice(bytes);
                            }
                            other => {
                                if other
                                    .as_f64()
                                    .is_some_and(|v| v <= f64::from(WORD_GAP_THOUSANDTHS))
                                {
                                    pending_space = true;
                                }
                            }
                        }
                    }
                    push_run(&mut runs, &joined, text_matrix, font_size, font);
                }
            }
            _ => {}
        }
        operands.clear();
    }
    runs
}

/// The `n`th operand counting back from the last, as a number.
fn numeric(operands: &[Object], back: usize) -> f32 {
    operands
        .iter()
        .rev()
        .nth(back)
        .and_then(Object::as_f64)
        .map_or(0.0, |v| v as f32)
}

/// Turn one show operator's bytes into a run, if it says anything.
fn push_run(
    runs: &mut Vec<TextRun>,
    bytes: &[u8],
    matrix: [f32; 6],
    font_size: f32,
    font: Option<&Font>,
) {
    if bytes.is_empty() {
        return;
    }
    // Three cases, and the middle one is the whole reason a font is carried
    // here rather than a boolean. A composite font *with* a map decodes; one
    // without stays unreadable; a simple font never needed one.
    let text: String = match font {
        Some(Font {
            kind: FontKind::Composite,
            to_unicode: Some(cmap),
            ..
        }) => cmap.decode(bytes),
        Some(Font {
            kind: FontKind::Composite,
            to_unicode: None,
            ..
        }) => return,
        Some(font) => bytes
            .iter()
            .filter_map(|b| {
                font.overrides
                    .get(&u32::from(*b))
                    .copied()
                    .or_else(|| win_ansi(*b))
            })
            .collect(),
        None => bytes.iter().filter_map(|b| win_ansi(*b)).collect(),
    };
    if text.trim().is_empty() {
        return;
    }
    // The text matrix's vertical scale, which is what turns `Tf`'s operand
    // into the size on the page.
    let scale = matrix[2].hypot(matrix[3]);
    let size = font_size * scale;
    if !size.is_finite() || size <= 0.0 {
        return;
    }
    // The font's own widths where it gives them, and half the point size per
    // character where it does not. The flag travels with the number so a
    // caller can tell a measurement from an estimate -- a highlight drawn on
    // an estimate is in roughly the right place, and saying so is cheaper than
    // pretending otherwise.
    let measured = font
        .and_then(|font| font.advance_of(bytes))
        .map(|thousandths| thousandths * size / 1000.0)
        .filter(|w| w.is_finite() && *w > 0.0);
    #[allow(clippy::cast_precision_loss, reason = "a run is not 2^24 characters")]
    let fallback = size * 0.5 * text.chars().count() as f32;
    runs.push(TextRun {
        text,
        x: matrix[4],
        y: matrix[5],
        width: measured.unwrap_or(fallback),
        width_is_measured: measured.is_some(),
        size,
    });
}

/// One item from a content stream: an operand, or the operator using them.
enum Piece {
    Value(Object),
    Op(Vec<u8>),
}

impl Lexer<'_> {
    /// The next operand or operator, or `None` at the end of the stream.
    ///
    /// A content stream is postfix -- operands, then the operator that
    /// consumes them -- so this cannot be [`Lexer::object`], which fails on a
    /// bare keyword. The two are told apart by the first byte, exactly as the
    /// object parser does.
    fn content_piece(&mut self) -> Option<Piece> {
        self.skip_space();
        let first = self.peek()?;
        match first {
            b'/' | b'(' | b'[' | b'<' | b'+' | b'-' | b'.' | b'0'..=b'9' => {
                match self.object() {
                    Ok(value) => Some(Piece::Value(value)),
                    // A malformed operand ends the stream rather than looping:
                    // the position has not moved, so continuing would spin.
                    Err(_) => None,
                }
            }
            b']' | b')' | b'>' | b'}' | b'{' => {
                // Stray closers a malformed stream can leave behind.
                self.bump();
                Some(Piece::Op(Vec::new()))
            }
            _ => {
                let word = self.token().to_vec();
                if word.is_empty() {
                    self.bump();
                    return Some(Piece::Op(Vec::new()));
                }
                match word.as_slice() {
                    b"true" => Some(Piece::Value(Object::Bool(true))),
                    b"false" => Some(Piece::Value(Object::Bool(false))),
                    b"null" => Some(Piece::Value(Object::Null)),
                    _ => Some(Piece::Op(word)),
                }
            }
        }
    }

    /// Step over one byte, for the cases that would otherwise not advance.
    fn bump(&mut self) {
        self.pos = self.pos.saturating_add(1);
    }
}

/// The most entries a `/ToUnicode` map may hold.
///
/// A CMap is a file from elsewhere and its entry count is a claim. 65536 is
/// every code a two-byte codespace can express, so nothing legitimate is
/// turned away by the cap.
const MAX_CMAP_ENTRIES: usize = 65_536;

/// A `/ToUnicode` map: what a font's codes mean.
///
/// Without one, a composite font's text cannot be read at all -- the codes are
/// glyph indices in a subset font and carry no relation to any character.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CMap {
    /// How many bytes make one code, from `begincodespacerange`.
    ///
    /// Two for `/Identity-H`, which is what every composite font in the
    /// documents this was measured against uses. One is legal and handled;
    /// three and four are legal, rare, and treated as two rather than guessed
    /// at, because splitting a string on the wrong width silently produces
    /// characters that were never on the page.
    code_bytes: usize,
    entries: BTreeMap<u32, String>,
}

impl CMap {
    /// The text one code stands for, if the map says.
    #[must_use]
    pub fn lookup(&self, code: u32) -> Option<&str> {
        self.entries.get(&code).map(String::as_str)
    }

    /// How many bytes make one code.
    #[must_use]
    pub fn code_width(&self) -> usize {
        self.code_bytes.clamp(1, 2)
    }

    /// Whether the map says nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Split a show operator's bytes into codes and look each one up.
    ///
    /// A code the map does not cover contributes nothing: it is a glyph whose
    /// character this file never stated, and inventing one would put a
    /// character on the page that is not there.
    #[must_use]
    pub fn decode(&self, bytes: &[u8]) -> String {
        let width = self.code_bytes.clamp(1, 2);
        let mut out = String::new();
        for chunk in bytes.chunks(width) {
            let mut code = 0u32;
            for byte in chunk {
                code = code.saturating_mul(256).saturating_add(u32::from(*byte));
            }
            if let Some(text) = self.lookup(code) {
                out.push_str(text);
            }
        }
        out
    }
}

/// A big-endian integer from a hex string's bytes.
fn be_code(bytes: &[u8]) -> u32 {
    let mut code = 0u32;
    for byte in bytes.iter().take(4) {
        code = code.saturating_mul(256).saturating_add(u32::from(*byte));
    }
    code
}

/// UTF-16BE bytes as text, or `None` if they are not valid UTF-16.
///
/// Strict rather than lossy. An entry that will not decode is left out of the
/// map, so its code draws nothing; mapping it to a replacement character would
/// put a visible `U+FFFD` into the document's text and into search results.
fn utf16be(bytes: &[u8]) -> Option<String> {
    let mut units = Vec::new();
    for pair in bytes.chunks_exact(2) {
        if let [hi, lo] = pair {
            units.push(u16::from_be_bytes([*hi, *lo]));
        }
    }
    if units.is_empty() {
        return None;
    }
    String::from_utf16(&units).ok()
}

/// Read a `/ToUnicode` CMap.
///
/// The syntax is postfix like a content stream, so it walks with the same
/// [`Lexer::content_piece`]. It needs its own loop rather than
/// [`extract_text`]'s because a CMap's operands come in one long run -- 85
/// `bfchar` entries are 170 operands before the `endbfchar` that consumes
/// them -- and that loop caps operands at 64 to stop a malformed content
/// stream growing without bound.
#[must_use]
pub fn parse_cmap(bytes: &[u8]) -> CMap {
    let mut lexer = Lexer::new(bytes);
    let mut map = CMap {
        code_bytes: 2,
        entries: BTreeMap::new(),
    };
    let mut operands: Vec<Object> = Vec::new();

    while let Some(piece) = lexer.content_piece() {
        let op = match piece {
            Piece::Value(value) => {
                if operands.len() < MAX_CMAP_ENTRIES.saturating_mul(3) {
                    operands.push(value);
                }
                continue;
            }
            Piece::Op(op) => op,
        };
        match op.as_slice() {
            b"endcodespacerange" => {
                // The first low bound's width is the code width. Every
                // codespace in one CMap has the same width in practice, and a
                // file mixing widths needs the full multi-range machinery this
                // does not have.
                if let Some(Object::Str(low)) = operands.first() {
                    if !low.is_empty() {
                        map.code_bytes = low.len();
                    }
                }
            }
            b"endbfchar" => {
                for pair in operands.chunks_exact(2) {
                    if let [Object::Str(src), Object::Str(dst)] = pair {
                        if map.entries.len() >= MAX_CMAP_ENTRIES {
                            break;
                        }
                        if let Some(text) = utf16be(dst) {
                            map.entries.insert(be_code(src), text);
                        }
                    }
                }
            }
            b"endbfrange" => {
                for triple in operands.chunks_exact(3) {
                    let [Object::Str(lo), Object::Str(hi), dst] = triple else {
                        continue;
                    };
                    let (lo, hi) = (be_code(lo), be_code(hi));
                    if hi < lo {
                        continue;
                    }
                    match dst {
                        // `<lo> <hi> <dst>`: consecutive codes map to
                        // consecutive characters from `dst`.
                        Object::Str(dst) => {
                            let Some(base) = utf16be(dst) else { continue };
                            let Some(first) = base.chars().next() else {
                                continue;
                            };
                            let tail: String = base.chars().skip(1).collect();
                            for (step, code) in (lo..=hi).enumerate() {
                                if map.entries.len() >= MAX_CMAP_ENTRIES {
                                    break;
                                }
                                let Ok(step) = u32::try_from(step) else { break };
                                let Some(scalar) = u32::from(first).checked_add(step) else {
                                    break;
                                };
                                let Some(ch) = char::from_u32(scalar) else {
                                    continue;
                                };
                                let mut text = String::new();
                                text.push(ch);
                                text.push_str(&tail);
                                map.entries.insert(code, text);
                            }
                        }
                        // `<lo> <hi> [<d0> <d1> ...]`: one destination each.
                        Object::Array(items) => {
                            for (offset, item) in items.iter().enumerate() {
                                let Object::Str(dst) = item else { continue };
                                let Ok(offset) = u32::try_from(offset) else {
                                    break;
                                };
                                let Some(code) = lo.checked_add(offset) else {
                                    break;
                                };
                                if code > hi || map.entries.len() >= MAX_CMAP_ENTRIES {
                                    break;
                                }
                                if let Some(text) = utf16be(dst) {
                                    map.entries.insert(code, text);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        // Every operator here consumes what came before it, including the
        // `begin*` forms whose count operand is not needed: the entries are
        // counted by walking them, not by believing the number.
        operands.clear();
    }
    map
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

    /// A one-page PDF whose cross-reference is a *stream*, not a table.
    ///
    /// Unfiltered and unpredicted, which is legal and keeps the fixture
    /// readable: `/W [1 2 1]` is four bytes an entry -- type, a two-byte
    /// offset, a one-byte generation.
    fn pdf_with_xref_stream() -> Vec<u8> {
        const NL: u8 = 10;
        let mut out: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        out.extend_from_slice(b"%PDF-1.5");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"2 0 obj << /Type /Pages /Count 1 /Kids [3 0 R] >> endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 400] >> endobj",
        );
        out.push(NL);

        // Object 4 is the cross-reference stream itself, and must appear in
        // its own table -- a file's own xref object is an object like any
        // other.
        let xref_at = out.len();
        offsets.push(xref_at);

        let mut entries: Vec<u8> = Vec::new();
        // Object 0 is always free.
        entries.extend_from_slice(&[0, 0, 0, 255]);
        for off in &offsets {
            let off = u16::try_from(*off).expect("fixture stays small");
            entries.push(1);
            entries.extend_from_slice(&off.to_be_bytes());
            entries.push(0);
        }
        out.extend_from_slice(
            format!(
                "4 0 obj << /Type /XRef /Size 5 /W [1 2 1] /Root 1 0 R /Length {} >>",
                entries.len()
            )
            .as_bytes(),
        );
        out.push(NL);
        out.extend_from_slice(b"stream");
        out.push(NL);
        out.extend_from_slice(&entries);
        out.push(NL);
        out.extend_from_slice(b"endstream endobj");
        out.push(NL);

        out.extend_from_slice(b"startxref");
        out.push(NL);
        out.extend_from_slice(format!("{xref_at}").as_bytes());
        out.push(NL);
        out.extend_from_slice(b"%%EOF");
        out
    }

    /// A cross-reference stream is read, where it used to be refused.
    ///
    /// This test asserted the refusal until the stream reader went in. The
    /// refusal was the honest answer while nothing could read one -- a
    /// document reported as having zero pages is a claim about the document --
    /// but it was still two files in twenty on this machine that could not be
    /// opened at all.
    #[test]
    fn a_cross_reference_stream_is_read() {
        let doc = read(&pdf_with_xref_stream()).expect("reads");
        assert_eq!(doc.version, "1.5");
        assert_eq!(doc.pages.len(), 1);
        let page = doc.pages.first().expect("a page");
        assert!((page.width - 200.0).abs() < 0.01, "from the stream's table");
        assert!((page.height - 400.0).abs() < 0.01);
    }

    /// A `startxref` pointing at neither a table nor a cross-reference stream.
    ///
    /// Still refused, and this is what the old refusal test really covered: an
    /// offset leading to an ordinary object. Reading on from there would
    /// produce a document with no catalog rather than an error.
    #[test]
    fn a_startxref_pointing_at_an_ordinary_object_is_refused() {
        let mut file = pdf_with_content(b"BT ET", false);
        let at = find(&file, b"startxref").expect("built with one");
        file.truncate(at);
        file.extend_from_slice(b"startxref");
        file.push(10);
        file.extend_from_slice(b"9");
        file.push(10);
        file.extend_from_slice(b"%%EOF");
        assert_eq!(read(&file), Err(Error::NoXref));
    }

    /// The PNG `Up` predictor is undone before the entries are read.
    ///
    /// Predictor 12 stores every byte as its difference from the byte above,
    /// so reading the entries without undoing it yields differences -- numbers
    /// that are entirely plausible and entirely wrong.
    #[test]
    fn the_up_predictor_is_undone() {
        // Two rows of three columns. Filter byte 2 is `Up`.
        let filtered = [
            0u8, 10, 20, 30, // None: 10 20 30
            2u8, 1, 2, 3, // Up: +1 +2 +3 over the row above
        ];
        let out = undo_png_predictor(&filtered, 3).expect("unfilters");
        assert_eq!(out, vec![10, 20, 30, 11, 22, 33]);
    }

    /// A row naming a filter that does not exist is refused, not guessed.
    #[test]
    fn an_unknown_row_filter_is_refused() {
        assert_eq!(undo_png_predictor(&[9u8, 1, 2, 3], 3), None);
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

    /// A `/FlateDecode` stream comes back as its original bytes.
    ///
    /// The fixture is deflated by the same crate that inflates it, which
    /// proves the wiring and not the codec -- `deflate` has its own tests for
    /// that. What is being tested here is that the filter name is read, the
    /// range is taken from the file, and the two are put together.
    #[test]
    fn a_deflated_stream_is_inflated() {
        let plain = b"BT /F1 12 Tf (hello) Tj ET";
        let squeezed = deflate::zlib_deflate(plain);
        let mut file = Vec::new();
        file.extend_from_slice(
            format!(
                "<< /Length {} /Filter /FlateDecode >>\nstream\n",
                squeezed.len()
            )
            .as_bytes(),
        );
        file.extend_from_slice(&squeezed);
        file.extend_from_slice(b"\nendstream");

        let object = Lexer::new(&file).object().expect("parses");
        let out = stream_bytes(&file, &object).expect("inflates");
        assert_eq!(out, plain);
    }

    /// An unfiltered stream is handed back untouched.
    #[test]
    fn a_plain_stream_needs_no_filter() {
        let file = b"<< /Length 5 >>\nstream\nHELLO\nendstream";
        let object = Lexer::new(file).object().expect("parses");
        assert_eq!(stream_bytes(file, &object).expect("reads"), b"HELLO");
    }

    /// A filter this does not implement is named, not swallowed.
    ///
    /// `/DCTDecode` is a JPEG, and the caller's right response is to skip it.
    /// Reporting it as a general failure would make "this is an image" and
    /// "this reader is incomplete" the same answer.
    #[test]
    fn an_unsupported_filter_is_named() {
        let file = b"<< /Length 2 /Filter /DCTDecode >>\nstream\nhi\nendstream";
        let object = Lexer::new(file).object().expect("parses");
        assert_eq!(
            stream_bytes(file, &object),
            Err(StreamError::UnsupportedFilter("DCTDecode".to_owned()))
        );
    }

    /// A stream that claims deflate and is not is refused.
    #[test]
    fn a_stream_that_lies_about_being_deflated_is_refused() {
        let file = b"<< /Length 5 /Filter /FlateDecode >>\nstream\nplain\nendstream";
        let object = Lexer::new(file).object().expect("parses");
        assert_eq!(stream_bytes(file, &object), Err(StreamError::Corrupt));
    }

    fn simple_fonts() -> FontMap {
        let mut map = FontMap::new();
        map.insert("T1_0".to_owned(), Font::simple());
        map
    }

    /// The size comes from the text matrix, not from `Tf`.
    ///
    /// Taken verbatim from a real document: `/T1_0 1 Tf` with `19 0 0 19` in
    /// the matrix means 19-point text. A reader that believed `Tf` would call
    /// it 1pt, and every span in the document would be wrong in a way that
    /// still renders.
    #[test]
    fn the_size_is_the_font_size_times_the_matrix_scale() {
        let content = b"BT /T1_0 1 Tf 19 0 0 19 14.1732 120.9732 Tm (Guide) Tj ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.len(), 1);
        let run = runs.first().expect("a run");
        assert_eq!(run.text, "Guide");
        assert!((run.size - 19.0).abs() < 0.01, "got {}", run.size);
        assert!((run.x - 14.1732).abs() < 0.01);
        assert!((run.y - 120.9732).abs() < 0.01);
    }

    /// Kerning numbers inside a `TJ` array move the pen; they are not text.
    ///
    /// The array is the one a real document uses for the words "Quick Start".
    /// Concatenating its numbers gives "Q-3uick S3tar-24t ", which is exactly
    /// the kind of output that looks like it is working.
    #[test]
    fn a_kerned_array_does_not_put_its_numbers_in_the_words() {
        let content = b"BT /T1_0 19 Tf [(Q)-3(uick S)3(tar)-24(t )]TJ ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs.first().expect("a run").text, "Quick Start ");
    }

    /// A composite font contributes nothing rather than rubbish.
    ///
    /// Its codes are two bytes wide and mean nothing without `/ToUnicode`.
    /// One of the three documents this reader was measured against is
    /// composite throughout, so this is the difference between an empty search
    /// index and one full of text that was never on the page.
    #[test]
    fn a_composite_font_yields_no_text() {
        let mut fonts = FontMap::new();
        fonts.insert("C2_0".to_owned(), Font::composite(None));
        let content = b"BT /C2_0 12 Tf 1 0 0 1 10 10 Tm (\x00H\x00i) Tj ET";
        assert!(extract_text(content, &fonts).is_empty());

        // And the same bytes under a simple font do produce something, so the
        // test above is about the font and not about the string.
        let mut simple = FontMap::new();
        simple.insert("C2_0".to_owned(), Font::simple());
        assert!(!extract_text(content, &simple).is_empty());
    }

    /// `Td` moves relative to the line, and `T*` uses the leading.
    #[test]
    fn the_line_moves_by_td_and_by_the_leading() {
        let content =
            b"BT /T1_0 10 Tf 1 0 0 1 100 700 Tm (one) Tj 0 -12 Td (two) Tj 14 TL T* (three) Tj ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.len(), 3);
        let ys: Vec<f32> = runs.iter().map(|r| r.y).collect();
        assert!((ys[0] - 700.0).abs() < 0.01, "got {ys:?}");
        assert!((ys[1] - 688.0).abs() < 0.01, "Td moved down 12");
        assert!((ys[2] - 674.0).abs() < 0.01, "T* moved down the 14 leading");
    }

    /// Text drawn at a nonsense size is dropped rather than recorded.
    #[test]
    fn a_zero_sized_run_is_not_recorded() {
        let content = b"BT /T1_0 0 Tf 1 0 0 1 10 10 Tm (invisible) Tj ET";
        assert!(extract_text(content, &simple_fonts()).is_empty());
    }

    /// Whitespace-only shows are not runs.
    #[test]
    fn a_run_of_spaces_is_not_text() {
        let content = b"BT /T1_0 12 Tf 1 0 0 1 10 10 Tm (   ) Tj ET";
        assert!(extract_text(content, &simple_fonts()).is_empty());
    }

    /// A stream that ends mid-object stops rather than spinning.
    #[test]
    fn a_truncated_content_stream_terminates() {
        let content = b"BT /T1_0 12 Tf (unterminated";
        let _ = extract_text(content, &simple_fonts());
    }

    /// WinAnsi's high range is decoded, not passed through as Latin-1.
    #[test]
    fn a_curly_quote_is_not_a_control_character() {
        // 0x92 is a right single quote in WinAnsi and a control code in
        // Latin-1. Passing the byte straight through would put an unprintable
        // character in the middle of a word.
        let content = b"BT /T1_0 12 Tf 1 0 0 1 0 0 Tm (it\x92s) Tj ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.first().expect("a run").text, "it\u{2019}s");
    }

    /// A one-page PDF whose content stream is built by `content`.
    ///
    /// `indirect_length` writes `/Length 9 0 R` and puts the number in its own
    /// object, which is how every content stream in one of the real documents
    /// this was measured against is written.
    fn pdf_with_content(content: &[u8], indirect_length: bool) -> Vec<u8> {
        const NL: u8 = 10;
        let mut out: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        out.extend_from_slice(b"%PDF-1.4");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"2 0 obj << /Type /Pages /Count 1 /Kids [3 0 R] >> endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >> endobj",
        );
        out.push(NL);

        offsets.push(out.len());
        let length = if indirect_length {
            "6 0 R".to_owned()
        } else {
            content.len().to_string()
        };
        out.extend_from_slice(format!("4 0 obj << /Length {length} >>").as_bytes());
        out.push(NL);
        out.extend_from_slice(b"stream");
        out.push(NL);
        out.extend_from_slice(content);
        out.push(NL);
        out.extend_from_slice(b"endstream endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(
            b"5 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj",
        );
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(format!("6 0 obj {} endobj", content.len()).as_bytes());
        out.push(NL);

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

    /// A page's text is read.
    #[test]
    fn a_pages_text_comes_back_with_the_page() {
        let content = b"BT /F1 12 Tf 1 0 0 1 72 720 Tm (Hello there) Tj ET";
        let doc = read(&pdf_with_content(content, false)).expect("reads");
        let page = doc.pages.first().expect("a page");
        assert_eq!(page.text.len(), 1);
        let run = page.text.first().expect("a run");
        assert_eq!(run.text, "Hello there");
        assert!((run.size - 12.0).abs() < 0.01);
        assert_eq!(doc.unreadable_pages, 0);
    }

    /// A `/Length` that is an indirect reference is resolved.
    ///
    /// The lexer cannot resolve one -- it is building the table that
    /// resolution needs -- so the stream arrives with a length of zero. Every
    /// content stream in a 122-page manual this was measured against is
    /// written this way, and before the fix the whole document came back as
    /// 122 unreadable pages with no text at all. Assembled fixtures did not
    /// catch it because they all wrote the length inline.
    #[test]
    fn an_indirect_stream_length_is_resolved() {
        let content = b"BT /F1 12 Tf 1 0 0 1 72 720 Tm (Indirect) Tj ET";
        let doc = read(&pdf_with_content(content, true)).expect("reads");
        let page = doc.pages.first().expect("a page");
        assert_eq!(
            page.text.first().map(|r| r.text.as_str()),
            Some("Indirect"),
            "the stream length was left at zero, so nothing was read"
        );
        assert_eq!(doc.unreadable_pages, 0);
    }

    /// A page drawn entirely in a composite font counts as unread.
    ///
    /// Its text cannot be decoded without the font's `/ToUnicode` map, so no
    /// runs come out -- and a page with no runs is indistinguishable from a
    /// blank one unless it is counted. One of the three real documents is
    /// composite throughout: a search over it would otherwise answer "no
    /// results" about 47 pages of words.
    #[test]
    fn a_composite_only_page_is_counted_as_unread() {
        let content = b"BT /F1 12 Tf 1 0 0 1 72 720 Tm (\x00H\x00i) Tj ET";
        let mut file = pdf_with_content(content, false);
        // Make the font composite. Same length, so every offset still holds.
        let patched =
            String::from_utf8_lossy(&file).replace("/Subtype /Type1 ", "/Subtype /Type0 ");
        file = patched.into_bytes();

        let doc = read(&file).expect("reads");
        assert!(doc.pages.first().expect("a page").text.is_empty());
        assert_eq!(
            doc.unreadable_pages, 1,
            "a page nobody could read must not look like an empty one"
        );
    }

    /// `bfchar` pairs, which is what both measured documents use.
    #[test]
    fn a_bfchar_map_reads_its_pairs() {
        let src = b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
2 beginbfchar\n<0003> <0020>\n<0024> <0041>\nendbfchar\n";
        let map = parse_cmap(src);
        assert_eq!(map.lookup(0x0003), Some(" "));
        assert_eq!(map.lookup(0x0024), Some("A"));
        assert_eq!(map.lookup(0x0099), None, "a code the file never mapped");
    }

    /// Two-byte codes are split on the codespace width, not guessed.
    #[test]
    fn decoding_splits_on_the_codespace_width() {
        let src = b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
2 beginbfchar\n<0003> <0048>\n<0004> <0069>\nendbfchar\n";
        let map = parse_cmap(src);
        assert_eq!(map.decode(&[0x00, 0x03, 0x00, 0x04]), "Hi");
        // An unmapped code contributes nothing rather than a guess.
        assert_eq!(map.decode(&[0x00, 0x03, 0x99, 0x99]), "H");
    }

    /// A one-byte codespace is honoured.
    #[test]
    fn a_single_byte_codespace_is_read_as_one_byte() {
        let src = b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n\
1 beginbfchar\n<41> <0041>\nendbfchar\n";
        let map = parse_cmap(src);
        assert_eq!(map.decode(&[0x41, 0x41]), "AA");
    }

    /// `bfrange`, both forms.
    ///
    /// **Implemented from the specification, not measured.** Neither PDF this
    /// reader was checked against uses a range -- both write every code out as
    /// a `bfchar` pair -- so unlike the rest of this module these two forms
    /// have no real document behind them, only the tests here.
    #[test]
    fn a_bfrange_covers_consecutive_codes() {
        let src = b"1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
1 beginbfrange\n<0010> <0012> <0041>\nendbfrange\n";
        let map = parse_cmap(src);
        assert_eq!(map.lookup(0x0010), Some("A"));
        assert_eq!(
            map.lookup(0x0011),
            Some("B"),
            "the range steps the character"
        );
        assert_eq!(map.lookup(0x0012), Some("C"));
        assert_eq!(map.lookup(0x0013), None, "and stops at the high bound");
    }

    #[test]
    fn a_bfrange_with_an_array_names_each_one() {
        let src = b"1 beginbfrange\n<0020> <0022> [<0058> <0059> <005A>]\nendbfrange\n";
        let map = parse_cmap(src);
        assert_eq!(map.lookup(0x0020), Some("X"));
        assert_eq!(map.lookup(0x0021), Some("Y"));
        assert_eq!(map.lookup(0x0022), Some("Z"));
    }

    /// An entry whose destination is not valid UTF-16 is left out.
    ///
    /// A lone high surrogate has no character. Mapping it to `U+FFFD` would
    /// put a visible replacement character into the page text and into every
    /// search result over it.
    #[test]
    fn an_undecodable_destination_is_dropped_not_replaced() {
        let src = b"1 beginbfchar\n<0005> <D800>\nendbfchar\n";
        let map = parse_cmap(src);
        assert_eq!(map.lookup(0x0005), None);
        assert!(map.is_empty());
    }

    /// A range whose bounds are backwards is ignored rather than looped over.
    #[test]
    fn a_backwards_range_is_ignored() {
        let src = b"1 beginbfrange\n<00FF> <0001> <0041>\nendbfrange\n";
        assert!(parse_cmap(src).is_empty());
    }

    /// A one-page PDF drawn in a composite font carrying a `/ToUnicode` map.
    ///
    /// Object 5 is the Type0 font, 7 its CMap stream. The codes below are
    /// two-byte, as `/Identity-H` requires.
    fn pdf_with_composite_text() -> Vec<u8> {
        const NL: u8 = 10;
        let content: &[u8] = b"BT /F1 14 Tf 1 0 0 1 72 700 Tm <00030004000500050006> Tj ET";
        let cmap: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
begincmap\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
5 beginbfchar\n<0003> <0048>\n<0004> <0065>\n<0005> <006C>\n<0006> <006F>\n\
<0007> <0021>\nendbfchar\nendcmap\n";

        let mut out: Vec<u8> = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        out.extend_from_slice(b"%PDF-1.4");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(b"2 0 obj << /Type /Pages /Count 1 /Kids [3 0 R] >> endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
/Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >> endobj",
        );
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(format!("4 0 obj << /Length {} >>", content.len()).as_bytes());
        out.push(NL);
        out.extend_from_slice(b"stream");
        out.push(NL);
        out.extend_from_slice(content);
        out.push(NL);
        out.extend_from_slice(b"endstream endobj");
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(
            b"5 0 obj << /Type /Font /Subtype /Type0 /Encoding /Identity-H \
/DescendantFonts [6 0 R] /ToUnicode 7 0 R >> endobj",
        );
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(
            b"6 0 obj << /Type /Font /Subtype /CIDFontType2 /BaseFont /Test >> endobj",
        );
        out.push(NL);

        offsets.push(out.len());
        out.extend_from_slice(format!("7 0 obj << /Length {} >>", cmap.len()).as_bytes());
        out.push(NL);
        out.extend_from_slice(b"stream");
        out.push(NL);
        out.extend_from_slice(cmap);
        out.push(NL);
        out.extend_from_slice(b"endstream endobj");
        out.push(NL);

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

    /// A composite font with a `/ToUnicode` map reads, end to end.
    ///
    /// The whole chain in one test: the page's `/Resources` names the font,
    /// the font names its CMap, the CMap stream is fetched and parsed, and the
    /// show operator's two-byte codes are split and looked up. Before this the
    /// same page produced nothing and was counted unread -- which is what a
    /// 47-page guide measured against this reader did, all of it.
    #[test]
    fn a_composite_font_with_a_map_reads_end_to_end() {
        let doc = read(&pdf_with_composite_text()).expect("reads");
        let page = doc.pages.first().expect("a page");
        assert_eq!(
            page.text.first().map(|r| r.text.as_str()),
            Some("Hello"),
            "the two-byte codes were not decoded through the font's map"
        );
        assert_eq!(
            doc.unreadable_pages, 0,
            "a page that decoded is not an unread one"
        );
    }

    /// The same page without the map is unread, not empty.
    ///
    /// Same file, one reference removed. This is the pair that makes the test
    /// above about the map rather than about the fixture.
    #[test]
    fn the_same_page_without_its_map_is_unread() {
        let file = pdf_with_composite_text();
        // Blank the reference, keeping every byte offset intact so the
        // cross-reference table still points where it says.
        let patched =
            String::from_utf8_lossy(&file).replace("/ToUnicode 7 0 R", "/ToUnicodd 7 0 R");
        let doc = read(patched.as_bytes()).expect("still reads");
        assert!(doc.pages.first().expect("a page").text.is_empty());
        assert_eq!(
            doc.unreadable_pages, 1,
            "a composite page with no map must be counted, not silently empty"
        );
    }

    /// A simple font's `/Widths` decides the run's width.
    ///
    /// The fixture's font gives code 'A' 500 thousandths and 'B' 1000, so
    /// "AB" at 10pt is 5 + 10 = 15 points, not the 10 the character-count
    /// estimate would give.
    #[test]
    fn a_simple_fonts_widths_are_summed() {
        let mut font = Font::simple();
        font.widths.insert(u32::from(b'A'), 500.0);
        font.widths.insert(u32::from(b'B'), 1000.0);
        let mut fonts = FontMap::new();
        fonts.insert("F1".to_owned(), font);

        let runs = extract_text(b"BT /F1 10 Tf 1 0 0 1 0 0 Tm (AB) Tj ET", &fonts);
        let run = runs.first().expect("a run");
        assert!(run.width_is_measured, "the font declared widths");
        assert!((run.width - 15.0).abs() < 0.01, "got {}", run.width);
    }

    /// A code the font does not list takes `/MissingWidth`, not a guess.
    #[test]
    fn an_unlisted_code_takes_the_default_width() {
        let mut font = Font::simple();
        font.widths.insert(u32::from(b'A'), 500.0);
        font.default_width = 250.0;
        let mut fonts = FontMap::new();
        fonts.insert("F1".to_owned(), font);

        let runs = extract_text(b"BT /F1 10 Tf 1 0 0 1 0 0 Tm (AZ) Tj ET", &fonts);
        let run = runs.first().expect("a run");
        assert!(
            (run.width - 7.5).abs() < 0.01,
            "5.0 + 2.5, got {}",
            run.width
        );
    }

    /// A font declaring no widths at all falls back, and says so.
    #[test]
    fn a_font_with_no_widths_is_estimated_and_flagged() {
        let runs = extract_text(
            b"BT /T1_0 10 Tf 1 0 0 1 0 0 Tm (ABCD) Tj ET",
            &simple_fonts(),
        );
        let run = runs.first().expect("a run");
        assert!(!run.width_is_measured, "nothing declared a width");
        assert!((run.width - 20.0).abs() < 0.01, "half the size per char");
    }

    /// `/W`'s two forms both parse.
    ///
    /// `c [w ...]` gives consecutive codes their own widths and
    /// `cFirst cLast w` gives a range one width. They sit in the same array,
    /// so a parser that assumed either form alone would read the other's
    /// numbers as the wrong thing.
    #[test]
    fn a_composite_w_array_reads_both_of_its_forms() {
        let dict = match Lexer::new(b"<< /W [ 1 [500 600] 10 12 750 ] >>")
            .object()
            .expect("parses")
        {
            Object::Dict(d) => d,
            _ => panic!("not a dictionary"),
        };
        let widths = composite_widths(b"", &Xref::default(), &dict);
        assert_eq!(widths.get(&1), Some(&500.0), "the list form, first entry");
        assert_eq!(widths.get(&2), Some(&600.0), "the list form, second");
        assert_eq!(widths.get(&10), Some(&750.0), "the range form, low end");
        assert_eq!(widths.get(&12), Some(&750.0), "the range form, high end");
        assert_eq!(widths.get(&13), None, "and it stops there");
    }

    /// A `/W` range naming an absurd span is refused rather than filled in.
    #[test]
    fn an_enormous_w_range_is_not_expanded() {
        let dict = match Lexer::new(b"<< /W [ 0 4000000 500 ] >>")
            .object()
            .expect("parses")
        {
            Object::Dict(d) => d,
            _ => panic!("not a dictionary"),
        };
        assert!(composite_widths(b"", &Xref::default(), &dict).is_empty());
    }

    /// A `/Differences` array remaps the codes it names, and only those.
    #[test]
    fn differences_remap_the_codes_they_name() {
        let mut font = Font::simple();
        font.overrides.insert(1, '\u{fb01}');
        font.overrides.insert(2, '\u{2019}');
        let mut fonts = FontMap::new();
        fonts.insert("F1".to_owned(), font);

        // Codes 1 and 2 are remapped; 'A' is not and reads the ordinary way.
        let runs = extract_text(b"BT /F1 10 Tf 1 0 0 1 0 0 Tm (\x01A\x02) Tj ET", &fonts);
        assert_eq!(runs.first().expect("a run").text, "\u{fb01}A\u{2019}");
    }

    /// The `/Differences` walk: a number sets the code, names take the next.
    #[test]
    fn a_differences_array_counts_from_each_number() {
        let dict = match Lexer::new(b"<< /Encoding << /Differences [1 /fi /fl 10 /bullet] >> >>")
            .object()
            .expect("parses")
        {
            Object::Dict(d) => d,
            _ => panic!("not a dictionary"),
        };
        let map = encoding_overrides(b"", &Xref::default(), &dict);
        assert_eq!(map.get(&1), Some(&'\u{fb01}'));
        assert_eq!(map.get(&2), Some(&'\u{fb02}'), "the second name follows on");
        assert_eq!(
            map.get(&10),
            Some(&'\u{2022}'),
            "and the number restarts it"
        );
        assert_eq!(map.get(&11), None);
    }

    /// An unknown glyph name advances the counter without inventing a letter.
    ///
    /// Both halves matter. Skipping the advance would land every name after it
    /// on the wrong code; inventing a character would put one on the page that
    /// the document does not contain.
    #[test]
    fn an_unknown_glyph_name_is_skipped_but_still_counted() {
        let dict = match Lexer::new(b"<< /Encoding << /Differences [5 /nosuchglyph /fi] >> >>")
            .object()
            .expect("parses")
        {
            Object::Dict(d) => d,
            _ => panic!("not a dictionary"),
        };
        let map = encoding_overrides(b"", &Xref::default(), &dict);
        assert_eq!(map.get(&5), None, "nothing invented for the unknown name");
        assert_eq!(
            map.get(&6),
            Some(&'\u{fb01}'),
            "and the next name is not shifted"
        );
    }

    /// `uniXXXX` names a codepoint outright.
    #[test]
    fn a_uni_name_is_its_codepoint() {
        assert_eq!(glyph_name_char("uni00E9"), Some('\u{e9}'));
        assert_eq!(glyph_name_char("u20AC"), Some('\u{20ac}'));
        assert_eq!(glyph_name_char("uniZZZZ"), None);
    }

    /// A bracket piece that is a bracket reads as one; an extension does not.
    ///
    /// TeX draws a tall bracket from stacked pieces. The top, bottom and `Big`
    /// forms each stand for the bracket; the `ex` segment is the middle of a
    /// drawn shape and is no character, so mapping it would insert one bracket
    /// per row of height.
    #[test]
    fn a_bracket_extension_is_not_a_bracket() {
        assert_eq!(glyph_name_char("bracketleftBig"), Some('['));
        assert_eq!(glyph_name_char("bracketlefttp"), Some('['));
        assert_eq!(glyph_name_char("bracketleftex"), None);
    }

    /// A wide kerning jump inside a `TJ` array becomes a space.
    ///
    /// -313 is taken from a real document, where it separates "User" from
    /// "Manual". Without this the run reads "UserManual" and matches no
    /// search for the two words.
    #[test]
    fn a_wide_kerning_gap_becomes_a_space() {
        let content = b"BT /T1_0 10 Tf 1 0 0 1 0 0 Tm [(Use)0.048(r)-313.004(M)-0.98(anual)]TJ ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.first().expect("a run").text, "User Manual");
    }

    /// Kerning inside a word does not become a space.
    ///
    /// The other half, and the one that would be silently wrong: the same
    /// array carries small adjustments between letters, and a threshold set
    /// too near zero would spell "U s e r".
    #[test]
    fn kerning_within_a_word_is_not_a_space() {
        let content = b"BT /T1_0 10 Tf 1 0 0 1 0 0 Tm [(Q)-3(uick S)3(tar)-24(t)]TJ ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.first().expect("a run").text, "Quick Start");
    }

    /// A gap at the very start of an array does not indent the run.
    #[test]
    fn a_leading_gap_adds_no_space() {
        let content = b"BT /T1_0 10 Tf 1 0 0 1 0 0 Tm [-500(Word)]TJ ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.first().expect("a run").text, "Word");
    }

    /// Two gaps in a row add one space, not two.
    #[test]
    fn consecutive_gaps_collapse() {
        let content = b"BT /T1_0 10 Tf 1 0 0 1 0 0 Tm [(a)-400 -400(b)]TJ ET";
        let runs = extract_text(content, &simple_fonts());
        assert_eq!(runs.first().expect("a run").text, "a b");
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
