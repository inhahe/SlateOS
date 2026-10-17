//! The machine: `execute.c`, and `storage.c`'s variables, arrays, stacks
//! and parameters.
//!
//! The execution stack, the function stack and every variable outlive a
//! run, as upstream's globals do; a run ends by unwinding the functions it
//! was in and emptying the execution stack.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use bcnumber::{Diag, Num};

use crate::{Arg, ArrayLevel, BC_DIM_MAX, BC_INT_MAX, Bc};

/// `had_sigint`: an interrupt arrived while code ran.
static HAD_SIGINT: AtomicBool = AtomicBool::new(false);

/// `SIGSEGV`, which upstream dies of where a parameter list runs on past a
/// mismatch.
const SIGSEGV: i32 = 11;

/// `stop_execution`: the interrupt handler while code runs.
extern "C" fn stop_execution(_sig: i32) {
    HAD_SIGINT.store(true, Ordering::SeqCst);
}

/// `use_quit` as Debian builds it (`DONTEXIT`): the interrupt handler
/// between runs, which only says how to leave.
pub extern "C" fn use_quit(_sig: i32) {
    // Nothing can be done in a handler about a failed write.
    let _ = libcall::fd::write(1, b"\n(interrupt) use quit to exit.\n");
}

/// Install `handler` for `SIGINT`, as `signal` does.
pub fn on_interrupt(handler: libcall::signal::Handler) {
    // `signal`'s result is not looked at upstream either: without the
    // handler an interrupt ends the program, which is the default.
    let _ = libcall::signal::set_handler(libcall::signal::SIGINT, handler, true);
}

/// Where `push_constant` reads a number's characters.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CharSource {
    /// `prog_char`: the code being run.
    Program,
    /// `input_char`: standard input, for `read()`.
    Input,
}

/// A parameter list being matched to a call's arguments.
enum Params {
    /// The parameters still to match, by index into the list.
    At(usize),
    /// Past a mismatch: upstream steps its list pointer one node along in
    /// memory (`params++`) and follows that node's link, which lands on no
    /// parameter but is not null.
    Lost,
}

impl Diag for Bc {
    fn warn(&mut self, msg: &str) {
        self.rt_warn(msg.as_bytes());
    }

    fn error(&mut self, msg: &str) {
        self.rt_error(msg.as_bytes());
    }
}

impl Bc {
    /// `byte (&pc)`: the next byte of the code being run; past every byte
    /// ever written there, 0.
    fn byte(&mut self) -> u8 {
        let b = self
            .functions
            .get(self.pc_func)
            .and_then(|f| f.body.get(self.pc_addr))
            .copied()
            .unwrap_or(0);
        self.pc_addr = self.pc_addr.saturating_add(1);
        b
    }

    /// A variable's, array's or function's number in the code.
    fn number(&mut self) -> usize {
        let first = self.byte();
        if first & 0x80 == 0 {
            usize::from(first)
        } else {
            usize::from(u16::from_be_bytes([first & 0x7f, self.byte()]))
        }
    }

    /// The top of the execution stack (zero for none, which code that
    /// compiled never meets).
    fn top(&self) -> Num {
        self.ex_stack.last().cloned().unwrap_or_else(Num::zero)
    }

    /// The value under the top.
    fn second(&self) -> Num {
        let n = self.ex_stack.len();
        n.checked_sub(2)
            .and_then(|i| self.ex_stack.get(i))
            .cloned()
            .unwrap_or_else(Num::zero)
    }

    /// The top replaced by `num`.
    fn set_top(&mut self, num: Num) {
        if let Some(top) = self.ex_stack.last_mut() {
            *top = num;
        }
    }

    /// `pop ()`.
    fn pop(&mut self) {
        self.ex_stack.pop();
    }

    /// `check_stack (depth)`: at least `depth` values, or a runtime error.
    fn check_stack(&mut self, depth: usize) -> bool {
        if self.ex_stack.len() < depth {
            self.rt_error(b"Stack error.");
            return false;
        }
        true
    }

    /// `assign (code)`: the top replaced by 1 or 0.
    fn assign(&mut self, code: bool) {
        self.set_top(if code { Num::one() } else { Num::zero() });
    }

    /// `fpop ()`.
    fn fpop(&mut self) -> i32 {
        if let Some(v) = self.fn_stack.pop() {
            v
        } else {
            self.rt_error(b"function stack underflow, contact maintainer.");
            0
        }
    }

