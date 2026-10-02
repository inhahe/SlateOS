//! The awk parser: tokens to [`Program`].
//!
//! ## The four ambiguities awk's grammar actually has
//!
//! **`print a > b`.** The `>` is a redirection, not a comparison — but only at
//! the top level of a print's argument list, so `print (a > b)` compares. The
//! parser carries a `no_gt` flag through expression parsing for exactly this,
//! rather than trying to undo the parse afterwards.
//!
//! **`print (a, b)` versus `print (a)(b)`.** POSIX gives print a
//! parenthesised argument list, and an argument list may also *begin* with a
//! grouping. What follows the closing `)` decides: the end of the statement or
//! a redirection means the list, anything else more expression. See
//! `Parser::parenthesised_print_list`.
//!
//! **`a (b)`.** With no space this is a call of the function `a`; with a space
//! it is `a` concatenated with `b`. The *lexer* settles it, because by the time
//! the parser sees the tokens the space is gone.
//!
//! **`for (x in a)` versus `for (i = 1; …)`.** Both start `for (`. The parser
//! looks ahead for `NAME in NAME )` before committing.
//!
//! ## Why names are resolved here
//!
//! A reference becomes a slot index at parse time. Inside a function body, a
//! name that is one of the parameters is a local and everything else is a
//! global — which also means a name used as a global in one function and a
//! parameter in another is two different variables, as awk requires.
//!
//! Function *calls* are resolved late, because awk allows calling a function
//! defined further down the file. Every call records a slot in a table keyed by
//! name; at the end of the parse, a slot with no definition is the error
//! "calling undefined function".

use crate::ast::{
    BinOp, Builtin, CmpOp, Expr, ExprKind, Func, Getline, GetlineSrc, Loc, Lvalue, Pattern,
    Program, RedirMode, Redirect, Rule, SPECIALS, Stmt, VarRef,
};
use crate::lex::{BUILTINS, Kw, Lexer, Tok, Token};
use crate::source::{self, SourceMap};
use crate::value::Str;
use ere::awk::{self as escape, CompileError, Warnings};
use ere::{Regex, Syntax};
use std::collections::HashMap;
use std::rc::Rc;

/// Parse a whole program, its escape warnings going to `warnings`' tables and
/// to `said`, each beside the [`Loc`] of the token that earned it.
///
/// `map` says where the program text came from, so that what is reported --
/// here and, through the [`Loc`]s the statements carry, at run time -- names
/// the line as gawk does.
///
/// # Errors
/// Returns the diagnostics, one per line, in the order gawk gives them. awk
/// parses the entire program before running any of it, so a syntax error in a
/// rule that would never have matched still stops it — better than dying
/// halfway through a report.
///
/// ## How gawk orders what it finds
///
/// gawk's parser asks its lexer for one token at a time, so it meets the
/// problems in reading order, and they come in two strengths. An `error:` --
/// a constant zero divisor, `next` in a BEGIN action -- is reported and the
/// parse carries on, so a program can earn several; the program does not run,
/// exit 1. A syntax error, or a regex literal that will not compile, stops the
/// parse there. And the lexer's own refusals stop it too: a backslash-newline
/// in a string is `fatal:` (exit 2), an unterminated string a syntax error.
///
/// Our lexer reads the whole program before the parser starts, so the order
/// has to be put back. [`Lexer::tokenize`] hands over every token it made
/// before it stopped, the parser runs over those, and whatever the parser
/// reported first is kept ahead of the lexer's complaint -- unless the parser
/// needed a token from beyond where the lexer stopped, in which case gawk's
/// lexer would have been asked for it, and complained, first.
pub fn parse(
    src: &[u8],
    map: &SourceMap,
    warnings: &mut Warnings,
    said: &mut Vec<(Loc, Str)>,
) -> Result<Program, ParseError> {
    let names = map.names();
    let mut raw_said = Vec::new();
    let (tokens, lex_error) = Lexer::tokenize(src, warnings, &mut raw_said);
    said.extend(raw_said.into_iter().map(|(at, m)| (map.loc(at), m)));
    let locs = tokens.iter().map(|t| map.loc(t.at)).collect();
    let mut p = Parser {
        toks: tokens,
        locs,
        names,
        i: 0,
        globals: SPECIALS.iter().map(|s| (*s).to_string()).collect(),
        global_index: SPECIALS
            .iter()
            .enumerate()
            .map(|(i, s)| ((*s).to_string(), i))
            .collect(),
        locals: HashMap::new(),
        in_function: false,
        func_index: HashMap::new(),
        funcs: Vec::new(),
        called: Vec::new(),
        ranges: 0,
        loop_depth: 0,
        action: Action::Rule,
        groups: 0,
        errors: Vec::new(),
        cut: lex_error.is_some(),
        reached_cut: std::cell::Cell::new(false),
    };
    let parsed = p.program();
    let mut messages = std::mem::take(&mut p.errors);
    if let Some((at, e)) = lex_error {
        match parsed {
            // The parser stopped before it needed a token the lexer never
            // made: that is gawk's report, and gawk would not have read on.
            Err(syntax) if !p.reached_cut.get() => {
                messages.push(syntax);
                return Err(ParseError {
                    fatal: false,
                    messages,
                });
            }
            _ => {
                // gawk's fatal lexing errors name their line; its syntax
                // errors here are compared by presence and keep our wording.
                let fatal = e.starts_with("fatal: ");
                messages.push(if fatal {
                    located(&p.names, map.loc(at), &e)
                } else {
                    e
                });
                return Err(ParseError { fatal, messages });
            }
        }
    }
    match parsed {
        Err(syntax) => {
            messages.push(syntax);
            Err(ParseError {
                fatal: false,
                messages,
            })
        }
        Ok(_) if !messages.is_empty() => Err(ParseError {
            fatal: false,
            messages,
        }),
        Ok(mut prog) => {
            prog.sources = p.names;
            Ok(prog)
        }
    }
}

/// Why a program will not run, said before any of it has.
#[derive(Debug)]
pub struct ParseError {
    /// gawk reports one of these as `fatal:`, which is exit status 2 rather
    /// than 1 -- a backslash-newline inside a string under `--posix`, say,
    /// against a missing brace, a constant zero divisor or a regex literal
    /// that will not compile (gawk's `error:`).
    ///
    /// A field rather than a reading of [`ParseError::messages`]: a message
    /// can quote the program (`the string "a: fatal: b"`), and a file name in
    /// its location prefix can hold any byte, so no test of the text could
    /// tell the two kinds apart for every program.
    pub fatal: bool,
    /// The diagnostics, each a line after `awk: `, in the order gawk says them.
    pub messages: Vec<String>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.messages.join("\n"))
    }
}

impl From<ParseError> for String {
    fn from(e: ParseError) -> String {
        e.to_string()
    }
}

/// Which kind of action is being parsed: gawk refuses `next` and `nextfile`
/// in BEGIN and END outright, and leaves the decision to run time inside a
/// function, which any of them may call.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Begin,
    End,
    Rule,
    Function,
}

impl Action {
    /// gawk's name for a BEGIN or END action (`ruletab`), or `None`.
    fn begin_or_end(self) -> Option<&'static str> {
        match self {
            Action::Begin => Some("BEGIN"),
            Action::End => Some("END"),
            Action::Rule | Action::Function => None,
        }
    }
}

/// The value gawk's parser would fold `e` to, if it would fold it at all.
///
/// gawk checks a divisor at parse time, and only one it has reduced to a
/// single numeric constant instruction (`mk_binary` in awkgram.y): a numeric
/// literal, unary minus of one (`-0`), `!` of a constant (`!1`, `!"a"`), or
/// two constants combined by `+ - * / % ^`. A string is not folded (`"0"`),
/// nor is unary plus (`+0`), nor anything in parentheses, because gawk 5.2
/// always appends an `Op_parens` after a grouping -- `1/(2-2)` is a fatal at
/// run time, not this. The caller rules out parentheses; this is the rest.
fn folded_constant(e: &Expr) -> Option<f64> {
    match &e.kind {
        ExprKind::Num(n) => Some(*n),
        ExprKind::Neg(inner) => folded_constant(inner).map(|n| -n),
        ExprKind::Not(inner) => match &inner.kind {
            // `!` folds a string constant too: to 1 if it is empty, else 0.
            ExprKind::Str(s) => Some(if s.is_empty() { 1.0 } else { 0.0 }),
            _ => folded_constant(inner).map(|n| if n == 0.0 { 1.0 } else { 0.0 }),
        },
        ExprKind::Bin(op, a, b) => {
            let (l, r) = (folded_constant(a)?, folded_constant(b)?);
            Some(match op {
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                BinOp::Mul => l * r,
                // A zero divisor is refused, not folded: gawk reports it and
                // keeps the operation, so nothing folds through it.
                BinOp::Div if r == 0.0 => return None,
                BinOp::Mod if r == 0.0 => return None,
                BinOp::Div => l / r,
                BinOp::Mod => l % r,
                BinOp::Pow => crate::value::calc_exp(l, r),
            })
        }
        _ => None,
    }
}

