//! Reads the parser tree-sitter generates for a grammar -- its `src/parser.c`
//! -- and writes it out as Rust, so the grammar can be built without a C
//! compiler.
//!
//! # Why
//!
//! The code editor's syntax highlighting is tree-sitter's (`gui/syntax`,
//! `design-decisions.md` §1437). Its runtime is available as pure Rust
//! (`tree-sitter-c2rust`), but every grammar is a C file -- the generator's
//! output -- and this tree builds with no C compiler: not for the host tests,
//! not for the Linux lint, not for SlateOS itself. Rather than add one to every
//! build, this reads the one file the grammar is, which the generator writes in
//! a single fixed shape, and turns it into what the runtime reads:
//!
//! - the **tables** -- parse tables, lexer modes, symbol metadata, field maps,
//!   alias sequences -- as little-endian bytes laid out exactly as the C
//!   compiler would lay out the generator's structs, which the Rust side
//!   includes (`include_bytes!`) and hands the runtime pointers into; and
//! - the **lexers** -- `ts_lex` and `ts_lex_keywords`, state machines of
//!   `if`s and jumps -- as Rust functions ([`clex`]).
//!
//! The upstream `parser.c` stays in the tree verbatim, so a grammar is
//! updated by replacing it and can be diffed against upstream at any time; the
//! conversion runs in `gui/syntax/build.rs` on every build that needs it.
//!
//! # What the output assumes
//!
//! The Rust written here names the pieces `gui/syntax`'s `ffi` module
//! provides -- `Lexer`, `set_contains`, the `TSLanguage` mirror, `Aligned`,
//! `SyncLanguage`, `SyncPtrs`, `lexer_entry!`, `scanner_table` -- and, when the
//! grammar has an external scanner, a type called `Scanner` in scope where it
//! is included: the hand-ported scanner (`gui/syntax/src/grammars/*`).
//!
//! # What it refuses
//!
//! Anything in the file it does not recognise -- an unknown macro in a lexer,
//! a name with no value, a table the wrong size -- is an [`Error`] naming the
//! line, never a guess: a wrong table is a parser that mis-reads every file
//! quietly, and the build is the place to find out.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "offsets, counts and indexes into one file held in memory, by a \
              tool that runs at build time over the tree-sitter generator's \
              output; an overflow is a debug-build panic in a build script, \
              which fails the build loudly"
)]

mod cinit;
mod clex;
mod ctoken;

use core::fmt;
use core::fmt::Write as _;

use cinit::{Constants, Expr, Init, Parser};
use ctoken::Tok;

/// Why a `parser.c` could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The line of the file it is about; 0 when it is about the whole file.
    pub line: usize,
    /// What is wrong.
    pub message: String,
}

impl Error {
    pub(crate) fn at(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }

