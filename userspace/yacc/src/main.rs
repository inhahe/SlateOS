//! OurOS `yacc` -- LALR(1) parser generator
//!
//! A yacc/bison-compatible parser generator that reads grammar files in the
//! standard yacc format and produces C source code containing parse tables and
//! a `yyparse()` function.  Supports operator precedence, associativity,
//! %union, %type, error recovery via the special `error` token, %prec for
//! contextual precedence, verbose state output (-v), and YYDEBUG trace mode.
//!
//! Multi-personality: when invoked as `bison` (via argv[0]) it enables
//! extended diagnostics by default.

#![cfg_attr(not(test), no_main)]
// LALR(1) table construction inherently shuffles long argument lists
// (item sets, action/goto tables, terminal/nonterminal indices, ...);
// adding builder structs for the inner helpers would obscure the
// classic algorithm. Allow too_many_arguments at file scope.
#![allow(clippy::too_many_arguments)]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
#[cfg(not(test))]
use std::env;
use std::fmt::Write as FmtWrite;
#[cfg(not(test))]
use std::fs;

// -------------------------------------------------------------------------
// Constants
// -------------------------------------------------------------------------

/// Sentinel token ID for the end-of-input marker (`$end`).
const EOF_TOKEN: usize = 0;
/// Sentinel token ID for the `error` pseudo-token used in error recovery.
const ERROR_TOKEN: usize = 256;

/// Name displayed in usage / version output when invoked as `yacc`.
const YACC_NAME: &str = "yacc";
/// Name displayed when invoked as `bison`.
const BISON_NAME: &str = "bison";

// -------------------------------------------------------------------------
// Associativity / precedence
// -------------------------------------------------------------------------

/// Operator associativity for conflict resolution.
///
/// `NonAssoc` mirrors the `%nonassoc` directive in YACC/Bison grammar
/// files, so we keep the conventional name even though it ends in the
/// enum's own name.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Assoc {
    Left,
    Right,
    NonAssoc,
}

/// A precedence entry: level (higher = tighter) plus associativity.
#[derive(Debug, Clone, Copy)]
struct PrecEntry {
    level: usize,
    assoc: Assoc,
}

// -------------------------------------------------------------------------
// Grammar representation
// -------------------------------------------------------------------------

/// A single symbol in a grammar rule (terminal or non-terminal).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Symbol {
    Terminal(String),
    NonTerminal(String),
    /// The special `error` pseudo-token for error recovery.
    Error,
}

impl Symbol {
    fn name(&self) -> &str {
        match self {
            Symbol::Terminal(s) | Symbol::NonTerminal(s) => s.as_str(),
            Symbol::Error => "error",
        }
    }
}

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Symbol::Terminal(s) => write!(f, "{s}"),
            Symbol::NonTerminal(s) => write!(f, "{s}"),
            Symbol::Error => write!(f, "error"),
        }
    }
}

/// A grammar production: `lhs -> rhs[0] rhs[1] ...` with an optional action.
#[derive(Debug, Clone)]
struct Production {
    lhs: String,
    rhs: Vec<Symbol>,
    action: String,
    /// Explicit precedence from `%prec TAG`.
    prec_tag: Option<String>,
    /// Production number (0-based; production 0 is the augmented start rule).
    number: usize,
}

/// Complete parsed grammar, ready for table generation.
#[derive(Debug)]
struct Grammar {
    /// All productions (index 0 is the augmented start production).
    productions: Vec<Production>,
    /// Map from non-terminal name to the list of production indices.
    nonterminal_prods: HashMap<String, Vec<usize>>,
    /// Set of terminal names.
    terminals: BTreeSet<String>,
    /// Set of non-terminal names.
    nonterminals: BTreeSet<String>,
    /// Precedence/associativity table keyed by terminal name.
    prec_table: HashMap<String, PrecEntry>,
    /// The start symbol (before augmentation).  Used when generating
    /// verbose output and for future extensions.
    #[allow(dead_code)]
    start_symbol: String,
    /// %union body (verbatim C code), if any.
    union_body: Option<String>,
    /// %type declarations mapping type tag -> list of symbol names.
    /// Preserved for future typed code generation.
    #[allow(dead_code)]
    type_decls: Vec<(String, Vec<String>)>,
    /// Token name -> integer value mapping.
    token_values: HashMap<String, usize>,
    /// Verbatim code from the prologue (%{ ... %}) sections.
    prologue: String,
    /// Verbatim code from the epilogue (after the second %%) section.
    epilogue: String,
}

// -------------------------------------------------------------------------
// Lexer for yacc grammar files
// -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum GrammarToken {
    /// `%token`
    DeclToken,
    /// `%left`
    DeclLeft,
    /// `%right`
    DeclRight,
    /// `%nonassoc`
    DeclNonassoc,
    /// `%type`
    DeclType,
    /// `%start`
    DeclStart,
    /// `%union`
    DeclUnion,
    /// `%prec`
    DeclPrec,
    /// `%%`
    DoublePct,
    /// An identifier (terminal or non-terminal name).
    Ident(String),
    /// A single-quoted character literal like `'+'`.
    CharLit(char),
    /// An integer literal (for %token VALUE).
    IntLit(usize),
    /// `<tag>` for typing.
    TypeTag(String),
    /// A C code block `{ ... }`.
    Action(String),
    /// The colon separating LHS from RHS.
    Colon,
    /// The semicolon terminating a rule group.
    Semicolon,
    /// The pipe separating alternative RHS.
    Pipe,
    /// Prologue block `%{ ... %}`.
    Prologue(String),
    /// End of file.
    Eof,
}

