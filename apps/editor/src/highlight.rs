//! Syntax highlighting engine for the SlateOS text editor.
//!
//! Provides token-based syntax highlighting for common programming languages.
//! Each language is implemented as a simple state machine that scans lines
//! character by character and emits styled tokens with byte-offset ranges.
//!
//! Multi-line constructs (block comments, triple-quoted strings, etc.) are
//! tracked via [`HighlightState`], which must be carried from one line to the
//! next during rendering.

#![allow(dead_code)]

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

/// Detect language from a filename (extension-based).
pub fn detect_language(filename: &str) -> Option<Language> {
    let ext = filename.rsplit('.').next()?;
    let lang = Language::from_extension(ext);
    if lang == Language::Plain {
        None
    } else {
        Some(lang)
    }
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

/// Advance `i` past the current byte and return the new position.
fn advance(bytes: &[u8], i: usize) -> usize {
    i + 1 + bytes.get(i).map_or(0, |b| match b {
        0x00..=0x7F => 0,
        0xC0..=0xDF => 1,
        0xE0..=0xEF => 2,
        0xF0..=0xFF => 3,
        _ => 0,
    })
}

/// Check if `bytes[i..]` starts with the given ASCII slice.
fn starts_with_at(bytes: &[u8], i: usize, needle: &[u8]) -> bool {
    bytes.get(i..i + needle.len()) == Some(needle)
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
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }
    // Safety: we only accepted ASCII bytes, so this is valid UTF-8.
    let word = std::str::from_utf8(&bytes[i..end]).unwrap_or("");
    (end, word)
}

/// Scan a number literal (int or float, with optional 0x/0o/0b prefix).
fn scan_number(bytes: &[u8], i: usize) -> usize {
    let mut end = i;
    // Hex/oct/bin prefix
    if end + 1 < bytes.len() && bytes[end] == b'0' {
        match bytes.get(end + 1) {
            Some(b'x' | b'X') => {
                end += 2;
                while end < bytes.len()
                    && (bytes[end].is_ascii_hexdigit() || bytes[end] == b'_')
                {
                    end += 1;
                }
                return end;
            }
            Some(b'o' | b'O') => {
                end += 2;
                while end < bytes.len()
                    && (bytes[end].is_ascii_digit() || bytes[end] == b'_')
                {
                    end += 1;
                }
                return end;
            }
            Some(b'b' | b'B') => {
                end += 2;
                while end < bytes.len()
                    && (bytes[end] == b'0' || bytes[end] == b'1' || bytes[end] == b'_')
                {
                    end += 1;
                }
                return end;
            }
            _ => {}
        }
    }
    // Decimal digits
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'_') {
        end += 1;
    }
    // Decimal point + fraction
    if end < bytes.len() && bytes[end] == b'.' {
        let after_dot = end + 1;
        if after_dot < bytes.len() && bytes[after_dot].is_ascii_digit() {
            end = after_dot;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'_') {
                end += 1;
            }
        }
    }
    // Exponent
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        let mut exp = end + 1;
        if exp < bytes.len() && (bytes[exp] == b'+' || bytes[exp] == b'-') {
            exp += 1;
        }
        if exp < bytes.len() && bytes[exp].is_ascii_digit() {
            end = exp;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'_') {
                end += 1;
            }
        }
    }
    // Type suffix (u8, i32, f64, usize, ...)
    if end < bytes.len() && bytes[end].is_ascii_alphabetic() {
        while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
            end += 1;
        }
    }
    end
}

