//! The input language: `eval.c` -- what each byte means, evaluating a
//! string (a macro, or `-e`'s expression), and evaluating a file or
//! standard input as it is read.
//!
//! A command that needs more than its own byte -- a register name, a
//! number's digits, a bracketed string, a shell line -- says so in its
//! [`Status`], and the two evaluators each read the rest their own way: the
//! string evaluator from the string, the file evaluator from its `FILE`,
//! one byte of lookahead ahead.

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::io::Input;
use crate::numeric::{self, int2data};
use crate::{Data, Dc, Verdict, show_id};

/// `DC_IBASE_MAX`: `I` is a command, and bases 17 and up are not read.
const IBASE_MAX: i32 = 16;

/// `interrupt_seen`: set by the `SIGINT` handler while a terminal is read.
static INTERRUPT_SEEN: AtomicBool = AtomicBool::new(false);

/// `dc_trap_interrupt`.
extern "C" fn trap_interrupt(_signo: i32) {
    INTERRUPT_SEEN.store(true, Ordering::Relaxed);
}

/// `dc_status`: what a command leaves its evaluator to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Nothing more.
    Okay,
    /// Eat the lookahead byte (a register's name).
    EatOne,
    /// Evaluate the register the lookahead byte names.
    EvalReg,
    /// Evaluate the value on top of the stack.
    EvalTos,
    /// Quit `unwind_depth` levels of evaluation.
    Quit,
    /// Read a number.
    Int,
    /// Read a bracketed string.
    Str,
    /// Run the rest of the line through the shell.
    System,
    /// Skip the rest of the line.
    Comment,
    /// The next comparison is negated (`!<`, `!=`, `!>`).
    NegCmp,
    /// The input ended where a command needed more.
    EofError,
    /// Standard input could not be read: upstream's `DC_FAIL`, returned from
    /// `evalstr` where a status was expected (it is `DC_EVALREG`'s number
    /// there, and no caller tells the two apart).
    Fail,
}

/// `skip_past_eol`: past the next newline, or to the end.
fn skip_past_eol(s: &[u8], from: usize) -> usize {
    let rest = s.get(from..).unwrap_or_default();
    match rest.iter().position(|&b| b == b'\n') {
        Some(nl) => from.saturating_add(nl).saturating_add(1),
        None => s.len(),
    }
}

/// `dc_readstring (fp, ldelim, rdelim)`: bytes up to the `rdelim` that
/// balances the `ldelim` already read (or the end), the delimiter eaten.
/// The caller checks the input's error flag, as `checkferror_input` does
/// inside upstream's.
fn read_string(input: &mut Input, ldelim: u8, rdelim: u8) -> Vec<u8> {
    let mut depth = 1u32;
    let mut out = Vec::new();
    while let Some(c) = input.getc() {
        if c == rdelim {
            depth = depth.saturating_sub(1);
            if depth < 1 {
                break;
            }
        } else if c == ldelim {
            depth = depth.saturating_add(1);
        }
        out.push(c);
    }
    out
}

/// The `FILE` being evaluated: standard input, or a file of its own.
fn input<'a>(stdin: &'a mut Input, file: &'a mut Option<Input>) -> &'a mut Input {
    match file {
        Some(f) => f,
        None => stdin,
    }
}

impl Dc {
    /// `checkferror_input (stdin)`.
    fn check_stdin(&mut self) {
        let e = self.stdin.error().then(|| self.stdin.errno());
        self.check_input(e);
    }

    /// `checkferror_input (fp)` for the `FILE` being evaluated.
    fn check_fp(&mut self, file: Option<&Input>) {
        let e = match file {
            Some(f) => f.error().then(|| f.errno()),
            None => self.stdin.error().then(|| self.stdin.errno()),
        };
        self.check_input(e);
    }

