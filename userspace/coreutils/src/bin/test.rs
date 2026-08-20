//! test — evaluate a conditional expression.
//!
//! `test EXPRESSION`, or `[ EXPRESSION ]`. Every shell script in the system
//! reaches this program, or the shell's builtin copy of it, on nearly every
//! line, and its whole product is an exit status. That is the thing to get
//! right, and there are **three** statuses, not two:
//!
//! | | |
//! |---|---|
//! | 0 | the expression is true |
//! | 1 | the expression is false |
//! | 2 | the expression is not an expression |
//!
//! The third is the one that matters and the one that is easy to lose.
//! `if [ "$x" -eq 0 ]` with a non-numeric `$x` must fail *loudly*: a `test`
//! that quietly answers "false" sends the script down the else branch, and one
//! that quietly answers "true" sends it down the then branch with garbage. The
//! implementation this replaced did the latter — `test abc -eq 0` was **true**,
//! because it parsed both sides with `unwrap_or(0)`.
//!
//! # Argument count decides before the operators do
//!
//! This is the part that looks like an implementation detail and is actually
//! the specification. POSIX defines `test` by the *number* of arguments first,
//! and only past four does a parser get involved:
//!
//! | Arguments | Rule |
//! |---|---|
//! | 0 | false |
//! | 1 | true if the argument is non-empty |
//! | 2 | `!` + one-argument, or a unary operator and its operand |
//! | 3 | a binary operator; or `!` + two-argument; or `( STRING )`; or `A -a B`/`A -o B` |
//! | 4 | `!` + three-argument; or `( … )` around a two-argument form; else the parser |
//! | 5+ | the parser |
//!
//! The order inside each row is load-bearing, because the forms overlap. At
//! three arguments the *binary operator* is looked for first, so `test ! = x`
//! compares the string `!` with the string `x` rather than negating anything,
//! and `test ( = )` compares `(` with `)`. Checking `!` first — which is what
//! reads naturally — silently changes the answer for every expression whose
//! left operand happens to be `!` or `(`.
//!
//! Beyond four arguments the grammar is
//! `expr := or`, `or := and { -o and }`, `and := term { -a term }`,
//! `term := { ! } ( '(' expr ')' | unary OPERAND | OPERAND binop OPERAND |
//! STRING )`. Note that `-a` binds **tighter** than `-o`, and that neither
//! short-circuits: GNU evaluates both sides of every `-a` and `-o` and combines
//! them afterwards, so a syntax error on the right of an already-decided
//! expression is still reported. `test x = x -o BAD -eq 1` is an error, not
//! true.
//!
//! # Integers are compared as text, at arbitrary precision
//!
//! `-eq` and its four relatives do not parse their operands into a machine
//! integer. gnulib compares the decimal strings directly, so
//! `test 99999999999999999999999999 -eq 99999999999999999999999999` is true —
//! measured, not assumed — where any `i64` implementation reports an overflow
//! or, worse, silently wraps. [`int_cmp`] reproduces this.
//!
//! What counts as an integer is also narrower and wider than `parse::<i64>()`
//! in different places. Blanks are allowed on both ends (`test 5 -eq ' 5 '` is
//! true), a leading `+` is allowed and dropped, leading zeros are allowed
//! (`-00000000005 -eq -5` is true), and **nothing else is**: `0x10` is not
//! sixteen, `010` is not eight, and an empty or all-blank operand is an error
//! rather than zero.
//!
//! # `-l STRING` is an integer
//!
//! Undocumented outside `--help`'s last line and absent from most
//! reimplementations: wherever an integer may appear in a numeric comparison,
//! `-l STRING` may appear instead and means that string's **length**. So
//! `test -l abc -eq 3` is true. It is recognised only in the dyadic position,
//! only when there are enough arguments left for it, and it is refused with a
//! specific message on the three operators that compare files rather than
//! numbers (`-nt`, `-ot`, `-ef`).
//!
//! # `--help` and `--version` are options only under the name `[`
//!
//! POSIX requires `test --help` to exit silently with status 0 — it is a
//! one-argument expression whose argument is a non-empty string, and nothing
//! more. Measured against GNU 9.4: status 0, zero bytes written, for both
//! spellings. That is not a quirk we are free to tidy; a script that runs
//! `test --version` expects a status, not a paragraph on stdout.
//!
//! The options exist only under the other name, and only in the one shape that
//! cannot be a valid expression: `[ --help` as the *sole* argument, with no
//! closing `]`. Upstream compares the string directly instead of going through
//! its option parser, so abbreviations like `[ --hel` are not accepted either.
//!
//! Under the name `[`, the final argument must be `]` and is removed before
//! anything else happens; a missing one is `missing ']'`, status 2.
//!
//! # Provenance
//!
//! The grammar above, the argument-count table, the order of the checks inside
//! each count, the exact wording of all nine diagnostics and the arbitrary-
//! precision comparison were transcribed from `coreutils-9.4/src/test.c` rather
//! than inferred from behaviour, after `scripts/test-diff.sh` showed the
//! previous implementation disagreeing with GNU on a large fraction of its
//! cases. The rules interact too much to be recovered by probing: the argument
//! count changes what an operator *means*, so a wrong count rule hides every
//! operator bug behind it.

use coreutils::quote::{os_bytes, quote};
use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs::Metadata;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::SystemTime;

/// A malformed expression. Always status 2 — `test` has no other error.
#[derive(Debug)]
struct Fail(String);

type Answer = Result<bool, Fail>;

/// True, false, and "that was not an expression".
const TRUE: u8 = 0;
const FALSE: u8 = 1;
const SYNTAX: u8 = 2;

fn main() -> ExitCode {
    let raw: Vec<OsString> = std::env::args_os().collect();
    let mut argv: Vec<Vec<u8>> = raw
        .iter()
        .map(|a| os_bytes(a.as_os_str()).into_owned())
        .collect();

    // Invoked as `[` — by the file name, so a path or an `.exe` suffix on the
    // development host still counts.
    let bracket = argv
        .first()
        .map(|a| basename_is_bracket(a))
        .unwrap_or(false);

    if bracket {
        // Direct comparison rather than an option parser, because an option
        // parser would accept `[ --hel`, and upstream is explicit that
        // abbreviations must not be recognised here.
        if argv.len() == 2 {
            if argv.get(1).is_some_and(|a| a == b"--help") {
                print!("{}", usage_text());
                return ExitCode::from(TRUE);
            }
            if argv.get(1).is_some_and(|a| a == b"--version") {
                // `[`, not `test`: this branch is only reachable under the
                // bracket alias, and GNU names the invoked program here
                // (measured: `[ (GNU coreutils) 9.4`).
                println!("[ (SlateOS coreutils) 9.4");
                return ExitCode::from(TRUE);
            }
        }
        if argv.len() < 2 || argv.last().is_some_and(|a| a != b"]") {
            return fail(bracket, &Fail(format!("missing {}", quote(b"]"))));
        }
        argv.pop();
    }

    let mut ctx = Ctx { argv, pos: 1 };
    if ctx.pos >= ctx.argc() {
        return ExitCode::from(FALSE);
    }

    let nargs = ctx.argc().saturating_sub(1);
    match ctx.posixtest(nargs) {
        Err(e) => fail(bracket, &e),
        Ok(value) => {
            // Everything must have been consumed. This is what turns
            // `test x = x y` into an error rather than a quiet truth.
            if ctx.pos != ctx.argc() {
                let extra = ctx.at(ctx.pos).to_vec();
                return fail(bracket, &Fail(format!("extra argument {}", quote(&extra))));
            }
            ExitCode::from(if value { TRUE } else { FALSE })
        }
    }
}

