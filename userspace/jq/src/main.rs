//! jq — OurOS JSON processor
//!
//! A subset of jq's functionality: JSON parsing, pretty printing, and
//! filter expressions for querying and transforming JSON data.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read as _, Write as _};

// ============================================================================
// Output helpers
// ============================================================================
//
// These route through std, which reaches the native OurOS write syscall via
// the posix libc layer.  (Previously they hand-rolled `syscall(1, ...)` using
// the Linux write number — but native syscall 1 is SYS_EXIT, so every write
// terminated the process.)

fn write_stdout(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}

fn write_stderr(s: &str) {
    let _ = io::stderr().write_all(s.as_bytes());
}

// ============================================================================
// JSON Value
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    fn is_truthy(&self) -> bool {
        !matches!(self, Value::Null | Value::Bool(false))
    }

    fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }

    fn length(&self) -> Value {
        match self {
            Value::Null => Value::Number(0.0),
            Value::Bool(_) => Value::Null,
            Value::Number(_) => Value::Null,
            Value::String(s) => Value::Number(s.len() as f64),
            Value::Array(a) => Value::Number(a.len() as f64),
            Value::Object(o) => Value::Number(o.len() as f64),
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
            (Value::String(a), Value::String(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}

// ============================================================================
// JSON Parser
// ============================================================================

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input: input.as_bytes(), pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() {
            match self.input[self.pos] {
                b' ' | b'\t' | b'\n' | b'\r' => self.pos += 1,
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn expect(&mut self, ch: u8) -> Result<(), String> {
        self.skip_ws();
        match self.advance() {
            Some(b) if b == ch => Ok(()),
            Some(b) => Err(format!("expected '{}', got '{}'", ch as char, b as char)),
            None => Err(format!("expected '{}', got EOF", ch as char)),
        }
    }

    fn parse_value(&mut self) -> Result<Value, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'"') => self.parse_string().map(Value::String),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_literal("true", Value::Bool(true)),
            Some(b'f') => self.parse_literal("false", Value::Bool(false)),
            Some(b'n') => self.parse_literal("null", Value::Null),
            Some(b) if b == b'-' || b.is_ascii_digit() => self.parse_number(),
            Some(b) => Err(format!("unexpected character: '{}'", b as char)),
            None => Err("unexpected EOF".into()),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(b'"') => return Ok(s),
                Some(b'\\') => {
                    match self.advance() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b't') => s.push('\t'),
                        Some(b'r') => s.push('\r'),
                        Some(b'b') => s.push('\u{0008}'),
                        Some(b'f') => s.push('\u{000C}'),
                        Some(b'u') => {
                            let hex = self.parse_hex4()?;
                            if let Some(ch) = char::from_u32(hex as u32) {
                                s.push(ch);
                            } else if (0xD800..=0xDBFF).contains(&hex) {
                                // High surrogate — expect \uXXXX low surrogate
                                if self.advance() == Some(b'\\') && self.advance() == Some(b'u') {
                                    let low = self.parse_hex4()?;
                                    if (0xDC00..=0xDFFF).contains(&low) {
                                        let cp = 0x10000 + ((hex as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
                                        if let Some(ch) = char::from_u32(cp) {
                                            s.push(ch);
                                        } else {
                                            s.push('\u{FFFD}');
                                        }
                                    } else {
                                        s.push('\u{FFFD}');
                                    }
                                } else {
                                    s.push('\u{FFFD}');
                                }
                            } else {
                                s.push('\u{FFFD}');
                            }
                        }
                        Some(b) => return Err(format!("invalid escape: \\{}", b as char)),
                        None => return Err("unterminated string escape".into()),
                    }
                }
                Some(b) => {
                    // Handle UTF-8 multi-byte
                    if b < 0x80 {
                        s.push(b as char);
                    } else {
                        // Collect UTF-8 bytes
                        let start = self.pos - 1;
                        let width = if b & 0xE0 == 0xC0 { 2 }
                            else if b & 0xF0 == 0xE0 { 3 }
                            else if b & 0xF8 == 0xF0 { 4 }
                            else { 1 };
                        let end = (start + width).min(self.input.len());
                        self.pos = end;
                        if let Ok(utf8) = std::str::from_utf8(&self.input[start..end]) {
                            s.push_str(utf8);
                        } else {
                            s.push('\u{FFFD}');
                        }
                    }
                }
                None => return Err("unterminated string".into()),
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u16, String> {
        let mut val: u16 = 0;
        for _ in 0..4 {
            let b = self.advance().ok_or("incomplete \\uXXXX")?;
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(format!("invalid hex digit: '{}'", b as char)),
            };
            val = val.checked_mul(16).ok_or("hex overflow")?.checked_add(digit as u16).ok_or("hex overflow")?;
        }
        Ok(val)
    }

    fn parse_number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        // Integer part
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else {
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err("invalid number".into());
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        // Fractional part
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err("expected digit after decimal point".into());
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        // Exponent
        if self.peek() == Some(b'e') || self.peek() == Some(b'E') {
            self.pos += 1;
            if self.peek() == Some(b'+') || self.peek() == Some(b'-') {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err("expected digit in exponent".into());
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos]).map_err(|e| e.to_string())?;
        let n: f64 = s.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
        Ok(Value::Number(n))
    }

    fn parse_array(&mut self) -> Result<Value, String> {
        self.expect(b'[')?;
        self.skip_ws();
        let mut arr = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(arr));
        }
        loop {
            arr.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b']') => { self.pos += 1; return Ok(Value::Array(arr)); }
                _ => return Err("expected ',' or ']' in array".into()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, String> {
        self.expect(b'{')?;
        self.skip_ws();
        let mut map = BTreeMap::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.expect(b':')?;
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b'}') => { self.pos += 1; return Ok(Value::Object(map)); }
                _ => return Err("expected ',' or '}' in object".into()),
            }
        }
    }

    fn parse_literal(&mut self, expected: &str, value: Value) -> Result<Value, String> {
        for b in expected.bytes() {
            match self.advance() {
                Some(got) if got == b => {}
                _ => return Err(format!("expected '{}'", expected)),
            }
        }
        Ok(value)
    }
}

fn parse_json(input: &str) -> Result<Value, String> {
    let mut parser = Parser::new(input);
    let val = parser.parse_value()?;
    parser.skip_ws();
    if parser.pos < parser.input.len() {
        return Err(format!("trailing data at position {}", parser.pos));
    }
    Ok(val)
}

// ============================================================================
// JSON Printer
// ============================================================================

fn format_json(val: &Value, compact: bool, indent: usize) -> String {
    let mut out = String::new();
    format_value(val, &mut out, compact, indent, 0);
    out
}

fn format_value(val: &Value, out: &mut String, compact: bool, indent: usize, depth: usize) {
    match val {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if n.fract() == 0.0 && n.abs() < 1e15 {
                let _ = fmt::Write::write_fmt(out, format_args!("{}", *n as i64));
            } else {
                let _ = fmt::Write::write_fmt(out, format_args!("{}", n));
            }
        }
        Value::String(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    '\u{0008}' => out.push_str("\\b"),
                    '\u{000C}' => out.push_str("\\f"),
                    c if c < ' ' => {
                        let _ = fmt::Write::write_fmt(out, format_args!("\\u{:04x}", c as u32));
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            if !compact { out.push('\n'); }
            for (i, v) in arr.iter().enumerate() {
                if !compact {
                    for _ in 0..(depth + 1) * indent { out.push(' '); }
                }
                format_value(v, out, compact, indent, depth + 1);
                if i + 1 < arr.len() {
                    out.push(',');
                }
                if !compact { out.push('\n'); }
            }
            if !compact {
                for _ in 0..depth * indent { out.push(' '); }
            }
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            if !compact { out.push('\n'); }
            let len = map.len();
            for (i, (k, v)) in map.iter().enumerate() {
                if !compact {
                    for _ in 0..(depth + 1) * indent { out.push(' '); }
                }
                out.push('"');
                out.push_str(k);
                out.push('"');
                out.push(':');
                if !compact { out.push(' '); }
                format_value(v, out, compact, indent, depth + 1);
                if i + 1 < len {
                    out.push(',');
                }
                if !compact { out.push('\n'); }
            }
            if !compact {
                for _ in 0..depth * indent { out.push(' '); }
            }
            out.push('}');
        }
    }
}

// ============================================================================
// Filter AST
// ============================================================================

/// Value-type class used by the type-selection builtins (`numbers`, `strings`,
/// `arrays`, etc.), each of which keeps only inputs matching the class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeClass {
    Numbers,
    Strings,
    Booleans,
    Arrays,
    Objects,
    Nulls,
    Iterables, // arrays and objects
    Scalars,   // anything except arrays and objects
}