/// Simple scanner for yacc grammar source text.
struct GrammarLexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> GrammarLexer<'a> {
    fn new(src: &'a [u8]) -> Self {
        Self {
            src,
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
        }
        Some(b)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            // Skip whitespace
            while let Some(b) = self.peek() {
                if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                    self.advance();
                } else {
                    break;
                }
            }
            // Skip C-style comments
            if self.pos + 1 < self.src.len()
                && self.src[self.pos] == b'/'
                && self.src[self.pos + 1] == b'*'
            {
                self.pos += 2;
                while self.pos + 1 < self.src.len() {
                    if self.src[self.pos] == b'*' && self.src[self.pos + 1] == b'/' {
                        self.pos += 2;
                        break;
                    }
                    if self.src[self.pos] == b'\n' {
                        self.line += 1;
                    }
                    self.pos += 1;
                }
                continue;
            }
            // Skip // comments
            if self.pos + 1 < self.src.len()
                && self.src[self.pos] == b'/'
                && self.src[self.pos + 1] == b'/'
            {
                while let Some(b) = self.peek() {
                    self.advance();
                    if b == b'\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    /// Read a brace-delimited C code block.  Handles nested braces, string
    /// literals, character literals, and comments so that we stop at the
    /// correct closing `}`.
    fn read_action(&mut self) -> Result<String, String> {
        // Caller has already consumed the opening `{`.
        let mut depth: usize = 1;
        let mut buf = String::new();
        while depth > 0 {
            match self.advance() {
                None => return Err(format!("line {}: unterminated action block", self.line)),
                Some(b'{') => {
                    depth += 1;
                    buf.push('{');
                }
                Some(b'}') => {
                    depth -= 1;
                    if depth > 0 {
                        buf.push('}');
                    }
                }
                Some(b'\'') => {
                    buf.push('\'');
                    // character literal
                    if let Some(c) = self.advance() {
                        buf.push(c as char);
                        if c == b'\\'
                            && let Some(c2) = self.advance() {
                                buf.push(c2 as char);
                            }
                    }
                    // closing quote
                    if let Some(b'\'') = self.peek() {
                        self.advance();
                        buf.push('\'');
                    }
                }
                Some(b'"') => {
                    buf.push('"');
                    loop {
                        match self.advance() {
                            None => break,
                            Some(b'"') => {
                                buf.push('"');
                                break;
                            }
                            Some(b'\\') => {
                                buf.push('\\');
                                if let Some(c) = self.advance() {
                                    buf.push(c as char);
                                }
                            }
                            Some(c) => buf.push(c as char),
                        }
                    }
                }
                Some(b'/') if self.peek() == Some(b'*') => {
                    buf.push('/');
                    buf.push('*');
                    self.advance();
                    loop {
                        match self.advance() {
                            None => break,
                            Some(b'*') if self.peek() == Some(b'/') => {
                                self.advance();
                                buf.push('*');
                                buf.push('/');
                                break;
                            }
                            Some(c) => buf.push(c as char),
                        }
                    }
                }
                Some(c) => buf.push(c as char),
            }
        }
        Ok(buf)
    }

    /// Read until `%}` (prologue block).
    fn read_prologue(&mut self) -> Result<String, String> {
        let mut buf = String::new();
        while self.pos + 1 < self.src.len() {
            if self.src[self.pos] == b'%' && self.src[self.pos + 1] == b'}' {
                self.pos += 2;
                return Ok(buf);
            }
            if self.src[self.pos] == b'\n' {
                self.line += 1;
            }
            buf.push(self.src[self.pos] as char);
            self.pos += 1;
        }
        Err(format!("line {}: unterminated %{{ ... %}}", self.line))
    }

    /// Produce the next token.
    fn next_token(&mut self) -> Result<GrammarToken, String> {
        self.skip_whitespace_and_comments();

        if self.pos >= self.src.len() {
            return Ok(GrammarToken::Eof);
        }

        let b = self.src[self.pos];

        // `%` directives
        if b == b'%' {
            if self.pos + 1 < self.src.len() {
                let b2 = self.src[self.pos + 1];
                if b2 == b'%' {
                    self.pos += 2;
                    return Ok(GrammarToken::DoublePct);
                }
                if b2 == b'{' {
                    self.pos += 2;
                    let body = self.read_prologue()?;
                    return Ok(GrammarToken::Prologue(body));
                }
            }
            // keyword directive
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_alphanumeric() {
                self.pos += 1;
            }
            let kw =
                std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("");
            match kw {
                "token" => Ok(GrammarToken::DeclToken),
                "left" => Ok(GrammarToken::DeclLeft),
                "right" => Ok(GrammarToken::DeclRight),
                "nonassoc" => Ok(GrammarToken::DeclNonassoc),
                "type" => Ok(GrammarToken::DeclType),
                "start" => Ok(GrammarToken::DeclStart),
                "union" => Ok(GrammarToken::DeclUnion),
                "prec" => Ok(GrammarToken::DeclPrec),
                _ => Err(format!("line {}: unknown directive %{kw}", self.line)),
            }
        } else if b == b':' {
            self.pos += 1;
            Ok(GrammarToken::Colon)
        } else if b == b';' {
            self.pos += 1;
            Ok(GrammarToken::Semicolon)
        } else if b == b'|' {
            self.pos += 1;
            Ok(GrammarToken::Pipe)
        } else if b == b'{' {
            self.pos += 1;
            let body = self.read_action()?;
            Ok(GrammarToken::Action(body))
        } else if b == b'<' {
            // Type tag: <ident>
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos] != b'>' {
                self.pos += 1;
            }
            let tag = std::str::from_utf8(&self.src[start..self.pos])
                .unwrap_or("")
                .to_string();
            if self.pos < self.src.len() {
                self.pos += 1; // skip '>'
            }
            Ok(GrammarToken::TypeTag(tag))
        } else if b == b'\'' {
            // Character literal
            self.pos += 1;
            let ch = if self.pos < self.src.len() && self.src[self.pos] == b'\\' {
                self.pos += 1;
                
                if self.pos < self.src.len() {
                    let e = self.src[self.pos];
                    self.pos += 1;
                    match e {
                        b'n' => '\n',
                        b't' => '\t',
                        b'\\' => '\\',
                        b'\'' => '\'',
                        b'0' => '\0',
                        other => other as char,
                    }
                } else {
                    return Err(format!("line {}: unterminated char literal", self.line));
                }
            } else if self.pos < self.src.len() {
                let c = self.src[self.pos] as char;
                self.pos += 1;
                c
            } else {
                return Err(format!("line {}: unterminated char literal", self.line));
            };
            if self.pos < self.src.len() && self.src[self.pos] == b'\'' {
                self.pos += 1;
            }
            Ok(GrammarToken::CharLit(ch))
        } else if b.is_ascii_digit() {
            let start = self.pos;
            while self.pos < self.src.len() && self.src[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap_or("0");
            let val = s.parse::<usize>().unwrap_or(0);
            Ok(GrammarToken::IntLit(val))
        } else if b == b'_' || b.is_ascii_alphabetic() {
            let start = self.pos;
            while self.pos < self.src.len()
                && (self.src[self.pos] == b'_'
                    || self.src[self.pos] == b'.'
                    || self.src[self.pos].is_ascii_alphanumeric())
            {
                self.pos += 1;
            }
            let s = std::str::from_utf8(&self.src[start..self.pos])
                .unwrap_or("")
                .to_string();
            Ok(GrammarToken::Ident(s))
        } else {
            // Skip unknown byte
            self.pos += 1;
            self.next_token()
        }
    }
}

// -------------------------------------------------------------------------
// Grammar parser
// -------------------------------------------------------------------------

/// Parse a yacc grammar file from raw bytes and return a `Grammar`.
fn parse_grammar(src: &[u8]) -> Result<Grammar, String> {
    let mut lex = GrammarLexer::new(src);
    let mut terminals = BTreeSet::new();
    let mut nonterminals = BTreeSet::new();
    let mut prec_table: HashMap<String, PrecEntry> = HashMap::new();
    let mut prec_level: usize = 0;
    let mut start_symbol: Option<String> = None;
    let mut union_body: Option<String> = None;
    let mut type_decls: Vec<(String, Vec<String>)> = Vec::new();
    let mut token_values: HashMap<String, usize> = HashMap::new();
    let mut prologue = String::new();
    let mut productions: Vec<Production> = Vec::new();

    // Phase 1: declarations (before the first %%)
    loop {
        let tok = lex.next_token()?;
        match tok {
            GrammarToken::DoublePct => break,
            GrammarToken::Eof => break,
            GrammarToken::Prologue(body) => {
                prologue.push_str(&body);
                prologue.push('\n');
            }
            GrammarToken::DeclToken => {
                let mut tag: Option<String> = None;
                // optional <type>
                let first = lex.next_token()?;
                let mut pending = match first {
                    GrammarToken::TypeTag(t) => {
                        tag = Some(t);
                        None
                    }
                    other => Some(other),
                };
                // read token names
                let mut names: Vec<String> = Vec::new();
                loop {
                    let t = if let Some(p) = pending.take() {
                        p
                    } else {
                        lex.next_token()?
                    };
                    match t {
                        GrammarToken::Ident(name) => {
                            terminals.insert(name.clone());
                            // check for optional value
                            let maybe_val = lex.next_token()?;
                            match maybe_val {
                                GrammarToken::IntLit(v) => {
                                    token_values.insert(name.clone(), v);
                                }
                                other => {
                                    pending = Some(other);
                                }
                            }
                            names.push(name);
                        }
                        GrammarToken::CharLit(ch) => {
                            let name = format!("'{ch}'");
                            terminals.insert(name.clone());
                            names.push(name);
                        }
                        _ => {
                            // Push back: we don't have a real pushback mechanism,
                            // so we'll handle this by re-scanning below.  For
                            // simplicity, we break and re-push by storing in
                            // pending for the outer loop.  Since we're in a
                            // declaration-reading loop, just break and let the
                            // outer loop re-read.
                            //
                            // This is a bit awkward; we handle it by setting
                            // pos backwards.  Instead, break and push back.
                            // We'll use a small pushback buffer on the lexer
                            // side.  For now, handle known cases.
                            //
                            // Actually, let's just break and handle the token
                            // in the next outer iteration by recursing through
                            // a pushback list.
                            //
                            // Simplification: store the token for re-processing.
                            // We'll use a Vec<GrammarToken> pushback.
                            // For now, break and re-dispatch:
                            match &t {
                                GrammarToken::DeclToken
                                | GrammarToken::DeclLeft
                                | GrammarToken::DeclRight
                                | GrammarToken::DeclNonassoc
                                | GrammarToken::DeclType
                                | GrammarToken::DeclStart
                                | GrammarToken::DeclUnion
                                | GrammarToken::DoublePct => {
                                    // These need to be handled by outer loop.
                                    // We'll use a simple approach: store the
                                    // token.  Since we can't push back to the
                                    // lexer easily, we'll use a local pushback.
                                    // For a self-contained solution, just handle
                                    // it inline.
                                    //
                                    // Re-dispatch manually:
                                    if let Some(ref tag_val) = tag {
                                        let syms = names.clone();
                                        if !syms.is_empty() {
                                            type_decls
                                                .push((tag_val.clone(), syms));
                                        }
                                    }
                                    // Now handle the token `t` as if it were
                                    // the next outer-loop token.
                                    return parse_grammar_with_pushback(
                                        lex,
                                        t,
                                        terminals,
                                        nonterminals,
                                        prec_table,
                                        prec_level,
                                        start_symbol,
                                        union_body,
                                        type_decls,
                                        token_values,
                                        prologue,
                                        productions,
                                    );
                                }
                                _ => break,
                            }
                        }
                    }
                }
                if let Some(ref tag_val) = tag
                    && !names.is_empty() {
                        type_decls.push((tag_val.clone(), names));
                    }
            }
            GrammarToken::DeclLeft
            | GrammarToken::DeclRight
            | GrammarToken::DeclNonassoc => {
                prec_level += 1;
                let assoc = match tok {
                    GrammarToken::DeclLeft => Assoc::Left,
                    GrammarToken::DeclRight => Assoc::Right,
                    _ => Assoc::NonAssoc,
                };
                let tag_tok = lex.next_token()?;
                let mut pending: Option<GrammarToken> = None;
                // optional <type>
                let _type_tag = match tag_tok {
                    GrammarToken::TypeTag(t) => Some(t),
                    other => {
                        pending = Some(other);
                        None
                    }
                };
                loop {
                    let t = if let Some(p) = pending.take() {
                        p
                    } else {
                        lex.next_token()?
                    };
                    match t {
                        GrammarToken::Ident(name) => {
                            terminals.insert(name.clone());
                            prec_table.insert(
                                name,
                                PrecEntry {
                                    level: prec_level,
                                    assoc,
                                },
                            );
                        }
                        GrammarToken::CharLit(ch) => {
                            let name = format!("'{ch}'");
                            terminals.insert(name.clone());
                            prec_table.insert(
                                name,
                                PrecEntry {
                                    level: prec_level,
                                    assoc,
                                },
                            );
                        }
                        _ => break,
                    }
                }
            }
            GrammarToken::DeclType => {
                let tag_tok = lex.next_token()?;
                let tag = match tag_tok {
                    GrammarToken::TypeTag(t) => t,
                    _ => {
                        return Err(format!(
                            "line {}: expected <type> after %type",
                            lex.line
                        ))
                    }
                };
                let mut names = Vec::new();
                loop {
                    let t = lex.next_token()?;
                    match t {
                        GrammarToken::Ident(name) => names.push(name),
                        GrammarToken::CharLit(ch) => names.push(format!("'{ch}'")),
                        _ => break,
                    }
                }
                if !names.is_empty() {
                    type_decls.push((tag, names));
                }
            }
            GrammarToken::DeclStart => {
                let t = lex.next_token()?;
                match t {
                    GrammarToken::Ident(name) => {
                        start_symbol = Some(name);
                    }
                    _ => {
                        return Err(format!(
                            "line {}: expected symbol after %start",
                            lex.line
                        ))
                    }
                }
            }
            GrammarToken::DeclUnion => {
                // The union body is delimited by { ... }
                let t = lex.next_token()?;
                match t {
                    GrammarToken::Action(body) => {
                        union_body = Some(body);
                    }
                    _ => {
                        return Err(format!(
                            "line {}: expected {{ after %union",
                            lex.line
                        ))
                    }
                }
            }
            _ => {
                // skip unrecognized top-level tokens
            }
        }
    }

    // Phase 2: grammar rules (between %% and %%)
    parse_rules(
        &mut lex,
        &mut productions,
        &terminals,
        &mut nonterminals,
    )?;

    // Phase 3: epilogue (after second %%)
    let epilogue = if lex.pos < lex.src.len() {
        String::from_utf8_lossy(&lex.src[lex.pos..]).to_string()
    } else {
        String::new()
    };

    // Determine start symbol
    let start = if let Some(s) = start_symbol {
        s
    } else if let Some(p) = productions.first() {
        p.lhs.clone()
    } else {
        return Err("no productions defined".into());
    };

    // Build augmented grammar: insert $accept -> start $end at index 0
    let augmented = Production {
        lhs: "$accept".to_string(),
        rhs: vec![Symbol::NonTerminal(start.clone()), Symbol::Terminal("$end".to_string())],
        action: String::new(),
        prec_tag: None,
        number: 0,
    };
    let mut all_prods = vec![augmented];
    for (i, mut p) in productions.into_iter().enumerate() {
        p.number = i + 1;
        all_prods.push(p);
    }

    nonterminals.insert("$accept".to_string());
    terminals.insert("$end".to_string());

    // Build nonterminal -> production indices map
    let mut nt_prods: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in all_prods.iter().enumerate() {
        nt_prods
            .entry(p.lhs.clone())
            .or_default()
            .push(i);
    }

    Ok(Grammar {
        productions: all_prods,
        nonterminal_prods: nt_prods,
        terminals,
        nonterminals,
        prec_table,
        start_symbol: start,
        union_body,
        type_decls,
        token_values,
        prologue,
        epilogue,
    })
}

/// Continue parsing after a pushback token was encountered in the
/// declaration phase.  This is a helper to avoid complex pushback logic in
/// the lexer.
fn parse_grammar_with_pushback(
    mut lex: GrammarLexer<'_>,
    pushed: GrammarToken,
    mut terminals: BTreeSet<String>,
    mut nonterminals: BTreeSet<String>,
    mut prec_table: HashMap<String, PrecEntry>,
    mut prec_level: usize,
    mut start_symbol: Option<String>,
    mut union_body: Option<String>,
    mut type_decls: Vec<(String, Vec<String>)>,
    mut token_values: HashMap<String, usize>,
    mut prologue: String,
    mut productions: Vec<Production>,
) -> Result<Grammar, String> {
    let mut pushback: Vec<GrammarToken> = vec![pushed];

    loop {
        let tok = if let Some(p) = pushback.pop() {
            p
        } else {
            lex.next_token()?
        };

        match tok {
            GrammarToken::DoublePct => break,
            GrammarToken::Eof => break,
            GrammarToken::Prologue(body) => {
                prologue.push_str(&body);
                prologue.push('\n');
            }
            GrammarToken::DeclToken => {
                // simplified: just read idents
                loop {
                    let t = lex.next_token()?;
                    match t {
                        GrammarToken::Ident(name) => {
                            terminals.insert(name.clone());
                            let maybe_val = lex.next_token()?;
                            if let GrammarToken::IntLit(v) = maybe_val {
                                token_values.insert(name, v);
                            }
                        }
                        GrammarToken::TypeTag(_) => continue,
                        GrammarToken::CharLit(ch) => {
                            terminals.insert(format!("'{ch}'"));
                        }
                        other => {
                            pushback.push(other);
                            break;
                        }
                    }
                }
            }
            GrammarToken::DeclLeft
            | GrammarToken::DeclRight
            | GrammarToken::DeclNonassoc => {
                prec_level += 1;
                let assoc = match tok {
                    GrammarToken::DeclLeft => Assoc::Left,
                    GrammarToken::DeclRight => Assoc::Right,
                    _ => Assoc::NonAssoc,
                };
                loop {
                    let t = lex.next_token()?;
                    match t {
                        GrammarToken::Ident(name) => {
                            terminals.insert(name.clone());
                            prec_table.insert(
                                name,
                                PrecEntry {
                                    level: prec_level,
                                    assoc,
                                },
                            );
                        }
                        GrammarToken::TypeTag(_) => continue,
                        GrammarToken::CharLit(ch) => {
                            let name = format!("'{ch}'");
                            terminals.insert(name.clone());
                            prec_table.insert(
                                name,
                                PrecEntry {
                                    level: prec_level,
                                    assoc,
                                },
                            );
                        }
                        other => {
                            pushback.push(other);
                            break;
                        }
                    }
                }
            }
            GrammarToken::DeclType => {
                let tag_tok = lex.next_token()?;
                if let GrammarToken::TypeTag(tag) = tag_tok {
                    let mut names = Vec::new();
                    loop {
                        let t = lex.next_token()?;
                        match t {
                            GrammarToken::Ident(name) => names.push(name),
                            other => {
                                pushback.push(other);
                                break;
                            }
                        }
                    }
                    if !names.is_empty() {
                        type_decls.push((tag, names));
                    }
                }
            }
            GrammarToken::DeclStart => {
                let t = lex.next_token()?;
                if let GrammarToken::Ident(name) = t {
                    start_symbol = Some(name);
                }
            }
            GrammarToken::DeclUnion => {
                let t = lex.next_token()?;
                if let GrammarToken::Action(body) = t {
                    union_body = Some(body);
                }
            }
            _ => {}
        }
    }

    // Phase 2: grammar rules
    parse_rules(
        &mut lex,
        &mut productions,
        &terminals,
        &mut nonterminals,
    )?;

    // Phase 3: epilogue
    let epilogue = if lex.pos < lex.src.len() {
        String::from_utf8_lossy(&lex.src[lex.pos..]).to_string()
    } else {
        String::new()
    };

    let start = if let Some(s) = start_symbol {
        s
    } else if let Some(p) = productions.first() {
        p.lhs.clone()
    } else {
        return Err("no productions defined".into());
    };

    let augmented = Production {
        lhs: "$accept".to_string(),
        rhs: vec![Symbol::NonTerminal(start.clone()), Symbol::Terminal("$end".to_string())],
        action: String::new(),
        prec_tag: None,
        number: 0,
    };
    let mut all_prods = vec![augmented];
    for (i, mut p) in productions.into_iter().enumerate() {
        p.number = i + 1;
        all_prods.push(p);
    }

    nonterminals.insert("$accept".to_string());
    terminals.insert("$end".to_string());

    let mut nt_prods: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in all_prods.iter().enumerate() {
        nt_prods.entry(p.lhs.clone()).or_default().push(i);
    }

    Ok(Grammar {
        productions: all_prods,
        nonterminal_prods: nt_prods,
        terminals,
        nonterminals,
        prec_table,
        start_symbol: start,
        union_body,
        type_decls,
        token_values,
        prologue,
        epilogue,
    })
}

/// Parse the rules section of the grammar (between the two `%%` markers).
fn parse_rules(
    lex: &mut GrammarLexer<'_>,
    productions: &mut Vec<Production>,
    terminals: &BTreeSet<String>,
    nonterminals: &mut BTreeSet<String>,
) -> Result<(), String> {
    // We loop reading rule groups:  LHS : rhs | rhs ... ;
    let mut current_lhs: Option<String> = None;

    loop {
        let tok = lex.next_token()?;
        match tok {
            GrammarToken::DoublePct | GrammarToken::Eof => break,
            GrammarToken::Ident(name) => {
                // Check if next token is ':', meaning this is a new LHS
                let next = lex.next_token()?;
                if next == GrammarToken::Colon {
                    nonterminals.insert(name.clone());
                    current_lhs = Some(name);
                    // Now read the first RHS
                    let (rhs, action, prec_tag) =
                        read_rhs(lex, terminals, nonterminals)?;
                    if let Some(ref lhs) = current_lhs {
                        productions.push(Production {
                            lhs: lhs.clone(),
                            rhs,
                            action,
                            prec_tag,
                            number: 0,
                        });
                    }
                } else {
                    // Not a new LHS -- probably a continuation.  This is an
                    // error in strict yacc, but we're lenient.
                    return Err(format!(
                        "line {}: expected ':' after rule LHS '{name}'",
                        lex.line
                    ));
                }
            }
            GrammarToken::Pipe => {
                // Alternative RHS for current LHS
                let (rhs, action, prec_tag) =
                    read_rhs(lex, terminals, nonterminals)?;
                if let Some(ref lhs) = current_lhs {
                    productions.push(Production {
                        lhs: lhs.clone(),
                        rhs,
                        action,
                        prec_tag,
                        number: 0,
                    });
                }
            }
            GrammarToken::Semicolon => {
                // End of this rule group; next iteration reads a new LHS.
            }
            _ => {
                // skip
            }
        }
    }
    Ok(())
}

/// Read a single RHS (right-hand side) of a production up to `|`, `;`, or `%%`.
fn read_rhs(
    lex: &mut GrammarLexer<'_>,
    terminals: &BTreeSet<String>,
    nonterminals: &mut BTreeSet<String>,
) -> Result<(Vec<Symbol>, String, Option<String>), String> {
    let mut rhs: Vec<Symbol> = Vec::new();
    let mut action = String::new();
    let mut prec_tag: Option<String> = None;

    loop {
        let tok = lex.next_token()?;
        match tok {
            GrammarToken::Pipe | GrammarToken::Semicolon | GrammarToken::DoublePct | GrammarToken::Eof => {
                // End of this alternative.  The lexer already consumed the
                // delimiter; for Pipe we need to "push back" conceptually
                // but our caller handles Pipe by reading another RHS, and
                // the outer loop handles Semicolon.  Since we already
                // consumed the token, we need the caller to know what
                // terminated us.  We use a bit of a trick: we store the
                // position just before we consumed, but that's complex.
                //
                // Instead, set up parse_rules to handle this by peeking.
                // Actually the simplest approach for yacc-like grammars:
                // return and let the caller re-read.  But we already ate
                // the token.
                //
                // Workaround: the caller's loop will work because:
                //   - After reading an RHS, the outer loop reads the next
                //     token which will be a new Ident (new rule) or Pipe.
                //   - If we consumed a Pipe here, the outer loop would
                //     miss it.
                //
                // Fix: do NOT consume Pipe/Semicolon here; let them be
                // handled by the outer loop.
                //
                // Since we already consumed, manually adjust the lexer
                // position.  This is fragile but works for our simple lexer.
                match tok {
                    GrammarToken::Pipe => {
                        // put back the pipe by decrementing pos
                        lex.pos -= 1;
                    }
                    GrammarToken::Semicolon => {
                        lex.pos -= 1;
                    }
                    GrammarToken::DoublePct => {
                        lex.pos -= 2;
                    }
                    _ => {} // Eof: nothing to undo
                }
                break;
            }
            GrammarToken::Action(body) => {
                action = body;
                // After an action, the RHS is done; break and let outer
                // loop handle the next delimiter.
                break;
            }
            GrammarToken::DeclPrec => {
                // %prec TAG
                let ptok = lex.next_token()?;
                match ptok {
                    GrammarToken::Ident(tag) => {
                        prec_tag = Some(tag);
                    }
                    GrammarToken::CharLit(ch) => {
                        prec_tag = Some(format!("'{ch}'"));
                    }
                    _ => {}
                }
            }
            GrammarToken::Ident(name) => {
                if name == "error" {
                    rhs.push(Symbol::Error);
                } else if terminals.contains(&name) {
                    rhs.push(Symbol::Terminal(name));
                } else {
                    // Assume it's a non-terminal (will be validated later).
                    nonterminals.insert(name.clone());
                    rhs.push(Symbol::NonTerminal(name));
                }
            }
            GrammarToken::CharLit(ch) => {
                rhs.push(Symbol::Terminal(format!("'{ch}'")));
            }
            _ => {
                // skip unknown tokens in RHS
            }
        }
    }
    Ok((rhs, action, prec_tag))
}

// -------------------------------------------------------------------------
// FIRST and FOLLOW set computation
// -------------------------------------------------------------------------

/// Compute FIRST sets for all grammar symbols.
///
/// `first[X]` = set of terminal names that can begin a string derived from `X`.
/// The empty string is represented by the sentinel `""` (empty string in set).
fn compute_first_sets(grammar: &Grammar) -> HashMap<String, BTreeSet<String>> {
    let mut first: HashMap<String, BTreeSet<String>> = HashMap::new();

    // Terminals: FIRST(a) = {a}
    for t in &grammar.terminals {
        let mut s = BTreeSet::new();
        s.insert(t.clone());
        first.insert(t.clone(), s);
    }
    // Error token
    {
        let mut s = BTreeSet::new();
        s.insert("error".to_string());
        first.insert("error".to_string(), s);
    }

    // Non-terminals: initialize empty
    for nt in &grammar.nonterminals {
        first.entry(nt.clone()).or_default();
    }

    // Fixed-point iteration
    let mut changed = true;
    while changed {
        changed = false;
        for prod in &grammar.productions {
            let lhs = &prod.lhs;
            // Compute FIRST of the RHS string
            let rhs_first = first_of_string(&prod.rhs, &first);
            let lhs_set = first.entry(lhs.clone()).or_default();
            for sym in &rhs_first {
                if lhs_set.insert(sym.clone()) {
                    changed = true;
                }
            }
        }
    }

    first
}

/// Compute FIRST of a string of symbols (the RHS of a production).
fn first_of_string(
    symbols: &[Symbol],
    first: &HashMap<String, BTreeSet<String>>,
) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    if symbols.is_empty() {
        result.insert(String::new()); // epsilon
        return result;
    }

    for sym in symbols {
        let sym_name = sym.name();
        if let Some(sym_first) = first.get(sym_name) {
            for f in sym_first {
                if !f.is_empty() {
                    result.insert(f.clone());
                }
            }
            if !sym_first.contains("") {
                return result; // no epsilon => stop
            }
        } else {
            // Unknown symbol -- treat as terminal
            result.insert(sym_name.to_string());
            return result;
        }
    }
    // All symbols can derive epsilon
    result.insert(String::new());
    result
}