/// Report a malformed expression and yield status 2.
///
/// The prefix is the *invoked* name, so a failing `[ … ]` says `[:` and a
/// failing `test …` says `test:`. That is not cosmetic: the two spellings sit
/// on different lines of a script, and a message naming the wrong one sends the
/// reader to the wrong place.
///
/// GNU prints `argv[0]` verbatim, so an absolute invocation there reads
/// `/usr/bin/test: …`. We print the bare name instead, matching every other
/// utility in this crate (`split:`, `id:`, …); a path in the prefix tells the
/// reader nothing they did not already know and makes the message wrap.
/// `scripts/test-diff.sh` normalises the prefix on both sides so this choice
/// does not masquerade as a behavioural difference.
fn fail(bracket: bool, e: &Fail) -> ExitCode {
    let prog = if bracket { "[" } else { "test" };
    eprintln!("{prog}: {}", e.0);
    ExitCode::from(SYNTAX)
}

/// Whether argv[0] names this program under its bracket alias.
///
/// The `.exe` strip is for the development host only; on the target the name is
/// exactly `[`.
fn basename_is_bracket(arg0: &[u8]) -> bool {
    let name = arg0
        .rsplit(|&c| c == b'/' || c == b'\\')
        .next()
        .unwrap_or(arg0);
    let name = name.strip_suffix(b".exe").unwrap_or(name);
    name == b"["
}

/// The argument vector and the cursor into it.
///
/// `pos` starts at 1 and `argv[0]` is the program name, exactly as upstream
/// indexes them. Renumbering to a zero-based operand slice would be tidier and
/// would make every one of the `pos + 1`, `op - 1`, `argc - pos` expressions
/// below differ from the source they were transcribed from, which is the one
/// property worth keeping here.
struct Ctx {
    argv: Vec<Vec<u8>>,
    pos: usize,
}

impl Ctx {
    fn argc(&self) -> usize {
        self.argv.len()
    }

    /// The argument at `i`, or empty if past the end.
    ///
    /// Upstream reads one past the end in two places and relies on the C
    /// argument vector's trailing null pointer to notice; returning an empty
    /// slice cannot reproduce that, so the two places are handled explicitly
    /// (see [`Ctx::term`]'s `)`-expected branch).
    fn at(&self, i: usize) -> &[u8] {
        self.argv.get(i).map_or(&[][..], Vec::as_slice)
    }

    fn is(&self, i: usize, s: &[u8]) -> bool {
        self.argv.get(i).is_some_and(|a| a == s)
    }

    /// Step over one argument. With `check`, running out here is an error.
    fn advance(&mut self, check: bool) -> Result<(), Fail> {
        self.pos = self.pos.saturating_add(1);
        if check && self.pos >= self.argc() {
            return Err(self.beyond());
        }
        Ok(())
    }

    /// Step over a unary operator *and* its operand, leaving `pos - 1` on the
    /// operand.
    fn unary_advance(&mut self) -> Result<(), Fail> {
        self.advance(true)?;
        self.pos = self.pos.saturating_add(1);
        Ok(())
    }

    /// Ran out of arguments. Names the **last** argument, not the one the
    /// cursor is on — so `test x -a` says "after '-a'" and `test -f` says
    /// "after '-f'".
    fn beyond(&self) -> Fail {
        let last = self.argv.last().map_or(&[][..], Vec::as_slice).to_vec();
        Fail(format!("missing argument after {}", quote(&last)))
    }

    // --- the POSIX argument-count forms ---------------------------------

    fn posixtest(&mut self, nargs: usize) -> Answer {
        match nargs {
            1 => self.one_argument(),
            2 => self.two_arguments(),
            3 => self.three_arguments(),
            4 => {
                if self.is(self.pos, b"!") {
                    self.advance(true)?;
                    return Ok(!self.three_arguments()?);
                }
                if self.is(self.pos, b"(") && self.is(self.pos.saturating_add(3), b")") {
                    self.advance(false)?;
                    let value = self.two_arguments()?;
                    self.advance(false)?;
                    return Ok(value);
                }
                self.expr()
            }
            _ => self.expr(),
        }
    }

    fn one_argument(&mut self) -> Answer {
        let value = !self.at(self.pos).is_empty();
        self.pos = self.pos.saturating_add(1);
        Ok(value)
    }

    fn two_arguments(&mut self) -> Answer {
        if self.is(self.pos, b"!") {
            self.advance(false)?;
            return Ok(!self.one_argument()?);
        }
        if is_dash_letter(self.at(self.pos)) {
            return self.unary_operator();
        }
        Err(self.beyond())
    }

    fn three_arguments(&mut self) -> Answer {
        // The binary operator is looked for *first*. `test ! = x` is a string
        // comparison, not a negation.
        if binop(self.at(self.pos.saturating_add(1))) {
            return self.binary_operator(false);
        }
        if self.is(self.pos, b"!") {
            self.advance(true)?;
            return Ok(!self.two_arguments()?);
        }
        if self.is(self.pos, b"(") && self.is(self.pos.saturating_add(2), b")") {
            self.advance(false)?;
            let value = self.one_argument()?;
            self.advance(false)?;
            return Ok(value);
        }
        if self.is(self.pos.saturating_add(1), b"-a") || self.is(self.pos.saturating_add(1), b"-o")
        {
            return self.expr();
        }
        let bad = self.at(self.pos.saturating_add(1)).to_vec();
        Err(Fail(format!("{}: binary operator expected", quote(&bad))))
    }

    // --- the parser, for five arguments and up ---------------------------

    fn expr(&mut self) -> Answer {
        if self.pos >= self.argc() {
            return Err(self.beyond());
        }
        self.or()
    }

    /// `or := and { -o and }`.
    ///
    /// Both sides are always evaluated — `value |= and()`, not
    /// `value || and()`. Short-circuiting would be faster and would change the
    /// answer: an error in the right operand of an already-true `-o` is still
    /// an error, and a script relies on that to catch its own typos.
    fn or(&mut self) -> Answer {
        let mut value = false;
        loop {
            value |= self.and()?;
            if !(self.pos < self.argc() && self.is(self.pos, b"-o")) {
                return Ok(value);
            }
            self.advance(false)?;
        }
    }