#[derive(Clone, Debug)]
enum Filter {
    Identity,                           // .
    Field(String),                      // .foo
    Index(i64),                         // .[N]
    Iterate,                            // .[]
    Pipe(Box<Filter>, Box<Filter>),     // f | g
    Comma(Box<Filter>, Box<Filter>),    // f, g
    Literal(Value),                     // null, true, 42, "str"
    Length,                             // length
    Keys,                               // keys
    Values,                             // values
    TypeF,                              // type
    Not,                                // not
    Add,                                // add
    Sort,                               // sort
    Unique,                             // unique
    Flatten,                            // flatten
    First,                              // first
    Last,                               // last
    Reverse,                            // reverse
    Empty,                              // empty
    Select(Box<Filter>),                // select(f)
    Map(Box<Filter>),                   // map(f)
    SortBy(Box<Filter>),               // sort_by(f)
    GroupBy(Box<Filter>),              // group_by(f)
    UniqueBy(Box<Filter>),             // unique_by(f)
    Limit(usize, Box<Filter>),         // limit(n; f)
    Has(String),                        // has("key")
    Contains(Box<Filter>),             // contains(f)
    ToEntries,                          // to_entries
    FromEntries,                        // from_entries
    Ascii,                              // ascii_downcase / ascii_upcase
    AsciiDown,
    AsciiUp,
    Compare(CmpOp, Box<Filter>, Box<Filter>),  // ==, !=, <, >, <=, >=
    Arith(ArithOp, Box<Filter>, Box<Filter>),   // +, -, *, /, %
    And(Box<Filter>, Box<Filter>),      // and
    Or(Box<Filter>, Box<Filter>),       // or
    IfThenElse(Box<Filter>, Box<Filter>, Option<Box<Filter>>), // if f then g (else h) end
    TryCatch(Box<Filter>),             // f?
    ObjConstruct(Vec<(String, Filter)>), // {key: expr, ...}
    ArrConstruct(Box<Filter>),          // [f]
    Recurse,                            // ..
    TypeSelect(TypeClass),              // numbers/strings/booleans/arrays/objects/nulls/values/iterables/scalars
    Env,                                // env
    Null,                               // null literal filter
    Input,                              // input
    Debug,                              // debug
    Def(String, Vec<String>, Box<Filter>, Box<Filter>), // def name(args): body; rest
    FuncCall(String, Vec<Filter>),       // user-defined function call
    StringInterp(Vec<StringPart>),       // string with \(expr) interpolations
    Optional(Box<Filter>),              // f?
    Assign(Box<Filter>, Box<Filter>),   // .field = expr (update)
    UpdateAssign(Box<Filter>, Box<Filter>), // .field |= expr
    Label(String, Box<Filter>),         // label $name | f
    Range(Box<Filter>, Box<Filter>),    // range(a; b)
    Split(String),                      // split("delim")
    Join(String),                       // join("delim")
    Test(String),                       // test("regex") — simple substring match
    Ltrimstr(String),                   // ltrimstr("prefix")
    Rtrimstr(String),                   // rtrimstr("suffix")
    Startswith(String),                 // startswith("prefix")
    Endswith(String),                   // endswith("suffix")
    Tostring,                           // tostring
    Tonumber,                           // tonumber
    Floor,                              // floor
    Ceil,                               // ceil
    Round,                              // round
    Fabs,                               // fabs
    Sqrt,                               // sqrt
    Indices(String),                    // indices("str")
    Inside(Box<Filter>),               // inside(f)
    Limit2(Box<Filter>, Box<Filter>),  // limit(n; f) — two-arg
    Any(Option<Box<Filter>>),           // any / any(f)
    All(Option<Box<Filter>>),           // all / all(f)
    MinMax(bool),                       // min / max (bool=true for max)
    MinMaxBy(bool, Box<Filter>),       // min_by(f) / max_by(f)
    Paths,                              // paths / leaf_paths
    GetPath(Box<Filter>),              // getpath(f)
    Delpaths(Box<Filter>),             // delpaths(f)
    Ascii2(bool),                       // ascii_downcase(false) / ascii_upcase(true)
    Explode,                            // explode
    Implode,                            // implode
    Tojson,                             // tojson
    Fromjson,                           // fromjson
    Format(FormatKind),                 // @base64, @csv, @tsv, @html, @uri, @json, @text
}

#[derive(Clone, Debug)]
enum StringPart {
    Lit(String),
    Expr(Filter),
}

#[derive(Clone, Debug)]
enum CmpOp { Eq, Ne, Lt, Gt, Le, Ge }

#[derive(Clone, Debug)]
enum ArithOp { Add, Sub, Mul, Div, Mod }

#[derive(Clone, Debug)]
enum FormatKind { Base64, Base64d, Csv, Tsv, Html, Uri, Json, Text }

// ============================================================================
// Filter Parser
// ============================================================================

struct FilterParser<'a> {
    tokens: Vec<Token>,
    pos: usize,
    _phantom: std::marker::PhantomData<&'a ()>,
}

#[derive(Clone, Debug)]
enum Token {
    Dot,
    Pipe,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    Semi,
    Question,
    DotDot,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Ident(String),
    Str(String),
    Num(f64),
    At(String),
    And,
    Or,
    Not,
    If,
    Then,
    Else,
    End,
    Def,
    As,
    Label,
    True,
    False,
    NullTok,
}

fn tokenize_filter(input: &str) -> Result<Vec<Token>, String> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => { i += 1; }
            b'.' if i + 1 < bytes.len() && bytes[i + 1] == b'.' => { tokens.push(Token::DotDot); i += 2; }
            b'.' => { tokens.push(Token::Dot); i += 1; }
            b'|' => { tokens.push(Token::Pipe); i += 1; }
            b',' => { tokens.push(Token::Comma); i += 1; }
            b'(' => { tokens.push(Token::LParen); i += 1; }
            b')' => { tokens.push(Token::RParen); i += 1; }
            b'[' => { tokens.push(Token::LBracket); i += 1; }
            b']' => { tokens.push(Token::RBracket); i += 1; }
            b'{' => { tokens.push(Token::LBrace); i += 1; }
            b'}' => { tokens.push(Token::RBrace); i += 1; }
            b':' => { tokens.push(Token::Colon); i += 1; }
            b';' => { tokens.push(Token::Semi); i += 1; }
            b'?' => { tokens.push(Token::Question); i += 1; }
            b'+' => { tokens.push(Token::Plus); i += 1; }
            b'-' if i + 1 < bytes.len() && bytes[i+1].is_ascii_digit() && (tokens.is_empty() || matches!(tokens.last(), Some(Token::LParen | Token::LBracket | Token::Pipe | Token::Comma | Token::Semi | Token::Colon))) => {
                let start = i;
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') { i += 1; }
                let s = std::str::from_utf8(&bytes[start..i]).map_err(|e| e.to_string())?;
                let n: f64 = s.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
                tokens.push(Token::Num(n));
            }
            b'-' => { tokens.push(Token::Minus); i += 1; }
            b'*' => { tokens.push(Token::Star); i += 1; }
            b'/' if i + 1 < bytes.len() && bytes[i+1] == b'/' => {
                // Comment — skip to end of line
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            }
            b'/' => { tokens.push(Token::Slash); i += 1; }
            b'%' => { tokens.push(Token::Percent); i += 1; }
            b'=' if i + 1 < bytes.len() && bytes[i+1] == b'=' => { tokens.push(Token::Eq); i += 2; }
            b'!' if i + 1 < bytes.len() && bytes[i+1] == b'=' => { tokens.push(Token::Ne); i += 2; }
            b'<' if i + 1 < bytes.len() && bytes[i+1] == b'=' => { tokens.push(Token::Le); i += 2; }
            b'>' if i + 1 < bytes.len() && bytes[i+1] == b'=' => { tokens.push(Token::Ge); i += 2; }
            b'<' => { tokens.push(Token::Lt); i += 1; }
            b'>' => { tokens.push(Token::Gt); i += 1; }
            b'@' => {
                i += 1;
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                let name = std::str::from_utf8(&bytes[start..i]).map_err(|e| e.to_string())?.to_string();
                tokens.push(Token::At(name));
            }
            b'"' => {
                i += 1;
                let mut s = String::new();
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                        match bytes[i] {
                            b'n' => s.push('\n'),
                            b't' => s.push('\t'),
                            b'\\' => s.push('\\'),
                            b'"' => s.push('"'),
                            b'/' => s.push('/'),
                            b'r' => s.push('\r'),
                            other => { s.push('\\'); s.push(other as char); }
                        }
                    } else {
                        s.push(bytes[i] as char);
                    }
                    i += 1;
                }
                if i < bytes.len() { i += 1; } // skip closing "
                tokens.push(Token::Str(s));
            }
            b'0'..=b'9' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'e' || bytes[i] == b'E') { i += 1; }
                let s = std::str::from_utf8(&bytes[start..i]).map_err(|e| e.to_string())?;
                let n: f64 = s.parse().map_err(|e: std::num::ParseFloatError| e.to_string())?;
                tokens.push(Token::Num(n));
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
                let word = std::str::from_utf8(&bytes[start..i]).map_err(|e| e.to_string())?;
                let tok = match word {
                    "and" => Token::And,
                    "or" => Token::Or,
                    "not" => Token::Not,
                    "if" => Token::If,
                    "then" => Token::Then,
                    "else" => Token::Else,
                    "end" => Token::End,
                    "def" => Token::Def,
                    "as" => Token::As,
                    "label" => Token::Label,
                    "true" => Token::True,
                    "false" => Token::False,
                    "null" => Token::NullTok,
                    _ => Token::Ident(word.to_string()),
                };
                tokens.push(tok);
            }
            b'#' => {
                // Comment
                while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            }
            other => return Err(format!("unexpected character in filter: '{}'", other as char)),
        }
    }
    Ok(tokens)
}

