//! Slate OS `dc` — desk calculator (reverse Polish notation)
//!
//! A traditional RPN calculator: arbitrary-precision fixed-point numbers, 256
//! register stacks, strings as macros, and conditional execution.
//!
//! # It is `bc` with a different syntax, and it shares `bc`'s numbers
//!
//! Every arithmetic command here is a call into [`bignum::Decimal`], the same
//! type `userspace/bc` computes on. That is not tidiness for its own sake:
//! historically `bc` was a *preprocessor* that translated infix into this
//! language and piped it to `dc`, so the two disagreeing about what `1/3` is,
//! or about the scale of a product, would be a contradiction rather than an
//! inconsistency. Until this rewrite `dc` computed in `f64` while its own
//! documentation claimed arbitrary precision — `2 200 ^ p` answered
//! `1606938044258990000000000000000000000000000000000000000000000` where the
//! true value ends `...835301376`, and every digit past the seventeenth was
//! fiction. See `design-decisions.md` §324.
//!
//! # A `dc` string is bytes, and so is a `dc` program
//!
//! `a` turns a number into a one-byte string, so a program can compute a byte
//! and print it — which means a string here may hold any of the 256 values,
//! not only those that spell something in some encoding. Registers are named
//! by a byte for the same reason, and there are exactly 256 of them. The
//! interpreter therefore runs on `[u8]` throughout: source, macro bodies,
//! register names and output. Reading a script as text instead would refuse to
//! run a perfectly good program over one byte in a string literal.
//!
//! # Errors do not stop the program
//!
//! A command that cannot be carried out — a division by zero, a pop from an
//! empty stack — writes a diagnostic and is abandoned. Execution resumes at the
//! next command with the stack as the failed command left it. This is what
//! traditional `dc` does, and it is the same rule `bc` follows one level up
//! (`design-decisions.md` §323): the granularity is the largest unit that can
//! be discarded without owing anyone a value, which in a language of
//! stack-machine commands is one command.

#![allow(unexpected_cfgs)]

use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::process;

use bignum::{BigInt, Decimal, DecimalError};

// ── Values ─────────────────────────────────────────────────────────

/// A stack entry. `dc` mixes numbers and strings on one stack; a string is
/// both a datum and a macro body, depending on which command reaches it.
///
/// A `dc` string is a sequence of *bytes*, not text: `2 55 * a` builds the byte
/// 110 with no claim that it is a character, `P` writes it out unchanged, and a
/// script may hold a string in any encoding at all — none of which survives
/// being forced through `String`. Holding `Vec<u8>` is also what lets the whole
/// interpreter run on bytes, so a `dc` program in Latin-1 executes instead of
/// being rejected before its first command.
#[derive(Debug, Clone)]
enum Value {
    Num(Decimal),
    Str(Vec<u8>),
}

impl Value {
    /// Render for output: a number in `obase` and broken at `line_length`, a
    /// string exactly as the program wrote it.
    ///
    /// The asymmetry is deliberate. A number is `dc`'s own rendering of a
    /// value and may be continued across lines with a trailing `\`; a string
    /// is the user's bytes, and inserting a backslash into it would corrupt
    /// the one thing they asked to be printed verbatim.
    fn display(&self, obase: u32, line_length: usize) -> Vec<u8> {
        match self {
            Value::Num(n) => bignum::wrap_number(&n.format(obase), line_length).into_bytes(),
            Value::Str(s) => s.clone(),
        }
    }
}

/// The output line length, from the environment or the traditional default.
///
/// A value of 0 turns the line break off. A setting that is not a number is
/// ignored rather than rejected: a malformed environment should not stop a
/// calculator from calculating.
fn line_length_from_env(var: &str) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(bignum::DEFAULT_LINE_LENGTH)
}

/// Why a command could not be carried out.
///
/// A string message would do, but naming the cases keeps the wording in one
/// place — `dc`'s diagnostics are the only thing a script can observe about a
/// failure, since the exit status stays zero.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DcError {
    StackEmpty,
    NotANumber(Vec<u8>),
    Math(DecimalError),
    BadInputBase,
    BadOutputBase,
    NegativeScale,
    NegativeExponent,
    ZeroModulus,
    NonIntegerModularArgument,
    MissingRegister(u8),
    TooDeep,
    Io(String),
}

impl From<DecimalError> for DcError {
    fn from(e: DecimalError) -> Self {
        DcError::Math(e)
    }
}

impl std::fmt::Display for DcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DcError::StackEmpty => write!(f, "stack empty"),
            // A diagnostic is prose for a person, not data for a program, so a
            // string that is not valid UTF-8 is shown with replacement
            // characters rather than suppressing the message. The value itself
            // is never routed through here.
            DcError::NotANumber(s) => {
                write!(f, "not a number: '{}'", String::from_utf8_lossy(s))
            }
            DcError::Math(e) => write!(f, "{e}"),
            DcError::BadInputBase => write!(f, "input base must be between 2 and 16 (inclusive)"),
            DcError::BadOutputBase => write!(f, "output base must be between 2 and 36 (inclusive)"),
            DcError::NegativeScale => write!(f, "scale must be a non-negative integer"),
            DcError::NegativeExponent => write!(f, "negative exponent in modular exponentiation"),
            DcError::ZeroModulus => write!(f, "remainder by zero in modular exponentiation"),
            DcError::NonIntegerModularArgument => {
                write!(f, "modular exponentiation requires integer arguments")
            }
            DcError::MissingRegister(c) => {
                write!(f, "'{}' requires a register name", char::from(*c))
            }
            DcError::TooDeep => write!(f, "macro recursion too deep"),
            DcError::Io(e) => write!(f, "write: {e}"),
        }
    }
}

/// What execution should do after a chunk of input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Fell off the end of the input; carry on.
    Normal,
    /// Stop, and keep stopping for this many more enclosing macro levels.
    Quit(usize),
}

/// Non-tail macro calls that may be nested before we refuse.
///
/// `dc`'s loops are written as recursive macros, so a limit here would be a
/// limit on how many times a program may iterate — which is why the tail call
/// is optimised away instead (see [`Dc::execute`]) and this bound only ever
/// sees *genuine* nesting. It exists so that a macro that calls itself in a
/// non-tail position reports a diagnostic rather than overflowing the process
/// stack, which is not an error a calculator can catch.
const MAX_DEPTH: usize = 256;