    /// `and := term { -a term }`. Also non-short-circuiting, for the same
    /// reason, and binding tighter than `-o` by virtue of being underneath it.
    fn and(&mut self) -> Answer {
        let mut value = true;
        loop {
            value &= self.term()?;
            if !(self.pos < self.argc() && self.is(self.pos, b"-a")) {
                return Ok(value);
            }
            self.advance(false)?;
        }
    }

    fn term(&mut self) -> Answer {
        let mut negated = false;
        while self.pos < self.argc() && self.at(self.pos) == b"!" {
            self.advance(true)?;
            negated = !negated;
        }
        if self.pos >= self.argc() {
            return Err(self.beyond());
        }

        let value = if self.at(self.pos) == b"(" {
            self.paren_term()?
        } else if self.argc().saturating_sub(self.pos) >= 4
            && self.at(self.pos) == b"-l"
            && binop(self.at(self.pos.saturating_add(2)))
        {
            // `-l STRING -eq N`: the left operand is a length.
            self.binary_operator(true)?
        } else if self.argc().saturating_sub(self.pos) >= 3
            && binop(self.at(self.pos.saturating_add(1)))
        {
            self.binary_operator(false)?
        } else if is_dash_letter(self.at(self.pos)) {
            self.unary_operator()?
        } else {
            let value = !self.at(self.pos).is_empty();
            self.advance(false)?;
            value
        };

        Ok(negated ^ value)
    }

    /// `( expr )`.
    ///
    /// The span inside the parentheses is measured before it is evaluated, and
    /// measuring stops at four: a group of one to four arguments is handed to
    /// the *count* rules, and anything longer to the parser. That is why
    /// `test '(' x = x -a y = y ')'` and `test x = x -a y = y` agree — the
    /// group is six arguments, so it takes the parser branch — while
    /// `test '(' ! x ')'` is the two-argument form and negates a string.
    fn paren_term(&mut self) -> Answer {
        self.advance(true)?;
        let mut nargs = 1usize;
        while self.pos.saturating_add(nargs) < self.argc()
            && !self.is(self.pos.saturating_add(nargs), b")")
        {
            if nargs == 4 {
                nargs = self.argc().saturating_sub(self.pos);
                break;
            }
            nargs = nargs.saturating_add(1);
        }

        let value = self.posixtest(nargs)?;

        // Upstream distinguishes "ran off the end" from "found something that
        // is not `)`" by testing the argument pointer against null, which only
        // a C argument vector supplies. The length check is the same question
        // asked in a way that is true here.
        if self.pos >= self.argc() {
            return Err(Fail(format!("{} expected", quote(b")"))));
        }
        if self.at(self.pos) != b")" {
            let found = self.at(self.pos).to_vec();
            return Err(Fail(format!(
                "{} expected, found {}",
                quote(b")"),
                quote(&found)
            )));
        }
        self.advance(false)?;
        Ok(value)
    }

    // --- operators --------------------------------------------------------

    /// The dyadic forms. `l_is_l` means the left operand is `-l STRING`.
    fn binary_operator(&mut self, l_is_l: bool) -> Answer {
        if l_is_l {
            self.advance(false)?;
        }
        let op = self.pos.saturating_add(1);

        // A right operand of `-l STRING` is recognised only when there is room
        // for it — `op < argc - 2` — which is why `test 1 -eq -l` compares
        // against the *string* `-l` and reports it as an invalid integer.
        let r_is_l = op.saturating_add(2) < self.argc() && self.is(op.saturating_add(1), b"-l");
        if r_is_l {
            self.advance(false)?;
        }

        let opname = self.at(op).to_vec();

        if opname.first() == Some(&b'-') {
            if is_numeric_op(&opname) {
                let left = if l_is_l {
                    self.at(op.saturating_sub(1)).len().to_string().into_bytes()
                } else {
                    find_int(self.at(op.saturating_sub(1)))?.to_vec()
                };
                let right = if r_is_l {
                    self.at(op.saturating_add(2)).len().to_string().into_bytes()
                } else {
                    find_int(self.at(op.saturating_add(1)))?.to_vec()
                };
                let cmp = int_cmp(&left, &right);
                self.pos = self.pos.saturating_add(3);
                // `xe` distinguishes `-le`/`-ge`/`-ne` from `-lt`/`-gt`/`-eq`
                // by their third letter, exactly as upstream does.
                let xe = opname.get(2) == Some(&b'e');
                return Ok(match opname.get(1) {
                    Some(&b'l') => {
                        if xe {
                            cmp != Ordering::Greater
                        } else {
                            cmp == Ordering::Less
                        }
                    }
                    Some(&b'g') => {
                        if xe {
                            cmp != Ordering::Less
                        } else {
                            cmp == Ordering::Greater
                        }
                    }
                    // -eq (xe false) and -ne (xe true).
                    _ => (cmp != Ordering::Equal) == xe,
                });
            }

            // The three file comparisons. None accepts a length.
            if opname == b"-nt" || opname == b"-ot" || opname == b"-ef" {
                let left = self.at(op.saturating_sub(1)).to_vec();
                let right = self.at(op.saturating_add(1)).to_vec();
                self.pos = self.pos.saturating_add(3);
                if l_is_l || r_is_l {
                    let name = String::from_utf8_lossy(&opname).into_owned();
                    return Err(Fail(format!("{name} does not accept -l")));
                }
                return Ok(match opname.as_slice() {
                    b"-nt" => {
                        // Asymmetric on purpose: a file that exists is newer
                        // than one that does not.
                        let l = mtime(&left);
                        let r = mtime(&right);
                        l.is_some() && (r.is_none() || l > r)
                    }
                    b"-ot" => {
                        let l = mtime(&left);
                        let r = mtime(&right);
                        r.is_some() && (l.is_none() || l < r)
                    }
                    _ => same_file(&left, &right),
                });
            }

            return Err(Fail(format!(
                "{}: unknown binary operator",
                quote(&opname)
            )));
        }

        let left = self.at(self.pos).to_vec();
        let right = self.at(self.pos.saturating_add(2)).to_vec();
        self.pos = self.pos.saturating_add(3);
        // `==` is a GNU extension and is exactly `=`.
        if opname == b"=" || opname == b"==" {
            return Ok(left == right);
        }
        if opname == b"!=" {
            return Ok(left != right);
        }
        // `binop` admitted it, so one of the branches above must have taken it.
        Err(Fail(format!(
            "{}: unknown binary operator",
            quote(&opname)
        )))
    }

