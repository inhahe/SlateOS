//! A generated lexer function -- `ts_lex`, `ts_lex_keywords` -- read from C
//! and written out as Rust.
//!
//! The generator writes every lexer as one state machine in one shape:
//!
//! ```c
//! static bool ts_lex(TSLexer *lexer, TSStateId state) {
//!   START_LEXER();
//!   eof = lexer->eof(lexer);
//!   switch (state) {
//!     case 0:
//!       if (eof) ADVANCE(74);
//!       ADVANCE_MAP('!', 172, '"', 158, ...);
//!       if (('\t' <= lookahead && lookahead <= '\r') || lookahead == ' ') SKIP(69);
//!       if (set_contains(sym_identifier_character_set_1, 685, lookahead)) ADVANCE(192);
//!       END_STATE();
//!     case 1:
//!       ACCEPT_TOKEN(anon_sym_SEMI);
//!       END_STATE();
//!     ...
//!     default:
//!       return false;
//!   }
//! }
//! ```
//!
//! where the macros (`tree_sitter/parser.h`) make each `ADVANCE` and `SKIP` a
//! jump back to the top: advance the lexer, read the next character, and
//! switch on the new state. In Rust each state is a function of its own,
//! answering where to go next (`LexStep`), and one loop in the syntax crate
//! (`ffi::run_lexer`) takes the character and calls the next state's
//! function out of a table. The conditions are C boolean expressions over
//! `lookahead` and `eof`, which read the same in Rust once each character
//! literal is written as its number.
//!
//! A state a function, rather than the whole machine one `loop` around one
//! `match`, is for the optimiser: a lexer has thousands of states, and one
//! function of thousands of arms took a release build of the syntax crate
//! more than a quarter of an hour of CPU, the optimiser's passes being far
//! from linear in the size of a function.

use core::fmt::Write as _;

use crate::Error;
use crate::cinit::{Constants, Parser};
use crate::ctoken::Tok;

/// One statement of a lexer's state.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stmt {
    /// `ADVANCE(n)`: go to state `n`, keeping the character.
    Advance(i64),
    /// `SKIP(n)`: go to state `n`, the character not part of the token.
    Skip(i64),
    /// `ADVANCE_MAP(c, n, ...)`: the state for each character.
    AdvanceMap(Vec<(i64, i64)>),
    /// `ACCEPT_TOKEN(sym)`: the token so far is `sym`.
    Accept(i64),
    /// `END_STATE()`: stop, answering whether a token was accepted.
    End,
    /// `return false;` -- a state that does not exist.
    ReturnFalse,
    /// `if (cond) stmt`, the condition already written as Rust.
    If(String, Box<Stmt>),
    /// `{ stmts }`.
    Block(Vec<Stmt>),
}

/// A lexer function, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LexFn {
    /// Whether it reads `eof` at each character (`eof = lexer->eof(lexer)`).
    reads_eof: bool,
    /// Its states, by number.
    states: Vec<(i64, Vec<Stmt>)>,
    /// What an unknown state does (the `default:`).
    default: Vec<Stmt>,
}