    fn whole(message: impl Into<String>) -> Self {
        Self::at(0, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            f.write_str(&self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

impl std::error::Error for Error {}

/// A static array the file declares: its element type, its dimensions and
/// its initializer.
struct Declared {
    ty: Vec<String>,
    dims: Vec<Expr>,
    init: Init,
    line: usize,
}

/// A grammar, read from its `parser.c`.
#[derive(Debug)]
pub struct Grammar {
    /// The grammar's name: `rust` for `tree_sitter_rust`.
    pub name: String,
    abi: u32,
    counts: Counts,
    keyword_capture_token: u16,
    metadata: Option<[u8; 3]>,
    symbol_names: Vec<Option<Vec<u8>>>,
    field_names: Vec<Option<Vec<u8>>>,
    /// Every table as the bytes the runtime reads, by name.
    blobs: Vec<Blob>,
    char_sets: Vec<(String, Vec<(i64, i64)>)>,
    lex: clex::LexFn,
    keyword_lex: Option<clex::LexFn>,
    /// The external tokens' names, in the scanner's order, when the grammar
    /// has an external scanner.
    external_tokens: Vec<String>,
}

/// The counts the generator `#define`s.
#[derive(Clone, Copy, Debug, Default)]
struct Counts {
    symbol: u32,
    alias: u32,
    token: u32,
    external_token: u32,
    state: u32,
    large_state: u32,
    production_id: u32,
    field: u32,
    max_alias_sequence_length: u16,
    max_reserved_word_set_size: u16,
    supertype: u32,
}

/// A table as bytes, and how it is aligned.
#[derive(Debug)]
struct Blob {
    /// The `TSLanguage` field it goes in.
    field: &'static str,
    bytes: Vec<u8>,
}

/// What [`Grammar::render`] writes: the Rust, and the files it includes.
#[derive(Debug)]
pub struct Output {
    /// The Rust source, to be `include!`d into the grammar's module.
    pub rust: String,
    /// Each table, as `(file name, bytes)`, to be written into the directory
    /// the Rust names.
    pub blobs: Vec<(String, Vec<u8>)>,
}

/// Read a grammar's `parser.c`.
///
/// # Errors
///
/// Whatever in the file this does not recognise, or a table whose size does
/// not agree with the counts the file declares.
pub fn parse(source: &str) -> Result<Grammar, Error> {
    let toks = ctoken::tokenize(source)?;
    let mut constants = Constants::new();
    let mut declared: Vec<(String, Declared)> = Vec::new();
    let mut lexers: Vec<(String, core::ops::Range<usize>)> = Vec::new();
    let mut function_name: Option<String> = None;
    let mut i = 0;
    while let Some((tok, _)) = toks.get(i) {
        match tok {
            Tok::Directive(d) => {
                if let Some(rest) = d.strip_prefix("define") {
                    let mut words = rest.split_whitespace();
                    if let (Some(name), Some(value), None) =
                        (words.next(), words.next(), words.next())
                        && let Ok(v) = value.parse::<i64>()
                    {
                        constants.define(name, v);
                    }
                }
                i += 1;
            }
            Tok::Ident("enum") => i = read_enum(&toks, i + 1, &mut constants)?,
            Tok::Ident("static") => {
                let (next, found) = read_static(&toks, i + 1)?;
                match found {
                    Static::Array(name, d) => declared.push((name, d)),
                    Static::Function(name, body) => lexers.push((name, body)),
                    Static::Other => {}
                }
                i = next;
            }
            Tok::Ident(name) if name.starts_with("tree_sitter_") => {
                // `tree_sitter_rust(void) {`: the function that returns the
                // language, and so the grammar's name. The scanner's
                // prototypes (`tree_sitter_rust_external_scanner_create`)
                // are not followed by `(void) {`.
                if matches!(toks.get(i + 1), Some((Tok::Punct("("), _)))
                    && matches!(toks.get(i + 2), Some((Tok::Ident("void"), _)))
                    && matches!(toks.get(i + 3), Some((Tok::Punct(")"), _)))
                    && matches!(toks.get(i + 4), Some((Tok::Punct("{"), _)))
                {
                    function_name = Some(name.trim_start_matches("tree_sitter_").to_owned());
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    build(&toks, &constants, &declared, &lexers, function_name)
}

/// `enum NAME { A = 1, B, ... };` from just after `enum`: each constant
/// defined. Returns where the declaration ends.
fn read_enum(
    toks: &[(Tok<'_>, usize)],
    mut i: usize,
    constants: &mut Constants,
) -> Result<usize, Error> {
    // The tag, if any.
    if matches!(toks.get(i), Some((Tok::Ident(_), _))) {
        i += 1;
    }
    let slice = toks.get(i..).unwrap_or_default();
    let mut p = Parser::new(slice);
    p.expect("{")?;
    let mut next_value = 0i64;
    loop {
        if p.eat("}") {
            break;
        }
        let line = p.line();
        let name = p.ident()?;
        let value = if p.eat("=") {
            let e = p.expr()?;
            constants.eval(&e, line)?
        } else {
            next_value
        };
        constants.define(name, value);
        next_value = value + 1;
        if !p.eat(",") {
            p.expect("}")?;
            break;
        }
    }
    p.expect(";")?;
    Ok(i + p.position())
}

/// What a `static` declared.
enum Static {
    /// An array or variable with an initializer.
    Array(String, Declared),
    /// A function: its name, and its body's tokens (between the braces).
    Function(String, core::ops::Range<usize>),
    /// Anything else: skipped.
    Other,
}

/// A `static` declaration from just after `static`. Returns where it ends.
fn read_static(toks: &[(Tok<'_>, usize)], start: usize) -> Result<(usize, Static), Error> {
    let line = toks.get(start).map_or(0, |(_, l)| *l);
    // The declaration's words up to `=`, `(`, `;` or `{`.
    let mut i = start;
    let mut words: Vec<String> = Vec::new();
    let mut name: Option<String> = None;
    let mut dims: Vec<Expr> = Vec::new();
    loop {
        let Some((tok, _)) = toks.get(i) else {
            return Err(Error::at(line, "a static declaration never ends"));
        };
        match tok {
            Tok::Ident(w) => {
                if let Some(n) = name.replace((*w).to_owned()) {
                    words.push(n);
                }
                i += 1;
            }
            Tok::Punct("*") => {
                words.push("*".to_owned());
                i += 1;
            }
            Tok::Punct("[") => {
                let slice = toks.get(i + 1..).unwrap_or_default();
                let mut p = Parser::new(slice);
                let dim = if p.eat("]") {
                    None
                } else {
                    let e = p.expr()?;
                    p.expect("]")?;
                    Some(e)
                };
                if let Some(e) = dim {
                    dims.push(e);
                }
                i += 1 + p.position();
            }
            Tok::Punct("=") => {
                let slice = toks.get(i + 1..).unwrap_or_default();
                let mut p = Parser::new(slice);
                let init = p.init()?;
                p.expect(";")?;
                let end = i + 1 + p.position();
                let name = name.ok_or_else(|| Error::at(line, "a static with no name"))?;
                return Ok((
                    end,
                    Static::Array(
                        name,
                        Declared {
                            ty: words,
                            dims,
                            init,
                            line,
                        },
                    ),
                ));
            }
            Tok::Punct("(") => {
                let name = name.ok_or_else(|| Error::at(line, "a function with no name"))?;
                // Past the parameters, to the body.
                let close = matching(toks, i, "(", ")")?;
                match toks.get(close + 1) {
                    Some((Tok::Punct("{"), _)) => {
                        let end = matching(toks, close + 1, "{", "}")?;
                        return Ok((end + 1, Static::Function(name, close + 2..end)));
                    }
                    _ => return Ok((close + 1, Static::Other)),
                }
            }
            Tok::Punct(";") => return Ok((i + 1, Static::Other)),
            _ => return Ok((i + 1, Static::Other)),
        }
    }
}

/// The index of the bracket closing the one at `open`.
fn matching(
    toks: &[(Tok<'_>, usize)],
    open: usize,
    left: &str,
    right: &str,
) -> Result<usize, Error> {
    let mut depth = 0usize;
    let mut i = open;
    while let Some((tok, _)) = toks.get(i) {
        match tok {
            Tok::Punct(p) if *p == left => depth += 1,
            Tok::Punct(p) if *p == right => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    let line = toks.get(open).map_or(0, |(_, l)| *l);
    Err(Error::at(line, format!("a `{left}` never closed")))
}

/// Everything read, made into a grammar.
fn build(
    toks: &[(Tok<'_>, usize)],
    constants: &Constants,
    declared: &[(String, Declared)],
    lexers: &[(String, core::ops::Range<usize>)],
    function_name: Option<String>,
) -> Result<Grammar, Error> {
    let count = |name: &str| -> Result<u32, Error> {
        constants
            .get(name)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| Error::whole(format!("`#define {name}` is missing")))
    };
    let optional = |name: &str| {
        constants
            .get(name)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(0)
    };
    let abi = count("LANGUAGE_VERSION")?;
    if !(13..=15).contains(&abi) {
        return Err(Error::whole(format!(
            "ABI version {abi}: this reads 13 to 15, which the runtime does"
        )));
    }
    let counts = Counts {
        symbol: count("SYMBOL_COUNT")?,
        alias: count("ALIAS_COUNT")?,
        token: count("TOKEN_COUNT")?,
        external_token: count("EXTERNAL_TOKEN_COUNT")?,
        state: count("STATE_COUNT")?,
        large_state: count("LARGE_STATE_COUNT")?,
        production_id: count("PRODUCTION_ID_COUNT")?,
        field: count("FIELD_COUNT")?,
        max_alias_sequence_length: u16::try_from(count("MAX_ALIAS_SEQUENCE_LENGTH")?)
            .map_err(|_| Error::whole("MAX_ALIAS_SEQUENCE_LENGTH past 16 bits"))?,
        max_reserved_word_set_size: u16::try_from(optional("MAX_RESERVED_WORD_SET_SIZE"))
            .map_err(|_| Error::whole("MAX_RESERVED_WORD_SET_SIZE past 16 bits"))?,
        supertype: optional("SUPERTYPE_COUNT"),
    };
    let get = |name: &str| declared.iter().find(|(n, _)| n == name).map(|(_, d)| d);
    let need = |name: &str| get(name).ok_or_else(|| Error::whole(format!("`{name}` is missing")));
    let usize_of = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
    let symbols_and_aliases = usize_of(counts.symbol + counts.alias);

    // The language struct: the name, the keyword token, the metadata.
    let language = need("language")?;
    let name = match cinit::field(&language.init, "name") {
        Some(Init::Expr(Expr::Str(bytes))) => String::from_utf8(bytes.clone())
            .map_err(|_| Error::at(language.line, "the grammar's name is not text"))?,
        _ => function_name.ok_or_else(|| Error::whole("the grammar has no name"))?,
    };
    let keyword_capture_token = match cinit::field(&language.init, "keyword_capture_token") {
        Some(Init::Expr(e)) => u16::try_from(constants.eval(e, language.line)?)
            .map_err(|_| Error::at(language.line, "the keyword token past 16 bits"))?,
        _ => 0,
    };
    let metadata = match cinit::field(&language.init, "metadata") {
        Some(m) => {
            let v = cinit::fields(
                m,
                &["major_version", "minor_version", "patch_version"],
                constants,
                language.line,
            )?;
            let byte = |i: usize| v.get(i).and_then(|x| u8::try_from(*x).ok()).unwrap_or(0);
            Some([byte(0), byte(1), byte(2)])
        }
        None => None,
    };
    let has_keyword_lexer = cinit::field(&language.init, "keyword_lex_fn").is_some();

    let symbol_names = strings(need("ts_symbol_names")?, symbols_and_aliases, constants)?;
    let field_names = match get("ts_field_names") {
        Some(d) => strings(d, usize_of(counts.field) + 1, constants)?,
        None if counts.field == 0 => Vec::new(),
        None => return Err(Error::whole("`ts_field_names` is missing")),
    };

    let mut blobs = Vec::new();
    let mut push = |field: &'static str, bytes: Vec<u8>| blobs.push(Blob { field, bytes });

    // parse_table: [LARGE_STATE_COUNT][SYMBOL_COUNT] u16.
    let large = need("ts_parse_table")?;
    push(
        "parse_table",
        u16s(&grid(
            large,
            usize_of(counts.large_state),
            usize_of(counts.symbol),
            constants,
        )?)?,
    );
    if let Some(small) = get("ts_small_parse_table") {
        let values = scalars(small, None, constants)?;
        push("small_parse_table", u16s(&values)?);
        let map = scalars(need("ts_small_parse_table_map")?, None, constants)?;
        push("small_parse_table_map", u32s(&map)?);
    }
    push(
        "parse_actions",
        parse_actions(need("ts_parse_actions")?, constants)?,
    );
    push(
        "symbol_metadata",
        bytes_of(
            &structs(
                need("ts_symbol_metadata")?,
                &["visible", "named", "supertype"],
                Some(symbols_and_aliases),
                constants,
            )?,
            &[1, 1, 1],
        )?,
    );
    push(
        "public_symbol_map",
        u16s(&scalars(
            need("ts_symbol_map")?,
            Some(symbols_and_aliases),
            constants,
        )?)?,
    );
    if let Some(d) = get("ts_non_terminal_alias_map") {
        push("alias_map", u16s(&scalars(d, None, constants)?)?);
    }
    if let Some(d) = get("ts_alias_sequences") {
        push(
            "alias_sequences",
            u16s(&grid(
                d,
                usize_of(counts.production_id),
                usize::from(counts.max_alias_sequence_length),
                constants,
            )?)?,
        );
    }
    if let Some(d) = get("ts_field_map_slices") {
        push(
            "field_map_slices",
            bytes_of(
                &structs(
                    d,
                    &["index", "length"],
                    Some(usize_of(counts.production_id)),
                    constants,
                )?,
                &[2, 2],
            )?,
        );
        push(
            "field_map_entries",
            bytes_of(
                &structs(
                    need("ts_field_map_entries")?,
                    &["field_id", "child_index", "inherited"],
                    None,
                    constants,
                )?,
                &[2, 1, 1],
            )?,
        );
    }
    let mode_fields: &[&str] = if abi >= 15 {
        &["lex_state", "external_lex_state", "reserved_word_set_id"]
    } else {
        &["lex_state", "external_lex_state"]
    };
    let mode_widths: &[usize] = if abi >= 15 { &[2, 2, 2] } else { &[2, 2] };
    push(
        "lex_modes",
        bytes_of(
            &structs(
                need("ts_lex_modes")?,
                mode_fields,
                Some(usize_of(counts.state)),
                constants,
            )?,
            mode_widths,
        )?,
    );
    if let Some(d) = get("ts_primary_state_ids") {
        push(
            "primary_state_ids",
            u16s(&scalars(d, Some(usize_of(counts.state)), constants)?)?,
        );
    }
    if let Some(d) = get("ts_reserved_words") {
        let rows = dim(d, 0, constants)?
            .ok_or_else(|| Error::at(d.line, "the reserved words' size is not given"))?;
        push(
            "reserved_words",
            u16s(&grid(
                d,
                rows,
                usize::from(counts.max_reserved_word_set_size),
                constants,
            )?)?,
        );
    }
    if let Some(d) = get("ts_supertype_symbols") {
        push(
            "supertype_symbols",
            u16s(&scalars(d, Some(usize_of(counts.supertype)), constants)?)?,
        );
        push(
            "supertype_map_slices",
            bytes_of(
                &structs(
                    need("ts_supertype_map_slices")?,
                    &["index", "length"],
                    None,
                    constants,
                )?,
                &[2, 2],
            )?,
        );
        push(
            "supertype_map_entries",
            u16s(&scalars(
                need("ts_supertype_map_entries")?,
                None,
                constants,
            )?)?,
        );
    }
    let mut external_tokens = Vec::new();
    if counts.external_token > 0 {
        let n = usize_of(counts.external_token);
        push(
            "external_scanner.symbol_map",
            u16s(&scalars(
                need("ts_external_scanner_symbol_map")?,
                Some(n),
                constants,
            )?)?,
        );
        let states = need("ts_external_scanner_states")?;
        let rows = dim(states, 0, constants)?
            .ok_or_else(|| Error::at(states.line, "the scanner states' size is not given"))?;
        let flags = grid(states, rows, n, constants)?;
        push(
            "external_scanner.states",
            flags.iter().map(|v| u8::from(*v != 0)).collect(),
        );
        // The tokens' names, in order, from the enum the scanner's states
        // are indexed by.
        let mut named: Vec<(i64, String)> = Vec::new();
        for (tok, _) in toks {
            if let Tok::Ident(w) = tok
                && let Some(short) = w.strip_prefix("ts_external_token_")
                && let Some(v) = constants.get(w)
                && !named.iter().any(|(_, n)| n == short)
            {
                named.push((v, short.to_owned()));
            }
        }
        named.sort();
        external_tokens = named.into_iter().map(|(_, n)| n).collect();
        if external_tokens.len() != n {
            return Err(Error::whole(format!(
                "{} external tokens named, {n} counted",
                external_tokens.len()
            )));
        }
    }

    // The character sets the lexers test against.
    let mut char_sets = Vec::new();
    for (name, d) in declared {
        if d.ty.last().is_some_and(|t| t == "TSCharacterRange") {
            let ranges = structs(d, &["start", "end"], None, constants)?;
            char_sets.push((
                name.clone(),
                ranges
                    .into_iter()
                    .map(|r| {
                        (
                            r.first().copied().unwrap_or(0),
                            r.get(1).copied().unwrap_or(0),
                        )
                    })
                    .collect::<Vec<_>>(),
            ));
        }
    }
    let set_sizes: Vec<(String, usize)> = char_sets
        .iter()
        .map(|(n, r)| (n.clone(), r.len()))
        .collect();
    let body = |name: &str| {
        lexers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r.clone())
    };
    let lex_range = body("ts_lex").ok_or_else(|| Error::whole("`ts_lex` is missing"))?;
    let lex = clex::read(
        toks.get(lex_range).unwrap_or_default(),
        constants,
        &set_sizes,
    )?;
    let keyword_lex = match (body("ts_lex_keywords"), has_keyword_lexer) {
        (Some(r), true) => Some(clex::read(
            toks.get(r).unwrap_or_default(),
            constants,
            &set_sizes,
        )?),
        (None, true) => return Err(Error::whole("`ts_lex_keywords` is named but missing")),
        (_, false) => None,
    };

    Ok(Grammar {
        name,
        abi,
        counts,
        keyword_capture_token,
        metadata,
        symbol_names,
        field_names,
        blobs,
        char_sets,
        lex,
        keyword_lex,
        external_tokens,
    })
}

/// Dimension `n` of a declaration, when it is given.
fn dim(d: &Declared, n: usize, constants: &Constants) -> Result<Option<usize>, Error> {
    d.dims
        .get(n)
        .map(|e| {
            usize::try_from(constants.eval(e, d.line)?)
                .map_err(|_| Error::at(d.line, "a negative size"))
        })
        .transpose()
}

/// A one-dimensional array of numbers, `len` long (or as long as its last
/// element), zero where the initializer says nothing.
fn scalars(d: &Declared, len: Option<usize>, constants: &Constants) -> Result<Vec<i64>, Error> {
    let items = cinit::elements(&d.init, constants, d.line)?;
    let len = match (len, dim(d, 0, constants)?) {
        (Some(want), Some(given)) if want != given => {
            return Err(Error::at(
                d.line,
                format!("declared {given} long, counted {want}"),
            ));
        }
        (Some(n), _) | (None, Some(n)) => n,
        (None, None) => items.iter().map(|(i, _)| i + 1).max().unwrap_or(0),
    };
    let mut out = vec![0; len];
    for (index, value) in items {
        let v = match value {
            Init::Expr(e) => constants.eval(e, d.line)?,
            Init::List(_) => cinit::fields(value, &["value"], constants, d.line)?
                .first()
                .copied()
                .unwrap_or(0),
        };
        *out.get_mut(index)
            .ok_or_else(|| Error::at(d.line, format!("element {index} of {len}")))? = v;
    }
    Ok(out)
}

/// A two-dimensional array of numbers, `rows` by `cols`, row after row.
fn grid(d: &Declared, rows: usize, cols: usize, constants: &Constants) -> Result<Vec<i64>, Error> {
    if let Some(given) = dim(d, 0, constants)?
        && given != rows
    {
        return Err(Error::at(
            d.line,
            format!("declared {given} rows, counted {rows}"),
        ));
    }
    let mut out = vec![0; rows * cols];
    for (row, value) in cinit::elements(&d.init, constants, d.line)? {
        if row >= rows {
            return Err(Error::at(d.line, format!("row {row} of {rows}")));
        }
        let cells = cinit::elements(value, constants, d.line)?;
        for (col, cell) in cells {
            if col >= cols {
                return Err(Error::at(d.line, format!("column {col} of {cols}")));
            }
            let Init::Expr(e) = cell else {
                return Err(Error::at(d.line, "a list where a number belongs"));
            };
            if let Some(slot) = out.get_mut(row * cols + col) {
                *slot = constants.eval(e, d.line)?;
            }
        }
    }
    Ok(out)
}

/// An array of structs, each as its fields' values.
fn structs(
    d: &Declared,
    names: &[&str],
    len: Option<usize>,
    constants: &Constants,
) -> Result<Vec<Vec<i64>>, Error> {
    let items = cinit::elements(&d.init, constants, d.line)?;
    let len = match (len, dim(d, 0, constants)?) {
        (Some(want), Some(given)) if want != given => {
            return Err(Error::at(
                d.line,
                format!("declared {given} long, counted {want}"),
            ));
        }
        (Some(n), _) | (None, Some(n)) => n,
        (None, None) => items.iter().map(|(i, _)| i + 1).max().unwrap_or(0),
    };
    let mut out = vec![vec![0; names.len()]; len];
    for (index, value) in items {
        *out.get_mut(index)
            .ok_or_else(|| Error::at(d.line, format!("element {index} of {len}")))? =
            cinit::fields(value, names, constants, d.line)?;
    }
    Ok(out)
}

/// An array of strings (or `NULL`s), `len` long -- each read as C reads a
/// string, up to its first NUL: Go's grammar names a token `"\0"`, which C,
/// and the runtime reading the names as C strings, sees as the empty name.
fn strings(d: &Declared, len: usize, constants: &Constants) -> Result<Vec<Option<Vec<u8>>>, Error> {
    let mut out = vec![None; len];
    for (index, value) in cinit::elements(&d.init, constants, d.line)? {
        let text = match value {
            Init::Expr(Expr::Str(bytes)) => {
                let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                Some(bytes.get(..end).unwrap_or_default().to_vec())
            }
            Init::Expr(Expr::Ident(n)) if n == "NULL" => None,
            _ => return Err(Error::at(d.line, "a name that is not a string")),
        };
        *out.get_mut(index)
            .ok_or_else(|| Error::at(d.line, format!("name {index} of {len}")))? = text;
    }
    Ok(out)
}

/// The parse actions, eight bytes each: `TSParseActionEntry`, a union of an
/// entry header (`count`, `reusable`) and an action -- shift (`type`,
/// `state` at 2, `extra` at 4, `repetition` at 5) or reduce (`type`,
/// `child_count`, `symbol` at 2, `dynamic_precedence` at 4, `production_id`
/// at 6) -- as `tree_sitter/parser.h` declares them.
fn parse_actions(d: &Declared, constants: &Constants) -> Result<Vec<u8>, Error> {
    const SHIFT: u8 = 0;
    const REDUCE: u8 = 1;
    const ACCEPT: u8 = 2;
    const RECOVER: u8 = 3;
    let items = cinit::elements(&d.init, constants, d.line)?;
    let len = items.iter().map(|(i, _)| i + 1).max().unwrap_or(0);
    let mut out = vec![[0u8; 8]; len];
    let u16_of = |v: i64| -> Result<[u8; 2], Error> {
        // A dynamic precedence is signed; everything else is unsigned.
        let v = i16::try_from(v)
            .map(|s| s.to_le_bytes())
            .or_else(|_| u16::try_from(v).map(u16::to_le_bytes));
        v.map_err(|_| Error::at(d.line, "an action's value past 16 bits"))
    };
    for (index, value) in items {
        let mut e = [0u8; 8];
        match value {
            Init::List(_) => {
                let entry = cinit::field(value, "entry")
                    .ok_or_else(|| Error::at(d.line, "an action entry with no `.entry`"))?;
                let v = cinit::fields(entry, &["count", "reusable"], constants, d.line)?;
                e[0] = u8::try_from(v.first().copied().unwrap_or(0))
                    .map_err(|_| Error::at(d.line, "an entry's count past a byte"))?;
                e[1] = u8::from(v.get(1).copied().unwrap_or(0) != 0);
            }
            Init::Expr(Expr::Call(name, args)) => {
                let nums: Vec<i64> = args
                    .iter()
                    .map(|a| constants.eval(a, d.line))
                    .collect::<Result<_, _>>()?;
                match (name.as_str(), nums.as_slice()) {
                    ("SHIFT", [state]) => {
                        e[0] = SHIFT;
                        e[2..4].copy_from_slice(&u16_of(*state)?);
                    }
                    ("SHIFT_REPEAT", [state]) => {
                        e[0] = SHIFT;
                        e[2..4].copy_from_slice(&u16_of(*state)?);
                        e[5] = 1;
                    }
                    ("SHIFT_EXTRA", []) => {
                        e[0] = SHIFT;
                        e[4] = 1;
                    }
                    ("REDUCE", [symbol, children, precedence, production]) => {
                        e[0] = REDUCE;
                        e[1] = u8::try_from(*children).map_err(|_| {
                            Error::at(d.line, "a reduction of more than 255 children")
                        })?;
                        e[2..4].copy_from_slice(&u16_of(*symbol)?);
                        e[4..6].copy_from_slice(&u16_of(*precedence)?);
                        e[6..8].copy_from_slice(&u16_of(*production)?);
                    }
                    ("ACCEPT_INPUT", []) => e[0] = ACCEPT,
                    ("RECOVER", []) => e[0] = RECOVER,
                    _ => {
                        return Err(Error::at(
                            d.line,
                            format!("an action this does not know: {name}{nums:?}"),
                        ));
                    }
                }
            }
            Init::Expr(other) => {
                return Err(Error::at(
                    d.line,
                    format!("an action this does not know: {other:?}"),
                ));
            }
        }
        if let Some(slot) = out.get_mut(index) {
            *slot = e;
        }
    }
    Ok(out.concat())
}

fn u16s(values: &[i64]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(values.len() * 2);
    for v in values {
        let v =
            u16::try_from(*v).map_err(|_| Error::whole(format!("{v} does not fit a u16 table")))?;
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(out)
}

fn u32s(values: &[i64]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        let v =
            u32::try_from(*v).map_err(|_| Error::whole(format!("{v} does not fit a u32 table")))?;
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(out)
}

/// Structs as bytes, each field `widths[i]` bytes wide, little-endian, packed
/// as C packs these (every field is aligned to its own width already, and
/// each struct's size is a multiple of its widest field).
fn bytes_of(rows: &[Vec<i64>], widths: &[usize]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::with_capacity(rows.len() * widths.iter().sum::<usize>());
    for row in rows {
        for (v, w) in row.iter().zip(widths) {
            match w {
                1 => out.push(
                    u8::try_from(*v)
                        .map_err(|_| Error::whole(format!("{v} does not fit a byte")))?,
                ),
                2 => out.extend_from_slice(
                    &u16::try_from(*v)
                        .map_err(|_| Error::whole(format!("{v} does not fit a u16")))?
                        .to_le_bytes(),
                ),
                _ => return Err(Error::whole("a field width this does not write")),
            }
        }
    }
    Ok(out)
}

/// `bytes` as a C string literal for Rust: `c"..."`.
fn c_literal(bytes: &[u8]) -> String {
    let mut out = String::from("c\"");
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b' '..=b'~' => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\x{b:02x}");
            }
        }
    }
    out.push('"');
    out
}

impl Grammar {
    /// The external tokens' names in the scanner's order -- what a
    /// hand-ported scanner's token list must agree with.
    #[must_use]
    pub fn external_tokens(&self) -> &[String] {
        &self.external_tokens
    }

    /// How many symbols the grammar has, aliases included.
    #[must_use]
    pub fn symbol_count(&self) -> usize {
        self.symbol_names.len()
    }

    /// The grammar as Rust, its tables in files beside it: `module` is the
    /// directory under `OUT_DIR` the build script writes them into.
    #[must_use]
    pub fn render(&self, module: &str) -> Output {
        let mut rust = String::new();
        let mut blobs = Vec::new();
        let _ = writeln!(
            rust,
            "// @generated by tsgrammar from the `{}` grammar's parser.c (gui/syntax/build.rs).\n\
             // Do not edit: replace the parser.c and rebuild.\n",
            self.name
        );
        rust.push_str("use crate::ffi::{Lexer, set_contains};\n\n");
        let c = &self.counts;
        let _ = writeln!(
            rust,
            "/// How many external tokens the grammar's scanner produces."
        );
        let _ = writeln!(
            rust,
            "pub(crate) const EXTERNAL_TOKEN_COUNT: usize = {};",
            c.external_token
        );
        let _ = writeln!(
            rust,
            "/// The external tokens, in the order the scanner's `valid` list has them."
        );
        let _ = write!(
            rust,
            "pub(crate) const EXTERNAL_TOKENS: [&str; {}] = [",
            self.external_tokens.len()
        );
        for t in &self.external_tokens {
            let _ = write!(rust, "{t:?}, ");
        }
        rust.push_str("];\n\n");

        // The tables.
        for blob in &self.blobs {
            let ident = blob.field.replace('.', "_").to_ascii_uppercase();
            let file = format!("{}.bin", blob.field.replace('.', "_"));
            let _ = writeln!(
                rust,
                "static {ident}: crate::ffi::Aligned<[u8; {}]> = crate::ffi::Aligned(*include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{module}/{file}\")));",
                blob.bytes.len()
            );
            blobs.push((file, blob.bytes.clone()));
        }
        let names = |list: &[Option<Vec<u8>>]| -> String {
            list.iter()
                .map(|n| {
                    n.as_ref().map_or_else(
                        || "core::ptr::null()".to_owned(),
                        |b| format!("{}.as_ptr()", c_literal(b)),
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            rust,
            "static SYMBOL_NAMES: crate::ffi::SyncPtrs<[*const core::ffi::c_char; {}]> = crate::ffi::SyncPtrs([{}]);",
            self.symbol_names.len(),
            names(&self.symbol_names)
        );
        let _ = writeln!(
            rust,
            "static FIELD_NAMES: crate::ffi::SyncPtrs<[*const core::ffi::c_char; {}]> = crate::ffi::SyncPtrs([{}]);",
            self.field_names.len(),
            names(&self.field_names)
        );
        for (name, ranges) in &self.char_sets {
            let _ = write!(rust, "const {}: &[(i32, i32)] = &[", clex::set_name(name));
            for (a, b) in ranges {
                let _ = write!(rust, "({a}, {b}), ");
            }
            rust.push_str("];\n");
        }
        rust.push('\n');

        // The lexers.
        self.lex.write_rust("lex_main", &mut rust);
        rust.push_str("crate::ffi::lexer_entry!(ts_lex, lex_main);\n");
        if let Some(k) = &self.keyword_lex {
            k.write_rust("lex_keywords", &mut rust);
            rust.push_str("crate::ffi::lexer_entry!(ts_lex_keywords, lex_keywords);\n");
        }
        rust.push('\n');

        // The language.
        let ptr = |field: &str, ty: &str| -> String {
            self.blobs.iter().find(|b| b.field == field).map_or_else(
                || "core::ptr::null()".to_owned(),
                |b| {
                    format!(
                        "{}.0.as_ptr().cast::<{ty}>()",
                        b.field.replace('.', "_").to_ascii_uppercase()
                    )
                },
            )
        };
        let _ = writeln!(rust, "/// The grammar, as the runtime reads it.");
        rust.push_str("pub(crate) static LANGUAGE: crate::ffi::SyncLanguage = crate::ffi::SyncLanguage(crate::ffi::TSLanguage {\n");
        let _ = writeln!(rust, "    abi_version: {},", self.abi);
        let _ = writeln!(rust, "    symbol_count: {},", c.symbol);
        let _ = writeln!(rust, "    alias_count: {},", c.alias);
        let _ = writeln!(rust, "    token_count: {},", c.token);
        let _ = writeln!(rust, "    external_token_count: {},", c.external_token);
        let _ = writeln!(rust, "    state_count: {},", c.state);
        let _ = writeln!(rust, "    large_state_count: {},", c.large_state);
        let _ = writeln!(rust, "    production_id_count: {},", c.production_id);
        let _ = writeln!(rust, "    field_count: {},", c.field);
        let _ = writeln!(
            rust,
            "    max_alias_sequence_length: {},",
            c.max_alias_sequence_length
        );
        let _ = writeln!(rust, "    parse_table: {},", ptr("parse_table", "u16"));
        let _ = writeln!(
            rust,
            "    small_parse_table: {},",
            ptr("small_parse_table", "u16")
        );
        let _ = writeln!(
            rust,
            "    small_parse_table_map: {},",
            ptr("small_parse_table_map", "u32")
        );
        let _ = writeln!(
            rust,
            "    parse_actions: {},",
            ptr("parse_actions", "crate::ffi::ParseActionEntry")
        );
        rust.push_str("    symbol_names: SYMBOL_NAMES.0.as_ptr(),\n");
        if self.field_names.is_empty() {
            rust.push_str("    field_names: core::ptr::null(),\n");
        } else {
            rust.push_str("    field_names: FIELD_NAMES.0.as_ptr(),\n");
        }
        let _ = writeln!(
            rust,
            "    field_map_slices: {},",
            ptr("field_map_slices", "crate::ffi::MapSlice")
        );
        let _ = writeln!(
            rust,
            "    field_map_entries: {},",
            ptr("field_map_entries", "crate::ffi::FieldMapEntry")
        );
        let _ = writeln!(
            rust,
            "    symbol_metadata: {},",
            ptr("symbol_metadata", "crate::ffi::SymbolMetadata")
        );
        let _ = writeln!(
            rust,
            "    public_symbol_map: {},",
            ptr("public_symbol_map", "u16")
        );
        let _ = writeln!(rust, "    alias_map: {},", ptr("alias_map", "u16"));
        let _ = writeln!(
            rust,
            "    alias_sequences: {},",
            ptr("alias_sequences", "u16")
        );
        let _ = writeln!(
            rust,
            "    lex_modes: {},",
            ptr("lex_modes", "crate::ffi::LexerMode")
        );
        rust.push_str("    lex_fn: Some(ts_lex),\n");
        if self.keyword_lex.is_some() {
            rust.push_str("    keyword_lex_fn: Some(ts_lex_keywords),\n");
        } else {
            rust.push_str("    keyword_lex_fn: None,\n");
        }
        let _ = writeln!(
            rust,
            "    keyword_capture_token: {},",
            self.keyword_capture_token
        );
        if self.external_tokens.is_empty() {
            rust.push_str("    external_scanner: crate::ffi::ExternalScannerTable::NONE,\n");
        } else {
            let _ = writeln!(
                rust,
                "    external_scanner: crate::ffi::scanner_table::<Scanner>({}, {}),",
                ptr("external_scanner.states", "bool"),
                ptr("external_scanner.symbol_map", "u16")
            );
        }
        let _ = writeln!(
            rust,
            "    primary_state_ids: {},",
            ptr("primary_state_ids", "u16")
        );
        if self.abi >= 15 {
            let _ = writeln!(
                rust,
                "    name: {}.as_ptr(),",
                c_literal(self.name.as_bytes())
            );
        } else {
            rust.push_str("    name: core::ptr::null(),\n");
        }
        let _ = writeln!(
            rust,
            "    reserved_words: {},",
            ptr("reserved_words", "u16")
        );
        let _ = writeln!(
            rust,
            "    max_reserved_word_set_size: {},",
            c.max_reserved_word_set_size
        );
        let _ = writeln!(
            rust,
            "    supertype_count: {},",
            if self.blobs.iter().any(|b| b.field == "supertype_symbols") {
                c.supertype
            } else {
                0
            }
        );
        let _ = writeln!(
            rust,
            "    supertype_symbols: {},",
            ptr("supertype_symbols", "u16")
        );
        let _ = writeln!(
            rust,
            "    supertype_map_slices: {},",
            ptr("supertype_map_slices", "crate::ffi::MapSlice")
        );
        let _ = writeln!(
            rust,
            "    supertype_map_entries: {},",
            ptr("supertype_map_entries", "u16")
        );
        let [major, minor, patch] = self.metadata.unwrap_or([0, 0, 0]);
        let _ = writeln!(
            rust,
            "    metadata: crate::ffi::LanguageMetadata {{ major_version: {major}, minor_version: {minor}, patch_version: {patch} }},"
        );
        rust.push_str("});\n");
        Output { rust, blobs }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests;