    fn unary_operator(&mut self) -> Answer {
        let letter = self.at(self.pos).get(1).copied().unwrap_or(0);
        // `-t` and the string tests take a non-file operand; everything else
        // takes a file name. All of them consume operator and operand.
        match letter {
            b'e' | b'r' | b'w' | b'x' | b'f' | b'd' | b's' | b'b' | b'c' | b'p' | b'S' | b'g'
            | b'u' | b'k' | b'O' | b'G' | b'L' | b'h' | b'N' | b'n' | b'z' | b't' => {}
            _ => {
                let bad = self.at(self.pos).to_vec();
                return Err(Fail(format!("{}: unary operator expected", quote(&bad))));
            }
        }
        self.unary_advance()?;
        let operand = self.at(self.pos.saturating_sub(1)).to_vec();

        Ok(match letter {
            b'n' => !operand.is_empty(),
            b'z' => operand.is_empty(),
            b't' => {
                // `-t N` runs its operand through the *same* integer check as
                // `-eq` and friends, so `test -t x` is `invalid integer 'x'`,
                // status 2 — not false. Upstream: `arg = find_int (argv[pos-1])`
                // before the `strtol`, and `find_int` is the routine that
                // raises the syntax error.
                //
                // The two failures are therefore not the same failure. A
                // *malformed* number is an error; a well-formed one that is
                // merely too large to be a descriptor is quietly false,
                // because upstream learns about that from `strtol`'s ERANGE
                // rather than from the syntax check. `test -t 99999999999999`
                // is false; `test -t x` is an error.
                let digits = find_int(&operand)?;
                let fd = std::str::from_utf8(digits.trim_ascii_end())
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok());
                match fd {
                    Some(n) => i32::try_from(n).is_ok_and(|fd| fd >= 0 && is_a_tty(fd)),
                    None => false,
                }
            }
            b'e' => stat(&operand).is_some(),
            b'r' => access(&operand, AccessMode::Read),
            b'w' => access(&operand, AccessMode::Write),
            b'x' => access(&operand, AccessMode::Execute),
            b'f' => stat(&operand).is_some_and(|m| m.is_file()),
            b'd' => stat(&operand).is_some_and(|m| m.is_dir()),
            b's' => stat(&operand).is_some_and(|m| m.len() > 0),
            b'L' | b'h' => lstat(&operand).is_some_and(|m| m.file_type().is_symlink()),
            b'N' => stat(&operand).is_some_and(|m| {
                // Modified since last read: mtime > atime.
                match (m.modified().ok(), m.accessed().ok()) {
                    (Some(mt), Some(at)) => mt > at,
                    _ => false,
                }
            }),
            b'b' => is_kind(&operand, Kind::Block),
            b'c' => is_kind(&operand, Kind::Char),
            b'p' => is_kind(&operand, Kind::Fifo),
            b'S' => is_kind(&operand, Kind::Socket),
            b'g' => has_mode_bit(&operand, 0o2000),
            b'u' => has_mode_bit(&operand, 0o4000),
            b'k' => has_mode_bit(&operand, 0o1000),
            b'O' => owned_by_euid(&operand),
            b'G' => owned_by_egid(&operand),
            _ => false,
        })
    }
}

/// `-X` where X is exactly one character — upstream's
/// `arg[0] == '-' && arg[1] && !arg[2]`.
///
/// This tests the *shape*, not membership in the operator set: `--` passes it
/// and is then rejected by name inside [`Ctx::unary_operator`] as
/// `'--': unary operator expected`. That two-step is exactly why `test -- x`
/// is an error rather than an end-of-options marker followed by a string test:
/// `test` has no `--` convention, so `--` is either an ordinary string
/// (`test --` alone is true, being non-empty) or a misspelled operator.
/// Multi-character names like `-ef` fail the shape test and are only ever
/// reachable as *binary* operators.
fn is_dash_letter(arg: &[u8]) -> bool {
    arg.len() == 2 && arg.first() == Some(&b'-')
}

/// The dyadic operator names. `<` and `>` are **not** here: those are bash's
/// `[[ ]]`, and `test a '<' b` is a three-argument form with no binary operator
/// in it, hence an error.
fn binop(s: &[u8]) -> bool {
    matches!(
        s,
        b"=" | b"!=" | b"==" | b"-nt" | b"-ot" | b"-ef" | b"-eq" | b"-ne" | b"-lt" | b"-le"
            | b"-gt" | b"-ge"
    )
}

/// `-eq -ne -lt -le -gt -ge`, spelled the way upstream detects them: first
/// letter in `lgen`, third in `etq`, exactly three characters.
fn is_numeric_op(op: &[u8]) -> bool {
    if op.len() != 3 {
        return false;
    }
    let (a, b) = (op.get(1).copied().unwrap_or(0), op.get(2).copied().unwrap_or(0));
    ((a == b'l' || a == b'g') && (b == b'e' || b == b't'))
        || (a == b'e' && b == b'q')
        || (a == b'n' && b == b'e')
}

/// Validate an integer operand and return the slice the comparison should see.
///
/// The returned slice starts at the digits (or at the `-`), so a leading `+`
/// and any leading blanks are dropped; trailing blanks are left on, because the
/// comparison ignores them. An operand that is not an integer is an *error*,
/// which is the single most important behaviour in this file.
fn find_int(s: &[u8]) -> Result<&[u8], Fail> {
    let mut i = 0usize;
    while matches!(s.get(i), Some(b' ' | b'\t')) {
        i = i.saturating_add(1);
    }
    let start = if s.get(i) == Some(&b'+') {
        i = i.saturating_add(1);
        i
    } else {
        let here = i;
        if s.get(i) == Some(&b'-') {
            i = i.saturating_add(1);
        }
        here
    };

    if !matches!(s.get(i), Some(b'0'..=b'9')) {
        return Err(Fail(format!("invalid integer {}", quote(s))));
    }
    while matches!(s.get(i), Some(b'0'..=b'9')) {
        i = i.saturating_add(1);
    }
    while matches!(s.get(i), Some(b' ' | b'\t')) {
        i = i.saturating_add(1);
    }
    if i != s.len() {
        return Err(Fail(format!("invalid integer {}", quote(s))));
    }
    Ok(s.get(start..).unwrap_or_default())
}

