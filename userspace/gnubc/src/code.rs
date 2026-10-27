//! The code generator and the loader: `util.c`'s `init_gen`, `generate`,
//! `run_code`, `lookup` and `check_params`, and `load.c`.
//!
//! The parser's actions produce upstream's textual code (`"K12:s5:p"`),
//! which is either printed (`-c`) or loaded into a function's body as byte
//! code -- a number's digits as their values, a variable's or a function's
//! number as one byte below 128 or two with the top bit set, a label as two
//! bytes, low first.

use crate::{Arg, Bc, MAX_STORE};

/// What a name is looked up as: `lookup`'s `namekind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `SIMPLE`: a variable.
    Simple,
    /// `ARRAY`.
    Array,
    /// `FUNCT`: a function called.
    Funct,
    /// `FUNCTDEF`: a function being defined.
    FunctDef,
}

/// `BC_LABEL_GROUP`: labels per group.
const LABEL_GROUP: usize = 64;

/// The text up to its first NUL: what a C string holding it is.
fn c_str(text: &[u8]) -> &[u8] {
    let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
    text.get(..end).unwrap_or_default()
}

/// `long_val (&str)`: an optionally negative decimal number at `text[*at..]`,
/// `*at` moved past it.
fn long_val(text: &[u8], at: &mut usize) -> i64 {
    let mut neg = false;
    if text.get(*at) == Some(&b'-') {
        neg = true;
        *at = at.saturating_add(1);
    }
    // `int val`: upstream's accumulator wraps as an int's would.
    let mut val: i32 = 0;
    while let Some(&d) = text.get(*at).filter(|d| d.is_ascii_digit()) {
        val = val
            .wrapping_mul(10)
            .wrapping_add(i32::from(d.wrapping_sub(b'0')));
        *at = at.saturating_add(1);
    }
    let val = i64::from(val);
    if neg { val.wrapping_neg() } else { val }
}

impl Bc {
    /// `init_gen ()`: ready for the next run of code.
    pub(crate) fn init_gen(&mut self) {
        self.break_label = 0;
        self.continue_label = 0;
        self.next_label = 1;
        self.out_count = 2;
        if self.compile_only {
            self.out.write(b"@i");
            self.check_stdout();
        } else {
            self.init_load();
        }
        self.had_error = false;
        self.did_gen = false;
    }

    /// `generate (str)`: code for the machine -- printed under `-c`, else
    /// loaded.
    pub(crate) fn generate(&mut self, code: &[u8]) {
        let code = c_str(code);
        self.did_gen = true;
        if self.compile_only {
            self.out.write(code);
            self.out_count = self.out_count.saturating_add(code.len());
            if self.out_count > 60 {
                self.out.write(b"\n");
                self.out_count = 0;
            }
            self.check_stdout();
        } else {
            self.load_code(code);
        }
    }

    /// `run_code ()`: the code loaded since the last run, run unless it
    /// failed to compile; then ready for more.
    pub(crate) fn run_code(&mut self) {
        if !self.had_error && self.did_gen {
            if self.compile_only {
                self.out.write(b"@r\n");
                self.check_stdout();
                self.out_count = 0;
            } else {
                self.execute();
            }
        }
        if self.did_gen {
            self.init_gen();
        } else {
            self.had_error = false;
        }
    }

    /// `init_load ()`: the main program emptied, loading to its start.
    pub(crate) fn init_load(&mut self) {
        self.clear_func(0);
        self.load_func = 0;
        self.load_addr = 0;
        self.load_str = false;
        self.load_const = false;
    }

    /// Function `f`, made if it is not there yet (upstream's arrays grow by
    /// 32 at a time, each entry empty).
    #[allow(clippy::indexing_slicing, reason = "grown first to hold `f`")]
    pub(crate) fn function_mut(&mut self, f: usize) -> &mut crate::Function {
        if self.functions.len() <= f {
            self.functions
                .resize_with(f.saturating_add(1), crate::Function::default);
        }
        if self.f_names.len() <= f {
            self.f_names.resize(f.saturating_add(1), None);
        }
        &mut self.functions[f]
    }

    /// `clear_func (func)`: the function emptied for (re)definition. Its
    /// body's old bytes stay where they are, as upstream's do, past the new
    /// code's end.
    pub(crate) fn clear_func(&mut self, func: usize) {
        let f = self.function_mut(func);
        f.defined = false;
        f.code_size = 0;
        f.autos.clear();
        f.params.clear();
        f.labels.clear();
    }

    /// `addbyte (thebyte)`: a byte onto the end of the code being loaded.
    fn addbyte(&mut self, byte: u8) {
        if self.had_error {
            return;
        }
        let addr = self.load_addr;
        self.load_addr = self.load_addr.saturating_add(1);
        let f = self.function_mut(self.load_func);
        if f.body.len() <= addr {
            f.body.resize(addr.saturating_add(1), 0);
        }
        if let Some(slot) = f.body.get_mut(addr) {
            *slot = byte;
        }
        f.code_size = f.code_size.saturating_add(1);
    }