    /// Two operands for a binary operation, popped, the result pushed.
    fn binary(&mut self, op: impl FnOnce(&mut Self, &Num, &Num) -> Num) {
        if self.check_stack(2) {
            let (a, b) = (self.second(), self.top());
            let r = op(self, &a, &b);
            self.pop();
            self.pop();
            self.ex_stack.push(r);
        }
    }

    /// A comparison of the two operands, replacing them with 1 or 0.
    fn compare(&mut self, test: impl FnOnce(i32) -> bool) {
        if self.check_stack(2) {
            let c = test(bcnumber::compare(&self.second(), &self.top()));
            self.c_code = c;
            self.pop();
            self.assign(c);
        }
    }

    /// `execute ()`: the main program loaded, run.
    #[allow(clippy::too_many_lines, reason = "execute's one switch")]
    pub(crate) fn execute(&mut self) {
        self.pc_func = 0;
        self.pc_addr = 0;
        self.runtime_error = false;
        if self.interactive {
            on_interrupt(stop_execution);
        }
        HAD_SIGINT.store(false, Ordering::SeqCst);
        loop {
            let size = self.functions.get(self.pc_func).map_or(0, |f| f.code_size);
            if self.pc_addr >= size || self.runtime_error || HAD_SIGINT.load(Ordering::SeqCst) {
                break;
            }
            let inst = self.byte();
            match inst {
                b'A' => {
                    let v = self.number();
                    self.incr_array(v);
                }
                b'B' | b'Z' | b'J' => {
                    if inst != b'J' {
                        self.c_code = !bcnumber::is_zero(&self.top());
                        self.pop();
                    }
                    // Low byte first.
                    let lo = self.byte();
                    let label = usize::from(u16::from_le_bytes([lo, self.byte()]));
                    if inst == b'J'
                        || (inst == b'B' && self.c_code)
                        || (inst == b'Z' && !self.c_code)
                    {
                        let target = self
                            .functions
                            .get(self.pc_func)
                            .and_then(|f| f.labels.get(label / 64))
                            .and_then(|g| g.get(label % 64).copied().flatten());
                        match target {
                            Some(addr) => self.pc_addr = addr,
                            None => self.rt_error(b"Internal error."),
                        }
                    }
                }
                b'C' => {
                    let new_func = self.number();
                    if !self.functions.get(new_func).is_some_and(|f| f.defined) {
                        let mut msg = b"Function ".to_vec();
                        msg.extend_from_slice(&self.f_name(new_func));
                        msg.extend_from_slice(b" not defined.");
                        self.rt_error(&msg);
                        continue;
                    }
                    self.process_params(new_func);
                    let autos = self
                        .functions
                        .get(new_func)
                        .map(|f| f.autos.clone())
                        .unwrap_or_default();
                    for a in &autos {
                        self.auto_var(a.name);
                    }
                    self.fn_stack.push(i32::try_from(self.pc_func).unwrap_or(0));
                    self.fn_stack.push(i32::try_from(self.pc_addr).unwrap_or(0));
                    self.fn_stack.push(self.i_base);
                    self.pc_func = new_func;
                    self.pc_addr = 0;
                }
                b'D' => {
                    let t = self.top();
                    self.ex_stack.push(t);
                }
                b'K' => {
                    let base = if self.pc_func == 0 {
                        self.i_base
                    } else {
                        self.fn_stack.last().copied().unwrap_or(10)
                    };
                    if base == 10 {
                        self.push_b10_const();
                    } else {
                        self.push_constant(CharSource::Program, base);
                    }
                }
                b'L' => {
                    let v = self.number();
                    self.load_array(v);
                }
                b'M' => {
                    let v = self.number();
                    self.decr_array(v);
                }
                b'O' => {
                    loop {
                        let ch = self.byte();
                        if ch == b'"' {
                            break;
                        }
                        if ch != b'\\' {
                            self.out_schar(ch);
                            continue;
                        }
                        let ch = self.byte();
                        if ch == b'"' {
                            break;
                        }
                        let out = match ch {
                            b'a' => Some(7),
                            b'b' => Some(8),
                            b'f' => Some(12),
                            b'n' => Some(b'\n'),
                            b'q' => Some(b'"'),
                            b'r' => Some(b'\r'),
                            b't' => Some(b'\t'),
                            b'\\' => Some(b'\\'),
                            _ => None,
                        };
                        if let Some(c) = out {
                            self.out_schar(c);
                        }
                    }
                    self.out.flush();
                    self.check_stdout();
                }
                b'R' => {
                    if self.pc_func == 0 {
                        self.rt_error(b"Return from main program.");
                    } else {
                        self.return_from_function();
                    }
                }
                b'S' => {
                    let v = self.number();
                    self.store_array(v);
                }
                b'T' | b'!' => {
                    self.c_code = bcnumber::is_zero(&self.top());
                    self.assign(self.c_code);
                }
                b'W' | b'P' => {
                    let t = self.top();
                    let leading_zero = self.std_only;
                    let o_base = self.o_base;
                    bcnumber::out_num(&t, o_base, &mut |c| self.out_char(c), leading_zero);
                    if inst == b'W' {
                        self.out_char(b'\n');
                    }
                    self.store_var(4);
                    self.out.flush();
                    self.check_stdout();
                    self.pop();
                }
                b'c' => match self.byte() {
                    b'L' => {
                        let t = self.top();
                        let digits = if t.len == 1 && t.scale != 0 && t.v.first() == Some(&0) {
                            t.scale
                        } else {
                            t.len.saturating_add(t.scale)
                        };
                        self.set_top(bcnumber::int2num(int_of(digits)));
                    }
                    b'S' => {
                        let t = self.top();
                        self.set_top(bcnumber::int2num(int_of(t.scale)));
                    }
                    b'R' => {
                        let t = self.top();
                        let scale = usize::try_from(self.scale).unwrap_or(0);
                        match bcnumber::sqrt(&t, scale, self) {
                            Some(r) => self.set_top(r),
                            None => self.rt_error(b"Square root of a negative number"),
                        }
                    }
                    b'I' => self.push_constant(CharSource::Input, self.i_base),
                    b'X' => {
                        let r = self.random.next();
                        self.ex_stack.push(bcnumber::int2num(r));
                    }
                    _ => {}
                },
                b'd' => {
                    let v = self.number();
                    self.decr_var(v);
                }
                b'h' => self.exit(0),
                b'i' => {
                    let v = self.number();
                    self.incr_var(v);
                }
                b'l' => {
                    let v = self.number();
                    self.load_var(v);
                }
                b'n' => {
                    let t = self.top();
                    self.set_top(bcnumber::sub(&Num::zero(), &t, 0));
                }
                b'p' => self.pop(),
                b's' => {
                    let v = self.number();
                    self.store_var(v);
                }
                b'w' => {
                    loop {
                        let ch = self.byte();
                        if ch == b'"' {
                            break;
                        }
                        self.out_schar(ch);
                    }
                    self.out.flush();
                    self.check_stdout();
                }
                b'x' => {
                    if self.check_stack(2) {
                        let n = self.ex_stack.len();
                        self.ex_stack.swap(n.saturating_sub(2), n.saturating_sub(1));
                    }
                }
                b'0' => self.ex_stack.push(Num::zero()),
                b'1' => self.ex_stack.push(Num::one()),
                b'&' => {
                    if self.check_stack(2) {
                        let c =
                            !bcnumber::is_zero(&self.second()) && !bcnumber::is_zero(&self.top());
                        self.c_code = c;
                        self.pop();
                        self.assign(c);
                    }
                }
                b'|' => {
                    if self.check_stack(2) {
                        let c =
                            !bcnumber::is_zero(&self.second()) || !bcnumber::is_zero(&self.top());
                        self.c_code = c;
                        self.pop();
                        self.assign(c);
                    }
                }
                b'+' => self.binary(|_, a, b| bcnumber::add(a, b, 0)),
                b'-' => self.binary(|_, a, b| bcnumber::sub(a, b, 0)),
                b'*' => {
                    let scale = i64::from(self.scale);
                    self.binary(|_, a, b| bcnumber::multiply(a, b, scale));
                }
                b'/' => {
                    if self.check_stack(2) {
                        let scale = usize::try_from(self.scale).unwrap_or(0);
                        match bcnumber::divide(&self.second(), &self.top(), scale) {
                            Some(q) => {
                                self.pop();
                                self.pop();
                                self.ex_stack.push(q);
                            }
                            None => self.rt_error(b"Divide by zero"),
                        }
                    }
                }
                b'%' => {
                    if self.check_stack(2) {
                        if bcnumber::is_zero(&self.top()) {
                            self.rt_error(b"Modulo by zero");
                        } else {
                            let scale = usize::try_from(self.scale).unwrap_or(0);
                            let r = bcnumber::modulo(&self.second(), &self.top(), scale)
                                .unwrap_or_else(Num::zero);
                            self.pop();
                            self.pop();
                            self.ex_stack.push(r);
                        }
                    }
                }
                b'^' => {
                    if self.check_stack(2) {
                        let (a, b) = (self.second(), self.top());
                        let scale = i64::from(self.scale);
                        // `temp_num` holds zero between operations.
                        let r = bcnumber::raise(&a, &b, Num::zero(), scale, self);
                        if bcnumber::is_zero(&a) && bcnumber::is_neg(&b) {
                            self.rt_error(b"divide by zero");
                        }
                        self.pop();
                        self.pop();
                        self.ex_stack.push(r);
                    }
                }
                b'=' => self.compare(|c| c == 0),
                b'#' => self.compare(|c| c != 0),
                b'<' => self.compare(|c| c == -1),
                b'{' => self.compare(|c| c <= 0),
                b'>' => self.compare(|c| c == 1),
                b'}' => self.compare(|c| c >= 0),
                _ => {
                    let mut msg = b"bad instruction: inst=".to_vec();
                    msg.push(inst);
                    self.rt_error(&msg);
                }
            }
        }

        // Out of every function the run was in.
        while self.pc_func != 0 {
            self.return_from_function();
        }
        self.ex_stack.clear();

        if self.interactive {
            on_interrupt(use_quit);
            if HAD_SIGINT.load(Ordering::SeqCst) {
                self.out.write(b"\ninterrupted execution.\n");
                self.check_stdout();
            }
        }
    }