/// Compare two decimal integer strings **without converting them to a machine
/// integer**, so the comparison is exact at any width.
///
/// This is not an optimisation avoided; it is the specified behaviour. gnulib's
/// `strintcmp` compares the text, and `test` inherits arbitrary precision from
/// it — verified against GNU 9.4, which answers true to
/// `test 99999999999999999999999999 -eq 99999999999999999999999999`.
///
/// The inputs are what [`find_int`] returned: an optional `-`, digits, and
/// possibly trailing blanks.
fn int_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (a_neg, a_digits) = split_sign(a);
    let (b_neg, b_digits) = split_sign(b);

    // Zero has no sign: `-0` and `0` compare equal, so the sign test has to
    // come after stripping leading zeros rather than before.
    let a_zero = a_digits.is_empty();
    let b_zero = b_digits.is_empty();
    if a_zero && b_zero {
        return Ordering::Equal;
    }
    let a_neg = a_neg && !a_zero;
    let b_neg = b_neg && !b_zero;

    match (a_neg, b_neg) {
        (false, true) => return Ordering::Greater,
        (true, false) => return Ordering::Less,
        _ => {}
    }

    // Same sign: longer is bigger in magnitude, then lexicographic. Reversed
    // for two negatives.
    let magnitude = a_digits
        .len()
        .cmp(&b_digits.len())
        .then_with(|| a_digits.cmp(b_digits));
    if a_neg { magnitude.reverse() } else { magnitude }
}

/// Split off the sign and the leading zeros and the trailing blanks, leaving
/// the significant digits. An all-zero number leaves nothing, which is what
/// makes `-0 == 0` fall out.
fn split_sign(s: &[u8]) -> (bool, &[u8]) {
    let neg = s.first() == Some(&b'-');
    let mut d = if neg { s.get(1..).unwrap_or_default() } else { s };
    while d.last().is_some_and(|c| matches!(c, b' ' | b'\t')) {
        d = d.get(..d.len().saturating_sub(1)).unwrap_or_default();
    }
    while d.first() == Some(&b'0') {
        d = d.get(1..).unwrap_or_default();
    }
    (neg, d)
}

// --- the filesystem, and what the host can and cannot answer ----------------

fn path_of(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn stat(name: &[u8]) -> Option<Metadata> {
    std::fs::metadata(path_of(name)).ok()
}

fn lstat(name: &[u8]) -> Option<Metadata> {
    std::fs::symlink_metadata(path_of(name)).ok()
}

fn mtime(name: &[u8]) -> Option<SystemTime> {
    stat(name).and_then(|m| m.modified().ok())
}

enum AccessMode {
    Read,
    Write,
    Execute,
}

enum Kind {
    Block,
    Char,
    Fifo,
    Socket,
}

#[cfg(unix)]
unsafe extern "C" {
    /// The *effective*-uid form, not `access`. `test -r` must answer for the
    /// user the process is running as, not the one that started it — the two
    /// differ under a setuid binary, which is exactly when the answer matters.
    fn euidaccess(path: *const u8, mode: i32) -> i32;
    fn isatty(fd: i32) -> i32;
    fn geteuid() -> u32;
    fn getegid() -> u32;
}

#[cfg(unix)]
fn access(name: &[u8], mode: AccessMode) -> bool {
    let bits = match mode {
        AccessMode::Read => 4,
        AccessMode::Write => 2,
        AccessMode::Execute => 1,
    };
    let mut c_path = name.to_vec();
    if c_path.contains(&0) {
        return false;
    }
    c_path.push(0);
    // SAFETY: `c_path` is a NUL-terminated byte string with no interior NUL,
    // and lives across the call. `euidaccess` reads it and nothing else.
    unsafe { euidaccess(c_path.as_ptr(), bits) == 0 }
}

/// On the development host there is no POSIX permission model to consult.
///
/// Reporting "no" for everything would be worse than useless — `test -r FILE`
/// is true for practically every readable file, and a blanket false makes every
/// script that guards a read take the wrong branch. So existence stands in for
/// readability, the read-only attribute answers writability, and executability
/// is decided by the extension. This is a host-only approximation and is
/// documented as one; on SlateOS the branch above is what runs.
#[cfg(not(unix))]
fn access(name: &[u8], mode: AccessMode) -> bool {
    let Some(meta) = stat(name) else {
        return false;
    };
    match mode {
        AccessMode::Read => true,
        AccessMode::Write => !meta.permissions().readonly(),
        AccessMode::Execute => {
            if meta.is_dir() {
                return true;
            }
            let lower = name.to_ascii_lowercase();
            [b".exe".as_slice(), b".bat", b".cmd", b".com"]
                .iter()
                .any(|ext| lower.ends_with(ext))
        }
    }
}

#[cfg(unix)]
fn is_a_tty(fd: i32) -> bool {
    // SAFETY: `isatty` takes an integer and touches no memory. An invalid
    // descriptor is answered with 0 and `errno`, not undefined behaviour.
    unsafe { isatty(fd) == 1 }
}

#[cfg(not(unix))]
fn is_a_tty(fd: i32) -> bool {
    use std::io::IsTerminal;
    match fd {
        0 => std::io::stdin().is_terminal(),
        1 => std::io::stdout().is_terminal(),
        2 => std::io::stderr().is_terminal(),
        // The host has no way to ask about a descriptor this process did not
        // name, and inventing an answer would be worse than admitting none.
        _ => false,
    }
}

#[cfg(unix)]
fn is_kind(name: &[u8], kind: Kind) -> bool {
    use std::os::unix::fs::FileTypeExt;
    stat(name).is_some_and(|m| {
        let t = m.file_type();
        match kind {
            Kind::Block => t.is_block_device(),
            Kind::Char => t.is_char_device(),
            Kind::Fifo => t.is_fifo(),
            Kind::Socket => t.is_socket(),
        }
    })
}

/// The host has no block devices, character devices, FIFOs or sockets in the
/// filesystem namespace, so the honest answer is "no" — and unlike the
/// permission bits above, "no" here is also the *correct* answer for every path
/// a host test can name.
#[cfg(not(unix))]
fn is_kind(_name: &[u8], _kind: Kind) -> bool {
    false
}

#[cfg(unix)]
fn has_mode_bit(name: &[u8], bit: u32) -> bool {
    use std::os::unix::fs::PermissionsExt;
    stat(name).is_some_and(|m| m.permissions().mode() & bit != 0)
}

/// Setuid, setgid and sticky have no representation on the host.
#[cfg(not(unix))]
fn has_mode_bit(_name: &[u8], _bit: u32) -> bool {
    false
}

#[cfg(unix)]
fn owned_by_euid(name: &[u8]) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: a getter with no arguments and no failure mode.
    let me = unsafe { geteuid() };
    stat(name).is_some_and(|m| m.uid() == me)
}

/// Every file the host can see belongs to whoever is running, as far as this
/// program can tell, so the useful approximation is "it exists".
#[cfg(not(unix))]
fn owned_by_euid(name: &[u8]) -> bool {
    stat(name).is_some()
}

#[cfg(unix)]
fn owned_by_egid(name: &[u8]) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: as above.
    let me = unsafe { getegid() };
    stat(name).is_some_and(|m| m.gid() == me)
}

#[cfg(not(unix))]
fn owned_by_egid(name: &[u8]) -> bool {
    stat(name).is_some()
}