/// Compute FOLLOW sets for all non-terminals.
///
/// `follow[A]` = set of terminal names that can appear immediately after `A`
/// in some sentential form.  `$end` is always in `follow[start]`.
fn compute_follow_sets(
    grammar: &Grammar,
    first: &HashMap<String, BTreeSet<String>>,
) -> HashMap<String, BTreeSet<String>> {
    let mut follow: HashMap<String, BTreeSet<String>> = HashMap::new();

    // Initialize
    for nt in &grammar.nonterminals {
        follow.entry(nt.clone()).or_default();
    }
    // $end in FOLLOW(start)
    follow
        .entry("$accept".to_string())
        .or_default()
        .insert("$end".to_string());

    // Fixed-point iteration
    let mut changed = true;
    while changed {
        changed = false;
        for prod in &grammar.productions {
            let lhs = &prod.lhs;
            for (i, sym) in prod.rhs.iter().enumerate() {
                if let Symbol::NonTerminal(nt) = sym {
                    let beta = &prod.rhs[i + 1..];
                    let first_beta = first_of_string(beta, first);

                    let nt_follow = follow.entry(nt.clone()).or_default();
                    for f in &first_beta {
                        if !f.is_empty() && nt_follow.insert(f.clone()) {
                            changed = true;
                        }
                    }

                    // If epsilon in FIRST(beta), add FOLLOW(lhs) to FOLLOW(nt)
                    if first_beta.contains("") {
                        let lhs_follow: Vec<String> = follow
                            .get(lhs)
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .collect();
                        let nt_follow = follow.entry(nt.clone()).or_default();
                        for f in lhs_follow {
                            if nt_follow.insert(f) {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    follow
}

// -------------------------------------------------------------------------
// LR(0) item sets and LALR(1) table construction
// -------------------------------------------------------------------------

/// An LR(0) item: production number + dot position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Item {
    prod: usize,
    dot: usize,
}

/// An LR(0) item set (state).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemSet {
    items: BTreeSet<Item>,
}

// ItemSet construction is done inline; keep the struct simple.

/// Action in the parse table.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    Shift(usize),
    Reduce(usize),
    Accept,
    Error,
}

/// The generated LALR(1) parse tables.
struct ParseTable {
    /// action[state][terminal] -> Action
    action: Vec<HashMap<String, Action>>,
    /// goto_table[state][nonterminal] -> next state
    goto_table: Vec<HashMap<String, usize>>,
    /// Number of states.
    num_states: usize,
}

/// Conflict information for reporting.
#[derive(Debug)]
struct Conflict {
    state: usize,
    symbol: String,
    kind: ConflictKind,
}

#[derive(Debug)]
enum ConflictKind {
    ShiftReduce {
        shift_state: usize,
        reduce_prod: usize,
        resolved: Option<String>,
    },
    ReduceReduce {
        prod1: usize,
        prod2: usize,
    },
}

/// Compute the closure of an item set.
fn closure(items: &BTreeSet<Item>, grammar: &Grammar) -> BTreeSet<Item> {
    let mut result = items.clone();
    let mut worklist: VecDeque<Item> = items.iter().copied().collect();

    while let Some(item) = worklist.pop_front() {
        let prod = &grammar.productions[item.prod];
        if item.dot < prod.rhs.len()
            && let Symbol::NonTerminal(ref nt) = prod.rhs[item.dot]
                && let Some(prods) = grammar.nonterminal_prods.get(nt) {
                    for &pi in prods {
                        let new_item = Item { prod: pi, dot: 0 };
                        if result.insert(new_item) {
                            worklist.push_back(new_item);
                        }
                    }
                }
    }
    result
}

/// Compute the GOTO set: advance dot past `symbol` in `items`.
fn goto_set(
    items: &BTreeSet<Item>,
    symbol: &Symbol,
    grammar: &Grammar,
) -> BTreeSet<Item> {
    let mut kernel = BTreeSet::new();
    for &item in items {
        let prod = &grammar.productions[item.prod];
        if item.dot < prod.rhs.len() && prod.rhs[item.dot] == *symbol {
            kernel.insert(Item {
                prod: item.prod,
                dot: item.dot + 1,
            });
        }
    }
    closure(&kernel, grammar)
}

/// Build all LR(0) item sets (states) and transitions.
fn build_item_sets(grammar: &Grammar) -> (Vec<ItemSet>, Vec<HashMap<String, usize>>) {
    let mut states: Vec<ItemSet> = Vec::new();
    let mut transitions: Vec<HashMap<String, usize>> = Vec::new();
    let mut state_map: HashMap<BTreeSet<Item>, usize> = HashMap::new();

    // Initial state: closure of {[S' -> . S, $]}
    let initial_items = {
        let mut s = BTreeSet::new();
        s.insert(Item { prod: 0, dot: 0 });
        closure(&s, grammar)
    };
    let initial_set = ItemSet {
        items: initial_items.clone(),
    };
    state_map.insert(initial_items, 0);
    states.push(initial_set);
    transitions.push(HashMap::new());

    let mut worklist: VecDeque<usize> = VecDeque::new();
    worklist.push_back(0);

    while let Some(si) = worklist.pop_front() {
        // Collect all symbols after the dot in this state.
        let mut symbols_after_dot: BTreeSet<Symbol> = BTreeSet::new();
        for item in &states[si].items {
            let prod = &grammar.productions[item.prod];
            if item.dot < prod.rhs.len() {
                symbols_after_dot.insert(prod.rhs[item.dot].clone());
            }
        }

        for sym in symbols_after_dot {
            let goto = goto_set(&states[si].items, &sym, grammar);
            if goto.is_empty() {
                continue;
            }

            let target = if let Some(&existing) = state_map.get(&goto) {
                existing
            } else {
                let idx = states.len();
                state_map.insert(goto.clone(), idx);
                states.push(ItemSet { items: goto });
                transitions.push(HashMap::new());
                worklist.push_back(idx);
                idx
            };

            transitions[si].insert(sym.name().to_string(), target);
        }
    }

    (states, transitions)
}

/// Determine the precedence/associativity of a production.
///
/// The precedence is taken from the `%prec` directive if present, otherwise
/// from the rightmost terminal in the RHS.
fn production_prec(
    prod: &Production,
    prec_table: &HashMap<String, PrecEntry>,
) -> Option<PrecEntry> {
    if let Some(ref tag) = prod.prec_tag {
        return prec_table.get(tag).copied();
    }
    // Rightmost terminal
    for sym in prod.rhs.iter().rev() {
        match sym {
            Symbol::Terminal(t) => {
                if let Some(entry) = prec_table.get(t) {
                    return Some(*entry);
                }
            }
            Symbol::Error => {}
            Symbol::NonTerminal(_) => {}
        }
    }
    None
}

/// Build LALR(1) parse tables using SLR(1) lookaheads (FOLLOW sets) as an
/// approximation.  True LALR(1) lookahead propagation would require the
/// DeRemer-Pennello algorithm; SLR(1) is sufficient for the vast majority
/// of practical grammars and much simpler to implement.
fn build_parse_table(
    grammar: &Grammar,
    states: &[ItemSet],
    transitions: &[HashMap<String, usize>],
    follow: &HashMap<String, BTreeSet<String>>,
) -> (ParseTable, Vec<Conflict>) {
    let num_states = states.len();
    let mut action: Vec<HashMap<String, Action>> = vec![HashMap::new(); num_states];
    let mut goto_table: Vec<HashMap<String, usize>> = vec![HashMap::new(); num_states];
    let mut conflicts: Vec<Conflict> = Vec::new();

    for (si, state) in states.iter().enumerate() {
        // GOTO entries for non-terminals
        for (sym, &target) in &transitions[si] {
            if grammar.nonterminals.contains(sym) {
                goto_table[si].insert(sym.clone(), target);
            }
        }

        // Action entries
        for &item in &state.items {
            let prod = &grammar.productions[item.prod];

            if item.dot < prod.rhs.len() {
                // Shift
                let sym = &prod.rhs[item.dot];
                match sym {
                    Symbol::Terminal(_) | Symbol::Error if {
                        let key = sym.name();
                        transitions[si].contains_key(key)
                    } => {
                        let key = sym.name().to_string();
                        let target = transitions[si][&key];
                        let shift_action = Action::Shift(target);

                        if let Some(existing) = action[si].get(&key) {
                            if *existing != shift_action {
                                // Shift-reduce or shift-shift conflict
                                if let Action::Reduce(rp) = existing {
                                    // Try to resolve with precedence
                                    let resolved = try_resolve_sr(
                                        grammar,
                                        &key,
                                        *rp,
                                        target,
                                    );
                                    match resolved {
                                        Some((act, reason)) => {
                                            conflicts.push(Conflict {
                                                state: si,
                                                symbol: key.clone(),
                                                kind: ConflictKind::ShiftReduce {
                                                    shift_state: target,
                                                    reduce_prod: *rp,
                                                    resolved: Some(reason),
                                                },
                                            });
                                            action[si].insert(key, act);
                                        }
                                        None => {
                                            // Default: prefer shift
                                            conflicts.push(Conflict {
                                                state: si,
                                                symbol: key.clone(),
                                                kind: ConflictKind::ShiftReduce {
                                                    shift_state: target,
                                                    reduce_prod: *rp,
                                                    resolved: Some(
                                                        "default shift".into(),
                                                    ),
                                                },
                                            });
                                            action[si].insert(key, shift_action);
                                        }
                                    }
                                }
                                // else: shift-shift is unusual, keep first
                            }
                        } else {
                            action[si].insert(key, shift_action);
                        }
                    }
                    _ => {}
                }
            } else {
                // Reduce (dot at end)
                if item.prod == 0 {
                    // Accept: $accept -> start . $end
                    action[si].insert("$end".to_string(), Action::Accept);
                } else {
                    // Reduce by this production for each terminal in
                    // FOLLOW(lhs).
                    let lhs = &prod.lhs;
                    if let Some(follow_set) = follow.get(lhs) {
                        for la in follow_set {
                            let reduce_action = Action::Reduce(item.prod);
                            if let Some(existing) = action[si].get(la) {
                                if *existing != reduce_action {
                                    match existing {
                                        Action::Shift(target) => {
                                            let resolved = try_resolve_sr(
                                                grammar,
                                                la,
                                                item.prod,
                                                *target,
                                            );
                                            match resolved {
                                                Some((act, reason)) => {
                                                    conflicts.push(Conflict {
                                                        state: si,
                                                        symbol: la.clone(),
                                                        kind: ConflictKind::ShiftReduce {
                                                            shift_state: *target,
                                                            reduce_prod: item.prod,
                                                            resolved: Some(reason),
                                                        },
                                                    });
                                                    action[si].insert(la.clone(), act);
                                                }
                                                None => {
                                                    conflicts.push(Conflict {
                                                        state: si,
                                                        symbol: la.clone(),
                                                        kind: ConflictKind::ShiftReduce {
                                                            shift_state: *target,
                                                            reduce_prod: item.prod,
                                                            resolved: Some(
                                                                "default shift".into(),
                                                            ),
                                                        },
                                                    });
                                                    // default: keep shift
                                                }
                                            }
                                        }
                                        Action::Reduce(other_prod) => {
                                            conflicts.push(Conflict {
                                                state: si,
                                                symbol: la.clone(),
                                                kind: ConflictKind::ReduceReduce {
                                                    prod1: *other_prod,
                                                    prod2: item.prod,
                                                },
                                            });
                                            // Keep the lower-numbered production
                                            if item.prod < *other_prod {
                                                action[si]
                                                    .insert(la.clone(), reduce_action);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            } else {
                                action[si].insert(la.clone(), reduce_action);
                            }
                        }
                    }
                }
            }
        }
    }

    (
        ParseTable {
            action,
            goto_table,
            num_states,
        },
        conflicts,
    )
}

/// Try to resolve a shift-reduce conflict using precedence/associativity.
/// Returns `Some((chosen_action, explanation))` if resolved, `None` otherwise.
fn try_resolve_sr(
    grammar: &Grammar,
    terminal: &str,
    reduce_prod: usize,
    shift_state: usize,
) -> Option<(Action, String)> {
    let prod = &grammar.productions[reduce_prod];
    let prod_prec = production_prec(prod, &grammar.prec_table)?;
    let tok_prec = grammar.prec_table.get(terminal)?;

    if prod_prec.level > tok_prec.level {
        Some((
            Action::Reduce(reduce_prod),
            format!(
                "reduce (prod prec {} > tok prec {})",
                prod_prec.level, tok_prec.level
            ),
        ))
    } else if prod_prec.level < tok_prec.level {
        Some((
            Action::Shift(shift_state),
            format!(
                "shift (tok prec {} > prod prec {})",
                tok_prec.level, prod_prec.level
            ),
        ))
    } else {
        // Same precedence: use associativity
        match tok_prec.assoc {
            Assoc::Left => Some((
                Action::Reduce(reduce_prod),
                "reduce (left assoc)".into(),
            )),
            Assoc::Right => Some((
                Action::Shift(shift_state),
                "shift (right assoc)".into(),
            )),
            Assoc::NonAssoc => Some((Action::Error, "error (nonassoc)".into())),
        }
    }
}

// -------------------------------------------------------------------------
// C code generation
// -------------------------------------------------------------------------

/// Assign integer IDs to terminal symbols for the generated C tables.
fn assign_token_ids(grammar: &Grammar) -> BTreeMap<String, usize> {
    let mut map = BTreeMap::new();
    map.insert("$end".to_string(), EOF_TOKEN);
    map.insert("error".to_string(), ERROR_TOKEN);

    let mut next_id: usize = 257;

    // First, assign any explicitly-valued tokens.
    for (name, &val) in &grammar.token_values {
        map.insert(name.clone(), val);
        if val >= next_id {
            next_id = val + 1;
        }
    }

    // Character literals get their ASCII value.
    for t in &grammar.terminals {
        if t.starts_with('\'') && t.ends_with('\'') && t.len() >= 3 {
            let inner = &t[1..t.len() - 1];
            let ch = if inner.starts_with('\\') {
                match inner.as_bytes().get(1) {
                    Some(b'n') => b'\n',
                    Some(b't') => b'\t',
                    Some(b'\\') => b'\\',
                    Some(b'\'') => b'\'',
                    Some(b'0') => 0,
                    Some(&c) => c,
                    None => 0,
                }
            } else {
                inner.as_bytes().first().copied().unwrap_or(0)
            };
            map.insert(t.clone(), ch as usize);
        }
    }

    // Remaining named tokens
    for t in &grammar.terminals {
        if !map.contains_key(t) {
            map.insert(t.clone(), next_id);
            next_id += 1;
        }
    }

    map
}

// Symbol-to-ID mapping is done inline in generate_c_output via the
// token_ids and nt_ids maps directly.

/// Generate the complete C output for the parser.
fn generate_c_output(
    grammar: &Grammar,
    table: &ParseTable,
    token_ids: &BTreeMap<String, usize>,
    enable_debug: bool,
) -> String {
    let mut out = String::new();

    // Header
    let _ = writeln!(out, "/* Generated by OurOS yacc */");
    let _ = writeln!(out, "#include <stdio.h>");
    let _ = writeln!(out, "#include <stdlib.h>");
    let _ = writeln!(out);

    // Prologue
    if !grammar.prologue.is_empty() {
        let _ = writeln!(out, "/* --- prologue --- */");
        let _ = writeln!(out, "{}", grammar.prologue);
    }

    // YYSTYPE union
    if let Some(ref ub) = grammar.union_body {
        let _ = writeln!(out, "typedef union YYSTYPE {{");
        let _ = writeln!(out, "{ub}");
        let _ = writeln!(out, "}} YYSTYPE;");
    } else {
        let _ = writeln!(out, "#ifndef YYSTYPE");
        let _ = writeln!(out, "typedef int YYSTYPE;");
        let _ = writeln!(out, "#endif");
    }
    let _ = writeln!(out);

    // Token defines
    let _ = writeln!(out, "/* Token definitions */");
    for (name, &val) in token_ids {
        if name == "$end" || name == "error" || name.starts_with('\'') {
            continue;
        }
        let _ = writeln!(out, "#define {name} {val}");
    }
    let _ = writeln!(out);

    // Forward declarations
    let _ = writeln!(out, "extern int yylex(void);");
    let _ = writeln!(out, "extern void yyerror(const char *s);");
    let _ = writeln!(out);

    // YYSTYPE and variables
    let _ = writeln!(out, "YYSTYPE yylval;");
    let _ = writeln!(out, "int yychar;");
    let _ = writeln!(out, "int yynerrs;");
    let _ = writeln!(out);

    // Debug support
    if enable_debug {
        let _ = writeln!(out, "#ifndef YYDEBUG");
        let _ = writeln!(out, "#define YYDEBUG 1");
        let _ = writeln!(out, "#endif");
    } else {
        let _ = writeln!(out, "#ifndef YYDEBUG");
        let _ = writeln!(out, "#define YYDEBUG 0");
        let _ = writeln!(out, "#endif");
    }
    let _ = writeln!(out, "int yydebug = 0;");
    let _ = writeln!(out);

    // Non-terminal ID mapping
    let mut nt_ids: HashMap<String, usize> = HashMap::new();
    for (next_nt, nt) in grammar.nonterminals.iter().enumerate() {
        nt_ids.insert(nt.clone(), 10000 + next_nt);
    }

    // Production tables: yylen[i] = #symbols in RHS of production i,
    // yylhs[i] = LHS non-terminal ID.
    let nprods = grammar.productions.len();
    let _ = writeln!(out, "/* Number of productions: {nprods} */");
    let _ = writeln!(out, "/* Number of states: {} */", table.num_states);
    let _ = writeln!(out);

    // yyr1[i] = LHS symbol of production i (as token/nt id)
    let _ = write!(out, "static const int yyr1[] = {{ ");
    for (i, p) in grammar.productions.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ", ");
        }
        let id = nt_ids.get(&p.lhs).copied().unwrap_or(0);
        let _ = write!(out, "{id}");
    }
    let _ = writeln!(out, " }};");

    // yyr2[i] = number of RHS symbols in production i
    let _ = write!(out, "static const int yyr2[] = {{ ");
    for (i, p) in grammar.productions.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ", ");
        }
        let _ = write!(out, "{}", p.rhs.len());
    }
    let _ = writeln!(out, " }};");
    let _ = writeln!(out);

    // Generate action and goto tables as compact arrays.
    // We'll use a simple encoding: for each state, store (token_id, action_code)
    // pairs.  action_code > 0 means shift to state N, < 0 means reduce by
    // production -N, 0 means accept, INT_MIN means error.

    // Collect all terminal IDs used in action tables
    let mut all_terminals: Vec<(String, usize)> = Vec::new();
    for (name, &id) in token_ids {
        all_terminals.push((name.clone(), id));
    }
    all_terminals.sort_by_key(|(_, id)| *id);

    // Action table as 2D array [state][terminal_index] -> action_code
    let _ = writeln!(out, "#define YY_NSTATES {}", table.num_states);
    let _ = writeln!(out, "#define YY_NPRODS {nprods}");
    let _ = writeln!(out, "#define YY_ACCEPT 0");
    let _ = writeln!(out, "#define YY_ERROR (-32768)");
    let _ = writeln!(out);

    // Encode actions per state as a flat table.  For simplicity, use a
    // list-based encoding that yyparse() walks.
    //
    // yy_action_table[state] = array of { token_id, action_code } pairs,
    // terminated by { -1, 0 }.
    //
    // For compact representation, write all entries into a single array
    // `yy_actions[]` and use `yy_action_idx[state]` to index into it.

    let mut action_entries: Vec<i32> = Vec::new();
    let mut action_idx: Vec<usize> = Vec::new();

    for si in 0..table.num_states {
        action_idx.push(action_entries.len());
        for (tname, &tid) in token_ids {
            if let Some(act) = table.action[si].get(tname) {
                let code = match act {
                    Action::Shift(s) => (*s as i32) + 1, // positive = shift
                    Action::Reduce(r) => -(*r as i32),     // negative = reduce
                    Action::Accept => 0,                    // zero = accept
                    Action::Error => -32768,
                };
                action_entries.push(tid as i32);
                action_entries.push(code);
            }
        }
        action_entries.push(-1); // sentinel
        action_entries.push(0);
    }

    let _ = writeln!(out, "static const int yy_actions[] = {{");
    for (i, &val) in action_entries.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ",");
        }
        if i % 20 == 0 {
            let _ = writeln!(out);
            let _ = write!(out, "  ");
        }
        let _ = write!(out, "{val}");
    }
    let _ = writeln!(out, "\n}};");
    let _ = writeln!(out);

    let _ = writeln!(out, "static const int yy_action_idx[] = {{");
    for (i, &idx) in action_idx.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ",");
        }
        if i % 20 == 0 {
            let _ = writeln!(out);
            let _ = write!(out, "  ");
        }
        let _ = write!(out, "{idx}");
    }
    let _ = writeln!(out, "\n}};");
    let _ = writeln!(out);

    // Goto table: same approach
    let mut goto_entries: Vec<i32> = Vec::new();
    let mut goto_idx: Vec<usize> = Vec::new();

    for si in 0..table.num_states {
        goto_idx.push(goto_entries.len());
        for (nt, &target) in &table.goto_table[si] {
            let nt_id = nt_ids.get(nt).copied().unwrap_or(0) as i32;
            goto_entries.push(nt_id);
            goto_entries.push(target as i32);
        }
        goto_entries.push(-1);
        goto_entries.push(0);
    }

    let _ = writeln!(out, "static const int yy_gotos[] = {{");
    for (i, &val) in goto_entries.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ",");
        }
        if i % 20 == 0 {
            let _ = writeln!(out);
            let _ = write!(out, "  ");
        }
        let _ = write!(out, "{val}");
    }
    let _ = writeln!(out, "\n}};");
    let _ = writeln!(out);

    let _ = writeln!(out, "static const int yy_goto_idx[] = {{");
    for (i, &idx) in goto_idx.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ",");
        }
        if i % 20 == 0 {
            let _ = writeln!(out);
            let _ = write!(out, "  ");
        }
        let _ = write!(out, "{idx}");
    }
    let _ = writeln!(out, "\n}};");
    let _ = writeln!(out);

    // Token name table for debug
    let _ = writeln!(out, "#if YYDEBUG");
    let _ = writeln!(out, "static const char *yy_token_names[] = {{");
    for (i, (name, _)) in all_terminals.iter().enumerate() {
        if i > 0 {
            let _ = write!(out, ",");
        }
        let _ = write!(out, "\n  \"{name}\"");
    }
    let _ = writeln!(out, "\n}};");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);

    // yyparse() function
    let _ = writeln!(out, "#define YYMAXDEPTH 10000");
    let _ = writeln!(out, "#ifndef YYINITDEPTH");
    let _ = writeln!(out, "#define YYINITDEPTH 200");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "int yyparse(void) {{");
    let _ = writeln!(out, "  int yy_state_stack[YYMAXDEPTH];");
    let _ = writeln!(out, "  YYSTYPE yy_val_stack[YYMAXDEPTH];");
    let _ = writeln!(out, "  int yy_top = 0;");
    let _ = writeln!(out, "  int yy_state;");
    let _ = writeln!(out, "  int yy_token;");
    let _ = writeln!(out, "  YYSTYPE yyval;");
    let _ = writeln!(out, "  int yy_errflag = 0;");
    let _ = writeln!(out);
    let _ = writeln!(out, "  yynerrs = 0;");
    let _ = writeln!(out, "  yy_state_stack[0] = 0;");
    let _ = writeln!(out, "  yy_token = yylex();");
    let _ = writeln!(out);
    let _ = writeln!(out, "  for (;;) {{");
    let _ = writeln!(out, "    yy_state = yy_state_stack[yy_top];");
    let _ = writeln!(out, "    const int *p = &yy_actions[yy_action_idx[yy_state]];");
    let _ = writeln!(out, "    int yy_act = YY_ERROR;");
    let _ = writeln!(out, "    while (*p != -1) {{");
    let _ = writeln!(out, "      if (*p == yy_token) {{");
    let _ = writeln!(out, "        yy_act = p[1];");
    let _ = writeln!(out, "        break;");
    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "      p += 2;");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out);
    let _ = writeln!(out, "#if YYDEBUG");
    let _ = writeln!(
        out,
        "    if (yydebug) fprintf(stderr, \"state %d, token %d, action %d\\n\", yy_state, yy_token, yy_act);"
    );
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "    if (yy_act > 0) {{");
    let _ = writeln!(out, "      /* Shift */");
    let _ = writeln!(out, "      yy_top++;");
    let _ = writeln!(out, "      if (yy_top >= YYMAXDEPTH) {{");
    let _ = writeln!(
        out,
        "        yyerror(\"parse stack overflow\");"
    );
    let _ = writeln!(out, "        return 2;");
    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "      yy_state_stack[yy_top] = yy_act - 1;");
    let _ = writeln!(out, "      yy_val_stack[yy_top] = yylval;");
    let _ = writeln!(out, "      yy_token = yylex();");
    let _ = writeln!(out, "      if (yy_errflag > 0) yy_errflag--;");
    let _ = writeln!(out, "    }} else if (yy_act < 0 && yy_act != YY_ERROR) {{");
    let _ = writeln!(out, "      /* Reduce */");
    let _ = writeln!(out, "      int yy_rule = -yy_act;");
    let _ = writeln!(out, "      int yy_len = yyr2[yy_rule];");
    let _ = writeln!(out, "      yyval = yy_val_stack[yy_top - yy_len + 1];");
    let _ = writeln!(out, "      switch (yy_rule) {{");

    // Emit action cases
    for (i, p) in grammar.productions.iter().enumerate() {
        if i == 0 || p.action.is_empty() {
            continue;
        }
        let _ = writeln!(out, "      case {i}: {{");
        // Replace $$ and $N references
        let action = translate_action(&p.action, p.rhs.len());
        let _ = writeln!(out, "        {action}");
        let _ = writeln!(out, "      }} break;");
    }

    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "      yy_top -= yy_len;");
    let _ = writeln!(out, "      yy_state = yy_state_stack[yy_top];");
    let _ = writeln!(out, "      /* GOTO */");
    let _ = writeln!(out, "      int yy_lhs = yyr1[yy_rule];");
    let _ = writeln!(out, "      const int *g = &yy_gotos[yy_goto_idx[yy_state]];");
    let _ = writeln!(out, "      int yy_nstate = -1;");
    let _ = writeln!(out, "      while (*g != -1) {{");
    let _ = writeln!(out, "        if (*g == yy_lhs) {{");
    let _ = writeln!(out, "          yy_nstate = g[1];");
    let _ = writeln!(out, "          break;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "        g += 2;");
    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "      if (yy_nstate < 0) {{");
    let _ = writeln!(out, "        yyerror(\"internal parser error: no goto\");");
    let _ = writeln!(out, "        return 2;");
    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "      yy_top++;");
    let _ = writeln!(out, "      yy_state_stack[yy_top] = yy_nstate;");
    let _ = writeln!(out, "      yy_val_stack[yy_top] = yyval;");
    let _ = writeln!(out, "    }} else if (yy_act == YY_ACCEPT) {{");
    let _ = writeln!(out, "      return 0;");
    let _ = writeln!(out, "    }} else {{");
    let _ = writeln!(out, "      /* Error */");
    let _ = writeln!(out, "      if (yy_errflag == 0) {{");
    let _ = writeln!(out, "        yyerror(\"syntax error\");");
    let _ = writeln!(out, "        yynerrs++;");
    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "      if (yy_errflag < 3) {{");
    let _ = writeln!(out, "        yy_errflag = 3;");
    let _ = writeln!(out, "        /* Error recovery: pop until error shift is possible */");
    let _ = writeln!(out, "        while (yy_top >= 0) {{");
    let _ = writeln!(out, "          yy_state = yy_state_stack[yy_top];");
    let _ = writeln!(out, "          p = &yy_actions[yy_action_idx[yy_state]];");
    let _ = writeln!(out, "          while (*p != -1) {{");
    let _ = writeln!(out, "            if (*p == {ERROR_TOKEN}) {{");
    let _ = writeln!(out, "              if (p[1] > 0) {{");
    let _ = writeln!(out, "                yy_top++;");
    let _ = writeln!(out, "                yy_state_stack[yy_top] = p[1] - 1;");
    let _ = writeln!(out, "                goto yy_continue;");
    let _ = writeln!(out, "              }}");
    let _ = writeln!(out, "            }}");
    let _ = writeln!(out, "            p += 2;");
    let _ = writeln!(out, "          }}");
    let _ = writeln!(out, "          yy_top--;");
    let _ = writeln!(out, "        }}");
    let _ = writeln!(out, "        return 1;");
    let _ = writeln!(out, "      }} else {{");
    let _ = writeln!(out, "        if (yy_token == 0) return 1; /* EOF */");
    let _ = writeln!(out, "        yy_token = yylex();");
    let _ = writeln!(out, "      }}");
    let _ = writeln!(out, "    }}");
    let _ = writeln!(out, "yy_continue:;");
    let _ = writeln!(out, "  }}");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    // Epilogue
    if !grammar.epilogue.is_empty() {
        let _ = writeln!(out, "/* --- epilogue --- */");
        let _ = writeln!(out, "{}", grammar.epilogue);
    }

    out
}

