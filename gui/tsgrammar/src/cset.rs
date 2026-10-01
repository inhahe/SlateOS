//! An older generator's character sets, as ranges.
//!
//! A newer `parser.c` declares each set a lexer tests against as a table of
//! ranges, `static const TSCharacterRange sym_x_character_set_1[] = {...}`,
//! and tests it with `set_contains`. An older one (tree-sitter-linkerscript's)
//! writes a predicate instead:
//!
//! ```c
//! static inline bool aux_sym_x_character_set_1(int32_t c) {
//!   return (c < 'a'
//!     ? (c < 'L' ? c == 'A' : c <= 'X')
//!     : (c <= 'a' || (c >= 'r' && c <= 'z')));
//! }
//! ```
//!
//! -- a decision tree of comparisons with constants. Such a predicate can
//! change its answer only where one of its comparisons does, at a constant
//! `k` or just past it (`k + 1`), so evaluating it at each of those points
//! and holding the answer until the next gives the exact set, as ranges --
//! which the rest of the converter then treats as a newer file's table.

use crate::Error;
use crate::ctoken::Tok;

/// The largest character: a range ends here at most.
const MAX_CHAR: i64 = 0x10_FFFF;

/// A comparison's operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

/// A predicate over the character `c`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pred {
    /// `cond ? then : otherwise`.
    Choose(Box<Pred>, Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    And(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
    /// `c OP k`.
    Compare(Op, i64),
}

impl Pred {
    fn holds(&self, c: i64) -> bool {
        match self {
            Self::Choose(cond, then, otherwise) => {
                if cond.holds(c) {
                    then.holds(c)
                } else {
                    otherwise.holds(c)
                }
            }
            Self::Or(a, b) => a.holds(c) || b.holds(c),
            Self::And(a, b) => a.holds(c) && b.holds(c),
            Self::Not(a) => !a.holds(c),
            Self::Compare(op, k) => match op {
                Op::Lt => c < *k,
                Op::Le => c <= *k,
                Op::Gt => c > *k,
                Op::Ge => c >= *k,
                Op::Eq => c == *k,
                Op::Ne => c != *k,
            },
        }
    }

    /// Every constant it compares with.
    fn constants(&self, out: &mut Vec<i64>) {
        match self {
            Self::Choose(a, b, c) => {
                a.constants(out);
                b.constants(out);
                c.constants(out);
            }
            Self::Or(a, b) | Self::And(a, b) => {
                a.constants(out);
                b.constants(out);
            }
            Self::Not(a) => a.constants(out),
            Self::Compare(_, k) => out.push(*k),
        }
    }
}

/// Reads a predicate's expression from tokens.
struct Reader<'t, 'a> {
    toks: &'t [(Tok<'a>, usize)],
    at: usize,
}

impl Reader<'_, '_> {
    fn line(&self) -> usize {
        self.toks
            .get(self.at)
            .or_else(|| self.toks.last())
            .map_or(0, |(_, l)| *l)
    }

    fn peek_punct(&self, p: &str) -> bool {
        matches!(self.toks.get(self.at), Some((Tok::Punct(q), _)) if *q == p)
    }

    fn eat(&mut self, p: &str) -> bool {
        let found = self.peek_punct(p);
        if found {
            self.at += 1;
        }
        found
    }

    fn expect(&mut self, p: &str) -> Result<(), Error> {
        if self.eat(p) {
            Ok(())
        } else {
            Err(Error::at(
                self.line(),
                format!("expected `{p}` in a character set"),
            ))
        }
    }