    /// The `R` instruction's work: the function's autos and parameters
    /// popped, and back to its caller.
    fn return_from_function(&mut self) {
        let (autos, params) = self
            .functions
            .get(self.pc_func)
            .map(|f| (f.autos.clone(), f.params.clone()))
            .unwrap_or_default();
        self.pop_vars(&autos);
        self.pop_vars(&params);
        self.fpop();
        self.pc_addr = usize::try_from(self.fpop()).unwrap_or(0);
        self.pc_func = usize::try_from(self.fpop()).unwrap_or(0);
    }

    /// `input_char ()`: a character of standard input, classified as
    /// `push_constant` wants it -- a digit's value, `.`, `+` or `-`, `~`
    /// for a blank or the end, `:` for anything else.
    fn input_char(&mut self) -> i32 {
        let mut c = self.stdin.getc();
        if c == Some(b'\\') {
            c = self.stdin.getc();
            if c == Some(b'\n') {
                c = self.stdin.getc();
                self.out_col = 0;
            }
        }
        self.check_stdin();
        let Some(c) = c else {
            return i32::from(b'~');
        };
        // `isdigit` and the rest, on a `char` read as an `int` (0-255).
        match c {
            b'0'..=b'9' => i32::from(c.wrapping_sub(b'0')),
            b'A'..=b'Z' => i32::from(c.wrapping_sub(b'A').wrapping_add(10)),
            b'a'..=b'z' => i32::from(c.wrapping_sub(b'a').wrapping_add(10)),
            b'.' | b'+' | b'-' => i32::from(c),
            b'~' => i32::from(b':'),
            _ if c <= b' ' => i32::from(b'~'),
            _ => i32::from(b':'),
        }
    }