/// `message` with gawk's location prefix: `cmd. line:3: fatal: ...`.
fn located(names: &[Option<Str>], loc: Loc, message: &str) -> String {
    let mut out = String::from_utf8_lossy(&source::prefix(names, loc)).into_owned();
    out.push_str(message);
    out
}

struct Parser {
    toks: Vec<Token>,
    /// Where each token is, by index with `toks`.
    locs: Vec<Loc>,
    /// The sources `locs` index, for a diagnostic's prefix.
    names: Vec<Option<Str>>,
    i: usize,
    globals: Vec<String>,
    global_index: HashMap<String, usize>,
    /// Parameter name to frame slot, non-empty only inside a function body.
    locals: HashMap<String, usize>,
    in_function: bool,
    func_index: HashMap<String, usize>,
    funcs: Vec<Option<Func>>,
    /// Every call site's function name, for the undefined-function check.
    called: Vec<String>,
    ranges: usize,
    loop_depth: usize,
    /// The kind of action being parsed, for `next` and `nextfile`.
    action: Action,
    /// How many parenthesised groupings have been parsed so far. A divisor
    /// whose parse moved it is not a folded constant to gawk; see
    /// [`folded_constant`].
    groups: usize,
    /// gawk's `error:`s found so far, each already placed: the parse carries
    /// on past them, and the program does not run.
    errors: Vec<String>,
    /// The token list ends where the lexer stopped with an error, not at the
    /// end of the program, so its last `Eof` is a cut rather than the end.
    cut: bool,
    /// The parser has looked at the cut: it needed a token the lexer never
    /// made, so the lexer's error is what gawk would have reported first. A
    /// `Cell` because looking (`peek`) is not otherwise a change.
    reached_cut: std::cell::Cell<bool>,
}

impl Parser {
    // ---- token access -----------------------------------------------------

    fn peek(&self) -> &Tok {
        self.peek_at(0)
    }
    /// Where the next token is.
    fn loc_here(&self) -> Loc {
        self.locs.get(self.i).copied().unwrap_or_default()
    }
    /// Where the token just consumed is.
    fn loc_prev(&self) -> Loc {
        self.locs
            .get(self.i.saturating_sub(1))
            .copied()
            .unwrap_or_default()
    }
    fn peek_at(&self, k: usize) -> &Tok {
        let at = self.i.saturating_add(k);
        if self.cut && at.saturating_add(1) >= self.toks.len() {
            self.reached_cut.set(true);
        }
        self.toks.get(at).map_or(&Tok::Eof, |t| &t.kind)
    }