impl<'a> FilterParser<'a> {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, _phantom: std::marker::PhantomData }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos)?.clone();
        self.pos += 1;
        Some(t)
    }

    fn expect_token(&mut self, desc: &str) -> Result<Token, String> {
        self.advance().ok_or_else(|| format!("expected {}, got EOF", desc))
    }

    fn parse(&mut self) -> Result<Filter, String> {
        self.parse_pipe()
    }

    fn parse_pipe(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_comma()?;
        while matches!(self.peek(), Some(Token::Pipe)) {
            self.advance();
            let right = self.parse_comma()?;
            left = Filter::Pipe(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_comma(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_or()?;
        while matches!(self.peek(), Some(Token::Comma)) {
            self.advance();
            let right = self.parse_or()?;
            left = Filter::Comma(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// Parse an object-construction value: a `|` pipe chain whose operands stop
    /// before a top-level `,` (which separates object entries) so that
    /// `{a: .x, b: .y}` is read as two entries rather than one comma expression.
    fn parse_obj_value(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_or()?;
        while matches!(self.peek(), Some(Token::Pipe)) {
            self.advance();
            let right = self.parse_or()?;
            left = Filter::Pipe(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.advance();
            let right = self.parse_and()?;
            left = Filter::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_compare()?;
        while matches!(self.peek(), Some(Token::And)) {
            self.advance();
            let right = self.parse_compare()?;
            left = Filter::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_compare(&mut self) -> Result<Filter, String> {
        let left = self.parse_addition()?;
        let op = match self.peek() {
            Some(Token::Eq) => CmpOp::Eq,
            Some(Token::Ne) => CmpOp::Ne,
            Some(Token::Lt) => CmpOp::Lt,
            Some(Token::Gt) => CmpOp::Gt,
            Some(Token::Le) => CmpOp::Le,
            Some(Token::Ge) => CmpOp::Ge,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_addition()?;
        Ok(Filter::Compare(op, Box::new(left), Box::new(right)))
    }

    fn parse_addition(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_multiply()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.advance();
                    let right = self.parse_multiply()?;
                    left = Filter::Arith(ArithOp::Add, Box::new(left), Box::new(right));
                }
                Some(Token::Minus) => {
                    self.advance();
                    let right = self.parse_multiply()?;
                    left = Filter::Arith(ArithOp::Sub, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_multiply(&mut self) -> Result<Filter, String> {
        let mut left = self.parse_postfix()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.advance();
                    let right = self.parse_postfix()?;
                    left = Filter::Arith(ArithOp::Mul, Box::new(left), Box::new(right));
                }
                Some(Token::Slash) => {
                    self.advance();
                    let right = self.parse_postfix()?;
                    left = Filter::Arith(ArithOp::Div, Box::new(left), Box::new(right));
                }
                Some(Token::Percent) => {
                    self.advance();
                    let right = self.parse_postfix()?;
                    left = Filter::Arith(ArithOp::Mod, Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_postfix(&mut self) -> Result<Filter, String> {
        let mut f = self.parse_primary()?;
        loop {
            match self.peek() {
                Some(Token::LBracket) => {
                    self.advance();
                    if matches!(self.peek(), Some(Token::RBracket)) {
                        self.advance();
                        f = Filter::Pipe(Box::new(f), Box::new(Filter::Iterate));
                    } else {
                        let idx = self.parse_pipe()?;
                        if !matches!(self.peek(), Some(Token::RBracket)) {
                            return Err("expected ']'".into());
                        }
                        self.advance();
                        // If idx is a literal number, use Index
                        if let Filter::Literal(Value::Number(n)) = &idx {
                            f = Filter::Pipe(Box::new(f), Box::new(Filter::Index(*n as i64)));
                        } else {
                            // Dynamic index — treat as pipe to the index expression
                            f = Filter::Pipe(Box::new(f), Box::new(idx));
                        }
                    }
                }
                Some(Token::Dot) if self.pos + 1 < self.tokens.len() => {
                    if let Some(Token::Ident(_)) = self.tokens.get(self.pos + 1) {
                        self.advance(); // consume .
                        if let Some(Token::Ident(name)) = self.advance() {
                            f = Filter::Pipe(Box::new(f), Box::new(Filter::Field(name)));
                        }
                    } else {
                        break;
                    }
                }
                Some(Token::Question) => {
                    self.advance();
                    f = Filter::TryCatch(Box::new(f));
                }
                _ => break,
            }
        }
        Ok(f)
    }

    fn parse_primary(&mut self) -> Result<Filter, String> {
        match self.peek().cloned() {
            Some(Token::Dot) => {
                self.advance();
                // Check for field access .foo or .[
                match self.peek() {
                    Some(Token::Ident(_)) => {
                        if let Some(Token::Ident(name)) = self.advance() {
                            Ok(Filter::Field(name))
                        } else {
                            Ok(Filter::Identity)
                        }
                    }
                    Some(Token::LBracket) => {
                        self.advance();
                        if matches!(self.peek(), Some(Token::RBracket)) {
                            self.advance();
                            Ok(Filter::Iterate)
                        } else {
                            let idx_filter = self.parse_pipe()?;
                            if !matches!(self.peek(), Some(Token::RBracket)) {
                                return Err("expected ']'".into());
                            }
                            self.advance();
                            if let Filter::Literal(Value::Number(n)) = &idx_filter {
                                Ok(Filter::Index(*n as i64))
                            } else {
                                Ok(idx_filter)
                            }
                        }
                    }
                    Some(Token::Str(_)) => {
                        if let Some(Token::Str(s)) = self.advance() {
                            Ok(Filter::Field(s))
                        } else {
                            Ok(Filter::Identity)
                        }
                    }
                    _ => Ok(Filter::Identity),
                }
            }
            Some(Token::DotDot) => { self.advance(); Ok(Filter::Recurse) }
            Some(Token::Num(n)) => { self.advance(); Ok(Filter::Literal(Value::Number(n))) }
            Some(Token::Str(s)) => { self.advance(); Ok(Filter::Literal(Value::String(s))) }
            Some(Token::True) => { self.advance(); Ok(Filter::Literal(Value::Bool(true))) }
            Some(Token::False) => { self.advance(); Ok(Filter::Literal(Value::Bool(false))) }
            Some(Token::NullTok) => { self.advance(); Ok(Filter::Literal(Value::Null)) }
            Some(Token::Not) => { self.advance(); Ok(Filter::Not) }
            Some(Token::LParen) => {
                self.advance();
                let f = self.parse_pipe()?;
                if !matches!(self.peek(), Some(Token::RParen)) {
                    return Err("expected ')'".into());
                }
                self.advance();
                Ok(f)
            }
            Some(Token::LBracket) => {
                self.advance();
                if matches!(self.peek(), Some(Token::RBracket)) {
                    self.advance();
                    return Ok(Filter::ArrConstruct(Box::new(Filter::Empty)));
                }
                let f = self.parse_pipe()?;
                if !matches!(self.peek(), Some(Token::RBracket)) {
                    return Err("expected ']'".into());
                }
                self.advance();
                Ok(Filter::ArrConstruct(Box::new(f)))
            }
            Some(Token::LBrace) => {
                self.advance();
                let mut fields = Vec::new();
                if !matches!(self.peek(), Some(Token::RBrace)) {
                    loop {
                        let key = match self.peek() {
                            Some(Token::Ident(_)) => {
                                if let Some(Token::Ident(k)) = self.advance() { k } else { return Err("expected key".into()); }
                            }
                            Some(Token::Str(_)) => {
                                if let Some(Token::Str(k)) = self.advance() { k } else { return Err("expected key".into()); }
                            }
                            _ => return Err("expected object key".into()),
                        };
                        if matches!(self.peek(), Some(Token::Colon)) {
                            self.advance();
                            let val = self.parse_obj_value()?;
                            fields.push((key, val));
                        } else {
                            // Shorthand: {name} means {name: .name}
                            fields.push((key.clone(), Filter::Field(key)));
                        }
                        if matches!(self.peek(), Some(Token::Comma)) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                if !matches!(self.peek(), Some(Token::RBrace)) {
                    return Err("expected '}'".into());
                }
                self.advance();
                Ok(Filter::ObjConstruct(fields))
            }
            Some(Token::If) => {
                self.advance();
                let cond = self.parse_pipe()?;
                if !matches!(self.peek(), Some(Token::Then)) {
                    return Err("expected 'then'".into());
                }
                self.advance();
                let then_f = self.parse_pipe()?;
                let else_f = if matches!(self.peek(), Some(Token::Else)) {
                    self.advance();
                    Some(Box::new(self.parse_pipe()?))
                } else {
                    None
                };
                if !matches!(self.peek(), Some(Token::End)) {
                    return Err("expected 'end'".into());
                }
                self.advance();
                Ok(Filter::IfThenElse(Box::new(cond), Box::new(then_f), else_f))
            }
            Some(Token::Minus) => {
                self.advance();
                let right = self.parse_primary()?;
                Ok(Filter::Arith(ArithOp::Sub, Box::new(Filter::Literal(Value::Number(0.0))), Box::new(right)))
            }
            Some(Token::At(name)) => {
                self.advance();
                let kind = match name.as_str() {
                    "base64" => FormatKind::Base64,
                    "base64d" => FormatKind::Base64d,
                    "csv" => FormatKind::Csv,
                    "tsv" => FormatKind::Tsv,
                    "html" => FormatKind::Html,
                    "uri" => FormatKind::Uri,
                    "json" => FormatKind::Json,
                    "text" => FormatKind::Text,
                    _ => return Err(format!("unknown format: @{}", name)),
                };
                Ok(Filter::Format(kind))
            }
            Some(Token::Ident(name)) => {
                self.advance();
                self.parse_builtin_or_call(&name)
            }
            _ => Err(format!("unexpected token in filter: {:?}", self.peek())),
        }
    }

    fn parse_builtin_or_call(&mut self, name: &str) -> Result<Filter, String> {
        match name {
            "length" => Ok(Filter::Length),
            "keys" | "keys_unsorted" => Ok(Filter::Keys),
            "values" => Ok(Filter::Values),
            "type" => Ok(Filter::TypeF),
            "add" => Ok(Filter::Add),
            "sort" => Ok(Filter::Sort),
            "unique" => Ok(Filter::Unique),
            "flatten" => Ok(Filter::Flatten),
            "first" => Ok(Filter::First),
            "last" => Ok(Filter::Last),
            "reverse" => Ok(Filter::Reverse),
            "empty" => Ok(Filter::Empty),
            "to_entries" => Ok(Filter::ToEntries),
            "from_entries" => Ok(Filter::FromEntries),
            "ascii_downcase" => Ok(Filter::AsciiDown),
            "ascii_upcase" => Ok(Filter::AsciiUp),
            "tostring" => Ok(Filter::Tostring),
            "tonumber" => Ok(Filter::Tonumber),
            "floor" => Ok(Filter::Floor),
            "ceil" => Ok(Filter::Ceil),
            "round" => Ok(Filter::Round),
            "fabs" => Ok(Filter::Fabs),
            "sqrt" => Ok(Filter::Sqrt),
            "tojson" => Ok(Filter::Tojson),
            "fromjson" => Ok(Filter::Fromjson),
            "explode" => Ok(Filter::Explode),
            "implode" => Ok(Filter::Implode),
            "env" => Ok(Filter::Env),
            "input" => Ok(Filter::Input),
            "debug" => Ok(Filter::Debug),
            "paths" | "leaf_paths" => Ok(Filter::Paths),
            "numbers" => Ok(Filter::TypeSelect(TypeClass::Numbers)),
            "strings" => Ok(Filter::TypeSelect(TypeClass::Strings)),
            "booleans" => Ok(Filter::TypeSelect(TypeClass::Booleans)),
            "arrays" => Ok(Filter::TypeSelect(TypeClass::Arrays)),
            "objects" => Ok(Filter::TypeSelect(TypeClass::Objects)),
            "nulls" => Ok(Filter::TypeSelect(TypeClass::Nulls)),
            "iterables" => Ok(Filter::TypeSelect(TypeClass::Iterables)),
            "scalars" => Ok(Filter::TypeSelect(TypeClass::Scalars)),
            "nan" => Ok(Filter::Literal(Value::Number(f64::NAN))),
            "infinite" => Ok(Filter::Literal(Value::Number(f64::INFINITY))),
            "isinfinite" => Ok(Filter::FuncCall("isinfinite".into(), vec![])),
            "isnan" => Ok(Filter::FuncCall("isnan".into(), vec![])),
            "min" => Ok(Filter::MinMax(false)),
            "max" => Ok(Filter::MinMax(true)),
            "any" | "all" => {
                let is_all = name == "all";
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.advance();
                    let f = self.parse_pipe()?;
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        return Err("expected ')'".into());
                    }
                    self.advance();
                    if is_all { Ok(Filter::All(Some(Box::new(f)))) }
                    else { Ok(Filter::Any(Some(Box::new(f)))) }
                } else {
                    if is_all { Ok(Filter::All(None)) }
                    else { Ok(Filter::Any(None)) }
                }
            }
            "select" | "map" | "sort_by" | "group_by" | "unique_by" | "min_by" | "max_by"
            | "has" | "contains" | "inside" | "test" | "split" | "join"
            | "ltrimstr" | "rtrimstr" | "startswith" | "endswith"
            | "limit" | "range" | "getpath" | "delpaths" | "indices" => {
                if !matches!(self.peek(), Some(Token::LParen)) {
                    return Err(format!("{}() requires arguments", name));
                }
                self.advance();
                match name {
                    "select" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::Select(Box::new(f)))
                    }
                    "map" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::Map(Box::new(f)))
                    }
                    "sort_by" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::SortBy(Box::new(f)))
                    }
                    "group_by" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::GroupBy(Box::new(f)))
                    }
                    "unique_by" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::UniqueBy(Box::new(f)))
                    }
                    "min_by" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::MinMaxBy(false, Box::new(f)))
                    }
                    "max_by" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::MinMaxBy(true, Box::new(f)))
                    }
                    "has" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Has(s)) }
                        else { Err("has() requires a string argument".into()) }
                    }
                    "contains" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::Contains(Box::new(f)))
                    }
                    "inside" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::Inside(Box::new(f)))
                    }
                    "test" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Test(s)) }
                        else { Err("test() requires a string argument".into()) }
                    }
                    "split" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Split(s)) }
                        else { Err("split() requires a string argument".into()) }
                    }
                    "join" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Join(s)) }
                        else { Err("join() requires a string argument".into()) }
                    }
                    "ltrimstr" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Ltrimstr(s)) }
                        else { Err("ltrimstr() requires a string argument".into()) }
                    }
                    "rtrimstr" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Rtrimstr(s)) }
                        else { Err("rtrimstr() requires a string argument".into()) }
                    }
                    "startswith" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Startswith(s)) }
                        else { Err("startswith() requires a string argument".into()) }
                    }
                    "endswith" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Endswith(s)) }
                        else { Err("endswith() requires a string argument".into()) }
                    }
                    "indices" => {
                        let arg = self.expect_token("string argument")?;
                        self.expect_rparen()?;
                        if let Token::Str(s) = arg { Ok(Filter::Indices(s)) }
                        else { Err("indices() requires a string argument".into()) }
                    }
                    "limit" => {
                        let n = self.parse_pipe()?;
                        if !matches!(self.peek(), Some(Token::Semi)) {
                            return Err("limit() requires two arguments separated by ';'".into());
                        }
                        self.advance();
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::Limit2(Box::new(n), Box::new(f)))
                    }
                    "range" => {
                        let a = self.parse_pipe()?;
                        if matches!(self.peek(), Some(Token::Semi)) {
                            self.advance();
                            let b = self.parse_pipe()?;
                            self.expect_rparen()?;
                            Ok(Filter::Range(Box::new(a), Box::new(b)))
                        } else {
                            self.expect_rparen()?;
                            Ok(Filter::Range(Box::new(Filter::Literal(Value::Number(0.0))), Box::new(a)))
                        }
                    }
                    "getpath" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::GetPath(Box::new(f)))
                    }
                    "delpaths" => {
                        let f = self.parse_pipe()?;
                        self.expect_rparen()?;
                        Ok(Filter::Delpaths(Box::new(f)))
                    }
                    _ => Err(format!("unknown function: {}", name)),
                }
            }
            _ => {
                // Unknown identifier — try as function call
                if matches!(self.peek(), Some(Token::LParen)) {
                    self.advance();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::RParen)) {
                        loop {
                            args.push(self.parse_pipe()?);
                            if matches!(self.peek(), Some(Token::Semi)) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_rparen()?;
                    Ok(Filter::FuncCall(name.to_string(), args))
                } else {
                    Ok(Filter::FuncCall(name.to_string(), vec![]))
                }
            }
        }
    }

    fn expect_rparen(&mut self) -> Result<(), String> {
        if !matches!(self.peek(), Some(Token::RParen)) {
            return Err("expected ')'".into());
        }
        self.advance();
        Ok(())
    }
}