    /// `def_label (lab)`: label `lab` is the address being loaded.
    fn def_label(&mut self, lab: usize) {
        let group = lab / LABEL_GROUP;
        let offset = lab % LABEL_GROUP;
        let addr = self.load_addr;
        let f = self.function_mut(self.load_func);
        if f.labels.len() <= group {
            f.labels
                .resize(group.saturating_add(1), [None; LABEL_GROUP]);
        }
        if let Some(slot) = f.labels.get_mut(group).and_then(|g| g.get_mut(offset)) {
            *slot = Some(addr);
        }
    }

    /// A variable's, array's or function's number into the code: one byte
    /// below 128, else two, the high one flagged.
    fn add_number(&mut self, n: i64) {
        // `unsigned long`, then `char`: only the low bits are kept.
        let n = n.cast_unsigned();
        if n < 128 {
            self.addbyte(n.to_le_bytes()[0]);
        } else {
            self.addbyte(((n >> 8) & 0xff).to_le_bytes()[0] | 0x80);
            self.addbyte((n & 0xff).to_le_bytes()[0]);
        }
    }

    /// `load_code (code)`: textual code into byte code, where `load_func`
    /// and `load_addr` say. A string or a constant may continue from one
    /// call to the next.
    #[allow(clippy::too_many_lines, reason = "load_code's one switch")]
    pub(crate) fn load_code(&mut self, code: &[u8]) {
        let code = c_str(code);
        let at = |i: usize| code.get(i).copied().unwrap_or(0);
        let mut i = 0usize;
        while at(i) != 0 {
            if self.had_error {
                return;
            }
            let c = at(i);
            if self.load_str {
                if c == b'"' {
                    self.load_str = false;
                }
                self.addbyte(c);
                i = i.saturating_add(1);
                continue;
            }
            if self.load_const {
                if c == b'\n' {
                    i = i.saturating_add(1);
                } else if c == b':' {
                    self.load_const = false;
                    self.addbyte(c);
                    i = i.saturating_add(1);
                } else if c == b'.' {
                    self.addbyte(c);
                    i = i.saturating_add(1);
                } else {
                    if c > b'F' && (self.warn_not_std || self.std_only) {
                        if self.std_only {
                            self.yyerror(b"Error in numeric constant");
                        } else {
                            self.ct_warn(b"Non-standard base in numeric constant");
                        }
                    }
                    let value = if c >= b'A' {
                        c.wrapping_add(10).wrapping_sub(b'A')
                    } else {
                        c.wrapping_sub(b'0')
                    };
                    self.addbyte(value);
                    i = i.saturating_add(1);
                }
                continue;
            }
            match c {
                b'"' => self.load_str = true,
                b'N' => {
                    i = i.saturating_add(1);
                    let lab = long_val(code, &mut i);
                    self.def_label(usize::try_from(lab).unwrap_or(0));
                }
                b'B' | b'J' | b'Z' => {
                    self.addbyte(c);
                    i = i.saturating_add(1);
                    let lab = long_val(code, &mut i);
                    if lab.cast_unsigned() > 65535 {
                        self.err.write(b"Program too big.\n");
                        self.exit(1);
                    }
                    self.addbyte((lab & 0xff).to_le_bytes()[0]);
                    self.addbyte(((lab >> 8) & 0xff).to_le_bytes()[0]);
                }
                b'F' => {
                    i = i.saturating_add(1);
                    let func = usize::try_from(long_val(code, &mut i)).unwrap_or(0);
                    self.clear_func(func);
                    let mut params = Vec::new();
                    // `while (*str++ != '.')`.
                    loop {
                        let was = at(i);
                        i = i.saturating_add(1);
                        if was == b'.' || was == 0 {
                            break;
                        }
                        if at(i) == b'.' {
                            i = i.saturating_add(1);
                            break;
                        }
                        let is_var = at(i) == b'*';
                        if is_var {
                            i = i.saturating_add(1);
                        }
                        let name = long_val(code, &mut i);
                        params.insert(
                            0,
                            Arg {
                                name: i32::try_from(name).unwrap_or(0),
                                is_var,
                            },
                        );
                    }
                    let mut autos = Vec::new();
                    while at(i) != b'[' && at(i) != 0 {
                        if at(i) == b',' {
                            i = i.saturating_add(1);
                        }
                        let name = long_val(code, &mut i);
                        autos.insert(
                            0,
                            Arg {
                                name: i32::try_from(name).unwrap_or(0),
                                is_var: false,
                            },
                        );
                    }
                    let f = self.function_mut(func);
                    f.params = params;
                    f.autos = autos;
                    self.save_func = self.load_func;
                    self.save_addr = self.load_addr;
                    self.load_func = func;
                    self.load_addr = 0;
                }
                b']' => {
                    let f = self.load_func;
                    self.function_mut(f).defined = true;
                    self.load_func = self.save_func;
                    self.load_addr = self.save_addr;
                }
                b'C' => {
                    self.addbyte(c);
                    i = i.saturating_add(1);
                    let func = long_val(code, &mut i);
                    self.add_number(func);
                    if at(i) == b',' {
                        i = i.saturating_add(1);
                    }
                    while at(i) != b':' && at(i) != 0 {
                        self.addbyte(at(i));
                        i = i.saturating_add(1);
                    }
                    self.addbyte(b':');
                }
                b'c' => {
                    self.addbyte(c);
                    i = i.saturating_add(1);
                    self.addbyte(at(i));
                }
                b'K' => {
                    self.addbyte(c);
                    self.load_const = true;
                }
                b'd' | b'i' | b'l' | b's' | b'A' | b'M' | b'L' | b'S' => {
                    self.addbyte(c);
                    i = i.saturating_add(1);
                    let n = long_val(code, &mut i);
                    self.add_number(n);
                }
                b'@' => {
                    i = i.saturating_add(1);
                    match at(i) {
                        b'i' => self.init_load(),
                        b'r' => self.execute(),
                        _ => {}
                    }
                }
                b'\n' => {}
                _ => self.addbyte(c),
            }
            // Past the instruction's last byte: a label's or a number's
            // ':', a function's '[', or the instruction itself. At the end
            // of the text (an instruction cut short) the loop ends.
            if at(i) == 0 {
                break;
            }
            i = i.saturating_add(1);
        }
    }