    /// Report gawk's `error:` at `loc` and carry on parsing.
    fn error(&mut self, loc: Loc, message: &str) {
        self.errors
            .push(located(&self.names, loc, &format!("error: {message}")));
    }
    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        if t != Tok::Eof {
            self.i = self.i.saturating_add(1);
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.i = self.i.saturating_add(1);
            return true;
        }
        false
    }
    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), String> {
        if self.eat(t) {
            return Ok(());
        }
        Err(format!(
            "syntax error: expected {what}, found {}",
            describe(self.peek())
        ))
    }
    /// Skip newlines and semicolons that separate items or statements.
    fn skip_terms(&mut self) {
        while matches!(self.peek(), Tok::Newline | Tok::Semi) {
            self.i = self.i.saturating_add(1);
        }
    }
    /// Skip newlines only — used where a newline is allowed but a `;` would be
    /// a statement of its own.
    fn skip_newlines(&mut self) {
        while self.peek() == &Tok::Newline {
            self.i = self.i.saturating_add(1);
        }
    }

    // ---- names ------------------------------------------------------------

    fn var(&mut self, name: &str) -> VarRef {
        if self.in_function
            && let Some(slot) = self.locals.get(name)
        {
            return VarRef::Local(*slot);
        }
        if let Some(slot) = self.global_index.get(name) {
            return VarRef::Global(*slot);
        }
        let slot = self.globals.len();
        self.globals.push(name.to_string());
        self.global_index.insert(name.to_string(), slot);
        VarRef::Global(slot)
    }

    fn func_slot(&mut self, name: &str) -> usize {
        if let Some(s) = self.func_index.get(name) {
            return *s;
        }
        let s = self.funcs.len();
        self.funcs.push(None);
        self.func_index.insert(name.to_string(), s);
        s
    }

    // ---- program ----------------------------------------------------------

    fn program(&mut self) -> Result<Program, String> {
        let mut prog = Program::default();
        self.skip_terms();
        while self.peek() != &Tok::Eof {
            if self.eat(&Tok::Keyword(Kw::Function)) {
                self.action = Action::Function;
                self.function()?;
            } else if self.eat(&Tok::Keyword(Kw::Begin)) {
                self.action = Action::Begin;
                self.skip_newlines();
                let body = self.block()?;
                prog.begin.extend(body);
            } else if self.eat(&Tok::Keyword(Kw::End)) {
                self.action = Action::End;
                self.skip_newlines();
                let body = self.block()?;
                prog.end.extend(body);
            } else if self.peek() == &Tok::LBrace {
                self.action = Action::Rule;
                let action = self.block()?;
                prog.rules.push(Rule {
                    pattern: Pattern::Always,
                    action: Some(action),
                });
            } else {
                self.action = Action::Rule;
                let first = self.expr(false)?;
                let pattern = if self.eat(&Tok::Comma) {
                    self.skip_newlines();
                    let second = self.expr(false)?;
                    let id = self.ranges;
                    self.ranges = self.ranges.saturating_add(1);
                    Pattern::Range(first, second, id)
                } else {
                    Pattern::Expr(first)
                };
                let action = if self.peek() == &Tok::LBrace {
                    Some(self.block()?)
                } else {
                    None
                };
                prog.rules.push(Rule { pattern, action });
            }
            self.skip_terms();
        }

        // Ours, not gawk's (which finds an undefined function when it is
        // called): see main.rs's table of deliberate differences. A program
        // that gawk's own `error:`s already stop is not checked further, as
        // gawk would not have got as far as running it.
        if self.errors.is_empty() {
            for name in &self.called {
                let defined = self
                    .func_index
                    .get(name)
                    .and_then(|s| self.funcs.get(*s))
                    .is_some_and(Option::is_some);
                if !defined {
                    return Err(format!("calling undefined function {name}"));
                }
            }
        }
        prog.funcs = self
            .funcs
            .iter()
            .map(|f| {
                f.clone().unwrap_or(Func {
                    name: String::new(),
                    params: Vec::new(),
                    body: Vec::new(),
                    loc: Loc::default(),
                })
            })
            .collect();
        prog.globals = self.globals.len();
        prog.global_names = std::mem::take(&mut self.globals);
        prog.ranges = self.ranges;
        Ok(prog)
    }

    fn function(&mut self) -> Result<(), String> {
        let tok = self.bump();
        let loc = self.loc_prev();
        let name = match tok {
            Tok::Name(n) | Tok::FuncName(n) => n,
            other => {
                return Err(format!(
                    "syntax error: expected a function name, found {}",
                    describe(&other)
                ));
            }
        };
        if BUILTINS.iter().any(|(b, _, _)| *b == name) {
            return Err(format!("cannot redefine the built-in function {name}"));
        }
        self.expect(&Tok::LParen, "`(' after the function name")?;
        let mut params: Vec<String> = Vec::new();
        self.skip_newlines();
        if !self.eat(&Tok::RParen) {
            loop {
                self.skip_newlines();
                match self.bump() {
                    Tok::Name(p) => {
                        if params.contains(&p) {
                            return Err(format!("function {name}: parameter {p} appears twice"));
                        }
                        params.push(p);
                    }
                    other => {
                        return Err(format!(
                            "syntax error: expected a parameter name, found {}",
                            describe(&other)
                        ));
                    }
                }
                self.skip_newlines();
                if self.eat(&Tok::Comma) {
                    continue;
                }
                self.expect(&Tok::RParen, "`)' after the parameter list")?;
                break;
            }
        }

        let slot = self.func_slot(&name);
        if self.funcs.get(slot).is_some_and(Option::is_some) {
            return Err(format!("function {name} is defined twice"));
        }
        self.locals = params
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect();
        self.in_function = true;
        self.skip_newlines();
        let body = self.block()?;
        self.in_function = false;
        self.locals.clear();
        if let Some(entry) = self.funcs.get_mut(slot) {
            *entry = Some(Func {
                name,
                params,
                body,
                loc,
            });
        }
        Ok(())
    }

    // ---- statements -------------------------------------------------------

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Tok::LBrace, "`{'")?;
        let mut out = Vec::new();
        loop {
            self.skip_terms();
            if self.eat(&Tok::RBrace) {
                return Ok(out);
            }
            if self.peek() == &Tok::Eof {
                return Err("syntax error: unexpected end of program, `}' missing".to_string());
            }
            out.push(self.stmt()?);
        }
    }

    /// A statement, plus whatever terminates it, wrapped in the [`Stmt::At`]
    /// that tells a diagnostic raised while it runs which line it is on.
    fn stmt(&mut self) -> Result<Stmt, String> {
        let loc = self.loc_here();
        let s = self.bare_stmt()?;
        Ok(Stmt::At(loc, Box::new(s)))
    }

    fn bare_stmt(&mut self) -> Result<Stmt, String> {
        let s = self.unterminated_stmt()?;
        // A statement that ends in another statement — the body of an `if`, a
        // `while`, a `for` — has already had its terminator eaten by that body,
        // and a block ends at its `}`. Demanding a second one here would refuse
        // `for (i = 1; i <= 3; i++) s = s i; print s`, which is ordinary awk.
        // `do … while (…)` is not in the list: it ends at the `)`, so it still
        // needs one of its own.
        if matches!(
            s,
            Stmt::Block(_) | Stmt::If(..) | Stmt::While(..) | Stmt::For { .. } | Stmt::ForIn { .. }
        ) {
            return Ok(s);
        }
        // Otherwise a statement ends at a newline, a `;`, or the `}` that closes
        // its block; anything else means the statement did not consume what it
        // should have, and saying so here beats a confusing error later.
        if matches!(self.peek(), Tok::Newline | Tok::Semi) {
            self.i = self.i.saturating_add(1);
        } else if !matches!(self.peek(), Tok::RBrace | Tok::Eof | Tok::Keyword(Kw::Else)) {
            return Err(format!("syntax error at {}", describe(self.peek())));
        }
        Ok(s)
    }

    fn unterminated_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek().clone() {
            Tok::LBrace => Ok(Stmt::Block(self.block()?)),
            Tok::Semi => Ok(Stmt::Nop),
            Tok::Keyword(Kw::If) => self.if_stmt(),
            Tok::Keyword(Kw::While) => self.while_stmt(),
            Tok::Keyword(Kw::Do) => self.do_stmt(),
            Tok::Keyword(Kw::For) => self.for_stmt(),
            Tok::Keyword(Kw::Print) => {
                self.i = self.i.saturating_add(1);
                self.print_stmt(false)
            }
            Tok::Keyword(Kw::Printf) => {
                self.i = self.i.saturating_add(1);
                self.print_stmt(true)
            }
            // gawk refuses these in BEGIN and END as it parses them -- an
            // `error:`, so it carries on -- and decides at run time inside a
            // function, because which rule calls the function is not known
            // until then.
            Tok::Keyword(Kw::Next) => {
                self.i = self.i.saturating_add(1);
                if let Some(rule) = self.action.begin_or_end() {
                    self.error(self.loc_prev(), &format!("`next' used in {rule} action"));
                }
                Ok(Stmt::Next)
            }
            Tok::Keyword(Kw::NextFile) => {
                self.i = self.i.saturating_add(1);
                if let Some(rule) = self.action.begin_or_end() {
                    self.error(
                        self.loc_prev(),
                        &format!("`nextfile' used in {rule} action"),
                    );
                }
                Ok(Stmt::NextFile)
            }
            Tok::Keyword(Kw::Break) => {
                self.i = self.i.saturating_add(1);
                if self.loop_depth == 0 {
                    return Err("break used outside a loop".to_string());
                }
                Ok(Stmt::Break)
            }
            Tok::Keyword(Kw::Continue) => {
                self.i = self.i.saturating_add(1);
                if self.loop_depth == 0 {
                    return Err("continue used outside a loop".to_string());
                }
                Ok(Stmt::Continue)
            }
            Tok::Keyword(Kw::Exit) => {
                self.i = self.i.saturating_add(1);
                Ok(Stmt::Exit(self.optional_expr()?))
            }
            Tok::Keyword(Kw::Return) => {
                self.i = self.i.saturating_add(1);
                if !self.in_function {
                    return Err("return used outside a function".to_string());
                }
                Ok(Stmt::Return(self.optional_expr()?))
            }
            Tok::Keyword(Kw::Delete) => {
                self.i = self.i.saturating_add(1);
                self.delete_stmt()
            }
            _ => Ok(Stmt::Expr(self.expr(false)?)),
        }
    }

    fn optional_expr(&mut self) -> Result<Option<Expr>, String> {
        if matches!(
            self.peek(),
            Tok::Newline | Tok::Semi | Tok::RBrace | Tok::Eof
        ) {
            return Ok(None);
        }
        Ok(Some(self.expr(false)?))
    }

    fn if_stmt(&mut self) -> Result<Stmt, String> {
        self.i = self.i.saturating_add(1);
        self.expect(&Tok::LParen, "`(' after if")?;
        let cond = self.expr(false)?;
        self.expect(&Tok::RParen, "`)' after the if condition")?;
        self.skip_newlines();
        let then = Box::new(self.stmt()?);
        // The `else` may be separated from the then-branch by any number of
        // terminators; that is why this looks ahead rather than trusting that
        // `stmt` stopped on it.
        let save = self.i;
        self.skip_terms();
        if self.eat(&Tok::Keyword(Kw::Else)) {
            self.skip_newlines();
            let other = Box::new(self.stmt()?);
            return Ok(Stmt::If(cond, then, Some(other)));
        }
        self.i = save;
        Ok(Stmt::If(cond, then, None))
    }

    fn while_stmt(&mut self) -> Result<Stmt, String> {
        self.i = self.i.saturating_add(1);
        self.expect(&Tok::LParen, "`(' after while")?;
        let cond = self.expr(false)?;
        self.expect(&Tok::RParen, "`)' after the while condition")?;
        self.skip_newlines();
        // `while (x);` is a loop with an empty body, not a syntax error.
        if self.eat(&Tok::Semi) {
            return Ok(Stmt::While(cond, Box::new(Stmt::Nop)));
        }
        self.loop_depth = self.loop_depth.saturating_add(1);
        let body = self.stmt();
        self.loop_depth = self.loop_depth.saturating_sub(1);
        Ok(Stmt::While(cond, Box::new(body?)))
    }

    fn do_stmt(&mut self) -> Result<Stmt, String> {
        self.i = self.i.saturating_add(1);
        self.skip_newlines();
        self.loop_depth = self.loop_depth.saturating_add(1);
        let body = self.stmt();
        self.loop_depth = self.loop_depth.saturating_sub(1);
        let body = body?;
        self.skip_terms();
        self.expect(
            &Tok::Keyword(Kw::While),
            "`while' after the body of a do loop",
        )?;
        self.expect(&Tok::LParen, "`(' after while")?;
        let cond = self.expr(false)?;
        self.expect(&Tok::RParen, "`)' after the while condition")?;
        Ok(Stmt::DoWhile(Box::new(body), cond))
    }

    fn for_stmt(&mut self) -> Result<Stmt, String> {
        self.i = self.i.saturating_add(1);
        self.expect(&Tok::LParen, "`(' after for")?;
        // `for (x in a)` and `for (i = 1; …)` both start here, so look ahead
        // for the shape of the first rather than backtracking out of a failed
        // parse of the second.
        if let Tok::Name(n) = self.peek().clone()
            && self.peek_at(1) == &Tok::Keyword(Kw::In)
            && let Tok::Name(arr) = self.peek_at(2).clone()
            && self.peek_at(3) == &Tok::RParen
        {
            self.i = self.i.saturating_add(4);
            let var = self.var(&n);
            let array = self.var(&arr);
            self.skip_newlines();
            self.loop_depth = self.loop_depth.saturating_add(1);
            let body = self.stmt();
            self.loop_depth = self.loop_depth.saturating_sub(1);
            return Ok(Stmt::ForIn {
                var,
                array,
                body: Box::new(body?),
            });
        }

        let init = if self.peek() == &Tok::Semi {
            None
        } else {
            Some(Box::new(Stmt::Expr(self.expr(false)?)))
        };
        self.expect(&Tok::Semi, "`;' in a for header")?;
        self.skip_newlines();
        let cond = if self.peek() == &Tok::Semi {
            None
        } else {
            Some(self.expr(false)?)
        };
        self.expect(&Tok::Semi, "`;' in a for header")?;
        self.skip_newlines();
        let step = if self.peek() == &Tok::RParen {
            None
        } else {
            Some(Box::new(Stmt::Expr(self.expr(false)?)))
        };
        self.expect(&Tok::RParen, "`)' after the for header")?;
        self.skip_newlines();
        if self.eat(&Tok::Semi) {
            return Ok(Stmt::For {
                init,
                cond,
                step,
                body: Box::new(Stmt::Nop),
            });
        }
        self.loop_depth = self.loop_depth.saturating_add(1);
        let body = self.stmt();
        self.loop_depth = self.loop_depth.saturating_sub(1);
        Ok(Stmt::For {
            init,
            cond,
            step,
            body: Box::new(body?),
        })
    }

    fn delete_stmt(&mut self) -> Result<Stmt, String> {
        let name = match self.bump() {
            Tok::Name(n) | Tok::FuncName(n) => n,
            other => {
                return Err(format!(
                    "syntax error: delete wants an array name, found {}",
                    describe(&other)
                ));
            }
        };
        let arr = self.var(&name);
        if self.eat(&Tok::LBracket) {
            let subs = self.expr_list(&Tok::RBracket)?;
            self.expect(&Tok::RBracket, "`]'")?;
            if subs.is_empty() {
                return Err("delete: an empty subscript is not a subscript".to_string());
            }
            return Ok(Stmt::Delete(arr, subs));
        }
        // `delete a (…)` cannot happen — the lexer only makes a `FuncName` when
        // a `(` follows, and that is the one shape `delete` does not accept.
        if self.eat(&Tok::LParen) {
            let subs = self.expr_list(&Tok::RParen)?;
            self.expect(&Tok::RParen, "`)'")?;
            return Ok(Stmt::Delete(arr, subs));
        }
        Ok(Stmt::Delete(arr, Vec::new()))
    }

    fn print_stmt(&mut self, formatted: bool) -> Result<Stmt, String> {
        // Inside a print's argument list a bare `>` redirects, so expressions
        // are parsed with `no_gt`. `print (a > b)` still compares, because the
        // parenthesised expression is parsed without the flag.
        let mut args: Vec<Expr> = Vec::new();
        if let Some(close) = self.parenthesised_print_list() {
            // `printf("%s\n", x)`: the parentheses hold the whole list, and
            // inside them a `>` compares, as anywhere in parentheses.
            self.i = self.i.saturating_add(1);
            args = self.expr_list(&Tok::RParen)?;
            self.expect(&Tok::RParen, "`)' after the print list")?;
            debug_assert_eq!(self.i, close.saturating_add(1));
            if args.is_empty() {
                return Err("syntax error: `()' is not an expression".to_string());
            }
        } else if !matches!(
            self.peek(),
            Tok::Newline | Tok::Semi | Tok::RBrace | Tok::Eof | Tok::Gt | Tok::Append | Tok::Pipe
        ) {
            loop {
                args.push(self.expr(true)?);
                if self.eat(&Tok::Comma) {
                    self.skip_newlines();
                    continue;
                }
                break;
            }
        }
        let redirect = match self.peek() {
            Tok::Gt => {
                self.i = self.i.saturating_add(1);
                Some(RedirMode::Truncate)
            }
            Tok::Append => {
                self.i = self.i.saturating_add(1);
                Some(RedirMode::Append)
            }
            Tok::Pipe => {
                self.i = self.i.saturating_add(1);
                Some(RedirMode::Pipe)
            }
            _ => None,
        };
        let redirect = match redirect {
            Some(mode) => {
                // The target is a concatenation-level expression: `> "out" i`
                // names one file per value of `i`.
                let target = self.concat(true)?;
                Some(Redirect { mode, target })
            }
            None => None,
        };
        if formatted && args.is_empty() {
            return Err("printf: no format string".to_string());
        }
        if formatted {
            Ok(Stmt::Printf(args, redirect))
        } else {
            Ok(Stmt::Print(args, redirect))
        }
    }

    /// Whether a print's arguments are one parenthesised list, `print (a, b)`,
    /// rather than expressions that happen to begin with a grouping, `print
    /// (a)(b)` or `print (i, j) in arr`; if so, the index of the closing `)`.
    ///
    /// POSIX's grammar has both shapes (`Print '(' multiple_expr_list ')'`
    /// beside a list whose first expression is grouped), and what tells them
    /// apart comes *after* the parentheses: the list form is followed by the
    /// end of the statement or a redirection, the other by more expression.
    /// So find the matching `)` and look at what follows it, which is the
    /// decision gawk's grammar makes with its `in_parens` state.
    fn parenthesised_print_list(&self) -> Option<usize> {
        if self.peek() != &Tok::LParen {
            return None;
        }
        let mut depth = 0usize;
        let mut at = self.i;
        loop {
            match self.toks.get(at).map(|t| &t.kind)? {
                Tok::LParen => depth = depth.saturating_add(1),
                Tok::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                // Not closed before the program, or the lexer, ran out: not
                // this shape, and the ordinary parse reports what is wrong.
                Tok::Eof => return None,
                _ => {}
            }
            at = at.saturating_add(1);
        }
        let after = self.toks.get(at.saturating_add(1)).map(|t| &t.kind);
        matches!(
            after,
            Some(
                Tok::Newline
                    | Tok::Semi
                    | Tok::RBrace
                    | Tok::Eof
                    | Tok::Gt
                    | Tok::Append
                    | Tok::Pipe
            )
        )
        .then_some(at)
    }

    // ---- expressions ------------------------------------------------------

    fn expr_list(&mut self, end: &Tok) -> Result<Vec<Expr>, String> {
        let mut out = Vec::new();
        self.skip_newlines();
        if self.peek() == end {
            return Ok(out);
        }
        loop {
            out.push(self.expr(false)?);
            self.skip_newlines();
            if self.eat(&Tok::Comma) {
                self.skip_newlines();
                continue;
            }
            return Ok(out);
        }
    }

    /// Assignment is the lowest-precedence operator and is right-associative.
    ///
    /// It is parsed by parsing the whole conditional expression first and then
    /// checking whether an assignment operator follows, which is how a
    /// recursive-descent parser handles an operator whose left side must be an
    /// lvalue without a separate grammar level for lvalues.
    fn expr(&mut self, no_gt: bool) -> Result<Expr, String> {
        let lhs = self.ternary(no_gt)?;
        let op = match self.peek() {
            Tok::Assign => None,
            Tok::AddAssign => Some(BinOp::Add),
            Tok::SubAssign => Some(BinOp::Sub),
            Tok::MulAssign => Some(BinOp::Mul),
            Tok::DivAssign => Some(BinOp::Div),
            Tok::ModAssign => Some(BinOp::Mod),
            Tok::PowAssign => Some(BinOp::Pow),
            _ => return Ok(lhs),
        };
        self.i = self.i.saturating_add(1);
        let loc = self.loc_prev();
        self.skip_newlines();
        let ExprKind::Get(target) = lhs.kind else {
            return Err("syntax error: the left side of an assignment must be a variable, a field or an array element".to_string());
        };
        let rhs = Box::new(self.expr(no_gt)?);
        Ok(Expr::new(
            match op {
                None => ExprKind::Assign(target, rhs),
                Some(o) => ExprKind::AugAssign(target, o, rhs),
            },
            loc,
        ))
    }

    fn ternary(&mut self, no_gt: bool) -> Result<Expr, String> {
        let cond = self.or(no_gt)?;
        if !self.eat(&Tok::Question) {
            return Ok(cond);
        }
        let loc = self.loc_prev();
        self.skip_newlines();
        let yes = self.expr(no_gt)?;
        self.skip_newlines();
        self.expect(&Tok::Colon, "`:' in a ?: expression")?;
        self.skip_newlines();
        let no = self.expr(no_gt)?;
        Ok(Expr::new(
            ExprKind::Cond(Box::new(cond), Box::new(yes), Box::new(no)),
            loc,
        ))
    }

    fn or(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.and(no_gt)?;
        while self.eat(&Tok::Or) {
            let loc = self.loc_prev();
            self.skip_newlines();
            let rhs = self.and(no_gt)?;
            lhs = Expr::new(ExprKind::Or(Box::new(lhs), Box::new(rhs)), loc);
        }
        Ok(lhs)
    }

    fn and(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.in_expr(no_gt)?;
        while self.eat(&Tok::And) {
            let loc = self.loc_prev();
            self.skip_newlines();
            let rhs = self.in_expr(no_gt)?;
            lhs = Expr::new(ExprKind::And(Box::new(lhs), Box::new(rhs)), loc);
        }
        Ok(lhs)
    }

    fn in_expr(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.match_expr(no_gt)?;
        while self.peek() == &Tok::Keyword(Kw::In) {
            self.i = self.i.saturating_add(1);
            let loc = self.loc_prev();
            let name = match self.bump() {
                Tok::Name(n) => n,
                other => {
                    return Err(format!(
                        "syntax error: `in' wants an array name, found {}",
                        describe(&other)
                    ));
                }
            };
            let arr = self.var(&name);
            lhs = Expr::new(ExprKind::In(vec![lhs], arr), loc);
        }
        Ok(lhs)
    }

    fn match_expr(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.relational(no_gt)?;
        loop {
            let neg = match self.peek() {
                Tok::Match => false,
                Tok::NoMatch => true,
                _ => return Ok(lhs),
            };
            self.i = self.i.saturating_add(1);
            let loc = self.loc_prev();
            let rhs = self.relational(no_gt)?;
            lhs = Expr::new(
                ExprKind::Match {
                    neg,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                loc,
            );
        }
    }

    /// Comparison is *non*-associative in awk: `a < b < c` is `(a < b) < c` in
    /// C but a syntax error in POSIX awk. Accepting the C reading would quietly
    /// give a wrong answer, so only one comparison is parsed here.
    fn relational(&mut self, no_gt: bool) -> Result<Expr, String> {
        let lhs = self.pipe_getline(no_gt)?;
        let op = match self.peek() {
            Tok::Lt => CmpOp::Lt,
            Tok::Le => CmpOp::Le,
            Tok::Ge => CmpOp::Ge,
            Tok::Eq => CmpOp::Eq,
            Tok::Ne => CmpOp::Ne,
            // In a print argument list a `>` is a redirection.
            Tok::Gt if !no_gt => CmpOp::Gt,
            _ => return Ok(lhs),
        };
        self.i = self.i.saturating_add(1);
        let loc = self.loc_prev();
        self.skip_newlines();
        let rhs = self.pipe_getline(no_gt)?;
        Ok(Expr::new(
            ExprKind::Cmp(op, Box::new(lhs), Box::new(rhs)),
            loc,
        ))
    }

    /// `"cmd" | getline [var]`.
    ///
    /// This sits between comparison and concatenation so that
    /// `"cmd" | getline line > 0` reads as `(("cmd" | getline line) > 0)`,
    /// which is how the idiom is always written.
    fn pipe_getline(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.concat(no_gt)?;
        while self.peek() == &Tok::Pipe && self.peek_at(1) == &Tok::Keyword(Kw::Getline) {
            self.i = self.i.saturating_add(2);
            // The read is gawk's `getline` instruction, made from that token.
            let loc = self.loc_prev();
            let into = self.optional_getline_target()?;
            lhs = Expr::new(
                ExprKind::Getline(Box::new(Getline {
                    into,
                    src: GetlineSrc::Cmd(lhs),
                })),
                loc,
            );
        }
        Ok(lhs)
    }

    /// Concatenation has no operator: two adjacent operands are concatenated.
    ///
    /// `+` and `-` are deliberately not treated as the start of an operand
    /// here, because `a - b` has to be subtraction; the additive level below
    /// has already taken them.
    fn concat(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.additive(no_gt)?;
        while self.starts_operand() {
            let rhs = self.additive(no_gt)?;
            // Concatenation has no operator token; it is placed with its
            // left operand.
            let loc = lhs.loc;
            lhs = Expr::new(ExprKind::Concat(Box::new(lhs), Box::new(rhs)), loc);
        }
        Ok(lhs)
    }

    fn starts_operand(&self) -> bool {
        match self.peek() {
            Tok::Number(_)
            | Tok::Str(_)
            | Tok::Ere { .. }
            | Tok::Name(_)
            | Tok::FuncName(_)
            | Tok::Builtin(_)
            | Tok::Dollar
            | Tok::Not
            | Tok::LParen
            | Tok::Incr
            | Tok::Decr => true,
            // `getline` concatenates like any other operand, but `cmd | getline`
            // is handled a level up, so a bare `getline` here is the main-input
            // form.
            Tok::Keyword(Kw::Getline) => true,
            _ => false,
        }
    }

    fn additive(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.multiplicative(no_gt)?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.i = self.i.saturating_add(1);
            let loc = self.loc_prev();
            let rhs = self.multiplicative(no_gt)?;
            lhs = Expr::new(ExprKind::Bin(op, Box::new(lhs), Box::new(rhs)), loc);
        }
    }

    fn multiplicative(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.unary(no_gt)?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => return Ok(lhs),
            };
            self.i = self.i.saturating_add(1);
            let loc = self.loc_prev();
            let groups = self.groups;
            let rhs = self.unary(no_gt)?;
            // gawk refuses a divisor it has folded to zero while it parses
            // (`mk_binary`), reports it at the operator, and parses on: the
            // program does not run, exit 1. `1/0`, `x/-0`, `x/0.0` and
            // `x/1e-400` all earn it, `x/(0)` and `x/"0"` do not.
            if self.groups == groups
                && matches!(op, BinOp::Div | BinOp::Mod)
                && folded_constant(&rhs) == Some(0.0)
            {
                self.error(
                    loc,
                    if op == BinOp::Div {
                        "division by zero attempted"
                    } else {
                        "division by zero attempted in `%'"
                    },
                );
            }
            lhs = Expr::new(ExprKind::Bin(op, Box::new(lhs), Box::new(rhs)), loc);
        }
    }

    fn unary(&mut self, no_gt: bool) -> Result<Expr, String> {
        let kind: fn(Box<Expr>) -> ExprKind = match self.peek() {
            Tok::Not => ExprKind::Not,
            Tok::Minus => ExprKind::Neg,
            Tok::Plus => ExprKind::Pos,
            _ => return self.power(no_gt),
        };
        self.i = self.i.saturating_add(1);
        let loc = self.loc_prev();
        let operand = self.unary(no_gt)?;
        Ok(Expr::new(kind(Box::new(operand)), loc))
    }

    /// `^` is right-associative and binds tighter than unary minus on the
    /// right: `2^3^2` is 512, and `-2^2` is -4.
    fn power(&mut self, no_gt: bool) -> Result<Expr, String> {
        let base = self.postfix(no_gt)?;
        if self.eat(&Tok::Caret) {
            let loc = self.loc_prev();
            let exp = self.unary(no_gt)?;
            return Ok(Expr::new(
                ExprKind::Bin(BinOp::Pow, Box::new(base), Box::new(exp)),
                loc,
            ));
        }
        Ok(base)
    }

    fn postfix(&mut self, no_gt: bool) -> Result<Expr, String> {
        let e = self.primary(no_gt)?;
        // `x++` only makes sense on an lvalue; `(a+b)++` is a parse of `(a+b)`
        // followed by `++` starting the next operand, and leaving it alone here
        // is what lets that keep working.
        if let ExprKind::Get(lv) = &e.kind {
            let delta = if self.eat(&Tok::Incr) {
                1.0
            } else if self.eat(&Tok::Decr) {
                -1.0
            } else {
                return Ok(e);
            };
            return Ok(Expr::new(
                ExprKind::PostIncr(lv.clone(), delta),
                self.loc_prev(),
            ));
        }
        Ok(e)
    }

    fn primary(&mut self, no_gt: bool) -> Result<Expr, String> {
        let tok = self.bump();
        // Every primary is placed at its first token: the literal, the name,
        // the `$`, the `++`, the `getline`.
        let loc = self.loc_prev();
        let kind = match tok {
            Tok::Number(n) => ExprKind::Num(n),
            Tok::Str(s) => ExprKind::Str(Rc::new(s)),
            // gawk compiles a regex literal as it parses it, and one that will
            // not compile is its `error:` -- exit 1, like a syntax error, but
            // worded and placed as gawk words and places it. gawk stops the
            // parse there, unlike its other `error:`s.
            Tok::Ere { source, pattern } => {
                let compiled = compile_literal(&source, &pattern)
                    .map_err(|e| located(&self.names, loc, &e))?;
                ExprKind::Regex(Rc::new(compiled))
            }
            Tok::Dollar => {
                // `$` binds tighter than everything but `()` and `++`, so
                // `$NF-1` is `($NF)-1` and `$i++` increments `$i`.
                let inner = self.primary(no_gt)?;
                ExprKind::Get(Lvalue::Field(Box::new(inner), loc))
            }
            Tok::Incr => ExprKind::PreIncr(self.lvalue_operand(no_gt)?, 1.0),
            Tok::Decr => ExprKind::PreIncr(self.lvalue_operand(no_gt)?, -1.0),
            Tok::LParen => {
                let items = self.expr_list(&Tok::RParen)?;
                self.expect(&Tok::RParen, "`)'")?;
                if self.peek() == &Tok::Keyword(Kw::In) {
                    self.i = self.i.saturating_add(1);
                    let in_loc = self.loc_prev();
                    let name = match self.bump() {
                        Tok::Name(n) => n,
                        other => {
                            return Err(format!(
                                "syntax error: `in' wants an array name, found {}",
                                describe(&other)
                            ));
                        }
                    };
                    let arr = self.var(&name);
                    return Ok(Expr::new(ExprKind::In(items, arr), in_loc));
                }
                let mut it = items.into_iter();
                let Some(first) = it.next() else {
                    return Err("syntax error: `()' is not an expression".to_string());
                };
                if it.next().is_some() {
                    // `(a, b)` is only a list before `in`; anywhere else it is
                    // a grouping with a stray comma.
                    return Err(
                        "syntax error: a parenthesised list is only allowed before `in'"
                            .to_string(),
                    );
                }
                // A grouping is not a node of its own, but gawk marks one
                // (`Op_parens`), and a constant inside one is not folded.
                self.groups = self.groups.saturating_add(1);
                return Ok(first);
            }
            Tok::Name(n) => {
                let v = self.var(&n);
                if self.eat(&Tok::LBracket) {
                    let subs = self.expr_list(&Tok::RBracket)?;
                    self.expect(&Tok::RBracket, "`]'")?;
                    if subs.is_empty() {
                        return Err(
                            "syntax error: an empty subscript is not a subscript".to_string()
                        );
                    }
                    ExprKind::Get(Lvalue::Index(v, subs))
                } else {
                    ExprKind::Get(Lvalue::Var(v))
                }
            }
            Tok::FuncName(n) => {
                self.expect(&Tok::LParen, "`(' in a function call")?;
                let args = self.expr_list(&Tok::RParen)?;
                self.expect(&Tok::RParen, "`)' after the arguments")?;
                let slot = self.func_slot(&n);
                self.called.push(n);
                ExprKind::Call(slot, args)
            }
            Tok::Builtin(name) => return self.builtin_call(name, loc),
            Tok::Keyword(Kw::Getline) => {
                let into = self.optional_getline_target()?;
                let src = if self.eat(&Tok::Lt) {
                    GetlineSrc::File(self.concat(no_gt)?)
                } else {
                    GetlineSrc::Main
                };
                ExprKind::Getline(Box::new(Getline { into, src }))
            }
            other => return Err(format!("syntax error at {}", describe(&other))),
        };
        Ok(Expr::new(kind, loc))
    }

    /// The variable `++`/`--` applies to.
    fn lvalue_operand(&mut self, no_gt: bool) -> Result<Lvalue, String> {
        let e = self.primary(no_gt)?;
        match e.kind {
            ExprKind::Get(lv) => Ok(lv),
            _ => Err(
                "syntax error: ++ and -- want a variable, a field or an array element".to_string(),
            ),
        }
    }

    /// `getline`'s optional target, which must be a plain lvalue.
    ///
    /// Only a bare name, a subscripted name, or a `$`-field counts. Anything
    /// else is the *next* expression — `getline > 0` compares the result, it
    /// does not read into the variable `0`.
    fn optional_getline_target(&mut self) -> Result<Option<Lvalue>, String> {
        match self.peek().clone() {
            Tok::Name(n) => {
                self.i = self.i.saturating_add(1);
                let v = self.var(&n);
                if self.eat(&Tok::LBracket) {
                    let subs = self.expr_list(&Tok::RBracket)?;
                    self.expect(&Tok::RBracket, "`]'")?;
                    return Ok(Some(Lvalue::Index(v, subs)));
                }
                Ok(Some(Lvalue::Var(v)))
            }
            Tok::Dollar => {
                self.i = self.i.saturating_add(1);
                let loc = self.loc_prev();
                let inner = self.primary(false)?;
                Ok(Some(Lvalue::Field(Box::new(inner), loc)))
            }
            _ => Ok(None),
        }
    }

    /// A call of a built-in, placed at its name.
    fn builtin_call(&mut self, name: &'static str, loc: Loc) -> Result<Expr, String> {
        let b = builtin_of(name);
        let args = if self.eat(&Tok::LParen) {
            let a = self.expr_list(&Tok::RParen)?;
            self.expect(&Tok::RParen, "`)' after the arguments")?;
            a
        } else {
            // `length` alone is `length($0)`. It is the only built-in that may
            // be written without parentheses, and POSIX says so explicitly.
            if b != Builtin::Length {
                return Err(format!(
                    "syntax error: {name} needs its arguments in parentheses"
                ));
            }
            Vec::new()
        };
        let (_, min, max) = BUILTINS
            .iter()
            .find(|(n, _, _)| *n == name)
            .copied()
            .unwrap_or((name, 0, usize::MAX));
        if args.len() < min || args.len() > max {
            let want = if min == max {
                format!("{min}")
            } else if max == usize::MAX {
                format!("at least {min}")
            } else {
                format!("{min} to {max}")
            };
            return Err(format!(
                "{name}: wants {want} arguments, given {}",
                args.len()
            ));
        }
        // The arguments that must be a particular *shape* rather than any
        // expression. Checking here means `split(s, "x")` is refused before the
        // program runs, not at the line where it first happens.
        match b {
            Builtin::Split
                if !matches!(
                    args.get(1).map(|a| &a.kind),
                    Some(ExprKind::Get(Lvalue::Var(_)))
                ) =>
            {
                return Err("split: the second argument must be an array".to_string());
            }
            Builtin::Sub | Builtin::Gsub => {
                if let Some(target) = args.get(2)
                    && !matches!(target.kind, ExprKind::Get(_))
                {
                    return Err(format!(
                        "{name}: the third argument must be a variable, a field or an array element"
                    ));
                }
            }
            _ => {}
        }
        Ok(Expr::new(ExprKind::Builtin(b, args), loc))
    }
}

