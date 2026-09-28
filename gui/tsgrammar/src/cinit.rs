//! C initializers -- `{ [sym_x] = {.visible = true}, ACTIONS(3), ... }` --
//! parsed into a tree and evaluated against the grammar's constants.
//!
//! What the generator writes in an initializer is a small corner of C:
//! braces, designators (`[index] =`, `.field =`), integers, characters,
//! strings, the enum constants it declared, the table macros (`ACTIONS`,
//! `STATE`, `SMALL_STATE`, `SHIFT`, `REDUCE` and the rest), casts, a unary
//! minus, and in the language struct an address or two. That is what is
//! parsed here, and anything else is an error rather than a guess.

use std::collections::HashMap;

use crate::Error;
use crate::ctoken::Tok;

/// An initializer: a braced list, or one expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Init {
    /// `{ ... }`.
    List(Vec<Item>),
    /// Anything else.
    Expr(Expr),
}

/// One element of a braced list, with its designator if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) designator: Option<Designator>,
    pub(crate) value: Init,
}

/// `[index] =` or `.field =`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Designator {
    Index(Expr),
    Field(String),
}

/// An expression, as much of one as an initializer holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Expr {
    Int(i64),
    Ident(String),
    Str(Vec<u8>),
    /// `NAME(args)`: one of the table macros.
    Call(String, Vec<Expr>),
    Neg(Box<Expr>),
    /// `(type)expr`: the type, as its words joined by blanks.
    Cast(String, Box<Expr>),
    /// `&expr`.
    Addr(Box<Expr>),
    /// `expr[index]`.
    Index(Box<Expr>, Box<Expr>),
}

/// The words a cast's type is made of: what tells `(TSStateId)(-1)` from a
/// parenthesised expression.
const TYPE_WORDS: &[&str] = &[
    "const",
    "void",
    "char",
    "int",
    "unsigned",
    "signed",
    "short",
    "long",
    "bool",
    "uint8_t",
    "uint16_t",
    "uint32_t",
    "int16_t",
    "int32_t",
    "TSStateId",
    "TSSymbol",
    "TSFieldId",
];

/// A cursor over tokens.
pub(crate) struct Parser<'t, 'a> {
    toks: &'t [(Tok<'a>, usize)],
    at: usize,
}