// ── The machine ────────────────────────────────────────────────────

struct Dc<'a> {
    stack: Vec<Value>,
    /// 256 register stacks, indexed by the byte that names them.
    registers: Vec<Vec<Value>>,
    ibase: u32,
    obase: u32,
    /// `dc`'s `k`: the number of fractional digits division and square root
    /// produce. It is *not* a property of the numbers on the stack, each of
    /// which carries its own scale.
    scale: usize,
    /// Columns before a printed number is broken with a trailing `\`.
    line_length: usize,
    depth: usize,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl<'a> Dc<'a> {
    fn new(out: &'a mut dyn Write, err: &'a mut dyn Write) -> Self {
        Self {
            stack: Vec::new(),
            registers: vec![Vec::new(); 256],
            ibase: 10,
            obase: 10,
            scale: 0,
            line_length: line_length_from_env("DC_LINE_LENGTH"),
            depth: 0,
            out,
            err,
        }
    }

    // ── Stack helpers ──

    fn pop(&mut self) -> Result<Value, DcError> {
        self.stack.pop().ok_or(DcError::StackEmpty)
    }

    fn pop_num(&mut self) -> Result<Decimal, DcError> {
        match self.pop()? {
            Value::Num(n) => Ok(n),
            Value::Str(s) => Err(DcError::NotANumber(s)),
        }
    }

    /// Pop two numbers, returning them in the order they were pushed.
    ///
    /// Every binary operator wants `a op b` where `b` was on top, and popping
    /// them in the wrong order is the classic RPN bug — one place to get it
    /// right is better than eight.
    fn pop_two(&mut self) -> Result<(Decimal, Decimal), DcError> {
        let b = self.pop_num()?;
        let a = match self.pop_num() {
            Ok(a) => a,
            Err(e) => {
                // The first operand is still owed to the stack: a failed
                // command must not silently eat the value it did manage to
                // pop, or the stack is left in a state the user cannot explain.
                self.stack.push(Value::Num(b));
                return Err(e);
            }
        };
        Ok((a, b))
    }

    /// Pop a number that must be a non-negative integer index.
    fn pop_count(&mut self) -> Result<usize, DcError> {
        let n = self.pop_num()?.rescale(0);
        if n.is_negative() {
            return Ok(0);
        }
        Ok(n.digits.to_usize_saturating())
    }

    fn push_num(&mut self, n: Decimal) {
        self.stack.push(Value::Num(n));
    }

    fn write_out(&mut self, bytes: &[u8]) -> Result<(), DcError> {
        self.out
            .write_all(bytes)
            .map_err(|e| DcError::Io(e.to_string()))
    }

    fn report(&mut self, e: &DcError) {
        // A diagnostic that cannot be written is the end of the line for
        // reporting; there is nowhere left to say so.
        let _ = writeln!(self.err, "dc: {e}");
    }

    // ── Execution ──

    /// Run a chunk of `dc` source.
    ///
    /// Macro invocation in *tail position* — the last command in the current
    /// chunk — replaces the chunk rather than recursing, because that is how
    /// every loop in `dc` is written:
    ///
    /// ```text
    /// [ ... 1 - d 0 <L ] sL
    /// ```
    ///
    /// The macro's last act is to call itself. Recursing in Rust for that would
    /// make the maximum iteration count a function of the process stack size —
    /// a loop of a million would abort the process rather than produce an
    /// answer. Replacing the chunk makes it a loop, in constant stack, which is
    /// what the source says it is.
    fn execute(&mut self, input: &[u8]) -> Flow {
        let mut program: Vec<u8> = input.to_vec();

        'tail: loop {
            let mut i = 0usize;
            while let Some(&c) = program.get(i) {
                // Every arm returns the index of the next command, so that the
                // ones that read a register name or scan a number can consume
                // more than one character without a shared mutable cursor.
                let step = match self.step(&program, i, c) {
                    Ok(Step::Next(next)) => next,
                    Ok(Step::Call { body, next }) => {
                        if is_tail_position(&program, next) {
                            program = body;
                            continue 'tail;
                        }
                        match self.call(&body) {
                            Ok(Flow::Normal) => next,
                            Ok(Flow::Quit(n)) => {
                                // This macro is one of the levels being quit.
                                match n.checked_sub(1) {
                                    Some(0) | None => next,
                                    Some(rest) => return Flow::Quit(rest),
                                }
                            }
                            Err(e) => {
                                self.report(&e);
                                next
                            }
                        }
                    }
                    Ok(Step::Quit(n)) => return Flow::Quit(n),
                    Err(e) => {
                        self.report(&e);
                        // Resume after the command that failed. `advance_past`
                        // knows how many characters it would have consumed, so
                        // a failed `sa` does not leave the `a` to be run as a
                        // command in its own right.
                        advance_past(&program, i, c)
                    }
                };
                i = step;
            }
            return Flow::Normal;
        }
    }

    /// Run a macro body one level deeper, refusing if that is too deep.
    fn call(&mut self, body: &[u8]) -> Result<Flow, DcError> {
        if self.depth >= MAX_DEPTH {
            return Err(DcError::TooDeep);
        }
        self.depth = self.depth.saturating_add(1);
        let flow = self.execute(body);
        self.depth = self.depth.saturating_sub(1);
        Ok(flow)
    }

    /// Carry out the command at `i`, reporting where the next one starts.
    #[allow(clippy::too_many_lines)]
    fn step(&mut self, program: &[u8], i: usize, c: u8) -> Result<Step, DcError> {
        let next = i.saturating_add(1);

        match c {
            b' ' | b'\t' | b'\n' | b'\r' => {}

            // A number. `_` is the negative sign, because `-` is subtraction.
            // `A`-`F` are digit values 10-15 and may begin a number, so the
            // scanner has to trigger on them as well as on `0`-`9`.
            b'0'..=b'9' | b'.' | b'_' | b'A'..=b'F' => {
                let negative = c == b'_';
                let mut j = if negative { next } else { i };
                let mut text = String::new();
                while let Some(&d) = program.get(j) {
                    if d.is_ascii_digit() || d == b'.' || (b'A'..=b'F').contains(&d) {
                        text.push(char::from(d));
                        j = j.saturating_add(1);
                    } else {
                        break;
                    }
                }
                let value = Decimal::parse(&text, self.ibase);
                self.push_num(if negative { value.negate() } else { value });
                return Ok(Step::Next(j));
            }

            // A string, which nests, so that a macro can contain a macro.
            b'[' => {
                let mut depth = 1usize;
                let mut s: Vec<u8> = Vec::new();
                let mut j = next;
                while let Some(&d) = program.get(j) {
                    j = j.saturating_add(1);
                    match d {
                        b'[' => {
                            depth = depth.saturating_add(1);
                            s.push(b'[');
                        }
                        b']' => {
                            depth = depth.saturating_sub(1);
                            if depth == 0 {
                                break;
                            }
                            s.push(b']');
                        }
                        _ => s.push(d),
                    }
                }
                self.stack.push(Value::Str(s));
                return Ok(Step::Next(j));
            }

            // ── Arithmetic ──
            b'+' => {
                let (a, b) = self.pop_two()?;
                self.push_num(a.add(&b));
            }
            b'-' => {
                let (a, b) = self.pop_two()?;
                self.push_num(a.sub(&b));
            }
            b'*' => {
                let (a, b) = self.pop_two()?;
                // POSIX's scale for a product, not the `k` register: `k`
                // governs division, where digits must be invented.
                self.push_num(a.multiply(&b, self.scale));
            }
            b'/' => {
                let (a, b) = self.pop_two()?;
                self.push_num(a.div(&b, self.scale)?);
            }
            b'%' => {
                let (a, b) = self.pop_two()?;
                self.push_num(a.modulo(&b, self.scale)?);
            }
            b'~' => {
                // Quotient and remainder together, quotient pushed first.
                let (a, b) = self.pop_two()?;
                let quotient = a.div(&b, self.scale)?;
                let remainder = a.modulo(&b, self.scale)?;
                self.push_num(quotient);
                self.push_num(remainder);
            }
            b'^' => {
                let (base, exp) = self.pop_two()?;
                self.push_num(base.pow(&exp, self.scale)?);
            }
            b'|' => {
                let modulus = self.pop_num()?;
                let exp = self.pop_num()?;
                let base = self.pop_num()?;
                self.push_num(modular_power(&base, &exp, &modulus)?);
            }
            b'v' => {
                let n = self.pop_num()?;
                // POSIX gives a square root the larger of `k` and the operand's
                // own scale, so `v` on an exact value does not throw away
                // digits the operand already carried.
                let target = self.scale.max(n.scale);
                self.push_num(n.sqrt(target)?);
            }

            // ── Stack ──
            b'p' => {
                let val = self.stack.last().ok_or(DcError::StackEmpty)?.clone();
                let mut line = val.display(self.obase, self.line_length);
                line.push(b'\n');
                self.write_out(&line)?;
            }
            b'n' => {
                let val = self.pop()?;
                let s = val.display(self.obase, self.line_length);
                self.write_out(&s)?;
            }
            b'f' => {
                // The whole stack, top first, without disturbing it.
                let lines: Vec<Vec<u8>> = self
                    .stack
                    .iter()
                    .rev()
                    .map(|v| v.display(self.obase, self.line_length))
                    .collect();
                for mut line in lines {
                    line.push(b'\n');
                    self.write_out(&line)?;
                }
            }
            b'P' => {
                // A string prints as itself; a number prints as the bytes of
                // its magnitude, base 256, most significant first.
                let bytes = match self.pop()? {
                    Value::Str(s) => s,
                    Value::Num(n) => base256_bytes(&n),
                };
                self.write_out(&bytes)?;
            }
            b'a' => {
                // The top of the stack as a one-byte string: a number becomes
                // its low-order byte, a string keeps its first byte. This is
                // how a `dc` program builds output it computed -- `2 55 * a P`
                // writes `n` -- and it is the reason a string here is bytes
                // rather than text: byte 200 is a perfectly good result and is
                // not a character in any encoding `dc` gets to assume.
                let byte = match self.pop()? {
                    Value::Num(n) => {
                        let (_, rem) = n.rescale(0).digits.divmod(&BigInt::from_i64(256));
                        let magnitude = u8::try_from(rem.to_usize_saturating() & 0xff).unwrap_or(0);
                        // A negative value contributes the same low byte its
                        // two's-complement representation would.
                        if rem.negative && magnitude != 0 {
                            Some(0u8.wrapping_sub(magnitude))
                        } else {
                            Some(magnitude)
                        }
                    }
                    // An empty string has no first byte, and inventing one
                    // would put a byte on the stack the program never made.
                    Value::Str(s) => s.first().copied(),
                };
                self.stack
                    .push(Value::Str(byte.map(|b| vec![b]).unwrap_or_default()));
            }
            b'c' => self.stack.clear(),
            b'd' => {
                let val = self.stack.last().ok_or(DcError::StackEmpty)?.clone();
                self.stack.push(val);
            }
            b'r' => {
                let len = self.stack.len();
                let (Some(x), Some(y)) = (len.checked_sub(1), len.checked_sub(2)) else {
                    return Err(DcError::StackEmpty);
                };
                self.stack.swap(x, y);
            }
            b'R' => {
                let n = self.pop_count()?;
                let len = self.stack.len();
                if n > 1 && n <= len {
                    let top = self.pop()?;
                    // `len - n` is the position the rotated element moves to;
                    // `len` has not changed yet at the time it is computed.
                    let at = len.saturating_sub(n);
                    self.stack.insert(at, top);
                }
            }
            b'z' => {
                let depth = i64::try_from(self.stack.len()).unwrap_or(i64::MAX);
                self.push_num(Decimal::from_i64(depth));
            }
            b'Z' => {
                let len = match self.pop()? {
                    Value::Num(n) => n.length(),
                    // Bytes, not characters: `Z` answers how much there is to
                    // print, and `P` prints bytes.
                    Value::Str(s) => s.len(),
                };
                self.push_num(Decimal::from_i64(i64::try_from(len).unwrap_or(i64::MAX)));
            }
            b'X' => {
                let scale = match self.pop()? {
                    Value::Num(n) => n.scale,
                    // A string has no fractional part, and `dc` answers zero
                    // rather than refusing.
                    Value::Str(_) => 0,
                };
                self.push_num(Decimal::from_i64(i64::try_from(scale).unwrap_or(i64::MAX)));
            }

            // ── Parameters ──
            b'i' => {
                let base = self.pop_num()?.rescale(0);
                let value = base.digits.to_usize_saturating();
                if base.is_negative() || !(2..=16).contains(&value) {
                    return Err(DcError::BadInputBase);
                }
                self.ibase = u32::try_from(value).unwrap_or(10);
            }
            b'o' => {
                let base = self.pop_num()?.rescale(0);
                let value = base.digits.to_usize_saturating();
                if base.is_negative() || !(2..=36).contains(&value) {
                    return Err(DcError::BadOutputBase);
                }
                self.obase = u32::try_from(value).unwrap_or(10);
            }
            b'k' => {
                let k = self.pop_num()?.rescale(0);
                if k.is_negative() {
                    return Err(DcError::NegativeScale);
                }
                self.scale = k.digits.to_usize_saturating();
            }
            b'I' => {
                self.push_num(Decimal::from_i64(i64::from(self.ibase)));
            }
            b'O' => {
                self.push_num(Decimal::from_i64(i64::from(self.obase)));
            }
            b'K' => {
                let k = i64::try_from(self.scale).unwrap_or(i64::MAX);
                self.push_num(Decimal::from_i64(k));
            }

            // ── Registers ──
            b's' | b'l' | b'S' | b'L' => {
                let reg = register_at(program, next, c)?;
                match c {
                    // `s` replaces the top of the register stack; `S` pushes.
                    b's' => {
                        let val = self.pop()?;
                        let slot = self.register_mut(reg);
                        if slot.is_empty() {
                            slot.push(val);
                        } else {
                            let last = slot.len().saturating_sub(1);
                            if let Some(top) = slot.get_mut(last) {
                                *top = val;
                            }
                        }
                    }
                    b'S' => {
                        let val = self.pop()?;
                        self.register_mut(reg).push(val);
                    }
                    // An unset register reads as zero, which is what makes the
                    // `0 sX` initialisation that every dc program opens with
                    // optional rather than required.
                    b'l' => {
                        let val = self
                            .register_mut(reg)
                            .last()
                            .cloned()
                            .unwrap_or(Value::Num(Decimal::zero()));
                        self.stack.push(val);
                    }
                    _ => {
                        let val = self
                            .register_mut(reg)
                            .pop()
                            .unwrap_or(Value::Num(Decimal::zero()));
                        self.stack.push(val);
                    }
                }
                return Ok(Step::Next(next.saturating_add(1)));
            }

            // ── Macros and conditionals ──
            b'x' => {
                return Ok(match self.pop()? {
                    Value::Str(body) => Step::Call { body, next },
                    // A number is not a macro; `dc` puts it back rather than
                    // discarding it.
                    other @ Value::Num(_) => {
                        self.stack.push(other);
                        Step::Next(next)
                    }
                });
            }
            b'>' | b'<' | b'=' => {
                let reg = register_at(program, next, c)?;
                let after = next.saturating_add(1);
                return Ok(self.conditional(c, false, reg, after));
            }
            b'!' if matches!(program.get(next), Some(b'>' | b'<' | b'=')) => {
                let op = program.get(next).copied().unwrap_or(b'=');
                let name_at = next.saturating_add(1);
                let reg = register_at(program, name_at, op)?;
                let after = name_at.saturating_add(1);
                return Ok(self.conditional(op, true, reg, after));
            }

            // ── Input ──
            b'?' => {
                // Read the line as bytes: a `?` may be answered with anything
                // the caller's terminal or pipe produced, and refusing input
                // that is not UTF-8 would make the command unusable for the
                // byte-building programs `a` and `P` exist to serve.
                let mut line: Vec<u8> = Vec::new();
                io::stdin()
                    .lock()
                    .read_until(b'\n', &mut line)
                    .map_err(|e| DcError::Io(e.to_string()))?;
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                return Ok(Step::Call { body: line, next });
            }

            // `q` leaves two levels: the macro that ran it and the one that
            // called that. At the top level there are not two, so dc exits.
            b'q' => return Ok(Step::Quit(2)),
            b'Q' => {
                let levels = self.pop_count()?;
                return Ok(Step::Quit(levels.max(1)));
            }

            b'#' => {
                let mut j = i;
                while let Some(&d) = program.get(j) {
                    if d == b'\n' {
                        break;
                    }
                    j = j.saturating_add(1);
                }
                return Ok(Step::Next(j));
            }

            // Traditional dc ignores what it does not recognise.
            _ => {}
        }

        Ok(Step::Next(next))
    }

    /// Evaluate a comparison and, if it holds, hand back the macro to run.
    ///
    /// The comparison is `a op b` where `b` is the *second* value popped —
    /// `3 5 <a` asks whether 3 is less than 5, reading in source order rather
    /// than stack order.
    fn conditional(&mut self, op: u8, negated: bool, reg: u8, next: usize) -> Step {
        let (a, b) = match self.pop_two() {
            Ok(pair) => pair,
            Err(e) => {
                self.report(&e);
                return Step::Next(next);
            }
        };
        // The top of the stack is the *left* operand: `5 3 >a` asks whether 3
        // is greater than 5, not the other way round. That reads backwards
        // until you write the two idioms every `dc` program is built from --
        // `[d 1 - d 1 <F *] sF` and `[1 + d 20 >L] sL` -- both of which put
        // the loop bound on top and only terminate under this reading. POSIX
        // words it as "the top-of-stack is greater"; the previous version of
        // this file had it reversed, which silently turned every such loop
        // into a single pass (30! came out as 870).
        let ordering = b.cmp(&a);
        let holds = match op {
            b'>' => ordering.is_gt(),
            b'<' => ordering.is_lt(),
            _ => ordering.is_eq(),
        } != negated;

        if !holds {
            return Step::Next(next);
        }
        match self.register_mut(reg).last().cloned() {
            Some(Value::Str(body)) => Step::Call { body, next },
            // A register holding a number is not a macro; the condition is
            // satisfied and there is simply nothing to run.
            _ => Step::Next(next),
        }
    }

    fn register_mut(&mut self, reg: u8) -> &mut Vec<Value> {
        // `registers` is built with exactly 256 entries and `reg` is a `u8`, so
        // the slot always exists; the fallback keeps the promise without an
        // index that could panic if that ever stopped being true.
        let idx = reg as usize;
        if idx >= self.registers.len() {
            self.registers.resize(idx.saturating_add(1), Vec::new());
        }
        self.registers.get_mut(idx).unwrap_or(&mut self.stack)
    }
}