    /// `or (? choose : choose)?`
    fn choose(&mut self) -> Result<Pred, Error> {
        let cond = self.or()?;
        if !self.eat("?") {
            return Ok(cond);
        }
        let then = self.choose()?;
        self.expect(":")?;
        let otherwise = self.choose()?;
        Ok(Pred::Choose(
            Box::new(cond),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    fn or(&mut self) -> Result<Pred, Error> {
        let mut p = self.and()?;
        while self.eat("||") {
            p = Pred::Or(Box::new(p), Box::new(self.and()?));
        }
        Ok(p)
    }

    fn and(&mut self) -> Result<Pred, Error> {
        let mut p = self.unary()?;
        while self.eat("&&") {
            p = Pred::And(Box::new(p), Box::new(self.unary()?));
        }
        Ok(p)
    }

    fn unary(&mut self) -> Result<Pred, Error> {
        if self.eat("!") {
            return Ok(Pred::Not(Box::new(self.unary()?)));
        }
        if self.eat("(") {
            let p = self.choose()?;
            self.expect(")")?;
            return Ok(p);
        }
        self.compare()
    }

    /// `c OP k` or `k OP c`.
    fn compare(&mut self) -> Result<Pred, Error> {
        let line = self.line();
        let left = self.operand()?;
        let op = match self.toks.get(self.at) {
            Some((Tok::Punct(p), _)) => match *p {
                "<" => Op::Lt,
                "<=" => Op::Le,
                ">" => Op::Gt,
                ">=" => Op::Ge,
                "==" => Op::Eq,
                "!=" => Op::Ne,
                other => {
                    return Err(Error::at(
                        line,
                        format!("a character set compares with `{other}`"),
                    ));
                }
            },
            other => {
                return Err(Error::at(
                    line,
                    format!("a character set's comparison is {other:?}"),
                ));
            }
        };
        self.at += 1;
        let right = self.operand()?;
        match (left, right) {
            (None, Some(k)) => Ok(Pred::Compare(op, k)),
            // `k OP c` is `c OP' k`.
            (Some(k), None) => Ok(Pred::Compare(
                match op {
                    Op::Lt => Op::Gt,
                    Op::Le => Op::Ge,
                    Op::Gt => Op::Lt,
                    Op::Ge => Op::Le,
                    same => same,
                },
                k,
            )),
            _ => Err(Error::at(
                line,
                "a character set compares two constants, or `c` with itself",
            )),
        }
    }

    /// `c` (none) or a constant.
    fn operand(&mut self) -> Result<Option<i64>, Error> {
        let line = self.line();
        let tok = self.toks.get(self.at).map(|(t, _)| t);
        self.at += 1;
        match tok {
            Some(Tok::Ident("c")) => Ok(None),
            Some(Tok::Int(v) | Tok::Char(v)) => Ok(Some(*v)),
            other => Err(Error::at(
                line,
                format!("a character set's operand is {other:?}"),
            )),
        }
    }
}

/// The ranges -- first and last character, in order, apart -- a character
/// set's predicate holds for, from its function's body: `return EXPR;`.
/// `None` when the body is not a predicate's (another kind of function).
///
/// # Errors
///
/// A predicate with something this does not read.
pub(crate) fn ranges(body: &[(Tok<'_>, usize)]) -> Result<Option<Vec<(i64, i64)>>, Error> {
    if !matches!(body.first(), Some((Tok::Ident("return"), _))) {
        return Ok(None);
    }
    let mut reader = Reader { toks: body, at: 1 };
    let pred = reader.choose()?;
    reader.expect(";")?;
    if reader.at != body.len() {
        return Err(Error::at(
            reader.line(),
            "a character set's function goes on past its `return`",
        ));
    }
    // Where the answer can change: 0, each constant and the character past
    // it; each answer holds up to the next such point.
    let mut points = vec![0];
    let mut constants = Vec::new();
    pred.constants(&mut constants);
    for k in constants {
        for p in [k, k.saturating_add(1)] {
            if (0..=MAX_CHAR).contains(&p) {
                points.push(p);
            }
        }
    }
    points.sort_unstable();
    points.dedup();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for (i, &start) in points.iter().enumerate() {
        if !pred.holds(start) {
            continue;
        }
        let end = points
            .get(i.saturating_add(1))
            .map_or(MAX_CHAR, |next| next.saturating_sub(1));
        match out.last_mut() {
            Some(last) if last.1.saturating_add(1) == start => last.1 = end,
            _ => out.push((start, end)),
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ctoken::tokenize;

    fn of(body: &str) -> Vec<(i64, i64)> {
        let toks = tokenize(body).unwrap();
        ranges(&toks).unwrap().unwrap()
    }

    /// **A decision tree of comparisons is its set, exactly**: every range
    /// it holds for, the ends included, merged where they meet.
    #[test]
    fn a_decision_tree_is_its_set_exactly() {
        let body = "return (c < 'a'
            ? (c < 'L'
              ? (c < 'I'
                ? c == 'A'
                : c <= 'I')
              : (c <= 'L' || (c < 'W'
                ? c == 'R'
                : c <= 'X')))
            : (c <= 'a' || (c < 'r'
              ? (c < 'l'
                ? c == 'i'
                : c <= 'l')
              : (c <= 'r' || (c >= 'w' && c <= 'x')))));";
        let a = |c: char| i64::from(u32::from(c));
        assert_eq!(
            of(body),
            [
                (a('A'), a('A')),
                (a('I'), a('I')),
                (a('L'), a('L')),
                (a('R'), a('R')),
                (a('W'), a('X')),
                (a('a'), a('a')),
                (a('i'), a('i')),
                (a('l'), a('l')),
                (a('r'), a('r')),
                (a('w'), a('x')),
            ]
        );
    }

    /// **Every operator, either way round, and to the last character.**
    #[test]
    fn every_operator_either_way_round() {
        assert_eq!(of("return c > 100;"), [(101, MAX_CHAR)]);
        assert_eq!(of("return 100 > c;"), [(0, 99)]);
        assert_eq!(of("return c != 5;"), [(0, 4), (6, MAX_CHAR)]);
        assert_eq!(of("return !(c >= 10) && c >= 3;"), [(3, 9)]);
        assert_eq!(of("return 0x30 <= c && c <= 0x39;"), [(0x30, 0x39)]);
    }

    /// **A body that is not a predicate's is none; one this cannot read is
    /// an error.**
    #[test]
    fn other_bodies_are_not_sets() {
        let toks = tokenize("START_LEXER(); END_STATE();").unwrap();
        assert_eq!(ranges(&toks).unwrap(), None);
        for bad in [
            "return c + 1;",
            "return c < d;",
            "return c < 1; x;",
            "return 1 < 2;",
        ] {
            let toks = tokenize(bad).unwrap();
            assert!(ranges(&toks).is_err(), "{bad}");
        }
    }
}