impl<'t, 'a> Parser<'t, 'a> {
    pub(crate) fn new(toks: &'t [(Tok<'a>, usize)]) -> Self {
        Self { toks, at: 0 }
    }

    /// The line of the current token, for errors.
    pub(crate) fn line(&self) -> usize {
        self.toks
            .get(self.at)
            .or_else(|| self.toks.last())
            .map_or(0, |(_, line)| *line)
    }

    pub(crate) fn peek(&self) -> Option<&Tok<'a>> {
        self.toks.get(self.at).map(|(t, _)| t)
    }

    pub(crate) fn peek_at(&self, ahead: usize) -> Option<&Tok<'a>> {
        self.toks.get(self.at + ahead).map(|(t, _)| t)
    }

    pub(crate) fn next(&mut self) -> Option<&Tok<'a>> {
        let tok = self.toks.get(self.at).map(|(t, _)| t);
        if tok.is_some() {
            self.at += 1;
        }
        tok
    }

    /// How many tokens have been used.
    pub(crate) fn position(&self) -> usize {
        self.at
    }

    pub(crate) fn at_end(&self) -> bool {
        self.at >= self.toks.len()
    }

    /// Whether the current token is the punctuation `p`; consumes it if so.
    pub(crate) fn eat(&mut self, p: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Punct(q)) if *q == p) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    /// Whether the current token is the word `w`; consumes it if so.
    pub(crate) fn eat_word(&mut self, w: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Ident(q)) if *q == w) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, p: &str) -> Result<(), Error> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(Error::at(
                self.line(),
                format!("expected `{p}`, found {:?}", self.peek()),
            ))
        }
    }

    pub(crate) fn expect_word(&mut self, w: &str) -> Result<(), Error> {
        if self.eat_word(w) {
            Ok(())
        } else {
            Err(Error::at(
                self.line(),
                format!("expected `{w}`, found {:?}", self.peek()),
            ))
        }
    }

    pub(crate) fn ident(&mut self) -> Result<&'a str, Error> {
        match self.peek() {
            Some(Tok::Ident(w)) => {
                let w = *w;
                self.at += 1;
                Ok(w)
            }
            other => Err(Error::at(
                self.line(),
                format!("expected a name, found {other:?}"),
            )),
        }
    }

    /// An initializer: a braced list or an expression.
    pub(crate) fn init(&mut self) -> Result<Init, Error> {
        if !self.eat("{") {
            return self.expr().map(Init::Expr);
        }
        let mut items = Vec::new();
        loop {
            if self.eat("}") {
                return Ok(Init::List(items));
            }
            let designator = if self.eat("[") {
                let index = self.expr()?;
                self.expect("]")?;
                self.expect("=")?;
                Some(Designator::Index(index))
            } else if matches!(self.peek(), Some(Tok::Punct(".")))
                && matches!(self.peek_at(1), Some(Tok::Ident(_)))
                && matches!(self.peek_at(2), Some(Tok::Punct("=")))
            {
                self.at += 1;
                let field = self.ident()?.to_owned();
                self.expect("=")?;
                Some(Designator::Field(field))
            } else {
                None
            };
            let value = self.init()?;
            items.push(Item { designator, value });
            if !self.eat(",") {
                self.expect("}")?;
                return Ok(Init::List(items));
            }
        }
    }

    /// An expression: unary operators and casts over a postfix expression.
    pub(crate) fn expr(&mut self) -> Result<Expr, Error> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.expr()?)));
        }
        if self.eat("&") {
            return Ok(Expr::Addr(Box::new(self.expr()?)));
        }
        if let Some(ty) = self.cast_type() {
            return Ok(Expr::Cast(ty, Box::new(self.expr()?)));
        }
        let mut e = self.primary()?;
        loop {
            if self.eat("[") {
                let index = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(index));
            } else {
                return Ok(e);
            }
        }
    }

    /// If a cast starts here, its type -- consumed.
    fn cast_type(&mut self) -> Option<String> {
        if !matches!(self.peek(), Some(Tok::Punct("("))) {
            return None;
        }
        let mut words = Vec::new();
        let mut ahead = 1;
        loop {
            match self.peek_at(ahead)? {
                Tok::Ident(w) if TYPE_WORDS.contains(w) => words.push(*w),
                Tok::Punct("*") => words.push("*"),
                Tok::Punct(")") if !words.is_empty() => break,
                _ => return None,
            }
            ahead += 1;
        }
        self.at += ahead + 1;
        Some(words.join(" "))
    }

    fn primary(&mut self) -> Result<Expr, Error> {
        let line = self.line();
        match self.next() {
            Some(Tok::Int(v) | Tok::Char(v)) => Ok(Expr::Int(*v)),
            Some(Tok::Str(s)) => {
                let mut bytes = s.clone();
                // Adjacent literals are one string.
                while let Some(Tok::Str(more)) = self.peek() {
                    bytes.extend_from_slice(more);
                    self.at += 1;
                }
                Ok(Expr::Str(bytes))
            }
            Some(Tok::Ident(w)) => {
                let w = (*w).to_owned();
                if self.eat("(") {
                    let mut args = Vec::new();
                    if !self.eat(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.eat(")") {
                                break;
                            }
                            self.expect(",")?;
                        }
                    }
                    Ok(Expr::Call(w, args))
                } else {
                    Ok(Expr::Ident(w))
                }
            }
            Some(Tok::Punct("(")) => {
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            other => Err(Error::at(
                line,
                format!("expected a value, found {other:?}"),
            )),
        }
    }
}

/// The names an initializer may use, with their values: the enums' constants,
/// the `#define`d counts, and C's own.
#[derive(Debug, Default)]
pub(crate) struct Constants {
    values: HashMap<String, i64>,
}

impl Constants {
    pub(crate) fn new() -> Self {
        let mut values = HashMap::new();
        for (name, value) in [
            ("true", 1),
            ("false", 0),
            ("NULL", 0),
            ("ts_builtin_sym_end", 0),
            ("ts_builtin_sym_error", 0xFFFF),
            ("ts_builtin_sym_error_repeat", 0xFFFE),
        ] {
            values.insert(name.to_owned(), value);
        }
        Self { values }
    }