/// What [`Dc::step`] decided.
enum Step {
    /// Continue at this index.
    Next(usize),
    /// Run this macro body, then continue at `next`.
    Call { body: Vec<u8>, next: usize },
    /// Stop, unwinding this many levels.
    Quit(usize),
}

/// Whether index `at` is past the last command in `program`.
///
/// Only whitespace may follow: a macro call is in tail position when nothing
/// remains for the caller to do after it returns, which is exactly when its
/// frame can be reused rather than stacked.
fn is_tail_position(program: &[u8], at: usize) -> bool {
    program
        .get(at..)
        .is_none_or(|rest| rest.iter().all(u8::is_ascii_whitespace))
}

/// The index just past the command starting at `i`, for use after a failure.
///
/// The commands that take a register name occupy two characters (three for
/// `!<`, `!>`, `!=`). Resuming at `i + 1` after one of them fails would run the
/// register name as a command — a failed `sa` would push nothing and then be
/// followed by a stray `a`.
fn advance_past(program: &[u8], i: usize, c: u8) -> usize {
    let width = match c {
        b's' | b'l' | b'S' | b'L' | b'>' | b'<' | b'=' => 2,
        b'!' if matches!(program.get(i.saturating_add(1)), Some(b'>' | b'<' | b'=')) => 3,
        _ => 1,
    };
    i.saturating_add(width)
}