    /// `lookup (name, namekind)`: the number of `name` as a variable,
    /// array (negated) or function, numbered on first sight.
    pub(crate) fn lookup(&mut self, name: &[u8], kind: Kind) -> i32 {
        if name.len() != 1 {
            let mut msg = b"multiple letter name - ".to_vec();
            msg.extend_from_slice(name);
            self.ct_warn(&msg);
        }
        let mut id = self.names.get(name).copied().unwrap_or_default();
        match kind {
            Kind::Array => {
                if id.a_name != 0 {
                    return id.a_name.saturating_neg();
                }
                id.a_name = self.next_array;
                self.next_array = self.next_array.saturating_add(1);
                self.names.insert(name.to_vec(), id);
                if id.a_name >= MAX_STORE {
                    self.yyerror(b"Too many array variables");
                    self.exit(1);
                }
                let a = usize::try_from(id.a_name).unwrap_or(0);
                if self.a_names.len() <= a {
                    self.a_names.resize(a.saturating_add(1), None);
                    self.arrays.resize_with(a.saturating_add(1), Vec::new);
                }
                if let Some(slot) = self.a_names.get_mut(a) {
                    *slot = Some(name.to_vec());
                }
                id.a_name.saturating_neg()
            }
            Kind::Funct | Kind::FunctDef => {
                if id.f_name != 0 {
                    // A definition of a math library function makes a new
                    // function under the name, unnamed in messages.
                    if self.use_math && kind == Kind::FunctDef && id.f_name <= 6 {
                        id.f_name = self.next_func;
                        self.next_func = self.next_func.saturating_add(1);
                        self.names.insert(name.to_vec(), id);
                        self.function_mut(usize::try_from(id.f_name).unwrap_or(0));
                    }
                    return id.f_name;
                }
                id.f_name = self.next_func;
                self.next_func = self.next_func.saturating_add(1);
                self.names.insert(name.to_vec(), id);
                if id.f_name >= MAX_STORE {
                    self.yyerror(b"Too many functions");
                    self.exit(1);
                }
                let f = usize::try_from(id.f_name).unwrap_or(0);
                self.function_mut(f);
                if let Some(slot) = self.f_names.get_mut(f) {
                    *slot = Some(name.to_vec());
                }
                id.f_name
            }
            Kind::Simple => {
                if id.v_name != 0 {
                    return id.v_name;
                }
                id.v_name = self.next_var;
                self.next_var = self.next_var.saturating_add(1);
                self.names.insert(name.to_vec(), id);
                if id.v_name > MAX_STORE {
                    self.yyerror(b"Too many variables");
                    self.exit(1);
                }
                let v = usize::try_from(id.v_name).unwrap_or(0);
                if self.variables.len() <= v {
                    self.variables.resize_with(v.saturating_add(1), Vec::new);
                    self.v_names.resize(v.saturating_add(1), None);
                }
                // Upstream's own off-by-one: variable n's name at n - 1,
                // where the messages that name a variable do not look.
                if let Some(slot) = self.v_names.get_mut(v.saturating_sub(1)) {
                    *slot = Some(name.to_vec());
                }
                id.v_name
            }
        }
    }