fn parse_filter(input: &str) -> Result<Filter, String> {
    let tokens = tokenize_filter(input)?;
    if tokens.is_empty() {
        return Ok(Filter::Identity);
    }
    let mut parser = FilterParser::new(tokens);
    let f = parser.parse()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!("unexpected token at position {}: {:?}", parser.pos, parser.tokens.get(parser.pos)));
    }
    Ok(f)
}

// ============================================================================
// Filter Evaluator
// ============================================================================

fn eval(filter: &Filter, input: &Value) -> Result<Vec<Value>, String> {
    match filter {
        Filter::Identity => Ok(vec![input.clone()]),
        Filter::Literal(v) => Ok(vec![v.clone()]),
        Filter::Field(name) => {
            match input {
                Value::Object(map) => {
                    Ok(vec![map.get(name).cloned().unwrap_or(Value::Null)])
                }
                Value::Null => Ok(vec![Value::Null]),
                _ => Err(format!("cannot index {} with string \"{}\"", input.type_name(), name)),
            }
        }
        Filter::Index(idx) => {
            match input {
                Value::Array(arr) => {
                    let i = if *idx < 0 { arr.len() as i64 + *idx } else { *idx };
                    if i >= 0 && (i as usize) < arr.len() {
                        Ok(vec![arr[i as usize].clone()])
                    } else {
                        Ok(vec![Value::Null])
                    }
                }
                Value::Null => Ok(vec![Value::Null]),
                _ => Err(format!("cannot index {} with number", input.type_name())),
            }
        }
        Filter::Iterate => {
            match input {
                Value::Array(arr) => Ok(arr.clone()),
                Value::Object(map) => Ok(map.values().cloned().collect()),
                Value::Null => Ok(vec![]),
                _ => Err(format!("cannot iterate over {}", input.type_name())),
            }
        }
        Filter::Pipe(left, right) => {
            let mid = eval(left, input)?;
            let mut results = Vec::new();
            for v in &mid {
                results.extend(eval(right, v)?);
            }
            Ok(results)
        }
        Filter::Comma(left, right) => {
            let mut results = eval(left, input)?;
            results.extend(eval(right, input)?);
            Ok(results)
        }
        Filter::Length => Ok(vec![input.length()]),
        Filter::Keys => {
            match input {
                Value::Object(map) => Ok(vec![Value::Array(map.keys().map(|k| Value::String(k.clone())).collect())]),
                Value::Array(arr) => Ok(vec![Value::Array((0..arr.len()).map(|i| Value::Number(i as f64)).collect())]),
                _ => Err(format!("{} has no keys", input.type_name())),
            }
        }
        Filter::Values => {
            match input {
                Value::Object(map) => Ok(vec![Value::Array(map.values().cloned().collect())]),
                Value::Array(arr) => Ok(vec![Value::Array(arr.clone())]),
                _ => Err(format!("{} has no values", input.type_name())),
            }
        }
        Filter::TypeF => Ok(vec![Value::String(input.type_name().to_string())]),
        Filter::Not => Ok(vec![Value::Bool(!input.is_truthy())]),
        Filter::Empty => Ok(vec![]),
        Filter::Add => {
            match input {
                Value::Array(arr) if arr.is_empty() => Ok(vec![Value::Null]),
                Value::Array(arr) => {
                    let mut acc = arr[0].clone();
                    for item in &arr[1..] {
                        acc = add_values(&acc, item)?;
                    }
                    Ok(vec![acc])
                }
                _ => Err("add requires array input".into()),
            }
        }
        Filter::Sort => {
            match input {
                Value::Array(arr) => {
                    let mut sorted = arr.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Ok(vec![Value::Array(sorted)])
                }
                _ => Err("sort requires array input".into()),
            }
        }
        Filter::Unique => {
            match input {
                Value::Array(arr) => {
                    let mut sorted = arr.clone();
                    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    sorted.dedup();
                    Ok(vec![Value::Array(sorted)])
                }
                _ => Err("unique requires array input".into()),
            }
        }
        Filter::Flatten => {
            match input {
                Value::Array(arr) => {
                    let mut flat = Vec::new();
                    flatten_into(arr, &mut flat);
                    Ok(vec![Value::Array(flat)])
                }
                _ => Err("flatten requires array input".into()),
            }
        }
        Filter::First => {
            match input {
                Value::Array(arr) => Ok(vec![arr.first().cloned().unwrap_or(Value::Null)]),
                _ => Err("first requires array input".into()),
            }
        }
        Filter::Last => {
            match input {
                Value::Array(arr) => Ok(vec![arr.last().cloned().unwrap_or(Value::Null)]),
                _ => Err("last requires array input".into()),
            }
        }
        Filter::Reverse => {
            match input {
                Value::Array(arr) => {
                    let mut rev = arr.clone();
                    rev.reverse();
                    Ok(vec![Value::Array(rev)])
                }
                Value::String(s) => Ok(vec![Value::String(s.chars().rev().collect())]),
                _ => Err("reverse requires array or string".into()),
            }
        }
        Filter::Select(f) => {
            let results = eval(f, input)?;
            if results.iter().any(|v| v.is_truthy()) {
                Ok(vec![input.clone()])
            } else {
                Ok(vec![])
            }
        }
        Filter::Map(f) => {
            match input {
                Value::Array(arr) => {
                    let mut out = Vec::new();
                    for item in arr {
                        out.extend(eval(f, item)?);
                    }
                    Ok(vec![Value::Array(out)])
                }
                _ => Err("map requires array input".into()),
            }
        }
        Filter::SortBy(f) => {
            match input {
                Value::Array(arr) => {
                    let mut pairs: Vec<(Value, Value)> = arr.iter()
                        .map(|item| {
                            let key = eval(f, item).ok().and_then(|v| v.into_iter().next()).unwrap_or(Value::Null);
                            (key, item.clone())
                        })
                        .collect();
                    pairs.sort_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Ok(vec![Value::Array(pairs.into_iter().map(|(_, v)| v).collect())])
                }
                _ => Err("sort_by requires array input".into()),
            }
        }
        Filter::GroupBy(f) => {
            match input {
                Value::Array(arr) => {
                    let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                    for item in arr {
                        let key = eval(f, item).ok().and_then(|v| v.into_iter().next()).unwrap_or(Value::Null);
                        if let Some(group) = groups.iter_mut().find(|(k, _)| k == &key) {
                            group.1.push(item.clone());
                        } else {
                            groups.push((key, vec![item.clone()]));
                        }
                    }
                    Ok(vec![Value::Array(groups.into_iter().map(|(_, v)| Value::Array(v)).collect())])
                }
                _ => Err("group_by requires array input".into()),
            }
        }
        Filter::UniqueBy(f) => {
            match input {
                Value::Array(arr) => {
                    let mut seen = Vec::new();
                    let mut result = Vec::new();
                    for item in arr {
                        let key = eval(f, item).ok().and_then(|v| v.into_iter().next()).unwrap_or(Value::Null);
                        if !seen.contains(&key) {
                            seen.push(key);
                            result.push(item.clone());
                        }
                    }
                    Ok(vec![Value::Array(result)])
                }
                _ => Err("unique_by requires array input".into()),
            }
        }
        Filter::Has(key) => {
            match input {
                Value::Object(map) => Ok(vec![Value::Bool(map.contains_key(key))]),
                Value::Array(arr) => {
                    if let Ok(idx) = key.parse::<usize>() {
                        Ok(vec![Value::Bool(idx < arr.len())])
                    } else {
                        Ok(vec![Value::Bool(false)])
                    }
                }
                _ => Ok(vec![Value::Bool(false)]),
            }
        }
        Filter::ToEntries => {
            match input {
                Value::Object(map) => {
                    let entries: Vec<Value> = map.iter().map(|(k, v)| {
                        let mut entry = BTreeMap::new();
                        entry.insert("key".to_string(), Value::String(k.clone()));
                        entry.insert("value".to_string(), v.clone());
                        Value::Object(entry)
                    }).collect();
                    Ok(vec![Value::Array(entries)])
                }
                _ => Err("to_entries requires object input".into()),
            }
        }
        Filter::FromEntries => {
            match input {
                Value::Array(arr) => {
                    let mut map = BTreeMap::new();
                    for item in arr {
                        if let Value::Object(obj) = item {
                            let key = obj.get("key").or_else(|| obj.get("name"));
                            let val = obj.get("value").cloned().unwrap_or(Value::Null);
                            if let Some(Value::String(k)) = key {
                                map.insert(k.clone(), val);
                            }
                        }
                    }
                    Ok(vec![Value::Object(map)])
                }
                _ => Err("from_entries requires array input".into()),
            }
        }
        Filter::Compare(op, left, right) => {
            let l_vals = eval(left, input)?;
            let r_vals = eval(right, input)?;
            let l = l_vals.first().cloned().unwrap_or(Value::Null);
            let r = r_vals.first().cloned().unwrap_or(Value::Null);
            let result = match op {
                CmpOp::Eq => l == r,
                CmpOp::Ne => l != r,
                CmpOp::Lt => l.partial_cmp(&r) == Some(std::cmp::Ordering::Less),
                CmpOp::Gt => l.partial_cmp(&r) == Some(std::cmp::Ordering::Greater),
                CmpOp::Le => l.partial_cmp(&r).is_some_and(|o| o != std::cmp::Ordering::Greater),
                CmpOp::Ge => l.partial_cmp(&r).is_some_and(|o| o != std::cmp::Ordering::Less),
            };
            Ok(vec![Value::Bool(result)])
        }
        Filter::Arith(op, left, right) => {
            let l_vals = eval(left, input)?;
            let r_vals = eval(right, input)?;
            let l = l_vals.first().cloned().unwrap_or(Value::Null);
            let r = r_vals.first().cloned().unwrap_or(Value::Null);
            match op {
                ArithOp::Add => Ok(vec![add_values(&l, &r)?]),
                ArithOp::Sub => {
                    match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => Ok(vec![Value::Number(a - b)]),
                        _ => Err(format!("cannot subtract {} and {}", l.type_name(), r.type_name())),
                    }
                }
                ArithOp::Mul => {
                    match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => Ok(vec![Value::Number(a * b)]),
                        _ => Err(format!("cannot multiply {} and {}", l.type_name(), r.type_name())),
                    }
                }
                ArithOp::Div => {
                    match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => {
                            if *b == 0.0 { Err("division by zero".into()) }
                            else { Ok(vec![Value::Number(a / b)]) }
                        }
                        _ => Err(format!("cannot divide {} by {}", l.type_name(), r.type_name())),
                    }
                }
                ArithOp::Mod => {
                    match (&l, &r) {
                        (Value::Number(a), Value::Number(b)) => {
                            if *b == 0.0 { Err("modulo by zero".into()) }
                            else { Ok(vec![Value::Number(a % b)]) }
                        }
                        _ => Err(format!("cannot modulo {} by {}", l.type_name(), r.type_name())),
                    }
                }
            }
        }
        Filter::And(left, right) => {
            let l = eval(left, input)?.into_iter().next().unwrap_or(Value::Null);
            if !l.is_truthy() { return Ok(vec![Value::Bool(false)]); }
            let r = eval(right, input)?.into_iter().next().unwrap_or(Value::Null);
            Ok(vec![Value::Bool(r.is_truthy())])
        }
        Filter::Or(left, right) => {
            let l = eval(left, input)?.into_iter().next().unwrap_or(Value::Null);
            if l.is_truthy() { return Ok(vec![Value::Bool(true)]); }
            let r = eval(right, input)?.into_iter().next().unwrap_or(Value::Null);
            Ok(vec![Value::Bool(r.is_truthy())])
        }
        Filter::IfThenElse(cond, then_f, else_f) => {
            let c = eval(cond, input)?.into_iter().next().unwrap_or(Value::Null);
            if c.is_truthy() {
                eval(then_f, input)
            } else if let Some(ef) = else_f {
                eval(ef, input)
            } else {
                Ok(vec![input.clone()])
            }
        }
        Filter::TryCatch(f) => {
            match eval(f, input) {
                Ok(v) => Ok(v),
                Err(_) => Ok(vec![]),
            }
        }
        Filter::ObjConstruct(fields) => {
            let mut map = BTreeMap::new();
            for (key, val_filter) in fields {
                let vals = eval(val_filter, input)?;
                let val = vals.into_iter().next().unwrap_or(Value::Null);
                map.insert(key.clone(), val);
            }
            Ok(vec![Value::Object(map)])
        }
        Filter::ArrConstruct(f) => {
            let results = eval(f, input)?;
            Ok(vec![Value::Array(results)])
        }
        Filter::Recurse => {
            let mut results = Vec::new();
            recurse_into(input, &mut results);
            Ok(results)
        }
        Filter::TypeSelect(class) => {
            let keep = match class {
                TypeClass::Numbers => matches!(input, Value::Number(_)),
                TypeClass::Strings => matches!(input, Value::String(_)),
                TypeClass::Booleans => matches!(input, Value::Bool(_)),
                TypeClass::Arrays => matches!(input, Value::Array(_)),
                TypeClass::Objects => matches!(input, Value::Object(_)),
                TypeClass::Nulls => matches!(input, Value::Null),
                TypeClass::Iterables => matches!(input, Value::Array(_) | Value::Object(_)),
                TypeClass::Scalars => !matches!(input, Value::Array(_) | Value::Object(_)),
            };
            if keep { Ok(vec![input.clone()]) } else { Ok(vec![]) }
        }
        Filter::AsciiDown => {
            match input {
                Value::String(s) => Ok(vec![Value::String(s.to_lowercase())]),
                _ => Err("ascii_downcase requires string".into()),
            }
        }
        Filter::AsciiUp => {
            match input {
                Value::String(s) => Ok(vec![Value::String(s.to_uppercase())]),
                _ => Err("ascii_upcase requires string".into()),
            }
        }
        Filter::Tostring => {
            match input {
                Value::String(s) => Ok(vec![Value::String(s.clone())]),
                other => Ok(vec![Value::String(format_json(other, true, 0))]),
            }
        }
        Filter::Tonumber => {
            match input {
                Value::Number(_) => Ok(vec![input.clone()]),
                Value::String(s) => {
                    let n: f64 = s.trim().parse().map_err(|_| format!("cannot convert \"{}\" to number", s))?;
                    Ok(vec![Value::Number(n)])
                }
                _ => Err(format!("cannot convert {} to number", input.type_name())),
            }
        }
        Filter::Floor => num_op(input, f64::floor),
        Filter::Ceil => num_op(input, f64::ceil),
        Filter::Round => num_op(input, f64::round),
        Filter::Fabs => num_op(input, f64::abs),
        Filter::Sqrt => num_op(input, f64::sqrt),
        Filter::Split(delim) => {
            match input {
                Value::String(s) => {
                    let parts: Vec<Value> = s.split(delim.as_str()).map(|p| Value::String(p.to_string())).collect();
                    Ok(vec![Value::Array(parts)])
                }
                _ => Err("split requires string input".into()),
            }
        }
        Filter::Join(sep) => {
            match input {
                Value::Array(arr) => {
                    let strs: Vec<String> = arr.iter().map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => format_json(other, true, 0),
                    }).collect();
                    Ok(vec![Value::String(strs.join(sep))])
                }
                _ => Err("join requires array input".into()),
            }
        }
        Filter::Test(pat) => {
            match input {
                Value::String(s) => Ok(vec![Value::Bool(s.contains(pat.as_str()))]),
                _ => Err("test requires string input".into()),
            }
        }
        Filter::Ltrimstr(prefix) => {
            match input {
                Value::String(s) => {
                    if let Some(rest) = s.strip_prefix(prefix.as_str()) {
                        Ok(vec![Value::String(rest.to_string())])
                    } else {
                        Ok(vec![input.clone()])
                    }
                }
                _ => Ok(vec![input.clone()]),
            }
        }
        Filter::Rtrimstr(suffix) => {
            match input {
                Value::String(s) => {
                    if let Some(rest) = s.strip_suffix(suffix.as_str()) {
                        Ok(vec![Value::String(rest.to_string())])
                    } else {
                        Ok(vec![input.clone()])
                    }
                }
                _ => Ok(vec![input.clone()]),
            }
        }
        Filter::Startswith(prefix) => {
            match input {
                Value::String(s) => Ok(vec![Value::Bool(s.starts_with(prefix.as_str()))]),
                _ => Ok(vec![Value::Bool(false)]),
            }
        }
        Filter::Endswith(suffix) => {
            match input {
                Value::String(s) => Ok(vec![Value::Bool(s.ends_with(suffix.as_str()))]),
                _ => Ok(vec![Value::Bool(false)]),
            }
        }
        Filter::MinMax(is_max) => {
            match input {
                Value::Array(arr) if arr.is_empty() => Ok(vec![Value::Null]),
                Value::Array(arr) => {
                    let mut best = &arr[0];
                    for item in &arr[1..] {
                        if let Some(ord) = item.partial_cmp(best)
                            && ((*is_max && ord == std::cmp::Ordering::Greater) || (!*is_max && ord == std::cmp::Ordering::Less)) {
                                best = item;
                            }
                    }
                    Ok(vec![best.clone()])
                }
                _ => Err("min/max requires array".into()),
            }
        }
        Filter::Range(start_f, end_f) => {
            let start = eval(start_f, input)?.into_iter().next().unwrap_or(Value::Number(0.0));
            let end = eval(end_f, input)?.into_iter().next().unwrap_or(Value::Number(0.0));
            match (start.as_f64(), end.as_f64()) {
                (Some(s), Some(e)) => {
                    let mut results = Vec::new();
                    let mut i = s as i64;
                    let end_i = e as i64;
                    while i < end_i {
                        results.push(Value::Number(i as f64));
                        i += 1;
                    }
                    Ok(results)
                }
                _ => Err("range requires numeric arguments".into()),
            }
        }
        Filter::Format(kind) => {
            let s = match input {
                Value::String(s) => s.clone(),
                other => format_json(other, true, 0),
            };
            match kind {
                FormatKind::Base64 => Ok(vec![Value::String(base64_encode(s.as_bytes()))]),
                FormatKind::Base64d => {
                    let decoded = base64_decode(&s).map_err(|e| format!("base64 decode error: {}", e))?;
                    Ok(vec![Value::String(String::from_utf8_lossy(&decoded).to_string())])
                }
                FormatKind::Html => {
                    let mut out = String::new();
                    for ch in s.chars() {
                        match ch {
                            '&' => out.push_str("&amp;"),
                            '<' => out.push_str("&lt;"),
                            '>' => out.push_str("&gt;"),
                            '\'' => out.push_str("&#39;"),
                            '"' => out.push_str("&quot;"),
                            c => out.push(c),
                        }
                    }
                    Ok(vec![Value::String(out)])
                }
                FormatKind::Uri => {
                    let mut out = String::new();
                    for b in s.bytes() {
                        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
                            out.push(b as char);
                        } else {
                            let _ = fmt::Write::write_fmt(&mut out, format_args!("%{:02X}", b));
                        }
                    }
                    Ok(vec![Value::String(out)])
                }
                FormatKind::Csv => format_csv(input),
                FormatKind::Tsv => format_tsv(input),
                FormatKind::Json => Ok(vec![Value::String(format_json(input, true, 0))]),
                FormatKind::Text => Ok(vec![Value::String(s)]),
            }
        }
        Filter::Tojson => Ok(vec![Value::String(format_json(input, true, 0))]),
        Filter::Fromjson => {
            match input {
                Value::String(s) => {
                    let v = parse_json(s)?;
                    Ok(vec![v])
                }
                _ => Err("fromjson requires string input".into()),
            }
        }
        Filter::Contains(f) => {
            let other = eval(f, input)?.into_iter().next().unwrap_or(Value::Null);
            Ok(vec![Value::Bool(value_contains(input, &other))])
        }
        // Catch-all for unimplemented filters
        _ => Err(format!("filter not implemented: {:?}", filter)),
    }
}

