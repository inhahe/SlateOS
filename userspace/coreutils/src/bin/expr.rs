//! expr — evaluate an expression given as command-line arguments.
//!
//! ```text
//! expr EXPRESSION
//! expr --help | --version
//! ```
//!
//! Precedence, loosest first; every level is left-associative:
//!
//! | level | operators |
//! |---|---|
//! | 1 | <code>&#124;</code> — the left operand if it is neither null nor zero, else the right, else `0` |
//! | 2 | `&` — the left operand if neither side is null or zero, else `0` |
//! | 3 | `<` `<=` `=` `==` `!=` `>=` `>` |
//! | 4 | `+` `-` |
//! | 5 | `*` `/` `%` |
//! | 6 | `:` — anchored match of a BRE against a string |
//! | 7 | `match` `substr` `index` `length`, `+ TOKEN`, `( … )`, a literal |
//!
//! Exit status: 0 if the value is neither null nor zero, 1 if it is, 2 if the
//! expression will not parse or an operand is wrong for its operator.
//!
//! ## What this used to be
//!
//! Until this rewrite `expr` had no `:` operator and no `match` — not a
//! substring-matching approximation of them, as `known-issues.md` recorded, but
//! nothing at all: `expr abc : 'a.c'` was three literals in a row and died with
//! "syntax error". `substr` and `index` were absent too. Arithmetic was `i64`
//! with unchecked `+` and `*`, so `expr 9223372036854775807 + 1` panicked in a
//! debug build and silently wrapped in a release one, and a non-numeric operand
//! became `0` without a word — `expr $x + 1` on an empty `$x` printed `1`,
//! which is a plausible-looking wrong answer rather than the error GNU gives.
//! Comparison did not loop, so `expr 1 = 1 = 1` was a syntax error.
//!
//! ## Where the pieces come from
//!
//! `:` is a POSIX **Basic** regular expression, so it goes through
//! [`ere::bre`] — the same translator `grep` and `sed` use, feeding the same
//! matcher the shell's `[[ =~ ]]` uses. That is the entire point of the `ere`
//! crate: `expr "$f" : '^[a-z]*$'` and `grep '^[a-z]*$'` now agree about a
//! file, which they could not when one of them was `str::contains`. See
//! `design-decisions.md` §322.
//!
//! Arithmetic goes through [`bignum`], because GNU's does:
//! `expr 99999999999999999999 '*' 99999999999999999999` prints all forty
//! digits. A shell script reaches sizes past `i64` by accident — a byte count
//! multiplied by a byte count — and the answer it gets should not depend on
//! which of this tree's four bignum implementations it happened to reach.
//!
//! ## Text is bytes
//!
//! Operands are `Vec<u8>`: a path on this system may hold any byte but `/` and
//! NUL, and `expr "$path" : '.*/\(.*\)'` is one of the oldest ways to spell
//! `basename`. Where a count is of *characters* rather than bytes — `length`,
//! `substr`, `index`, and the number `:` returns — it counts characters, using
//! `ere`'s character model, which matches GNU expr in a UTF-8 locale.
//!
//! ## Where this deliberately differs from GNU expr
//!
//! `scripts/expr-diff.sh` runs both against the same expressions and requires
//! them to agree; these are recorded there as expected disagreements.
//!
//! | Case | Ours | GNU |
//! |---|---|---|
//! | a backreference in the pattern (`\(a\)\1`) | refused, exit 2 | matched |
//! | a stacked quantifier (`a**`) | refused, exit 2 | matched, `*` folded |
//! | the text of a bad-pattern diagnostic | `ere`'s wording | `regcomp`'s wording |
//!
//! The first is the Pike VM's one real limitation — it is the same property
//! that makes the engine immune to catastrophic backtracking — and refusing is
//! better than the alternative, which is treating `\1` as a literal `1` and
//! quietly answering the wrong question. See `known-issues.md`.

use std::io::Write as _;
use std::process;

use bignum::BigInt;
use ere::ch::{Ch, chars};

/// An operand, an operator, and a result: all bytes.
type Str = Vec<u8>;

const USAGE: &str = "usage: expr EXPRESSION";

/// Why evaluation stopped. Every case exits 2 — GNU reserves 3 for an I/O
/// failure, which is the only thing this program can fail at afterwards.
struct Fail(String);