    /// The next character for `push_constant`.
    fn in_char(&mut self, from: CharSource) -> i32 {
        match from {
            CharSource::Program => i32::from(self.byte()),
            CharSource::Input => self.input_char(),
        }
    }

    /// `push_constant (in_char, conv_base)`: a number read in base
    /// `conv_base`, pushed. A digit too big for the base counts as the
    /// biggest there is -- except a lone digit, which counts as itself --
    /// and the first character after the point is a digit whatever it is.
    fn push_constant(&mut self, from: CharSource, conv_base: i32) {
        let mult = bcnumber::int2num(conv_base);
        let mut build = Num::zero();
        let mut negative = false;
        let tilde = i32::from(b'~');
        let mut in_ch = self.in_char(from);
        while in_ch == tilde {
            in_ch = self.in_char(from);
        }
        if in_ch == i32::from(b'+') {
            in_ch = self.in_char(from);
        } else if in_ch == i32::from(b'-') {
            negative = true;
            in_ch = self.in_char(from);
        }
        if in_ch < 36 {
            let mut first = in_ch;
            in_ch = self.in_char(from);
            if in_ch < 36 && first >= conv_base {
                first = conv_base.saturating_sub(1);
            }
            build = bcnumber::int2num(first);
        }
        while in_ch < 36 {
            if in_ch >= conv_base {
                in_ch = conv_base.saturating_sub(1);
            }
            let result = bcnumber::multiply(&build, &mult, 0);
            build = bcnumber::add(&result, &bcnumber::int2num(in_ch), 0);
            in_ch = self.in_char(from);
        }
        if in_ch == i32::from(b'.') {
            in_ch = self.in_char(from);
            if in_ch >= conv_base {
                in_ch = conv_base.saturating_sub(1);
            }
            let mut divisor = Num::one();
            let mut result = Num::zero();
            let mut digits = 0usize;
            while in_ch < 36 {
                result = bcnumber::multiply(&result, &mult, 0);
                result = bcnumber::add(&result, &bcnumber::int2num(in_ch), 0);
                divisor = bcnumber::multiply(&divisor, &mult, 0);
                digits = digits.saturating_add(1);
                in_ch = self.in_char(from);
                if in_ch < 36 && in_ch >= conv_base {
                    in_ch = conv_base.saturating_sub(1);
                }
            }
            // The divisor is a power of the base, never zero.
            let fraction = bcnumber::divide(&result, &divisor, digits).unwrap_or(result);
            build = bcnumber::add(&build, &fraction, 0);
        }
        if negative {
            build = bcnumber::sub(&Num::zero(), &build, 0);
        }
        self.ex_stack.push(build);
    }