/// Read the body of a lexer function -- the tokens between its braces.
pub(crate) fn read(
    body: &[(Tok<'_>, usize)],
    constants: &Constants,
    sets: &[(String, usize)],
) -> Result<LexFn, Error> {
    let mut p = Parser::new(body);
    p.expect_word("START_LEXER")?;
    p.expect("(")?;
    p.expect(")")?;
    p.expect(";")?;
    let mut reads_eof = false;
    if p.eat_word("eof") {
        p.expect("=")?;
        p.expect_word("lexer")?;
        p.expect("->")?;
        p.expect_word("eof")?;
        p.expect("(")?;
        p.expect_word("lexer")?;
        p.expect(")")?;
        p.expect(";")?;
        reads_eof = true;
    }
    p.expect_word("switch")?;
    p.expect("(")?;
    p.expect_word("state")?;
    p.expect(")")?;
    p.expect("{")?;
    let mut states: Vec<(i64, Vec<Stmt>)> = Vec::new();
    let mut default = Vec::new();
    // Which list the statements go into: a state's, or the default's.
    let mut in_default = false;
    loop {
        if p.eat("}") {
            break;
        }
        if p.eat_word("case") {
            let line = p.line();
            let value = match p.next() {
                Some(Tok::Int(v)) => *v,
                other => {
                    return Err(Error::at(
                        line,
                        format!("a case that is not a number: {other:?}"),
                    ));
                }
            };
            p.expect(":")?;
            states.push((value, Vec::new()));
            in_default = false;
            continue;
        }
        if p.eat_word("default") {
            p.expect(":")?;
            in_default = true;
            continue;
        }
        let stmt = statement(&mut p, constants, sets)?;
        let list = if in_default {
            &mut default
        } else {
            &mut states
                .last_mut()
                .ok_or_else(|| Error::at(p.line(), "a statement before the first case"))?
                .1
        };
        list.push(stmt);
    }
    if !p.at_end() {
        return Err(Error::at(p.line(), "something after the lexer's switch"));
    }
    Ok(LexFn {
        reads_eof,
        states,
        default,
    })
}

fn statement(
    p: &mut Parser<'_, '_>,
    constants: &Constants,
    sets: &[(String, usize)],
) -> Result<Stmt, Error> {
    let line = p.line();
    if p.eat("{") {
        let mut stmts = Vec::new();
        while !p.eat("}") {
            stmts.push(statement(p, constants, sets)?);
        }
        return Ok(Stmt::Block(stmts));
    }
    if p.eat_word("if") {
        p.expect("(")?;
        let cond = condition(p, constants, sets)?;
        let then = statement(p, constants, sets)?;
        return Ok(Stmt::If(cond, Box::new(then)));
    }
    if p.eat_word("return") {
        p.expect_word("false")?;
        p.expect(";")?;
        return Ok(Stmt::ReturnFalse);
    }
    let name = p.ident()?;
    p.expect("(")?;
    let mut args = Vec::new();
    if !p.eat(")") {
        loop {
            let e = p.expr()?;
            args.push(constants.eval(&e, line)?);
            if p.eat(")") {
                break;
            }
            p.expect(",")?;
            // ADVANCE_MAP's list ends with a comma before its parenthesis.
            if p.eat(")") {
                break;
            }
        }
    }
    p.expect(";")?;
    match (name, args.as_slice()) {
        ("ADVANCE", [n]) => Ok(Stmt::Advance(*n)),
        ("SKIP", [n]) => Ok(Stmt::Skip(*n)),
        ("ACCEPT_TOKEN", [sym]) => Ok(Stmt::Accept(*sym)),
        ("END_STATE", []) => Ok(Stmt::End),
        ("ADVANCE_MAP", pairs) if pairs.len() % 2 == 0 => {
            // The first entry for a character wins, as C's loop finds it.
            let mut map: Vec<(i64, i64)> = Vec::with_capacity(pairs.len() / 2);
            for pair in pairs.chunks_exact(2) {
                if let [c, n] = pair
                    && !map.iter().any(|(seen, _)| seen == c)
                {
                    map.push((*c, *n));
                }
            }
            Ok(Stmt::AdvanceMap(map))
        }
        _ => Err(Error::at(
            line,
            format!("a lexer statement this does not know: {name}({args:?})"),
        )),
    }
}

/// An `if`'s condition, up to and including its closing parenthesis, as
/// Rust.
fn condition(
    p: &mut Parser<'_, '_>,
    constants: &Constants,
    sets: &[(String, usize)],
) -> Result<String, Error> {
    let mut out = String::new();
    let mut depth = 0usize;
    loop {
        let line = p.line();
        let Some(tok) = p.next() else {
            return Err(Error::at(line, "a condition never ends"));
        };
        match tok {
            Tok::Punct(")") if depth == 0 => return Ok(out),
            Tok::Punct(")") => {
                depth -= 1;
                out.push(')');
            }
            Tok::Punct("(") => {
                depth += 1;
                out.push('(');
            }
            Tok::Punct(op @ ("==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||")) => {
                let _ = write!(out, " {op} ");
            }
            Tok::Punct("!") => out.push('!'),
            Tok::Int(v) | Tok::Char(v) => {
                let _ = write!(out, "{v}");
            }
            Tok::Ident("lookahead") => out.push_str("lookahead"),
            Tok::Ident("eof") => out.push_str("eof"),
            Tok::Ident("set_contains") => {
                p.expect("(")?;
                let name = p.ident()?.to_owned();
                p.expect(",")?;
                let len_line = p.line();
                let len = match p.next() {
                    Some(Tok::Int(v)) => *v,
                    other => {
                        return Err(Error::at(
                            len_line,
                            format!("a set's length that is not a number: {other:?}"),
                        ));
                    }
                };
                p.expect(",")?;
                p.expect_word("lookahead")?;
                p.expect(")")?;
                let Some((_, size)) = sets.iter().find(|(n, _)| *n == name) else {
                    return Err(Error::at(
                        line,
                        format!("a character set never declared: {name}"),
                    ));
                };
                if usize::try_from(len).ok() != Some(*size) {
                    return Err(Error::at(
                        line,
                        format!("{name} is used as {len} ranges but declared with {size}"),
                    ));
                }
                let _ = write!(out, "set_contains({}, lookahead)", set_name(&name));
            }
            Tok::Ident(name) => {
                // A constant, should the generator ever compare with one.
                let v = constants
                    .get(name)
                    .ok_or_else(|| Error::at(line, format!("a name in a condition: {name}")))?;
                let _ = write!(out, "{v}");
            }
            other => {
                return Err(Error::at(
                    line,
                    format!("a condition this does not know: {other:?}"),
                ));
            }
        }
    }
}

/// The Rust name of a character set.
pub(crate) fn set_name(c_name: &str) -> String {
    c_name.to_ascii_uppercase()
}

impl LexFn {
    /// The lexer as Rust: `fn name(lexer: &mut Lexer<'_>, start: u16) ->
    /// bool`, which runs the states through `crate::ffi::run_lexer`; each
    /// state a function `name_N` in the table `NAME_STATES` by number (a
    /// number no case has is `None`, and runs the default), and the default
    /// `name_default`.
    pub(crate) fn write_rust(&self, name: &str, out: &mut String) {
        let table = format!("{}_STATES", name.to_ascii_uppercase());
        let _ = writeln!(
            out,
            "fn {name}(lexer: &mut Lexer<'_>, start: u16) -> bool {{\n    \
             crate::ffi::run_lexer(lexer, start, &{table}, {name}_default, {})\n}}",
            self.reads_eof
        );
        let numbers: std::collections::BTreeSet<i64> =
            self.states.iter().map(|(value, _)| *value).collect();
        let len = numbers.last().map_or(0, |last| last.saturating_add(1));
        let _ = writeln!(
            out,
            "static {table}: [Option<crate::ffi::LexState>; {len}] = ["
        );
        for n in 0..len {
            if numbers.contains(&n) {
                let _ = writeln!(out, "    Some({name}_{n}),");
            } else {
                out.push_str("    None,\n");
            }
        }
        out.push_str("];\n");
        for (value, stmts) in &self.states {
            // A state that falls off its end: the old loop took the
            // character and ran the same state again. Every generator ends a
            // state with END_STATE(), so this is only ever the shape's.
            write_state(
                &format!("{name}_{value}"),
                stmts,
                &format!("Take({value})"),
                out,
            );
        }
        // A default that falls off the end: what C's function would do is
        // undefined, and every generator writes `return false`, so do that.
        write_state(
            &format!("{name}_default"),
            &self.default,
            "Stop(false)",
            out,
        );
    }
}

/// One state as a function: its statements, then -- if they can end without
/// saying where to go -- `fallthrough`, a `LexStep` variant.
fn write_state(fname: &str, stmts: &[Stmt], fallthrough: &str, out: &mut String) {
    let _ = writeln!(
        out,
        "fn {fname}(lexer: &mut Lexer<'_>, lookahead: i32, eof: bool, result: &mut bool) \
         -> crate::ffi::LexStep {{"
    );
    for stmt in stmts {
        write_stmt(stmt, 1, out);
    }
    if !stmts.last().is_some_and(always_returns) {
        let _ = writeln!(out, "    crate::ffi::LexStep::{fallthrough}");
    }
    out.push_str("}\n");
}

/// Whether `stmt` always returns: the last statement of a state that does
/// needs nothing after it.
fn always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Advance(_) | Stmt::Skip(_) | Stmt::End | Stmt::ReturnFalse => true,
        Stmt::Block(stmts) => stmts.last().is_some_and(always_returns),
        Stmt::AdvanceMap(_) | Stmt::Accept(_) | Stmt::If(..) => false,
    }
}