/// Translate `$$` to `yyval` and `$N` to stack references in an action string.
fn translate_action(action: &str, rhs_len: usize) -> String {
    let mut result = String::with_capacity(action.len());
    let bytes = action.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'$' {
                result.push_str("yyval");
                i += 2;
                continue;
            }
            // $N or $-N
            let start = i + 1;
            let mut j = start;
            if j < bytes.len() && bytes[j] == b'-' {
                j += 1;
            }
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start {
                let num_str = std::str::from_utf8(&bytes[start..j]).unwrap_or("0");
                if let Ok(n) = num_str.parse::<i32>() {
                    // $N => yy_val_stack[yy_top - (rhs_len - N)]
                    let offset = rhs_len as i32 - n;
                    if offset >= 0 {
                        let _ = write!(result, "yy_val_stack[yy_top - {offset}]");
                    } else {
                        let abs_off = -offset;
                        let _ = write!(result, "yy_val_stack[yy_top + {abs_of}]", abs_of = abs_off);
                    }
                    i = j;
                    continue;
                }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Generate verbose output describing parser states (for -v flag).
fn generate_verbose_output(
    grammar: &Grammar,
    states: &[ItemSet],
    transitions: &[HashMap<String, usize>],
    table: &ParseTable,
    conflicts: &[Conflict],
) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "Grammar:");
    let _ = writeln!(out);
    for (i, p) in grammar.productions.iter().enumerate() {
        let rhs_str: Vec<String> = p.rhs.iter().map(|s| s.to_string()).collect();
        let rhs_display = if rhs_str.is_empty() {
            "/* empty */".to_string()
        } else {
            rhs_str.join(" ")
        };
        let _ = writeln!(out, "  {i}: {lhs} -> {rhs_display}", lhs = p.lhs);
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "Terminals:");
    for t in &grammar.terminals {
        let _ = writeln!(out, "  {t}");
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "Non-terminals:");
    for nt in &grammar.nonterminals {
        let _ = writeln!(out, "  {nt}");
    }
    let _ = writeln!(out);

    // States
    for (si, state) in states.iter().enumerate() {
        let _ = writeln!(out, "State {si}:");
        for item in &state.items {
            let prod = &grammar.productions[item.prod];
            let mut rhs_parts = Vec::new();
            for (j, sym) in prod.rhs.iter().enumerate() {
                if j == item.dot {
                    rhs_parts.push(".".to_string());
                }
                rhs_parts.push(sym.to_string());
            }
            if item.dot == prod.rhs.len() {
                rhs_parts.push(".".to_string());
            }
            let _ = writeln!(
                out,
                "  [{}] {} -> {}",
                item.prod,
                prod.lhs,
                rhs_parts.join(" ")
            );
        }

        // Transitions
        if !transitions[si].is_empty() {
            let _ = writeln!(out, "  Transitions:");
            for (sym, target) in &transitions[si] {
                let _ = writeln!(out, "    {sym} -> state {target}");
            }
        }

        // Actions
        if !table.action[si].is_empty() {
            let _ = writeln!(out, "  Actions:");
            for (tok, act) in &table.action[si] {
                let act_str = match act {
                    Action::Shift(s) => format!("shift {s}"),
                    Action::Reduce(r) => format!("reduce {r}"),
                    Action::Accept => "accept".to_string(),
                    Action::Error => "error".to_string(),
                };
                let _ = writeln!(out, "    {tok}: {act_str}");
            }
        }

        let _ = writeln!(out);
    }

    // Conflicts
    if !conflicts.is_empty() {
        let _ = writeln!(out, "Conflicts:");
        for c in conflicts {
            match &c.kind {
                ConflictKind::ShiftReduce {
                    shift_state,
                    reduce_prod,
                    resolved,
                } => {
                    let res = resolved.as_deref().unwrap_or("unresolved");
                    let _ = writeln!(
                        out,
                        "  State {}: shift/reduce on '{}' (shift {}, reduce {}) [{}]",
                        c.state, c.symbol, shift_state, reduce_prod, res
                    );
                }
                ConflictKind::ReduceReduce { prod1, prod2 } => {
                    let _ = writeln!(
                        out,
                        "  State {}: reduce/reduce on '{}' (prod {}, prod {})",
                        c.state, c.symbol, prod1, prod2
                    );
                }
            }
        }
    }

    out
}