fn main() {
    let raw: Vec<Str> = std::env::args_os().skip(1).map(|a| arg_bytes(&a)).collect();

    // `--help` and `--version` are recognised only as the whole command line,
    // as GNU's option parser does: `expr --help = --help` is a comparison of
    // two strings that happen to look like options, and answers 1.
    if let [only] = raw.as_slice() {
        if only.as_slice() == b"--help" {
            println!("{USAGE}");
            return;
        }
        if only.as_slice() == b"--version" {
            println!("expr (SlateOS coreutils)");
            return;
        }
    }

    // A leading `--` ends options, so `expr -- -1 + 1` can start with what
    // would otherwise look like one.
    let args: &[Str] = match raw.split_first() {
        Some((first, rest)) if first.as_slice() == b"--" => rest,
        _ => &raw,
    };

    if args.is_empty() {
        eprintln!("expr: missing operand");
        eprintln!("{USAGE}");
        process::exit(2);
    }

    let mut p = Parser { args, pos: 0 };
    let value = match p.or() {
        Ok(v) => v,
        Err(Fail(msg)) => die(&msg),
    };
    if let Some(extra) = p.peek() {
        die(&format!(
            "syntax error: unexpected argument '{}'",
            String::from_utf8_lossy(extra)
        ));
    }

    let mut out = std::io::stdout().lock();
    if out.write_all(&value).and_then(|()| out.write_all(b"\n")).is_err() {
        // Nothing left to say it on; GNU's exit 3 is "an error occurred".
        process::exit(3);
    }
    // The value *is* the status: a shell writes `if expr "$a" '<' "$b"`.
    process::exit(i32::from(is_null(&value)));
}

fn die(msg: &str) -> ! {
    eprintln!("expr: {msg}");
    process::exit(2)
}

