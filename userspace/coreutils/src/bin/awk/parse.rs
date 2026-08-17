//! The awk parser: tokens to [`Program`].
//!
//! ## The three ambiguities awk's grammar actually has
//!
//! **`print a > b`.** The `>` is a redirection, not a comparison — but only at
//! the top level of a print's argument list, so `print (a > b)` compares. The
//! parser carries a `no_gt` flag through expression parsing for exactly this,
//! rather than trying to undo the parse afterwards.
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
    BinOp, Builtin, CmpOp, Expr, Func, Getline, GetlineSrc, Lvalue, Pattern, Program, RedirMode,
    Redirect, Rule, SPECIALS, Stmt, VarRef,
};
use crate::lex::{BUILTINS, Kw, Lexer, Tok, Token};
use ere::Regex;
use std::collections::HashMap;
use std::rc::Rc;

/// Parse a whole program.
///
/// # Errors
/// Returns a one-line diagnostic. awk parses the entire program before running
/// any of it, so a syntax error in a rule that would never have matched is
/// still fatal — better than dying halfway through a report.
pub fn parse(src: &[u8]) -> Result<Program, String> {
    let tokens = Lexer::new(src).tokens()?;
    let mut p = Parser {
        toks: tokens,
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
    };
    let prog = p.program()?;
    Ok(prog)
}

struct Parser {
    toks: Vec<Token>,
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
}

impl Parser {
    // ---- token access -----------------------------------------------------

    fn peek(&self) -> &Tok {
        self.toks.get(self.i).map_or(&Tok::Eof, |t| &t.kind)
    }
    fn peek_at(&self, k: usize) -> &Tok {
        self.toks
            .get(self.i.saturating_add(k))
            .map_or(&Tok::Eof, |t| &t.kind)
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
                self.function()?;
            } else if self.eat(&Tok::Keyword(Kw::Begin)) {
                self.skip_newlines();
                let body = self.block()?;
                prog.begin.extend(body);
            } else if self.eat(&Tok::Keyword(Kw::End)) {
                self.skip_newlines();
                let body = self.block()?;
                prog.end.extend(body);
            } else if self.peek() == &Tok::LBrace {
                let action = self.block()?;
                prog.rules.push(Rule {
                    pattern: Pattern::Always,
                    action: Some(action),
                });
            } else {
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
        prog.funcs = self
            .funcs
            .iter()
            .map(|f| {
                f.clone().unwrap_or(Func {
                    name: String::new(),
                    params: Vec::new(),
                    body: Vec::new(),
                })
            })
            .collect();
        prog.globals = self.globals.len();
        prog.global_names = std::mem::take(&mut self.globals);
        prog.ranges = self.ranges;
        Ok(prog)
    }