// -------------------------------------------------------------------------
// Header file generation
// -------------------------------------------------------------------------

/// Generate a C header with token definitions.
fn generate_header(
    grammar: &Grammar,
    token_ids: &BTreeMap<String, usize>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "/* Generated by OurOS yacc */");
    let _ = writeln!(out, "#ifndef YYTOKENTYPE");
    let _ = writeln!(out, "#define YYTOKENTYPE");
    let _ = writeln!(out, "enum yytokentype {{");
    for (name, &val) in token_ids {
        if name == "$end" || name == "error" || name.starts_with('\'') {
            continue;
        }
        let _ = writeln!(out, "  {name} = {val},");
    }
    let _ = writeln!(out, "}};");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);

    if let Some(ref ub) = grammar.union_body {
        let _ = writeln!(out, "typedef union YYSTYPE {{");
        let _ = writeln!(out, "{ub}");
        let _ = writeln!(out, "}} YYSTYPE;");
    } else {
        let _ = writeln!(out, "#ifndef YYSTYPE");
        let _ = writeln!(out, "typedef int YYSTYPE;");
        let _ = writeln!(out, "#endif");
    }
    let _ = writeln!(out, "extern YYSTYPE yylval;");

    out
}

// -------------------------------------------------------------------------
// Command-line driver
// -------------------------------------------------------------------------