fn write_stmt(stmt: &Stmt, depth: usize, out: &mut String) {
    let pad = "    ".repeat(depth);
    match stmt {
        Stmt::Advance(n) => {
            let _ = writeln!(out, "{pad}return crate::ffi::LexStep::Take({n});");
        }
        Stmt::Skip(n) => {
            let _ = writeln!(out, "{pad}return crate::ffi::LexStep::Skip({n});");
        }
        Stmt::AdvanceMap(map) => {
            let _ = writeln!(out, "{pad}match lookahead {{");
            for (c, n) in map {
                let _ = writeln!(
                    out,
                    "{pad}    {c} => return crate::ffi::LexStep::Take({n}),"
                );
            }
            let _ = writeln!(out, "{pad}    _ => {{}}\n{pad}}}");
        }
        Stmt::Accept(sym) => {
            let _ = writeln!(out, "{pad}*result = true;\n{pad}lexer.accept({sym});");
        }
        Stmt::End => {
            let _ = writeln!(out, "{pad}return crate::ffi::LexStep::Stop(*result);");
        }
        Stmt::ReturnFalse => {
            let _ = writeln!(out, "{pad}return crate::ffi::LexStep::Stop(false);");
        }
        Stmt::If(cond, then) => {
            let _ = writeln!(out, "{pad}if {cond} {{");
            write_stmt(then, depth + 1, out);
            let _ = writeln!(out, "{pad}}}");
        }
        Stmt::Block(stmts) => {
            let _ = writeln!(out, "{pad}{{");
            for s in stmts {
                write_stmt(s, depth + 1, out);
            }
            let _ = writeln!(out, "{pad}}}");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::ctoken::tokenize;

    fn lexer(source: &str) -> LexFn {
        let toks = tokenize(source).unwrap();
        let mut c = Constants::new();
        c.define("anon_sym_SEMI", 2);
        c.define("sym_identifier", 5);
        read(
            &toks,
            &c,
            &[("sym_identifier_character_set_1".to_owned(), 3)],
        )
        .unwrap()
    }

    /// **A lexer's states, statements and conditions are read** -- and a
    /// map's repeated character keeps its first state, as C's loop would.
    #[test]
    fn a_generated_lexer_is_read() {
        let l = lexer(
            "START_LEXER(); eof = lexer->eof(lexer); switch (state) {
               case 0:
                 if (eof) ADVANCE(4);
                 ADVANCE_MAP('!', 1, ';', 2, '!', 3,);
                 if (('\\t' <= lookahead && lookahead <= '\\r') || lookahead == ' ') SKIP(0);
                 if (set_contains(sym_identifier_character_set_1, 3, lookahead)) ADVANCE(3);
                 END_STATE();
               case 2:
                 ACCEPT_TOKEN(anon_sym_SEMI);
                 END_STATE();
               default:
                 return false;
             }",
        );
        assert!(l.reads_eof);
        assert_eq!(l.states.len(), 2);
        assert_eq!(
            l.states[0].1,
            [
                Stmt::If("eof".to_owned(), Box::new(Stmt::Advance(4))),
                Stmt::AdvanceMap(vec![(33, 1), (59, 2)]),
                Stmt::If(
                    "(9 <= lookahead && lookahead <= 13) || lookahead == 32".to_owned(),
                    Box::new(Stmt::Skip(0))
                ),
                Stmt::If(
                    "set_contains(SYM_IDENTIFIER_CHARACTER_SET_1, lookahead)".to_owned(),
                    Box::new(Stmt::Advance(3))
                ),
                Stmt::End,
            ]
        );
        assert_eq!(l.states[1], (2, vec![Stmt::Accept(2), Stmt::End]));
        assert_eq!(l.default, [Stmt::ReturnFalse]);
    }

    /// **Each state is a function, run by one loop out of a table**: a jump
    /// returns where to go, a state no case has is `None` in the table, and
    /// the lexer is `run_lexer` over the table and the default.
    #[test]
    fn each_state_is_a_function_run_out_of_a_table() {
        let l = lexer(
            "START_LEXER(); eof = lexer->eof(lexer); switch (state) {
               case 0: if (lookahead == 'a') ADVANCE(2); SKIP(0); END_STATE();
               case 2: ACCEPT_TOKEN(sym_identifier); END_STATE();
               default: return false; }",
        );
        let mut rust = String::new();
        l.write_rust("lex_main", &mut rust);
        for want in [
            "fn lex_main(lexer: &mut Lexer<'_>, start: u16) -> bool {\n    \
             crate::ffi::run_lexer(lexer, start, &LEX_MAIN_STATES, lex_main_default, true)\n}",
            "static LEX_MAIN_STATES: [Option<crate::ffi::LexState>; 3] = [\n    \
             Some(lex_main_0),\n    None,\n    Some(lex_main_2),\n];",
            "fn lex_main_0(lexer: &mut Lexer<'_>, lookahead: i32, eof: bool, result: &mut bool) \
             -> crate::ffi::LexStep {",
            "    if lookahead == 97 {\n        return crate::ffi::LexStep::Take(2);\n    }",
            "    return crate::ffi::LexStep::Skip(0);",
            "    *result = true;\n    lexer.accept(5);",
            "    return crate::ffi::LexStep::Stop(*result);\n}",
            "fn lex_main_default(",
            "    return crate::ffi::LexStep::Stop(false);\n}",
        ] {
            assert!(rust.contains(want), "{want}\n--- in ---\n{rust}");
        }
        // Every state ends by returning, so none falls through.
        assert!(
            !rust.contains("    crate::ffi::LexStep::Take(0)\n}"),
            "{rust}"
        );
    }

    /// **A state that can fall off its end goes round again** -- as the old
    /// loop took the character and ran the same state -- and a default that
    /// can stops, answering no token; a lexer that never reads `eof` says so.
    #[test]
    fn a_state_that_falls_off_its_end_goes_round_again() {
        let l = lexer(
            "START_LEXER(); switch (state) {
               case 1: if (lookahead == 'a') ADVANCE(1);
               default: if (eof) ADVANCE(1); }",
        );
        let mut rust = String::new();
        l.write_rust("lex_keywords", &mut rust);
        assert!(rust.contains("lex_keywords_default, false)"), "{rust}");
        assert!(
            rust.contains("[\n    None,\n    Some(lex_keywords_1),\n];"),
            "{rust}"
        );
        assert!(
            rust.contains("    }\n    crate::ffi::LexStep::Take(1)\n}"),
            "{rust}"
        );
        assert!(
            rust.contains("    }\n    crate::ffi::LexStep::Stop(false)\n}"),
            "{rust}"
        );
    }

    /// **What the generator does not write is refused**, not guessed at: a
    /// set used at the wrong length, an unknown macro, an unknown name.
    #[test]
    fn what_the_generator_does_not_write_is_refused() {
        let c = Constants::new();
        let sets = [("s".to_owned(), 3)];
        for bad in [
            "START_LEXER(); switch (state) { case 0: if (set_contains(s, 4, lookahead)) ADVANCE(1); }",
            "START_LEXER(); switch (state) { case 0: GOTO(1); }",
            "START_LEXER(); switch (state) { case 0: if (mystery) ADVANCE(1); }",
            "START_LEXER(); switch (state) { ADVANCE(1); }",
            "switch (state) { }",
        ] {
            let toks = tokenize(bad).unwrap();
            assert!(read(&toks, &c, &sets).is_err(), "{bad}");
        }
    }
}