    pub(crate) fn define(&mut self, name: &str, value: i64) {
        self.values.insert(name.to_owned(), value);
    }

    pub(crate) fn get(&self, name: &str) -> Option<i64> {
        self.values.get(name).copied()
    }

    /// The value of `e`, which must be an integer: a table macro is its
    /// argument (`SMALL_STATE` less the large states), a cast truncates.
    pub(crate) fn eval(&self, e: &Expr, line: usize) -> Result<i64, Error> {
        match e {
            Expr::Int(v) => Ok(*v),
            Expr::Ident(name) => self
                .get(name)
                .ok_or_else(|| Error::at(line, format!("a name with no value: {name}"))),
            Expr::Neg(inner) => Ok(-self.eval(inner, line)?),
            Expr::Cast(ty, inner) => {
                let v = self.eval(inner, line)?;
                Ok(match ty.as_str() {
                    "TSStateId" | "TSSymbol" | "TSFieldId" | "uint16_t" => v & 0xFFFF,
                    "uint8_t" => v & 0xFF,
                    "uint32_t" | "unsigned" => v & 0xFFFF_FFFF,
                    _ => v,
                })
            }
            Expr::Call(name, args) => match (name.as_str(), args.as_slice()) {
                ("ACTIONS" | "STATE", [x]) => self.eval(x, line),
                ("SMALL_STATE", [x]) => {
                    let large = self.get("LARGE_STATE_COUNT").ok_or_else(|| {
                        Error::at(line, "SMALL_STATE before LARGE_STATE_COUNT is defined")
                    })?;
                    Ok(self.eval(x, line)? - large)
                }
                _ => Err(Error::at(line, format!("not a number: {name}(...)"))),
            },
            Expr::Str(_) | Expr::Addr(_) | Expr::Index(..) => {
                Err(Error::at(line, format!("not a number: {e:?}")))
            }
        }
    }
}

/// A one-dimensional array initializer as `(index, value)` pairs, the
/// positional ones numbered as C numbers them: from the last designator on.
pub(crate) fn elements<'i>(
    init: &'i Init,
    constants: &Constants,
    line: usize,
) -> Result<Vec<(usize, &'i Init)>, Error> {
    let Init::List(items) = init else {
        return Err(Error::at(line, "an array's initializer is not a list"));
    };
    let mut out = Vec::with_capacity(items.len());
    let mut next = 0usize;
    for item in items {
        let index = match &item.designator {
            Some(Designator::Index(e)) => usize::try_from(constants.eval(e, line)?)
                .map_err(|_| Error::at(line, "a negative index"))?,
            Some(Designator::Field(f)) => {
                return Err(Error::at(line, format!("a field `.{f}` in an array")));
            }
            None => next,
        };
        out.push((index, &item.value));
        next = index + 1;
    }
    Ok(out)
}

/// A struct initializer as its fields' values, in `fields`' order, the ones
/// it leaves out 0 -- positional values filling fields from the last
/// designator on, as in C. A field's value must be an integer.
pub(crate) fn fields(
    init: &Init,
    names: &[&str],
    constants: &Constants,
    line: usize,
) -> Result<Vec<i64>, Error> {
    let mut out = vec![0; names.len()];
    let Init::List(items) = init else {
        // A scalar for a struct: C takes it as the first field.
        if let (Init::Expr(e), Some(first)) = (init, out.first_mut()) {
            *first = constants.eval(e, line)?;
        }
        return Ok(out);
    };
    let mut next = 0usize;
    for item in items {
        let at = match &item.designator {
            Some(Designator::Field(f)) => names
                .iter()
                .position(|n| n == f)
                .ok_or_else(|| Error::at(line, format!("no field `.{f}` in {names:?}")))?,
            Some(Designator::Index(_)) => {
                return Err(Error::at(line, "an index in a struct"));
            }
            None => next,
        };
        let value = match &item.value {
            Init::Expr(e) => constants.eval(e, line)?,
            // `{0}` for a scalar, which the generator writes for an empty
            // alias sequence.
            Init::List(inner) => match inner.as_slice() {
                [] => 0,
                [
                    Item {
                        designator: None,
                        value: Init::Expr(e),
                    },
                ] => constants.eval(e, line)?,
                _ => return Err(Error::at(line, "a list where a number belongs")),
            },
        };
        let slot = out
            .get_mut(at)
            .ok_or_else(|| Error::at(line, format!("more values than {names:?} has fields")))?;
        *slot = value;
        next = at + 1;
    }
    Ok(out)
}