/// Parsed command-line options.
struct Options {
    input_file: Option<String>,
    output_file: Option<String>,
    header_file: Option<String>,
    verbose: bool,
    /// Reserved for explicit verbose-file path (e.g. --verbose=FILE).
    #[allow(dead_code)]
    verbose_file: Option<String>,
    debug_mode: bool,
    bison_mode: bool,
}

fn parse_options(args: &[String]) -> Result<Options, String> {
    let bison_mode = args
        .first()
        .map(|a| {
            let lower = a.to_ascii_lowercase();
            lower.ends_with("bison") || lower.ends_with("bison.exe")
        })
        .unwrap_or(false);

    let mut opts = Options {
        input_file: None,
        output_file: None,
        header_file: None,
        verbose: false,
        verbose_file: None,
        debug_mode: false,
        bison_mode,
    };

    let mut i = 1; // skip argv[0]
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-o" => {
                i += 1;
                if i >= args.len() {
                    return Err("-o requires an argument".into());
                }
                opts.output_file = Some(args[i].clone());
            }
            "-d" => {
                opts.header_file = Some(String::new()); // auto-name
            }
            "-v" => {
                opts.verbose = true;
            }
            "-t" => {
                opts.debug_mode = true;
            }
            "--defines" => {
                opts.header_file = Some(String::new());
            }
            "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("--output requires an argument".into());
                }
                opts.output_file = Some(args[i].clone());
            }
            "--verbose" => {
                opts.verbose = true;
            }
            "--debug" => {
                opts.debug_mode = true;
            }
            "--help" | "-h" => {
                let name = if bison_mode { BISON_NAME } else { YACC_NAME };
                eprintln!("Usage: {name} [options] grammar.y");
                eprintln!("Options:");
                eprintln!("  -o FILE    Write parser to FILE");
                eprintln!("  -d         Generate header file with token defs");
                eprintln!("  -v         Verbose output (write .output file)");
                eprintln!("  -t         Enable YYDEBUG trace");
                eprintln!("  --help     Show this help");
                eprintln!("  --version  Show version");
                return Err(String::new()); // signal exit
            }
            "--version" => {
                let name = if bison_mode { BISON_NAME } else { YACC_NAME };
                eprintln!("{name} (OurOS) 0.1.0");
                return Err(String::new());
            }
            _ => {
                if arg.starts_with('-') {
                    // Handle combined options or --defines=FILE
                    if let Some(rest) = arg.strip_prefix("--defines=") {
                        opts.header_file = Some(rest.to_string());
                    } else if let Some(rest) = arg.strip_prefix("--output=") {
                        opts.output_file = Some(rest.to_string());
                    } else if let Some(rest) = arg.strip_prefix("-o") {
                        opts.output_file = Some(rest.to_string());
                    } else {
                        return Err(format!("unknown option: {arg}"));
                    }
                } else {
                    opts.input_file = Some(arg.clone());
                }
            }
        }
        i += 1;
    }

    Ok(opts)
}

/// Derive output file name from input file name.
fn derive_output_name(input: &str) -> String {
    if let Some(stem) = input.strip_suffix(".y") {
        format!("{stem}.tab.c")
    } else if let Some(stem) = input.strip_suffix(".yy") {
        format!("{stem}.tab.c")
    } else {
        format!("{input}.tab.c")
    }
}

/// Derive header file name from output file name.
fn derive_header_name(output: &str) -> String {
    if let Some(stem) = output.strip_suffix(".c") {
        format!("{stem}.h")
    } else {
        format!("{output}.h")
    }
}

/// Derive verbose output file name from input file name.
fn derive_verbose_name(input: &str) -> String {
    if let Some(stem) = input.strip_suffix(".y") {
        format!("{stem}.output")
    } else if let Some(stem) = input.strip_suffix(".yy") {
        format!("{stem}.output")
    } else {
        format!("{input}.output")
    }
}