fn num_op(input: &Value, f: fn(f64) -> f64) -> Result<Vec<Value>, String> {
    match input {
        Value::Number(n) => Ok(vec![Value::Number(f(*n))]),
        _ => Err(format!("{} requires number input", input.type_name())),
    }
}

fn add_values(a: &Value, b: &Value) -> Result<Value, String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x + y)),
        (Value::String(x), Value::String(y)) => Ok(Value::String(format!("{}{}", x, y))),
        (Value::Array(x), Value::Array(y)) => {
            let mut combined = x.clone();
            combined.extend(y.iter().cloned());
            Ok(Value::Array(combined))
        }
        (Value::Object(x), Value::Object(y)) => {
            let mut combined = x.clone();
            combined.extend(y.iter().map(|(k, v)| (k.clone(), v.clone())));
            Ok(Value::Object(combined))
        }
        (Value::Null, other) | (other, Value::Null) => Ok(other.clone()),
        _ => Err(format!("cannot add {} and {}", a.type_name(), b.type_name())),
    }
}

fn flatten_into(arr: &[Value], out: &mut Vec<Value>) {
    for item in arr {
        match item {
            Value::Array(inner) => flatten_into(inner, out),
            other => out.push(other.clone()),
        }
    }
}

fn recurse_into(val: &Value, out: &mut Vec<Value>) {
    out.push(val.clone());
    match val {
        Value::Array(arr) => { for item in arr { recurse_into(item, out); } }
        Value::Object(map) => { for v in map.values() { recurse_into(v, out); } }
        _ => {}
    }
}