    /// `push_b10_const (&pc)`: a base-10 constant from the code, pushed --
    /// each digit as it stands, a letter among several as 9, a lone letter
    /// as its value.
    fn push_b10_const(&mut self) {
        let start = self.pc_addr;
        let mut kdigits = 0usize;
        let mut kscale = 0usize;
        let mut c = self.byte();
        while c != b'.' && c != b':' {
            kdigits = kdigits.saturating_add(1);
            c = self.byte();
        }
        if c == b'.' {
            c = self.byte();
            while c != b':' {
                kscale = kscale.saturating_add(1);
                c = self.byte();
            }
        }
        self.pc_addr = start;
        let mut c = self.byte();
        if kdigits == 1 && kscale == 0 {
            if c == 0 || c == 1 || c > 9 {
                self.ex_stack.push(bcnumber::int2num(i32::from(c)));
                self.byte();
                return;
            }
        }
        let mut build = Num::new(kdigits.max(1), kscale);
        let mut at = usize::from(kdigits == 0);
        while c != b':' {
            if c != b'.' {
                if let Some(d) = build.v.get_mut(at) {
                    *d = c.min(9);
                }
                at = at.saturating_add(1);
            }
            c = self.byte();
        }
        self.ex_stack.push(build);
    }

    // ---- storage.c

    /// `get_array_num (var_index, idx)`'s array: the top level of array
    /// `a`'s stack, made (with its elements) if there is none.
    fn array_elements(&mut self, a: usize) -> Rc<RefCell<BTreeMap<u32, Num>>> {
        if self.arrays.len() <= a {
            self.arrays.resize_with(a.saturating_add(1), Vec::new);
        }
        let Some(levels) = self.arrays.get_mut(a) else {
            return Rc::default();
        };
        if levels.is_empty() {
            levels.push(ArrayLevel {
                value: None,
                param: false,
            });
        }
        match levels.last_mut() {
            Some(top) => Rc::clone(top.value.get_or_insert_with(Rc::default)),
            None => Rc::default(),
        }
    }

    /// The subscript on top of the stack, if it is one: 0 to `BC_DIM_MAX`,
    /// integer part taken. Otherwise the out-of-bounds error.
    fn subscript(&mut self, a: usize, index: &Num) -> Option<u32> {
        let idx = bcnumber::num2long(index);
        if idx < 0 || idx > BC_DIM_MAX || (idx == 0 && !bcnumber::is_zero(index)) {
            let mut msg = b"Array ".to_vec();
            msg.extend_from_slice(
                &self
                    .a_names
                    .get(a)
                    .cloned()
                    .flatten()
                    .unwrap_or_else(|| b"(null)".to_vec()),
            );
            msg.extend_from_slice(b" subscript out of bounds.");
            self.rt_error(&msg);
            return None;
        }
        u32::try_from(idx).ok()
    }

