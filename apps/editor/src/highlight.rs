//! Syntax highlighting engine for the Slate OS text editor.
//!
//! Provides token-based syntax highlighting for common programming languages.
//! Each language is implemented as a simple state machine that scans lines
//! character by character and emits styled tokens with byte-offset ranges.
//!
//! Multi-line constructs (block comments, triple-quoted strings, etc.) are
//! tracked via [`HighlightState`], which must be carried from one line to the
//! next during rendering.

use crate::Language;
use guitk::color::Color;

// ============================================================================
// Token types
// ============================================================================

/// Semantic token kind produced by the highlighter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    /// Language keyword (`fn`, `let`, `if`, `for`, ...).
    Keyword,
    /// Type name (`u32`, `String`, `bool`, ...).
    Type,
    /// String literal (including delimiters).
    String,
    /// Numeric literal.
    Number,
    /// Comment (line or block).
    Comment,
    /// Operator (`+`, `-`, `=`, `=>`, ...).
    Operator,
    /// Punctuation (braces, parens, semicolons, ...).
    Punctuation,
    /// C/C++ preprocessor directive (`#include`, `#define`, ...).
    Preprocessor,
    /// Rust attribute (`#[derive(...)]`), Python decorator (`@foo`).
    Attribute,
    /// Macro invocation (`println!`, `vec!`).
    Macro,
    /// Built-in name (`print`, `len`, `range`, ...).
    Builtin,
    /// Variable reference (shell `$VAR`).
    Variable,
    /// Function name at call site.
    Function,
    /// Markdown heading (`# ...`).
    Heading,
    /// Markdown bold (`**...**`).
    Bold,
    /// Markdown italic (`*...*`).
    Italic,
    /// Markdown link (`[text](url)`).
    Link,
    /// Markdown fenced code block delimiter.
    CodeBlock,
    /// Unclassified text.
    Plain,
}

// ============================================================================
// Styled token
// ============================================================================

/// A token with its byte-offset range in the source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledToken {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// Semantic kind.
    pub kind: Token,
}

// ============================================================================
// Highlight state (carried between lines)
// ============================================================================

/// State carried across line boundaries for multi-line constructs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HighlightState {
    /// Normal code — no multi-line construct in progress.
    Normal,
    /// Inside a `/* ... */` block comment.  The `usize` tracks nesting depth
    /// for languages that support nested block comments (Rust).
    BlockComment { depth: usize },
    /// Inside a multi-line string literal.
    /// `delimiter` is the opening sequence (e.g. `\"\"\"` for Python, `` ` ``
    /// for JS template literals).
    MultiLineString { delimiter: StringDelimiter },
    /// Inside a Markdown fenced code block.
    CodeFence,
}

/// Identifies the kind of multi-line string delimiter so we know when to close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringDelimiter {
    /// Python `\"\"\"`.
    TripleDouble,
    /// Python `'''`.
    TripleSingle,
    /// JavaScript/TypeScript template literal (`` ` ``).
    Backtick,
    /// Rust raw string `r#"..."#` with a given number of `#` signs.
    RustRaw { hashes: usize },
}

// ============================================================================
// Theme mapping
// ============================================================================

/// Maps semantic tokens to colours.  The default uses Catppuccin Mocha.
pub struct Theme {
    pub keyword: Color,
    pub type_name: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub preprocessor: Color,
    pub attribute: Color,
    pub macro_name: Color,
    pub builtin: Color,
    pub variable: Color,
    pub function: Color,
    pub heading: Color,
    pub bold: Color,
    pub italic: Color,
    pub link: Color,
    pub code_block: Color,
    pub plain: Color,
}

impl Theme {
    /// Catppuccin Mocha dark theme.
    pub const fn catppuccin_mocha() -> Self {
        Self {
            keyword: Color::from_hex(0xCBA6F7),      // mauve
            type_name: Color::from_hex(0xF9E2AF),    // yellow
            string: Color::from_hex(0xA6E3A1),       // green
            number: Color::from_hex(0xFAB387),       // peach
            comment: Color::from_hex(0x6C7086),      // overlay0
            operator: Color::from_hex(0x89DCEB),      // sky
            punctuation: Color::from_hex(0x9399B2),   // overlay2
            preprocessor: Color::from_hex(0xF5C2E7),  // pink
            attribute: Color::from_hex(0xF5C2E7),     // pink
            macro_name: Color::from_hex(0x94E2D5),    // teal
            builtin: Color::from_hex(0xFAB387),       // peach
            variable: Color::from_hex(0xCDD6F4),      // text
            function: Color::from_hex(0x89B4FA),      // blue
            heading: Color::from_hex(0xF38BA8),       // red
            bold: Color::from_hex(0xFAB387),          // peach
            italic: Color::from_hex(0xF5C2E7),        // pink
            link: Color::from_hex(0x89B4FA),           // blue
            code_block: Color::from_hex(0xA6E3A1),    // green
            plain: Color::from_hex(0xCDD6F4),         // text
        }
    }

    /// Look up the colour for a given token kind.
    pub const fn color_for(&self, token: Token) -> Color {
        match token {
            Token::Keyword => self.keyword,
            Token::Type => self.type_name,
            Token::String => self.string,
            Token::Number => self.number,
            Token::Comment => self.comment,
            Token::Operator => self.operator,
            Token::Punctuation => self.punctuation,
            Token::Preprocessor => self.preprocessor,
            Token::Attribute => self.attribute,
            Token::Macro => self.macro_name,
            Token::Builtin => self.builtin,
            Token::Variable => self.variable,
            Token::Function => self.function,
            Token::Heading => self.heading,
            Token::Bold => self.bold,
            Token::Italic => self.italic,
            Token::Link => self.link,
            Token::CodeBlock => self.code_block,
            Token::Plain => self.plain,
        }
    }
}

/// Default theme.
pub static DEFAULT_THEME: Theme = Theme::catppuccin_mocha();

// ============================================================================
// Language detection
// ============================================================================

/// The language a path's extension names, or [`Language::Plain`] for a path
/// with no extension or an unrecognised one.
///
/// The one place the mapping happens, because a document that is *loaded* as
/// Rust and *coloured* as plain text is a disagreement the user sees and cannot
/// explain.
pub fn language_of_path(path: &std::path::Path) -> Language {
    path.extension().map_or(Language::Plain, |ext| {
        Language::from_extension(&ext.to_string_lossy())
    })
}

// ============================================================================
// Highlight entry point
// ============================================================================