    /// A popped value as a C `int`, `fallback` for a string.
    fn int_of(&mut self, d: &Data, fallback: i32) -> i32 {
        match d {
            Data::Num(n) => self.num2int(n),
            Data::Str(_) => fallback,
        }
    }

    /// `dc_func (c, peekc, negcmp)`: command `c`, with the lookahead byte
    /// `peekc` (`None` at the end).
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's one switch, case for case"
    )]
    fn func(&mut self, c: u8, peekc: Option<u8>, negcmp: bool) -> Status {
        let scale = self.scale;
        match c {
            b'_' | b'.' | b'0'..=b'9' | b'A'..=b'F' => return Status::Int,
            b' ' | b'\t' | b'\n' => {}
            b'+' => self.binop(numeric::add, scale),
            b'-' => self.binop(numeric::sub, scale),
            b'*' => self.binop(numeric::mul, scale),
            b'/' => self.binop(numeric::div, scale),
            b'%' => self.binop(numeric::rem, scale),
            b'~' => self.binop2(numeric::divrem, scale),
            b'|' => self.triop(numeric::modexp, scale),
            b'^' => self.binop(numeric::exp, scale),
            b'<' | b'=' | b'>' => {
                if peekc.is_none() {
                    return Status::EofError;
                }
                let cmp = self.cmpop();
                let holds = match c {
                    b'<' => cmp < 0,
                    b'=' => cmp == 0,
                    _ => cmp > 0,
                };
                return if holds != negcmp {
                    Status::EvalReg
                } else {
                    Status::EatOne
                };
            }
            b'?' => {
                if let Some(la) = self.stdin_lookahead.take() {
                    self.stdin.ungetc(Some(la));
                }
                let line = read_string(&mut self.stdin, b'\n', b'\n');
                self.check_stdin();
                self.push(Data::string(&line));
                return Status::EvalTos;
            }
            b'[' => return Status::Str,
            b'!' => {
                return if matches!(peekc, Some(b'<' | b'=' | b'>')) {
                    Status::NegCmp
                } else {
                    Status::System
                };
            }
            b'#' => return Status::Comment,
            b'a' => {
                if let Some(d) = self.pop() {
                    let byte = match &d {
                        // `(char) dc_num2int (...)`: the low byte.
                        Data::Num(n) => self.num2int(n).to_le_bytes()[0],
                        // The first byte; an empty string's NUL.
                        Data::Str(s) => s.first().copied().unwrap_or(0),
                    };
                    self.push(Data::string(&[byte]));
                }
            }
            b'c' => self.clear_stack(),
            b'd' => {
                if let Some(d) = self.top() {
                    self.push(d);
                }
            }
            b'f' => self.print_all(),
            b'i' => {
                if let Some(d) = self.pop() {
                    let base = self.int_of(&d, 0);
                    if (2..=IBASE_MAX).contains(&base) {
                        self.ibase = base;
                    } else {
                        self.complain(
                            format!("input base must be a number between 2 and {IBASE_MAX} (inclusive)\n")
                                .as_bytes(),
                        );
                    }
                }
            }
            b'k' => {
                if let Some(d) = self.pop() {
                    let k = self.int_of(&d, -1);
                    if k >= 0 {
                        self.scale = k;
                    } else {
                        self.complain(b"scale must be a nonnegative number\n");
                    }
                }
            }
            b'l' => {
                let Some(reg) = peekc else {
                    return Status::EofError;
                };
                if let Some(d) = self.register_get(reg) {
                    self.push(d);
                }
                return Status::EatOne;
            }
            b'n' => {
                if let Some(d) = self.pop() {
                    self.print(&d, false);
                }
            }
            b'o' => {
                if let Some(d) = self.pop() {
                    let base = self.int_of(&d, 0);
                    if base > 1 {
                        self.obase = base;
                    } else {
                        self.complain(b"output base must be a number greater than 1\n");
                    }
                }
            }
            b'p' => {
                if let Some(d) = self.top() {
                    self.print(&d, true);
                }
            }
            b'q' => {
                self.unwind_depth = 1;
                self.unwind_noexit = false;
                return Status::Quit;
            }
            b'r' => self.rotate(2),
            b's' => {
                let Some(reg) = peekc else {
                    return Status::EofError;
                };
                if let Some(d) = self.pop() {
                    self.register_set(reg, d);
                }
                return Status::EatOne;
            }
            b'v' => {
                if let Some(d) = self.pop() {
                    match d {
                        Data::Num(n) => {
                            if let Some(root) = numeric::sqrt(self, &n, scale) {
                                self.push(Data::num(root));
                            }
                        }
                        Data::Str(_) => self.complain(b"square root of nonnumeric attempted\n"),
                    }
                }
            }
            b'x' => return Status::EvalTos,
            b'z' => {
                let depth = self.depth();
                self.push(int2data(depth));
            }
            b'I' => self.push(int2data(self.ibase)),
            b'K' => self.push(int2data(scale)),
            b'L' => {
                let Some(reg) = peekc else {
                    return Status::EofError;
                };
                if let Some(d) = self.register_pop(reg) {
                    self.push(d);
                }
                return Status::EatOne;
            }
            b'O' => self.push(int2data(self.obase)),
            b'P' => {
                if let Some(d) = self.pop() {
                    match &d {
                        Data::Num(n) => self.dump_num(n),
                        Data::Str(s) => {
                            self.out.write(s);
                            self.check_stdout();
                        }
                    }
                }
                self.out.flush();
                self.check_stdout();
            }
            b'Q' => {
                if let Some(d) = self.pop() {
                    self.unwind_depth = 0;
                    self.unwind_noexit = true;
                    if let Data::Num(n) = &d {
                        self.unwind_depth = self.num2int(n);
                    }
                    let was = self.unwind_depth;
                    self.unwind_depth = was.wrapping_sub(1);
                    if was > 0 {
                        return Status::Quit;
                    }
                    self.unwind_depth = 0;
                    self.complain(b"Q command requires a number >= 1\n");
                }
            }
            b'R' => {
                if let Some(d) = self.pop() {
                    let n = self.int_of(&d, 0);
                    self.rotate(n);
                }
            }
            b'S' => {
                let Some(reg) = peekc else {
                    return Status::EofError;
                };
                if let Some(d) = self.pop() {
                    self.register_push(reg, d);
                }
                return Status::EatOne;
            }
            b'X' => {
                if let Some(d) = self.pop() {
                    let k = match &d {
                        Data::Num(n) => i32::try_from(n.scale).unwrap_or(i32::MAX),
                        Data::Str(_) => 0,
                    };
                    self.push(int2data(k));
                }
            }
            b'Z' => {
                if let Some(d) = self.pop() {
                    self.push(int2data(Self::tell_length(&d)));
                }
            }
            b':' => {
                let Some(reg) = peekc else {
                    return Status::EofError;
                };
                if let Some(d) = self.pop() {
                    let index = self.int_of(&d, -1);
                    if let Some(value) = self.pop() {
                        if index < 0 {
                            self.complain(b"array index must be a nonnegative integer\n");
                        } else {
                            self.array_set(reg, index, value);
                        }
                    }
                }
                return Status::EatOne;
            }
            b';' => {
                let Some(reg) = peekc else {
                    return Status::EofError;
                };
                if let Some(d) = self.pop() {
                    let index = self.int_of(&d, -1);
                    if index < 0 {
                        self.complain(b"array index must be a nonnegative integer\n");
                    } else {
                        let v = self.array_get(reg, index);
                        self.push(v);
                    }
                }
                return Status::EatOne;
            }
            _ => {
                // "dc: " to standard error, the rest to standard output --
                // unflushed, as upstream leaves it.
                self.say(b"");
                self.check_stderr();
                self.out.write(&show_id(c, b" unimplemented\n"));
                self.check_stdout();
                self.check_stdout();
            }
        }
        Status::Okay
    }

    /// `dc_system (s)`: the shell run on the string from `from` to its next
    /// newline -- or to its first NUL, where upstream's C string ends -- and
    /// the position after what was used.
    fn system_from(&mut self, s: &[u8], from: usize) -> usize {
        let rest = s.get(from..).unwrap_or_default();
        let nul = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let line = rest.get(..nul).unwrap_or_default();
        match line.iter().position(|&b| b == b'\n') {
            Some(nl) => {
                self.shell.system(line.get(..nl).unwrap_or_default());
                from.saturating_add(nl).saturating_add(1)
            }
            None => {
                self.shell.system(line);
                from.saturating_add(nul)
            }
        }
    }

    /// `evalstr (string)`: evaluate a string, the last macro of a string
    /// replacing it rather than nesting (tail recursion). A value that is
    /// not a string is complained of.
    #[allow(clippy::too_many_lines, reason = "upstream's one loop, case for case")]
    fn evalstr(&mut self, string: Data) -> Status {
        let Data::Str(mut s) = string else {
            self.complain(b"eval called with non-string argument\n");
            return Status::Okay;
        };
        INTERRUPT_SEEN.store(false, Ordering::Relaxed);
        let mut i = 0usize;
        let mut next_negcmp = false;
        let mut tail_depth: i32 = 1;
        while i < s.len() && !INTERRUPT_SEEN.load(Ordering::Relaxed) {
            let Some(&c) = s.get(i) else {
                break;
            };
            i = i.saturating_add(1);
            let peekc = s.get(i).copied();
            let negcmp = next_negcmp;
            next_negcmp = false;
            let status = self.func(c, peekc, negcmp);
            match status {
                Status::Okay => {}
                Status::EatOne => {
                    if peekc.is_some() {
                        i = i.saturating_add(1);
                    }
                }
                Status::EvalReg | Status::EvalTos => {
                    if status == Status::EvalReg {
                        i = i.saturating_add(1);
                        let Some(reg) = peekc else {
                            continue;
                        };
                        let Some(v) = self.register_get(reg) else {
                            continue;
                        };
                        self.push(v);
                    }
                    // Trailing blanks and comments skipped, so that a macro
                    // that ends by evaluating another is seen to end there.
                    while let Some(&b) = s.get(i) {
                        if !matches!(b, b' ' | b'\t' | b'\n' | b'#') {
                            break;
                        }
                        i = i.saturating_add(1);
                        if b == b'#' {
                            i = skip_past_eol(&s, i);
                        }
                    }
                    if let Some(v) = self.pop() {
                        match v {
                            Data::Num(_) => self.push(v),
                            Data::Str(next) => {
                                if i == s.len() {
                                    s = next;
                                    i = 0;
                                    tail_depth = tail_depth.saturating_add(1);
                                } else if self.evalstr(Data::Str(next)) == Status::Quit {
                                    if self.unwind_depth > 0 {
                                        self.unwind_depth = self.unwind_depth.saturating_sub(1);
                                        return Status::Quit;
                                    }
                                    return Status::Okay;
                                }
                            }
                        }
                    }
                }
                Status::Quit => {
                    if self.unwind_depth >= tail_depth {
                        self.unwind_depth = self.unwind_depth.saturating_sub(tail_depth);
                        return Status::Quit;
                    }
                    tail_depth = tail_depth.saturating_sub(self.unwind_depth);
                }
                Status::Int => {
                    // Upstream reads the C string, which ends at a NUL.
                    let mut j = i.saturating_sub(1);
                    let ibase = self.ibase;
                    let text = Rc::clone(&s);
                    let (n, after) = numeric::getnum(
                        &mut || {
                            let b = text.get(j).copied().filter(|&b| b != 0)?;
                            j = j.saturating_add(1);
                            Some(b)
                        },
                        ibase,
                    );
                    self.push(Data::num(n));
                    i = if after.is_some() {
                        j.saturating_sub(1)
                    } else {
                        j
                    };
                }
                Status::Str => {
                    let mut count = 1i32;
                    let mut p = i;
                    while count > 0 {
                        let Some(&b) = s.get(p) else {
                            break;
                        };
                        if b == b']' {
                            count = count.saturating_sub(1);
                        } else if b == b'[' {
                            count = count.saturating_add(1);
                        }
                        p = p.saturating_add(1);
                    }
                    let len = p.saturating_sub(i);
                    let len = if count == 0 {
                        len.saturating_sub(1)
                    } else {
                        len
                    };
                    let body = s.get(i..i.saturating_add(len)).unwrap_or_default();
                    self.push(Data::string(body));
                    i = p;
                }
                Status::System => {
                    let text = Rc::clone(&s);
                    i = self.system_from(&text, i);
                }
                Status::Comment => i = skip_past_eol(&s, i),
                Status::NegCmp => next_negcmp = true,
                Status::EofError => {
                    if self.stdin.error() {
                        let e = self.stdin.errno();
                        self.say(b"");
                        self.perror(b"error reading stdin", e);
                        return Status::Fail;
                    }
                    self.complain(b"unexpected EOS\n");
                    return Status::Okay;
                }
                Status::Fail => {}
            }
        }
        Status::Okay
    }

    /// `dc_evalstr (string)`: evaluate `-e`'s expression; a quit fails
    /// unless it was `Q`'s.
    pub fn eval_string(&mut self, text: &[u8]) -> Verdict {
        match self.evalstr(Data::string(text)) {
            Status::Okay => Verdict::Success,
            Status::Quit if self.unwind_noexit => Verdict::Success,
            _ => Verdict::Fail,
        }
    }

    /// `dc_evalfile (fp)`: evaluate a file as it is read -- `file`, or
    /// standard input when `None`.
    #[allow(clippy::too_many_lines, reason = "upstream's one loop, case for case")]
    pub fn eval_file(&mut self, mut file: Option<Input>) -> Verdict {
        let is_stdin = file.is_none();
        // Interrupts are caught only while a terminal is read; otherwise
        // SIGINT keeps whatever it was given.
        let tty = input(&mut self.stdin, &mut file)
            .fd()
            .is_some_and(libcall::fd::is_terminal);
        let default = libcall::signal::save_action(libcall::signal::SIGINT).ok();
        let restore = |d: &Option<libcall::signal::SavedAction>| {
            if let Some(d) = d {
                // Putting back what was there: a failure leaves the handler
                // in place, which only sets a flag nothing reads after this.
                let _ = libcall::signal::restore_action(libcall::signal::SIGINT, d);
            }
        };
        self.stdin_lookahead = None;
        let mut next_negcmp = false;
        let mut c = input(&mut self.stdin, &mut file).getc();
        while let Some(ch) = c {
            let mut peekc = input(&mut self.stdin, &mut file).getc();
            self.check_stdin();
            if is_stdin {
                self.stdin_lookahead = peekc;
            }
            let negcmp = next_negcmp;
            next_negcmp = false;
            if tty {
                // A handler that cannot be set leaves interrupts as they were.
                let _ = libcall::signal::set_handler(libcall::signal::SIGINT, trap_interrupt, true);
            }
            let status = self.func(ch, peekc, negcmp);
            match status {
                Status::Okay => {
                    if self.stdin_lookahead != peekc && is_stdin {
                        peekc = self.stdin.getc();
                        self.check_stdin();
                    }
                }
                Status::EatOne => {
                    peekc = input(&mut self.stdin, &mut file).getc();
                    self.check_fp(file.as_ref());
                }
                Status::EvalReg | Status::EvalTos => 'eval: {
                    if status == Status::EvalReg {
                        let Some(reg) = peekc else {
                            break 'eval;
                        };
                        peekc = input(&mut self.stdin, &mut file).getc();
                        self.check_fp(file.as_ref());
                        self.stdin_lookahead = peekc;
                        let Some(v) = self.register_get(reg) else {
                            break 'eval;
                        };
                        self.push(v);
                    }
                    if self.stdin_lookahead != peekc && is_stdin {
                        peekc = self.stdin.getc();
                        self.check_stdin();
                    }
                    if let Some(v) = self.pop() {
                        match v {
                            Data::Num(_) => self.push(v),
                            Data::Str(_) => {
                                if self.evalstr(v) == Status::Quit {
                                    if !self.unwind_noexit {
                                        restore(&default);
                                        return Verdict::Fail;
                                    }
                                    self.complain(
                                        b"Q command argument exceeded string execution depth\n",
                                    );
                                }
                            }
                        }
                    }
                }
                Status::Quit => {
                    if !self.unwind_noexit {
                        restore(&default);
                        return Verdict::Fail;
                    }
                    self.complain(b"Q command argument exceeded string execution depth\n");
                    if self.stdin_lookahead != peekc && is_stdin {
                        peekc = self.stdin.getc();
                        self.check_stdin();
                    }
                }
                Status::Int => {
                    let ibase = self.ibase;
                    let fp = input(&mut self.stdin, &mut file);
                    fp.ungetc(peekc);
                    let mut first = Some(ch);
                    let (n, after) = numeric::getnum(
                        &mut || {
                            if let Some(b) = first.take() {
                                return Some(b);
                            }
                            // Upstream's `input_fil` checks the error after
                            // each byte and exits; nothing it does in
                            // between shows, so it is checked once below.
                            fp.getc()
                        },
                        ibase,
                    );
                    self.push(Data::num(n));
                    peekc = after;
                    self.check_fp(file.as_ref());
                }
                Status::Str => {
                    let fp = input(&mut self.stdin, &mut file);
                    fp.ungetc(peekc);
                    let body = read_string(fp, b'[', b']');
                    self.check_fp(file.as_ref());
                    self.push(Data::string(&body));
                    peekc = input(&mut self.stdin, &mut file).getc();
                }
                Status::System => {
                    let fp = input(&mut self.stdin, &mut file);
                    fp.ungetc(peekc);
                    let line = read_string(fp, b'\n', b'\n');
                    self.check_fp(file.as_ref());
                    let nul = line.iter().position(|&b| b == 0).unwrap_or(line.len());
                    self.shell.system(line.get(..nul).unwrap_or_default());
                    peekc = input(&mut self.stdin, &mut file).getc();
                }
                Status::Comment => {
                    let fp = input(&mut self.stdin, &mut file);
                    while peekc.is_some() && peekc != Some(b'\n') {
                        peekc = fp.getc();
                    }
                    if peekc.is_some() {
                        peekc = fp.getc();
                    }
                }
                Status::NegCmp => next_negcmp = true,
                Status::EofError => {
                    let fp = input(&mut self.stdin, &mut file);
                    if fp.error() {
                        let e = fp.errno();
                        self.say(b"");
                        self.perror(b"error reading input", e);
                        return Verdict::Fail;
                    }
                    self.say(b"unexpected EOF\n");
                    restore(&default);
                    return Verdict::Fail;
                }
                Status::Fail => {}
            }
            if INTERRUPT_SEEN.load(Ordering::Relaxed) {
                self.err.write(b"\nInterrupt!\n");
            }
            INTERRUPT_SEEN.store(false, Ordering::Relaxed);
            if tty {
                restore(&default);
            }
            c = peekc;
        }
        let fp = input(&mut self.stdin, &mut file);
        if fp.error() {
            let e = fp.errno();
            self.say(b"");
            self.perror(b"error reading input", e);
            return Verdict::Fail;
        }
        restore(&default);
        Verdict::Success
    }
}