fn builtin_of(name: &str) -> Builtin {
    match name {
        "substr" => Builtin::Substr,
        "index" => Builtin::Index,
        "split" => Builtin::Split,
        "sub" => Builtin::Sub,
        "gsub" => Builtin::Gsub,
        "match" => Builtin::Match,
        "sprintf" => Builtin::Sprintf,
        "sin" => Builtin::Sin,
        "cos" => Builtin::Cos,
        "atan2" => Builtin::Atan2,
        "exp" => Builtin::Exp,
        "log" => Builtin::Log,
        "sqrt" => Builtin::Sqrt,
        "int" => Builtin::Int,
        "rand" => Builtin::Rand,
        "srand" => Builtin::Srand,
        "tolower" => Builtin::Tolower,
        "toupper" => Builtin::Toupper,
        "system" => Builtin::System,
        "close" => Builtin::Close,
        "fflush" => Builtin::Fflush,
        _ => Builtin::Length,
    }
}

/// Compile a `/re/` literal the lexer has already put through awk's escape
/// layer, in the syntax gawk compiles with under `--posix`.
///
/// `source` is the text as the program had it -- what the diagnostic quotes,
/// as gawk's does -- and `pattern` what [`escape::regexp`] made of it. A
/// literal that will not compile is gawk's `error:`, a syntax error found
/// while parsing (exit 1), and the sentence after it is glibc's, which
/// [`ere::EreError::message`] carries: gawk prints whatever `regcomp` said.
///
/// `//` is legal awk and matches every record — `awk '//'` is `cat` — and the
/// engine compiles an empty pattern to exactly that.
///
/// # Errors
/// The diagnostic, worded as above.
pub fn compile_literal(source: &[u8], pattern: &[u8]) -> Result<Regex, String> {
    Regex::new_syntax(pattern, false, Syntax::POSIX_AWK).map_err(|e| {
        format!(
            "error: {}: /{}/",
            e.message(),
            String::from_utf8_lossy(source)
        )
    })
}