    /// `store_var (var_name)`: the top of the stack into a variable, or
    /// into `ibase`, `obase`, `scale` or `history`, within their limits.
    #[allow(clippy::too_many_lines, reason = "store_var's cases")]
    fn store_var(&mut self, var: usize) {
        let top = self.top();
        if var > 3 {
            self.variable_value(var, top);
            return;
        }
        let (temp, toobig): (i64, bool) = if bcnumber::is_neg(&top) {
            let set_to = match var {
                0 => {
                    self.rt_warn(b"negative ibase, set to 2");
                    2
                }
                1 => {
                    self.rt_warn(b"negative obase, set to 2");
                    2
                }
                2 => {
                    self.rt_warn(b"negative scale, set to 0");
                    0
                }
                _ => -1,
            };
            (set_to, false)
        } else {
            let value = bcnumber::num2long(&top);
            (value, !bcnumber::is_zero(&top) && value == 0)
        };
        let max = i64::from(BC_INT_MAX);
        match var {
            0 => {
                if temp < 2 && !toobig {
                    self.i_base = 2;
                    self.rt_warn(b"ibase too small, set to 2");
                } else if temp > 16 || toobig {
                    if self.std_only {
                        self.i_base = 16;
                        self.rt_warn(b"ibase too large, set to 16");
                    } else if temp > 36 || toobig {
                        self.i_base = 36;
                        self.rt_warn(b"ibase too large, set to 36");
                    } else {
                        if temp >= 16 && self.warn_not_std {
                            self.rt_warn(b"ibase larger than 16 is non-standard");
                        }
                        self.i_base = int_of_long(temp);
                    }
                } else {
                    self.i_base = int_of_long(temp);
                }
            }
            1 => {
                if temp < 2 && !toobig {
                    self.o_base = 2;
                    self.rt_warn(b"obase too small, set to 2");
                } else if temp > max || toobig {
                    self.o_base = BC_INT_MAX;
                    self.rt_warn(format!("obase too large, set to {BC_INT_MAX}").as_bytes());
                } else {
                    self.o_base = int_of_long(temp);
                }
            }
            2 => {
                if temp > max || toobig {
                    self.scale = BC_INT_MAX;
                    self.rt_warn(format!("scale too large, set to {BC_INT_MAX}").as_bytes());
                } else {
                    self.scale = int_of_long(temp);
                }
            }
            _ => {
                // `history`: the size readline keeps. One too large to say
                // leaves it as it was.
                if toobig {
                    self.rt_warn(b"history too large, set to unlimited");
                } else {
                    self.n_history = int_of_long(temp);
                }
            }
        }
    }

    /// A variable's value set, the variable made if it has none.
    fn variable_value(&mut self, var: usize, value: Num) {
        if self.variables.len() <= var {
            self.variables.resize_with(var.saturating_add(1), Vec::new);
        }
        if let Some(stack) = self.variables.get_mut(var) {
            match stack.last_mut() {
                Some(v) => *v = value,
                None => stack.push(value),
            }
        }
    }

    /// A variable's value: zero for one never set.
    fn variable(&self, var: usize) -> Num {
        self.variables
            .get(var)
            .and_then(|s| s.last())
            .cloned()
            .unwrap_or_else(Num::zero)
    }

    /// `store_array (var_name)`: the top of the stack into the element the
    /// value under it names; the value left on the stack.
    fn store_array(&mut self, a: usize) {
        if !self.check_stack(2) {
            return;
        }
        let index = self.second();
        let Some(idx) = self.subscript(a, &index) else {
            return;
        };
        let value = self.top();
        self.array_elements(a)
            .borrow_mut()
            .insert(idx, value.clone());
        self.pop();
        self.set_top(value);
    }

    /// `load_var (var_name)`: a copy of a variable pushed.
    fn load_var(&mut self, var: usize) {
        let value = match var {
            0 => bcnumber::int2num(self.i_base),
            1 => bcnumber::int2num(self.o_base),
            2 => bcnumber::int2num(self.scale),
            3 => bcnumber::int2num(self.n_history),
            _ => self.variable(var),
        };
        self.ex_stack.push(value);
    }

