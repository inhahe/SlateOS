//! Recursive-descent parser: tokens → [`ast::Program`].
//!
//! The parser also lowers lexer [`Seg`]s into [`ast::WordPart`]s, recursively
//! parsing command/parameter substitutions (their raw inner source is captured
//! by the lexer).

use crate::ast::{
    AndOr, AndOrOp, ArrayElem, ArrayIndex, AssignRhs, Assignment, BulkOp, CaseClause, CaseItem,
    CaseTerm,
    Command,
    CondBinOp,
    CondExpr, ForArithClause, ForClause, FunctionDef, IfClause, Item, LoopClause, ParamOp,
    Pipeline, Program,
    Redirect, RedirectOp, ReplaceAnchor, SelectClause, SimpleCommand, UnaryOp, Word, WordPart,
};
use crate::lexer::{Op, Seg, Tok, expand_aliases, tokenize, tokenize_spanned};
use std::collections::BTreeMap;

/// A parse error with a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parse shell source into a [`Program`].
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse(src: &str) -> Result<Program, ParseError> {
    let (toks, lines) = tokenize_spanned(src).map_err(|e| ParseError(e.0))?;
    parse_tokens(toks, lines)
}

/// Parse shell source, expanding shell aliases over the token stream first.
///
/// # Errors
/// Returns [`ParseError`] on a lexing or grammar error.
pub fn parse_with_aliases(
    src: &str,
    aliases: &BTreeMap<String, String>,
) -> Result<Program, ParseError> {
    let (toks, lines) = tokenize_spanned(src).map_err(|e| ParseError(e.0))?;
    let (toks, lines) = if aliases.is_empty() {
        (toks, lines)
    } else {
        expand_aliases(&toks, &lines, aliases)
    };
    parse_tokens(toks, lines)
}

fn parse_tokens(toks: Vec<Tok>, lines: Vec<u32>) -> Result<Program, ParseError> {
    let mut p = Parser { toks, lines, pos: 0 };
    let prog = p.parse_program(&[], true)?;
    if p.pos != p.toks.len() {
        // Leftover tokens — typically an unmatched `)` or a stray reserved
        // word. bash names the offending token (`near unexpected token \`)'`).
        return Err(p.unexpected_here());
    }
    Ok(prog)
}

struct Parser {
    toks: Vec<Tok>,
    /// Parallel to `toks`: the 1-based source line each token starts on, as
    /// computed by the lexer. Read via [`Parser::cur_line`] and stamped onto
    /// each [`Item`] to drive `$LINENO` and error line numbers. Using per-token
    /// lines (rather than counting `Newline` tokens) keeps line numbers correct
    /// across newlines swallowed inside here-docs, quoted strings, and command
    /// substitutions.
    lines: Vec<u32>,
    pos: usize,
}