#[cfg(not(test))]
fn run_main() -> i32 {
    let args: Vec<String> = env::args().collect();
    let opts = match parse_options(&args) {
        Ok(o) => o,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{}: {e}", if args.is_empty() { YACC_NAME } else { &args[0] });
            }
            return if e.is_empty() { 0 } else { 1 };
        }
    };

    let prog_name = if opts.bison_mode { BISON_NAME } else { YACC_NAME };

    let input_file = match opts.input_file {
        Some(ref f) => f.clone(),
        None => {
            eprintln!("{prog_name}: no input file");
            return 1;
        }
    };

    let src = match fs::read(&input_file) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("{prog_name}: cannot read '{input_file}': {e}");
            return 1;
        }
    };

    let grammar = match parse_grammar(&src) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{prog_name}: {e}");
            return 1;
        }
    };

    // Compute FIRST and FOLLOW
    let first = compute_first_sets(&grammar);
    let follow = compute_follow_sets(&grammar, &first);

    // Build item sets and parse table
    let (states, transitions) = build_item_sets(&grammar);
    let (table, conflicts) = build_parse_table(&grammar, &states, &transitions, &follow);

    // Assign token IDs
    let token_ids = assign_token_ids(&grammar);

    // Report unresolved conflicts
    let mut sr_count = 0usize;
    let mut rr_count = 0usize;
    for c in &conflicts {
        match &c.kind {
            ConflictKind::ShiftReduce { resolved, .. } => {
                if resolved.as_deref() == Some("default shift") {
                    sr_count += 1;
                }
            }
            ConflictKind::ReduceReduce { .. } => {
                rr_count += 1;
            }
        }
    }
    if sr_count > 0 {
        eprintln!("{prog_name}: {sr_count} shift/reduce conflict(s)");
    }
    if rr_count > 0 {
        eprintln!("{prog_name}: {rr_count} reduce/reduce conflict(s)");
    }

    // Generate C output
    let c_output = generate_c_output(&grammar, &table, &token_ids, opts.debug_mode);

    let output_file = opts
        .output_file
        .clone()
        .unwrap_or_else(|| derive_output_name(&input_file));

    if let Err(e) = fs::write(&output_file, c_output.as_bytes()) {
        eprintln!("{prog_name}: cannot write '{output_file}': {e}");
        return 1;
    }

    // Generate header if requested
    if let Some(ref hf) = opts.header_file {
        let header_name = if hf.is_empty() {
            derive_header_name(&output_file)
        } else {
            hf.clone()
        };
        let header = generate_header(&grammar, &token_ids);
        if let Err(e) = fs::write(&header_name, header.as_bytes()) {
            eprintln!("{prog_name}: cannot write '{header_name}': {e}");
            return 1;
        }
    }

    // Generate verbose output if requested
    if opts.verbose {
        let verbose_name = derive_verbose_name(&input_file);
        let verbose = generate_verbose_output(
            &grammar,
            &states,
            &transitions,
            &table,
            &conflicts,
        );
        if let Err(e) = fs::write(&verbose_name, verbose.as_bytes()) {
            eprintln!("{prog_name}: cannot write '{verbose_name}': {e}");
            return 1;
        }
    }

    0
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    run_main()
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------- Lexer tests -------

    #[test]
    fn lex_empty_input() {
        let mut lex = GrammarLexer::new(b"");
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Eof);
    }

    #[test]
    fn lex_double_pct() {
        let mut lex = GrammarLexer::new(b"%%");
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DoublePct);
    }

    #[test]
    fn lex_ident() {
        let mut lex = GrammarLexer::new(b"  foo_bar  ");
        assert_eq!(
            lex.next_token().unwrap(),
            GrammarToken::Ident("foo_bar".into())
        );
    }

    #[test]
    fn lex_char_literal() {
        let mut lex = GrammarLexer::new(b"'+'");
        assert_eq!(lex.next_token().unwrap(), GrammarToken::CharLit('+'));
    }

    #[test]
    fn lex_char_literal_escaped() {
        let mut lex = GrammarLexer::new(b"'\\n'");
        assert_eq!(lex.next_token().unwrap(), GrammarToken::CharLit('\n'));
    }

    #[test]
    fn lex_int_literal() {
        let mut lex = GrammarLexer::new(b"42");
        assert_eq!(lex.next_token().unwrap(), GrammarToken::IntLit(42));
    }

    #[test]
    fn lex_directives() {
        let src = b"%token %left %right %nonassoc %type %start %union %prec";
        let mut lex = GrammarLexer::new(src);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclToken);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclLeft);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclRight);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclNonassoc);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclType);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclStart);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclUnion);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclPrec);
    }

    #[test]
    fn lex_punctuation() {
        let mut lex = GrammarLexer::new(b": ; |");
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Colon);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Semicolon);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Pipe);
    }

    #[test]
    fn lex_type_tag() {
        let mut lex = GrammarLexer::new(b"<intval>");
        assert_eq!(
            lex.next_token().unwrap(),
            GrammarToken::TypeTag("intval".into())
        );
    }

    #[test]
    fn lex_action_simple() {
        let mut lex = GrammarLexer::new(b"{ $$ = $1 + $2; }");
        match lex.next_token().unwrap() {
            GrammarToken::Action(body) => {
                assert!(body.contains("$$ = $1 + $2;"));
            }
            other => panic!("expected Action, got {:?}", other),
        }
    }

    #[test]
    fn lex_action_nested_braces() {
        let mut lex = GrammarLexer::new(b"{ if (x) { y(); } }");
        match lex.next_token().unwrap() {
            GrammarToken::Action(body) => {
                assert!(body.contains("if (x) { y(); }"));
            }
            other => panic!("expected Action, got {:?}", other),
        }
    }

    #[test]
    fn lex_prologue() {
        let mut lex = GrammarLexer::new(b"%{ #include <stdio.h> %}");
        match lex.next_token().unwrap() {
            GrammarToken::Prologue(body) => {
                assert!(body.contains("#include <stdio.h>"));
            }
            other => panic!("expected Prologue, got {:?}", other),
        }
    }

    #[test]
    fn lex_c_comment_skipped() {
        let mut lex = GrammarLexer::new(b"/* comment */ FOO");
        assert_eq!(
            lex.next_token().unwrap(),
            GrammarToken::Ident("FOO".into())
        );
    }

    #[test]
    fn lex_cpp_comment_skipped() {
        let mut lex = GrammarLexer::new(b"// comment\nFOO");
        assert_eq!(
            lex.next_token().unwrap(),
            GrammarToken::Ident("FOO".into())
        );
    }

    #[test]
    fn lex_multiple_tokens() {
        let src = b"%token NUM PLUS\n%%\nexpr : NUM ;";
        let mut lex = GrammarLexer::new(src);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DeclToken);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Ident("NUM".into()));
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Ident("PLUS".into()));
        assert_eq!(lex.next_token().unwrap(), GrammarToken::DoublePct);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Ident("expr".into()));
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Colon);
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Ident("NUM".into()));
        assert_eq!(lex.next_token().unwrap(), GrammarToken::Semicolon);
    }

    // ------- Grammar parsing tests -------

    fn minimal_grammar() -> &'static [u8] {
        b"%token NUM\n%%\nexpr : NUM ;\n%%\n"
    }

    #[test]
    fn parse_minimal_grammar() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        assert!(g.terminals.contains("NUM"));
        assert!(g.nonterminals.contains("expr"));
        // prod 0 is augmented, prod 1 is "expr -> NUM"
        assert_eq!(g.productions.len(), 2);
        assert_eq!(g.productions[1].lhs, "expr");
        assert_eq!(g.productions[1].rhs.len(), 1);
    }

    #[test]
    fn parse_start_symbol() {
        let src = b"%token A B\n%start foo\n%%\nfoo : A ;\nbar : B ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert_eq!(g.start_symbol, "foo");
    }

    #[test]
    fn parse_precedence_decls() {
        let src = b"%token PLUS TIMES\n%left PLUS\n%left TIMES\n%%\nexpr : expr PLUS expr | expr TIMES expr | PLUS ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.prec_table.contains_key("PLUS"));
        assert!(g.prec_table.contains_key("TIMES"));
        let plus = g.prec_table.get("PLUS").unwrap();
        let times = g.prec_table.get("TIMES").unwrap();
        assert!(times.level > plus.level);
        assert_eq!(plus.assoc, Assoc::Left);
    }

    #[test]
    fn parse_right_assoc() {
        let src = b"%token EQ\n%right EQ\n%%\nexpr : expr EQ expr | EQ ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let eq = g.prec_table.get("EQ").unwrap();
        assert_eq!(eq.assoc, Assoc::Right);
    }

    #[test]
    fn parse_nonassoc() {
        let src = b"%token CMP\n%nonassoc CMP\n%%\nexpr : expr CMP expr | CMP ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let cmp = g.prec_table.get("CMP").unwrap();
        assert_eq!(cmp.assoc, Assoc::NonAssoc);
    }

    #[test]
    fn parse_union_decl() {
        let src = b"%union { int ival; double dval; }\n%token NUM\n%%\nexpr : NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.union_body.is_some());
        let body = g.union_body.as_ref().unwrap();
        assert!(body.contains("int ival"));
        assert!(body.contains("double dval"));
    }

    #[test]
    fn parse_type_decl() {
        let src = b"%union { int ival; }\n%token <ival> NUM\n%type <ival> expr\n%%\nexpr : NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(!g.type_decls.is_empty());
    }

    #[test]
    fn parse_multiple_alternatives() {
        let src = b"%token A B C\n%%\nfoo : A | B | C ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        // 1 augmented + 3 user productions
        assert_eq!(g.productions.len(), 4);
    }

    #[test]
    fn parse_multiple_rules() {
        let src = b"%token NUM PLUS\n%%\nexpr : expr PLUS term | term ;\nterm : NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.nonterminals.contains("expr"));
        assert!(g.nonterminals.contains("term"));
        // augmented + 2 expr alts + 1 term
        assert_eq!(g.productions.len(), 4);
    }

    #[test]
    fn parse_action_blocks() {
        let src = b"%token NUM\n%%\nexpr : NUM { $$ = $1; } ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.productions[1].action.contains("$$ = $1;"));
    }

    #[test]
    fn parse_char_literal_in_rule() {
        let src = b"%token NUM\n%left '+'\n%%\nexpr : expr '+' NUM | NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.terminals.contains("'+'"));
    }

    #[test]
    fn parse_prec_directive() {
        let src = b"%token NUM UMINUS\n%left '+'\n%left '*'\n%right UMINUS\n%%\nexpr : '-' expr %prec UMINUS | expr '+' expr | expr '*' expr | NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        // Find the production with %prec UMINUS
        let has_prec = g.productions.iter().any(|p| p.prec_tag.as_deref() == Some("UMINUS"));
        assert!(has_prec);
    }

    #[test]
    fn parse_error_token_in_rule() {
        let src = b"%token NUM\n%%\nexpr : NUM | error ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let error_prod = g.productions.iter().find(|p| {
            p.rhs.iter().any(|s| matches!(s, Symbol::Error))
        });
        assert!(error_prod.is_some());
    }

    #[test]
    fn parse_empty_production() {
        let src = b"%token A\n%%\nopt : A | ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        // Should have an empty alternative
        let empty_prod = g.productions.iter().any(|p| p.rhs.is_empty() && p.lhs == "opt");
        assert!(empty_prod);
    }

    #[test]
    fn parse_prologue_section() {
        let src = b"%{ #include <stdio.h>\nint x; %}\n%token NUM\n%%\nexpr : NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.prologue.contains("#include <stdio.h>"));
    }

    #[test]
    fn parse_epilogue_section() {
        let src = b"%token NUM\n%%\nexpr : NUM ;\n%%\nint main() { return yyparse(); }\n";
        let g = parse_grammar(src).unwrap();
        assert!(g.epilogue.contains("int main()"));
    }

    #[test]
    fn parse_token_with_value() {
        let src = b"%token FOO 300\n%%\nexpr : FOO ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        assert_eq!(g.token_values.get("FOO"), Some(&300));
    }

    // ------- Symbol tests -------

    #[test]
    fn symbol_display() {
        let t = Symbol::Terminal("PLUS".into());
        let nt = Symbol::NonTerminal("expr".into());
        let e = Symbol::Error;
        assert_eq!(format!("{t}"), "PLUS");
        assert_eq!(format!("{nt}"), "expr");
        assert_eq!(format!("{e}"), "error");
    }

    #[test]
    fn symbol_name() {
        assert_eq!(Symbol::Terminal("X".into()).name(), "X");
        assert_eq!(Symbol::NonTerminal("Y".into()).name(), "Y");
        assert_eq!(Symbol::Error.name(), "error");
    }

    // ------- FIRST/FOLLOW tests -------

    fn arithmetic_grammar() -> Grammar {
        let src = b"\
%token NUM PLUS TIMES LPAREN RPAREN\n\
%left PLUS\n\
%left TIMES\n\
%%\n\
expr : expr PLUS term | term ;\n\
term : term TIMES factor | factor ;\n\
factor : LPAREN expr RPAREN | NUM ;\n\
%%\n";
        parse_grammar(src).unwrap()
    }

    #[test]
    fn first_set_terminal() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let num_first = first.get("NUM").unwrap();
        assert!(num_first.contains("NUM"));
        assert_eq!(num_first.len(), 1);
    }

    #[test]
    fn first_set_nonterminal_factor() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let factor_first = first.get("factor").unwrap();
        assert!(factor_first.contains("LPAREN"));
        assert!(factor_first.contains("NUM"));
    }

    #[test]
    fn first_set_nonterminal_expr() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let expr_first = first.get("expr").unwrap();
        // expr derives from term derives from factor which starts with LPAREN or NUM
        assert!(expr_first.contains("LPAREN"));
        assert!(expr_first.contains("NUM"));
    }

    #[test]
    fn first_set_with_epsilon() {
        let src = b"%token A B\n%%\nS : A B | ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let s_first = first.get("S").unwrap();
        assert!(s_first.contains("A"));
        assert!(s_first.contains("")); // epsilon
    }

    #[test]
    fn follow_set_start() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let accept_follow = follow.get("$accept").unwrap();
        assert!(accept_follow.contains("$end"));
    }

    #[test]
    fn follow_set_expr() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let expr_follow = follow.get("expr").unwrap();
        assert!(expr_follow.contains("PLUS"));
        assert!(expr_follow.contains("RPAREN"));
    }

    #[test]
    fn follow_set_factor() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let factor_follow = follow.get("factor").unwrap();
        // factor is followed by TIMES (from term -> term TIMES factor)
        // and by FOLLOW(term) which includes PLUS, RPAREN, $end
        assert!(factor_follow.contains("TIMES") || factor_follow.contains("PLUS"));
    }

    // ------- Item set tests -------

    #[test]
    fn build_item_sets_minimal() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let (states, _) = build_item_sets(&g);
        assert!(states.len() >= 2); // at least initial + one goto
    }

    #[test]
    fn closure_initial_state() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let mut initial = BTreeSet::new();
        initial.insert(Item { prod: 0, dot: 0 });
        let cl = closure(&initial, &g);
        // Should include items for all expr productions
        assert!(cl.len() > 1);
    }

    #[test]
    fn goto_set_basic() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let mut initial = BTreeSet::new();
        initial.insert(Item { prod: 0, dot: 0 });
        let cl = closure(&initial, &g);
        let goto = goto_set(&cl, &Symbol::NonTerminal("expr".into()), &g);
        assert!(!goto.is_empty());
    }

    // ------- Parse table tests -------

    #[test]
    fn parse_table_minimal() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        assert!(table.num_states > 0);
        // There should be an accept action somewhere
        let has_accept = table.action.iter().any(|row| {
            row.values().any(|a| matches!(a, Action::Accept))
        });
        assert!(has_accept);
    }

    #[test]
    fn parse_table_has_shifts() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let has_shift = table.action.iter().any(|row| {
            row.values().any(|a| matches!(a, Action::Shift(_)))
        });
        assert!(has_shift);
    }

    #[test]
    fn parse_table_has_reduces() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let has_reduce = table.action.iter().any(|row| {
            row.values().any(|a| matches!(a, Action::Reduce(_)))
        });
        assert!(has_reduce);
    }

    #[test]
    fn parse_table_arithmetic() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        // With proper precedence, conflicts should be resolved
        assert!(table.num_states > 5);
        // All conflicts should be resolved (via precedence)
        for c in &conflicts {
            match &c.kind {
                ConflictKind::ShiftReduce { resolved, .. } => {
                    assert!(resolved.is_some(), "unresolved s/r conflict: {:?}", c);
                }
                ConflictKind::ReduceReduce { .. } => {
                    panic!("unexpected r/r conflict: {:?}", c);
                }
            }
        }
    }

    #[test]
    fn parse_table_goto_entries() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        // There should be goto entries for non-terminals
        let has_goto = table.goto_table.iter().any(|row| !row.is_empty());
        assert!(has_goto);
    }

    // ------- Precedence resolution tests -------

    #[test]
    fn prec_left_assoc_resolves() {
        let src = b"%token NUM\n%left '+'\n%%\nexpr : expr '+' expr | NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (_, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        // All conflicts should be resolved via left associativity
        for c in &conflicts {
            if let ConflictKind::ShiftReduce { resolved, .. } = &c.kind {
                assert!(resolved.is_some());
            }
        }
    }

    #[test]
    fn prec_right_assoc_resolves() {
        let src = b"%token NUM\n%right '='\n%%\nexpr : expr '=' expr | NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (_table, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        // Should resolve to shift for right-assoc
        let resolved_to_shift = conflicts.iter().any(|c| {
            if let ConflictKind::ShiftReduce { resolved, .. } = &c.kind {
                resolved.as_deref() == Some("shift (right assoc)")
            } else {
                false
            }
        });
        // We expect a shift/reduce conflict that is resolved via right assoc
        // (If no conflict, the grammar was too simple to trigger one)
        assert!(conflicts.is_empty() || resolved_to_shift);
    }

    #[test]
    fn prec_higher_wins() {
        let src = b"%token NUM\n%left '+'\n%left '*'\n%%\nexpr : expr '+' expr | expr '*' expr | NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (_, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        // All conflicts should be resolved
        for c in &conflicts {
            if let ConflictKind::ShiftReduce { resolved, .. } = &c.kind {
                assert!(resolved.is_some(), "unresolved: {:?}", c);
            }
        }
    }

    #[test]
    fn production_prec_from_prec_tag() {
        let prec_table: HashMap<String, PrecEntry> = {
            let mut m = HashMap::new();
            m.insert("UMINUS".into(), PrecEntry { level: 3, assoc: Assoc::Right });
            m
        };
        let prod = Production {
            lhs: "expr".into(),
            rhs: vec![Symbol::Terminal("'-'".into()), Symbol::NonTerminal("expr".into())],
            action: String::new(),
            prec_tag: Some("UMINUS".into()),
            number: 1,
        };
        let p = production_prec(&prod, &prec_table);
        assert!(p.is_some());
        assert_eq!(p.unwrap().level, 3);
    }

    #[test]
    fn production_prec_from_rightmost_terminal() {
        let prec_table: HashMap<String, PrecEntry> = {
            let mut m = HashMap::new();
            m.insert("PLUS".into(), PrecEntry { level: 1, assoc: Assoc::Left });
            m
        };
        let prod = Production {
            lhs: "expr".into(),
            rhs: vec![
                Symbol::NonTerminal("expr".into()),
                Symbol::Terminal("PLUS".into()),
                Symbol::NonTerminal("expr".into()),
            ],
            action: String::new(),
            prec_tag: None,
            number: 1,
        };
        let p = production_prec(&prod, &prec_table);
        assert!(p.is_some());
        assert_eq!(p.unwrap().assoc, Assoc::Left);
    }

    // ------- Token ID assignment tests -------

    #[test]
    fn token_ids_include_eof() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let ids = assign_token_ids(&g);
        assert_eq!(ids.get("$end"), Some(&EOF_TOKEN));
    }

    #[test]
    fn token_ids_include_error() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let ids = assign_token_ids(&g);
        assert_eq!(ids.get("error"), Some(&ERROR_TOKEN));
    }

    #[test]
    fn token_ids_named_start_at_257() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let ids = assign_token_ids(&g);
        let num_id = ids.get("NUM").unwrap();
        assert!(*num_id >= 257);
    }

    #[test]
    fn token_ids_char_literal_ascii() {
        let src = b"%token NUM\n%left '+'\n%%\nexpr : expr '+' NUM | NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let ids = assign_token_ids(&g);
        assert_eq!(ids.get("'+'"), Some(&(b'+' as usize)));
    }

    #[test]
    fn token_ids_explicit_value() {
        let src = b"%token FOO 500\n%%\nexpr : FOO ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let ids = assign_token_ids(&g);
        assert_eq!(ids.get("FOO"), Some(&500));
    }

    // ------- Action translation tests -------

    #[test]
    fn translate_dollar_dollar() {
        let result = translate_action("$$ = 42;", 0);
        assert_eq!(result, "yyval = 42;");
    }

    #[test]
    fn translate_dollar_1() {
        let result = translate_action("$$ = $1;", 1);
        assert!(result.contains("yyval"));
        assert!(result.contains("yy_val_stack"));
    }

    #[test]
    fn translate_dollar_2() {
        let result = translate_action("$$ = $1 + $3;", 3);
        assert!(result.contains("yyval"));
        // $1 with rhs_len=3: offset = 3-1 = 2 => yy_val_stack[yy_top - 2]
        assert!(result.contains("yy_val_stack[yy_top - 2]"));
        // $3 with rhs_len=3: offset = 3-3 = 0 => yy_val_stack[yy_top - 0]
        assert!(result.contains("yy_val_stack[yy_top - 0]"));
    }

    #[test]
    fn translate_no_dollars() {
        let result = translate_action("printf(\"hello\");", 0);
        assert_eq!(result, "printf(\"hello\");");
    }

    // ------- Code generation tests -------

    #[test]
    fn codegen_contains_yyparse() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("int yyparse(void)"));
    }

    #[test]
    fn codegen_contains_token_defines() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("#define NUM"));
    }

    #[test]
    fn codegen_contains_yystype() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("YYSTYPE"));
    }

    #[test]
    fn codegen_with_union() {
        let src = b"%union { int ival; }\n%token NUM\n%%\nexpr : NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("union YYSTYPE"));
        assert!(output.contains("int ival"));
    }

    #[test]
    fn codegen_with_debug() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, true);
        assert!(output.contains("#define YYDEBUG 1"));
    }

    #[test]
    fn codegen_prologue_included() {
        let src = b"%{ /* my prologue */ %}\n%token NUM\n%%\nexpr : NUM ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("/* my prologue */"));
    }

    #[test]
    fn codegen_epilogue_included() {
        let src = b"%token NUM\n%%\nexpr : NUM ;\n%%\n/* my epilogue */\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("/* my epilogue */"));
    }

    #[test]
    fn codegen_action_in_switch() {
        let src = b"%token NUM\n%%\nexpr : NUM { $$ = $1; } ;\n%%\n";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("case 1:"));
        assert!(output.contains("yyval"));
    }

    #[test]
    fn codegen_error_recovery() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("yyerror"));
        assert!(output.contains("yy_errflag"));
    }

    #[test]
    fn codegen_tables_present() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &ids, false);
        assert!(output.contains("yyr1[]"));
        assert!(output.contains("yyr2[]"));
        assert!(output.contains("yy_actions[]"));
        assert!(output.contains("yy_gotos[]"));
    }

    // ------- Header generation tests -------

    #[test]
    fn header_contains_enum() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let ids = assign_token_ids(&g);
        let header = generate_header(&g, &ids);
        assert!(header.contains("enum yytokentype"));
        assert!(header.contains("NUM"));
    }

    #[test]
    fn header_contains_extern_yylval() {
        let g = parse_grammar(minimal_grammar()).unwrap();
        let ids = assign_token_ids(&g);
        let header = generate_header(&g, &ids);
        assert!(header.contains("extern YYSTYPE yylval"));
    }

    // ------- Verbose output tests -------

    #[test]
    fn verbose_contains_grammar() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        let verbose = generate_verbose_output(&g, &states, &transitions, &table, &conflicts);
        assert!(verbose.contains("Grammar:"));
        assert!(verbose.contains("expr"));
        assert!(verbose.contains("State 0:"));
    }

    #[test]
    fn verbose_contains_terminals() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        let verbose = generate_verbose_output(&g, &states, &transitions, &table, &conflicts);
        assert!(verbose.contains("Terminals:"));
        assert!(verbose.contains("NUM"));
    }

    #[test]
    fn verbose_contains_nonterminals() {
        let g = arithmetic_grammar();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        let verbose = generate_verbose_output(&g, &states, &transitions, &table, &conflicts);
        assert!(verbose.contains("Non-terminals:"));
    }

    // ------- Option parsing tests -------

    #[test]
    fn options_input_file() {
        let args = vec!["yacc".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.input_file, Some("grammar.y".into()));
    }

    #[test]
    fn options_output_file() {
        let args = vec!["yacc".into(), "-o".into(), "out.c".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.output_file, Some("out.c".into()));
    }

    #[test]
    fn options_verbose() {
        let args = vec!["yacc".into(), "-v".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.verbose);
    }

    #[test]
    fn options_debug() {
        let args = vec!["yacc".into(), "-t".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.debug_mode);
    }

    #[test]
    fn options_header() {
        let args = vec!["yacc".into(), "-d".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.header_file.is_some());
    }

    #[test]
    fn options_bison_mode() {
        let args = vec!["bison".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert!(opts.bison_mode);
    }

    #[test]
    fn options_combined_output() {
        let args = vec!["yacc".into(), "-oout.c".into(), "grammar.y".into()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.output_file, Some("out.c".into()));
    }

    #[test]
    fn options_defines_eq() {
        let args = vec!["yacc".into(), "--defines=hdr.h".into(), "g.y".into()];
        let opts = parse_options(&args).unwrap();
        assert_eq!(opts.header_file, Some("hdr.h".into()));
    }

    // ------- File name derivation tests -------

    #[test]
    fn derive_output_from_y() {
        assert_eq!(derive_output_name("parser.y"), "parser.tab.c");
    }

    #[test]
    fn derive_output_from_yy() {
        assert_eq!(derive_output_name("parser.yy"), "parser.tab.c");
    }

    #[test]
    fn derive_output_other() {
        assert_eq!(derive_output_name("grammar"), "grammar.tab.c");
    }

    #[test]
    fn derive_header_from_c() {
        assert_eq!(derive_header_name("parser.tab.c"), "parser.tab.h");
    }

    #[test]
    fn derive_verbose_from_y() {
        assert_eq!(derive_verbose_name("parser.y"), "parser.output");
    }

    // ------- End-to-end integration test -------

    #[test]
    fn end_to_end_calculator() {
        let src = b"\
%{\n\
#include <stdio.h>\n\
#include <stdlib.h>\n\
%}\n\
\n\
%union {\n\
    int ival;\n\
}\n\
\n\
%token <ival> NUM\n\
%type <ival> expr term factor\n\
\n\
%left '+' '-'\n\
%left '*' '/'\n\
%right UMINUS\n\
\n\
%%\n\
\n\
program : expr { printf(\"%d\\n\", $1); }\n\
        ;\n\
\n\
expr : expr '+' expr { $$ = $1 + $3; }\n\
     | expr '-' expr { $$ = $1 - $3; }\n\
     | expr '*' expr { $$ = $1 * $3; }\n\
     | expr '/' expr { $$ = $1 / $3; }\n\
     | '-' expr %prec UMINUS { $$ = -$2; }\n\
     | '(' expr ')' { $$ = $2; }\n\
     | NUM { $$ = $1; }\n\
     ;\n\
\n\
%%\n\
int main() { return yyparse(); }\n\
";
        let g = parse_grammar(src).unwrap();

        // Verify grammar structure
        assert!(g.terminals.contains("NUM"));
        assert!(g.nonterminals.contains("expr"));
        assert!(g.nonterminals.contains("program"));
        assert!(g.union_body.is_some());
        assert!(!g.prologue.is_empty());
        assert!(g.epilogue.contains("int main()"));

        // Verify precedence
        assert!(g.prec_table.contains_key("'+'"));
        assert!(g.prec_table.contains_key("'*'"));
        assert!(g.prec_table.contains_key("UMINUS"));

        // Build tables
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, conflicts) = build_parse_table(&g, &states, &transitions, &follow);
        let token_ids = assign_token_ids(&g);

        // Generate output
        let output = generate_c_output(&g, &table, &token_ids, true);
        assert!(output.contains("int yyparse(void)"));
        assert!(output.contains("#define NUM"));
        assert!(output.contains("union YYSTYPE"));
        assert!(output.contains("#define YYDEBUG 1"));
        assert!(output.contains("int main()"));

        // Check header
        let header = generate_header(&g, &token_ids);
        assert!(header.contains("NUM"));

        // Check verbose
        let verbose = generate_verbose_output(&g, &states, &transitions, &table, &conflicts);
        assert!(verbose.contains("Grammar:"));
        assert!(verbose.contains("State 0:"));
    }

    #[test]
    fn end_to_end_error_recovery_grammar() {
        let src = b"\
%token NUM SEMI\n\
%%\n\
stmts : stmts stmt\n\
      | stmt\n\
      ;\n\
stmt : expr SEMI\n\
     | error SEMI\n\
     ;\n\
expr : NUM\n\
     ;\n\
%%\n\
";
        let g = parse_grammar(src).unwrap();
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        let token_ids = assign_token_ids(&g);
        let output = generate_c_output(&g, &table, &token_ids, false);
        assert!(output.contains("yyparse"));
    }

    // ------- first_of_string edge case -------

    #[test]
    fn first_of_empty_string() {
        let first: HashMap<String, BTreeSet<String>> = HashMap::new();
        let result = first_of_string(&[], &first);
        assert!(result.contains(""));
    }

    // ------- Item Eq/Ord -------

    #[test]
    fn item_equality() {
        let a = Item { prod: 1, dot: 2 };
        let b = Item { prod: 1, dot: 2 };
        let c = Item { prod: 1, dot: 3 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    // ------- Large grammar test -------

    #[test]
    fn large_grammar_many_rules() {
        // Grammar with many terminals and productions to stress-test table gen
        let mut src = String::from("%token ");
        for i in 0..20 {
            if i > 0 {
                src.push(' ');
            }
            let _ = write!(src, "T{i}");
        }
        src.push_str("\n%%\n");
        src.push_str("start : ");
        for i in 0..20 {
            if i > 0 {
                src.push_str(" | ");
            }
            let _ = write!(src, "T{i}");
        }
        src.push_str(" ;\n%%\n");

        let g = parse_grammar(src.as_bytes()).unwrap();
        assert_eq!(g.productions.len(), 21); // 1 augmented + 20 alternatives
        let first = compute_first_sets(&g);
        let follow = compute_follow_sets(&g, &first);
        let (states, transitions) = build_item_sets(&g);
        let (table, _) = build_parse_table(&g, &states, &transitions, &follow);
        assert!(table.num_states > 0);
    }
}