    fn function(&mut self) -> Result<(), String> {
        let name = match self.bump() {
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
            *entry = Some(Func { name, params, body });
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

    /// A statement, plus whatever terminates it.
    fn stmt(&mut self) -> Result<Stmt, String> {
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
            Tok::Keyword(Kw::Next) => {
                self.i = self.i.saturating_add(1);
                Ok(Stmt::Next)
            }
            Tok::Keyword(Kw::NextFile) => {
                self.i = self.i.saturating_add(1);
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
            let var = Lvalue::Var(self.var(&n));
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
        if !matches!(
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
        self.skip_newlines();
        let Expr::Get(target) = lhs else {
            return Err("syntax error: the left side of an assignment must be a variable, a field or an array element".to_string());
        };
        let rhs = Box::new(self.expr(no_gt)?);
        Ok(match op {
            None => Expr::Assign(target, rhs),
            Some(o) => Expr::AugAssign(target, o, rhs),
        })
    }

    fn ternary(&mut self, no_gt: bool) -> Result<Expr, String> {
        let cond = self.or(no_gt)?;
        if !self.eat(&Tok::Question) {
            return Ok(cond);
        }
        self.skip_newlines();
        let yes = self.expr(no_gt)?;
        self.skip_newlines();
        self.expect(&Tok::Colon, "`:' in a ?: expression")?;
        self.skip_newlines();
        let no = self.expr(no_gt)?;
        Ok(Expr::Cond(Box::new(cond), Box::new(yes), Box::new(no)))
    }

    fn or(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.and(no_gt)?;
        while self.eat(&Tok::Or) {
            self.skip_newlines();
            lhs = Expr::Or(Box::new(lhs), Box::new(self.and(no_gt)?));
        }
        Ok(lhs)
    }

    fn and(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.in_expr(no_gt)?;
        while self.eat(&Tok::And) {
            self.skip_newlines();
            lhs = Expr::And(Box::new(lhs), Box::new(self.in_expr(no_gt)?));
        }
        Ok(lhs)
    }

    fn in_expr(&mut self, no_gt: bool) -> Result<Expr, String> {
        let mut lhs = self.match_expr(no_gt)?;
        while self.peek() == &Tok::Keyword(Kw::In) {
            self.i = self.i.saturating_add(1);
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
            lhs = Expr::In(vec![lhs], arr);
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
            let rhs = self.relational(no_gt)?;
            lhs = Expr::Match {
                neg,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
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
        self.skip_newlines();
        let rhs = self.pipe_getline(no_gt)?;
        Ok(Expr::Cmp(op, Box::new(lhs), Box::new(rhs)))
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
            let into = self.optional_getline_target()?;
            lhs = Expr::Getline(Box::new(Getline {
                into,
                src: GetlineSrc::Cmd(lhs),
            }));
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
            lhs = Expr::Concat(Box::new(lhs), Box::new(self.additive(no_gt)?));
        }
        Ok(lhs)
    }

    fn starts_operand(&self) -> bool {
        match self.peek() {
            Tok::Number(_)
            | Tok::Str(_)
            | Tok::Ere(_)
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
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(self.multiplicative(no_gt)?));
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
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(self.unary(no_gt)?));
        }
    }

    fn unary(&mut self, no_gt: bool) -> Result<Expr, String> {
        match self.peek() {
            Tok::Not => {
                self.i = self.i.saturating_add(1);
                Ok(Expr::Not(Box::new(self.unary(no_gt)?)))
            }
            Tok::Minus => {
                self.i = self.i.saturating_add(1);
                Ok(Expr::Neg(Box::new(self.unary(no_gt)?)))
            }
            Tok::Plus => {
                self.i = self.i.saturating_add(1);
                Ok(Expr::Pos(Box::new(self.unary(no_gt)?)))
            }
            _ => self.power(no_gt),
        }
    }

    /// `^` is right-associative and binds tighter than unary minus on the
    /// right: `2^3^2` is 512, and `-2^2` is -4.
    fn power(&mut self, no_gt: bool) -> Result<Expr, String> {
        let base = self.postfix(no_gt)?;
        if self.eat(&Tok::Caret) {
            let exp = self.unary(no_gt)?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn postfix(&mut self, no_gt: bool) -> Result<Expr, String> {
        let e = self.primary(no_gt)?;
        // `x++` only makes sense on an lvalue; `(a+b)++` is a parse of `(a+b)`
        // followed by `++` starting the next operand, and leaving it alone here
        // is what lets that keep working.
        if let Expr::Get(lv) = &e {
            if self.eat(&Tok::Incr) {
                return Ok(Expr::PostIncr(lv.clone(), 1.0));
            }
            if self.eat(&Tok::Decr) {
                return Ok(Expr::PostIncr(lv.clone(), -1.0));
            }
        }
        Ok(e)
    }

    fn primary(&mut self, no_gt: bool) -> Result<Expr, String> {
        match self.bump() {
            Tok::Number(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(Rc::new(s))),
            Tok::Ere(s) => Ok(Expr::Regex(Rc::new(compile_regex(&s)?))),
            Tok::Dollar => {
                // `$` binds tighter than everything but `()` and `++`, so
                // `$NF-1` is `($NF)-1` and `$i++` increments `$i`.
                let inner = self.primary(no_gt)?;
                Ok(Expr::Get(Lvalue::Field(Box::new(inner))))
            }
            Tok::Incr => {
                let target = self.lvalue_operand(no_gt)?;
                Ok(Expr::PreIncr(target, 1.0))
            }
            Tok::Decr => {
                let target = self.lvalue_operand(no_gt)?;
                Ok(Expr::PreIncr(target, -1.0))
            }
            Tok::LParen => {
                let items = self.expr_list(&Tok::RParen)?;
                self.expect(&Tok::RParen, "`)'")?;
                if self.peek() == &Tok::Keyword(Kw::In) {
                    self.i = self.i.saturating_add(1);
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
                    return Ok(Expr::In(items, arr));
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
                Ok(first)
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
                    return Ok(Expr::Get(Lvalue::Index(v, subs)));
                }
                Ok(Expr::Get(Lvalue::Var(v)))
            }
            Tok::FuncName(n) => {
                self.expect(&Tok::LParen, "`(' in a function call")?;
                let args = self.expr_list(&Tok::RParen)?;
                self.expect(&Tok::RParen, "`)' after the arguments")?;
                let slot = self.func_slot(&n);
                self.called.push(n);
                Ok(Expr::Call(slot, args))
            }
            Tok::Builtin(name) => self.builtin_call(name),
            Tok::Keyword(Kw::Getline) => {
                let into = self.optional_getline_target()?;
                if self.eat(&Tok::Lt) {
                    let file = self.concat(no_gt)?;
                    return Ok(Expr::Getline(Box::new(Getline {
                        into,
                        src: GetlineSrc::File(file),
                    })));
                }
                Ok(Expr::Getline(Box::new(Getline {
                    into,
                    src: GetlineSrc::Main,
                })))
            }
            other => Err(format!("syntax error at {}", describe(&other))),
        }
    }

    /// The variable `++`/`--` applies to.
    fn lvalue_operand(&mut self, no_gt: bool) -> Result<Lvalue, String> {
        let e = self.primary(no_gt)?;
        match e {
            Expr::Get(lv) => Ok(lv),
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
                let inner = self.primary(false)?;
                Ok(Some(Lvalue::Field(Box::new(inner))))
            }
            _ => Ok(None),
        }
    }

    fn builtin_call(&mut self, name: &'static str) -> Result<Expr, String> {
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
            Builtin::Split if !matches!(args.get(1), Some(Expr::Get(Lvalue::Var(_)))) => {
                return Err("split: the second argument must be an array".to_string());
            }
            Builtin::Sub | Builtin::Gsub => {
                if let Some(target) = args.get(2)
                    && !matches!(target, Expr::Get(_))
                {
                    return Err(format!(
                        "{name}: the third argument must be a variable, a field or an array element"
                    ));
                }
            }
            _ => {}
        }
        Ok(Expr::Builtin(b, args))
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

/// Compile a `/re/` literal, wording the failure the way awk does.
///
/// `//` is legal awk and matches every record — `awk '//'` is `cat`. The engine
/// refuses an empty pattern outright, because its first caller was the shell's
/// `[[ =~ ]]`, where bash makes `[[ x =~ "" ]]` an error rather than a match; an
/// empty *group* is fine there, so that is what the empty pattern becomes.
pub fn compile_regex(pat: &[u8]) -> Result<Regex, String> {
    let source = if pat.is_empty() {
        b"()".as_slice()
    } else {
        pat
    };
    Regex::new(source).map_err(|e| {
        let shown = String::from_utf8_lossy(pat).into_owned();
        let why = String::from_utf8_lossy(&e.0).into_owned();
        format!("/{shown}/: {why}")
    })
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Eof => "the end of the program".to_string(),
        Tok::Newline => "a newline".to_string(),
        Tok::Number(n) => format!("`{n}'"),
        Tok::Str(s) => format!("the string \"{}\"", String::from_utf8_lossy(s)),
        Tok::Ere(s) => format!("the regex /{}/", String::from_utf8_lossy(s)),
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
mod tests {
    use super::*;

    fn ok(src: &str) -> Program {
        parse(src.as_bytes()).unwrap_or_else(|e| panic!("parsing {src:?}: {e}"))
    }
    fn err(src: &str) -> String {
        parse(src.as_bytes()).unwrap_err()
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
        else {
            panic!("expected an unredirected print");
        };
        assert!(matches!(args.first(), Some(Expr::Cmp(CmpOp::Gt, _, _))));
    }

    #[test]
    fn the_two_for_loops_are_told_apart() {
        assert!(matches!(
            ok("{ for (k in a) print k }")
                .rules
                .first()
                .and_then(|r| r.action.as_ref())
                .and_then(|a| a.first()),
            Some(Stmt::ForIn { .. })
        ));
        assert!(matches!(
            ok("{ for (i = 1; i <= 3; i++) print i }")
                .rules
                .first()
                .and_then(|r| r.action.as_ref())
                .and_then(|a| a.first()),
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
            f.body.first(),
            Some(Stmt::Return(Some(Expr::Get(Lvalue::Var(VarRef::Local(0))))))
        ));
    }

    #[test]
    fn comparison_does_not_chain() {
        // `a < b < c` is a syntax error in awk, not `(a<b)<c` as in C.
        assert!(err("BEGIN { x = 1 < 2 < 3 }").contains("syntax error"));
    }

    #[test]
    fn precedence_is_awks_and_not_cs() {
        // These are the four that catch a hand-rolled parser out.
        let cases = [
            "BEGIN { print 2 ^ 3 ^ 2 }", // right-assoc
            "BEGIN { print -2 ^ 2 }",    // -(2^2)
            "BEGIN { print $NF - 1 }",   // ($NF) - 1
            "BEGIN { print 1 \" \" 2 }", // concatenation
        ];
        for c in cases {
            let _ = ok(c);
        }
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