    /// `load_array (var_name)`: the subscript on top replaced by the element.
    fn load_array(&mut self, a: usize) {
        if !self.check_stack(1) {
            return;
        }
        let index = self.top();
        let Some(idx) = self.subscript(a, &index) else {
            return;
        };
        let value = self
            .array_elements(a)
            .borrow()
            .get(&idx)
            .cloned()
            .unwrap_or_else(Num::zero);
        self.set_top(value);
    }

    /// `decr_var (var_name)`.
    fn decr_var(&mut self, var: usize) {
        match var {
            0 => {
                if self.i_base > 2 {
                    self.i_base = self.i_base.saturating_sub(1);
                } else {
                    self.rt_warn(b"ibase too small in --");
                }
            }
            1 => {
                if self.o_base > 2 {
                    self.o_base = self.o_base.saturating_sub(1);
                } else {
                    self.rt_warn(b"obase too small in --");
                }
            }
            2 => {
                if self.scale > 0 {
                    self.scale = self.scale.saturating_sub(1);
                } else {
                    self.rt_warn(b"scale can not be negative in -- ");
                }
            }
            3 => {
                self.n_history = self.n_history.wrapping_sub(1);
                if self.n_history < 0 {
                    self.n_history = -1;
                    self.rt_warn(b"history is negative, set to unlimited");
                }
            }
            _ => {
                let v = bcnumber::sub(&self.variable(var), &Num::one(), 0);
                self.variable_value(var, v);
            }
        }
    }

    /// `incr_var (var_name)`.
    fn incr_var(&mut self, var: usize) {
        match var {
            0 => {
                if self.i_base < 16 {
                    self.i_base = self.i_base.saturating_add(1);
                } else {
                    self.rt_warn(b"ibase too big in ++");
                }
            }
            1 => {
                if self.o_base < BC_INT_MAX {
                    self.o_base = self.o_base.saturating_add(1);
                } else {
                    self.rt_warn(b"obase too big in ++");
                }
            }
            2 => {
                if self.scale < BC_INT_MAX {
                    self.scale = self.scale.saturating_add(1);
                } else {
                    self.rt_warn(b"Scale too big in ++");
                }
            }
            3 => {
                self.n_history = self.n_history.wrapping_add(1);
                if self.n_history <= 0 {
                    self.n_history = -1;
                    self.rt_warn(b"history set to unlimited");
                }
            }
            _ => {
                let v = bcnumber::add(&self.variable(var), &Num::one(), 0);
                self.variable_value(var, v);
            }
        }
    }

    /// `decr_array` and `incr_array`: the element the top names changed by
    /// one, the subscript popped.
    fn step_array(&mut self, a: usize, up: bool) {
        if !self.check_stack(1) {
            return;
        }
        let index = self.top();
        let Some(idx) = self.subscript(a, &index) else {
            return;
        };
        self.pop();
        let elements = self.array_elements(a);
        let mut map = elements.borrow_mut();
        let old = map.get(&idx).cloned().unwrap_or_else(Num::zero);
        let new = if up {
            bcnumber::add(&old, &Num::one(), 0)
        } else {
            bcnumber::sub(&old, &Num::one(), 0)
        };
        map.insert(idx, new);
    }

    fn decr_array(&mut self, a: usize) {
        self.step_array(a, false);
    }

    fn incr_array(&mut self, a: usize) {
        self.step_array(a, true);
    }

    /// `auto_var (name)`: a new, empty level of a variable (`name` > 0) or
    /// an array (`name` < 0).
    fn auto_var(&mut self, name: i32) {
        if name > 0 {
            let v = usize::try_from(name).unwrap_or(0);
            if self.variables.len() <= v {
                self.variables.resize_with(v.saturating_add(1), Vec::new);
            }
            if let Some(stack) = self.variables.get_mut(v) {
                stack.push(Num::zero());
            }
        } else {
            let a = usize::try_from(name.unsigned_abs()).unwrap_or(0);
            if self.arrays.len() <= a {
                self.arrays.resize_with(a.saturating_add(1), Vec::new);
            }
            if let Some(levels) = self.arrays.get_mut(a) {
                levels.push(ArrayLevel {
                    value: None,
                    param: false,
                });
            }
        }
    }