/// The value of the item designated `.name` in a struct initializer.
pub(crate) fn field<'i>(init: &'i Init, name: &str) -> Option<&'i Init> {
    let Init::List(items) = init else {
        return None;
    };
    items.iter().find_map(|item| match &item.designator {
        Some(Designator::Field(f)) if f == name => Some(&item.value),
        _ => None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;
    use crate::ctoken::tokenize;

    fn init(source: &str) -> Init {
        let toks = tokenize(source).unwrap();
        let mut p = Parser::new(&toks);
        let init = p.init().unwrap();
        assert!(p.at_end(), "left over: {:?}", p.peek());
        init
    }

    fn constants() -> Constants {
        let mut c = Constants::new();
        c.define("sym_a", 3);
        c.define("field_x", 1);
        c.define("field_y", 2);
        c.define("LARGE_STATE_COUNT", 10);
        c
    }

    /// **Designated and positional elements are numbered as C numbers
    /// them**, macros and constants evaluated.
    #[test]
    fn array_elements_are_numbered_as_c_numbers_them() {
        let c = constants();
        let i = init("{ [0] = 5, ACTIONS(3), [sym_a] = STATE(7), 1, [SMALL_STATE(12)] = sym_a, }");
        let got: Vec<(usize, i64)> = elements(&i, &c, 1)
            .unwrap()
            .into_iter()
            .map(|(at, v)| {
                let Init::Expr(e) = v else { panic!() };
                (at, c.eval(e, 1).unwrap())
            })
            .collect();
        assert_eq!(got, [(0, 5), (1, 3), (3, 7), (4, 1), (2, 3)]);
    }

    /// **A struct's fields, by name or by position from the last name**,
    /// the rest 0 -- and a cast truncates as C's does.
    #[test]
    fn struct_fields_are_filled_as_c_fills_them() {
        let c = constants();
        let names = ["field_id", "child_index", "inherited"];
        assert_eq!(
            fields(&init("{field_y, 1, .inherited = true}"), &names, &c, 1).unwrap(),
            [2, 1, 1]
        );
        assert_eq!(
            fields(&init("{.child_index = 4}"), &names, &c, 1).unwrap(),
            [0, 4, 0]
        );
        let modes = ["lex_state", "external_lex_state", "reserved_word_set_id"];
        assert_eq!(
            fields(&init("{(TSStateId)(-1),}"), &modes, &c, 1).unwrap(),
            [0xFFFF, 0, 0]
        );
        assert!(fields(&init("{.nope = 1}"), &names, &c, 1).is_err());
        assert!(fields(&init("{1, 2, 3, 4}"), &names, &c, 1).is_err());
    }

    /// **The language struct's own shapes parse**: addresses, casts to
    /// pointers, a string, a nested struct -- read by field name.
    #[test]
    fn the_language_structs_values_parse() {
        let i = init(
            "{ .parse_table = &ts_parse_table[0][0], .lex_modes = (const void*)ts_lex_modes, \
             .name = \"rust\", .metadata = { .major_version = 0, .minor_version = 24, }, \
             .external_scanner = { &s[0][0], m, create, }, }",
        );
        assert_eq!(
            field(&i, "name"),
            Some(&Init::Expr(Expr::Str(b"rust".to_vec())))
        );
        let meta = field(&i, "metadata").unwrap();
        assert_eq!(
            fields(
                meta,
                &["major_version", "minor_version", "patch_version"],
                &constants(),
                1
            )
            .unwrap(),
            [0, 24, 0]
        );
        assert!(matches!(
            field(&i, "lex_modes"),
            Some(Init::Expr(Expr::Cast(..)))
        ));
        assert_eq!(field(&i, "keyword_lex_fn"), None);
    }

    /// **What is not a number is said to be one nowhere.**
    #[test]
    fn a_value_that_is_no_number_is_refused() {
        let c = constants();
        for bad in ["sym_unknown", "\"s\"", "&x", "SHIFT(3)"] {
            let Init::Expr(e) = init(bad) else { panic!() };
            assert!(c.eval(&e, 1).is_err(), "{bad}");
        }
    }
}