#[cfg(unix)]
fn same_file(a: &[u8], b: &[u8]) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (stat(a), stat(b)) {
        (Some(x), Some(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// Device and inode are not reachable through stable `std` on Windows, so the
/// host compares canonical paths instead. That agrees with the real test for
/// every case except a hard link, which the host answers "no" to and the target
/// answers "yes" to.
#[cfg(not(unix))]
fn same_file(a: &[u8], b: &[u8]) -> bool {
    match (
        std::fs::canonicalize(path_of(a)),
        std::fs::canonicalize(path_of(b)),
    ) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn usage_text() -> String {
    "\
Usage: test EXPRESSION
  or:  test
  or:  [ EXPRESSION ]
  or:  [ ]
  or:  [ OPTION
Exit with the status determined by EXPRESSION.

      --help        display this help and exit
      --version     output version information and exit

An omitted EXPRESSION defaults to false.  Otherwise,
EXPRESSION is true or false and sets exit status.  It is one of:

  ( EXPRESSION )               EXPRESSION is true
  ! EXPRESSION                 EXPRESSION is false
  EXPRESSION1 -a EXPRESSION2   both EXPRESSION1 and EXPRESSION2 are true
  EXPRESSION1 -o EXPRESSION2   either EXPRESSION1 or EXPRESSION2 is true

  -n STRING            the length of STRING is nonzero
  STRING               equivalent to -n STRING
  -z STRING            the length of STRING is zero
  STRING1 = STRING2    the strings are equal
  STRING1 != STRING2   the strings are not equal

  INTEGER1 -eq INTEGER2   INTEGER1 is equal to INTEGER2
  INTEGER1 -ge INTEGER2   INTEGER1 is greater than or equal to INTEGER2
  INTEGER1 -gt INTEGER2   INTEGER1 is greater than INTEGER2
  INTEGER1 -le INTEGER2   INTEGER1 is less than or equal to INTEGER2
  INTEGER1 -lt INTEGER2   INTEGER1 is less than INTEGER2
  INTEGER1 -ne INTEGER2   INTEGER1 is not equal to INTEGER2

  FILE1 -ef FILE2   FILE1 and FILE2 have the same device and inode numbers
  FILE1 -nt FILE2   FILE1 is newer (modification date) than FILE2
  FILE1 -ot FILE2   FILE1 is older than FILE2

  -b FILE     FILE exists and is block special
  -c FILE     FILE exists and is character special
  -d FILE     FILE exists and is a directory
  -e FILE     FILE exists
  -f FILE     FILE exists and is a regular file
  -g FILE     FILE exists and is set-group-ID
  -G FILE     FILE exists and is owned by the effective group ID
  -h FILE     FILE exists and is a symbolic link (same as -L)
  -k FILE     FILE exists and has its sticky bit set
  -L FILE     FILE exists and is a symbolic link (same as -h)
  -N FILE     FILE exists and has been modified since it was last read
  -O FILE     FILE exists and is owned by the effective user ID
  -p FILE     FILE exists and is a named pipe
  -r FILE     FILE exists and the user has read access
  -s FILE     FILE exists and has a size greater than zero
  -S FILE     FILE exists and is a socket
  -t FD       file descriptor FD is opened on a terminal
  -u FILE     FILE exists and its set-user-ID bit is set
  -w FILE     FILE exists and the user has write access
  -x FILE     FILE exists and the user has execute (or search) access

Except for -h and -L, all FILE-related tests dereference symbolic links.
Beware that parentheses need to be escaped (e.g., by backslashes) for shells.
INTEGER may also be -l STRING, which evaluates to the length of STRING.

NOTE: Binary -a and -o are inherently ambiguous.  Use 'test EXPR1 && test
EXPR2' or 'test EXPR1 || test EXPR2' instead.

NOTE: [ honors the --help and --version options, but test does not.
test treats each of those as it treats any other nonempty STRING.
"
    .to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Evaluate as `test` would, returning the exit status: 0, 1 or 2.
    fn status(args: &[&str]) -> u8 {
        let mut argv: Vec<Vec<u8>> = vec![b"test".to_vec()];
        argv.extend(args.iter().map(|a| a.as_bytes().to_vec()));
        let argc = argv.len();
        let mut ctx = Ctx { argv, pos: 1 };
        if argc <= 1 {
            return FALSE;
        }
        match ctx.posixtest(argc - 1) {
            Err(_) => SYNTAX,
            Ok(v) => {
                if ctx.pos != ctx.argc() {
                    SYNTAX
                } else if v {
                    TRUE
                } else {
                    FALSE
                }
            }
        }
    }

    /// The message a malformed expression produces, for the cases where the
    /// wording is the thing under test.
    fn message(args: &[&str]) -> String {
        let mut argv: Vec<Vec<u8>> = vec![b"test".to_vec()];
        argv.extend(args.iter().map(|a| a.as_bytes().to_vec()));
        let argc = argv.len();
        let mut ctx = Ctx { argv, pos: 1 };
        match ctx.posixtest(argc - 1) {
            Err(e) => e.0,
            Ok(_) => {
                if ctx.pos != ctx.argc() {
                    format!("extra argument {}", quote(ctx.at(ctx.pos)))
                } else {
                    String::new()
                }
            }
        }
    }

    // --- the three statuses ------------------------------------------------

    /// The bug this rewrite exists for. A non-numeric operand to `-eq` is an
    /// *error*, status 2 — not false, and emphatically not true, which is what
    /// the previous implementation answered because it parsed both sides with
    /// `unwrap_or(0)` and then compared 0 with 0.
    #[test]
    fn a_non_numeric_integer_operand_is_an_error_not_a_zero() {
        assert_eq!(status(&["abc", "-eq", "0"]), SYNTAX);
        assert_eq!(status(&["0", "-eq", "abc"]), SYNTAX);
        assert_eq!(message(&["abc", "-eq", "0"]), "invalid integer 'abc'");
    }

    /// The other half: an operand that *is* numeric still compares.
    #[test]
    fn integers_compare_by_value() {
        assert_eq!(status(&["1", "-eq", "1"]), TRUE);
        assert_eq!(status(&["1", "-eq", "2"]), FALSE);
        assert_eq!(status(&["1", "-ne", "2"]), TRUE);
        assert_eq!(status(&["1", "-lt", "2"]), TRUE);
        assert_eq!(status(&["2", "-le", "2"]), TRUE);
        assert_eq!(status(&["3", "-gt", "2"]), TRUE);
        assert_eq!(status(&["3", "-ge", "4"]), FALSE);
    }

    /// Beyond `i64`. A machine-integer implementation reports an overflow or
    /// wraps; the comparison is over the text, so it simply works.
    #[test]
    fn integers_compare_at_arbitrary_precision() {
        let big = "99999999999999999999999999";
        assert_eq!(status(&[big, "-eq", big]), TRUE);
        assert_eq!(status(&[big, "-gt", "5"]), TRUE);
        assert_eq!(status(&["5", "-lt", big]), TRUE);
        let bigger = "99999999999999999999999999999999";
        assert_eq!(status(&[bigger, "-gt", big]), TRUE);
        assert_eq!(status(&[&format!("-{bigger}"), "-lt", &format!("-{big}")]), TRUE);
    }

    /// What counts as an integer: blanks and a sign yes, other bases no.
    #[test]
    fn an_integer_may_carry_blanks_a_sign_and_leading_zeros_and_nothing_else() {
        assert_eq!(status(&[" 5 ", "-eq", "5"]), TRUE);
        assert_eq!(status(&["+1", "-eq", "1"]), TRUE);
        assert_eq!(status(&["-00000000005", "-eq", "-5"]), TRUE);
        assert_eq!(status(&["-0", "-eq", "0"]), TRUE);
        // Not hexadecimal, not octal, not empty.
        assert_eq!(status(&["0x10", "-eq", "16"]), SYNTAX);
        assert_eq!(status(&["010", "-eq", "8"]), FALSE);
        assert_eq!(status(&["010", "-eq", "10"]), TRUE);
        assert_eq!(status(&["", "-eq", "1"]), SYNTAX);
        assert_eq!(status(&[" ", "-eq", "0"]), SYNTAX);
    }

    // --- the argument-count rules ------------------------------------------

    #[test]
    fn no_arguments_is_false_and_one_empty_argument_is_false_too() {
        assert_eq!(status(&[]), FALSE);
        assert_eq!(status(&[""]), FALSE);
        assert_eq!(status(&["x"]), TRUE);
    }

    /// A lone operator name is a *string*, and a non-empty one. This is the
    /// count rule beating the operator table, and it is why `test -f` cannot
    /// mean "is -f a file".
    #[test]
    fn a_lone_operator_name_is_just_a_non_empty_string() {
        assert_eq!(status(&["-f"]), TRUE);
        assert_eq!(status(&["!"]), TRUE);
        assert_eq!(status(&["("]), TRUE);
        assert_eq!(status(&["-a"]), TRUE);
    }

    /// At three arguments the binary operator is looked for **before** `!` and
    /// before `(`. Reversing those two checks — which is the natural way to
    /// write it — changes the answer here without changing anything else, so
    /// this test is the guard on that ordering.
    #[test]
    fn a_three_argument_form_prefers_the_binary_operator_over_a_leading_bang() {
        // `!` compared with `x` as strings: they differ, so `=` is false.
        assert_eq!(status(&["!", "=", "x"]), FALSE);
        assert_eq!(status(&["!", "=", "!"]), TRUE);
        // And `(` likewise, rather than being an unclosed group.
        assert_eq!(status(&["(", "=", ")"]), FALSE);
        assert_eq!(status(&["(", "=", "("]), TRUE);
    }

    #[test]
    fn two_arguments_are_a_negation_or_a_unary_operator() {
        assert_eq!(status(&["!", ""]), TRUE);
        assert_eq!(status(&["!", "x"]), FALSE);
        assert_eq!(status(&["-n", "x"]), TRUE);
        assert_eq!(status(&["-n", ""]), FALSE);
        assert_eq!(status(&["-z", ""]), TRUE);
        assert_eq!(status(&["-z", "x"]), FALSE);
    }

    /// Two arguments where the first is neither `!` nor a `-X` operator is an
    /// error, and the message is the "ran off the end" one rather than
    /// something about operators — an upstream oddity, reproduced.
    #[test]
    fn two_arguments_that_are_not_an_operator_are_an_error() {
        assert_eq!(status(&["x", "y"]), SYNTAX);
        assert_eq!(status(&["-q", "foo"]), SYNTAX);
        assert_eq!(message(&["-q", "foo"]), "'-q': unary operator expected");
        // `--` is two characters after the dash, so it is not a unary operator.
        assert_eq!(status(&["--", "x"]), SYNTAX);
    }

    #[test]
    fn three_arguments_can_be_a_parenthesised_string() {
        assert_eq!(status(&["(", "x", ")"]), TRUE);
        assert_eq!(status(&["(", "", ")"]), FALSE);
    }

    #[test]
    fn four_arguments_negate_a_three_argument_form_or_bracket_a_two() {
        assert_eq!(status(&["!", "x", "=", "x"]), FALSE);
        assert_eq!(status(&["!", "x", "=", "y"]), TRUE);
        assert_eq!(status(&["(", "-n", "x", ")"]), TRUE);
        assert_eq!(status(&["(", "-z", "x", ")"]), FALSE);
    }

    // --- the parser ---------------------------------------------------------

    /// `-a` binds tighter than `-o`. Written as `(x=x) -o ((y=y) -a (''=z))`,
    /// this is true; parsed left to right it would be false, because
    /// `'' = z` is false and would drag the whole thing down.
    #[test]
    fn and_binds_tighter_than_or() {
        assert_eq!(status(&["x", "=", "x", "-o", "y", "=", "y", "-a", "", "=", "z"]), TRUE);
        assert_eq!(status(&["", "=", "x", "-a", "x", "=", "x", "-o", "y", "=", "y"]), TRUE);
    }

    /// Neither connective short-circuits, so an error on the far side of an
    /// already-decided expression is still an error. A short-circuiting
    /// implementation returns true for the first of these.
    #[test]
    fn a_syntax_error_is_reported_even_when_the_answer_is_already_known() {
        assert_eq!(status(&["x", "=", "x", "-o", "abc", "-eq", "1"]), SYNTAX);
        assert_eq!(status(&["x", "=", "y", "-a", "abc", "-eq", "1"]), SYNTAX);
    }

    #[test]
    fn parentheses_group_and_nest() {
        assert_eq!(status(&["(", "x", "=", "y", "-o", "y", "=", "y", ")"]), TRUE);
        assert_eq!(status(&["(", "(", "x", "=", "x", ")", ")"]), TRUE);
        assert_eq!(status(&["!", "(", "x", "=", "x", ")"]), FALSE);
        assert_eq!(status(&["!", "(", "x", "=", "y", ")"]), TRUE);
        assert_eq!(
            status(&["(", "-n", "x", ")", "-a", "(", "-z", "", ")"]),
            TRUE
        );
    }

    /// A group of more than four arguments takes the parser branch, so it
    /// agrees with the same expression written without the parentheses.
    #[test]
    fn a_long_parenthesised_group_is_parsed_rather_than_counted() {
        assert_eq!(
            status(&["(", "x", "=", "x", "-a", "y", "=", "y", ")"]),
            status(&["x", "=", "x", "-a", "y", "=", "y"])
        );
    }

    #[test]
    fn repeated_bangs_cancel() {
        assert_eq!(status(&["!", "!", "-n", "x"]), TRUE);
        assert_eq!(status(&["!", "!", "!", "x"]), FALSE);
    }

    // --- the length operator ------------------------------------------------

    /// `-l STRING` is an integer equal to the string's length, on either side.
    #[test]
    fn dash_l_measures_a_string_where_an_integer_is_expected() {
        assert_eq!(status(&["-l", "abc", "-eq", "3"]), TRUE);
        assert_eq!(status(&["-l", "abc", "-gt", "2"]), TRUE);
        assert_eq!(status(&["3", "-eq", "-l", "abc"]), TRUE);
        assert_eq!(status(&["-l", "", "-eq", "0"]), TRUE);
    }

    /// It is refused, by name, on the three operators that compare files.
    #[test]
    fn dash_l_is_refused_by_the_file_comparisons() {
        assert_eq!(status(&["-l", "abc", "-ef", "x"]), SYNTAX);
        assert_eq!(message(&["-l", "abc", "-ef", "x"]), "-ef does not accept -l");
        assert_eq!(message(&["-l", "abc", "-nt", "x"]), "-nt does not accept -l");
        assert_eq!(message(&["-l", "abc", "-ot", "x"]), "-ot does not accept -l");
    }

    // --- diagnostics --------------------------------------------------------

    #[test]
    fn running_out_of_arguments_names_the_last_one() {
        assert_eq!(message(&["x", "-a"]), "missing argument after '-a'");
        assert_eq!(message(&["1", "-eq"]), "missing argument after '-eq'");
        assert_eq!(message(&["-f"]), "");
        assert_eq!(status(&["-f"]), TRUE);
    }

    #[test]
    fn an_unbalanced_parenthesis_says_so() {
        assert_eq!(status(&["(", "x"]), SYNTAX);
        assert_eq!(status(&["(", "x", "=", "x"]), SYNTAX);
        assert_eq!(status(&["(", "(", "x", ")"]), SYNTAX);
    }

    #[test]
    /// The two ways a leftover argument is reported are not interchangeable,
    /// and which one you get depends on *where* the parser stopped. Both
    /// spellings verified against GNU 9.4.
    fn a_trailing_argument_is_an_extra_argument() {
        assert_eq!(status(&["x", "=", "x", "y"]), SYNTAX);
        // Parsed a whole expression, then found something after it.
        assert_eq!(message(&["(", "x", ")", "x"]), "extra argument 'x'");
        assert_eq!(message(&["x", "-a", "y", "z"]), "extra argument 'z'");
        // Ran off the end mid-expression instead: `( x ) )` matches the
        // four-argument `( ... )` shape on its *outer* parens, so the inner
        // two arguments `x )` are handed to the two-argument rule, which wants
        // an operator and has none.
        assert_eq!(message(&["(", "x", ")", ")"]), "missing argument after ')'");
    }

    #[test]
    fn a_missing_binary_operator_is_named() {
        assert_eq!(message(&["x", "y", "z"]), "'y': binary operator expected");
    }

    /// `<` and `>` belong to bash's `[[ ]]`, not to `test`. Accepting them
    /// would make `test a '<' b` quietly true where GNU errors.
    #[test]
    fn angle_brackets_are_not_comparison_operators() {
        assert_eq!(status(&["x", "<", "y"]), SYNTAX);
        assert_eq!(status(&["x", ">", "y"]), SYNTAX);
    }

    /// `-t` validates its descriptor number with the *same* routine as `-eq`,
    /// so a malformed one is status 2 and only an out-of-range one is false.
    /// Caught by `scripts/test-diff.sh`, which found `test -t x` answering
    /// false where GNU says `invalid integer 'x'` — the same class of silent
    /// wrongness this rewrite exists to remove, surviving inside the rewrite
    /// itself because the operand did not look like a number to me.
    #[test]
    fn dash_t_separates_a_malformed_descriptor_from_an_out_of_range_one() {
        assert_eq!(message(&["-t", "x"]), "invalid integer 'x'");
        assert_eq!(message(&["-t", ""]), "invalid integer ''");
        assert_eq!(message(&["-t", " "]), "invalid integer ' '");
        // Well-formed but far past any descriptor: false, and silent.
        assert_eq!(status(&["-t", "99999999999999999999"]), FALSE);
        assert_eq!(message(&["-t", "99999999999999999999"]), "");
        assert_eq!(status(&["-t", "-1"]), FALSE);
    }

    // --- the bracket alias --------------------------------------------------

    #[test]
    fn the_bracket_alias_is_recognised_by_file_name() {
        assert!(basename_is_bracket(b"["));
        assert!(basename_is_bracket(b"/usr/bin/["));
        assert!(basename_is_bracket(b"C:\\slate\\[.exe"));
        assert!(!basename_is_bracket(b"test"));
        assert!(!basename_is_bracket(b"/usr/bin/test"));
        assert!(!basename_is_bracket(b"[["));
    }

    // --- the pieces, directly ------------------------------------------------

    #[test]
    fn find_int_returns_the_significant_text_or_an_error() {
        assert_eq!(find_int(b"5").unwrap(), b"5");
        assert_eq!(find_int(b"+5").unwrap(), b"5");
        assert_eq!(find_int(b"-5").unwrap(), b"-5");
        assert_eq!(find_int(b"  7  ").unwrap(), b"7  ");
        assert!(find_int(b"").is_err());
        assert!(find_int(b"-").is_err());
        assert!(find_int(b"+").is_err());
        assert!(find_int(b"5x").is_err());
        assert!(find_int(b"x5").is_err());
        assert!(find_int(b"5 5").is_err());
    }

    #[test]
    fn int_cmp_orders_by_value_not_by_text() {
        assert_eq!(int_cmp(b"9", b"10"), Ordering::Less);
        assert_eq!(int_cmp(b"10", b"9"), Ordering::Greater);
        assert_eq!(int_cmp(b"007", b"7"), Ordering::Equal);
        assert_eq!(int_cmp(b"-0", b"0"), Ordering::Equal);
        assert_eq!(int_cmp(b"-1", b"1"), Ordering::Less);
        assert_eq!(int_cmp(b"-10", b"-9"), Ordering::Less);
        assert_eq!(int_cmp(b"7  ", b"7"), Ordering::Equal);
    }

    #[test]
    fn is_dash_letter_accepts_only_a_dash_and_one_character() {
        assert!(is_dash_letter(b"-f"));
        assert!(is_dash_letter(b"-1"));
        assert!(!is_dash_letter(b"-"));
        assert!(!is_dash_letter(b"-ef"));
        assert!(!is_dash_letter(b"f"));
        // `--` is dash-letter *shaped*, so it reaches `unary_operator` and is
        // refused there by name. Verified against GNU 9.4: `test -- x` is
        // status 2 with this message, and `test --` alone is status 0.
        assert!(is_dash_letter(b"--"));
        assert_eq!(message(&["--", "x"]), "'--': unary operator expected");
        assert_eq!(status(&["--"]), TRUE);
    }
}