fn value_contains(haystack: &Value, needle: &Value) -> bool {
    match (haystack, needle) {
        (Value::String(h), Value::String(n)) => h.contains(n.as_str()),
        (Value::Array(h), Value::Array(n)) => {
            n.iter().all(|needle_item| h.iter().any(|h_item| value_contains(h_item, needle_item)))
        }
        (Value::Object(h), Value::Object(n)) => {
            n.iter().all(|(k, nv)| h.get(k).is_some_and(|hv| value_contains(hv, nv)))
        }
        (a, b) => a == b,
    }
}

fn format_csv(input: &Value) -> Result<Vec<Value>, String> {
    match input {
        Value::Array(arr) => {
            let fields: Vec<String> = arr.iter().map(|v| match v {
                Value::String(s) => {
                    if s.contains(',') || s.contains('"') || s.contains('\n') {
                        format!("\"{}\"", s.replace('"', "\"\""))
                    } else { s.clone() }
                }
                Value::Number(n) => {
                    if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{}", n) }
                }
                Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
                Value::Null => String::new(),
                other => format_json(other, true, 0),
            }).collect();
            Ok(vec![Value::String(fields.join(","))])
        }
        _ => Err("@csv requires array input".into()),
    }
}

fn format_tsv(input: &Value) -> Result<Vec<Value>, String> {
    match input {
        Value::Array(arr) => {
            let fields: Vec<String> = arr.iter().map(|v| match v {
                Value::String(s) => s.replace('\t', "\\t").replace('\n', "\\n").replace('\r', "\\r").replace('\\', "\\\\"),
                Value::Number(n) => {
                    if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{}", n) }
                }
                Value::Bool(b) => (if *b { "true" } else { "false" }).to_string(),
                Value::Null => String::new(),
                other => format_json(other, true, 0),
            }).collect();
            Ok(vec![Value::String(fields.join("\t"))])
        }
        _ => Err("@tsv requires array input".into()),
    }
}

// ============================================================================
// Base64
// ============================================================================