/// The register name at `at`, as the byte that indexes it.
///
/// Registers are named by a byte and there are 256 of them, so every byte is a
/// valid name — including the ones that are half of a multi-byte character in
/// some encoding. That is `dc`'s own rule, not a simplification of it.
fn register_at(program: &[u8], at: usize, command: u8) -> Result<u8, DcError> {
    program
        .get(at)
        .copied()
        .ok_or(DcError::MissingRegister(command))
}

/// `(base ^ exp) mod modulus`, on integers.
///
/// The three operands are truncated to integers by `dc`'s `|`, but silently
/// truncating them here would answer a question the user did not ask, so a
/// fractional operand is refused instead.
fn modular_power(base: &Decimal, exp: &Decimal, modulus: &Decimal) -> Result<Decimal, DcError> {
    for operand in [base, exp, modulus] {
        if operand.scale > 0 && operand != &operand.rescale(0) {
            return Err(DcError::NonIntegerModularArgument);
        }
    }
    if exp.is_negative() {
        return Err(DcError::NegativeExponent);
    }
    let m = modulus.rescale(0).digits;
    if m.is_zero() {
        return Err(DcError::ZeroModulus);
    }

    let mut result = BigInt::one();
    let mut b = reduce(&base.rescale(0).digits, &m);
    let mut e = exp.rescale(0).digits;
    let two = BigInt::from_i64(2);
    while !e.is_zero() {
        let (half, rem) = e.divmod(&two);
        if !rem.is_zero() {
            result = reduce(&result.mul(&b), &m);
        }
        e = half;
        if !e.is_zero() {
            b = reduce(&b.mul(&b), &m);
        }
    }
    Ok(Decimal {
        digits: result,
        scale: 0,
    })
}