/// Compile a regex whose text was computed at run time -- the right side of
/// `~`, a `match`, `split`, `sub` or `gsub` pattern, a multi-character `FS` or
/// `RS`: gawk's `make_regexp`, escape layer and compiler both.
///
/// # Errors
/// gawk's fatal diagnostic: `fatal: invalid regexp: <glibc's sentence>:
/// /<text>/`, or the escape layer's own.
pub fn compile_dynamic(text: &[u8], warnings: &mut Warnings) -> Result<Regex, String> {
    escape::compile(text, false, warnings).map_err(|e| match e {
        CompileError::Nul(n) => format!("fatal: {}", n.message()),
        CompileError::Regex(e) => format!(
            "fatal: invalid regexp: {}: /{}/",
            e.message(),
            String::from_utf8_lossy(text)
        ),
    })
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Eof => "the end of the program".to_string(),
        Tok::Newline => "a newline".to_string(),
        Tok::Number(n) => format!("`{n}'"),
        Tok::Str(s) => format!("the string \"{}\"", String::from_utf8_lossy(s)),
        Tok::Ere { source, .. } => format!("the regex /{}/", String::from_utf8_lossy(source)),
        Tok::Name(n) | Tok::FuncName(n) => format!("`{n}'"),
        Tok::Builtin(n) => format!("`{n}'"),
        Tok::Keyword(k) => format!("`{}'", keyword_text(*k)),
        other => format!("`{}'", punct_text(other)),
    }
}