const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i + 2 < data.len() {
        let b0 = data[i] as u32;
        let b1 = data[i + 1] as u32;
        let b2 = data[i + 2] as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        out.push(B64_CHARS[(triple & 0x3F) as usize] as char);
        i += 3;
    }
    let remaining = data.len() - i;
    if remaining == 1 {
        let b0 = data[i] as u32;
        out.push(B64_CHARS[((b0 >> 2) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((b0 << 4) & 0x3F) as usize] as char);
        out.push('=');
        out.push('=');
    } else if remaining == 2 {
        let b0 = data[i] as u32;
        let b1 = data[i + 1] as u32;
        let pair = (b0 << 8) | b1;
        out.push(B64_CHARS[((pair >> 10) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((pair >> 4) & 0x3F) as usize] as char);
        out.push(B64_CHARS[((pair << 2) & 0x3F) as usize] as char);
        out.push('=');
    }
    out
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut buf: u32 = 0;
    let mut bits = 0;
    for ch in input.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err(format!("invalid base64 character: {}", ch as char)),
        };
        buf = (buf << 6) | val as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

// ============================================================================
// CLI
// ============================================================================

struct Options {
    filter: String,
    files: Vec<String>,
    compact: bool,
    raw: bool,
    exit_status: bool,
    null_input: bool,
    slurp: bool,
    indent: usize,
    tab: bool,
    raw_input: bool,
    args: Vec<(String, String)>,
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = std::env::args().collect();
    let mut opts = Options {
        filter: ".".to_string(),
        files: Vec::new(),
        compact: false,
        raw: false,
        exit_status: false,
        null_input: false,
        slurp: false,
        indent: 2,
        tab: false,
        raw_input: false,
        args: Vec::new(),
    };

    let mut i = 1;
    let mut filter_set = false;
    while i < args.len() {
        match args[i].as_str() {
            "-c" | "--compact-output" => opts.compact = true,
            "-r" | "--raw-output" => opts.raw = true,
            "-e" | "--exit-status" => opts.exit_status = true,
            "-n" | "--null-input" => opts.null_input = true,
            "-s" | "--slurp" => opts.slurp = true,
            "-R" | "--raw-input" => opts.raw_input = true,
            "--tab" => opts.tab = true,
            "--indent" => {
                i += 1;
                if i >= args.len() { return Err("--indent requires argument".into()); }
                opts.indent = args[i].parse().map_err(|_| "invalid indent".to_string())?;
            }
            "--arg" => {
                if i + 2 >= args.len() { return Err("--arg requires name and value".into()); }
                let name = args[i + 1].clone();
                let value = args[i + 2].clone();
                opts.args.push((name, value));
                i += 2;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "--version" => {
                write_stdout("jq 0.1.0 (OurOS)\n");
                std::process::exit(0);
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("unknown option: {}", arg));
            }
            _ => {
                if !filter_set {
                    opts.filter = args[i].clone();
                    filter_set = true;
                } else {
                    opts.files.push(args[i].clone());
                }
            }
        }
        i += 1;
    }

    Ok(opts)
}

fn print_usage() {
    write_stdout("Usage: jq [OPTIONS] FILTER [FILE...]\n\n");
    write_stdout("JSON processor — transform and query JSON data.\n\n");
    write_stdout("Options:\n");
    write_stdout("  -c, --compact-output  Compact output\n");
    write_stdout("  -r, --raw-output      Raw string output (no quotes)\n");
    write_stdout("  -e, --exit-status     Exit with error if result is false/null\n");
    write_stdout("  -n, --null-input      Don't read input; use null\n");
    write_stdout("  -s, --slurp           Read all inputs into array\n");
    write_stdout("  -R, --raw-input        Read each line as string\n");
    write_stdout("  --tab                 Use tabs for indentation\n");
    write_stdout("  --indent N            Indent depth (default: 2)\n");
    write_stdout("  --arg name value      Set $name to value\n");
    write_stdout("  -h, --help            Show this help\n");
    write_stdout("  --version             Show version\n");
}

fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            write_stderr(&format!("jq: {}\n", e));
            std::process::exit(2);
        }
    };

    let filter = match parse_filter(&opts.filter) {
        Ok(f) => f,
        Err(e) => {
            write_stderr(&format!("jq: compile error: {}\n", e));
            std::process::exit(3);
        }
    };

    let indent = if opts.tab { 1 } else { opts.indent };
    let mut any_output = false;
    let mut last_was_falsy = false;

    let process_value = |val: &Value, filter: &Filter, opts: &Options, any: &mut bool, falsy: &mut bool| {
        match eval(filter, val) {
            Ok(results) => {
                for result in &results {
                    *any = true;
                    *falsy = matches!(result, Value::Null | Value::Bool(false));
                    if opts.raw && matches!(result, Value::String(_)) {
                        if let Value::String(s) = result {
                            write_stdout(s);
                            write_stdout("\n");
                        }
                    } else {
                        let formatted = format_json(result, opts.compact, indent);
                        write_stdout(&formatted);
                        write_stdout("\n");
                    }
                }
            }
            Err(e) => {
                write_stderr(&format!("jq: error: {}\n", e));
            }
        }
    };

    if opts.null_input {
        let input = Value::Null;
        process_value(&input, &filter, &opts, &mut any_output, &mut last_was_falsy);
    } else if opts.files.is_empty() {
        // Read from stdin
        let mut input_str = String::new();
        if io::stdin().read_to_string(&mut input_str).is_err() {
            write_stderr("jq: error reading stdin\n");
            std::process::exit(2);
        }

        if opts.raw_input {
            let lines: Vec<Value> = input_str.lines().map(|l| Value::String(l.to_string())).collect();
            if opts.slurp {
                let arr = Value::Array(lines);
                process_value(&arr, &filter, &opts, &mut any_output, &mut last_was_falsy);
            } else {
                for line in &lines {
                    process_value(line, &filter, &opts, &mut any_output, &mut last_was_falsy);
                }
            }
        } else if opts.slurp {
            // Parse all JSON values and slurp into array
            let mut values = Vec::new();
            let trimmed = input_str.trim();
            if !trimmed.is_empty() {
                let mut parser = Parser::new(trimmed);
                loop {
                    parser.skip_ws();
                    if parser.pos >= parser.input.len() { break; }
                    match parser.parse_value() {
                        Ok(v) => values.push(v),
                        Err(e) => {
                            write_stderr(&format!("jq: parse error: {}\n", e));
                            std::process::exit(2);
                        }
                    }
                }
            }
            let arr = Value::Array(values);
            process_value(&arr, &filter, &opts, &mut any_output, &mut last_was_falsy);
        } else {
            // Parse potentially multiple JSON values from stdin
            let trimmed = input_str.trim();
            if !trimmed.is_empty() {
                let mut parser = Parser::new(trimmed);
                loop {
                    parser.skip_ws();
                    if parser.pos >= parser.input.len() { break; }
                    match parser.parse_value() {
                        Ok(v) => process_value(&v, &filter, &opts, &mut any_output, &mut last_was_falsy),
                        Err(e) => {
                            write_stderr(&format!("jq: parse error: {}\n", e));
                            std::process::exit(2);
                        }
                    }
                }
            }
        }
    } else {
        for file in &opts.files {
            let content = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    write_stderr(&format!("jq: {}: {}\n", file, e));
                    continue;
                }
            };
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let mut parser = Parser::new(trimmed);
                loop {
                    parser.skip_ws();
                    if parser.pos >= parser.input.len() { break; }
                    match parser.parse_value() {
                        Ok(v) => process_value(&v, &filter, &opts, &mut any_output, &mut last_was_falsy),
                        Err(e) => {
                            write_stderr(&format!("jq: {}: parse error: {}\n", file, e));
                            break;
                        }
                    }
                }
            }
        }
    }

    if opts.exit_status && last_was_falsy {
        std::process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_str(filter: &str, json: &str) -> Vec<Value> {
        let val = parse_json(json).expect("parse json");
        let f = parse_filter(filter).expect("parse filter");
        eval(&f, &val).expect("eval")
    }

    fn eval_first(filter: &str, json: &str) -> Value {
        eval_str(filter, json).into_iter().next().expect("at least one result")
    }

    // --- JSON parser tests ---

    #[test]
    fn test_parse_null() { assert_eq!(parse_json("null").unwrap(), Value::Null); }

    #[test]
    fn test_parse_bool() {
        assert_eq!(parse_json("true").unwrap(), Value::Bool(true));
        assert_eq!(parse_json("false").unwrap(), Value::Bool(false));
    }

    #[test]
    fn test_parse_integer() { assert_eq!(parse_json("42").unwrap(), Value::Number(42.0)); }

    #[test]
    fn test_parse_negative() { assert_eq!(parse_json("-7").unwrap(), Value::Number(-7.0)); }

    #[test]
    fn test_parse_float() { assert_eq!(parse_json("3.25").unwrap(), Value::Number(3.25)); }

    #[test]
    fn test_parse_string() { assert_eq!(parse_json(r#""hello""#).unwrap(), Value::String("hello".into())); }

    #[test]
    fn test_parse_string_escapes() {
        assert_eq!(parse_json(r#""a\nb""#).unwrap(), Value::String("a\nb".into()));
        assert_eq!(parse_json(r#""a\tb""#).unwrap(), Value::String("a\tb".into()));
    }

    #[test]
    fn test_parse_empty_array() { assert_eq!(parse_json("[]").unwrap(), Value::Array(vec![])); }

    #[test]
    fn test_parse_array() {
        assert_eq!(parse_json("[1,2,3]").unwrap(), Value::Array(vec![
            Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)
        ]));
    }

    #[test]
    fn test_parse_empty_object() { assert_eq!(parse_json("{}").unwrap(), Value::Object(BTreeMap::new())); }

    #[test]
    fn test_parse_object() {
        let v = parse_json(r#"{"a":1,"b":"two"}"#).unwrap();
        if let Value::Object(map) = v {
            assert_eq!(map.get("a"), Some(&Value::Number(1.0)));
            assert_eq!(map.get("b"), Some(&Value::String("two".into())));
        } else { panic!("expected object"); }
    }

    #[test]
    fn test_parse_nested() {
        let v = parse_json(r#"{"items":[1,{"x":true}]}"#).unwrap();
        assert!(matches!(v, Value::Object(_)));
    }

    #[test]
    fn test_parse_whitespace() {
        assert_eq!(parse_json("  null  ").unwrap(), Value::Null);
        assert_eq!(parse_json(" [ 1 , 2 ] ").unwrap(), Value::Array(vec![Value::Number(1.0), Value::Number(2.0)]));
    }

    // --- Filter tests ---

    #[test]
    fn test_identity() { assert_eq!(eval_first(".", "42"), Value::Number(42.0)); }

    #[test]
    fn test_field_access() {
        assert_eq!(eval_first(".name", r#"{"name":"Alice"}"#), Value::String("Alice".into()));
    }

    #[test]
    fn test_nested_field() {
        assert_eq!(eval_first(".a.b", r#"{"a":{"b":99}}"#), Value::Number(99.0));
    }

    #[test]
    fn test_missing_field() {
        assert_eq!(eval_first(".missing", r#"{"a":1}"#), Value::Null);
    }

    #[test]
    fn test_array_index() {
        assert_eq!(eval_first(".[1]", "[10,20,30]"), Value::Number(20.0));
    }

    #[test]
    fn test_array_negative_index() {
        assert_eq!(eval_first(".[-1]", "[10,20,30]"), Value::Number(30.0));
    }

    #[test]
    fn test_iterate_array() {
        assert_eq!(eval_str(".[]", "[1,2,3]"), vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]);
    }

    #[test]
    fn test_iterate_object() {
        let results = eval_str(".[]", r#"{"a":1,"b":2}"#);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_pipe() {
        assert_eq!(eval_first(".a | .b", r#"{"a":{"b":42}}"#), Value::Number(42.0));
    }

    #[test]
    fn test_comma() {
        let results = eval_str(".a, .b", r#"{"a":1,"b":2}"#);
        assert_eq!(results, vec![Value::Number(1.0), Value::Number(2.0)]);
    }

    #[test]
    fn test_select() {
        let results = eval_str(".[] | select(. > 2)", "[1,2,3,4]");
        assert_eq!(results, vec![Value::Number(3.0), Value::Number(4.0)]);
    }

    #[test]
    fn test_length() {
        assert_eq!(eval_first("length", r#""hello""#), Value::Number(5.0));
        assert_eq!(eval_first("length", "[1,2,3]"), Value::Number(3.0));
        assert_eq!(eval_first("length", r#"{"a":1,"b":2}"#), Value::Number(2.0));
    }

    #[test]
    fn test_keys() {
        let v = eval_first("keys", r#"{"b":2,"a":1}"#);
        assert_eq!(v, Value::Array(vec![Value::String("a".into()), Value::String("b".into())]));
    }

    #[test]
    fn test_values() {
        let v = eval_first("values", r#"{"a":1,"b":2}"#);
        if let Value::Array(arr) = v { assert_eq!(arr.len(), 2); }
        else { panic!("expected array"); }
    }

    #[test]
    fn test_type() {
        assert_eq!(eval_first("type", "42"), Value::String("number".into()));
        assert_eq!(eval_first("type", r#""hi""#), Value::String("string".into()));
        assert_eq!(eval_first("type", "null"), Value::String("null".into()));
    }

    #[test]
    fn test_map() {
        assert_eq!(eval_first("map(. + 1)", "[1,2,3]"),
            Value::Array(vec![Value::Number(2.0), Value::Number(3.0), Value::Number(4.0)]));
    }

    #[test]
    fn test_sort() {
        assert_eq!(eval_first("sort", "[3,1,2]"),
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]));
    }

    #[test]
    fn test_unique() {
        assert_eq!(eval_first("unique", "[1,2,1,3,2]"),
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]));
    }

    #[test]
    fn test_flatten() {
        assert_eq!(eval_first("flatten", "[[1,2],[3,[4]]]"),
            Value::Array(vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0), Value::Number(4.0)]));
    }

    #[test]
    fn test_add_numbers() {
        assert_eq!(eval_first("add", "[1,2,3]"), Value::Number(6.0));
    }

    #[test]
    fn test_add_strings() {
        assert_eq!(eval_first("add", r#"["a","b","c"]"#), Value::String("abc".into()));
    }

    #[test]
    fn test_arithmetic() {
        assert_eq!(eval_first(". + 1", "5"), Value::Number(6.0));
        assert_eq!(eval_first(". - 2", "10"), Value::Number(8.0));
        assert_eq!(eval_first(". * 3", "4"), Value::Number(12.0));
        assert_eq!(eval_first(". / 2", "10"), Value::Number(5.0));
    }

    #[test]
    fn test_comparison() {
        assert_eq!(eval_first(". == 5", "5"), Value::Bool(true));
        assert_eq!(eval_first(". != 5", "3"), Value::Bool(true));
        assert_eq!(eval_first(". > 3", "5"), Value::Bool(true));
        assert_eq!(eval_first(". < 3", "5"), Value::Bool(false));
    }

    #[test]
    fn test_not() {
        assert_eq!(eval_first("not", "true"), Value::Bool(false));
        assert_eq!(eval_first("not", "false"), Value::Bool(true));
        assert_eq!(eval_first("not", "null"), Value::Bool(true));
    }

    #[test]
    fn test_if_then_else() {
        assert_eq!(eval_first("if . > 3 then \"big\" else \"small\" end", "5"), Value::String("big".into()));
        assert_eq!(eval_first("if . > 3 then \"big\" else \"small\" end", "1"), Value::String("small".into()));
    }

    #[test]
    fn test_has() {
        assert_eq!(eval_first("has(\"a\")", r#"{"a":1}"#), Value::Bool(true));
        assert_eq!(eval_first("has(\"b\")", r#"{"a":1}"#), Value::Bool(false));
    }

    #[test]
    fn test_to_entries() {
        let v = eval_first("to_entries", r#"{"a":1}"#);
        if let Value::Array(arr) = v {
            assert_eq!(arr.len(), 1);
            if let Value::Object(entry) = &arr[0] {
                assert_eq!(entry.get("key"), Some(&Value::String("a".into())));
                assert_eq!(entry.get("value"), Some(&Value::Number(1.0)));
            }
        }
    }

    #[test]
    fn test_from_entries() {
        let v = eval_first("from_entries", r#"[{"key":"a","value":1}]"#);
        if let Value::Object(map) = v {
            assert_eq!(map.get("a"), Some(&Value::Number(1.0)));
        }
    }

    #[test]
    fn test_ascii_case() {
        assert_eq!(eval_first("ascii_downcase", r#""HELLO""#), Value::String("hello".into()));
        assert_eq!(eval_first("ascii_upcase", r#""hello""#), Value::String("HELLO".into()));
    }

    #[test]
    fn test_split_join() {
        assert_eq!(eval_first("split(\",\")", r#""a,b,c""#),
            Value::Array(vec![Value::String("a".into()), Value::String("b".into()), Value::String("c".into())]));
        assert_eq!(eval_first("join(\"-\")", r#"["a","b","c"]"#), Value::String("a-b-c".into()));
    }

    #[test]
    fn test_startswith_endswith() {
        assert_eq!(eval_first("startswith(\"hel\")", r#""hello""#), Value::Bool(true));
        assert_eq!(eval_first("endswith(\"llo\")", r#""hello""#), Value::Bool(true));
    }

    #[test]
    fn test_contains() {
        assert_eq!(eval_first("contains(\"ell\")", r#""hello""#), Value::Bool(true));
    }

    #[test]
    fn test_first_last() {
        assert_eq!(eval_first("first", "[1,2,3]"), Value::Number(1.0));
        assert_eq!(eval_first("last", "[1,2,3]"), Value::Number(3.0));
    }

    #[test]
    fn test_reverse() {
        assert_eq!(eval_first("reverse", "[1,2,3]"),
            Value::Array(vec![Value::Number(3.0), Value::Number(2.0), Value::Number(1.0)]));
    }

    #[test]
    fn test_min_max() {
        assert_eq!(eval_first("min", "[3,1,2]"), Value::Number(1.0));
        assert_eq!(eval_first("max", "[3,1,2]"), Value::Number(3.0));
    }

    #[test]
    fn test_object_construct() {
        let v = eval_first("{a: .x, b: .y}", r#"{"x":1,"y":2}"#);
        if let Value::Object(map) = v {
            assert_eq!(map.get("a"), Some(&Value::Number(1.0)));
            assert_eq!(map.get("b"), Some(&Value::Number(2.0)));
        }
    }

    #[test]
    fn test_array_construct() {
        assert_eq!(eval_first("[.[] | . * 2]", "[1,2,3]"),
            Value::Array(vec![Value::Number(2.0), Value::Number(4.0), Value::Number(6.0)]));
    }

    #[test]
    fn test_try_catch() {
        // .foo? on a number should return empty, not error
        let results = eval_str(".foo?", "42");
        assert!(results.is_empty());
    }

    #[test]
    fn test_range() {
        assert_eq!(eval_str("range(3)", "null"),
            vec![Value::Number(0.0), Value::Number(1.0), Value::Number(2.0)]);
    }

    #[test]
    fn test_tostring_tonumber() {
        assert_eq!(eval_first("tostring", "42"), Value::String("42".into()));
        assert_eq!(eval_first("tonumber", r#""42""#), Value::Number(42.0));
    }

    #[test]
    fn test_floor_ceil_round() {
        assert_eq!(eval_first("floor", "3.7"), Value::Number(3.0));
        assert_eq!(eval_first("ceil", "3.2"), Value::Number(4.0));
        assert_eq!(eval_first("round", "3.5"), Value::Number(4.0));
    }

    #[test]
    fn test_format_json_compact() {
        let v = parse_json(r#"{"a":1,"b":[2,3]}"#).unwrap();
        let compact = format_json(&v, true, 0);
        assert!(compact.contains(r#""a":1"#));
        assert!(!compact.contains('\n'));
    }

    #[test]
    fn test_format_json_pretty() {
        let v = parse_json(r#"{"a":1}"#).unwrap();
        let pretty = format_json(&v, false, 2);
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    #[test]
    fn test_base64_roundtrip() {
        let data = b"Hello, World!";
        let encoded = base64_encode(data);
        let decoded = base64_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_base64_known() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn test_format_base64() {
        assert_eq!(eval_first("@base64", r#""hello""#), Value::String("aGVsbG8=".into()));
    }

    #[test]
    fn test_format_html() {
        assert_eq!(eval_first("@html", r#""<b>hi</b>""#), Value::String("&lt;b&gt;hi&lt;/b&gt;".into()));
    }

    #[test]
    fn test_sort_by() {
        let v = eval_first("sort_by(.x)", r#"[{"x":3},{"x":1},{"x":2}]"#);
        if let Value::Array(arr) = v
            && let Value::Object(first) = &arr[0]
        {
            assert_eq!(first.get("x"), Some(&Value::Number(1.0)));
        }
    }

    #[test]
    fn test_group_by() {
        let v = eval_first("group_by(.t)", r#"[{"t":"a","v":1},{"t":"b","v":2},{"t":"a","v":3}]"#);
        if let Value::Array(groups) = v {
            assert_eq!(groups.len(), 2);
        }
    }

    #[test]
    fn test_ltrimstr_rtrimstr() {
        assert_eq!(eval_first("ltrimstr(\"hello\")", r#""helloworld""#), Value::String("world".into()));
        assert_eq!(eval_first("rtrimstr(\"world\")", r#""helloworld""#), Value::String("hello".into()));
    }

    #[test]
    fn test_empty() {
        let results = eval_str("empty", "42");
        assert!(results.is_empty());
    }

    #[test]
    fn test_recurse() {
        let results = eval_str(".. | numbers", r#"{"a":1,"b":{"c":2}}"#);
        assert!(results.contains(&Value::Number(1.0)));
        assert!(results.contains(&Value::Number(2.0)));
    }

    #[test]
    fn test_and_or() {
        assert_eq!(eval_first("true and true", "null"), Value::Bool(true));
        assert_eq!(eval_first("true and false", "null"), Value::Bool(false));
        assert_eq!(eval_first("false or true", "null"), Value::Bool(true));
        assert_eq!(eval_first("false or false", "null"), Value::Bool(false));
    }

    #[test]
    fn test_parse_error() {
        assert!(parse_json("{invalid}").is_err());
        assert!(parse_json("[1,]").is_err());
    }

    #[test]
    fn test_unicode_escape() {
        assert_eq!(parse_json(r#""\u0041""#).unwrap(), Value::String("A".into()));
    }

    #[test]
    fn test_csv_format() {
        let v = eval_first("@csv", r#"["a","b,c",1]"#);
        if let Value::String(s) = v {
            assert!(s.contains("\"b,c\""));
        }
    }
}