/// Highlight a single line of source code.
///
/// `state` is updated in place to carry multi-line context (block comments,
/// multi-line strings, etc.) into subsequent lines.
///
/// Returns a list of styled tokens covering every byte of the input line.
pub fn highlight_line(
    line: &str,
    language: Language,
    state: &mut HighlightState,
) -> Vec<StyledToken> {
    match language {
        Language::Rust => highlight_rust(line, state),
        Language::Python => highlight_python(line, state),
        Language::C => highlight_c(line, state),
        Language::JavaScript => highlight_javascript(line, state),
        Language::Json => highlight_json(line, state),
        Language::Toml => highlight_toml(line, state),
        Language::Markdown => highlight_markdown(line, state),
        Language::Shell => highlight_shell(line, state),
        _ => vec![StyledToken {
            start: 0,
            end: line.len(),
            kind: Token::Plain,
        }],
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Push a token only if `start < end`.
fn push_token(tokens: &mut Vec<StyledToken>, start: usize, end: usize, kind: Token) {
    if start < end {
        tokens.push(StyledToken { start, end, kind });
    }
}

/// Advance `i` past the whole UTF-8 character starting there.
///
/// The `.min(bytes.len())` is defensive, not a bug fix, and the distinction
/// is worth stating because `scan_string` right below carries the *same*
/// clamp for a bug that is live. It cannot trigger on today's callers: every
/// one of them derives `bytes` from a `&str`, so a lead byte always has its
/// continuation bytes present and the unclamped sum never exceeds the length.
/// It is here because the signature promises less than the callers deliver —
/// this takes `&[u8]`, where a truncated sequence is representable — and
/// because returning a past-the-end offset is not a benign error here:
/// callers use the result as a token boundary, which panics when the renderer
/// slices it. Removing the clamp breaks no test; that is the point.
fn advance(bytes: &[u8], i: usize) -> usize {
    let extra = at(bytes, i).map_or(0, |b| match b {
        0xC0..=0xDF => 1,
        0xE0..=0xEF => 2,
        0xF0..=0xFF => 3,
        _ => 0,
    });
    i.saturating_add(1).saturating_add(extra).min(bytes.len())
}

/// Check if `bytes[i..]` starts with the given ASCII slice.
fn starts_with_at(bytes: &[u8], i: usize, needle: &[u8]) -> bool {
    bytes.get(i..i.saturating_add(needle.len())) == Some(needle)
}

/// The byte at `i`, or `None` at or past the end of the line.
///
/// Every scanner in this file is a loop of the shape "while the byte here is
/// X, advance". Written as `i < bytes.len() && bytes[i] == X` that states the
/// bound *twice* — once in the guard and once again, invisibly, inside the
/// index — and the two can drift apart when the loop body later changes how
/// far `i` moves. `at(bytes, i).is_some_and(...)` states it once, in the only
/// place that can be wrong, and the compiler stops us writing the second one.
fn at(bytes: &[u8], i: usize) -> Option<u8> {
    bytes.get(i).copied()
}

/// True when the bytes at `i` and `i + 1` are exactly `first` then `second`.
///
/// Two-byte lookahead (`/*`, `*/`, `//`, `->`, `==`) is the most common test
/// here, and the hand-written form has to get both the `i + 1 < len` guard and
/// the two indices right every time. Naming it removes thirty-odd chances to
/// get one of them wrong.
fn is_pair(bytes: &[u8], i: usize, first: u8, second: u8) -> bool {
    at(bytes, i) == Some(first) && at(bytes, i.saturating_add(1)) == Some(second)
}

/// Find the offset just past the next unescaped `needle` at or after `i`.
///
/// `None` means the line ends before the delimiter appears — which for a
/// multi-line string is not an error but the normal case, and is what tells
/// the caller to record a `HighlightState::MultiLineString` and colour the
/// rest of the line as string.
///
/// The callers previously each inlined a `while i + 2 < len` loop with the
/// delimiter's length baked into the bound as a literal `2` — correct only
/// for a three-byte needle, and correct there only by coincidence.
fn scan_to_delimiter(bytes: &[u8], mut i: usize, needle: &[u8]) -> Option<usize> {
    while i < bytes.len() {
        if at(bytes, i) == Some(b'\\') {
            i = i.saturating_add(2);
            continue;
        }
        if starts_with_at(bytes, i, needle) {
            return Some(i.saturating_add(needle.len()).min(bytes.len()));
        }
        i = i.saturating_add(1);
    }
    None
}

/// Offset just past the next `needle` at or after `from`, or the end of the
/// line if there isn't one.
///
/// This is Markdown's inline-span rule. Spans are highlighted optimistically:
/// a `**` with no closing `**` colours the rest of the line rather than
/// nothing, because while the user is still typing the span there is no
/// closing marker yet. Inline code and italic already behaved that way; bold
/// did not — its `while i + 1 < len` stopped one byte short, so `**abc`
/// bolded `**ab` and left `c` plain, which is neither of the two defensible
/// answers.
///
/// Unlike `scan_to_delimiter` this does *not* skip backslash escapes, which
/// preserves the existing behaviour exactly; Markdown escaping is a separate
/// question from where a span ends.
fn close_or_end(bytes: &[u8], from: usize, needle: &[u8]) -> usize {
    find_close(bytes, from, needle).unwrap_or(bytes.len())
}

/// As `close_or_end`, but distinguishing "closed at the very end of the line"
/// from "never closed".
///
/// Markdown's link syntax needs the distinction and the optimistic version
/// cannot express it: `[text](url)` is a link only once `](` has been seen,
/// and `](` occurring as the last two bytes returns the same offset as not
/// occurring at all.
fn find_close(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let mut i = from;
    while i < bytes.len() {
        if starts_with_at(bytes, i, needle) {
            return Some(i.saturating_add(needle.len()).min(bytes.len()));
        }
        i = i.saturating_add(1);
    }
    None
}

/// Recognise a triple-quote delimiter (`"""` or `'''`) at `i`.
///
/// Returns the delimiter both as the byte string to search for and as the
/// `StringDelimiter` to record in `HighlightState`, so the two cannot be
/// paired up wrongly at a call site.
fn triple_at(bytes: &[u8], i: usize) -> Option<(&'static [u8], StringDelimiter)> {
    if starts_with_at(bytes, i, b"\"\"\"") {
        Some((b"\"\"\"", StringDelimiter::TripleDouble))
    } else if starts_with_at(bytes, i, b"'''") {
        Some((b"'''", StringDelimiter::TripleSingle))
    } else {
        None
    }
}

/// Count consecutive `#` starting at `i`, stopping at `max`.
///
/// This is the Rust raw-string terminator test: `r##"…"##` ends at the first
/// `"` followed by exactly as many `#` as opened it. Both the resume-from-the
/// -line-above arm and the open-it-here arm need it, and both previously
/// inlined the same three-clause loop whose bound, index and counter all had
/// to agree.
fn count_hashes(bytes: &[u8], i: usize, max: usize) -> usize {
    let mut h = 0;
    while h < max && at(bytes, i.saturating_add(h)) == Some(b'#') {
        h = h.saturating_add(1);
    }
    h
}

/// Check whether `word` is in the given sorted keyword list.
fn is_keyword(word: &str, keywords: &[&str]) -> bool {
    keywords.binary_search(&word).is_ok()
}

/// Check whether the byte is an ASCII identifier character.
fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Scan an identifier/word starting at `i` and return (end_offset, word).
fn scan_word(bytes: &[u8], i: usize) -> (usize, &str) {
    let mut end = i;
    while at(bytes, end).is_some_and(is_ident_byte) {
        end = end.saturating_add(1);
    }
    // We only accepted ASCII bytes, so this is valid UTF-8; the `unwrap_or`
    // is unreachable rather than a fallback.
    let word = bytes
        .get(i..end)
        .and_then(|w| std::str::from_utf8(w).ok())
        .unwrap_or("");
    (end, word)
}

/// Scan a number literal (int or float, with optional 0x/0o/0b prefix).
fn scan_number(bytes: &[u8], i: usize) -> usize {
    /// Advance `end` over every byte matching `accept`.
    fn take_while(bytes: &[u8], end: &mut usize, accept: impl Fn(u8) -> bool) {
        while at(bytes, *end).is_some_and(&accept) {
            *end = end.saturating_add(1);
        }
    }

    let mut end = i;
    // Hex/oct/bin prefix. `radix_digit` is the acceptor for whichever base the
    // second byte named; picking it here keeps the three arms from being three
    // near-identical loops that can drift.
    if at(bytes, end) == Some(b'0') {
        let radix_digit: Option<fn(u8) -> bool> = match at(bytes, end.saturating_add(1)) {
            Some(b'x' | b'X') => Some(|b: u8| b.is_ascii_hexdigit() || b == b'_'),
            Some(b'o' | b'O') => Some(|b: u8| b.is_ascii_digit() || b == b'_'),
            Some(b'b' | b'B') => Some(|b: u8| matches!(b, b'0' | b'1' | b'_')),
            _ => None,
        };
        if let Some(accept) = radix_digit {
            end = end.saturating_add(2);
            take_while(bytes, &mut end, accept);
            return end;
        }
    }
    // Decimal digits
    take_while(bytes, &mut end, |b| b.is_ascii_digit() || b == b'_');
    // Decimal point + fraction. The point only belongs to the number if a
    // digit follows it, so `1..2` stays a range and `1.` stays an integer.
    if at(bytes, end) == Some(b'.') {
        let after_dot = end.saturating_add(1);
        if at(bytes, after_dot).is_some_and(|b| b.is_ascii_digit()) {
            end = after_dot;
            take_while(bytes, &mut end, |b| b.is_ascii_digit() || b == b'_');
        }
    }
    // Exponent — likewise only consumed if it is actually followed by digits,
    // so the `e` in `1e` stays a type suffix.
    if at(bytes, end).is_some_and(|b| b == b'e' || b == b'E') {
        let mut exp = end.saturating_add(1);
        if at(bytes, exp).is_some_and(|b| b == b'+' || b == b'-') {
            exp = exp.saturating_add(1);
        }
        if at(bytes, exp).is_some_and(|b| b.is_ascii_digit()) {
            end = exp;
            take_while(bytes, &mut end, |b| b.is_ascii_digit() || b == b'_');
        }
    }
    // Type suffix (u8, i32, f64, usize, ...)
    if at(bytes, end).is_some_and(|b| b.is_ascii_alphabetic()) {
        take_while(bytes, &mut end, |b| b.is_ascii_alphanumeric());
    }
    end
}

/// Scan a string literal starting at `i` (which must point to the opening
/// quote character).  Returns the end offset (past the closing quote).
/// Handles `\"` escapes inside the string.
fn scan_string(bytes: &[u8], i: usize, quote: u8) -> usize {
    let mut end = i.saturating_add(1); // skip the opening quote
    while let Some(b) = at(bytes, end) {
        if b == b'\\' {
            // Skip the escaped character — but a backslash in the *last* byte
            // of the line escapes nothing, so clamp rather than stepping to
            // `len + 1`. Every caller feeds this straight into `push_token`,
            // and a token whose `end` exceeds the line panics the moment the
            // renderer slices it. Clamping here fixes it for all ten callers.
            end = end.saturating_add(2).min(bytes.len());
        } else if b == quote {
            return end.saturating_add(1); // include closing quote
        } else {
            end = end.saturating_add(1);
        }
    }
    end // unterminated — extends to end of line
}

/// Scan forward through a `/* … */` block comment already known to be open.
///
/// `i` is the first byte *inside* the comment (past the opening `/*` for a
/// comment that starts on this line; `0` for one resumed from the line
/// above). `depth` is how many `/*` are currently open, always ≥ 1 on entry.
/// Returns the offset just past where the comment ended and the depth still
/// open there — a returned depth of `0` means it closed on this line, and any
/// other value is what belongs in `HighlightState::BlockComment`.
///
/// `nested` distinguishes Rust, where `/* /* */ */` is one comment, from C,
/// JavaScript and CSS, where the first `*/` closes it whatever came before.
/// Passing `false` makes an inner `/*` ordinary comment text.
///
/// This existed as six near-copies — an open-it-here and a resume-from-above
/// arm in each of three tokenizers — which is six places for the `i + 1 < len`
/// guard and the two-byte step to disagree.
fn scan_block_comment(bytes: &[u8], mut i: usize, mut depth: usize, nested: bool) -> (usize, usize) {
    while depth > 0 {
        if nested && is_pair(bytes, i, b'/', b'*') {
            depth = depth.saturating_add(1);
            i = i.saturating_add(2);
        } else if is_pair(bytes, i, b'*', b'/') {
            depth = depth.saturating_sub(1);
            i = i.saturating_add(2);
        } else if i < bytes.len() {
            i = i.saturating_add(1);
        } else {
            break; // ran off the end with the comment still open
        }
    }
    (i.min(bytes.len()), depth)
}

const OPERATOR_BYTES: &[u8] = b"+-*/%=!<>&|^~?@";

fn is_operator_byte(b: u8) -> bool {
    OPERATOR_BYTES.contains(&b)
}

const PUNCTUATION_BYTES: &[u8] = b"(){}[];:,.";

fn is_punctuation_byte(b: u8) -> bool {
    PUNCTUATION_BYTES.contains(&b)
}

// ============================================================================
// Rust highlighter
// ============================================================================

// Keywords must be sorted for binary search.
const RUST_KEYWORDS: &[&str] = &[
    "Self", "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
    "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod",
    "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
    "type", "union", "unsafe", "use", "where", "while", "yield",
];

const RUST_TYPES: &[&str] = &[
    "Arc", "Box", "HashMap", "HashSet", "Mutex", "Option", "Rc", "Result", "String", "Vec",
    "bool", "char", "f32", "f64", "i128", "i16", "i32", "i64", "i8", "isize", "str", "u128",
    "u16", "u32", "u64", "u8", "usize",
];

fn highlight_rust(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    // Continue multi-line state from previous line.
    match state {
        HighlightState::BlockComment { depth } => {
            let (end, remaining) = scan_block_comment(bytes, i, *depth, true);
            i = end;
            push_token(&mut tokens, 0, i, Token::Comment);
            if remaining == 0 {
                *state = HighlightState::Normal;
            } else {
                // Still inside the comment — it consumed the whole line.
                *depth = remaining;
                return tokens;
            }
        }
        HighlightState::MultiLineString {
            delimiter: StringDelimiter::RustRaw { hashes },
        } => {
            let needed = *hashes;
            while let Some(b) = at(bytes, i) {
                if b == b'"' && count_hashes(bytes, i.saturating_add(1), needed) == needed {
                    i = i.saturating_add(1).saturating_add(needed).min(len);
                    push_token(&mut tokens, 0, i, Token::String);
                    *state = HighlightState::Normal;
                    break;
                }
                i = i.saturating_add(1);
            }
            if *state != HighlightState::Normal {
                push_token(&mut tokens, 0, len, Token::String);
                return tokens;
            }
        }
        _ => {}
    }

    while let Some(b) = at(bytes, i) {

        // Line comment
        if is_pair(bytes, i, b'/', b'/') {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Block comment
        if is_pair(bytes, i, b'/', b'*') {
            let start = i;
            let (end, depth) = scan_block_comment(bytes, i.saturating_add(2), 1, true);
            i = end;
            if depth > 0 {
                *state = HighlightState::BlockComment { depth };
            }
            push_token(&mut tokens, start, i, Token::Comment);
            continue;
        }

        // Attribute: `#[...]` or `#![...]`
        if b == b'#'
            && (is_pair(bytes, i, b'#', b'[')
                || (is_pair(bytes, i, b'#', b'!')
                    && at(bytes, i.saturating_add(2)) == Some(b'[')))
        {
            let start = i;
            // Find the matching `]`. The scan starts on the `#`, and the
            // guard above guarantees a `[` within two bytes of it, so the
            // depth is raised before it can be lowered; `saturating_sub`
            // records that rather than relying on the reader to re-derive it.
            let mut bracket_depth = 0usize;
            while let Some(c) = at(bytes, i) {
                if c == b'[' {
                    bracket_depth = bracket_depth.saturating_add(1);
                } else if c == b']' {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    if bracket_depth == 0 {
                        i = i.saturating_add(1);
                        break;
                    }
                }
                i = i.saturating_add(1);
            }
            push_token(&mut tokens, start, i, Token::Attribute);
            continue;
        }

        // Raw string: r"...", r#"..."#, r##"..."##, etc.
        if b == b'r' && at(bytes, i.saturating_add(1)).is_some_and(|c| c == b'"' || c == b'#') {
            let start = i;
            // `usize::MAX` because the *opening* run of hashes has no bound —
            // it is what defines how many the closing run needs.
            let hashes = count_hashes(bytes, i.saturating_add(1), usize::MAX);
            let mut j = i.saturating_add(1).saturating_add(hashes);
            if at(bytes, j) == Some(b'"') {
                // It's a raw string.
                j = j.saturating_add(1); // past opening quote
                loop {
                    let Some(c) = at(bytes, j) else {
                        // Ran off the end: this raw string spans lines.
                        *state = HighlightState::MultiLineString {
                            delimiter: StringDelimiter::RustRaw { hashes },
                        };
                        push_token(&mut tokens, start, len, Token::String);
                        return tokens;
                    };
                    if c == b'"'
                        && count_hashes(bytes, j.saturating_add(1), hashes) == hashes
                    {
                        j = j.saturating_add(1).saturating_add(hashes).min(len);
                        break;
                    }
                    j = j.saturating_add(1);
                }
                push_token(&mut tokens, start, j, Token::String);
                i = j;
                continue;
            }
            // Not a raw string — fall through to identifier handling below.
        }

        // String or char literal
        if b == b'"' || b == b'\'' {
            let start = i;
            // Char literal `'a'` — but NOT lifetime `'a` followed by ident without closing quote on same token.
            if b == b'\'' {
                // Check if this looks like a lifetime: `'ident` not followed by `'`.
                let mut j = i.saturating_add(1);
                if at(bytes, j).is_some_and(|c| c.is_ascii_alphabetic() || c == b'_') {
                    // Could be a char literal like 'a' or a lifetime like 'a.
                    let mut k = j;
                    while at(bytes, k).is_some_and(is_ident_byte) {
                        k = k.saturating_add(1);
                    }
                    if at(bytes, k) == Some(b'\'') {
                        // Char literal.
                        i = k.saturating_add(1);
                        push_token(&mut tokens, start, i, Token::String);
                        continue;
                    }
                    // Lifetime.
                    push_token(&mut tokens, start, k, Token::Attribute);
                    i = k;
                    continue;
                }
                // Escaped char literal: '\n', '\\'
                if at(bytes, j) == Some(b'\\') {
                    j = j.saturating_add(1); // skip escape marker
                    if j < len {
                        j = j.saturating_add(1); // skip escaped char
                    }
                    if at(bytes, j) == Some(b'\'') {
                        j = j.saturating_add(1);
                    }
                    push_token(&mut tokens, start, j, Token::String);
                    i = j;
                    continue;
                }
                // Single char: 'x'
                if j + 1 < len && bytes[j + 1] == b'\'' {
                    i = j + 2;
                    push_token(&mut tokens, start, i, Token::String);
                    continue;
                }
                // Stray single quote — treat as operator.
                push_token(&mut tokens, i, i + 1, Token::Operator);
                i += 1;
                continue;
            }
            // Double-quoted string.
            let end = scan_string(bytes, i, b'"');
            push_token(&mut tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Number
        if b.is_ascii_digit()
                || (b == b'.'
                    && at(bytes, i.saturating_add(1)).is_some_and(|c| c.is_ascii_digit()))
            {
            let start = i;
            let end = scan_number(bytes, i);
            push_token(&mut tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Identifier / keyword / type / macro
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let (end, word) = scan_word(bytes, i);
            // Macro invocation: word followed by `!`
            if at(bytes, end) == Some(b'!') && !word.is_empty() {
                let kind = Token::Macro;
                push_token(&mut tokens, start, end + 1, kind);
                i = end + 1;
                continue;
            }
            let kind = if is_keyword(word, RUST_KEYWORDS) {
                Token::Keyword
            } else if is_keyword(word, RUST_TYPES) {
                Token::Type
            } else if at(bytes, end) == Some(b'(') {
                Token::Function
            } else if word.starts_with(|c: char| c.is_ascii_uppercase()) {
                Token::Type
            } else {
                Token::Plain
            };
            push_token(&mut tokens, start, end, kind);
            i = end;
            continue;
        }

        // Operators
        if is_operator_byte(b) {
            let start = i;
            while at(bytes, i).is_some_and(is_operator_byte) {
                i = i.saturating_add(1);
            }
            push_token(&mut tokens, start, i, Token::Operator);
            continue;
        }

        // Punctuation
        if is_punctuation_byte(b) {
            push_token(&mut tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        // Whitespace and other — plain
        let start = i;
        i = advance(bytes, i);
        push_token(&mut tokens, start, i, Token::Plain);
    }

    tokens
}

// ============================================================================
// Python highlighter
// ============================================================================

const PYTHON_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class",
    "continue", "def", "del", "elif", "else", "except", "finally", "for", "from", "global",
    "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return",
    "try", "while", "with", "yield",
];

const PYTHON_BUILTINS: &[&str] = &[
    "abs", "all", "any", "bin", "bool", "bytes", "callable", "chr", "classmethod", "compile",
    "complex", "delattr", "dict", "dir", "divmod", "enumerate", "eval", "exec", "filter",
    "float", "format", "frozenset", "getattr", "globals", "hasattr", "hash", "help", "hex",
    "id", "input", "int", "isinstance", "issubclass", "iter", "len", "list", "locals", "map",
    "max", "memoryview", "min", "next", "object", "oct", "open", "ord", "pow", "print",
    "property", "range", "repr", "reversed", "round", "set", "setattr", "slice", "sorted",
    "staticmethod", "str", "sum", "super", "tuple", "type", "vars", "zip",
];

fn highlight_python(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();

    // Continue multi-line string from previous line.
    if let HighlightState::MultiLineString { delimiter } = state {
        let needle: &[u8] = match delimiter {
            StringDelimiter::TripleDouble => b"\"\"\"",
            StringDelimiter::TripleSingle => b"'''",
            _ => {
                *state = HighlightState::Normal;
                return highlight_python_normal(line, 0, &mut tokens, state);
            }
        };
        if let Some(end) = scan_to_delimiter(bytes, 0, needle) {
            push_token(&mut tokens, 0, end, Token::String);
            *state = HighlightState::Normal;
            // Continue highlighting the rest of the line.
            highlight_python_normal(line, end, &mut tokens, state);
            return tokens;
        }
        // Didn't find closing — rest of line is string.
        push_token(&mut tokens, 0, len, Token::String);
        return tokens;
    }

    highlight_python_normal(line, 0, &mut tokens, state);
    tokens
}

fn highlight_python_normal(
    line: &str,
    start_offset: usize,
    tokens: &mut Vec<StyledToken>,
    state: &mut HighlightState,
) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut i = start_offset;

    while let Some(b) = at(bytes, i) {

        // Comment
        if b == b'#' {
            push_token(tokens, i, len, Token::Comment);
            return Vec::new();
        }

        // Decorator
        if b == b'@' {
            let start = i;
            i += 1;
            while at(bytes, i).is_some_and(|c| is_ident_byte(c) || c == b'.') {
                i = i.saturating_add(1);
            }
            push_token(tokens, start, i, Token::Attribute);
            continue;
        }

        // Triple-quoted string (must check before single-quoted)
        if (b == b'"' || b == b'\'') && i + 2 < len && bytes[i + 1] == b && bytes[i + 2] == b {
            // Check for f-string prefix
            let actual_start = if i > start_offset {
                let prev = bytes[i - 1];
                if prev == b'f' || prev == b'F' || prev == b'b' || prev == b'B'
                    || prev == b'r' || prev == b'R'
                {
                    // Rewrite the previous token if it was a plain single-char ident.
                    if let Some(last) = tokens.last() {
                        if last.end == i && last.start == i - 1 {
                            let prev_start = last.start;
                            tokens.pop();
                            prev_start
                        } else {
                            i
                        }
                    } else {
                        i
                    }
                } else {
                    i
                }
            } else {
                i
            };
            let delimiter = if b == b'"' {
                StringDelimiter::TripleDouble
            } else {
                StringDelimiter::TripleSingle
            };
            let needle: &[u8] = if b == b'"' { b"\"\"\"" } else { b"'''" };
            i += 3; // past opening triple quote
            loop {
                if i + 2 >= len {
                    // Multi-line string — extends to next line.
                    *state = HighlightState::MultiLineString {
                        delimiter,
                    };
                    push_token(tokens, actual_start, len, Token::String);
                    return Vec::new();
                }
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if starts_with_at(bytes, i, needle) {
                    i += 3;
                    break;
                }
                i += 1;
            }
            push_token(tokens, actual_start, i, Token::String);
            continue;
        }

        // f-string / b-string / r-string prefix before quote
        if (b == b'f' || b == b'F' || b == b'b' || b == b'B' || b == b'r' || b == b'R')
            && i + 1 < len
            && (bytes[i + 1] == b'"' || bytes[i + 1] == b'\'')
        {
            let start = i;
            i += 1; // skip prefix
            let quote = bytes[i];
            let end = scan_string(bytes, i, quote);
            push_token(tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Regular string
        if b == b'"' || b == b'\'' {
            let start = i;
            let end = scan_string(bytes, i, b);
            push_token(tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Number
        if b.is_ascii_digit()
                || (b == b'.'
                    && at(bytes, i.saturating_add(1)).is_some_and(|c| c.is_ascii_digit()))
            {
            let start = i;
            let end = scan_number(bytes, i);
            push_token(tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Identifier / keyword / builtin / function
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let (end, word) = scan_word(bytes, i);
            let kind = if is_keyword(word, PYTHON_KEYWORDS) {
                Token::Keyword
            } else if is_keyword(word, PYTHON_BUILTINS) {
                Token::Builtin
            } else if at(bytes, end) == Some(b'(') {
                Token::Function
            } else {
                Token::Plain
            };
            push_token(tokens, start, end, kind);
            i = end;
            continue;
        }

        // Operators
        if is_operator_byte(b) {
            let start = i;
            while at(bytes, i).is_some_and(is_operator_byte) {
                i = i.saturating_add(1);
            }
            push_token(tokens, start, i, Token::Operator);
            continue;
        }

        // Punctuation
        if is_punctuation_byte(b) {
            push_token(tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        // Whitespace / other
        let start = i;
        i = advance(bytes, i);
        push_token(tokens, start, i, Token::Plain);
    }

    Vec::new()
}

// ============================================================================
// C/C++ highlighter
// ============================================================================

const C_KEYWORDS: &[&str] = &[
    "alignas", "alignof", "auto", "bool", "break", "case", "catch", "class", "const",
    "constexpr", "constinit", "continue", "decltype", "default", "delete", "do", "else", "enum",
    "explicit", "export", "extern", "false", "final", "for", "friend", "goto", "if", "inline",
    "mutable", "namespace", "new", "noexcept", "nullptr", "operator", "override", "private",
    "protected", "public", "register", "requires", "return", "signed", "sizeof", "static",
    "static_assert", "static_cast", "struct", "switch", "template", "this", "throw", "true",
    "try", "typedef", "typeid", "typename", "union", "unsigned", "using", "virtual", "void",
    "volatile", "while",
];

const C_TYPES: &[&str] = &[
    "FILE", "char", "char16_t", "char32_t", "char8_t", "double", "float", "int", "int16_t",
    "int32_t", "int64_t", "int8_t", "intptr_t", "long", "ptrdiff_t", "short", "size_t",
    "ssize_t", "uint16_t", "uint32_t", "uint64_t", "uint8_t", "uintptr_t", "wchar_t",
];

fn highlight_c(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    // Continue block comment from previous line.
    if let HighlightState::BlockComment { .. } = state {
        // C comments do not nest: the first `*/` closes it, so `nested` is
        // false and the depth is only ever 1 or 0.
        let (end, depth) = scan_block_comment(bytes, i, 1, false);
        i = end;
        push_token(&mut tokens, 0, i, Token::Comment);
        if depth == 0 {
            *state = HighlightState::Normal;
        } else {
            return tokens;
        }
    }

    // Preprocessor directive — if first non-whitespace is `#`.
    // Only check when we haven't already consumed a block comment prefix.
    if i == 0
        && bytes
            .iter()
            .find(|&&b| b != b' ' && b != b'\t')
            .is_some_and(|&b| b == b'#')
    {
        push_token(&mut tokens, 0, len, Token::Preprocessor);
        return tokens;
    }

    while let Some(b) = at(bytes, i) {

        // Line comment
        if is_pair(bytes, i, b'/', b'/') {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Block comment. The old form here decided whether the comment had
        // closed by reading the two bytes *behind* the cursor, which is both
        // hard to read and one `i >= 2` guard away from a panic; the helper
        // returns the answer directly.
        if is_pair(bytes, i, b'/', b'*') {
            let start = i;
            let (end, depth) = scan_block_comment(bytes, i.saturating_add(2), 1, false);
            i = end;
            if depth > 0 {
                *state = HighlightState::BlockComment { depth: 1 };
            }
            push_token(&mut tokens, start, i, Token::Comment);
            continue;
        }

        // String / char literal
        if b == b'"' || b == b'\'' {
            let start = i;
            let end = scan_string(bytes, i, b);
            push_token(&mut tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Number
        if b.is_ascii_digit()
                || (b == b'.'
                    && at(bytes, i.saturating_add(1)).is_some_and(|c| c.is_ascii_digit()))
            {
            let start = i;
            let end = scan_number(bytes, i);
            push_token(&mut tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Identifier / keyword / type
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            let (end, word) = scan_word(bytes, i);
            let kind = if is_keyword(word, C_KEYWORDS) {
                Token::Keyword
            } else if is_keyword(word, C_TYPES) {
                Token::Type
            } else if at(bytes, end) == Some(b'(') {
                Token::Function
            } else if word.chars().all(|c| c.is_ascii_uppercase() || c == b'_' as char) && word.len() > 1 {
                // ALL_CAPS identifiers are usually macros/constants in C.
                Token::Macro
            } else {
                Token::Plain
            };
            push_token(&mut tokens, start, end, kind);
            i = end;
            continue;
        }

        // Operators
        if is_operator_byte(b) {
            let start = i;
            while at(bytes, i).is_some_and(is_operator_byte) {
                i = i.saturating_add(1);
            }
            push_token(&mut tokens, start, i, Token::Operator);
            continue;
        }

        // Punctuation
        if is_punctuation_byte(b) {
            push_token(&mut tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        let start = i;
        i = advance(bytes, i);
        push_token(&mut tokens, start, i, Token::Plain);
    }

    tokens
}

// ============================================================================
// JavaScript / TypeScript highlighter
// ============================================================================

const JS_KEYWORDS: &[&str] = &[
    "abstract", "arguments", "as", "async", "await", "break", "case", "catch", "class", "const",
    "continue", "debugger", "default", "delete", "do", "else", "enum", "export", "extends",
    "false", "finally", "for", "from", "function", "get", "if", "implements", "import", "in",
    "instanceof", "interface", "let", "new", "null", "of", "package", "private", "protected",
    "public", "return", "set", "static", "super", "switch", "this", "throw", "true", "try",
    "type", "typeof", "undefined", "var", "void", "while", "with", "yield",
];

const JS_BUILTINS: &[&str] = &[
    "Array", "Boolean", "Buffer", "Console", "Date", "Error", "Function", "Infinity", "JSON",
    "Map", "Math", "NaN", "Number", "Object", "Promise", "Proxy", "Reflect", "RegExp",
    "Set", "String", "Symbol", "WeakMap", "WeakSet", "clearInterval", "clearTimeout",
    "console", "decodeURI", "encodeURI", "eval", "fetch", "globalThis",
    "isFinite", "isNaN", "parseInt", "parseFloat", "process", "require",
    "setInterval", "setTimeout", "window",
];

fn highlight_javascript(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    // Continue block comment from previous line.
    if let HighlightState::BlockComment { .. } = state {
        // JavaScript comments do not nest, so `nested` is false.
        let (end, depth) = scan_block_comment(bytes, i, 1, false);
        i = end;
        push_token(&mut tokens, 0, i, Token::Comment);
        if depth == 0 {
            *state = HighlightState::Normal;
        } else {
            return tokens;
        }
    }

    // Continue template literal from previous line. This is `scan_string`'s
    // loop with the opening quote already consumed on an earlier line, so it
    // is spelled the same way — including clamping the escape step, since a
    // line ending in `\` inside a template literal has the same overshoot.
    if let HighlightState::MultiLineString {
        delimiter: StringDelimiter::Backtick,
    } = state
    {
        while let Some(b) = at(bytes, i) {
            if b == b'\\' {
                i = i.saturating_add(2).min(len);
            } else if b == b'`' {
                i = i.saturating_add(1);
                push_token(&mut tokens, 0, i, Token::String);
                *state = HighlightState::Normal;
                break;
            } else {
                i = i.saturating_add(1);
            }
        }
        if *state != HighlightState::Normal {
            push_token(&mut tokens, 0, len, Token::String);
            return tokens;
        }
    }

    while let Some(b) = at(bytes, i) {

        // Line comment
        if is_pair(bytes, i, b'/', b'/') {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Block comment
        if is_pair(bytes, i, b'/', b'*') {
            let start = i;
            let (end, depth) = scan_block_comment(bytes, i.saturating_add(2), 1, false);
            i = end;
            if depth > 0 {
                *state = HighlightState::BlockComment { depth: 1 };
            }
            push_token(&mut tokens, start, i, Token::Comment);
            continue;
        }

        // Template literal
        if b == b'`' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'`' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            // Check if it closed.
            if i <= len && i > start + 1 && bytes[i - 1] == b'`' {
                push_token(&mut tokens, start, i, Token::String);
            } else {
                *state = HighlightState::MultiLineString {
                    delimiter: StringDelimiter::Backtick,
                };
                push_token(&mut tokens, start, len, Token::String);
                return tokens;
            }
            continue;
        }

        // Regex literal — simple heuristic: `/` after `=`, `(`, `,`, `[`, `!`, `&`, `|`, `:`, `;`, `{`, `}`, `return`, newline start
        if b == b'/' && i + 1 < len && bytes[i + 1] != b'/' && bytes[i + 1] != b'*' {
            let is_regex = if i == 0 {
                true
            } else {
                let prev_non_ws = bytes[..i]
                    .iter()
                    .rposition(|&c| c != b' ' && c != b'\t');
                match prev_non_ws {
                    Some(p) => matches!(
                        bytes[p],
                        b'=' | b'(' | b',' | b'[' | b'!' | b'&' | b'|' | b':' | b';' | b'{' | b'}'
                    ),
                    None => true,
                }
            };
            if is_regex {
                let start = i;
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'/' {
                        i += 1;
                        // Regex flags
                        while i < len && bytes[i].is_ascii_alphabetic() {
                            i += 1;
                        }
                        break;
                    }
                    i += 1;
                }
                push_token(&mut tokens, start, i, Token::String);
                continue;
            }
        }

        // String
        if b == b'"' || b == b'\'' {
            let start = i;
            let end = scan_string(bytes, i, b);
            push_token(&mut tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Number
        if b.is_ascii_digit()
                || (b == b'.'
                    && at(bytes, i.saturating_add(1)).is_some_and(|c| c.is_ascii_digit()))
            {
            let start = i;
            let end = scan_number(bytes, i);
            push_token(&mut tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Identifier
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            while at(bytes, i).is_some_and(|c| is_ident_byte(c) || c == b'$') {
                i = i.saturating_add(1);
            }
            let word = bytes
                .get(start..i)
                .and_then(|w| std::str::from_utf8(w).ok())
                .unwrap_or("");
            let kind = if is_keyword(word, JS_KEYWORDS) {
                Token::Keyword
            } else if is_keyword(word, JS_BUILTINS) {
                Token::Builtin
            } else if i < len && bytes[i] == b'(' {
                Token::Function
            } else {
                Token::Plain
            };
            push_token(&mut tokens, start, i, kind);
            continue;
        }

        // Operators
        if is_operator_byte(b) {
            let start = i;
            while at(bytes, i).is_some_and(is_operator_byte) {
                i = i.saturating_add(1);
            }
            push_token(&mut tokens, start, i, Token::Operator);
            continue;
        }

        // Punctuation
        if is_punctuation_byte(b) {
            push_token(&mut tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        let start = i;
        i = advance(bytes, i);
        push_token(&mut tokens, start, i, Token::Plain);
    }

    tokens
}

// ============================================================================
// JSON highlighter
// ============================================================================

fn highlight_json(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let _ = state; // JSON has no multi-line constructs we need to track.
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    // Track whether the next string is a key (true) or value (false).
    // A string is a key if it's followed (ignoring whitespace) by `:`.
    while let Some(b) = at(bytes, i) {

        // String
        if b == b'"' {
            let start = i;
            let end = scan_string(bytes, i, b'"');
            // Look ahead for `:` to decide key vs value.
            let mut j = end;
            while j < len && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            let kind = if j < len && bytes[j] == b':' {
                Token::Function // Use Function colour for keys (blue).
            } else {
                Token::String
            };
            push_token(&mut tokens, start, end, kind);
            i = end;
            continue;
        }

        // Number
        if b.is_ascii_digit() || b == b'-' || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            // For `-`, only treat as number start if followed by digit.
            if b == b'-' {
                if i + 1 < len && bytes[i + 1].is_ascii_digit() {
                    let start = i;
                    i += 1; // skip minus
                    let end = scan_number(bytes, i);
                    push_token(&mut tokens, start, end, Token::Number);
                    i = end;
                    continue;
                }
                // Otherwise, it's an operator.
                push_token(&mut tokens, i, i + 1, Token::Operator);
                i += 1;
                continue;
            }
            let start = i;
            let end = scan_number(bytes, i);
            push_token(&mut tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Boolean / null keywords
        if b.is_ascii_alphabetic() {
            let start = i;
            let (end, word) = scan_word(bytes, i);
            let kind = match word {
                "true" | "false" | "null" => Token::Keyword,
                _ => Token::Plain,
            };
            push_token(&mut tokens, start, end, kind);
            i = end;
            continue;
        }

        // Colon
        if b == b':' {
            push_token(&mut tokens, i, i + 1, Token::Operator);
            i += 1;
            continue;
        }

        // Punctuation
        if is_punctuation_byte(b) {
            push_token(&mut tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        let start = i;
        i = advance(bytes, i);
        push_token(&mut tokens, start, i, Token::Plain);
    }

    tokens
}

// ============================================================================
// TOML highlighter
// ============================================================================

fn highlight_toml(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    // Continue a multi-line string from the previous line. Before this arm
    // existed, `highlight_toml` did `let _ = state;` and dropped the carry
    // entirely, so the second and later lines of a `"""…"""` were tokenized
    // as if they were keys and values.
    if let HighlightState::MultiLineString { delimiter } = state {
        let needle: &[u8] = match delimiter {
            StringDelimiter::TripleSingle => b"'''",
            // TOML has no backtick or raw-string form; anything else is a
            // stale state from another language and ends here.
            _ => b"\"\"\"",
        };
        if let Some(end) = scan_to_delimiter(bytes, 0, needle) {
            push_token(&mut tokens, 0, end, Token::String);
            *state = HighlightState::Normal;
            i = end;
        } else {
            push_token(&mut tokens, 0, len, Token::String);
            return tokens;
        }
    }

    // Skip leading whitespace.
    while at(bytes, i).is_some_and(|c| c == b' ' || c == b'\t') {
        i = i.saturating_add(1);
    }
    if i > 0 {
        push_token(&mut tokens, 0, i, Token::Plain);
    }

    if i >= len {
        return tokens;
    }

    // Comment line
    if at(bytes, i) == Some(b'#') {
        push_token(&mut tokens, i, len, Token::Comment);
        return tokens;
    }

    // Section header: `[section]` or `[[array]]`
    if at(bytes, i) == Some(b'[') {
        push_token(&mut tokens, i, len, Token::Attribute);
        return tokens;
    }

    // Key = value
    // Scan key (everything up to `=`).
    let key_start = i;
    while at(bytes, i).is_some_and(|c| c != b'=' && c != b'#') {
        i = i.saturating_add(1);
    }
    if at(bytes, i) == Some(b'=') {
        // Key
        push_token(&mut tokens, key_start, i, Token::Function);
        // Equals sign
        push_token(&mut tokens, i, i.saturating_add(1), Token::Operator);
        i = i.saturating_add(1);

        // Value — highlight strings, numbers, booleans.
        while let Some(b) = at(bytes, i) {

            if b == b'#' {
                push_token(&mut tokens, i, len, Token::Comment);
                return tokens;
            }

            // Multi-line strings: `"""…"""` (basic) and `'''…'''` (literal).
            // Both are ordinary TOML — `description = """…"""` spanning lines
            // is in half the Cargo.toml files in this tree.
            if let Some((needle, delimiter)) = triple_at(bytes, i) {
                let start = i;
                if let Some(end) = scan_to_delimiter(bytes, i.saturating_add(3), needle) {
                    push_token(&mut tokens, start, end, Token::String);
                    i = end;
                    continue;
                }
                // Unclosed: the string runs onto the next line.
                *state = HighlightState::MultiLineString { delimiter };
                push_token(&mut tokens, start, len, Token::String);
                return tokens;
            }

            if b == b'"' || b == b'\'' {
                let start = i;
                let end = scan_string(bytes, i, b);
                push_token(&mut tokens, start, end, Token::String);
                i = end;
                continue;
            }

            if b.is_ascii_digit()
                || (b == b'-' && i + 1 < len && bytes[i + 1].is_ascii_digit())
                || (b == b'+' && i + 1 < len && bytes[i + 1].is_ascii_digit())
            {
                let start = i;
                if b == b'-' || b == b'+' {
                    i += 1;
                }
                let end = scan_number(bytes, i);
                push_token(&mut tokens, start, end, Token::Number);
                i = end;
                continue;
            }

            if b.is_ascii_alphabetic() {
                let start = i;
                let (end, word) = scan_word(bytes, i);
                let kind = match word {
                    "true" | "false" => Token::Keyword,
                    _ => Token::Plain,
                };
                push_token(&mut tokens, start, end, kind);
                i = end;
                continue;
            }

            if is_punctuation_byte(b) {
                push_token(&mut tokens, i, i + 1, Token::Punctuation);
                i += 1;
                continue;
            }

            let start = i;
            i = advance(bytes, i);
            push_token(&mut tokens, start, i, Token::Plain);
        }
    } else {
        // No `=` found — treat rest as plain.
        push_token(&mut tokens, key_start, len, Token::Plain);
    }

    tokens
}

// ============================================================================
// Markdown highlighter
// ============================================================================

fn highlight_markdown(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();

    if len == 0 {
        return tokens;
    }

    // Code fence toggle
    if starts_with_at(bytes, 0, b"```") {
        push_token(&mut tokens, 0, len, Token::CodeBlock);
        *state = match state {
            HighlightState::CodeFence => HighlightState::Normal,
            _ => HighlightState::CodeFence,
        };
        return tokens;
    }

    // Inside code fence — everything is code.
    if *state == HighlightState::CodeFence {
        push_token(&mut tokens, 0, len, Token::String);
        return tokens;
    }

    let mut i = 0;

    // Heading: lines starting with `#`
    if bytes[0] == b'#' {
        push_token(&mut tokens, 0, len, Token::Heading);
        return tokens;
    }

    // Unordered list marker
    if len >= 2
        && (bytes[0] == b'-' || bytes[0] == b'*' || bytes[0] == b'+')
        && bytes[1] == b' '
    {
        push_token(&mut tokens, 0, 2, Token::Keyword);
        i = 2;
    }

    // Ordered list marker: `1. `, `12. ` etc.
    if bytes[0].is_ascii_digit() {
        let mut j = 0;
        while j < len && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j < len && bytes[j] == b'.' && j + 1 < len && bytes[j + 1] == b' ' {
            push_token(&mut tokens, 0, j + 2, Token::Keyword);
            i = j + 2;
        }
    }

    // Inline formatting
    while let Some(b) = at(bytes, i) {

        // Inline code: `...`
        if b == b'`' {
            let start = i;
            i = close_or_end(bytes, i.saturating_add(1), b"`");
            push_token(&mut tokens, start, i, Token::CodeBlock);
            continue;
        }

        // Bold: **...**
        if is_pair(bytes, i, b'*', b'*') {
            let start = i;
            i = close_or_end(bytes, i.saturating_add(2), b"**");
            push_token(&mut tokens, start, i, Token::Bold);
            continue;
        }

        // Italic: *...*
        if b == b'*' {
            let start = i;
            i = close_or_end(bytes, i.saturating_add(1), b"*");
            push_token(&mut tokens, start, i, Token::Italic);
            continue;
        }

        // Link: [text](url)
        if b == b'[' {
            let start = i;
            // A link is only a link once `](` has been seen; until then the
            // `[` could equally be a stray bracket, so the two halves are
            // found separately rather than with one scan.
            if let Some(after_bracket) = find_close(bytes, i.saturating_add(1), b"](") {
                i = close_or_end(bytes, after_bracket, b")");
                push_token(&mut tokens, start, i, Token::Link);
            } else {
                i = len;
                push_token(&mut tokens, start, i, Token::Plain);
            }
            continue;
        }

        let start = i;
        i = advance(bytes, i);
        push_token(&mut tokens, start, i, Token::Plain);
    }

    tokens
}

// ============================================================================
// Shell/Bash highlighter
// ============================================================================

const SHELL_KEYWORDS: &[&str] = &[
    "break", "case", "continue", "do", "done", "elif", "else", "esac", "export", "fi", "for",
    "function", "if", "in", "local", "read", "readonly", "return", "select", "shift", "source",
    "then", "trap", "unset", "until", "while",
];

const SHELL_BUILTINS: &[&str] = &[
    "alias", "bg", "bind", "builtin", "cd", "command", "compgen", "complete", "declare", "dirs",
    "disown", "echo", "enable", "eval", "exec", "exit", "fg", "getopts", "hash", "help",
    "history", "jobs", "kill", "let", "logout", "popd", "printf", "pushd", "pwd", "set",
    "shopt", "test", "times", "type", "ulimit", "umask", "unalias", "wait",
];

fn highlight_shell(line: &str, state: &mut HighlightState) -> Vec<StyledToken> {
    let _ = state;
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while let Some(b) = at(bytes, i) {

        // Comment (but not inside a string)
        if b == b'#' {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Variable: $VAR, ${VAR}, $0-$9, $$, $?, $!, $@, $*
        if b == b'$' {
            let start = i;
            i += 1;
            if i < len {
                if bytes[i] == b'{' {
                    // ${VAR}
                    i += 1;
                    while i < len && bytes[i] != b'}' {
                        i += 1;
                    }
                    if i < len {
                        i += 1; // include `}`
                    }
                } else if bytes[i] == b'(' {
                    // $(command) — treat as variable.
                    i += 1;
                    let mut paren_depth = 1u32;
                    while i < len && paren_depth > 0 {
                        if bytes[i] == b'(' {
                            paren_depth += 1;
                        } else if bytes[i] == b')' {
                            paren_depth -= 1;
                        }
                        i += 1;
                    }
                } else if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
                    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                        i += 1;
                    }
                } else {
                    // Special variables: $$, $?, $!, $@, $*, $#
                    i += 1;
                }
            }
            push_token(&mut tokens, start, i, Token::Variable);
            continue;
        }

        // Double-quoted string (with variable interpolation — we just colour the whole thing as string).
        if b == b'"' {
            let start = i;
            let end = scan_string(bytes, i, b'"');
            push_token(&mut tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Single-quoted string (no interpolation).
        if b == b'\'' {
            let start = i;
            let end = scan_string(bytes, i, b'\'');
            push_token(&mut tokens, start, end, Token::String);
            i = end;
            continue;
        }

        // Number
        if b.is_ascii_digit() {
            let start = i;
            let end = scan_number(bytes, i);
            push_token(&mut tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Pipe, redirect, background
        if b == b'|' || b == b'>' || b == b'<' || b == b'&' {
            let start = i;
            // Handle `||`, `&&`, `>>`, `<<`, `|&`
            i += 1;
            if i < len && (bytes[i] == bytes[i - 1] || bytes[i] == b'&') {
                i += 1;
            }
            push_token(&mut tokens, start, i, Token::Operator);
            continue;
        }

        // Semicolon
        if b == b';' {
            push_token(&mut tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        // Identifier / keyword / builtin
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            // Shell identifiers can include `-` in command names.
            while at(bytes, i).is_some_and(|c| is_ident_byte(c) || c == b'-') {
                i = i.saturating_add(1);
            }
            let word = bytes
                .get(start..i)
                .and_then(|w| std::str::from_utf8(w).ok())
                .unwrap_or("");
            let kind = if is_keyword(word, SHELL_KEYWORDS) {
                Token::Keyword
            } else if is_keyword(word, SHELL_BUILTINS) {
                Token::Builtin
            } else {
                Token::Plain
            };
            push_token(&mut tokens, start, i, kind);
            continue;
        }

        // Other operators
        if is_operator_byte(b) {
            push_token(&mut tokens, i, i + 1, Token::Operator);
            i += 1;
            continue;
        }

        // Punctuation
        if is_punctuation_byte(b) {
            push_token(&mut tokens, i, i + 1, Token::Punctuation);
            i += 1;
            continue;
        }

        let start = i;
        i = advance(bytes, i);
        push_token(&mut tokens, start, i, Token::Plain);
    }

    tokens
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: highlight a line and collect (kind, text) pairs.
    fn tokens_of(line: &str, lang: Language) -> Vec<(Token, String)> {
        let mut state = HighlightState::Normal;
        let toks = highlight_line(line, lang, &mut state);
        toks.iter()
            .map(|t| (t.kind, line[t.start..t.end].to_string()))
            .collect()
    }

    /// Helper: check that a specific token kind appears with a given text.
    fn has_token(toks: &[(Token, String)], kind: Token, text: &str) -> bool {
        toks.iter().any(|(k, t)| *k == kind && t == text)
    }

    // ====================================================================
    // Non-ASCII source
    // ====================================================================
    //
    // Every tokenizer here scans `line.as_bytes()` and advances one byte in
    // its default branch. Token ranges are byte ranges, so a boundary landing
    // inside a character would panic any caller that slices by them --
    // including `tokens_of` above, and including the renderer if highlighting
    // is ever wired up (see known-issues.md). Most boundaries should be sound
    // by UTF-8 self-synchronization, since they are decided by ASCII
    // delimiters, but "should be" is not "is": these check every language.

    /// Every language, with non-ASCII text in each construct a tokenizer
    /// treats specially: a string literal, a comment, and a bare identifier.
    fn non_ascii_lines() -> Vec<(Language, &'static str)> {
        vec![
            (Language::Rust, "let 変数 = \"日本語\"; // コメント"),
            (Language::Rust, "/* 日本語 */ fn αβγ() {}"),
            (Language::Rust, "let s = \"emoji 😀 in a string\";"),
            (Language::C, "int главная = 0; /* комментарий */"),
            (Language::C, "char *s = \"日本語\"; // コメント"),
            (Language::Python, "变量 = \"日本語\"  # コメント"),
            (Language::Python, "s = '''日本語 multi'''"),
            (Language::JavaScript, "const 変数 = `日本語 ${x}`; // コメント"),
            (Language::JavaScript, "let s = \"emoji 😀\"; /* κόσμε */"),
            (Language::Html, "<p class=\"日本語\">κόσμε</p><!-- コメント -->"),
            (Language::Css, ".日本語 { content: \"κόσμε\"; } /* コメント */"),
            (Language::Shell, "echo \"日本語\" # コメント"),
            (Language::Toml, "名前 = \"日本語\" # コメント"),
            (Language::Yaml, "名前: \"日本語\" # コメント"),
            (Language::Json, "{\"名前\": \"日本語\"}"),
            (Language::Markdown, "# 見出し **太字** `コード` [リンク](url)"),
            (Language::Plain, "日本語 κόσμε 😀"),
        ]
    }

    #[test]
    fn no_tokenizer_splits_a_character() {
        let mut checked = 0;
        for (lang, line) in non_ascii_lines() {
            let mut state = HighlightState::Normal;
            let toks = highlight_line(line, lang, &mut state);
            for t in &toks {
                // Checked before slicing so a failure names the offending
                // offset instead of aborting inside `str::index`.
                assert!(
                    t.start <= t.end && t.end <= line.len(),
                    "{lang:?}: token range {}..{} out of bounds for {line:?}",
                    t.start,
                    t.end
                );
                assert!(
                    line.is_char_boundary(t.start),
                    "{lang:?}: token start {} is inside a character in {line:?}",
                    t.start
                );
                assert!(
                    line.is_char_boundary(t.end),
                    "{lang:?}: token end {} is inside a character in {line:?}",
                    t.end
                );
            }
            // And the slice itself, which is what `tokens_of` does.
            let _ = tokens_of(line, lang);
            checked += 1;
        }
        assert!(checked >= 17, "only {checked} lines checked");
    }

    /// Every language, so a new one cannot be added without being swept by
    /// the truncation test below.
    const ALL_LANGUAGES: &[Language] = &[
        Language::Plain,
        Language::Rust,
        Language::C,
        Language::Python,
        Language::JavaScript,
        Language::Html,
        Language::Css,
        Language::Shell,
        Language::Toml,
        Language::Yaml,
        Language::Json,
        Language::Markdown,
    ];

    #[test]
    fn no_tokenizer_runs_off_the_end_of_a_truncated_line() {
        // Regression: `scan_string` stepped two bytes past a backslash without
        // checking that there was a second byte to step over, so a line ending
        // in `\` produced a token whose `end` was `line.len() + 1`. Nothing
        // clamped it in any of the ten call sites, so the first `line[..end]`
        // in the renderer panicked. `advance` had the identical bug for a
        // multi-byte character truncated by the end of the line.
        //
        // A line ending mid-construct is not exotic — it is what the buffer
        // holds for the whole time the user is typing the construct.
        let endings = [
            "\\",       // dangling escape
            "\"\\",     // open string, dangling escape
            "'\\",      // open char, dangling escape
            "\"",       // unterminated string
            "/*",       // unterminated block comment
            "/",        // half an operator or comment
            "0x",       // radix prefix with no digits
            "1e",       // exponent with no digits
            "1.",       // decimal point with no fraction
            "r#\"",     // unterminated Rust raw string
            "`",        // unterminated JS template literal
            "<",        // unterminated HTML tag
            "&",        // unterminated HTML entity
            "$",        // shell expansion with no name
        ];
        // Prefixes put the ending somewhere other than offset 0, and the
        // non-ASCII ones also exercise `advance`'s truncation clamp.
        let prefixes = ["", "let x = ", "日本語", "😀", "a\u{300}"];

        let mut checked = 0_usize;
        for &lang in ALL_LANGUAGES {
            for prefix in prefixes {
                for ending in endings {
                    let line = format!("{prefix}{ending}");
                    // Fresh state, and again resuming from each multi-line
                    // state, since those re-enter the scanners mid-construct.
                    for mut state in [
                        HighlightState::Normal,
                        HighlightState::BlockComment { depth: 1 },
                    ] {
                        let toks = highlight_line(&line, lang, &mut state);
                        for t in &toks {
                            assert!(
                                t.start <= t.end && t.end <= line.len(),
                                "{lang:?}: token {}..{} out of bounds for {line:?} \
                                 (len {})",
                                t.start,
                                t.end,
                                line.len()
                            );
                            assert!(
                                line.is_char_boundary(t.start)
                                    && line.is_char_boundary(t.end),
                                "{lang:?}: token {}..{} splits a character in \
                                 {line:?}",
                                t.start,
                                t.end
                            );
                            // The slice itself — the operation that panicked.
                            let _ = &line[t.start..t.end];
                        }
                        checked = checked.saturating_add(1);
                    }
                }
            }
        }
        assert_eq!(
            checked,
            ALL_LANGUAGES.len() * prefixes.len() * endings.len() * 2,
            "sweep did not cover every combination"
        );
    }

    #[test]
    fn a_closed_toml_triple_quoted_string_is_one_token_not_two() {
        // Regression: the branch pushed the token once on finding the closing
        // `"""` and then again on the way out, so every single-line `"""…"""`
        // was drawn twice, one draw exactly on top of the other.
        let line = "k = \"\"\"abc\"\"\"";
        let strings: Vec<_> = tokens_of(line, Language::Toml)
            .into_iter()
            .filter(|(kind, _)| *kind == Token::String)
            .collect();
        assert_eq!(
            strings,
            vec![(Token::String, "\"\"\"abc\"\"\"".to_string())],
            "expected exactly one String token"
        );
    }

    #[test]
    fn a_toml_multi_line_string_carries_to_the_following_lines() {
        // Regression: `highlight_toml` began `let _ = state;`, so it neither
        // set nor read the multi-line carry. An unterminated `"""` coloured
        // four bytes and stopped, and the following lines were tokenized as
        // though they were `key = value` pairs.
        let mut state = HighlightState::Normal;

        let open = "description = \"\"\"first";
        let toks = highlight_line(open, Language::Toml, &mut state);
        assert_eq!(
            state,
            HighlightState::MultiLineString {
                delimiter: StringDelimiter::TripleDouble
            },
            "opening ``\"\"\"`` should carry to the next line"
        );
        let last = toks.last().expect("expected a token");
        assert_eq!(
            (last.kind, last.end),
            (Token::String, open.len()),
            "the string should run to the end of the line"
        );

        // A middle line is entirely string, and does not close the state.
        let middle = "  key = not actually a key";
        let toks = highlight_line(middle, Language::Toml, &mut state);
        assert_eq!(
            toks.iter().map(|t| t.kind).collect::<Vec<_>>(),
            vec![Token::String],
            "a line inside the string is one String token and nothing else"
        );
        assert!(matches!(state, HighlightState::MultiLineString { .. }));

        // The closing line ends the string and resumes normal tokenizing.
        let close = "last\"\"\" # trailing comment";
        let toks = highlight_line(close, Language::Toml, &mut state);
        assert_eq!(state, HighlightState::Normal, "``\"\"\"`` should close it");
        assert_eq!(
            toks.first().map(|t| (t.kind, t.end)),
            Some((Token::String, 7)),
            "the string should end just past the closing delimiter"
        );

        // `'''` is the literal form and carries the same way.
        let mut state = HighlightState::Normal;
        highlight_line("k = '''open", Language::Toml, &mut state);
        assert_eq!(
            state,
            HighlightState::MultiLineString {
                delimiter: StringDelimiter::TripleSingle
            }
        );
    }

    #[test]
    fn an_unclosed_markdown_span_colours_the_rest_of_the_line() {
        // All four inline spans are highlighted optimistically: while the
        // user is still typing one there is no closing marker yet, so the
        // alternative is for the text to flicker uncoloured as it is typed.
        //
        // Regression: bold alone stopped one byte short of the end, because
        // its scan was `while i + 1 < len` and nothing extended it after the
        // loop. `**abc` bolded `**ab` and left `c` plain — not the
        // colour-it-all answer and not the colour-none answer either.
        for (line, kind, text) in [
            ("**abc", Token::Bold, "**abc"),
            ("*abc", Token::Italic, "*abc"),
            ("`abc", Token::CodeBlock, "`abc"),
            ("[abc", Token::Plain, "[abc"),
        ] {
            let toks = tokens_of(line, Language::Markdown);
            assert!(
                toks.contains(&(kind, text.to_string())),
                "{line:?}: expected {kind:?} {text:?}, got {toks:?}"
            );
        }

        // And the closed forms still stop at the closing marker.
        for (line, kind, text) in [
            ("**a** tail", Token::Bold, "**a**"),
            ("*a* tail", Token::Italic, "*a*"),
            ("`a` tail", Token::CodeBlock, "`a`"),
            ("[a](u) tail", Token::Link, "[a](u)"),
        ] {
            let toks = tokens_of(line, Language::Markdown);
            assert!(
                toks.contains(&(kind, text.to_string())),
                "{line:?}: expected {kind:?} {text:?}, got {toks:?}"
            );
        }

        // A `[` with no `](` is not a link, however much text follows.
        let toks = tokens_of("[not a link) at all", Language::Markdown);
        assert!(
            !toks.iter().any(|(k, _)| *k == Token::Link),
            "expected no Link token, got {toks:?}"
        );
    }

    #[test]
    fn multi_line_state_survives_non_ascii() {
        // Carrying a block comment across lines is the path where a tokenizer
        // resumes mid-construct, re-entering the byte loop at an offset it
        // did not choose itself.
        let mut state = HighlightState::Normal;
        let mut checked = 0;
        for line in ["/* 日本語", "まだコメント 😀", "終わり */ let x = 1;"] {
            let toks = highlight_line(line, Language::Rust, &mut state);
            for t in &toks {
                assert!(line.is_char_boundary(t.start), "{line:?} @ {}", t.start);
                assert!(line.is_char_boundary(t.end), "{line:?} @ {}", t.end);
            }
            checked += 1;
        }
        assert_eq!(checked, 3);
        assert_eq!(state, HighlightState::Normal, "comment should have closed");
    }

    // ====================================================================
    // Language detection
    // ====================================================================

    fn lang_of(name: &str) -> Language {
        language_of_path(std::path::Path::new(name))
    }

    #[test]
    fn detect_language_from_filename() {
        assert_eq!(lang_of("main.rs"), Language::Rust);
        assert_eq!(lang_of("script.py"), Language::Python);
        assert_eq!(lang_of("app.js"), Language::JavaScript);
        assert_eq!(lang_of("app.ts"), Language::JavaScript);
        assert_eq!(lang_of("lib.c"), Language::C);
        assert_eq!(lang_of("lib.cpp"), Language::C);
        assert_eq!(lang_of("lib.h"), Language::C);
        assert_eq!(lang_of("config.json"), Language::Json);
        assert_eq!(lang_of("Cargo.toml"), Language::Toml);
        assert_eq!(lang_of("README.md"), Language::Markdown);
        assert_eq!(lang_of("run.sh"), Language::Shell);
        assert_eq!(lang_of("file.txt"), Language::Plain);
        assert_eq!(lang_of("noext"), Language::Plain);
        // A whole path, not just a name: this is the form the document loader
        // actually calls it with.
        assert_eq!(lang_of("/home/u/src/main.rs"), Language::Rust);
        // A dotfile's leading dot is a name, not an extension.
        assert_eq!(lang_of(".bashrc"), Language::Plain);
    }

    // ====================================================================
    // Rust
    // ====================================================================

    #[test]
    fn rust_keywords() {
        let toks = tokens_of("fn main() {", Language::Rust);
        assert!(has_token(&toks, Token::Keyword, "fn"));
        assert!(has_token(&toks, Token::Function, "main"));
    }

    #[test]
    fn rust_types() {
        let toks = tokens_of("let x: u32 = 0;", Language::Rust);
        assert!(has_token(&toks, Token::Keyword, "let"));
        assert!(has_token(&toks, Token::Type, "u32"));
        assert!(has_token(&toks, Token::Number, "0"));
    }

    #[test]
    fn rust_string() {
        let toks = tokens_of(r#"let s = "hello";"#, Language::Rust);
        assert!(has_token(&toks, Token::String, r#""hello""#));
    }

    #[test]
    fn rust_line_comment() {
        let toks = tokens_of("let x = 1; // comment", Language::Rust);
        assert!(has_token(&toks, Token::Comment, "// comment"));
    }

    #[test]
    fn rust_block_comment_single_line() {
        let toks = tokens_of("/* block */ let x = 1;", Language::Rust);
        assert!(has_token(&toks, Token::Comment, "/* block */"));
        assert!(has_token(&toks, Token::Keyword, "let"));
    }

    #[test]
    fn rust_block_comment_multiline() {
        let mut state = HighlightState::Normal;

        let line1 = "/* start of";
        let toks1 = highlight_line(line1, Language::Rust, &mut state);
        assert_eq!(toks1.len(), 1);
        assert_eq!(toks1[0].kind, Token::Comment);
        assert!(matches!(state, HighlightState::BlockComment { depth: 1 }));

        let line2 = "   still comment */";
        let toks2 = highlight_line(line2, Language::Rust, &mut state);
        assert_eq!(toks2.len(), 1);
        assert_eq!(toks2[0].kind, Token::Comment);
        assert_eq!(state, HighlightState::Normal);
    }

    #[test]
    fn rust_nested_block_comment() {
        let mut state = HighlightState::Normal;

        let line1 = "/* outer /* inner */";
        let toks1 = highlight_line(line1, Language::Rust, &mut state);
        assert_eq!(toks1[0].kind, Token::Comment);
        // depth should be 1 — inner closed, outer still open.
        assert!(matches!(state, HighlightState::BlockComment { depth: 1 }));

        let line2 = " end outer */";
        let toks2 = highlight_line(line2, Language::Rust, &mut state);
        assert_eq!(toks2[0].kind, Token::Comment);
        assert_eq!(state, HighlightState::Normal);
    }

    #[test]
    fn rust_attribute() {
        let toks = tokens_of("#[derive(Debug)]", Language::Rust);
        assert!(has_token(&toks, Token::Attribute, "#[derive(Debug)]"));
    }

    #[test]
    fn rust_macro() {
        let toks = tokens_of("println!(\"hi\");", Language::Rust);
        assert!(has_token(&toks, Token::Macro, "println!"));
    }

    #[test]
    fn rust_lifetime() {
        let toks = tokens_of("fn foo<'a>(x: &'a str)", Language::Rust);
        assert!(has_token(&toks, Token::Attribute, "'a"));
    }

    #[test]
    fn rust_char_literal() {
        let toks = tokens_of("let c = 'x';", Language::Rust);
        assert!(has_token(&toks, Token::String, "'x'"));
    }

    #[test]
    fn rust_number_formats() {
        let toks = tokens_of("0xFF 0b1010 0o77 3.14 1_000", Language::Rust);
        assert!(has_token(&toks, Token::Number, "0xFF"));
        assert!(has_token(&toks, Token::Number, "0b1010"));
        assert!(has_token(&toks, Token::Number, "0o77"));
        assert!(has_token(&toks, Token::Number, "3.14"));
        assert!(has_token(&toks, Token::Number, "1_000"));
    }

    // ====================================================================
    // Python
    // ====================================================================

    #[test]
    fn python_keywords() {
        let toks = tokens_of("def hello():", Language::Python);
        assert!(has_token(&toks, Token::Keyword, "def"));
        assert!(has_token(&toks, Token::Function, "hello"));
    }

    #[test]
    fn python_builtins() {
        let toks = tokens_of("x = len(items)", Language::Python);
        assert!(has_token(&toks, Token::Builtin, "len"));
    }

    #[test]
    fn python_decorator() {
        let toks = tokens_of("@staticmethod", Language::Python);
        assert!(has_token(&toks, Token::Attribute, "@staticmethod"));
    }

    #[test]
    fn python_fstring() {
        let toks = tokens_of("f\"hello {name}\"", Language::Python);
        assert!(has_token(&toks, Token::String, "f\"hello {name}\""));
    }

    #[test]
    fn python_triple_quote() {
        let mut state = HighlightState::Normal;

        let line1 = "x = \"\"\"start";
        let toks1 = highlight_line(line1, Language::Python, &mut state);
        // The triple-quote string should start at `"""`
        let string_tok = toks1.iter().find(|t| t.kind == Token::String);
        assert!(string_tok.is_some());
        assert!(matches!(
            state,
            HighlightState::MultiLineString {
                delimiter: StringDelimiter::TripleDouble
            }
        ));

        let line2 = "end of string\"\"\"";
        let toks2 = highlight_line(line2, Language::Python, &mut state);
        assert!(toks2.iter().any(|t| t.kind == Token::String));
        assert_eq!(state, HighlightState::Normal);
    }

    #[test]
    fn python_comment() {
        let toks = tokens_of("x = 1  # comment", Language::Python);
        assert!(has_token(&toks, Token::Comment, "# comment"));
        assert!(has_token(&toks, Token::Number, "1"));
    }

    // ====================================================================
    // C/C++
    // ====================================================================

    #[test]
    fn c_preprocessor() {
        let toks = tokens_of("#include <stdio.h>", Language::C);
        assert_eq!(toks.len(), 1);
        assert_eq!(toks[0].0, Token::Preprocessor);
    }

    #[test]
    fn c_preprocessor_with_indent() {
        let toks = tokens_of("  #define MAX 100", Language::C);
        assert!(toks.iter().any(|t| t.0 == Token::Preprocessor));
    }

    #[test]
    fn c_block_comment_multiline() {
        let mut state = HighlightState::Normal;

        let line1 = "int x; /* start";
        let toks1 = highlight_line(line1, Language::C, &mut state);
        assert!(toks1.iter().any(|t| t.kind == Token::Comment));
        assert!(matches!(state, HighlightState::BlockComment { .. }));

        let line2 = "   middle";
        let toks2 = highlight_line(line2, Language::C, &mut state);
        assert_eq!(toks2[0].kind, Token::Comment);
        assert!(matches!(state, HighlightState::BlockComment { .. }));

        let line3 = "   end */ int y;";
        let toks3 = highlight_line(line3, Language::C, &mut state);
        assert!(toks3.iter().any(|t| t.kind == Token::Comment));
        assert!(toks3.iter().any(|t| t.kind == Token::Type)); // `int` is a type
        assert_eq!(state, HighlightState::Normal);
    }

    #[test]
    fn c_keywords_and_types() {
        let toks = tokens_of("int main(void) {", Language::C);
        assert!(has_token(&toks, Token::Type, "int"));
        assert!(has_token(&toks, Token::Function, "main"));
        assert!(has_token(&toks, Token::Keyword, "void"));
    }

    // ====================================================================
    // JavaScript/TypeScript
    // ====================================================================

    #[test]
    fn js_keywords() {
        let toks = tokens_of("const x = function() {}", Language::JavaScript);
        assert!(has_token(&toks, Token::Keyword, "const"));
        assert!(has_token(&toks, Token::Keyword, "function"));
    }

    #[test]
    fn js_template_literal() {
        let toks = tokens_of("const s = `hello ${name}`", Language::JavaScript);
        assert!(toks.iter().any(|t| t.0 == Token::String));
    }

    #[test]
    fn js_template_literal_multiline() {
        let mut state = HighlightState::Normal;

        let line1 = "const s = `start";
        let toks1 = highlight_line(line1, Language::JavaScript, &mut state);
        assert!(toks1.iter().any(|t| t.kind == Token::String));
        assert!(matches!(
            state,
            HighlightState::MultiLineString {
                delimiter: StringDelimiter::Backtick
            }
        ));

        let line2 = "end`";
        let toks2 = highlight_line(line2, Language::JavaScript, &mut state);
        assert!(toks2.iter().any(|t| t.kind == Token::String));
        assert_eq!(state, HighlightState::Normal);
    }

    #[test]
    fn js_line_comment() {
        let toks = tokens_of("// this is a comment", Language::JavaScript);
        assert_eq!(toks[0].0, Token::Comment);
    }

    // ====================================================================
    // JSON
    // ====================================================================

    #[test]
    fn json_key_vs_value() {
        let toks = tokens_of(r#"  "name": "Alice","#, Language::Json);
        // "name" should be key (Function), "Alice" should be value (String).
        assert!(has_token(&toks, Token::Function, "\"name\""));
        assert!(has_token(&toks, Token::String, "\"Alice\""));
    }

    #[test]
    fn json_number_and_bool() {
        let toks = tokens_of(r#"  "age": 42, "active": true"#, Language::Json);
        assert!(has_token(&toks, Token::Number, "42"));
        assert!(has_token(&toks, Token::Keyword, "true"));
    }

    #[test]
    fn json_null() {
        let toks = tokens_of(r#"  "val": null"#, Language::Json);
        assert!(has_token(&toks, Token::Keyword, "null"));
    }

    // ====================================================================
    // TOML
    // ====================================================================

    #[test]
    fn toml_section_header() {
        let toks = tokens_of("[dependencies]", Language::Toml);
        assert!(toks.iter().any(|t| t.0 == Token::Attribute));
    }

    #[test]
    fn toml_key_value() {
        let toks = tokens_of("name = \"editor\"", Language::Toml);
        assert!(toks.iter().any(|t| t.0 == Token::Function));
        assert!(has_token(&toks, Token::String, "\"editor\""));
    }

    #[test]
    fn toml_comment() {
        let toks = tokens_of("# a comment", Language::Toml);
        assert_eq!(toks[0].0, Token::Comment);
    }

    // ====================================================================
    // Markdown
    // ====================================================================

    #[test]
    fn markdown_heading() {
        let toks = tokens_of("# Hello World", Language::Markdown);
        assert_eq!(toks[0].0, Token::Heading);
    }

    #[test]
    fn markdown_bold() {
        let toks = tokens_of("some **bold** text", Language::Markdown);
        assert!(toks.iter().any(|t| t.0 == Token::Bold));
    }

    #[test]
    fn markdown_italic() {
        let toks = tokens_of("some *italic* text", Language::Markdown);
        assert!(toks.iter().any(|t| t.0 == Token::Italic));
    }

    #[test]
    fn markdown_link() {
        let toks = tokens_of("[text](https://example.com)", Language::Markdown);
        assert!(toks.iter().any(|t| t.0 == Token::Link));
    }

    #[test]
    fn markdown_code_fence() {
        let mut state = HighlightState::Normal;

        let line1 = "```rust";
        let toks1 = highlight_line(line1, Language::Markdown, &mut state);
        assert_eq!(toks1[0].kind, Token::CodeBlock);
        assert_eq!(state, HighlightState::CodeFence);

        let line2 = "let x = 1;";
        let toks2 = highlight_line(line2, Language::Markdown, &mut state);
        assert_eq!(toks2[0].kind, Token::String);
        assert_eq!(state, HighlightState::CodeFence);

        let line3 = "```";
        let toks3 = highlight_line(line3, Language::Markdown, &mut state);
        assert_eq!(toks3[0].kind, Token::CodeBlock);
        assert_eq!(state, HighlightState::Normal);
    }

    #[test]
    fn markdown_inline_code() {
        let toks = tokens_of("use `cargo build` to compile", Language::Markdown);
        assert!(has_token(&toks, Token::CodeBlock, "`cargo build`"));
    }

    #[test]
    fn markdown_list() {
        let toks = tokens_of("- item one", Language::Markdown);
        assert!(toks.iter().any(|t| t.0 == Token::Keyword));
    }

    // ====================================================================
    // Shell
    // ====================================================================

    #[test]
    fn shell_keywords() {
        let toks = tokens_of("if [ -f file ]; then", Language::Shell);
        assert!(has_token(&toks, Token::Keyword, "if"));
        assert!(has_token(&toks, Token::Keyword, "then"));
    }

    #[test]
    fn shell_variable() {
        let toks = tokens_of("echo $HOME", Language::Shell);
        assert!(has_token(&toks, Token::Variable, "$HOME"));
    }

    #[test]
    fn shell_variable_braces() {
        let toks = tokens_of("echo ${HOME}", Language::Shell);
        assert!(has_token(&toks, Token::Variable, "${HOME}"));
    }

    #[test]
    fn shell_string() {
        let toks = tokens_of("echo \"hello world\"", Language::Shell);
        assert!(has_token(&toks, Token::String, "\"hello world\""));
    }

    #[test]
    fn shell_comment() {
        let toks = tokens_of("# a comment", Language::Shell);
        assert_eq!(toks[0].0, Token::Comment);
    }

    #[test]
    fn shell_pipe() {
        let toks = tokens_of("cat file | grep pattern", Language::Shell);
        assert!(toks.iter().any(|t| t.0 == Token::Operator));
    }

    #[test]
    fn shell_builtins() {
        let toks = tokens_of("cd /home && echo done", Language::Shell);
        assert!(has_token(&toks, Token::Builtin, "cd"));
        assert!(has_token(&toks, Token::Builtin, "echo"));
    }

    // ====================================================================
    // Theme
    // ====================================================================

    #[test]
    fn theme_color_mapping() {
        let theme = Theme::catppuccin_mocha();
        assert_eq!(theme.color_for(Token::Keyword), Color::from_hex(0xCBA6F7));
        assert_eq!(theme.color_for(Token::String), Color::from_hex(0xA6E3A1));
        assert_eq!(theme.color_for(Token::Comment), Color::from_hex(0x6C7086));
        assert_eq!(theme.color_for(Token::Function), Color::from_hex(0x89B4FA));
        assert_eq!(theme.color_for(Token::Plain), Color::from_hex(0xCDD6F4));
    }

    // ====================================================================
    // Coverage: full-line token coverage
    // ====================================================================

    #[test]
    fn tokens_cover_entire_line() {
        // Verify that tokens span the entire line with no gaps or overlaps.
        let lines = &[
            ("fn main() { let x: u32 = 42; }", Language::Rust),
            ("def foo(x): return x + 1", Language::Python),
            ("int main(void) { return 0; }", Language::C),
            ("const x = () => { return 42; }", Language::JavaScript),
            (r#"{"key": "value", "n": 42}"#, Language::Json),
            ("name = \"editor\"  # comment", Language::Toml),
            ("# Heading", Language::Markdown),
            ("echo $HOME | grep foo", Language::Shell),
        ];

        for &(line, lang) in lines {
            let mut state = HighlightState::Normal;
            let toks = highlight_line(line, lang, &mut state);
            if toks.is_empty() && line.is_empty() {
                continue;
            }
            assert!(!toks.is_empty(), "no tokens for {:?}: {:?}", lang, line);
            assert_eq!(
                toks[0].start, 0,
                "first token doesn't start at 0 for {:?}: {:?}",
                lang, line
            );
            for pair in toks.windows(2) {
                assert_eq!(
                    pair[0].end, pair[1].start,
                    "gap or overlap between tokens for {:?}: {:?} -> {:?}",
                    lang, line, pair
                );
            }
            assert_eq!(
                toks.last().unwrap().end,
                line.len(),
                "last token doesn't end at line length for {:?}: {:?}",
                lang,
                line
            );
        }
    }
}