fn keyword_text(k: Kw) -> &'static str {
    match k {
        Kw::Begin => "BEGIN",
        Kw::End => "END",
        Kw::Function => "function",
        Kw::If => "if",
        Kw::Else => "else",
        Kw::While => "while",
        Kw::For => "for",
        Kw::Do => "do",
        Kw::Break => "break",
        Kw::Continue => "continue",
        Kw::Next => "next",
        Kw::NextFile => "nextfile",
        Kw::Exit => "exit",
        Kw::Return => "return",
        Kw::Delete => "delete",
        Kw::In => "in",
        Kw::Getline => "getline",
        Kw::Print => "print",
        Kw::Printf => "printf",
    }
}

fn punct_text(t: &Tok) -> &'static str {
    match t {
        Tok::Semi => ";",
        Tok::LBrace => "{",
        Tok::RBrace => "}",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBracket => "[",
        Tok::RBracket => "]",
        Tok::Comma => ",",
        Tok::Assign => "=",
        Tok::AddAssign => "+=",
        Tok::SubAssign => "-=",
        Tok::MulAssign => "*=",
        Tok::DivAssign => "/=",
        Tok::ModAssign => "%=",
        Tok::PowAssign => "^=",
        Tok::Or => "||",
        Tok::And => "&&",
        Tok::Not => "!",
        Tok::Lt => "<",
        Tok::Le => "<=",
        Tok::Gt => ">",
        Tok::Ge => ">=",
        Tok::Eq => "==",
        Tok::Ne => "!=",
        Tok::Match => "~",
        Tok::NoMatch => "!~",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Star => "*",
        Tok::Slash => "/",
        Tok::Percent => "%",
        Tok::Caret => "^",
        Tok::Incr => "++",
        Tok::Decr => "--",
        Tok::Dollar => "$",
        Tok::Question => "?",
        Tok::Colon => ":",
        Tok::Pipe => "|",
        Tok::Append => ">>",
        _ => "?",
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    /// The statement inside the [`Stmt::At`] that every parsed statement
    /// arrives in.
    fn bare(s: &Stmt) -> &Stmt {
        match s {
            Stmt::At(_, inner) => inner,
            other => other,
        }
    }

    fn ok(src: &str) -> Program {
        parse(
            src.as_bytes(),
            &SourceMap::operand(src.as_bytes()),
            &mut Warnings::default(),
            &mut Vec::new(),
        )
        .unwrap_or_else(|e| panic!("parsing {src:?}: {e}"))
    }
    fn err(src: &str) -> String {
        parse_error(src).to_string()
    }
    fn parse_error(src: &str) -> ParseError {
        parse(
            src.as_bytes(),
            &SourceMap::operand(src.as_bytes()),
            &mut Warnings::default(),
            &mut Vec::new(),
        )
        .unwrap_err()
    }

    /// Which failures are gawk's `fatal:` is carried beside the message, not
    /// read back out of it: a syntax error can quote a string that says
    /// `fatal:` and is still a syntax error.
    #[test]
    fn a_fatal_parse_error_is_told_from_a_syntax_error_by_kind_not_text() {
        // gawk --posix: a backslash-newline inside a string is `fatal: POSIX
        // does not allow physical newlines in string values`, placed on the
        // line of the backslash.
        let newline = parse_error("BEGIN { x = 1 }\nBEGIN { print \"a\\\nb\" }");
        assert!(newline.fatal);
        assert_eq!(
            newline.messages,
            ["cmd. line:2: fatal: POSIX does not allow physical newlines in string values"]
        );
        // A regex literal that will not compile is gawk's `error:` -- placed,
        // but exit 1 like any syntax error.
        let bad_regex = parse_error("BEGIN { x = 1 }\n/a(/");
        assert!(!bad_regex.fatal);
        assert_eq!(
            bad_regex.messages,
            [r"cmd. line:2: error: Unmatched ( or \(: /a(/"]
        );

        let syntax = parse_error(r#"BEGIN { delete "a: fatal: b" }"#);
        assert!(!syntax.fatal, "{syntax}");
        assert!(syntax.to_string().contains("a: fatal: b"), "{syntax}");
    }

    /// gawk folds a divisor that is a constant and refuses a zero one while
    /// it parses (`mk_binary`): an `error:` at the operator, after which the
    /// parse carries on and the program does not run. Measured against gawk
    /// 5.2.1 `--posix`, case by case.
    #[test]
    fn a_constant_zero_divisor_is_refused_where_gawk_folds_it() {
        for src in [
            "BEGIN { print 1/0 }",
            "BEGIN { x = 1; print x/0.0 }",
            "BEGIN { print 1/-0 }",
            "BEGIN { x = 1; print x/1e-400 }",
            "BEGIN { print 2^-1/0 }",
            "BEGIN { x = 1; print x / 0^2 }",
            "BEGIN { x = 1; print x / !1 }",
            r#"BEGIN { x = 1; print x / !"a" }"#,
            // In a branch that never runs, and in a function never called:
            // it is a property of the text.
            r#"BEGIN { if (0) print 1/0; print "ran" }"#,
            r#"function f() { return 1/0 } BEGIN { print "x" }"#,
        ] {
            let e = parse_error(src);
            assert!(!e.fatal, "{src}");
            assert_eq!(
                e.messages,
                ["cmd. line:1: error: division by zero attempted"],
                "{src}"
            );
        }
        let m = parse_error("BEGIN { x = 3; print x % 0 }");
        assert_eq!(
            m.messages,
            ["cmd. line:1: error: division by zero attempted in `%'"]
        );
        // What gawk does not fold, it leaves to run time.
        for src in [
            "BEGIN { print 1/(0) }",
            "BEGIN { print 1/(2-2) }",
            "BEGIN { print 1/+0 }",
            r#"BEGIN { print 1/"0" }"#,
            "BEGIN { print 1/z }",
            "BEGIN { x = 1; print x / 0^-1 }",
            "BEGIN { x = 1; x /= 0 }",
        ] {
            let _ = ok(src);
        }
    }

    /// The parse goes on past an `error:`, so each is reported, in order,
    /// and so is the syntax error or regex literal that stops it after them.
    #[test]
    fn errors_are_collected_in_reading_order() {
        let two = parse_error("BEGIN { print 1/0\n print 2%0 }");
        assert_eq!(
            two.messages,
            [
                "cmd. line:1: error: division by zero attempted",
                "cmd. line:2: error: division by zero attempted in `%'",
            ]
        );
        let then_regex = parse_error("BEGIN { print 1/0 }\n/a(/");
        assert_eq!(
            then_regex.messages,
            [
                "cmd. line:1: error: division by zero attempted",
                r"cmd. line:2: error: Unmatched ( or \(: /a(/",
            ]
        );
        let then_syntax = parse_error("BEGIN { print 1/0\n print ( }");
        assert_eq!(then_syntax.messages.len(), 2, "{then_syntax}");
        assert_eq!(
            then_syntax.messages.first().map(String::as_str),
            Some("cmd. line:1: error: division by zero attempted")
        );
        // A regex literal stops gawk's parse: only the first is reported.
        let regexes = parse_error("/a(/\n/b(/");
        assert_eq!(
            regexes.messages,
            [r"cmd. line:1: error: Unmatched ( or \(: /a(/"]
        );
    }

    /// The lexer reads the whole program first; what the parser met before
    /// the lexer's stopping point keeps its place ahead of the lexer's own
    /// complaint, as gawk, whose parser pulls one token at a time, says them.
    #[test]
    fn a_lexing_error_is_ordered_as_gawk_meets_it() {
        let after_error = parse_error("BEGIN { print 1/0 }\nBEGIN { print \"a\\\nb\" }");
        assert!(after_error.fatal);
        assert_eq!(
            after_error.messages,
            [
                "cmd. line:1: error: division by zero attempted",
                "cmd. line:2: fatal: POSIX does not allow physical newlines in string values",
            ]
        );
        // A syntax error before the lexer stopped is gawk's report, alone.
        let syntax_first = parse_error("BEGIN { print ( }\nBEGIN { print \"a\\\nb\" }");
        assert!(!syntax_first.fatal, "{syntax_first}");
        assert_eq!(syntax_first.messages.len(), 1, "{syntax_first}");
        assert!(
            !syntax_first.to_string().contains("POSIX"),
            "{syntax_first}"
        );
    }

    /// gawk refuses `next` and `nextfile` in BEGIN and END as it parses
    /// them, and leaves a function's to run time.
    #[test]
    fn next_in_begin_or_end_is_an_error() {
        assert_eq!(
            parse_error("BEGIN { next }").messages,
            ["cmd. line:1: error: `next' used in BEGIN action"]
        );
        assert_eq!(
            parse_error("END { x = 1\n nextfile }").messages,
            ["cmd. line:2: error: `nextfile' used in END action"]
        );
        let _ = ok("function f() { next } { f() }");
        let _ = ok("{ next } END { print NR }");
    }

    /// `printf("%s\n", x)`: one parenthesised list, told from expressions
    /// that merely begin with a grouping by what follows the `)`.
    #[test]
    fn a_print_list_may_be_parenthesised() {
        let args_of = |src: &str| -> (usize, bool) {
            let p = ok(src);
            match p.begin.first().map(bare) {
                Some(Stmt::Print(args, r) | Stmt::Printf(args, r)) => (args.len(), r.is_some()),
                other => panic!("{src:?}: {other:?}"),
            }
        };
        assert_eq!(args_of(r#"BEGIN { printf("%s-%s\n", 1, 2) }"#), (3, false));
        assert_eq!(args_of(r#"BEGIN { print("a", "b") }"#), (2, false));
        assert_eq!(args_of(r#"BEGIN { print("a", "b") > "f" }"#), (2, true));
        assert_eq!(args_of("BEGIN { printf(\"%s\\n\",\n 1) }"), (2, false));
        // Not the list form: the grouping is the start of an expression.
        assert_eq!(args_of(r#"BEGIN { print ("a")("b") }"#), (1, false));
        assert_eq!(args_of("BEGIN { print (1,2) in a }"), (1, false));
        assert_eq!(args_of("BEGIN { print (1), (2) }"), (2, false));
        assert!(err("BEGIN { print () }").contains("syntax error"));
    }

    /// An expression is placed at its operator, so a statement that spans
    /// lines names the line its failing operation is on.
    #[test]
    fn an_expression_is_placed_at_its_operator() {
        let p = ok("BEGIN {\n if (1 &&\n  1/z) print }");
        let Some(Stmt::If(cond, ..)) = p.begin.first().map(bare) else {
            panic!("no if");
        };
        assert_eq!(cond.loc.line, 2, "the `&&`");
        let ExprKind::And(_, rhs) = &cond.kind else {
            panic!("not an and");
        };
        assert_eq!(rhs.loc.line, 3, "the `/`");
        assert!(matches!(rhs.kind, ExprKind::Bin(BinOp::Div, ..)));
    }

    #[test]
    fn a_bare_pattern_gets_the_default_action() {
        let p = ok("/x/");
        assert_eq!(p.rules.len(), 1);
        assert!(p.rules.first().is_some_and(|r| r.action.is_none()));
    }

    #[test]
    fn a_range_pattern_is_told_from_two_arguments() {
        let p = ok("/a/,/b/ { print }");
        assert!(matches!(
            p.rules.first().map(|r| &r.pattern),
            Some(Pattern::Range(_, _, 0))
        ));
        assert_eq!(p.ranges, 1);
    }

    #[test]
    fn print_treats_a_bare_gt_as_a_redirection() {
        let p = ok(r#"{ print "x" > "f" }"#);
        let Some(Stmt::Print(_, Some(r))) = p
            .rules
            .first()
            .and_then(|r| r.action.as_ref())
            .and_then(|a| a.first())
            .map(bare)
        else {
            panic!("expected a redirected print");
        };
        assert_eq!(r.mode, RedirMode::Truncate);
        // …but a parenthesised `>` still compares.
        let p = ok(r#"{ print ("a" > "b") }"#);
        let Some(Stmt::Print(args, None)) = p
            .rules
            .first()
            .and_then(|r| r.action.as_ref())
            .and_then(|a| a.first())
            .map(bare)
        else {
            panic!("expected an unredirected print");
        };
        assert!(matches!(
            args.first().map(|a| &a.kind),
            Some(ExprKind::Cmp(CmpOp::Gt, _, _))
        ));
    }

    #[test]
    fn the_two_for_loops_are_told_apart() {
        assert!(matches!(
            ok("{ for (k in a) print k }")
                .rules
                .first()
                .and_then(|r| r.action.as_ref())
                .and_then(|a| a.first())
                .map(bare),
            Some(Stmt::ForIn { .. })
        ));
        assert!(matches!(
            ok("{ for (i = 1; i <= 3; i++) print i }")
                .rules
                .first()
                .and_then(|r| r.action.as_ref())
                .and_then(|a| a.first())
                .map(bare),
            Some(Stmt::For { .. })
        ));
    }

    #[test]
    fn a_function_may_be_called_before_it_is_defined() {
        let p = ok("BEGIN { print f(1) } function f(x) { return x + 1 }");
        assert_eq!(p.funcs.len(), 1);
        assert_eq!(p.funcs.first().map(|f| f.params.len()), Some(1));
    }

    #[test]
    fn calling_a_function_that_does_not_exist_is_caught_before_running() {
        assert!(err("BEGIN { nope(1) }").contains("undefined function nope"));
    }

    #[test]
    fn a_parameter_shadows_a_global_of_the_same_name() {
        let p = ok("function f(x) { return x } BEGIN { x = 1; print f(2), x }");
        let Some(f) = p.funcs.first() else {
            panic!("no function")
        };
        assert!(matches!(
            f.body.first().map(bare),
            Some(Stmt::Return(Some(Expr {
                kind: ExprKind::Get(Lvalue::Var(VarRef::Local(0))),
                ..
            })))
        ));
    }

    #[test]
    fn comparison_does_not_chain() {
        // `a < b < c` is a syntax error in awk, not `(a<b)<c` as in C.
        assert!(err("BEGIN { x = 1 < 2 < 3 }").contains("syntax error"));
    }

    #[test]
    fn precedence_is_awks_and_not_cs() {
        // WHAT THIS USED TO DO, AND WHY IT WAS WORTH REWRITING.
        //
        // It parsed the same four programs and then wrote `let _ = ok(c);`,
        // discarding every result. `ok` panics if a program fails to parse, so
        // the test did check that these four *parse* -- but its name claims
        // something else entirely, and the comment above the cases said "the
        // four that catch a hand-rolled parser out". A parser that read `^` as
        // left-associative, or bound unary minus tighter than `^`, or applied
        // `$` to the whole of `NF - 1`, would have produced a perfectly valid
        // tree for all four and passed. The one property named in the test was
        // the one property not checked.
        //
        // Each case below is written so the WRONG tree is a different shape,
        // not merely a different value, so the assertion cannot be satisfied
        // by an accident of evaluation order.

        // `2 ^ 3 ^ 2` is 2^(3^2) = 512 in awk, not (2^3)^2 = 64. Right-
        // associative, which is the opposite of every other binary operator
        // here and the reason `^` is worth its own case.
        let ExprKind::Bin(BinOp::Pow, lhs, rhs) = print_arg("2 ^ 3 ^ 2") else {
            panic!("`2 ^ 3 ^ 2` did not parse as a power at the top");
        };
        assert!(
            matches!(lhs.kind, ExprKind::Num(n) if (n - 2.0).abs() < f64::EPSILON),
            "left operand should be the bare 2, so the nesting is on the right"
        );
        assert!(
            matches!(rhs.kind, ExprKind::Bin(BinOp::Pow, _, _)),
            "`^` must be right-associative: 2^(3^2), not (2^3)^2"
        );

        // `-2 ^ 2` is -(2^2) = -4, not (-2)^2 = 4: `^` binds tighter than
        // unary minus, which is the case C gets the other way round.
        let neg = print_arg("-2 ^ 2");
        let ExprKind::Neg(inner) = neg else {
            panic!("`-2 ^ 2` should negate a power, not raise a negative");
        };
        assert!(
            matches!(inner.kind, ExprKind::Bin(BinOp::Pow, _, _)),
            "`^` binds tighter than unary minus: -(2^2), not (-2)^2"
        );

        // `$NF - 1` is ($NF) - 1, not $(NF - 1). Getting this wrong reads the
        // second-to-last field instead of subtracting from the last one, and
        // both are valid programs -- which is exactly why it needs a shape
        // assertion rather than a parse check.
        let ExprKind::Bin(BinOp::Sub, lhs, _) = print_arg("$NF - 1") else {
            panic!("`$NF - 1` should subtract at the top, not index a field");
        };
        assert!(
            matches!(lhs.kind, ExprKind::Get(Lvalue::Field(..))),
            "`$` binds tighter than `-`: ($NF) - 1, not $(NF - 1)"
        );

        // Concatenation is an operator with no symbol, and it binds looser
        // than arithmetic. `1 " " 2` is three operands joined, not a number.
        assert!(
            matches!(print_arg(r#"1 " " 2"#), ExprKind::Concat(_, _)),
            "adjacent expressions concatenate"
        );
    }

    /// The single expression of `BEGIN { print <src> }`.
    ///
    /// Precedence is a claim about the *shape* of the tree, so the tests above
    /// need the tree rather than a yes/no on parsing. Every failure here is a
    /// panic naming the program, because a test that cannot reach its subject
    /// has not passed.
    fn print_arg(src: &str) -> ExprKind {
        // A `BEGIN` block lands in `program.begin`, not in `program.rules` --
        // the first draft of this helper looked in `rules` and reported "no
        // rule parsed", which reads like a parser failure and was a navigation
        // mistake in the test.
        let program = ok(&format!("BEGIN {{ print {src} }}"));
        let stmt = program
            .begin
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no BEGIN statement parsed from {src:?}"));
        let Stmt::At(_, stmt) = stmt else {
            panic!("{src:?} parsed to a statement with no location");
        };
        let Stmt::Print(mut args, _) = *stmt else {
            panic!("{src:?} did not parse as a print");
        };
        assert_eq!(args.len(), 1, "{src:?} should print exactly one expression");
        args.pop().unwrap_or_else(|| unreachable!()).kind
    }

    #[test]
    fn getline_in_all_its_forms() {
        for src in [
            "{ getline }",
            "{ getline line }",
            "{ getline < \"f\" }",
            "{ getline line < \"f\" }",
            "{ \"cmd\" | getline }",
            "{ \"cmd\" | getline line }",
            "{ while ((\"cmd\" | getline line) > 0) print line }",
        ] {
            let _ = ok(src);
        }
    }

    #[test]
    fn the_builtins_check_their_arity_before_the_program_runs() {
        assert!(err("BEGIN { substr(\"a\") }").contains("wants"));
        assert!(err("BEGIN { split(\"a\", \"b\") }").contains("must be an array"));
        assert!(err("BEGIN { sub(/a/, \"b\", \"c\") }").contains("must be a variable"));
        // `length` is the one built-in that may drop its parentheses.
        let _ = ok("{ print length }");
    }

    #[test]
    fn a_newline_inside_a_continued_construct_is_not_a_terminator() {
        let _ = ok("BEGIN {\n  if (1 &&\n      2)\n    print \"y\"\n  else\n    print \"n\"\n}");
        let _ = ok("BEGIN { print 1,\n 2 }");
    }

    #[test]
    fn break_outside_a_loop_is_refused() {
        assert!(err("BEGIN { break }").contains("outside a loop"));
        assert!(err("BEGIN { return }").contains("outside a function"));
        let _ = ok("BEGIN { while (1) break }");
    }
}