/// Reserved words that terminate a command list or introduce a compound.
const RESERVED: &[&str] = &[
    "if", "then", "elif", "else", "fi", "while", "until", "do", "done", "for", "in", "{", "}",
    "!", "case", "esac", "select",
];

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    /// The 1-based source line of the current token. At end of input, falls back
    /// to the last token's line (or 1 for empty input), so an item that reaches
    /// EOF still reports a sensible line.
    fn cur_line(&self) -> u32 {
        self.lines
            .get(self.pos)
            .or_else(|| self.lines.last())
            .copied()
            .unwrap_or(1)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn at_op(&self, op: Op) -> bool {
        matches!(self.peek(), Some(Tok::Op(o)) if *o == op)
    }

    /// If the current token is an unquoted single-literal word, return it.
    fn reserved_here(&self) -> Option<String> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
            && RESERVED.contains(&s.as_str())
        {
            return Some(s.clone());
        }
        None
    }

    /// The literal text of a bare word token (single unquoted literal), if any.
    fn bare_word_here(&self) -> Option<String> {
        if let Some(Tok::Word(segs)) = self.peek()
            && let [Seg::Lit(s)] = segs.as_slice()
        {
            return Some(s.clone());
        }
        None
    }

    /// A short human-readable name for the current token, for syntax-error
    /// messages (mirrors bash's `near unexpected token '…'`).
    fn token_display(&self) -> String {
        match self.peek() {
            None => "end of input".to_string(),
            Some(Tok::Newline) => "newline".to_string(),
            Some(Tok::Op(op)) => match op {
                Op::DSemi => ";;",
                Op::SemiAmp => ";&",
                Op::DSemiAmp => ";;&",
                Op::LParen => "(",
                Op::RParen => ")",
                Op::Pipe => "|",
                Op::Amp => "&",
                Op::Semi => ";",
                Op::AndIf => "&&",
                Op::OrIf => "||",
                _ => "redirection",
            }
            .to_string(),
            _ => self.bare_word_here().unwrap_or_else(|| "word".to_string()),
        }
    }

    /// Build bash's canonical "unexpected" parser diagnostic for the current
    /// position: at end of input it is `syntax error: unexpected end of file`;
    /// otherwise `syntax error near unexpected token \`TOKEN'` — bash quotes the
    /// offending token with a leading backtick and a trailing single quote.
    fn unexpected_here(&self) -> ParseError {
        if self.peek().is_none() {
            ParseError("syntax error: unexpected end of file".to_string())
        } else {
            ParseError(format!(
                "syntax error near unexpected token `{}'",
                self.token_display()
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Tok::Newline)) {
            self.pos += 1;
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.peek(),
            Some(Tok::Newline) | Some(Tok::Op(Op::Semi))
        ) {
            self.pos += 1;
        }
    }

    /// Parse a command list until EOF or one of `stops` (reserved words) or a
    /// closing `)`. When `allow_empty` is false — every compound-command
    /// condition/body, subshell, and brace group — an empty list is a syntax
    /// error, matching bash (`if ; then`, `( )`, `while true; do done`). Only the
    /// top-level program and command substitutions (`$( )`) pass `true`. A bare
    /// separator (`;`/`&`) with no preceding command is likewise rejected
    /// (`; echo`, `echo a ; ; echo b`) — blank *lines* between commands are fine.
    fn parse_program(&mut self, stops: &[&str], allow_empty: bool) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        loop {
            // Blank lines between commands are fine; a bare `;`/`&` is not — it
            // denotes an empty command, which bash rejects.
            self.skip_newlines();
            if self.peek().is_none() || self.at_op(Op::RParen) {
                break;
            }
            if let Some(w) = self.reserved_here()
                && stops.contains(&w.as_str())
            {
                break;
            }
            if self.at_op(Op::Semi) || self.at_op(Op::Amp) {
                return Err(self.unexpected_here());
            }
            // Stamp the line on which this item begins (the lexer already
            // accounts for any newlines hidden inside earlier tokens).
            let line = self.cur_line();
            let list = self.parse_and_or()?;
            let mut background = false;
            let mut had_sep = false;
            match self.peek() {
                Some(Tok::Op(Op::Amp)) => {
                    background = true;
                    had_sep = true;
                    self.pos += 1;
                }
                Some(Tok::Newline) => {
                    had_sep = true;
                    self.pos += 1;
                }
                Some(Tok::Op(Op::Semi)) => {
                    had_sep = true;
                    self.pos += 1;
                }
                _ => {}
            }
            items.push(Item { list, background, line });
            // Without a separator (`;`, `&`, newline), the only valid follower is
            // a terminator for this context: end of input, a closing `)`, or a
            // stop keyword (`done`, `fi`, `esac`, `}`, …). Anything else — a bare
            // word or a stray reserved word/operator — means two commands abut
            // with no separator, which bash rejects as a syntax error (and which
            // osh previously mis-ran as a second command).
            if !had_sep {
                let at_terminator = self.peek().is_none()
                    || self.at_op(Op::RParen)
                    || self
                        .reserved_here()
                        .is_some_and(|w| stops.contains(&w.as_str()));
                if !at_terminator {
                    return Err(self.unexpected_here());
                }
            }
        }
        if items.is_empty() && !allow_empty {
            // A compound condition/body reduced to nothing (`if ; then`, `( )`,
            // `then fi`). bash reports the token that follows (the stop keyword,
            // `)`, or EOF).
            return Err(self.unexpected_here());
        }
        Ok(Program { items })
    }

    fn parse_and_or(&mut self) -> Result<AndOr, ParseError> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            let op = match self.peek() {
                Some(Tok::Op(Op::AndIf)) => AndOrOp::And,
                Some(Tok::Op(Op::OrIf)) => AndOrOp::Or,
                _ => break,
            };
            self.pos += 1;
            self.skip_newlines();
            let pipe = self.parse_pipeline()?;
            rest.push((op, pipe));
        }
        Ok(AndOr { first, rest })
    }

    fn parse_pipeline(&mut self) -> Result<Pipeline, ParseError> {
        // `time [-p]` is a reserved word only at the start of a pipeline; it is
        // not in RESERVED (so it stays usable as a plain word elsewhere, e.g.
        // in a `for … in` list). It precedes an optional `!` negation, and is
        // only a keyword when a pipeline body follows it.
        let mut timed = false;
        let mut time_posix = false;
        if self.bare_word_here().as_deref() == Some("time")
            && self.starts_command(self.pos + 1)
        {
            timed = true;
            self.pos += 1;
            self.skip_newlines();
            if self.bare_word_here().as_deref() == Some("-p")
                && self.starts_command(self.pos + 1)
            {
                time_posix = true;
                self.pos += 1;
                self.skip_newlines();
            }
        }
        let mut negated = false;
        if self.reserved_here().as_deref() == Some("!") {
            negated = true;
            self.pos += 1;
        }
        let mut commands = vec![self.parse_command()?];
        loop {
            // `cmd1 | cmd2` and `cmd1 |& cmd2`. The `|&` form is bash shorthand
            // for `cmd1 2>&1 | cmd2`: the *left* command additionally dups its
            // stderr onto stdout before the pipe carries both.
            let amp = if self.at_op(Op::Pipe) {
                false
            } else if self.at_op(Op::PipeAmp) {
                true
            } else {
                break;
            };
            self.pos += 1;
            if amp && let Some(prev) = commands.pop() {
                let dup = Redirect {
                    fd: 2,
                    op: RedirectOp::DupOut,
                    target: Word::literal("1"),
                    varfd: None,
                };
                commands.push(attach_redirect(prev, dup));
            }
            self.skip_newlines();
            commands.push(self.parse_command()?);
        }
        Ok(Pipeline { negated, timed, time_posix, commands })
    }

    /// Whether the token at `idx` could begin a command (used to decide whether
    /// a bare `time`/`-p` at pipeline start is the reserved word or an argv
    /// word — e.g. bare `time` at end of input is just the external `time`).
    fn starts_command(&self, idx: usize) -> bool {
        matches!(
            self.toks.get(idx),
            Some(Tok::Word(_)) | Some(Tok::Op(Op::LParen))
        )
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        if let Some(w) = self.reserved_here() {
            let cmd = match w.as_str() {
                "if" => self.parse_if()?,
                "while" => self.parse_loop(false)?,
                "until" => self.parse_loop(true)?,
                "for" => self.parse_for()?,
                "select" => self.parse_select()?,
                "case" => self.parse_case()?,
                "{" => self.parse_brace_group()?,
                _ => {
                    // A command that begins with a stray closing/continuation
                    // keyword (`then`, `do`, `fi`, `done`, `esac`, `else`, …):
                    // bash reports it as an unexpected token.
                    return Err(self.unexpected_here());
                }
            };
            return self.with_redirects(cmd);
        }
        if self.at_op(Op::LParen) {
            let cmd = self.parse_subshell()?;
            return self.with_redirects(cmd);
        }
        // `(( expr ))` arithmetic command (lexed as a single token).
        if let Some(Tok::ArithCmd(raw)) = self.peek() {
            let raw = raw.clone();
            self.pos += 1;
            return self.with_redirects(Command::Arith(raw));
        }
        // `[[ expr ]]` conditional expression.
        if self.bare_word_here().as_deref() == Some("[[") {
            let cmd = self.parse_cond()?;
            return self.with_redirects(cmd);
        }
        // Function definition: `name ( )`.
        if let Some(name) = self.bare_word_here()
            && is_valid_name(&name)
            && matches!(self.toks.get(self.pos + 1), Some(Tok::Op(Op::LParen)))
            && matches!(self.toks.get(self.pos + 2), Some(Tok::Op(Op::RParen)))
        {
            self.pos += 3;
            self.skip_newlines();
            let body = self.parse_compound_body()?;
            // bash allows redirections after the body (`f() { …; } >log`); they
            // are stored with the function and applied on every invocation.
            let mut redirects = Vec::new();
            while self.at_redirect_start() {
                redirects.push(self.parse_redirect()?);
            }
            return Ok(Command::Function(FunctionDef { name, body, redirects }));
        }
        // `function NAME [()] body` — bash keyword form of a function
        // definition (recognised only at command start).
        if self.bare_word_here().as_deref() == Some("function") {
            return self.parse_function_keyword();
        }
        // `coproc [NAME] command` — bash reserved word, recognised only at
        // command start.
        if self.bare_word_here().as_deref() == Some("coproc") {
            return self.parse_coproc();
        }
        self.parse_simple()
    }

    /// Parse the bash keyword form of a function definition:
    /// `function NAME [( )] compound-body`. Unlike the POSIX `NAME ( )` form,
    /// the parentheses are optional and bash permits function names that are
    /// not valid identifiers (e.g. `function foo-bar { …; }`), so the name is
    /// taken verbatim from the following word.
    fn parse_function_keyword(&mut self) -> Result<Command, ParseError> {
        self.pos += 1; // consume `function`
        let Some(name) = self.bare_word_here() else {
            return Err(self.unexpected_here());
        };
        self.pos += 1; // consume the name word
        // Optional `()` after the name.
        if self.at_op(Op::LParen) {
            if !matches!(self.toks.get(self.pos + 1), Some(Tok::Op(Op::RParen))) {
                return Err(self.unexpected_here());
            }
            self.pos += 2;
        }
        self.skip_newlines();
        let body = self.parse_compound_body()?;
        // bash allows redirections after the body (`function f { …; } >log`);
        // they are stored with the function and applied on every invocation.
        let mut redirects = Vec::new();
        while self.at_redirect_start() {
            redirects.push(self.parse_redirect()?);
        }
        Ok(Command::Function(FunctionDef { name, body, redirects }))
    }

    /// Parse a `coproc [NAME] command`. Grammar (matches bash):
    /// - `coproc simple_command` → default name `COPROC` (an explicit NAME is
    ///   *not* accepted before a simple command).
    /// - `coproc NAME compound_command` → explicit NAME (only when a valid
    ///   identifier is immediately followed by a compound-command starter).
    /// - `coproc compound_command` → default name `COPROC`.
    fn parse_coproc(&mut self) -> Result<Command, ParseError> {
        self.pos += 1; // consume `coproc`
        let mut name = None;
        if let Some(w) = self.bare_word_here()
            && is_valid_name(&w)
            && self.compound_starts_at(self.pos + 1)
        {
            name = Some(w);
            self.pos += 1;
        }
        let body = self.parse_command()?;
        Ok(Command::Coproc { name, body: Box::new(body) })
    }

    /// Whether the token at `idx` begins a compound command (`{`, `(`, `((`,
    /// `[[`, or a control keyword). Used to decide whether the word after
    /// `coproc` is an explicit array name or the command itself.
    fn compound_starts_at(&self, idx: usize) -> bool {
        match self.toks.get(idx) {
            Some(Tok::Op(Op::LParen)) | Some(Tok::ArithCmd(_)) => true,
            Some(Tok::Word(segs)) => {
                if let [Seg::Lit(s)] = segs.as_slice() {
                    matches!(
                        s.as_str(),
                        "{" | "[[" | "if" | "while" | "until" | "for" | "select" | "case"
                    )
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Is the current token the start of a redirection (`<`, `>`, `>>`, `2>`, …)?
    fn at_redirect_start(&self) -> bool {
        matches!(
            self.peek(),
            Some(Tok::Io(_))
                | Some(Tok::VarFd(_))
                | Some(Tok::Op(
                    Op::Less
                        | Op::Great
                        | Op::GreatPipe
                        | Op::DGreat
                        | Op::GreatAnd
                        | Op::LessAnd
                        | Op::LessGreat
                        | Op::AmpGreat
                        | Op::AmpDGreat
                        | Op::DLess
                        | Op::DLessDash
                        | Op::TLess,
                ))
        )
    }

    /// Attach any trailing redirections to a compound command, wrapping it in a
    /// [`Command::Redirected`] when at least one is present.
    fn with_redirects(&mut self, inner: Command) -> Result<Command, ParseError> {
        let mut redirects = Vec::new();
        while self.at_redirect_start() {
            redirects.push(self.parse_redirect()?);
        }
        if redirects.is_empty() {
            Ok(inner)
        } else {
            Ok(Command::Redirected {
                inner: Box::new(inner),
                redirects,
            })
        }
    }

    /// Parse a `{ … }` or `( … )` body used as a function body.
    fn parse_compound_body(&mut self) -> Result<Program, ParseError> {
        if let Some(w) = self.reserved_here()
            && w == "{"
            && let Command::BraceGroup(p) = self.parse_brace_group()?
        {
            return Ok(p);
        }
        if self.at_op(Op::LParen)
            && let Command::Subshell(p) = self.parse_subshell()?
        {
            return Ok(p);
        }
        // Not a valid compound body. bash diagnoses this positionally: at EOF
        // (`f()` / `function f` with no body) it reports "unexpected end of
        // file"; otherwise it names the offending token (`f() echo hi` →
        // "unexpected token `echo'"), matching both function-definition forms.
        Err(self.unexpected_here())
    }

    fn parse_brace_group(&mut self) -> Result<Command, ParseError> {
        // Consume `{`.
        self.pos += 1;
        let body = self.parse_program(&["}"], false)?;
        self.expect_reserved("}")?;
        Ok(Command::BraceGroup(body))
    }

    fn parse_subshell(&mut self) -> Result<Command, ParseError> {
        // Consume `(`.
        self.pos += 1;
        let body = self.parse_program(&[], false)?;
        if !self.at_op(Op::RParen) {
            return Err(ParseError("expected ')'".into()));
        }
        self.pos += 1;
        Ok(Command::Subshell(body))
    }

    fn parse_if(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("if")?;
        let cond = self.parse_program(&["then"], false)?;
        self.expect_reserved("then")?;
        let body = self.parse_program(&["elif", "else", "fi"], false)?;
        let mut elifs = Vec::new();
        while self.reserved_here().as_deref() == Some("elif") {
            self.pos += 1;
            let c = self.parse_program(&["then"], false)?;
            self.expect_reserved("then")?;
            let b = self.parse_program(&["elif", "else", "fi"], false)?;
            elifs.push((c, b));
        }
        let else_body = if self.reserved_here().as_deref() == Some("else") {
            self.pos += 1;
            Some(self.parse_program(&["fi"], false)?)
        } else {
            None
        };
        self.expect_reserved("fi")?;
        Ok(Command::If(IfClause {
            cond,
            body,
            elifs,
            else_body,
        }))
    }

    fn parse_loop(&mut self, until: bool) -> Result<Command, ParseError> {
        self.expect_reserved(if until { "until" } else { "while" })?;
        let cond = self.parse_program(&["do"], false)?;
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::Loop(LoopClause { until, cond, body }))
    }

    fn parse_for(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("for")?;
        // C-style `for (( init; cond; update ))` — the `(( … ))` lexes as a
        // single `ArithCmd` token carrying the raw `init; cond; update` text.
        if let Some(Tok::ArithCmd(raw)) = self.peek() {
            let raw = raw.clone();
            self.pos += 1;
            return self.parse_for_arith(&raw);
        }
        let Some(var) = self.bare_word_here() else {
            // `for` with no loop variable (`for; do …`, `for` at EOF, `for |`):
            // bash names the unexpected token / reports end of input.
            return Err(self.unexpected_here());
        };
        if !is_valid_name(&var) {
            return Err(ParseError(format!("`{var}': not a valid identifier")));
        }
        self.pos += 1;
        self.skip_newlines();
        let words = if self.reserved_here().as_deref() == Some("in") {
            self.pos += 1;
            let mut ws = Vec::new();
            while let Some(Tok::Word(segs)) = self.peek() {
                // Stop at reserved words like `do`.
                if let [Seg::Lit(s)] = segs.as_slice()
                    && RESERVED.contains(&s.as_str())
                {
                    break;
                }
                let segs = segs.clone();
                self.pos += 1;
                ws.push(self.word_from_segs(&segs)?);
            }
            self.skip_separators();
            Some(ws)
        } else {
            self.skip_separators();
            None
        };
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::For(ForClause { var, words, body }))
    }

    /// Parse `select name [in words]; do body; done`. Structurally identical to
    /// the word-list `for` loop; the runtime difference is the interactive menu.
    fn parse_select(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("select")?;
        let Some(var) = self.bare_word_here() else {
            return Err(self.unexpected_here());
        };
        if !is_valid_name(&var) {
            return Err(ParseError(format!("`{var}': not a valid identifier")));
        }
        self.pos += 1;
        self.skip_newlines();
        let words = if self.reserved_here().as_deref() == Some("in") {
            self.pos += 1;
            let mut ws = Vec::new();
            while let Some(Tok::Word(segs)) = self.peek() {
                if let [Seg::Lit(s)] = segs.as_slice()
                    && RESERVED.contains(&s.as_str())
                {
                    break;
                }
                let segs = segs.clone();
                self.pos += 1;
                ws.push(self.word_from_segs(&segs)?);
            }
            self.skip_separators();
            Some(ws)
        } else {
            self.skip_separators();
            None
        };
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::Select(SelectClause { var, words, body }))
    }

    /// Parse the body of a C-style `for (( init; cond; update ))` loop, given
    /// the raw `init; cond; update` text captured from the arithmetic token.
    /// The three sections are split on `;`; an omitted section is empty (an
    /// empty condition is treated as always-true at run time).
    fn parse_for_arith(&mut self, raw: &str) -> Result<Command, ParseError> {
        let parts: Vec<&str> = raw.split(';').collect();
        if parts.len() != 3 {
            return Err(ParseError(
                "C-style for loop requires 'for (( init; cond; update ))'".into(),
            ));
        }
        let init = parts[0].trim().to_string();
        let cond = parts[1].trim().to_string();
        let update = parts[2].trim().to_string();
        // An optional separator (`;`/newline) may precede `do`.
        self.skip_separators();
        self.expect_reserved("do")?;
        let body = self.parse_program(&["done"], false)?;
        self.expect_reserved("done")?;
        Ok(Command::ForArith(ForArithClause {
            init,
            cond,
            update,
            body,
        }))
    }

    fn parse_case(&mut self) -> Result<Command, ParseError> {
        self.expect_reserved("case")?;
        let Some(Tok::Word(segs)) = self.peek() else {
            return Err(ParseError("expected word after 'case'".into()));
        };
        let word = self.word_from_segs(&segs.clone())?;
        self.pos += 1;
        self.skip_newlines();
        self.expect_reserved("in")?;
        self.skip_newlines();
        let mut items = Vec::new();
        while self.reserved_here().as_deref() != Some("esac") {
            if self.peek().is_none() {
                // Unterminated `case` at end of input: bash reports
                // `syntax error: unexpected end of file`.
                return Err(self.unexpected_here());
            }
            // Optional leading '(' before the pattern list.
            if self.at_op(Op::LParen) {
                self.pos += 1;
            }
            // Pattern list: word ['|' word]*.
            let mut patterns = Vec::new();
            loop {
                let Some(Tok::Word(segs)) = self.peek() else {
                    return Err(ParseError("expected pattern in 'case'".into()));
                };
                patterns.push(self.word_from_segs(&segs.clone())?);
                self.pos += 1;
                if self.at_op(Op::Pipe) {
                    self.pos += 1;
                    continue;
                }
                break;
            }
            if !self.at_op(Op::RParen) {
                return Err(ParseError("expected ')' after 'case' pattern".into()));
            }
            self.pos += 1;
            let body = self.parse_case_body()?;
            // Determine the arm terminator: `;;` break, `;&` fall through,
            // `;;&` continue matching. A `;;`-less arm before `esac` breaks.
            let term = if self.at_op(Op::DSemiAmp) {
                self.pos += 1;
                self.skip_newlines();
                CaseTerm::ContinueMatch
            } else if self.at_op(Op::SemiAmp) {
                self.pos += 1;
                self.skip_newlines();
                CaseTerm::FallThrough
            } else if self.at_op(Op::DSemi) {
                self.pos += 1;
                self.skip_newlines();
                CaseTerm::Break
            } else {
                // Only `esac` may legitimately follow a terminator-less arm body.
                self.skip_newlines();
                CaseTerm::Break
            };
            items.push(CaseItem { patterns, body, term });
        }
        self.expect_reserved("esac")?;
        Ok(Command::Case(CaseClause { word, items }))
    }

    /// Parse a `case`-arm body: a command list terminated by `;;` or `esac`.
    fn parse_case_body(&mut self) -> Result<Program, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none()
                || self.at_op(Op::DSemi)
                || self.at_op(Op::SemiAmp)
                || self.at_op(Op::DSemiAmp)
                || self.reserved_here().as_deref() == Some("esac")
            {
                break;
            }
            let line = self.cur_line();
            let list = self.parse_and_or()?;
            let mut background = false;
            match self.peek() {
                Some(Tok::Op(Op::Amp)) => {
                    background = true;
                    self.pos += 1;
                }
                Some(Tok::Newline) => {
                    self.pos += 1;
                }
                Some(Tok::Op(Op::Semi)) => {
                    self.pos += 1;
                }
                _ => {}
            }
            items.push(Item { list, background, line });
        }
        Ok(Program { items })
    }

    /// Parse a `[[ … ]]` conditional expression. The opening `[[` word is at
    /// the current position; parsing stops at the matching `]]` word.
    fn parse_cond(&mut self) -> Result<Command, ParseError> {
        // Consume `[[`.
        self.pos += 1;
        let expr = self.parse_cond_or()?;
        if self.bare_word_here().as_deref() != Some("]]") {
            return Err(ParseError("expected ']]' to close '[['".into()));
        }
        self.pos += 1;
        Ok(Command::Cond(expr))
    }

    fn parse_cond_or(&mut self) -> Result<CondExpr, ParseError> {
        let mut left = self.parse_cond_and()?;
        while self.at_op(Op::OrIf) {
            self.pos += 1;
            let right = self.parse_cond_and()?;
            left = CondExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_and(&mut self) -> Result<CondExpr, ParseError> {
        let mut left = self.parse_cond_not()?;
        while self.at_op(Op::AndIf) {
            self.pos += 1;
            let right = self.parse_cond_not()?;
            left = CondExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_cond_not(&mut self) -> Result<CondExpr, ParseError> {
        if self.bare_word_here().as_deref() == Some("!") {
            self.pos += 1;
            let inner = self.parse_cond_not()?;
            return Ok(CondExpr::Not(Box::new(inner)));
        }
        self.parse_cond_primary()
    }

    fn parse_cond_primary(&mut self) -> Result<CondExpr, ParseError> {
        // Parenthesised sub-expression.
        if self.at_op(Op::LParen) {
            self.pos += 1;
            let inner = self.parse_cond_or()?;
            if !self.at_op(Op::RParen) {
                return Err(ParseError("expected ')' in '[[ … ]]'".into()));
            }
            self.pos += 1;
            return Ok(inner);
        }
        // Unary operator: `-f WORD`, `-z WORD`, …
        if let Some(text) = self.bare_word_here()
            && let Some(op) = unary_op_from(&text)
        {
            self.pos += 1;
            let operand = self.expect_cond_word()?;
            return Ok(CondExpr::Unary(op, operand));
        }
        // Otherwise: WORD [ binop WORD ].
        let left = self.expect_cond_word()?;
        if let Some(op) = self.peek_cond_binop() {
            self.advance_cond_binop();
            let right = self.expect_cond_word()?;
            if matches!(op, RawBinOp::Regex) {
                return Ok(CondExpr::Regex(Box::new(left), Box::new(right)));
            }
            return Ok(CondExpr::Binary(
                Box::new(left),
                op.into_bin_op(),
                Box::new(right),
            ));
        }
        Ok(CondExpr::Word(left))
    }

    /// Expect a word operand inside `[[ … ]]` (not an operator/closer).
    fn expect_cond_word(&mut self) -> Result<Word, ParseError> {
        if let Some(Tok::Word(segs)) = self.peek() {
            let segs = segs.clone();
            // `]]` is the closer, never an operand.
            if let [Seg::Lit(s)] = segs.as_slice()
                && s == "]]"
            {
                return Err(ParseError("unexpected ']]' (expected operand)".into()));
            }
            self.pos += 1;
            return self.word_from_segs(&segs);
        }
        Err(ParseError("expected operand in '[[ … ]]'".into()))
    }

    /// Peek at a binary operator following an operand, without consuming.
    fn peek_cond_binop(&self) -> Option<RawBinOp> {
        match self.peek() {
            Some(Tok::Op(Op::Less)) => Some(RawBinOp::StrLt),
            Some(Tok::Op(Op::Great)) => Some(RawBinOp::StrGt),
            Some(Tok::Word(segs)) => {
                if let [Seg::Lit(s)] = segs.as_slice() {
                    raw_binop_from(s)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn advance_cond_binop(&mut self) {
        self.pos += 1;
    }

    fn parse_simple(&mut self) -> Result<Command, ParseError> {
        let mut cmd = SimpleCommand {
            // Stamp the line the command begins on (its first token), so the
            // interpreter can report the exact line of this command — matching
            // bash's per-command `$LINENO` even inside a multi-line pipeline.
            line: self.cur_line(),
            ..SimpleCommand::default()
        };
        let mut seen_word = false;
        loop {
            match self.peek() {
                Some(Tok::Word(segs)) => {
                    // A reserved word ends the simple command (unless it's an
                    // argument position where reserved words are plain words —
                    // but at the start of a command a reserved word was already
                    // dispatched, so here we only stop for list terminators).
                    if !seen_word
                        && let [Seg::Lit(s)] = segs.as_slice()
                        && RESERVED.contains(&s.as_str())
                    {
                        break;
                    }
                    let segs = segs.clone();
                    // Assignment only valid before the first word.
                    if !seen_word
                        && let Some(a) = self.try_assignment(&segs)?
                    {
                        self.pos += 1;
                        cmd.assignments.push(a);
                        continue;
                    }
                    self.pos += 1;
                    cmd.words.push(self.word_from_segs(&segs)?);
                    seen_word = true;
                }
                Some(Tok::ArrayAssign { .. }) => {
                    // After the command word, an array literal is only allowed as
                    // an operand of a declaration builtin (`declare -A m=([k]=v)`);
                    // anywhere else it's a syntax error.
                    let is_decl_operand = seen_word && is_declaration_command(&cmd.words);
                    if seen_word && !is_decl_operand {
                        return Err(ParseError(
                            "array assignment is only valid before the command word".into(),
                        ));
                    }
                    let Some(Tok::ArrayAssign {
                        name,
                        append,
                        elems,
                    }) = self.bump()
                    else {
                        unreachable!("peek matched ArrayAssign");
                    };
                    let mut items = Vec::with_capacity(elems.len());
                    for segs in &elems {
                        items.push(parse_array_elem(segs)?);
                    }
                    let assign = Assignment {
                        name,
                        index: None,
                        append,
                        value: AssignRhs::Array(items),
                    };
                    if is_decl_operand {
                        cmd.decl_arrays.push(assign);
                    } else {
                        cmd.assignments.push(assign);
                    }
                }
                Some(Tok::Io(_))
                | Some(Tok::VarFd(_))
                | Some(Tok::Op(
                    Op::Less
                    | Op::Great
                    | Op::GreatPipe
                    | Op::DGreat
                    | Op::GreatAnd
                    | Op::LessAnd
                    | Op::LessGreat
                    | Op::AmpGreat
                    | Op::AmpDGreat
                    | Op::DLess
                    | Op::DLessDash
                    | Op::TLess,
                )) => {
                    let r = self.parse_redirect()?;
                    cmd.redirects.push(r);
                }
                _ => break,
            }
        }
        if cmd.words.is_empty() && cmd.assignments.is_empty() && cmd.redirects.is_empty() {
            // A command position that reduced to nothing — e.g. the right side of
            // a dangling pipe (`echo a | | echo b`) or a stray operator. bash
            // names the offending token / reports end of input.
            return Err(self.unexpected_here());
        }
        Ok(Command::Simple(cmd))
    }

    fn parse_redirect(&mut self) -> Result<Redirect, ParseError> {
        // A varfd prefix `{name}` (`{fd}>file`) takes the place of a numeric fd:
        // the executor allocates a free fd ≥ 10 at runtime and binds `name` to it.
        let varfd = if let Some(Tok::VarFd(name)) = self.peek() {
            let name = name.clone();
            self.pos += 1;
            Some(name)
        } else {
            None
        };
        let explicit_fd = if let Some(Tok::Io(n)) = self.peek() {
            let n = *n;
            self.pos += 1;
            Some(n)
        } else {
            None
        };
        // `>&` (GreatAnd) is `RedirectOp::DupOut` when its target is a numeric fd
        // (`>&1`) or `-` (`>&-`), but redirects *both* stdout and stderr to a
        // file when the target is a filename (`>&file`). We resolve that after
        // parsing the target below.
        let mut was_great_and = false;
        let op = match self.bump() {
            Some(Tok::Op(Op::Less)) => RedirectOp::Read,
            Some(Tok::Op(Op::Great)) => RedirectOp::Write,
            Some(Tok::Op(Op::GreatPipe)) => RedirectOp::Clobber,
            Some(Tok::Op(Op::DGreat)) => RedirectOp::Append,
            Some(Tok::Op(Op::GreatAnd)) => {
                was_great_and = true;
                RedirectOp::DupOut
            }
            Some(Tok::Op(Op::LessAnd)) => RedirectOp::DupIn,
            Some(Tok::Op(Op::LessGreat)) => RedirectOp::ReadWrite,
            Some(Tok::Op(Op::AmpGreat)) => RedirectOp::WriteBoth,
            Some(Tok::Op(Op::AmpDGreat)) => RedirectOp::AppendBoth,
            Some(Tok::Op(Op::DLess | Op::DLessDash)) => RedirectOp::HereDoc,
            Some(Tok::Op(Op::TLess)) => RedirectOp::HereStr,
            _ => return Err(ParseError("expected redirection operator".into())),
        };
        let fd = explicit_fd.unwrap_or(match op {
            RedirectOp::Read
            | RedirectOp::HereDoc
            | RedirectOp::HereStr
            | RedirectOp::DupIn
            | RedirectOp::ReadWrite => 0,
            _ => 1,
        });
        let target = match self.bump() {
            Some(Tok::Word(segs)) => self.word_from_segs(&segs)?,
            // The lexer emits the here-doc body as its own token right after the
            // `<<`/`<<-` operator. (Its swallowed body lines are already
            // accounted for by the lexer's per-token line stamping.)
            Some(Tok::HereDoc(segs)) => self.word_from_segs(&segs)?,
            _ => return Err(ParseError("expected redirection target".into())),
        };
        // `>&file` (non-numeric *literal* target, no explicit/var fd) means
        // "both fds to file". A `{v}>&…` form keeps its dup semantics (varfd is
        // not "both"). When the target contains expansions (`>&$v`) we cannot
        // classify it at parse time — it must be resolved at runtime: a numeric
        // expansion is a dup, a non-numeric one is an ambiguous redirect (or
        // "both to file" for the `1>&` corner). So keep it as `DupOut` and let
        // `resolve_redirects` decide.
        let target_is_literal = target
            .parts
            .iter()
            .all(|p| matches!(p, WordPart::Literal(_)));
        let op = if was_great_and
            && explicit_fd.is_none()
            && varfd.is_none()
            && target_is_literal
            && !dup_target_is_fd(&target)
        {
            RedirectOp::WriteBoth
        } else {
            op
        };
        Ok(Redirect { fd, op, target, varfd })
    }

    fn expect_reserved(&mut self, w: &str) -> Result<(), ParseError> {
        if self.reserved_here().as_deref() == Some(w) {
            self.pos += 1;
            Ok(())
        } else {
            // A missing closing keyword (`fi`/`done`/`then`/`esac`/`}`): bash
            // does not name the *expected* word — it reports the token actually
            // found (`syntax error near unexpected token \`done'`) or, at end of
            // input, `unexpected end of file`.
            Err(self.unexpected_here())
        }
    }

    /// Recognise `NAME=value`, `NAME+=value`, or `NAME[index]=value` (before the
    /// first command word).
    fn try_assignment(&self, segs: &[Seg]) -> Result<Option<Assignment>, ParseError> {
        let Some(Seg::Lit(first)) = segs.first() else {
            return Ok(None);
        };
        // A subscript containing expansions spans multiple segments, e.g.
        // `m[$k]=v` → [Lit("m["), Param("k"), Lit("]=v")]. The first segment
        // then has `[` but no closing `]`, so `=` isn't in it — handle here.
        if let Some(open) = first.find('[')
            && !first[open..].contains(']')
        {
            return self.spanning_subscript_assignment(segs, first, open);
        }
        let Some(eq) = first.find('=') else {
            return Ok(None);
        };
        let mut lhs = &first[..eq];
        // `+=` append.
        let append = lhs.ends_with('+');
        if append {
            lhs = &lhs[..lhs.len() - 1];
        }
        // Optional `[index]` subscript.
        let (name, index) = if let Some(open) = lhs.find('[') {
            if !lhs.ends_with(']') {
                return Ok(None);
            }
            let name = &lhs[..open];
            let idx_src = &lhs[open + 1..lhs.len() - 1];
            if name.is_empty() || !is_valid_name(name) || idx_src.is_empty() {
                return Ok(None);
            }
            // A subscript is parsed verbatim (no word-splitting/trimming): for an
            // associative array the expanded text — leading/trailing whitespace
            // included — is the literal key (bash: `h[ x ]=v` keys on ` x `). For
            // an indexed array the arithmetic evaluator ignores the whitespace, so
            // preserving it is harmless.
            (name, Some(word_verbatim_from_source(idx_src)?))
        } else {
            if lhs.is_empty() || !is_valid_name(lhs) {
                return Ok(None);
            }
            (lhs, None)
        };
        // Build the value word from the remainder of the first seg plus the
        // rest of the segments.
        let mut value_segs: Vec<Seg> = Vec::new();
        let after = &first[eq + 1..];
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_string()));
        }
        value_segs.extend_from_slice(&segs[1..]);
        Ok(Some(Assignment {
            name: name.to_string(),
            index,
            append,
            value: AssignRhs::Scalar(self.word_from_segs(&value_segs)?),
        }))
    }

    /// Lower lexer segments into an [`ast::Word`].
    fn word_from_segs(&self, segs: &[Seg]) -> Result<Word, ParseError> {
        word_from_segs(segs)
    }

    /// Parse `name[SUBSCRIPT]=value` / `name[SUBSCRIPT]+=value` where the
    /// subscript spans multiple segments (contains `$…` expansions). `open` is
    /// the byte offset of `[` in the first (literal) segment.
    fn spanning_subscript_assignment(
        &self,
        segs: &[Seg],
        first: &str,
        open: usize,
    ) -> Result<Option<Assignment>, ParseError> {
        let name = &first[..open];
        if name.is_empty() || !is_valid_name(name) {
            return Ok(None);
        }
        // Subscript segments: the first seg's text after `[`, then whole
        // segments, up to the segment that carries the closing `]`.
        let mut sub_segs: Vec<Seg> = Vec::new();
        let after_open = &first[open + 1..];
        if !after_open.is_empty() {
            sub_segs.push(Seg::Lit(after_open.to_string()));
        }
        let mut value_segs: Vec<Seg> = Vec::new();
        let mut append = false;
        let mut found = false;
        for seg in &segs[1..] {
            if found {
                value_segs.push(seg.clone());
                continue;
            }
            if let Seg::Lit(s) = seg
                && let Some(close) = s.find(']')
            {
                let before = &s[..close];
                if !before.is_empty() {
                    sub_segs.push(Seg::Lit(before.to_string()));
                }
                let rest = &s[close + 1..];
                let val_lit = if let Some(v) = rest.strip_prefix("+=") {
                    append = true;
                    v
                } else if let Some(v) = rest.strip_prefix('=') {
                    v
                } else {
                    // `]` not immediately followed by `=` — not an assignment.
                    return Ok(None);
                };
                if !val_lit.is_empty() {
                    value_segs.push(Seg::Lit(val_lit.to_string()));
                }
                found = true;
                continue;
            }
            sub_segs.push(seg.clone());
        }
        if !found || sub_segs.is_empty() {
            return Ok(None);
        }
        Ok(Some(Assignment {
            name: name.to_string(),
            index: Some(self.word_from_segs(&sub_segs)?),
            append,
            value: AssignRhs::Scalar(self.word_from_segs(&value_segs)?),
        }))
    }
}

/// Parse one array-literal element: either `[sub]=value` (keyed) or a bare
/// positional value. A keyed element is recognised when the first segment is a
/// literal that starts with `[` and contains `]=` (so the subscript is literal
/// text — an expanded key like `[$k]=v` inside a literal falls back to
/// positional; use element assignment `m[$k]=v` for that).
fn parse_array_elem(segs: &[Seg]) -> Result<ArrayElem, ParseError> {
    if let Some(Seg::Lit(first)) = segs.first()
        && first.starts_with('[')
        && let Some(close_eq) = first.find("]=")
    {
        // Verbatim: an associative keyed element `[ x ]=v` keys on the literal
        // ` x ` (bash preserves subscript whitespace); indexed elements
        // arithmetic-evaluate, which ignores it.
        let index = word_verbatim_from_source(&first[1..close_eq])?;
        let mut value_segs: Vec<Seg> = Vec::new();
        let after = &first[close_eq + 2..];
        if !after.is_empty() {
            value_segs.push(Seg::Lit(after.to_string()));
        }
        value_segs.extend_from_slice(&segs[1..]);
        return Ok(ArrayElem::Keyed {
            index,
            value: word_from_segs(&value_segs)?,
        });
    }
    // General keyed element: the subscript spans quoted or expansion segments,
    // so the closing `]=` is not in the same literal as the opening `[`
    // (`["k v"]=1`, `['k']=1`, `[$x]=1`). The opening `[` is the start of the
    // first literal; everything up to the first unquoted `]=` (which lands in a
    // later literal segment) is the key — intervening quoted/expansion segments
    // belong to it and are copied verbatim.
    if let Some(Seg::Lit(first)) = segs.first()
        && first.starts_with('[')
        && !first.contains("]=")
    {
        let mut key_segs: Vec<Seg> = Vec::new();
        let head = &first[1..];
        if !head.is_empty() {
            key_segs.push(Seg::Lit(head.to_string()));
        }
        for (i, seg) in segs.iter().enumerate().skip(1) {
            if let Seg::Lit(s) = seg
                && let Some(pos) = s.find("]=")
            {
                if !s[..pos].is_empty() {
                    key_segs.push(Seg::Lit(s[..pos].to_string()));
                }
                let index = word_from_segs(&key_segs)?;
                let mut value_segs: Vec<Seg> = Vec::new();
                let after = &s[pos + 2..];
                if !after.is_empty() {
                    value_segs.push(Seg::Lit(after.to_string()));
                }
                value_segs.extend_from_slice(&segs[i + 1..]);
                return Ok(ArrayElem::Keyed {
                    index,
                    value: word_from_segs(&value_segs)?,
                });
            }
            key_segs.push(seg.clone());
        }
    }
    Ok(ArrayElem::Positional(word_from_segs(segs)?))
}

/// True when the command word (`words[0]`) is a declaration/assignment builtin,
/// so that a following array literal (`declare -A m=([k]=v)`, `readonly a=(1 2)`)
/// is parsed as an operand rather than rejected. The word must be a single
/// unquoted literal. bash treats `declare`/`typeset`/`local`/`export`/`readonly`
/// as assignment builtins that accept `name=(…)` compound-array arguments.
fn is_declaration_command(words: &[Word]) -> bool {
    let Some(first) = words.first() else {
        return false;
    };
    let [WordPart::Literal(name)] = first.parts.as_slice() else {
        return false;
    };
    matches!(
        name.as_str(),
        "declare" | "typeset" | "local" | "export" | "readonly"
    )
}

/// Append a redirection to an already-parsed command. Simple commands carry
/// their own redirect list; a `Command::Redirected` extends its list; every
/// other (compound) form is wrapped. Used to lower the `|&` pipe operator's
/// implicit `2>&1` onto the left-hand command.
fn attach_redirect(cmd: Command, redir: Redirect) -> Command {
    match cmd {
        Command::Simple(mut sc) => {
            sc.redirects.push(redir);
            Command::Simple(sc)
        }
        Command::Redirected { inner, mut redirects } => {
            redirects.push(redir);
            Command::Redirected { inner, redirects }
        }
        other => Command::Redirected {
            inner: Box::new(other),
            redirects: vec![redir],
        },
    }
}

/// Lower lexer segments into an [`ast::Word`] (stateless).
fn word_from_segs(segs: &[Seg]) -> Result<Word, ParseError> {
    let mut parts = Vec::with_capacity(segs.len());
    for s in segs {
        parts.push(seg_to_part(s)?);
    }
    Ok(Word { parts })
}

fn seg_to_part(seg: &Seg) -> Result<WordPart, ParseError> {
    Ok(match seg {
        Seg::Lit(s) => WordPart::Literal(s.clone()),
        Seg::Sq(s) => WordPart::SingleQuoted(s.clone()),
        Seg::Dq(inner) => {
            let mut parts = Vec::with_capacity(inner.len());
            for s in inner {
                parts.push(seg_to_part(s)?);
            }
            WordPart::DoubleQuoted(parts)
        }
        Seg::Param(n) => WordPart::Param(n.clone()),
        Seg::ParamBraced(raw) => parse_braced_param(raw)?,
        Seg::CmdSub(raw) => WordPart::CommandSub(parse(raw)?),
        Seg::Arith(raw) => WordPart::ArithSub(raw.clone()),
        Seg::ProcSub(input, raw) => WordPart::ProcSub {
            input: *input,
            body: parse(raw)?,
        },
    })
}

/// Parse the inner text of a `${ … }` expansion.
/// Split a `${…}` body into `(name, optional-subscript, remaining-chars)`.
///
/// The name is a run of name chars, a run of digits, or a single special
/// character. If a `[…]` subscript immediately follows the name, it is parsed
/// into an [`ArrayIndex`] and the characters after the closing `]` are returned
/// as the remainder (for operator forms). The closing bracket is taken as the
/// last `]` in the body so arithmetic subscripts like `arr[i+1]` still parse.
/// Given `bytes[open] == '['`, return the index of the `]` that closes it,
/// balancing nested `[`/`]` (arithmetic subscripts like `a[b[0]]`). This is
/// deliberately *not* "the last `]` in the body": characters after the
/// subscript can contain their own `]` — e.g. a slice offset with a nested
/// parameter expansion `${a[@]:${#a[@]}-2}`, where the `]` inside `${#a[@]}`
/// must not be mistaken for the subscript's close. Brackets inside any valid
/// nested `${…}`/`$(…)` are themselves balanced, so plain depth counting over
/// `[`/`]` handles those correctly too.
fn matching_subscript_close(bytes: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn split_name_subscript(
    bytes: &[char],
) -> Result<(String, Option<ArrayIndex>, Vec<char>), ParseError> {
    if bytes.is_empty() {
        return Err(ParseError("empty '${}' expansion".into()));
    }
    let mut i = 0;
    if bytes[0].is_ascii_digit() {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else if is_name_start(bytes[0]) {
        while i < bytes.len() && is_name_char(bytes[i]) {
            i += 1;
        }
    } else {
        // A special single-char parameter (`@`, `*`, `?`, `#`, `!`, `$`, …).
        i = 1;
    }
    let name: String = bytes[..i].iter().collect();
    if bytes.get(i) == Some(&'[')
        && let Some(close) = matching_subscript_close(bytes, i)
    {
        let inner: String = bytes[i + 1..close].iter().collect();
        let index = match inner.as_str() {
            "@" => ArrayIndex::All,
            "*" => ArrayIndex::Star,
            "" => return Err(ParseError("empty array subscript '[]'".into())),
            // Verbatim so an associative read `${h[ x ]}` keys on the literal
            // ` x ` (bash preserves subscript whitespace); indexed reads
            // arithmetic-evaluate, which ignores the whitespace.
            _ => ArrayIndex::Index(Box::new(word_verbatim_from_source(&inner)?)),
        };
        return Ok((name, Some(index), bytes[close + 1..].to_vec()));
    }
    Ok((name, None, bytes[i..].to_vec()))
}

/// Parse the `offset[:length]` portion of a substring/slice expansion (the
/// text after the leading `:`). The offset and each length are parsed as
/// arithmetic words. Splits on the *first* unescaped `:` only.
fn parse_slice_bounds(rest: &[char]) -> Result<(Box<Word>, Option<Box<Word>>), ParseError> {
    let body: String = rest.iter().collect();
    let (off_str, len_str) = match body.find(':') {
        Some(idx) => (body[..idx].to_string(), Some(body[idx + 1..].to_string())),
        None => (body, None),
    };
    let length = match len_str {
        Some(s) => Some(Box::new(word_from_source(&s)?)),
        None => None,
    };
    Ok((Box::new(word_from_source(&off_str)?), length))
}

pub(crate) fn parse_braced_param(raw: &str) -> Result<WordPart, ParseError> {
    if let Some(after_hash) = raw.strip_prefix('#') {
        if after_hash.is_empty() {
            // `${#}` is the positional-parameter count — treat as `$#`.
            return Ok(WordPart::Param("#".into()));
        }
        let bytes: Vec<char> = after_hash.chars().collect();
        let (name, subscript, remaining) = split_name_subscript(&bytes)?;
        if let Some(index) = subscript {
            if !remaining.is_empty() {
                // bash accepts this at parse time and rejects it only during
                // expansion as a runtime "bad substitution" (DISCARD-class).
                return Ok(WordPart::BadSubst(raw.to_string()));
            }
            // `${#name[@]}` / `${#name[i]}` — array element count / element length.
            return Ok(WordPart::ArrayRef {
                name,
                index,
                length: true,
            });
        }
        return Ok(WordPart::Length(after_hash.to_string()));
    }
    if let Some(after_bang) = raw.strip_prefix('!') {
        // `${!prefix*}` / `${!prefix@}` — names of set variables beginning with
        // `prefix`. Distinguished from the array-keys form (`${!a[@]}`) by
        // ending in a bare `*`/`@` (no closing `]`). A valid name prefix is
        // required so we don't mistake other expansions.
        // A *non-empty* prefix is required for the name-listing form: a bare
        // `${!*}`/`${!@}` is instead indirect expansion through the positional
        // list (`$*`/`$@`), handled below, not a listing of every variable.
        if let Some(prefix) = after_bang.strip_suffix('*')
            && !prefix.is_empty()
            && !prefix.contains('[')
            && is_valid_name(prefix)
        {
            return Ok(WordPart::VarNames {
                prefix: prefix.to_string(),
                star: true,
            });
        }
        if let Some(prefix) = after_bang.strip_suffix('@')
            && !prefix.is_empty()
            && !prefix.contains('[')
            && is_valid_name(prefix)
        {
            return Ok(WordPart::VarNames {
                prefix: prefix.to_string(),
                star: false,
            });
        }
        // `${!name[@]}` / `${!name[*]}` — the keys/indices of an array.
        let bytes: Vec<char> = after_bang.chars().collect();
        let (name, subscript, remaining) = split_name_subscript(&bytes)?;
        if let Some(index) = &subscript
            && remaining.is_empty()
            && matches!(index, ArrayIndex::All | ArrayIndex::Star)
        {
            return Ok(WordPart::ArrayKeys {
                star: matches!(index, ArrayIndex::Star),
                name,
            });
        }
        // `${!name}` — indirect expansion. The referent (`name`) may be a plain
        // identifier, a positional parameter (`${!1}`), or a special parameter
        // (`${!#}`, `${!$}`, …). Its *value* is then used as the parameter name
        // to expand. The named target may itself carry a subscript.
        if subscript.is_none() && is_indirect_referent(&name) {
            if remaining.is_empty() {
                return Ok(WordPart::Indirect(name));
            }
            // `${!ref<op>}` — indirect expansion combined with a modifier
            // (`${!ref:-def}`, `${!ref^^}`, `${!ref#pat}`, `${!ref/a/b}`, …).
            // Parse the modifier as if it were written against `ref` directly;
            // the placeholder name is rewritten to the resolved target at
            // expansion time. Only scalar modifiers combine with indirection,
            // and only a plain-name referent may carry a trailing modifier.
            if is_valid_name(&name) {
                let modifier_src: String =
                    name.chars().chain(remaining.iter().copied()).collect();
                let target = parse_braced_param(&modifier_src)?;
                if matches!(
                    target,
                    WordPart::ParamOp { .. }
                        | WordPart::ParamTrim { .. }
                        | WordPart::ParamSubstr { .. }
                        | WordPart::ParamReplace { .. }
                        | WordPart::ParamCase { .. }
                        | WordPart::ParamTransform { .. }
                ) {
                    return Ok(WordPart::IndirectOp {
                        refname: name,
                        target: Box::new(target),
                    });
                }
            }
        }
        // bash accepts this at parse time and rejects it only during expansion
        // as a runtime "bad substitution" (DISCARD-class).
        return Ok(WordPart::BadSubst(raw.to_string()));
    }
    let bytes: Vec<char> = raw.chars().collect();
    let (name, subscript, rest) = split_name_subscript(&bytes)?;
    // A subscript may be combined with an operator: `${a[i]:-def}`, `${a[i]#pat}`,
    // etc. Only a specific `[expr]` index is allowed with an operator — `[@]`/`[*]`
    // + operator (bulk transform) is not supported.
    let elem_index: Option<Box<Word>> = match subscript {
        None => None,
        Some(ArrayIndex::Index(w)) => {
            if rest.is_empty() {
                return Ok(WordPart::ArrayRef {
                    name,
                    index: ArrayIndex::Index(w),
                    length: false,
                });
            }
            Some(w)
        }
        Some(index @ (ArrayIndex::All | ArrayIndex::Star)) => {
            if rest.is_empty() {
                return Ok(WordPart::ArrayRef {
                    name,
                    index,
                    length: false,
                });
            }
            // `${a[@]:off:len}` / `${a[*]:off:len}` — array slice (a `:` not
            // followed by a `-=+?` operator char).
            if rest[0] == ':' && !matches!(rest.get(1), Some('-' | '=' | '+' | '?')) {
                let (offset, length) = parse_slice_bounds(&rest[1..])?;
                return Ok(WordPart::ArraySlice {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    offset,
                    length,
                });
            }
            // `${a[@]#pat}` / `${a[*]/x/y}` / `${a[@]^^}` / `${a[@]@Q}` — an
            // element-wise transform applied to every element.
            if let Some(op) = parse_bulk_op(&rest)? {
                return Ok(WordPart::ArrayBulk {
                    name,
                    star: matches!(index, ArrayIndex::Star),
                    op,
                });
            }
            // `${a[@]:-x}` / `${a[*]:+x}` / `${a[@]:?msg}` — use/alternate/error
            // operators on a whole-array reference. Bash treats `[@]`/`[*]` like
            // `$@`: substitute the elements when active, else the operand word.
            let star = matches!(index, ArrayIndex::Star);
            let mut chs = rest.iter();
            let mut c = *chs.next().unwrap_or(&'\0');
            let colon = c == ':';
            if colon {
                c = *chs.next().unwrap_or(&'\0');
            }
            let arg_str: String = chs.collect();
            let op = match c {
                '-' => ParamOp::UseDefault,
                '=' => ParamOp::AssignDefault,
                '+' => ParamOp::UseAlternate,
                '?' => ParamOp::ErrorIfUnset,
                _ => {
                    // bash accepts this at parse time and rejects it only during
                    // expansion as a runtime "bad substitution" (DISCARD-class).
                    return Ok(WordPart::BadSubst(raw.to_string()));
                }
            };
            return Ok(WordPart::ArrayOp {
                name,
                star,
                op,
                colon,
                arg: Box::new(word_verbatim_from_source(&arg_str)?),
            });
        }
    };
    // `${@:off:len}` / `${*:off:len}` — positional-parameter slice (same `:`
    // rule as the array form; distinguished from string substring because the
    // parameter names the whole positional list).
    if (name == "@" || name == "*")
        && !rest.is_empty()
        && rest[0] == ':'
        && !matches!(rest.get(1), Some('-' | '=' | '+' | '?'))
    {
        let (offset, length) = parse_slice_bounds(&rest[1..])?;
        return Ok(WordPart::ArraySlice {
            name: name.clone(),
            star: name == "*",
            offset,
            length,
        });
    }
    // `${@#pat}` / `${*/x/y}` / `${@^^}` — element-wise transform over the
    // positional parameters.
    if (name == "@" || name == "*")
        && !rest.is_empty()
        && let Some(op) = parse_bulk_op(&rest)?
    {
        return Ok(WordPart::ArrayBulk {
            name: name.clone(),
            star: name == "*",
            op,
        });
    }
    if rest.is_empty() {
        return Ok(WordPart::Param(name));
    }
    match rest[0] {
        // Prefix / suffix trimming: `#`, `##`, `%`, `%%`.
        '#' | '%' => {
            let suffix = rest[0] == '%';
            let longest = rest.get(1) == Some(&rest[0]);
            let pat_start = if longest { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(WordPart::ParamTrim {
                name,
                index: elem_index,
                suffix,
                longest,
                pattern: Box::new(word_verbatim_from_source(&pat)?),
            })
        }
        // Case modification: `^`/`^^` (upper), `,`/`,,` (lower), `~`/`~~` (toggle).
        '^' | ',' | '~' => {
            let mode = match rest[0] {
                '^' => crate::ast::CaseMode::Upper,
                ',' => crate::ast::CaseMode::Lower,
                _ => crate::ast::CaseMode::Toggle,
            };
            let all = rest.get(1) == Some(&rest[0]);
            let pat_start = if all { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(WordPart::ParamCase {
                name,
                index: elem_index,
                mode,
                all,
                pattern: Box::new(word_verbatim_from_source(&pat)?),
            })
        }
        // Parameter transformation: `${name@Q}`, `${name@U}`, etc.
        '@' => {
            if rest.len() != 2 {
                // bash accepts this at parse time and rejects it only during
                // expansion as a runtime "bad substitution" (DISCARD-class).
                return Ok(WordPart::BadSubst(raw.to_string()));
            }
            Ok(WordPart::ParamTransform {
                name,
                index: elem_index,
                op: rest[1],
            })
        }
        // Pattern substitution: `/pat/repl`, `//pat/repl`, `/#…`, `/%…`.
        '/' => parse_param_replace(name, elem_index, &rest[1..]),
        // Substring `:offset[:length]` — but `:` followed by one of -=+? is the
        // use/assign/alt/error operator, handled below.
        ':' if !matches!(rest.get(1), Some('-' | '=' | '+' | '?')) => {
            let (offset, length) = parse_slice_bounds(&rest[1..])?;
            Ok(WordPart::ParamSubstr {
                name,
                index: elem_index,
                offset,
                length,
            })
        }
        // `:-`, `:=`, `:+`, `:?` and the colon-less `-=+?` forms.
        _ => {
            let mut chs = rest.iter();
            let mut c = *chs.next().unwrap_or(&'\0');
            // A leading `:` selects the null-or-unset (colon) form; without it the
            // operator acts only when the parameter is genuinely unset.
            let colon = c == ':';
            if colon {
                c = *chs.next().unwrap_or(&'\0');
            }
            let arg_str: String = chs.collect();
            let op = match c {
                '-' => ParamOp::UseDefault,
                '=' => ParamOp::AssignDefault,
                '+' => ParamOp::UseAlternate,
                '?' => ParamOp::ErrorIfUnset,
                _ => {
                    // bash accepts this at parse time and rejects it only during
                    // expansion as a runtime "bad substitution" (DISCARD-class).
                    return Ok(WordPart::BadSubst(raw.to_string()));
                }
            };
            Ok(WordPart::ParamOp {
                name,
                index: elem_index,
                op,
                colon,
                arg: Box::new(word_verbatim_from_source(&arg_str)?),
            })
        }
    }
}

/// Parse the body of a `${name/…}` substitution (chars after the first `/`).
/// Parse the `[/|#|%]pat/repl` body of a substitution into its component pieces
/// (`all`, anchor, pattern, replacement), shared by the scalar and bulk-array
/// substitution parsers.
#[allow(clippy::type_complexity)]
fn parse_replace_pieces(
    body: &[char],
) -> Result<(bool, ReplaceAnchor, Box<Word>, Box<Word>), ParseError> {
    let mut i = 0;
    let mut all = false;
    let mut anchor = ReplaceAnchor::None;
    match body.first() {
        Some('/') => {
            all = true;
            i = 1;
        }
        Some('#') => {
            anchor = ReplaceAnchor::Start;
            i = 1;
        }
        Some('%') => {
            anchor = ReplaceAnchor::End;
            i = 1;
        }
        _ => {}
    }
    // Pattern runs to the next unescaped '/'; the remainder is the replacement.
    let mut pattern = String::new();
    let mut replacement = String::new();
    let mut in_repl = false;
    while i < body.len() {
        let c = body[i];
        if !in_repl && c == '\\' && body.get(i + 1) == Some(&'/') {
            pattern.push('/');
            i += 2;
            continue;
        }
        if !in_repl && c == '/' {
            in_repl = true;
            i += 1;
            continue;
        }
        if in_repl {
            replacement.push(c);
        } else {
            pattern.push(c);
        }
        i += 1;
    }
    Ok((
        all,
        anchor,
        Box::new(word_verbatim_from_source(&pattern)?),
        Box::new(word_replacement_from_source(&replacement)?),
    ))
}

fn parse_param_replace(
    name: String,
    index: Option<Box<Word>>,
    body: &[char],
) -> Result<WordPart, ParseError> {
    let (all, anchor, pattern, replacement) = parse_replace_pieces(body)?;
    Ok(WordPart::ParamReplace {
        name,
        index,
        all,
        anchor,
        pattern,
        replacement,
    })
}

/// Parse the operator portion of a bulk array expansion (`${a[@]OP}`) into a
/// [`BulkOp`], or `None` when `rest` is not a recognized element-wise operator
/// (e.g. the `:-`/`:=` default operators, which do not apply to `[@]`).
fn parse_bulk_op(rest: &[char]) -> Result<Option<BulkOp>, ParseError> {
    if rest.is_empty() {
        return Ok(None);
    }
    match rest[0] {
        '#' | '%' => {
            let suffix = rest[0] == '%';
            let longest = rest.get(1) == Some(&rest[0]);
            let pat_start = if longest { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(Some(BulkOp::Trim {
                suffix,
                longest,
                pattern: Box::new(word_verbatim_from_source(&pat)?),
            }))
        }
        '^' | ',' | '~' => {
            let mode = match rest[0] {
                '^' => crate::ast::CaseMode::Upper,
                ',' => crate::ast::CaseMode::Lower,
                _ => crate::ast::CaseMode::Toggle,
            };
            let all = rest.get(1) == Some(&rest[0]);
            let pat_start = if all { 2 } else { 1 };
            let pat: String = rest[pat_start..].iter().collect();
            Ok(Some(BulkOp::Case {
                mode,
                all,
                pattern: Box::new(word_verbatim_from_source(&pat)?),
            }))
        }
        '/' => {
            let (all, anchor, pattern, replacement) = parse_replace_pieces(&rest[1..])?;
            Ok(Some(BulkOp::Replace {
                all,
                anchor,
                pattern,
                replacement,
            }))
        }
        '@' if rest.len() == 2 => Ok(Some(BulkOp::Transform { op: rest[1] })),
        _ => Ok(None),
    }
}

/// Build a single [`Word`] from arbitrary source text (used for the argument of
/// a parameter expansion). Words separated by blanks are joined with a literal
/// space — a best-effort reconstruction adequate for `${x:-a b}`.
/// Parse `s` as a single word preserving literal whitespace (no word-splitting
/// or operator tokenization) — for the pattern and replacement of
/// `${var/pat/repl}`, where bash applies only expansion and quote removal.
fn word_verbatim_from_source(s: &str) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_word_verbatim(s).map_err(|e| ParseError(e.0))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg)?);
    }
    Ok(Word { parts })
}

/// Like [`word_verbatim_from_source`] but for the *replacement* half of
/// `${var/pat/repl}`: a literal `\&`/`\\` is preserved (not consumed at lex
/// time) so the runtime `&`-substitution can distinguish an escaped ampersand
/// from an active one. See [`crate::lexer::lex_replacement_verbatim`].
fn word_replacement_from_source(s: &str) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let segs = crate::lexer::lex_replacement_verbatim(s).map_err(|e| ParseError(e.0))?;
    let mut parts: Vec<WordPart> = Vec::with_capacity(segs.len());
    for seg in &segs {
        parts.push(seg_to_part(seg)?);
    }
    Ok(Word { parts })
}

fn word_from_source(s: &str) -> Result<Word, ParseError> {
    if s.is_empty() {
        return Ok(Word::default());
    }
    let toks = tokenize(s).map_err(|e| ParseError(e.0))?;
    let mut parts: Vec<WordPart> = Vec::new();
    let mut first = true;
    for t in &toks {
        if let Tok::Word(segs) = t {
            if !first {
                parts.push(WordPart::Literal(" ".into()));
            }
            first = false;
            for seg in segs {
                parts.push(seg_to_part(seg)?);
            }
        }
    }
    Ok(Word { parts })
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub(crate) fn is_valid_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if is_name_start(c) => {}
        _ => return false,
    }
    it.all(is_name_char)
}

/// A referent usable in a *bare* indirect expansion `${!name}`: a plain
/// identifier, a positional parameter (all digits, `${!1}`), or a special
/// single-char parameter. bash accepts `#`, `?`, and `-` here but **rejects**
/// `$` and `!` (`${!$}`/`${!!}` are a "bad substitution"). A bare `@`/`*` is
/// indirect expansion through the positional list: `${!@}` / `${!*}` treat
/// each positional parameter's *value* as a variable name to indirect through
/// (bash then rejects them as "invalid variable name" unless empty). Only a
/// *prefixed* `@`/`*` (`${!prefix@}`) is the variable-name listing form.
fn is_indirect_referent(name: &str) -> bool {
    is_valid_name(name)
        || (!name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()))
        || matches!(name, "#" | "?" | "-" | "@" | "*")
}

/// True when a `>&`/`<&` target denotes an fd duplication (a bare number or
/// `-`) rather than a filename. Only a single unquoted literal qualifies, so
/// `>&$var` or `>&"file"` are treated as filenames (redirect both).
fn dup_target_is_fd(target: &Word) -> bool {
    if let [WordPart::Literal(s)] = target.parts.as_slice() {
        s == "-" || (!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    } else {
        false
    }
}

/// Map a `[[ … ]]` unary operator string to its [`UnaryOp`].
fn unary_op_from(s: &str) -> Option<UnaryOp> {
    Some(match s {
        "-e" => UnaryOp::Exists,
        "-f" => UnaryOp::File,
        "-d" => UnaryOp::Dir,
        "-r" => UnaryOp::Readable,
        "-w" => UnaryOp::Writable,
        "-x" => UnaryOp::Executable,
        "-s" => UnaryOp::NonEmptyFile,
        "-z" => UnaryOp::ZeroLen,
        "-n" => UnaryOp::NonZeroLen,
        "-v" => UnaryOp::VarSet,
        "-o" => UnaryOp::OptionSet,
        "-L" | "-h" => UnaryOp::Symlink,
        "-t" => UnaryOp::Terminal,
        _ => return None,
    })
}

/// Raw binary operator recognised inside `[[ … ]]` (before lowering; `Regex`
/// is recognised so it can be rejected with a clear message).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawBinOp {
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    Regex,
    NumEq,
    NumNe,
    NumLt,
    NumLe,
    NumGt,
    NumGe,
    FileNewer,
    FileOlder,
    SameFile,
}

impl RawBinOp {
    fn into_bin_op(self) -> CondBinOp {
        match self {
            RawBinOp::StrEq => CondBinOp::StrEq,
            RawBinOp::StrNe => CondBinOp::StrNe,
            RawBinOp::StrLt => CondBinOp::StrLt,
            RawBinOp::StrGt => CondBinOp::StrGt,
            // `Regex` is rejected before lowering; map defensively to StrEq.
            RawBinOp::Regex => CondBinOp::StrEq,
            RawBinOp::NumEq => CondBinOp::NumEq,
            RawBinOp::NumNe => CondBinOp::NumNe,
            RawBinOp::NumLt => CondBinOp::NumLt,
            RawBinOp::NumLe => CondBinOp::NumLe,
            RawBinOp::NumGt => CondBinOp::NumGt,
            RawBinOp::NumGe => CondBinOp::NumGe,
            RawBinOp::FileNewer => CondBinOp::FileNewer,
            RawBinOp::FileOlder => CondBinOp::FileOlder,
            RawBinOp::SameFile => CondBinOp::SameFile,
        }
    }
}

/// Map a `[[ … ]]` binary operator word to its [`RawBinOp`].
fn raw_binop_from(s: &str) -> Option<RawBinOp> {
    Some(match s {
        "==" | "=" => RawBinOp::StrEq,
        "!=" => RawBinOp::StrNe,
        "=~" => RawBinOp::Regex,
        "-eq" => RawBinOp::NumEq,
        "-ne" => RawBinOp::NumNe,
        "-lt" => RawBinOp::NumLt,
        "-le" => RawBinOp::NumLe,
        "-gt" => RawBinOp::NumGt,
        "-ge" => RawBinOp::NumGe,
        "-nt" => RawBinOp::FileNewer,
        "-ot" => RawBinOp::FileOlder,
        "-ef" => RawBinOp::SameFile,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_command() {
        let prog = parse("echo hello world").unwrap();
        assert_eq!(prog.items.len(), 1);
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected simple command");
        };
        assert_eq!(sc.words.len(), 3);
    }

    #[test]
    fn pipeline_and_andor() {
        let prog = parse("a | b && c || d").unwrap();
        let ao = &prog.items[0].list;
        assert_eq!(ao.first.commands.len(), 2);
        assert_eq!(ao.rest.len(), 2);
    }

    #[test]
    fn assignment_and_redirect() {
        let prog = parse("FOO=bar cmd arg > out 2> err").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert_eq!(sc.assignments.len(), 1);
        assert_eq!(sc.assignments[0].name, "FOO");
        assert_eq!(sc.redirects.len(), 2);
        assert_eq!(sc.redirects[1].fd, 2);
    }

    #[test]
    fn if_clause() {
        let prog = parse("if true; then echo yes; else echo no; fi").unwrap();
        let Command::If(_) = &prog.items[0].list.first.commands[0] else {
            panic!("expected if");
        };
    }

    #[test]
    fn for_loop() {
        let prog = parse("for x in a b c; do echo $x; done").unwrap();
        let Command::For(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected for");
        };
        assert_eq!(f.var, "x");
        assert_eq!(f.words.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn while_loop() {
        let prog = parse("while false; do echo x; done").unwrap();
        let Command::Loop(l) = &prog.items[0].list.first.commands[0] else {
            panic!("expected loop");
        };
        assert!(!l.until);
    }

    #[test]
    fn case_statement() {
        let prog =
            parse("case $x in a|b) echo ab;; *.txt) echo text;; *) echo default;; esac").unwrap();
        let Command::Case(c) = &prog.items[0].list.first.commands[0] else {
            panic!("expected case");
        };
        assert_eq!(c.items.len(), 3);
        assert_eq!(c.items[0].patterns.len(), 2);
        assert_eq!(c.items[2].patterns.len(), 1);
    }

    #[test]
    fn case_empty_and_final_no_dsemi() {
        // Last arm may omit `;;`; an empty body is allowed.
        let prog = parse("case y in x) ;; y) echo hit\nesac").unwrap();
        let Command::Case(c) = &prog.items[0].list.first.commands[0] else {
            panic!("expected case");
        };
        assert_eq!(c.items.len(), 2);
        assert!(c.items[0].body.items.is_empty());
    }

    #[test]
    fn here_doc_and_here_string() {
        let prog = parse("cat <<EOF\nhi\nEOF\n").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!("expected simple");
        };
        assert_eq!(sc.redirects.len(), 1);
        assert!(matches!(sc.redirects[0].op, RedirectOp::HereDoc));
        assert_eq!(sc.redirects[0].fd, 0);

        let prog2 = parse("cat <<< hello").unwrap();
        let Command::Simple(sc2) = &prog2.items[0].list.first.commands[0] else {
            panic!("expected simple");
        };
        assert!(matches!(sc2.redirects[0].op, RedirectOp::HereStr));
    }

    #[test]
    fn function_def() {
        let prog = parse("greet() { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "greet");
    }

    #[test]
    fn function_keyword_forms() {
        // bash keyword form without parentheses.
        let prog = parse("function greet { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "greet");

        // bash keyword form WITH parentheses.
        let prog = parse("function greet() { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "greet");

        // A non-identifier name is permitted in the keyword form.
        let prog = parse("function foo-bar { echo hi; }").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "foo-bar");

        // Multi-line body and a subshell body.
        assert!(parse("function f {\necho a\necho b\n}").is_ok());
        let prog = parse("function f() ( echo sub )").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.name, "f");

        // Trailing redirection is attached to the definition.
        let prog = parse("function f { echo a; } >/dev/null").unwrap();
        let Command::Function(f) = &prog.items[0].list.first.commands[0] else {
            panic!("expected function");
        };
        assert_eq!(f.redirects.len(), 1);
    }

    #[test]
    fn function_missing_body_errors_like_bash() {
        // At EOF, both the keyword form and the POSIX form report bash's
        // canonical "unexpected end of file" (not a bespoke message).
        assert_eq!(
            parse("function f").unwrap_err().0,
            "syntax error: unexpected end of file"
        );
        assert_eq!(
            parse("f()").unwrap_err().0,
            "syntax error: unexpected end of file"
        );
        // A non-body token after the header names the offending token.
        assert_eq!(
            parse("f() echo hi").unwrap_err().0,
            "syntax error near unexpected token `echo'"
        );
    }

    #[test]
    fn param_expansions() {
        let prog = parse("echo ${x:-default} ${#y}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(sc.words[1].parts[0], WordPart::ParamOp { .. }));
        assert!(matches!(sc.words[2].parts[0], WordPart::Length(_)));
    }

    #[test]
    fn array_ref_parsing() {
        let prog = parse("echo ${a[0]} ${a[@]} ${a[*]} ${#a[@]}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(
            sc.words[1].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::Index(_),
                length: false,
                ..
            }
        ));
        assert!(matches!(
            sc.words[2].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::All,
                length: false,
                ..
            }
        ));
        assert!(matches!(
            sc.words[3].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::Star,
                ..
            }
        ));
        assert!(matches!(
            sc.words[4].parts[0],
            WordPart::ArrayRef {
                index: ArrayIndex::All,
                length: true,
                ..
            }
        ));
    }

    #[test]
    fn array_assignment_parsing() {
        let prog = parse("a=(one two three)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        let AssignRhs::Array(words) = &sc.assignments[0].value else {
            panic!("expected array assignment");
        };
        assert_eq!(words.len(), 3);
        assert!(!sc.assignments[0].append);
    }

    #[test]
    fn array_append_and_index_assignment() {
        let prog = parse("a+=(x); a[2]=y").unwrap();
        let Command::Simple(app) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(app.assignments[0].append);
        let idx = parse("a[2]=y").unwrap();
        let Command::Simple(sc) = &idx.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments[0].index.is_some());
        assert!(matches!(sc.assignments[0].value, AssignRhs::Scalar(_)));
    }

    #[test]
    fn assoc_keyed_literal_and_keys_parsing() {
        // `${!m[@]}` → ArrayKeys.
        let prog = parse("echo ${!m[@]} ${!m[*]}").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(
            sc.words[1].parts[0],
            WordPart::ArrayKeys { star: false, .. }
        ));
        assert!(matches!(
            sc.words[2].parts[0],
            WordPart::ArrayKeys { star: true, .. }
        ));
        // Keyed array-literal element `[k]=v`.
        let prog = parse("m=([a]=1 plain)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        let AssignRhs::Array(items) = &sc.assignments[0].value else {
            panic!("expected array literal");
        };
        assert!(matches!(items[0], ArrayElem::Keyed { .. }));
        assert!(matches!(items[1], ArrayElem::Positional(_)));
    }

    #[test]
    fn spanning_subscript_assignment_parsing() {
        // `m[$k]=v` — subscript spans segments; still recognised as assignment.
        let prog = parse("m[$k]=v").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments[0].index.is_some());
        assert!(!sc.assignments[0].append);
        assert!(sc.words.is_empty());
    }

    #[test]
    fn declare_array_operand_parsing() {
        // `declare -A m=([k]=v)` — the array literal after the command word is
        // captured as a declaration operand, not a leading prefix assignment.
        let prog = parse("declare -A m=([k]=v)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(sc.assignments.is_empty());
        assert_eq!(sc.decl_arrays.len(), 1);
        assert_eq!(sc.decl_arrays[0].name, "m");
        // The command word and its flag are ordinary words.
        assert_eq!(sc.words.len(), 2);
    }

    #[test]
    fn array_literal_after_plain_command_rejected() {
        // Only declaration builtins may take an array-literal operand.
        assert!(parse("foo m=(a b)").is_err());
    }

    #[test]
    fn stray_word_after_compound_command_rejected() {
        // A compound command cannot be followed by a bare word without a
        // separator; bash rejects this and osh previously mis-ran the trailing
        // word(s) as a second command.
        assert!(parse("for i in 1 2; do echo $i; done extra").is_err());
        assert!(parse("while false; do :; done foo bar").is_err());
        assert!(parse("if true; then echo hi; fi extra").is_err());
        assert!(parse("{ echo a; } extra").is_err());
        assert!(parse("case x in x) :; esac extra").is_err());
        assert!(parse("( echo a ) extra").is_err());
        // A stray `;;` outside a case arm is likewise an error.
        assert!(parse("echo a ;;").is_err());
        // But legitimate followers (separators, redirects, pipes, `&&`, a
        // closing `)`/keyword) must still parse.
        assert!(parse("for i in 1; do echo $i; done > /dev/null").is_ok());
        assert!(parse("for i in 1; do echo $i; done | cat").is_ok());
        assert!(parse("while false; do :; done && echo ok").is_ok());
        assert!(parse("{ echo a; }; echo b").is_ok());
        assert!(parse("( echo a ); echo b").is_ok());
        assert!(parse("x=$(for i in 1 2; do echo $i; done)").is_ok());
    }

    #[test]
    fn empty_compound_list_rejected() {
        // A compound-command condition or body that reduces to nothing is a
        // syntax error in bash; osh previously accepted these (and an empty
        // `while` condition even looped forever).
        assert!(parse("( )").is_err());
        assert!(parse("{ }").is_err());
        assert!(parse("if true; then fi").is_err());
        assert!(parse("if ; then echo x; fi").is_err());
        assert!(parse("while ; do echo x; done").is_err());
        assert!(parse("while false; do done").is_err());
        assert!(parse("until false; do done").is_err());
        assert!(parse("for x in a; do done").is_err());
        // But an empty *command substitution* / top-level program is fine, as is
        // any non-empty compound body.
        assert!(parse("echo $()").is_ok());
        assert!(parse("echo $( )").is_ok());
        assert!(parse("").is_ok());
        assert!(parse("( : )").is_ok());
        assert!(parse("{ :; }").is_ok());
        assert!(parse("if true; then :; fi").is_ok());
    }

    #[test]
    fn bare_separator_rejected() {
        // A `;` or `&` with no preceding command denotes an empty command, which
        // bash rejects — but blank lines between commands are fine.
        assert!(parse("; echo hi").is_err());
        assert!(parse("& echo hi").is_err());
        assert!(parse("echo a ; ; echo b").is_err());
        assert!(parse("echo a\n\n\necho b").is_ok());
        assert!(parse("echo a ; echo b ;").is_ok());
        assert!(parse("echo a; echo b").is_ok());
    }

    #[test]
    fn command_substitution() {
        let prog = parse("echo $(echo nested)").unwrap();
        let Command::Simple(sc) = &prog.items[0].list.first.commands[0] else {
            panic!();
        };
        assert!(matches!(sc.words[1].parts[0], WordPart::CommandSub(_)));
    }

    #[test]
    fn negated_pipeline() {
        let prog = parse("! false").unwrap();
        assert!(prog.items[0].list.first.negated);
    }

    #[test]
    fn time_keyword_pipeline() {
        let prog = parse("time echo hi").unwrap();
        let p = &prog.items[0].list.first;
        assert!(p.timed);
        assert!(!p.time_posix);
        // The `time` word is consumed, so the body is just `echo hi`.
        let Command::Simple(sc) = &p.commands[0] else { panic!() };
        assert_eq!(sc.words[0].parts.len(), 1);

        let prog = parse("time -p sleep 0 | cat").unwrap();
        let p = &prog.items[0].list.first;
        assert!(p.timed);
        assert!(p.time_posix);
        assert_eq!(p.commands.len(), 2);

        // `time` precedes `!` negation.
        let prog = parse("time ! false").unwrap();
        let p = &prog.items[0].list.first;
        assert!(p.timed);
        assert!(p.negated);

        // A bare `time` with nothing after it is an ordinary command word.
        let prog = parse("time").unwrap();
        let p = &prog.items[0].list.first;
        assert!(!p.timed);

        // `time` inside a `for … in` list stays a plain word.
        let prog = parse("for x in time now; do echo $x; done").unwrap();
        assert!(!prog.items[0].list.first.timed);
    }

    #[test]
    fn cond_expression() {
        let prog = parse("[[ $x == foo ]]").unwrap();
        let Command::Cond(CondExpr::Binary(_, op, _)) = &prog.items[0].list.first.commands[0]
        else {
            panic!("expected cond binary");
        };
        assert_eq!(*op, CondBinOp::StrEq);
    }

    #[test]
    fn cond_logical_precedence() {
        // `||` binds looser than `&&`: a || b && c parses as a || (b && c).
        let prog = parse("[[ 1 -eq 1 || 2 -eq 2 && 3 -eq 3 ]]").unwrap();
        let Command::Cond(CondExpr::Or(_, right)) = &prog.items[0].list.first.commands[0] else {
            panic!("expected top-level Or");
        };
        assert!(matches!(**right, CondExpr::And(_, _)));
    }

    #[test]
    fn cond_regex_parses() {
        let prog = parse("[[ $x =~ foo ]]").unwrap();
        assert!(matches!(
            prog.items[0].list.first.commands[0],
            Command::Cond(CondExpr::Regex(_, _))
        ));
    }

    #[test]
    fn arith_command() {
        let prog = parse("(( x + 1 ))").unwrap();
        let Command::Arith(raw) = &prog.items[0].list.first.commands[0] else {
            panic!("expected arith command");
        };
        assert_eq!(raw.trim(), "x + 1");
    }

    #[test]
    fn double_paren_vs_nested_subshell() {
        // `((` = arithmetic; `( (` = nested subshell.
        assert!(matches!(
            parse("(( 1 ))").unwrap().items[0].list.first.commands[0],
            Command::Arith(_)
        ));
        assert!(matches!(
            parse("( ( echo ) )").unwrap().items[0].list.first.commands[0],
            Command::Subshell(_)
        ));
    }
}