/// `v mod m`, always in `[0, |m|)`.
///
/// `divmod` gives a remainder with the dividend's sign, which is right for `%`
/// and wrong for modular arithmetic — a residue is not negative.
fn reduce(v: &BigInt, m: &BigInt) -> BigInt {
    let (_, mut r) = v.divmod(m);
    if r.negative {
        let mut magnitude = m.clone();
        magnitude.negative = false;
        r = r.add(&magnitude);
    }
    r.normalize();
    r
}

/// The magnitude of `n` as base-256 digits, most significant first.
///
/// This is what `dc`'s `P` prints for a number: the value read as a string of
/// bytes, which is how a `dc` program emits text it computed.
fn base256_bytes(n: &Decimal) -> Vec<u8> {
    let mut value = n.rescale(0).digits;
    value.negative = false;
    if value.is_zero() {
        return vec![0];
    }
    let radix = BigInt::from_i64(256);
    let mut bytes = Vec::new();
    while !value.is_zero() {
        let (q, r) = value.divmod(&radix);
        bytes.push(u8::try_from(r.to_usize_saturating()).unwrap_or(0));
        value = q;
    }
    bytes.reverse();
    bytes
}

// ── Main ───────────────────────────────────────────────────────────

fn run() -> Result<(), String> {
    let mut files: Vec<String> = Vec::new();
    let mut expressions: Vec<String> = Vec::new();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("Usage: dc [OPTIONS] [FILE]...");
                println!("Desk calculator (reverse Polish notation).");
                println!();
                println!("  -e, --expression=EXPR   evaluate EXPR");
                println!("  -f, --file=FILE         execute FILE");
                println!("  -h, --help              display this help");
                process::exit(0);
            }
            "-e" | "--expression" => {
                let expr = args.next().ok_or("option '-e' requires an argument")?;
                expressions.push(expr);
            }
            "-f" | "--file" => {
                let file = args.next().ok_or("option '-f' requires an argument")?;
                files.push(file);
            }
            _ => {
                if let Some(expr) = arg.strip_prefix("--expression=") {
                    expressions.push(expr.to_string());
                } else if let Some(file) = arg.strip_prefix("--file=") {
                    files.push(file.to_string());
                } else {
                    files.push(arg);
                }
            }
        }
    }

    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut out = stdout.lock();
    let mut err = stderr.lock();
    let mut dc = Dc::new(&mut out, &mut err);

    for expr in &expressions {
        if dc.execute(expr.as_bytes()) != Flow::Normal {
            return Ok(());
        }
    }

    for file in &files {
        // Bytes, not text. A `dc` script is a byte stream: `[»]P` is a
        // perfectly good program in whatever encoding its author used, and
        // `read_to_string` would refuse to run it at all rather than execute
        // the arithmetic and print the bytes back.
        let content = fs::read(file).map_err(|e| format!("{file}: {e}"))?;
        if dc.execute(&content) != Flow::Normal {
            return Ok(());
        }
    }

    if expressions.is_empty() && files.is_empty() {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        loop {
            let mut line: Vec<u8> = Vec::new();
            if input
                .read_until(b'\n', &mut line)
                .map_err(|e| format!("read: {e}"))?
                == 0
            {
                break;
            }
            if dc.execute(&line) != Flow::Normal {
                return Ok(());
            }
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("dc: {e}");
        process::exit(1);
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// Run `input`, returning what it wrote to stdout.
    fn eval(input: &str) -> String {
        String::from_utf8(eval_bytes(input.as_bytes())).unwrap_or_default()
    }

    /// Run `input`, returning the raw bytes it wrote to stdout.
    ///
    /// `dc`'s output is not text — `a` and `P` exist precisely to emit bytes
    /// that no encoding need accept — so the tests for those go through this
    /// rather than through [`eval`], which would flatten them to nothing.
    fn eval_bytes(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut dc = Dc::new(&mut out, &mut err);
            dc.execute(input);
        }
        out
    }

    /// Run `input`, returning what it wrote to stderr.
    fn eval_err(input: &str) -> String {
        let mut out = Vec::new();
        let mut err = Vec::new();
        {
            let mut dc = Dc::new(&mut out, &mut err);
            dc.execute(input.as_bytes());
        }
        String::from_utf8(err).unwrap_or_default()
    }

    /// Run `input`, returning the stack that is left.
    fn eval_stack(input: &str) -> Vec<Value> {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut dc = Dc::new(&mut out, &mut err);
        dc.execute(input.as_bytes());
        dc.stack
    }

    // ── The reason this file was rewritten ──

    #[test]
    fn a_power_that_does_not_fit_a_double_is_still_exact() {
        // In f64 this answered 1606938044258990000000000000000000000000000000
        // 000000000000000, with every digit past the seventeenth invented.
        assert_eq!(
            eval("2 200 ^ p"),
            "1606938044258990275541962092341162602522202993782792835301376\n"
        );
    }

    #[test]
    fn integers_past_two_to_the_fifty_three_stay_exact() {
        // 2^53 + 1 is the first integer an f64 cannot represent.
        assert_eq!(eval("9007199254740993 p"), "9007199254740993\n");
        assert_eq!(eval("9007199254740992 1 + p"), "9007199254740993\n");
        assert_eq!(
            eval("12345678901234567890 d * p"),
            "152415787532388367501905199875019052100\n"
        );
    }

    #[test]
    fn a_quotient_has_exactly_the_digits_k_asks_for() {
        // Truncation, not rounding, and no binary approximation underneath.
        assert_eq!(eval("1 3 / p"), "0\n");
        assert_eq!(eval("20 k 1 3 / p"), ".33333333333333333333\n");
        assert_eq!(eval("20 k 2 3 / p"), ".66666666666666666666\n");
        // Every one of the five places `k` asked for is printed. Trimming the
        // zeros would make the two lines below indistinguishable.
        assert_eq!(eval("5 k 1 10 / p"), ".10000\n");
        assert_eq!(eval("1 k 1 10 / p"), ".1\n");
    }

    #[test]
    fn a_root_is_correct_to_the_digit_k_asks_for() {
        assert_eq!(eval("30 k 2 v p"), "1.414213562373095048801688724209\n");
        assert_eq!(eval("144 v p"), "12\n");
    }

    // ── Basic arithmetic ──

    #[test]
    fn test_add() {
        assert_eq!(eval("3 5 + p"), "8\n");
    }

    #[test]
    fn test_subtract() {
        assert_eq!(eval("10 3 - p"), "7\n");
    }

    #[test]
    fn test_multiply() {
        assert_eq!(eval("4 5 * p"), "20\n");
    }

    #[test]
    fn test_divide() {
        assert_eq!(eval("20 4 / p"), "5\n");
    }

    #[test]
    fn test_remainder() {
        assert_eq!(eval("17 5 % p"), "2\n");
    }

    #[test]
    fn test_power() {
        assert_eq!(eval("2 10 ^ p"), "1024\n");
    }

    #[test]
    fn test_sqrt() {
        assert_eq!(eval("144 v p"), "12\n");
    }

    #[test]
    fn a_negative_is_entered_with_underscore_and_printed_with_a_minus() {
        // `_` exists on input only because `-` is already the subtraction
        // command. Output has no such collision, and dc prints `-`.
        assert_eq!(eval("_5 3 + p"), "-2\n");
        assert_eq!(eval("_5 p"), "-5\n");
        assert_eq!(eval("0 5 - p"), "-5\n");
    }

    #[test]
    fn a_product_follows_the_posix_scale_rule() {
        // `k` governs division; a product keeps the digits it already has.
        assert_eq!(eval("1.5 1.5 * p"), "2.2\n");
        assert_eq!(eval("10 k 1.5 1.5 * p"), "2.25\n");
    }

    #[test]
    fn a_sum_carries_the_larger_scale_of_its_operands() {
        assert_eq!(eval("0.1 0.02 + p"), ".12\n");
        assert_eq!(eval("1 0.5 - p"), ".5\n");
    }

    // ── Stack operations ──

    #[test]
    fn test_duplicate() {
        assert_eq!(eval("5 d + p"), "10\n");
    }

    #[test]
    fn test_swap() {
        assert_eq!(eval("3 5 r p"), "3\n");
    }

    #[test]
    fn test_clear() {
        assert!(eval_stack("1 2 3 c").is_empty());
    }

    #[test]
    fn test_stack_depth() {
        assert_eq!(eval("1 2 3 z p"), "3\n");
    }

    #[test]
    fn test_print_stack() {
        assert_eq!(eval("1 2 3 f"), "3\n2\n1\n");
    }

    #[test]
    fn test_print_no_newline() {
        assert_eq!(eval("42 n"), "42");
    }

    #[test]
    fn test_rotate() {
        // Bring the third element to the top.
        assert_eq!(eval("1 2 3 3 R f"), "2\n1\n3\n");
    }

    #[test]
    fn a_number_prints_as_bytes_with_capital_p() {
        // 0x48 0x69 -- the way a dc program emits text.
        assert_eq!(eval("18537 P"), "Hi");
        assert_eq!(eval("[hello] P"), "hello");
    }

    // ── Registers ──

    #[test]
    fn test_store_load() {
        assert_eq!(eval("42 sa la p"), "42\n");
    }

    #[test]
    fn test_register_push_pop() {
        assert_eq!(eval("10 Sa 20 Sa La p"), "20\n");
    }

    #[test]
    fn test_register_stack() {
        assert_eq!(eval("10 Sa 20 Sa La p La p"), "20\n10\n");
    }

    #[test]
    fn an_unset_register_reads_as_zero() {
        assert_eq!(eval("lq p"), "0\n");
    }

    // ── Parameters ──

    #[test]
    fn test_ibase() {
        assert_eq!(eval("16 i FF p"), "255\n");
    }

    #[test]
    fn test_obase_hex() {
        assert_eq!(eval("16 o 255 p"), "FF\n");
    }

    #[test]
    fn test_obase_binary() {
        assert_eq!(eval("2 o 10 p"), "1010\n");
    }

    #[test]
    fn test_obase_octal() {
        assert_eq!(eval("8 o 255 p"), "377\n");
    }

    #[test]
    fn a_hex_fraction_is_sixteenths() {
        // .8 hex is a half, not eight tenths.
        assert_eq!(eval("16 i .8 p"), ".5\n");
        assert_eq!(eval("16 i A.8 p"), "10.5\n");
    }

    #[test]
    fn test_query_ibase() {
        assert_eq!(eval("I p"), "10\n");
    }

    #[test]
    fn test_query_obase() {
        assert_eq!(eval("O p"), "10\n");
    }

    #[test]
    fn test_query_precision() {
        assert_eq!(eval("K p"), "0\n");
    }

    #[test]
    fn an_out_of_range_base_is_refused_and_leaves_the_old_one() {
        assert!(eval_err("99 i").contains("input base"));
        // The base is unchanged, so `FF` is still read in base ten -- where
        // POSIX gives `F` the value 15 all the same, making this 15*10+15.
        assert_eq!(eval("99 i FF p"), "165\n");
        assert!(eval_err("1 o").contains("output base"));
        assert!(eval_err("_1 k").contains("scale"));
    }

    // ── Strings and macros ──

    #[test]
    fn test_string_push() {
        assert_eq!(eval("[hello] p"), "hello\n");
    }

    #[test]
    fn test_nested_string() {
        assert_eq!(eval("[[nested]] p"), "[nested]\n");
    }

    #[test]
    fn test_execute_string() {
        assert_eq!(eval("[3 5 +] x p"), "8\n");
    }

    #[test]
    fn a_tail_recursive_macro_loops_without_growing_the_stack() {
        // The standard dc loop. At 20000 iterations a version that recursed in
        // Rust would abort the process rather than answer.
        assert_eq!(eval("[1 + d 20000 >L] sL 0 lL x p"), "20000\n");
    }

    #[test]
    fn a_runaway_non_tail_recursion_is_reported_not_a_crash() {
        // `lR x` followed by another command cannot be a tail call, so this
        // nests for real -- and must hit the depth limit rather than the
        // process stack.
        let err = eval_err("[lR x 1] sR lR x");
        assert!(err.contains("too deep"), "unexpected diagnostic: {err}");
    }

    // ── Conditionals ──

    #[test]
    fn test_greater_than_true() {
        // `A B >r` runs r when B -- the top of the stack -- is the greater.
        // Both directions are asserted because a comparison that is merely
        // reversed still passes any test that only checks one of them.
        assert_eq!(eval("[10 p] sa 3 5 >a"), "10\n");
        assert_eq!(eval("[10 p] sa 5 3 >a"), "");
    }

    #[test]
    fn test_less_than_true() {
        assert_eq!(eval("[99 p] sa 5 3 <a"), "99\n");
        assert_eq!(eval("[99 p] sa 3 5 <a"), "");
    }

    #[test]
    fn test_equal_true() {
        assert_eq!(eval("[42 p] sa 5 5 =a"), "42\n");
    }

    #[test]
    fn test_not_equal() {
        assert_eq!(eval("[77 p] sa 3 5 !=a"), "77\n");
    }

    #[test]
    fn test_condition_false() {
        // A condition that does not hold runs nothing, and having consumed
        // both operands leaves the stack where it was.
        assert_eq!(eval("[99 p] sa 3 5 <a"), "");
        assert_eq!(eval("[99 p] sa 5 3 >a"), "");
        assert_eq!(eval("[99 p] sa 5 3 =a"), "");
        assert_eq!(eval("[99 p] sa 1 3 5 <a f"), "1\n");
    }

    #[test]
    fn a_comparison_of_equal_values_at_different_scales_holds() {
        // 1.5 and 1.50 are one number; the type's Ord says so.
        // `[same]` is pushed as a string and printed by the inner `p`; writing
        // it as `[same p]` would instead *run* `s`, `a`, `m`, `e` as commands.
        assert_eq!(eval("[[same] p] sa 1.5 1.50 =a"), "same\n");
    }

    // ── Divmod and modular exponentiation ──

    #[test]
    fn test_divmod() {
        assert_eq!(eval("17 5 ~ p r p"), "2\n3\n");
    }

    #[test]
    fn test_mod_pow_simple() {
        assert_eq!(eval("2 10 1000 | p"), "24\n");
    }

    #[test]
    fn modular_exponentiation_is_exact_far_past_a_double() {
        // (2^4096) mod (10^30 + 1), which no f64 could have attempted.
        let big = eval("2 4096 1000000000000000000000000000001 | p");
        assert_eq!(big.trim(), "977645768768699022168096304556");
    }

    #[test]
    fn a_modular_residue_is_never_negative() {
        assert_eq!(eval("_7 1 5 | p"), "3\n");
    }

    #[test]
    fn modular_exponentiation_refuses_what_it_cannot_answer() {
        assert!(eval_err("2 _1 5 |").contains("negative exponent"));
        assert!(eval_err("2 3 0 |").contains("zero"));
        assert!(eval_err("2.5 3 5 |").contains("integer"));
    }

    // ── Errors are reported and execution continues ──

    #[test]
    fn a_division_by_zero_is_reported_and_the_next_command_still_runs() {
        assert_eq!(eval("1 0 / 7 p"), "7\n");
        assert!(eval_err("1 0 /").contains("divide by zero"));
    }

    #[test]
    fn a_negative_square_root_is_reported_and_the_next_command_still_runs() {
        assert_eq!(eval("_4 v 7 p"), "7\n");
        assert!(eval_err("_4 v").contains("square root"));
    }

    #[test]
    fn an_empty_stack_is_reported_and_the_next_command_still_runs() {
        assert_eq!(eval("+ 7 p"), "7\n");
        assert!(eval_err("+").contains("stack empty"));
    }

    #[test]
    fn a_failed_binary_operator_puts_back_the_operand_it_popped() {
        // `5 +` has only one operand. It must not vanish.
        let stack = eval_stack("5 +");
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0].display(10, bignum::DEFAULT_LINE_LENGTH), b"5");
    }

    #[test]
    fn a_failed_register_command_does_not_run_its_register_name_as_a_command() {
        // `sp` on an empty stack fails. Resuming one character later would run
        // `p`, which would then report a second, spurious "stack empty".
        assert_eq!(eval_err("sp").matches("stack empty").count(), 1);
    }

    #[test]
    fn arithmetic_on_a_string_is_refused_by_name() {
        assert!(eval_err("[abc] 1 +").contains("not a number"));
    }

    // ── Z and X ──

    #[test]
    fn test_digit_count() {
        assert_eq!(eval("12345 Z p"), "5\n");
    }

    #[test]
    fn test_string_length() {
        assert_eq!(eval("[hello] Z p"), "5\n");
    }

    #[test]
    fn digit_count_includes_the_leading_zeros_of_a_fraction() {
        assert_eq!(eval("0.001 Z p"), "3\n");
        assert_eq!(eval("1.001 Z p"), "4\n");
    }

    #[test]
    fn scale_reports_the_fractional_places() {
        assert_eq!(eval("1.001 X p"), "3\n");
        assert_eq!(eval("5 X p"), "0\n");
    }

    // ── Quitting ──

    #[test]
    fn q_inside_a_macro_stops_the_program() {
        assert_eq!(eval("[1 p q 2 p] x 3 p"), "1\n");
    }

    #[test]
    fn capital_q_unwinds_exactly_the_levels_it_is_given() {
        // One level: the inner macro stops, the outer carries on.
        assert_eq!(eval("[[1 p 1 Q 2 p] x 3 p] x 4 p"), "1\n3\n4\n");
        // Two levels: both stop.
        assert_eq!(eval("[[1 p 2 Q 2 p] x 3 p] x 4 p"), "1\n4\n");
    }

    // ── Comments and whitespace ──

    #[test]
    fn test_comment() {
        assert_eq!(eval("5 # this is a comment\np"), "5\n");
    }

    #[test]
    fn test_empty_input() {
        assert_eq!(eval(""), "");
    }

    #[test]
    fn test_only_whitespace() {
        assert_eq!(eval("   \t\n  "), "");
    }

    #[test]
    fn test_zero() {
        assert_eq!(eval("0 p"), "0\n");
    }

    #[test]
    fn test_large_number() {
        assert_eq!(eval("999999999 1 + p"), "1000000000\n");
    }

    #[test]
    fn test_chained_operations() {
        assert_eq!(eval("2 3 + 4 * p"), "20\n");
    }

    #[test]
    fn test_factorial_like() {
        assert_eq!(eval("1 2 * 3 * 4 * 5 * p"), "120\n");
    }

    #[test]
    fn a_number_too_long_for_a_line_is_continued() {
        // End to end, through the real print path rather than the formatter:
        // 2^1000 is 302 digits, so four continued lines of 69 and a last of 26.
        let out = eval("2 1000 ^ p");
        let lines: Vec<&str> = out.trim_end().split('\n').collect();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].ends_with('\\'), "no continuation: {}", lines[0]);
        assert_eq!(lines[0].len(), 70);
        let rejoined: String = lines
            .iter()
            .map(|l| l.strip_suffix('\\').unwrap_or(l))
            .collect();
        assert_eq!(rejoined.len(), 302);
        assert!(rejoined.ends_with("069376"));
        // A string is printed as the program wrote it, never continued: a
        // backslash inserted into the user's own bytes would corrupt them.
        let long_string = "x".repeat(200);
        assert_eq!(
            eval(&format!("[{long_string}] p")),
            format!("{long_string}\n")
        );
    }

    #[test]
    fn a_number_becomes_the_byte_it_names_and_a_string_keeps_its_first() {
        // `a` is how a dc program emits text it computed: build the byte, then
        // print it. 72 is 'H', 105 is 'i'.
        assert_eq!(eval("72 a P 105 a P"), "Hi");
        // A string contributes its first byte and nothing else.
        assert_eq!(eval("[hello] a P"), "h");
        // Only the low-order byte of a number is taken, so 321 is 'A' (65).
        assert_eq!(eval("321 a P"), "A");
        // A fraction is truncated before the byte is taken, not rounded.
        assert_eq!(eval("65.9 a P"), "A");
        // A negative number contributes the byte its two's-complement
        // representation ends in: -321 is ...FEBF, so 0xBF.
        assert_eq!(eval_bytes(b"_321 a P"), vec![0xbf]);
        // -256 ends in a zero byte, which is a byte like any other.
        assert_eq!(eval_bytes(b"_256 a P"), vec![0x00]);
        // The result is a string, so `Z` answers one and `x` would run it.
        assert_eq!(eval("65 a Z p"), "1\n");
        // An empty string has no first byte; inventing one would put a byte on
        // the stack the program never made.
        assert_eq!(eval("[] a Z p"), "0\n");
    }

    #[test]
    fn a_byte_that_is_not_text_survives_being_built_and_printed() {
        // The reason `Value::Str` holds bytes rather than a `String`: 200 is
        // not a character in any encoding dc is entitled to assume, and
        // rendering it as UTF-8 would emit two bytes where dc emits one.
        assert_eq!(eval_bytes(b"200 a P"), vec![200u8]);
        assert_eq!(eval_bytes(b"255 a P"), vec![255u8]);
        // A source file may hold such a byte in a string literal, and it must
        // come back out unchanged rather than stopping the program.
        assert_eq!(eval_bytes(b"[\xc3\x28] P"), vec![0xc3, 0x28]);
        // And it is counted as the bytes it is.
        assert_eq!(eval_bytes(b"[\xc3\x28] Z p"), b"2\n".to_vec());
    }

    #[test]
    fn a_register_may_be_named_by_any_byte() {
        // There are 256 registers and every byte names one, including bytes
        // that are half of a character in some encoding.
        assert_eq!(eval_bytes(b"42 s\xff l\xff p"), b"42\n".to_vec());
    }

    #[test]
    fn a_factorial_macro_agrees_with_the_exact_value() {
        // 30! has 33 digits, so this is a whole-program check that nothing
        // along the way fell back to a double.
        assert_eq!(
            eval("[d 1 - d 1 <F *] sF 30 lF x p").trim(),
            "265252859812191058636308480000000"
        );
    }
}