/// Scan a string literal starting at `i` (which must point to the opening
/// quote character).  Returns the end offset (past the closing quote).
/// Handles `\"` escapes inside the string.
fn scan_string(bytes: &[u8], i: usize, quote: u8) -> usize {
    let mut end = i + 1; // skip the opening quote
    while end < bytes.len() {
        if bytes[end] == b'\\' {
            end += 2; // skip escaped character
        } else if bytes[end] == quote {
            end += 1; // include closing quote
            return end;
        } else {
            end += 1;
        }
    }
    end // unterminated — extends to end of line
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
            let start = 0;
            while i + 1 < len {
                if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    *depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    *depth -= 1;
                    i += 2;
                    if *depth == 0 {
                        push_token(&mut tokens, start, i, Token::Comment);
                        *state = HighlightState::Normal;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if *state != HighlightState::Normal {
                // Still inside block comment — consume the rest of the line.
                push_token(&mut tokens, start, len, Token::Comment);
                return tokens;
            }
        }
        HighlightState::MultiLineString {
            delimiter: StringDelimiter::RustRaw { hashes },
        } => {
            let needed = *hashes;
            let start = 0;
            while i < len {
                if bytes[i] == b'"' {
                    let mut h = 0;
                    while i + 1 + h < len && bytes[i + 1 + h] == b'#' && h < needed {
                        h += 1;
                    }
                    if h == needed {
                        i += 1 + needed;
                        push_token(&mut tokens, start, i, Token::String);
                        *state = HighlightState::Normal;
                        break;
                    }
                }
                i += 1;
            }
            if *state != HighlightState::Normal {
                push_token(&mut tokens, start, len, Token::String);
                return tokens;
            }
        }
        _ => {}
    }

    while i < len {
        let b = bytes[i];

        // Line comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Block comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            let mut depth: usize = 1;
            i += 2;
            while i + 1 < len {
                if bytes[i] == b'/' && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if depth > 0 {
                // Consume last byte if we stopped due to `i + 1 >= len`.
                i = len;
                *state = HighlightState::BlockComment { depth };
            }
            push_token(&mut tokens, start, i, Token::Comment);
            continue;
        }

        // Attribute: `#[...]` or `#![...]`
        if b == b'#' && i + 1 < len && (bytes[i + 1] == b'[' || (bytes[i + 1] == b'!' && i + 2 < len && bytes[i + 2] == b'[')) {
            let start = i;
            // Find matching `]`
            let mut bracket_depth = 0usize;
            while i < len {
                if bytes[i] == b'[' {
                    bracket_depth += 1;
                } else if bytes[i] == b']' {
                    bracket_depth -= 1;
                    if bracket_depth == 0 {
                        i += 1;
                        break;
                    }
                }
                i += 1;
            }
            push_token(&mut tokens, start, i, Token::Attribute);
            continue;
        }

        // Raw string: r"...", r#"..."#, r##"..."##, etc.
        if b == b'r' && i + 1 < len && (bytes[i + 1] == b'"' || bytes[i + 1] == b'#') {
            let start = i;
            let mut hashes = 0usize;
            let mut j = i + 1;
            while j < len && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < len && bytes[j] == b'"' {
                // It's a raw string.
                j += 1; // past opening quote
                loop {
                    if j >= len {
                        // Multi-line raw string.
                        *state = HighlightState::MultiLineString {
                            delimiter: StringDelimiter::RustRaw { hashes },
                        };
                        push_token(&mut tokens, start, len, Token::String);
                        return tokens;
                    }
                    if bytes[j] == b'"' {
                        let mut h = 0;
                        while j + 1 + h < len && bytes[j + 1 + h] == b'#' && h < hashes {
                            h += 1;
                        }
                        if h == hashes {
                            j += 1 + hashes;
                            break;
                        }
                    }
                    j += 1;
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
                let mut j = i + 1;
                if j < len && (bytes[j].is_ascii_alphabetic() || bytes[j] == b'_') {
                    // Could be a char literal like 'a' or a lifetime like 'a.
                    let mut k = j;
                    while k < len && is_ident_byte(bytes[k]) {
                        k += 1;
                    }
                    if k < len && bytes[k] == b'\'' {
                        // Char literal.
                        i = k + 1;
                        push_token(&mut tokens, start, i, Token::String);
                        continue;
                    }
                    // Lifetime.
                    push_token(&mut tokens, start, k, Token::Attribute);
                    i = k;
                    continue;
                }
                // Escaped char literal: '\n', '\\'
                if j < len && bytes[j] == b'\\' {
                    j += 1; // skip escape marker
                    if j < len {
                        j += 1; // skip escaped char
                    }
                    if j < len && bytes[j] == b'\'' {
                        j += 1;
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
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
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
            if end < len && bytes[end] == b'!' && !word.is_empty() {
                let kind = Token::Macro;
                push_token(&mut tokens, start, end + 1, kind);
                i = end + 1;
                continue;
            }
            let kind = if is_keyword(word, RUST_KEYWORDS) {
                Token::Keyword
            } else if is_keyword(word, RUST_TYPES) {
                Token::Type
            } else if end < len && bytes[end] == b'(' {
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
            while i < len && is_operator_byte(bytes[i]) {
                i += 1;
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
    let mut i = 0;

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
        let start = 0;
        while i + 2 < len {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if starts_with_at(bytes, i, needle) {
                i += 3;
                push_token(&mut tokens, start, i, Token::String);
                *state = HighlightState::Normal;
                // Continue highlighting the rest of the line.
                highlight_python_normal(line, i, &mut tokens, state);
                return tokens;
            }
            i += 1;
        }
        // Didn't find closing — rest of line is string.
        push_token(&mut tokens, start, len, Token::String);
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

    while i < len {
        let b = bytes[i];

        // Comment
        if b == b'#' {
            push_token(tokens, i, len, Token::Comment);
            return Vec::new();
        }

        // Decorator
        if b == b'@' {
            let start = i;
            i += 1;
            while i < len && (is_ident_byte(bytes[i]) || bytes[i] == b'.') {
                i += 1;
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
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
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
            } else if end < len && bytes[end] == b'(' {
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
            while i < len && is_operator_byte(bytes[i]) {
                i += 1;
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
        let start = 0;
        while i + 1 < len {
            if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                i += 2;
                push_token(&mut tokens, start, i, Token::Comment);
                *state = HighlightState::Normal;
                break;
            }
            i += 1;
        }
        if *state != HighlightState::Normal {
            push_token(&mut tokens, start, len, Token::Comment);
            return tokens;
        }
    }

    // Preprocessor directive — if first non-whitespace is `#`.
    // Only check when we haven't already consumed a block comment prefix.
    if i == 0 {
        let trimmed_start = bytes.iter().position(|&b| b != b' ' && b != b'\t');
        if let Some(ts) = trimmed_start
            && bytes[ts] == b'#' {
                push_token(&mut tokens, 0, len, Token::Preprocessor);
                return tokens;
            }
    }

    while i < len {
        let b = bytes[i];

        // Line comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Block comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    push_token(&mut tokens, start, i, Token::Comment);
                    break;
                }
                i += 1;
            }
            // Check if comment was closed.
            if i + 1 >= len && !(i >= 2 && bytes[i - 2] == b'*' && bytes[i - 1] == b'/') {
                i = len;
                *state = HighlightState::BlockComment { depth: 1 };
                push_token(&mut tokens, start, len, Token::Comment);
            }
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
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
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
            } else if end < len && bytes[end] == b'(' {
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
            while i < len && is_operator_byte(bytes[i]) {
                i += 1;
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
        let start = 0;
        while i + 1 < len {
            if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                i += 2;
                push_token(&mut tokens, start, i, Token::Comment);
                *state = HighlightState::Normal;
                break;
            }
            i += 1;
        }
        if *state != HighlightState::Normal {
            push_token(&mut tokens, start, len, Token::Comment);
            return tokens;
        }
    }

    // Continue template literal from previous line.
    if let HighlightState::MultiLineString {
        delimiter: StringDelimiter::Backtick,
    } = state
    {
        let start = 0;
        while i < len {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'`' {
                i += 1;
                push_token(&mut tokens, start, i, Token::String);
                *state = HighlightState::Normal;
                break;
            }
            i += 1;
        }
        if *state != HighlightState::Normal {
            push_token(&mut tokens, start, len, Token::String);
            return tokens;
        }
    }

    while i < len {
        let b = bytes[i];

        // Line comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            push_token(&mut tokens, i, len, Token::Comment);
            return tokens;
        }

        // Block comment
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            let mut closed = false;
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                i = len;
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
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            let start = i;
            let end = scan_number(bytes, i);
            push_token(&mut tokens, start, end, Token::Number);
            i = end;
            continue;
        }

        // Identifier
        if b.is_ascii_alphabetic() || b == b'_' || b == b'$' {
            let start = i;
            while i < len && (is_ident_byte(bytes[i]) || bytes[i] == b'$') {
                i += 1;
            }
            let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
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
            while i < len && is_operator_byte(bytes[i]) {
                i += 1;
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
    while i < len {
        let b = bytes[i];

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
    let _ = state;
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    // Skip leading whitespace.
    while i < len && (bytes[i] == b' ' || bytes[i] == b'\t') {
        i += 1;
    }
    if i > 0 {
        push_token(&mut tokens, 0, i, Token::Plain);
    }

    if i >= len {
        return tokens;
    }

    // Comment line
    if bytes[i] == b'#' {
        push_token(&mut tokens, i, len, Token::Comment);
        return tokens;
    }

    // Section header: `[section]` or `[[array]]`
    if bytes[i] == b'[' {
        push_token(&mut tokens, i, len, Token::Attribute);
        return tokens;
    }

    // Key = value
    // Scan key (everything up to `=`).
    let key_start = i;
    while i < len && bytes[i] != b'=' && bytes[i] != b'#' {
        i += 1;
    }
    if i < len && bytes[i] == b'=' {
        // Key
        push_token(&mut tokens, key_start, i, Token::Function);
        // Equals sign
        push_token(&mut tokens, i, i + 1, Token::Operator);
        i += 1;

        // Value — highlight strings, numbers, booleans.
        while i < len {
            let b = bytes[i];

            if b == b'#' {
                push_token(&mut tokens, i, len, Token::Comment);
                return tokens;
            }

            if b == b'"' {
                // Triple-quoted string
                if i + 2 < len && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                    let start = i;
                    i += 3;
                    while i + 2 < len {
                        if bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                            i += 3;
                            push_token(&mut tokens, start, i, Token::String);
                            break;
                        }
                        i += 1;
                    }
                    if i > start {
                        push_token(&mut tokens, start, i.min(len), Token::String);
                    }
                    continue;
                }
                let start = i;
                let end = scan_string(bytes, i, b'"');
                push_token(&mut tokens, start, end, Token::String);
                i = end;
                continue;
            }

            if b == b'\'' {
                let start = i;
                let end = scan_string(bytes, i, b'\'');
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
    while i < len {
        let b = bytes[i];

        // Inline code: `...`
        if b == b'`' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'`' {
                i += 1;
            }
            if i < len {
                i += 1; // include closing backtick
            }
            push_token(&mut tokens, start, i, Token::CodeBlock);
            continue;
        }

        // Bold: **...**
        if b == b'*' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len {
                if bytes[i] == b'*' && bytes[i + 1] == b'*' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            push_token(&mut tokens, start, i, Token::Bold);
            continue;
        }

        // Italic: *...*
        if b == b'*' {
            let start = i;
            i += 1;
            while i < len && bytes[i] != b'*' {
                i += 1;
            }
            if i < len {
                i += 1;
            }
            push_token(&mut tokens, start, i, Token::Italic);
            continue;
        }

        // Link: [text](url)
        if b == b'[' {
            let start = i;
            i += 1;
            // Find `](`
            let mut found_bracket = false;
            while i < len {
                if bytes[i] == b']' && i + 1 < len && bytes[i + 1] == b'(' {
                    found_bracket = true;
                    i += 2;
                    // Find closing `)`
                    while i < len && bytes[i] != b')' {
                        i += 1;
                    }
                    if i < len {
                        i += 1;
                    }
                    break;
                }
                i += 1;
            }
            if found_bracket {
                push_token(&mut tokens, start, i, Token::Link);
            } else {
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

    while i < len {
        let b = bytes[i];

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
            while i < len && (is_ident_byte(bytes[i]) || bytes[i] == b'-') {
                i += 1;
            }
            let word = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
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
    // Language detection
    // ====================================================================

    #[test]
    fn detect_language_from_filename() {
        assert_eq!(detect_language("main.rs"), Some(Language::Rust));
        assert_eq!(detect_language("script.py"), Some(Language::Python));
        assert_eq!(detect_language("app.js"), Some(Language::JavaScript));
        assert_eq!(detect_language("app.ts"), Some(Language::JavaScript));
        assert_eq!(detect_language("lib.c"), Some(Language::C));
        assert_eq!(detect_language("lib.cpp"), Some(Language::C));
        assert_eq!(detect_language("lib.h"), Some(Language::C));
        assert_eq!(detect_language("config.json"), Some(Language::Json));
        assert_eq!(detect_language("Cargo.toml"), Some(Language::Toml));
        assert_eq!(detect_language("README.md"), Some(Language::Markdown));
        assert_eq!(detect_language("run.sh"), Some(Language::Shell));
        assert_eq!(detect_language("file.txt"), None);
        assert_eq!(detect_language("noext"), None);
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