    /// `check_params (params, autos)`: no name twice among a function's
    /// parameters and autos, and no `*` among the autos.
    pub(crate) fn check_params(&mut self, params: &[Arg], autos: &[Arg]) {
        for (k, p) in params.iter().enumerate() {
            for q in params.iter().skip(k.saturating_add(1)) {
                if q.name == p.name {
                    self.yyerror(b"duplicate parameter names");
                }
            }
            if p.is_var {
                self.ct_warn(b"Variable array parameter");
            }
        }
        for (k, a) in autos.iter().enumerate() {
            for b in autos.iter().skip(k.saturating_add(1)) {
                if b.name == a.name {
                    self.yyerror(b"duplicate auto variable names");
                }
            }
            if a.is_var {
                self.yyerror(b"* not allowed here");
            }
        }
        for p in params {
            for a in autos {
                if a.name == p.name {
                    self.yyerror(b"variable in both parameter and auto lists");
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::io::{Input, Output, Sink, Source};

    fn bc() -> Bc {
        Bc::new(
            Output::new(Sink::Memory(Vec::new()), false, false),
            Output::new(Sink::Memory(Vec::new()), true, false),
            Input::new(Source::Memory(Vec::new(), 0)),
        )
    }

    #[test]
    fn a_long_val_reads_a_signed_number_and_stops_after_it() {
        let mut i = 0;
        assert_eq!(long_val(b"-12:", &mut i), -12);
        assert_eq!(i, 3);
        let mut i = 1;
        assert_eq!(long_val(b"K:", &mut i), 0);
        assert_eq!(i, 1);
    }

    #[test]
    fn a_function_record_takes_its_parameters_and_autos_in_reverse() {
        let mut b = bc();
        b.load_code(b"F1,5,*-2.6,7[l5:R]");
        let f = &b.functions[1];
        assert!(f.defined);
        assert_eq!(
            f.params,
            [
                Arg {
                    name: -2,
                    is_var: true
                },
                Arg {
                    name: 5,
                    is_var: false
                }
            ]
        );
        assert_eq!(
            f.autos,
            [
                Arg {
                    name: 7,
                    is_var: false
                },
                Arg {
                    name: 6,
                    is_var: false
                }
            ]
        );
        assert_eq!(f.body.get(..f.code_size).unwrap(), b"l\x05R");
        assert_eq!(
            (b.load_func, b.load_addr),
            (0, 0),
            "back to the main program"
        );
    }

    #[test]
    fn numbers_constants_strings_and_labels_load_as_bytes() {
        let mut b = bc();
        b.load_code(b"K1A.F:s200:N3:J3:w\"a\"C7,01:");
        let f = &b.functions[0];
        let code = f.body.get(..f.code_size).unwrap();
        assert_eq!(
            code,
            [
                b'K', 1, 10, b'.', 15, b':', b's', 0x80, 200, b'J', 3, 0, b'w', b'a', b'"', b'C',
                7, b'0', b'1', b':'
            ]
        );
        assert_eq!(f.labels[0][3], Some(9));
    }

    #[test]
    fn a_constant_continues_into_the_next_text() {
        let mut b = bc();
        b.load_code(b"K12");
        b.load_code(b"\n3:");
        let f = &b.functions[0];
        assert_eq!(f.body.get(..f.code_size).unwrap(), [b'K', 1, 2, 3, b':']);
    }

    #[test]
    fn names_are_numbered_on_first_sight_by_kind() {
        let mut b = bc();
        assert_eq!(b.lookup(b"x", Kind::Simple), 5);
        assert_eq!(b.lookup(b"x", Kind::Array), -1);
        assert_eq!(b.lookup(b"y", Kind::Simple), 6);
        assert_eq!(b.lookup(b"x", Kind::Simple), 5);
        assert_eq!(b.lookup(b"x", Kind::Funct), 1);
        assert_eq!(
            b.v_names[4].as_deref(),
            Some(&b"x"[..]),
            "variable 5's name at 4"
        );
        assert_eq!(b.a_names[1].as_deref(), Some(&b"x"[..]));
        assert_eq!(b.f_names[1].as_deref(), Some(&b"x"[..]));
    }

    #[test]
    fn a_math_library_function_redefined_is_a_new_one() {
        let mut b = bc();
        b.use_math = true;
        assert_eq!(b.lookup(b"e", Kind::Funct), 1);
        assert_eq!(b.lookup(b"e", Kind::FunctDef), 2);
        assert_eq!(b.lookup(b"e", Kind::Funct), 2);
        assert_eq!(b.f_names.get(2).cloned().flatten(), None);
    }
}