/// An argument as bytes.
///
/// On a platform whose arguments are already bytes — SlateOS, and Unix — this
/// is exact. On the development host, where they are UTF-16, an argument that
/// is not valid Unicode cannot be expressed and the lossy conversion is all
/// that is left; it affects only the host.
#[cfg(unix)]
fn arg_bytes(a: &std::ffi::OsString) -> Str {
    use std::os::unix::ffi::OsStrExt as _;
    a.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(a: &std::ffi::OsString) -> Str {
    a.to_string_lossy().into_owned().into_bytes()
}

// ---------------------------------------------------------------- truth

/// Whether a value counts as false: empty, or a `-`-signed run of zeros.
///
/// The exact shape matters and is not "parses as zero": `expr +0 '|' x` prints
/// `+0` because the leading `+` is not part of the pattern, while `expr -0 '|'
/// x` prints `x`. Scripts do not depend on that, but a differential test does,
/// and matching it costs one line.
fn is_null(v: &[u8]) -> bool {
    if v.is_empty() {
        return true;
    }
    let digits = v.strip_prefix(b"-").unwrap_or(v);
    !digits.is_empty() && digits.iter().all(|&b| b == b'0')
}

/// Whether a value may be used as a number: optional `-`, then digits.
///
/// A leading `+` is *not* accepted, which is why `expr +1 + 1` is an error and
/// `expr +1 '<' 2` is a string comparison.
fn looks_like_integer(v: &[u8]) -> bool {
    let digits = v.strip_prefix(b"-").unwrap_or(v);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// A value as an exact integer, or the error the arithmetic operators report.
fn to_int(v: &[u8]) -> Result<BigInt, Fail> {
    if !looks_like_integer(v) {
        return Err(Fail("non-integer argument".to_string()));
    }
    Ok(BigInt::from_str(&String::from_utf8_lossy(v)))
}

/// A value as a machine integer for `substr`'s position and length, saturating
/// rather than wrapping: a position past `i64::MAX` is past the end of every
/// string, and a length past it covers the whole rest, so clamping gives the
/// right answer without a bignum in the middle of a slice index.
fn to_small_int(v: &[u8]) -> Option<i64> {
    if !looks_like_integer(v) {
        return None;
    }
    let negative = v.starts_with(b"-");
    let digits = v.strip_prefix(b"-").unwrap_or(v);
    let mut n: i64 = 0;
    for &b in digits {
        n = n
            .saturating_mul(10)
            .saturating_add(i64::from(b.wrapping_sub(b'0')));
    }
    Some(if negative { n.saturating_neg() } else { n })
}

/// `1` or `0` as a value — what every comparison yields.
fn boolean(b: bool) -> Str {
    if b { b"1".to_vec() } else { b"0".to_vec() }
}

// ---------------------------------------------------------------- parser

/// The command line, and how far through it evaluation has got.
///
/// There is no separate lexer: `expr`'s tokens are its arguments, already split
/// by the shell, which is the whole reason its operators have to be quoted.
struct Parser<'a> {
    args: &'a [Str],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a Str> {
        self.args.get(self.pos)
    }

    /// Consume the next argument if it is exactly `tok`.
    fn eat(&mut self, tok: &[u8]) -> bool {
        if self.peek().is_some_and(|a| a.as_slice() == tok) {
            self.pos = self.pos.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Consume the next argument if it is any of `toks`, and say which.
    fn eat_any(&mut self, toks: &[&'static [u8]]) -> Option<&'static [u8]> {
        let next = self.peek()?;
        let hit = toks.iter().copied().find(|t| *t == next.as_slice())?;
        self.pos = self.pos.saturating_add(1);
        Some(hit)
    }

    /// Consume the next argument, whatever it is.
    fn take(&mut self) -> Option<&'a Str> {
        let a = self.args.get(self.pos)?;
        self.pos = self.pos.saturating_add(1);
        Some(a)
    }

    /// The argument just consumed, for "missing argument after 'X'".
    fn previous(&self) -> String {
        let prev = self
            .pos
            .checked_sub(1)
            .and_then(|i| self.args.get(i))
            .map_or(&b""[..], Vec::as_slice);
        String::from_utf8_lossy(prev).into_owned()
    }

    fn missing(&self) -> Fail {
        Fail(format!(
            "syntax error: missing argument after '{}'",
            self.previous()
        ))
    }

    /// `|` — the first operand that is neither null nor zero, else `0`.
    ///
    /// The trailing normalisation to `0` is GNU's and is load-bearing:
    /// `expr '' '|' ''` prints `0`, not an empty line, so a script that writes
    /// `n=$(expr "$a" '|' "$b")` always gets something it can compare.
    fn or(&mut self) -> Result<Str, Fail> {
        let mut left = self.and()?;
        while self.eat(b"|") {
            let right = self.and()?;
            if is_null(&left) {
                left = right;
            }
            if is_null(&left) {
                left = b"0".to_vec();
            }
        }
        Ok(left)
    }

    /// `&` — the left operand when neither side is null or zero, else `0`.
    fn and(&mut self) -> Result<Str, Fail> {
        let mut left = self.comparison()?;
        while self.eat(b"&") {
            let right = self.comparison()?;
            if is_null(&left) || is_null(&right) {
                left = b"0".to_vec();
            }
        }
        Ok(left)
    }

    /// The six comparisons, plus `==` as a spelling of `=`.
    ///
    /// Numeric when *both* sides look like integers, byte-lexicographic
    /// otherwise. That is why `expr 3 '<' 20` is 1 while `expr +3 '<' 20` is 0:
    /// the second is a string comparison, because `+3` is not a number here.
    fn comparison(&mut self) -> Result<Str, Fail> {
        const OPS: &[&[u8]] = &[b"<=", b">=", b"!=", b"==", b"<", b">", b"="];
        let mut left = self.additive()?;
        while let Some(op) = self.eat_any(OPS) {
            let right = self.additive()?;
            let ordering = if looks_like_integer(&left) && looks_like_integer(&right) {
                let difference = to_int(&left)?.sub(&to_int(&right)?);
                if difference.is_zero() {
                    std::cmp::Ordering::Equal
                } else if difference.negative {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Greater
                }
            } else {
                left.cmp(&right)
            };
            left = boolean(match op {
                b"=" | b"==" => ordering.is_eq(),
                b"!=" => ordering.is_ne(),
                b"<" => ordering.is_lt(),
                b"<=" => ordering.is_le(),
                b">" => ordering.is_gt(),
                _ => ordering.is_ge(),
            });
        }
        Ok(left)
    }

    /// `+` and `-`.
    fn additive(&mut self) -> Result<Str, Fail> {
        const OPS: &[&[u8]] = &[b"+", b"-"];
        let mut left = self.multiplicative()?;
        while let Some(op) = self.eat_any(OPS) {
            let right = self.multiplicative()?;
            let (l, r) = (to_int(&left)?, to_int(&right)?);
            let sum = if op == b"+" { l.add(&r) } else { l.sub(&r) };
            left = sum.to_string_base10().into_bytes();
        }
        Ok(left)
    }

    /// `*`, `/` and `%`, the last two truncating toward zero as C does.
    fn multiplicative(&mut self) -> Result<Str, Fail> {
        const OPS: &[&[u8]] = &[b"*", b"/", b"%"];
        let mut left = self.anchored_match()?;
        while let Some(op) = self.eat_any(OPS) {
            let right = self.anchored_match()?;
            let (l, r) = (to_int(&left)?, to_int(&right)?);
            let value = if op == b"*" {
                l.mul(&r)
            } else {
                if r.is_zero() {
                    return Err(Fail("division by zero".to_string()));
                }
                let (quotient, remainder) = l.divmod(&r);
                if op == b"/" { quotient } else { remainder }
            };
            left = value.to_string_base10().into_bytes();
        }
        Ok(left)
    }

    /// `:` — match a BRE against a string, anchored at its start.
    fn anchored_match(&mut self) -> Result<Str, Fail> {
        let mut left = self.primary()?;
        while self.eat(b":") {
            let right = self.primary()?;
            left = colon(&left, &right)?;
        }
        Ok(left)
    }

    /// The keywords, the quote operator, a parenthesised expression, or a
    /// literal.
    ///
    /// Every keyword takes its operands at *this* level rather than as raw
    /// arguments, which is why `expr length '(' abc ')'` is 3 and
    /// `expr match abc a : x` is `(match abc a) : x` — the keywords bind
    /// tighter than every operator, including `:`.
    fn primary(&mut self) -> Result<Str, Fail> {
        if self.eat(b"(") {
            let inner = self.or()?;
            if !self.eat(b")") {
                return Err(match self.peek() {
                    Some(a) => Fail(format!(
                        "syntax error: expecting ')' instead of '{}'",
                        String::from_utf8_lossy(a)
                    )),
                    None => Fail(format!(
                        "syntax error: expecting ')' after '{}'",
                        self.previous()
                    )),
                });
            }
            return Ok(inner);
        }

        // `+ TOKEN` is the escape hatch for an operand that would otherwise be
        // read as an operator or a keyword: `expr + match` is the word "match".
        if self.eat(b"+") {
            return match self.take() {
                Some(t) => Ok(t.clone()),
                None => Err(self.missing()),
            };
        }

        if self.eat(b"match") {
            let subject = self.primary()?;
            let pattern = self.primary()?;
            return colon(&subject, &pattern);
        }
        if self.eat(b"substr") {
            let subject = self.primary()?;
            let start = self.primary()?;
            let count = self.primary()?;
            return Ok(substr(&subject, &start, &count));
        }
        if self.eat(b"index") {
            let subject = self.primary()?;
            let set = self.primary()?;
            return Ok(index_of(&subject, &set));
        }
        if self.eat(b"length") {
            let subject = self.primary()?;
            return Ok(chars(&subject).count().to_string().into_bytes());
        }

        // A `)` here closes a group that was never opened. Everything else is
        // an operand, including a bare `:` or `*` — `expr : abc` is the string
        // ":" followed by a stray argument, not an operator missing a left
        // side.
        if self.peek().is_some_and(|a| a.as_slice() == b")") {
            return Err(Fail("syntax error: unexpected ')'".to_string()));
        }
        match self.take() {
            Some(t) => Ok(t.clone()),
            None => Err(self.missing()),
        }
    }
}

// ---------------------------------------------------------------- operators

/// `STRING : REGEX`, and `match STRING REGEX`, which is the same thing.
///
/// The match is anchored at the start of the subject — POSIX says so, and it is
/// the difference between `expr "$f" : 'lib'` testing a prefix and testing for
/// a substring. Anchoring falls out of the engine's leftmost-longest rule
/// rather than needing a `^` spliced into the pattern: if the leftmost match
/// does not begin at offset 0 then no match begins at 0, and splicing would
/// have been wrong anyway (`^` in front of `a\|b` anchors only the first
/// branch).
///
/// What comes back depends on the pattern, not on the subject: a pattern
/// containing `\(…\)` yields group 1, and one without yields the number of
/// characters consumed. Both yield their own kind of falsehood on no match —
/// the empty string and `0` — which is why the caller can test either with the
/// same [`is_null`].
fn colon(subject: &[u8], pattern: &[u8]) -> Result<Str, Fail> {
    // The engine refuses an empty pattern, and it is right to: an empty ERE is
    // a syntax error everywhere it can be written. Here the pattern is a whole
    // argument, so `expr abc : ''` is reachable and GNU answers 0 — the empty
    // match at offset 0, zero characters long, with no groups to report.
    if pattern.is_empty() {
        return Ok(b"0".to_vec());
    }
    let re = ere::bre::compile(pattern, false)
        .map_err(|e| Fail(String::from_utf8_lossy(&e.0).into_owned()))?;
    let matched = re
        .capture_spans(subject)
        .filter(|spans| matches!(spans.first(), Some(&Some((0, _)))));

    if re.group_count() > 0 {
        let group = matched
            .as_ref()
            .and_then(|spans| spans.get(1).copied().flatten())
            .and_then(|(s, e)| subject.get(s..e));
        return Ok(group.unwrap_or_default().to_vec());
    }
    let length = matched
        .and_then(|spans| spans.first().copied().flatten())
        .and_then(|(_, e)| subject.get(..e))
        .map_or(0, |m| chars(m).count());
    Ok(length.to_string().into_bytes())
}

/// `substr STRING POS LENGTH`, counting characters, `POS` from 1.
///
/// Every way of asking for nothing — a non-numeric bound, a position before the
/// string or past its end, a length of zero or less — answers the empty string
/// rather than an error, because the shell idiom `expr substr "$s" "$i" 1` is
/// written to walk off the end and stop.
fn substr(subject: &[u8], start: &[u8], count: &[u8]) -> Str {
    let (Some(start), Some(count)) = (to_small_int(start), to_small_int(count)) else {
        return Str::new();
    };
    if start < 1 || count < 1 {
        return Str::new();
    }
    // `start` and `count` are >= 1 here, so both conversions are exact for any
    // value a string could be indexed by; a larger one saturates to a position
    // past the end, which the `skip` below turns into the empty string.
    let skip = usize::try_from(start.saturating_sub(1)).unwrap_or(usize::MAX);
    let take = usize::try_from(count).unwrap_or(usize::MAX);
    ere::from_chars(chars(subject).skip(skip).take(take))
}

/// `index STRING CHARS` — where in `STRING` any character of `CHARS` first
/// appears, counting characters from 1, or `0`.
///
/// `CHARS` is a *set*, not a substring: `expr index abcdef fc` is 3, the `c`,
/// because that is the earliest position at which any of the two appears.
fn index_of(subject: &[u8], set: &[u8]) -> Str {
    let wanted: Vec<Ch> = chars(set).collect();
    let found = chars(subject)
        .position(|c| wanted.contains(&c))
        .map_or(0, |i| i.saturating_add(1));
    found.to_string().into_bytes()
}

// ---------------------------------------------------------------- tests

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

    /// Evaluate a whole command line, as `main` does, and require it to be
    /// fully consumed.
    fn eval(tokens: &[&str]) -> String {
        let args: Vec<Str> = tokens.iter().map(|t| t.as_bytes().to_vec()).collect();
        let mut p = Parser {
            args: &args,
            pos: 0,
        };
        let v = p.or().unwrap_or_else(|Fail(m)| panic!("{m}"));
        assert!(p.peek().is_none(), "unconsumed argument {:?}", p.peek());
        String::from_utf8_lossy(&v).into_owned()
    }

    /// Evaluate, expecting the diagnostic rather than a value.
    fn eval_err(tokens: &[&str]) -> String {
        let args: Vec<Str> = tokens.iter().map(|t| t.as_bytes().to_vec()).collect();
        let mut p = Parser {
            args: &args,
            pos: 0,
        };
        match p.or() {
            Ok(v) => panic!("expected failure, got {:?}", String::from_utf8_lossy(&v)),
            Err(Fail(m)) => m,
        }
    }

    // ------------------------------------------------ arithmetic

    #[test]
    fn the_four_operations_and_the_remainder() {
        assert_eq!(eval(&["2", "+", "3"]), "5");
        assert_eq!(eval(&["10", "-", "4"]), "6");
        assert_eq!(eval(&["3", "*", "4"]), "12");
        assert_eq!(eval(&["7", "/", "2"]), "3");
        assert_eq!(eval(&["7", "%", "3"]), "1");
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        assert_eq!(eval(&["2", "+", "3", "*", "4"]), "14");
        assert_eq!(eval(&["(", "2", "+", "3", ")", "*", "4"]), "20");
    }

    #[test]
    fn subtraction_is_left_associative() {
        assert_eq!(eval(&["10", "-", "3", "-", "2"]), "5");
    }

    #[test]
    fn division_truncates_toward_zero_on_both_signs() {
        assert_eq!(eval(&["-7", "/", "2"]), "-3");
        assert_eq!(eval(&["-7", "%", "2"]), "-1");
    }

    /// The reason `expr` needs `bignum` rather than `i64`: this is the sum a
    /// shell script reaches by adding one to a file offset.
    #[test]
    fn arithmetic_is_exact_past_the_range_of_i64() {
        assert_eq!(eval(&["9223372036854775807", "+", "1"]), "9223372036854775808");
        assert_eq!(
            eval(&["99999999999999999999", "*", "99999999999999999999"]),
            "9999999999999999999800000000000000000001"
        );
    }

    #[test]
    fn leading_zeros_are_a_number_and_a_leading_plus_is_not() {
        assert_eq!(eval(&["010", "+", "1"]), "11");
        assert_eq!(eval_err(&["+1", "+", "1"]), "non-integer argument");
    }

    #[test]
    fn a_non_numeric_operand_is_an_error_not_a_zero() {
        assert_eq!(eval_err(&["foo", "+", "1"]), "non-integer argument");
        assert_eq!(eval_err(&["", "+", "1"]), "non-integer argument");
        assert_eq!(eval_err(&[" 10 ", "+", "1"]), "non-integer argument");
    }

    #[test]
    fn division_by_zero_is_refused() {
        assert_eq!(eval_err(&["1", "/", "0"]), "division by zero");
        assert_eq!(eval_err(&["5", "%", "0"]), "division by zero");
    }

    // ------------------------------------------------ comparison

    #[test]
    fn comparisons_are_numeric_when_both_sides_are_numbers() {
        assert_eq!(eval(&["3", "<", "20"]), "1");
        assert_eq!(eval(&["2", ">", "10"]), "0");
        assert_eq!(eval(&["5", "=", "5"]), "1");
        assert_eq!(eval(&["5", "!=", "6"]), "1");
        assert_eq!(eval(&["5", "<=", "5"]), "1");
        assert_eq!(eval(&["3", ">=", "3"]), "1");
        assert_eq!(eval(&["1", "==", "1"]), "1");
    }

    #[test]
    fn comparisons_are_lexicographic_otherwise() {
        assert_eq!(eval(&["apple", "<", "banana"]), "1");
        assert_eq!(eval(&["banana", "<", "apple"]), "0");
        assert_eq!(eval(&["foo", "=", "foo"]), "1");
        // "+3" is not a number here, so this compares '+' (0x2B) with '2'.
        assert_eq!(eval(&["+3", "<", "20"]), "1");
        assert_eq!(eval(&["1", "=", "+1"]), "0");
    }

    #[test]
    fn comparison_is_left_associative_and_loops() {
        // (1 = 1) = 1  ->  1 = 1  ->  1. The old expr made this a syntax error.
        assert_eq!(eval(&["1", "=", "1", "=", "1"]), "1");
    }

    #[test]
    fn arithmetic_binds_tighter_than_comparison() {
        assert_eq!(eval(&["1", "+", "1", "=", "2"]), "1");
    }

    // ------------------------------------------------ logic

    #[test]
    fn or_yields_the_first_truthy_operand() {
        assert_eq!(eval(&["hello", "|", "world"]), "hello");
        assert_eq!(eval(&["0", "|", "world"]), "world");
        assert_eq!(eval(&["", "|", "world"]), "world");
    }

    #[test]
    fn or_normalises_a_falsy_result_to_zero() {
        assert_eq!(eval(&["", "|", ""]), "0");
        assert_eq!(eval(&["0", "|", "00"]), "0");
    }

    #[test]
    fn only_a_minus_signed_run_of_zeros_is_falsy() {
        assert_eq!(eval(&["-0", "|", "x"]), "x");
        assert_eq!(eval(&["+0", "|", "x"]), "+0");
        assert_eq!(eval(&["0.0", "|", "x"]), "0.0");
        assert_eq!(eval(&["-", "|", "x"]), "-");
    }

    #[test]
    fn and_yields_the_left_operand_or_zero() {
        assert_eq!(eval(&["foo", "&", "bar"]), "foo");
        assert_eq!(eval(&["x", "&", "y", "&", "z"]), "x");
        assert_eq!(eval(&["0", "&", "bar"]), "0");
        assert_eq!(eval(&["foo", "&", ""]), "0");
    }

    #[test]
    fn or_is_looser_than_comparison() {
        assert_eq!(eval(&["1", "<", "1", "|", "2"]), "2");
    }

    // ------------------------------------------------ the colon operator

    #[test]
    fn a_pattern_without_a_group_counts_the_characters_it_matched() {
        assert_eq!(eval(&["abc", ":", "a*"]), "1");
        assert_eq!(eval(&["abc", ":", "[a-c]*"]), "3");
        assert_eq!(eval(&["aab", ":", "a\\{2\\}"]), "2");
    }

    #[test]
    fn the_match_is_anchored_at_the_start() {
        // `b*` matches the empty string at 0, not the `b` in the middle.
        assert_eq!(eval(&["abc", ":", "b*"]), "0");
        assert_eq!(eval(&["abc", ":", "c$"]), "0");
        assert_eq!(eval(&["abc", ":", "abc$"]), "3");
        assert_eq!(eval(&["abc", ":", "^abc"]), "3");
    }

    #[test]
    fn a_pattern_with_a_group_yields_the_group() {
        assert_eq!(eval(&["abc", ":", "a\\(b\\)"]), "b");
        assert_eq!(eval(&["abcabc", ":", ".*\\(b\\)"]), "b");
        assert_eq!(eval(&["abc", ":", "\\(a\\)\\(b\\)"]), "a");
    }

    #[test]
    fn a_failed_match_is_null_in_whichever_shape_the_pattern_asked_for() {
        assert_eq!(eval(&["abc", ":", "\\(b*\\)"]), "");
        assert_eq!(eval(&["abc", ":", "abcd"]), "0");
        assert_eq!(eval(&["abc", ":", ""]), "0");
    }

    /// The idiom this operator exists for, and the one the old `expr` could not
    /// spell at all.
    #[test]
    fn the_basename_idiom() {
        assert_eq!(eval(&["/usr/lib/libc.so", ":", ".*/\\(.*\\)"]), "libc.so");
        assert_eq!(eval(&["v1.24.3", ":", "v\\([0-9]*\\)"]), "1");
    }

    #[test]
    fn a_leading_star_is_a_literal_as_bre_requires() {
        assert_eq!(eval(&["abc", ":", "*"]), "0");
        assert_eq!(eval(&["*abc", ":", "*"]), "1");
    }

    #[test]
    fn gnu_bre_alternation_and_repetition_are_accepted() {
        assert_eq!(eval(&["abc", ":", "a\\|b"]), "1");
        assert_eq!(eval(&["abc", ":", "a\\+"]), "1");
    }

    #[test]
    fn a_backreference_is_refused_rather_than_mistranslated() {
        // The Pike VM cannot express one; treating `\1` as a literal `1` would
        // answer a different question with no diagnostic.
        assert!(eval_err(&["abc", ":", "\\(a\\)\\1"]).contains("backreference"));
    }

    #[test]
    fn match_is_the_same_operator_spelled_as_a_keyword() {
        assert_eq!(eval(&["match", "abc", "a"]), "1");
        assert_eq!(eval(&["match", "abc", "b"]), "0");
    }

    #[test]
    fn colon_binds_tighter_than_multiplication_and_looser_than_the_keywords() {
        // 2 * (3 : 3)  ->  2 * 1
        assert_eq!(eval(&["2", "*", "3", ":", "3"]), "2");
        // (match abc a) : x  ->  "1" : "x"
        assert_eq!(eval(&["match", "abc", "a", ":", "x"]), "0");
        // Left-associative, like every other level.
        assert_eq!(eval(&["abc", ":", "a", ":", "b"]), "0");
    }

    // ------------------------------------------------ substr, index, length

    #[test]
    fn substr_counts_from_one_and_clamps_its_length() {
        assert_eq!(eval(&["substr", "abcdef", "2", "3"]), "bcd");
        assert_eq!(eval(&["substr", "abcdef", "2", "100"]), "bcdef");
    }

    #[test]
    fn every_way_of_asking_substr_for_nothing_yields_nothing() {
        for bounds in [["0", "3"], ["9", "3"], ["2", "-1"], ["2", "x"], ["a", "3"]] {
            assert_eq!(
                eval(&["substr", "abcdef", bounds[0], bounds[1]]),
                "",
                "substr abcdef {} {}",
                bounds[0],
                bounds[1]
            );
        }
    }

    #[test]
    fn index_searches_for_any_of_a_set_of_characters() {
        assert_eq!(eval(&["index", "abcdef", "cd"]), "3");
        assert_eq!(eval(&["index", "abcdef", "fc"]), "3");
        assert_eq!(eval(&["index", "abcdef", "z"]), "0");
        assert_eq!(eval(&["index", "", "a"]), "0");
    }

    #[test]
    fn length_counts_characters() {
        assert_eq!(eval(&["length", "abcdef"]), "6");
        assert_eq!(eval(&["length", ""]), "0");
    }

    /// This system is UTF-8 throughout, so the character operations count
    /// characters — matching GNU expr in a UTF-8 locale, and differing from it
    /// in the C locale, where they count bytes.
    #[test]
    fn the_character_operations_are_not_byte_operations() {
        assert_eq!(eval(&["length", "héllo"]), "5");
        assert_eq!(eval(&["substr", "héllo", "2", "2"]), "él");
        assert_eq!(eval(&["index", "héllo", "é"]), "2");
        assert_eq!(eval(&["héllo", ":", ".é"]), "2");
        assert_eq!(eval(&["héllo", ":", "\\(.é\\)"]), "hé");
    }

    /// A path may hold any byte but `/` and NUL, and `expr` is how a portable
    /// script takes it apart.
    #[test]
    fn an_undecodable_byte_is_one_character_and_survives() {
        let args = vec![b"a\xffb".to_vec(), b":".to_vec(), b".*".to_vec()];
        let mut p = Parser {
            args: &args,
            pos: 0,
        };
        assert_eq!(p.or().ok().as_deref(), Some(&b"3"[..]));

        let args = vec![b"length".to_vec(), b"a\xffb".to_vec()];
        let mut p = Parser {
            args: &args,
            pos: 0,
        };
        assert_eq!(p.or().ok().as_deref(), Some(&b"3"[..]));
    }

    // ------------------------------------------------ the quote operator

    #[test]
    fn plus_makes_the_next_argument_a_literal() {
        assert_eq!(eval(&["+", "length"]), "length");
        assert_eq!(eval(&["+", "match"]), "match");
        assert_eq!(eval(&["+", "+"]), "+");
        assert_eq!(eval(&["+", ")"]), ")");
        assert_eq!(eval(&["1", "+", "+", "2"]), "3");
    }

    // ------------------------------------------------ syntax errors

    #[test]
    fn a_missing_operand_names_what_it_follows() {
        assert_eq!(
            eval_err(&["1", "+"]),
            "syntax error: missing argument after '+'"
        );
        assert_eq!(
            eval_err(&["length"]),
            "syntax error: missing argument after 'length'"
        );
        assert_eq!(
            eval_err(&["substr", "abc", "1"]),
            "syntax error: missing argument after '1'"
        );
        assert_eq!(
            eval_err(&["abc", ":"]),
            "syntax error: missing argument after ':'"
        );
    }

    #[test]
    fn an_unclosed_group_names_what_it_expected_after() {
        assert_eq!(
            eval_err(&["(", "1"]),
            "syntax error: expecting ')' after '1'"
        );
        assert_eq!(
            eval_err(&["(", "1", "]"]),
            "syntax error: expecting ')' instead of ']'"
        );
    }

    #[test]
    fn a_stray_close_paren_is_reported_where_it_is_found() {
        assert_eq!(eval_err(&[")"]), "syntax error: unexpected ')'");
        assert_eq!(eval_err(&["length", ")"]), "syntax error: unexpected ')'");
    }

    #[test]
    fn a_bare_operator_word_is_just_a_string() {
        assert_eq!(eval(&[":"]), ":");
        assert_eq!(eval(&["-"]), "-");
        assert_eq!(eval(&["hello"]), "hello");
        assert_eq!(eval(&["(", "42", ")"]), "42");
    }

    // ------------------------------------------------ the exit status

    #[test]
    fn the_value_decides_the_status() {
        for falsy in ["", "0", "00", "-0"] {
            assert!(is_null(falsy.as_bytes()), "{falsy:?} should be falsy");
        }
        for truthy in ["1", "-1", "+0", "0.0", "-", "x"] {
            assert!(!is_null(truthy.as_bytes()), "{truthy:?} should be truthy");
        }
    }
}