    /// `pop_vars (list)`: each name's top level dropped.
    fn pop_vars(&mut self, list: &[Arg]) {
        for arg in list {
            if arg.name > 0 {
                if let Some(stack) = usize::try_from(arg.name)
                    .ok()
                    .and_then(|v| self.variables.get_mut(v))
                {
                    stack.pop();
                }
            } else if let Some(levels) = usize::try_from(arg.name.unsigned_abs())
                .ok()
                .and_then(|a| self.arrays.get_mut(a))
            {
                levels.pop();
            }
        }
    }

    /// `process_params (&pc, func)`: the call's arguments, on the stack,
    /// given to `func`'s parameters -- the last argument to the last
    /// parameter -- by the kinds the call's code lists.
    fn process_params(&mut self, func: usize) {
        let params = self
            .functions
            .get(func)
            .map(|f| f.params.clone())
            .unwrap_or_default();
        let mut at = Params::At(0);
        loop {
            let ch = self.byte();
            if ch == b':' {
                break;
            }
            let k = match at {
                Params::At(k) if k < params.len() => k,
                Params::At(_) => {
                    self.rt_error(b"Parameter number mismatch");
                    return;
                }
                Params::Lost => {
                    // Upstream follows a link out of a heap chunk's header
                    // and dies there.
                    self.out_of_the_list();
                }
            };
            let Some(&param) = params.get(k) else {
                return;
            };
            if ch == b'0' && param.name > 0 {
                let value = self.top();
                let v = usize::try_from(param.name).unwrap_or(0);
                if self.variables.len() <= v {
                    self.variables.resize_with(v.saturating_add(1), Vec::new);
                }
                if let Some(stack) = self.variables.get_mut(v) {
                    stack.push(value);
                }
                at = Params::At(k.saturating_add(1));
            } else if ch == b'1' && param.name < 0 {
                // The argument is the array's number.
                let src_a = usize::try_from(bcnumber::num2long(&self.top())).unwrap_or(0);
                let source = self.array_elements(src_a);
                self.auto_var(param.name);
                let dest_a = usize::try_from(param.name.unsigned_abs()).unwrap_or(0);
                let value = if param.is_var {
                    source
                } else {
                    Rc::new(RefCell::new(source.borrow().clone()))
                };
                if let Some(level) = self.arrays.get_mut(dest_a).and_then(|l| l.last_mut()) {
                    level.param = param.is_var;
                    level.value = Some(value);
                }
                at = Params::At(k.saturating_add(1));
            } else {
                let msg = if param.name < 0 {
                    let a = usize::try_from(param.name.unsigned_abs()).unwrap_or(0);
                    let mut m = b"Parameter type mismatch parameter ".to_vec();
                    m.extend_from_slice(
                        &self
                            .a_names
                            .get(a)
                            .cloned()
                            .flatten()
                            .unwrap_or_else(|| b"(null)".to_vec()),
                    );
                    m
                } else {
                    // `v_names[av_name]`, where the name is at av_name - 1:
                    // the next variable's name, if there is one.
                    let v = usize::try_from(param.name).unwrap_or(0);
                    let mut m = b"Parameter type mismatch, parameter ".to_vec();
                    m.extend_from_slice(
                        &self
                            .v_names
                            .get(v)
                            .cloned()
                            .flatten()
                            .unwrap_or_else(|| b"(null)".to_vec()),
                    );
                    m
                };
                let mut msg = msg;
                msg.push(b'.');
                self.rt_error(&msg);
                at = Params::Lost;
            }
            self.pop();
        }
        let done = match at {
            Params::At(k) => k >= params.len(),
            Params::Lost => false,
        };
        if !done {
            self.rt_error(b"Parameter number mismatch");
        }
    }

    /// Where upstream reads a parameter through the pointer a mismatch
    /// left: it dies of `SIGSEGV`, its buffered output unwritten.
    fn out_of_the_list(&mut self) -> ! {
        // Nothing to do about a failure: the abort below ends it either way.
        let _ = libcall::signal::set_default(SIGSEGV);
        let _ = libcall::signal::raise(SIGSEGV);
        std::process::abort()
    }
}

/// A count as `bc_int2num` takes it, an `int`.
fn int_of(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A `long` cast to `int`, as upstream's `(int) temp` is.
fn int_of_long(n: i64) -> i32 {
    let [a, b, c, d, ..] = n.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}
