//! The interpreter: a tree walk over the parsed program.
//!
//! ## `$0` and the fields are one thing viewed two ways
//!
//! Assigning `$2` changes `$0`; assigning `$0` changes every field; assigning
//! `NF` truncates the record. Doing any of that eagerly would mean splitting
//! every record whether the program looks at a field or not, which is most of
//! the cost of running awk on a file it barely inspects. So the record and the
//! field vector each carry a "still valid" flag and are reconciled on demand —
//! [`Interp::record`] rebuilds from the fields with `OFS` between them, and
//! [`Interp::ensure_split`] splits the record with `FS`. `NF` is not stored
//! anywhere; it is the field count, computed when read.
//!
//! ## Where a character is not a byte
//!
//! Records, fields and every string are bytes, because awk is a filter and a
//! line that is not UTF-8 has to come out unchanged. But `length`, `substr`,
//! `index`, `RSTART` and `RLENGTH` are specified in *characters*, and the regex
//! engine indexes by character too, so those five convert. Mixing the two up is
//! how `substr($0, 1, 3)` cuts a UTF-8 character in half.

use crate::ast::{
    BinOp, Builtin, CmpOp, Expr, GetlineSrc, Lvalue, Pattern, Program, RedirMode, Redirect, Stmt,
    VarRef, V_ARGC, V_ARGV, V_CONVFMT, V_ENVIRON, V_FILENAME, V_FNR, V_FS, V_NF, V_NR, V_OFMT,
    V_OFS, V_ORS, V_RLENGTH, V_RS, V_RSTART, V_SUBSEP,
};
use crate::io::{Inputs, Outputs, Records, Rs};
use crate::value::{compare, num_to_str, Str, Value};
use ere::{ch, Regex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;

/// An error that stops the program. awk has no exceptions; every one of these
/// ends the run with a diagnostic and status 2.
pub struct Fatal(pub String);

impl From<String> for Fatal {
    fn from(s: String) -> Fatal {
        Fatal(s)
    }
}

type R<T> = Result<T, Fatal>;

/// An awk array: shared, because arrays are passed to functions by reference.
type Array = Rc<RefCell<HashMap<Str, Value>>>;

/// What a variable slot holds.
#[derive(Clone)]
enum Cell {
    Val(Value),
    Arr(Array),
}

impl Default for Cell {
    fn default() -> Cell {
        Cell::Val(Value::Uninit)
    }
}

/// How control left a statement.
enum Flow {
    Normal,
    Break,
    Continue,
    Next,
    NextFile,
    Exit,
    Return(Value),
}

/// How `FS` splits a record.
enum Fs {
    /// `FS = " "`, the default: split on runs of blanks, ignoring leading and
    /// trailing ones. This is *not* the same as the single character `" "`.
    Whitespace,
    /// A single character, used literally even when it is a regex
    /// metacharacter — `FS = "."` splits on dots.
    Char(u8),
    /// `FS = ""`: every character is a field.
    Chars,
    Regex(Rc<Regex>),
}

/// The record and its fields, kept consistent lazily.
struct Fields {
    record: Str,
    fields: Vec<Str>,
    /// The field vector agrees with the record.
    split_valid: bool,
    /// The record agrees with the field vector.
    record_valid: bool,
}

impl Fields {
    fn new() -> Fields {
        Fields { record: Str::new(), fields: Vec::new(), split_valid: true, record_valid: true }
    }
}

/// The main input: the files named in `ARGV`, read in order.
struct MainInput {
    /// The next `ARGV` index to consider. The program may change `ARGV` and
    /// `ARGC` while running, so this is an index rather than a prepared list.
    argv: usize,
    current: Option<Records>,
    /// Whether any real file has been opened, which decides whether standard
    /// input is used when the arguments run out.
    opened_any: bool,
    stdin_used: bool,
    /// Set once every argument has been examined.
    done: bool,
}

pub struct Interp {
    prog: Program,
    globals: Vec<Cell>,
    /// The call frames. The last is the running function's.
    frames: Vec<Vec<Cell>>,
    f: Fields,
    fs: Fs,
    /// The `FS` string the current [`Fs`] was built from, so it is only rebuilt
    /// when it actually changes.
    fs_src: Str,
    rs: Rs,
    rs_src: Str,
    ranges: Vec<bool>,
    out: Outputs,
    inputs: Inputs,
    re_cache: HashMap<Str, Rc<Regex>>,
    main: MainInput,
    /// Set by `exit`; also the process's status.
    exit_code: Option<i32>,
    /// Depth of user function calls, bounded so a runaway recursion is a
    /// diagnostic rather than a stack overflow — which on a kernel with a
    /// guard page is a crash the script cannot catch.
    depth: usize,
    rng: u64,
    seed: f64,
    /// True while the END rules are running, so `exit` inside END does not
    /// re-run them.
    in_end: bool,
}

/// A recursion this deep is a program that will never finish; the limit exists
/// so it fails with a message instead of a stack overflow.
const MAX_DEPTH: usize = 2500;

impl Interp {
    /// Build an interpreter for `prog`, with `argv` as awk's `ARGV[1..]`.
    #[must_use]
    pub fn new(prog: Program, argv: &[Str], env: &[(Str, Str)]) -> Interp {
        let globals = vec![Cell::default(); prog.globals];
        let mut it = Interp {
            prog,
            globals,
            frames: Vec::new(),
            f: Fields::new(),
            fs: Fs::Whitespace,
            fs_src: b" ".to_vec(),
            rs: Rs::Char(b'\n'),
            rs_src: b"\n".to_vec(),
            ranges: Vec::new(),
            out: Outputs::new(),
            inputs: Inputs::new(),
            re_cache: HashMap::new(),
            main: MainInput {
                argv: 1,
                current: None,
                opened_any: false,
                stdin_used: false,
                done: false,
            },
            exit_code: None,
            depth: 0,
            rng: 0,
            seed: 0.0,
            in_end: false,
        };
        it.ranges = vec![false; it.prog.ranges];
        it.set_global(V_FS, Value::str(b" ".to_vec()));
        it.set_global(V_OFS, Value::str(b" ".to_vec()));
        it.set_global(V_ORS, Value::str(b"\n".to_vec()));
        it.set_global(V_RS, Value::str(b"\n".to_vec()));
        it.set_global(V_SUBSEP, Value::str(vec![0x1c]));
        it.set_global(V_CONVFMT, Value::str(b"%.6g".to_vec()));
        it.set_global(V_OFMT, Value::str(b"%.6g".to_vec()));
        it.set_global(V_NR, Value::Num(0.0));
        it.set_global(V_FNR, Value::Num(0.0));
        it.set_global(V_RSTART, Value::Num(0.0));
        it.set_global(V_RLENGTH, Value::Num(-1.0));
        it.set_global(V_FILENAME, Value::str(Str::new()));

        let environ = it.array_slot(V_ENVIRON);
        {
            let mut m = environ.borrow_mut();
            for (k, v) in env {
                m.insert(k.clone(), Value::from_input(v.clone()));
            }
        }
        let argv_arr = it.array_slot(V_ARGV);
        {
            let mut m = argv_arr.borrow_mut();
            m.insert(b"0".to_vec(), Value::str(b"awk".to_vec()));
            for (i, a) in argv.iter().enumerate() {
                let k = format!("{}", i.saturating_add(1)).into_bytes();
                m.insert(k, Value::from_input(a.clone()));
            }
        }
        // ARGC counts ARGV[0] ("awk") as well as the operands, so a program that
        // loops `for (i = 1; i < ARGC; i++)` sees exactly the file arguments.
        let argc = u32::try_from(argv.len()).unwrap_or(u32::MAX).saturating_add(1);
        it.set_global(V_ARGC, Value::Num(f64::from(argc)));
        // Seed the generator the way awk does: deterministically, so a program
        // that never calls `srand` gives the same answers on every run.
        it.rng = 0x2545_f491_4f6c_dd1d;
        it
    }

    /// Set a variable named on the command line by `-v` or as `var=value`.
    ///
    /// The value is a strnum, so `-v n=10` compares numerically — which is what
    /// makes `awk -v n=10 '$1 == n'` work on a numeric column.
    ///
    /// # Errors
    /// Returns a diagnostic if the name is one of the built-in arrays.
    pub fn assign_cli(&mut self, name: &str, value: Str) -> Result<(), String> {
        let Some(slot) = self.prog.global_names.iter().position(|n| n == name) else {
            // A name the program never mentions still has to be settable: a
            // script that reads `-v debug=1` only in a branch it does not have
            // must not fail, and a later `-f` file might use it.
            return Ok(());
        };
        if matches!(self.globals.get(slot), Some(Cell::Arr(_))) {
            return Err(format!("{name} is an array and cannot be assigned on the command line"));
        }
        // Through `set_var`, not `set_global`: `-F:` and `-v RS=;` have to take
        // effect, and it is `set_var` that recompiles the splitter when `FS` or
        // `RS` changes. Writing the slot directly stored the new value where
        // nothing would read it until the next assignment.
        self.set_var(VarRef::Global(slot), Value::from_input(value));
        Ok(())
    }

    /// Run the whole program, returning the process exit status.
    ///
    /// # Errors
    /// Returns the fatal diagnostic; the caller prints it and exits 2.
    pub fn run(&mut self) -> R<i32> {
        let begin = std::mem::take(&mut self.prog.begin);
        let res = self.exec_all(&begin);
        self.prog.begin = begin;
        let flow = res?;

        let needs_input = !self.prog.rules.is_empty() || !self.prog.end.is_empty();
        if !matches!(flow, Flow::Exit) && needs_input {
            self.main_loop()?;
        }

        // POSIX: `exit` in BEGIN or in a rule still runs END — that is what lets
        // a program bail out early and still print its totals. The only thing
        // that skips END is an `exit` from inside END itself, which empties the
        // list on its way out.
        let end = std::mem::take(&mut self.prog.end);
        self.in_end = true;
        let res = self.exec_all(&end);
        self.prog.end = end;
        res?;

        self.out
            .finish_all()
            .map_err(|e| Fatal(format!("write error: {}", coreutils::errmsg::strerror(&e))))?;
        Ok(self.exit_code.unwrap_or(0))
    }

    fn main_loop(&mut self) -> R<()> {
        loop {
            let Some(rec) = self.next_main_record()? else { return Ok(()) };
            self.bump(V_NR);
            self.bump(V_FNR);
            self.set_record(rec);
            match self.run_rules()? {
                Flow::Exit => return Ok(()),
                Flow::NextFile => {
                    self.main.current = None;
                }
                _ => {}
            }
        }
    }

    fn run_rules(&mut self) -> R<Flow> {
        let rules = std::mem::take(&mut self.prog.rules);
        let mut result = Ok(Flow::Normal);
        for rule in &rules {
            let matched = match &rule.pattern {
                Pattern::Always => Ok(true),
                Pattern::Expr(e) => self.eval(e).map(|v| v.truthy()),
                Pattern::Range(a, b, id) => self.range_matches(a, b, *id),
            };
            let matched = match matched {
                Ok(m) => m,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            };
            if !matched {
                continue;
            }
            let flow = match &rule.action {
                None => self.print_values(&[], None).map(|()| Flow::Normal),
                Some(body) => self.exec_all(body),
            };
            match flow {
                Ok(Flow::Normal) => {}
                Ok(Flow::Next) => break,
                Ok(other) => {
                    result = Ok(other);
                    break;
                }
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        self.prog.rules = rules;
        result
    }

    /// A `first, last` pattern. It is stateful on purpose: once `first` has
    /// matched the rule stays on until `last` does, and a record where both
    /// match is a one-record range, not the start of one.
    fn range_matches(&mut self, a: &Expr, b: &Expr, id: usize) -> R<bool> {
        let open = self.ranges.get(id).copied().unwrap_or(false);
        if open {
            if self.eval(b)?.truthy()
                && let Some(slot) = self.ranges.get_mut(id)
            {
                *slot = false;
            }
            return Ok(true);
        }
        if !self.eval(a)?.truthy() {
            return Ok(false);
        }
        if !self.eval(b)?.truthy()
            && let Some(slot) = self.ranges.get_mut(id)
        {
            *slot = true;
        }
        Ok(true)
    }

    // ---- the main input ---------------------------------------------------

    fn next_main_record(&mut self) -> R<Option<Str>> {
        loop {
            if let Some(reader) = self.main.current.as_mut() {
                let rs = self.rs.clone();
                match reader.next(&rs) {
                    Ok(Some(rec)) => return Ok(Some(rec)),
                    Ok(None) => self.main.current = None,
                    Err(e) => {
                        return Err(Fatal(format!(
                            "read error: {}",
                            coreutils::errmsg::strerror(&e)
                        )))
                    }
                }
            }
            if !self.open_next_input()? {
                return Ok(None);
            }
        }
    }

    /// Move to the next `ARGV` entry, honouring the `var=value` entries that
    /// are assignments rather than files. Returns false when there is no more
    /// input anywhere.
    fn open_next_input(&mut self) -> R<bool> {
        loop {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let argc = {
                let n = self.get_global(V_ARGC).to_num();
                if n <= 0.0 { 0usize } else { n as usize }
            };
            if self.main.argv >= argc {
                if self.main.done {
                    return Ok(false);
                }
                self.main.done = true;
                if self.main.opened_any || self.main.stdin_used {
                    return Ok(false);
                }
                // No file arguments at all: read standard input, as every other
                // filter does.
                self.main.stdin_used = true;
                self.main.current = Some(Records::new(Box::new(std::io::stdin())));
                self.set_global(V_FNR, Value::Num(0.0));
                return Ok(true);
            }
            let key = format!("{}", self.main.argv).into_bytes();
            self.main.argv = self.main.argv.saturating_add(1);
            let arg = {
                let arr = self.array_slot(V_ARGV);
                let v = arr.borrow().get(&key).cloned();
                v.unwrap_or(Value::Uninit)
            };
            let text = self.to_str(&arg);
            if text.is_empty() {
                // An emptied ARGV entry is skipped — that is how a program
                // removes a file from the list.
                continue;
            }
            if let Some((name, value)) = command_assignment(&text) {
                let value = unescape(&value);
                let _ = self.assign_cli(&name, value);
                continue;
            }
            self.main.opened_any = true;
            self.set_global(V_FILENAME, Value::str(text.as_ref().clone()));
            self.set_global(V_FNR, Value::Num(0.0));
            let src: Box<dyn Read> = if text.as_ref() == b"-" || text.as_ref() == b"/dev/stdin" {
                self.main.stdin_used = true;
                Box::new(std::io::stdin())
            } else {
                match std::fs::File::open(crate::io::os_path(&text)) {
                    Ok(f) => Box::new(f),
                    Err(e) => {
                        // An input file that cannot be opened stops the run. It
                        // is tempting to skip it and carry on, but an awk
                        // program is usually computing a total over the files it
                        // was given, and a total that silently omits one of them
                        // is worse than no answer at all.
                        return Err(Fatal(format!(
                            "can't open file {}: {}",
                            String::from_utf8_lossy(&text),
                            coreutils::errmsg::strerror(&e)
                        )));
                    }
                }
            };
            self.main.current = Some(Records::new(src));
            return Ok(true);
        }
    }

    // ---- statements -------------------------------------------------------

    fn exec_all(&mut self, body: &[Stmt]) -> R<Flow> {
        for s in body {
            match self.exec(s)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, s: &Stmt) -> R<Flow> {
        match s {
            Stmt::Nop => Ok(Flow::Normal),
            Stmt::Expr(e) => {
                self.eval(e)?;
                Ok(Flow::Normal)
            }
            Stmt::Block(b) => self.exec_all(b),
            Stmt::Print(args, r) => {
                let vals = self.eval_all(args)?;
                let target = self.redirect_name(r.as_ref())?;
                self.print_values(&vals, target)?;
                Ok(Flow::Normal)
            }
            Stmt::Printf(args, r) => {
                let vals = self.eval_all(args)?;
                let target = self.redirect_name(r.as_ref())?;
                self.printf_values(&vals, target)?;
                Ok(Flow::Normal)
            }
            Stmt::If(c, t, e) => {
                if self.eval(c)?.truthy() {
                    return self.exec(t);
                }
                match e {
                    Some(e) => self.exec(e),
                    None => Ok(Flow::Normal),
                }
            }
            Stmt::While(c, body) => {
                while self.eval(c)?.truthy() {
                    match self.exec(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::DoWhile(body, c) => {
                loop {
                    match self.exec(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        other => return Ok(other),
                    }
                    if !self.eval(c)?.truthy() {
                        break;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For { init, cond, step, body } => {
                if let Some(i) = init {
                    self.exec(i)?;
                }
                loop {
                    if let Some(c) = cond
                        && !self.eval(c)?.truthy()
                    {
                        break;
                    }
                    match self.exec(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        other => return Ok(other),
                    }
                    if let Some(s) = step {
                        self.exec(s)?;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::ForIn { var, array, body } => {
                let arr = self.array_ref(*array);
                // A snapshot of the keys, because the body is allowed to delete
                // from the array it is iterating — and often does.
                let keys: Vec<Str> = arr.borrow().keys().cloned().collect();
                for k in keys {
                    if !arr.borrow().contains_key(&k) {
                        continue;
                    }
                    self.assign(var, Value::from_input(k))?;
                    match self.exec(body)? {
                        Flow::Normal | Flow::Continue => {}
                        Flow::Break => break,
                        other => return Ok(other),
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Next => Ok(Flow::Next),
            Stmt::NextFile => Ok(Flow::NextFile),
            Stmt::Break => Ok(Flow::Break),
            Stmt::Continue => Ok(Flow::Continue),
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Value::Uninit,
                };
                Ok(Flow::Return(v))
            }
            Stmt::Exit(e) => {
                if let Some(e) = e {
                    #[allow(clippy::cast_possible_truncation)]
                    let code = self.eval(e)?.to_num() as i32;
                    self.exit_code = Some(code);
                } else if self.exit_code.is_none() {
                    self.exit_code = Some(0);
                }
                if self.in_end {
                    // `exit` inside END must not re-enter END.
                    self.prog.end.clear();
                }
                Ok(Flow::Exit)
            }
            Stmt::Delete(arr, subs) => {
                let a = self.array_ref(*arr);
                if subs.is_empty() {
                    a.borrow_mut().clear();
                } else {
                    let key = self.subscript(subs)?;
                    a.borrow_mut().remove(&key);
                }
                Ok(Flow::Normal)
            }
        }
    }

    fn eval_all(&mut self, args: &[Expr]) -> R<Vec<Value>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.eval(a)?);
        }
        Ok(out)
    }

    fn redirect_name(&mut self, r: Option<&Redirect>) -> R<Option<(RedirMode, Str)>> {
        match r {
            None => Ok(None),
            Some(r) => {
                let v = self.eval(&r.target)?;
                Ok(Some((r.mode, self.to_str(&v).as_ref().clone())))
            }
        }
    }

    fn print_values(&mut self, vals: &[Value], target: Option<(RedirMode, Str)>) -> R<()> {
        let ofs = self.string_of(V_OFS);
        let ors = self.string_of(V_ORS);
        let ofmt = self.string_of(V_OFMT);
        let mut line = Str::new();
        if vals.is_empty() {
            line.extend_from_slice(self.record().as_slice());
        } else {
            for (i, v) in vals.iter().enumerate() {
                if i > 0 {
                    line.extend_from_slice(&ofs);
                }
                // `print` formats a number with OFMT, not CONVFMT — the two are
                // separately settable and a program that changes one expects
                // the other to stay put.
                match v {
                    Value::Num(n) => line.extend_from_slice(&num_to_str(*n, &ofmt)),
                    other => line.extend_from_slice(&self.to_str(other)),
                }
            }
        }
        line.extend_from_slice(&ors);
        self.emit(&line, target)
    }

    fn printf_values(&mut self, vals: &[Value], target: Option<(RedirMode, Str)>) -> R<()> {
        let Some(fmt) = vals.first() else {
            return Err(Fatal("printf: no format string".to_string()));
        };
        let fmt = self.to_str(fmt);
        let convfmt = self.string_of(V_CONVFMT);
        let rest = vals.get(1..).unwrap_or_default();
        let text = crate::fmt::sprintf(&fmt, rest, &convfmt).map_err(Fatal)?;
        self.emit(&text, target)
    }

    fn emit(&mut self, bytes: &[u8], target: Option<(RedirMode, Str)>) -> R<()> {
        let res = match &target {
            None => self.out.write_stdout(bytes),
            Some((mode, name)) => self.out.write_to(name, *mode, bytes),
        };
        res.map_err(|e| {
            let where_ = match &target {
                None => "standard output".to_string(),
                Some((_, n)) => String::from_utf8_lossy(n).into_owned(),
            };
            Fatal(format!("{where_}: {}", coreutils::errmsg::strerror(&e)))
        })
    }

    // ---- expressions ------------------------------------------------------

    #[allow(clippy::too_many_lines)]
    fn eval(&mut self, e: &Expr) -> R<Value> {
        match e {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(Rc::clone(s))),
            // A bare regex in a value context asks whether it matches `$0`.
            Expr::Regex(re) => {
                let rec = self.record().clone();
                Ok(Value::Num(f64::from(u8::from(re.is_match(&rec)))))
            }
            Expr::Get(lv) => self.load(lv),
            Expr::Assign(lv, rhs) => {
                let v = self.eval(rhs)?;
                self.assign(lv, v.clone())?;
                Ok(v)
            }
            Expr::AugAssign(lv, op, rhs) => {
                let r = self.eval(rhs)?.to_num();
                let l = self.load(lv)?.to_num();
                let v = Value::Num(arith(*op, l, r)?);
                self.assign(lv, v.clone())?;
                Ok(v)
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c)?.truthy() {
                    self.eval(a)
                } else {
                    self.eval(b)
                }
            }
            Expr::Or(a, b) => {
                if self.eval(a)?.truthy() {
                    return Ok(Value::Num(1.0));
                }
                Ok(Value::Num(f64::from(u8::from(self.eval(b)?.truthy()))))
            }
            Expr::And(a, b) => {
                if !self.eval(a)?.truthy() {
                    return Ok(Value::Num(0.0));
                }
                Ok(Value::Num(f64::from(u8::from(self.eval(b)?.truthy()))))
            }
            Expr::Not(a) => Ok(Value::Num(f64::from(u8::from(!self.eval(a)?.truthy())))),
            Expr::Neg(a) => Ok(Value::Num(-self.eval(a)?.to_num())),
            Expr::Pos(a) => Ok(Value::Num(self.eval(a)?.to_num())),
            Expr::In(subs, arr) => {
                let key = self.subscript(subs)?;
                let a = self.array_ref(*arr);
                let present = a.borrow().contains_key(&key);
                Ok(Value::Num(f64::from(u8::from(present))))
            }
            Expr::Match { neg, lhs, rhs } => {
                let subject = self.eval(lhs)?;
                let subject = self.to_str(&subject);
                let re = self.regex_of(rhs)?;
                let m = re.is_match(&subject);
                Ok(Value::Num(f64::from(u8::from(m != *neg))))
            }
            Expr::Cmp(op, a, b) => {
                let l = self.eval(a)?;
                let r = self.eval(b)?;
                let convfmt = self.string_of(V_CONVFMT);
                let ord = compare(&l, &r, &convfmt);
                let yes = match ord {
                    // NaN compares false against everything, as in C.
                    None => false,
                    Some(o) => match op {
                        CmpOp::Lt => o.is_lt(),
                        CmpOp::Le => o.is_le(),
                        CmpOp::Gt => o.is_gt(),
                        CmpOp::Ge => o.is_ge(),
                        CmpOp::Eq => o.is_eq(),
                        CmpOp::Ne => o.is_ne(),
                    },
                };
                Ok(Value::Num(f64::from(u8::from(yes))))
            }
            Expr::Concat(a, b) => {
                let l = self.eval(a)?;
                let r = self.eval(b)?;
                let mut s = self.to_str(&l).as_ref().clone();
                s.extend_from_slice(&self.to_str(&r));
                Ok(Value::str(s))
            }
            Expr::Bin(op, a, b) => {
                let l = self.eval(a)?.to_num();
                let r = self.eval(b)?.to_num();
                Ok(Value::Num(arith(*op, l, r)?))
            }
            Expr::PreIncr(lv, d) => {
                let v = self.load(lv)?.to_num() + d;
                self.assign(lv, Value::Num(v))?;
                Ok(Value::Num(v))
            }
            Expr::PostIncr(lv, d) => {
                let old = self.load(lv)?.to_num();
                self.assign(lv, Value::Num(old + d))?;
                Ok(Value::Num(old))
            }
            Expr::Call(f, args) => self.call(*f, args),
            Expr::Builtin(b, args) => self.builtin(*b, args),
            Expr::Getline(g) => self.getline(g),
        }
    }

    // ---- variables and fields --------------------------------------------

    fn load(&mut self, lv: &Lvalue) -> R<Value> {
        match lv {
            Lvalue::Var(v) => Ok(self.get_var(*v)),
            Lvalue::Field(e) => {
                let n = self.field_index(e)?;
                Ok(self.get_field(n))
            }
            Lvalue::Index(v, subs) => {
                let key = self.subscript(subs)?;
                let a = self.array_ref(*v);
                // Referring to `a[k]` *creates* it, which is why
                // `if (a[k] == "") …` makes `k in a` true afterwards. Every awk
                // does this and programs test for it.
                let mut m = a.borrow_mut();
                Ok(m.entry(key).or_insert(Value::Uninit).clone())
            }
        }
    }

    fn assign(&mut self, lv: &Lvalue, v: Value) -> R<()> {
        match lv {
            Lvalue::Var(r) => {
                self.set_var(*r, v);
                Ok(())
            }
            Lvalue::Field(e) => {
                let n = self.field_index(e)?;
                let s = self.to_str(&v).as_ref().clone();
                self.set_field(n, s);
                Ok(())
            }
            Lvalue::Index(r, subs) => {
                let key = self.subscript(subs)?;
                let a = self.array_ref(*r);
                a.borrow_mut().insert(key, v);
                Ok(())
            }
        }
    }

    fn field_index(&mut self, e: &Expr) -> R<usize> {
        let n = self.eval(e)?.to_num();
        if n < 0.0 || !n.is_finite() {
            return Err(Fatal(format!("attempt to access field {n}")));
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = n as usize;
        // A field number this large is a typo, not a record; without the bound
        // `$1000000000 = "x"` allocates until the machine gives up.
        if i > 16_000_000 {
            return Err(Fatal(format!("field {i} is beyond any plausible record")));
        }
        Ok(i)
    }

    fn get_var(&mut self, v: VarRef) -> Value {
        if v == VarRef::Global(V_NF) {
            self.ensure_split();
            let n = self.f.fields.len();
            return Value::Num(f64::from(u32::try_from(n).unwrap_or(u32::MAX)));
        }
        match self.cell(v) {
            Some(Cell::Val(val)) => val.clone(),
            // Using an array where a scalar was wanted is a program bug, but it
            // is not worth killing the run over: the empty value is what an
            // unset variable gives and it keeps the diagnostic in one place.
            _ => Value::Uninit,
        }
    }

    fn set_var(&mut self, v: VarRef, val: Value) {
        if let VarRef::Global(slot) = v {
            match slot {
                V_NF => {
                    self.ensure_split();
                    let n = val.to_num();
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let n = if n < 0.0 { 0usize } else { (n as usize).min(16_000_000) };
                    self.f.fields.resize(n, Str::new());
                    self.f.record_valid = false;
                    self.f.split_valid = true;
                    return;
                }
                V_FS | V_RS => {
                    self.set_global(slot, val);
                    self.refresh_separators();
                    return;
                }
                _ => {}
            }
        }
        self.set_cell(v, Cell::Val(val));
    }

    fn get_global(&self, slot: usize) -> Value {
        match self.globals.get(slot) {
            Some(Cell::Val(v)) => v.clone(),
            _ => Value::Uninit,
        }
    }

    fn set_global(&mut self, slot: usize, v: Value) {
        if let Some(c) = self.globals.get_mut(slot) {
            *c = Cell::Val(v);
        }
    }

    fn bump(&mut self, slot: usize) {
        let n = self.get_global(slot).to_num();
        self.set_global(slot, Value::Num(n + 1.0));
    }

    fn cell(&self, v: VarRef) -> Option<&Cell> {
        match v {
            VarRef::Global(s) => self.globals.get(s),
            VarRef::Local(s) => self.frames.last().and_then(|f| f.get(s)),
        }
    }

    fn set_cell(&mut self, v: VarRef, c: Cell) {
        let slot = match v {
            VarRef::Global(s) => self.globals.get_mut(s),
            VarRef::Local(s) => self.frames.last_mut().and_then(|f| f.get_mut(s)),
        };
        if let Some(slot) = slot {
            *slot = c;
        }
    }

    /// The array in a slot, creating it if the slot is still untouched.
    fn array_ref(&mut self, v: VarRef) -> Array {
        if let Some(Cell::Arr(a)) = self.cell(v) {
            return Rc::clone(a);
        }
        let a: Array = Rc::new(RefCell::new(HashMap::new()));
        self.set_cell(v, Cell::Arr(Rc::clone(&a)));
        a
    }

    fn array_slot(&mut self, slot: usize) -> Array {
        self.array_ref(VarRef::Global(slot))
    }

    /// Join subscripts with `SUBSEP`, which is how awk gives a one-dimensional
    /// map two-dimensional syntax.
    fn subscript(&mut self, subs: &[Expr]) -> R<Str> {
        if subs.len() == 1
            && let Some(one) = subs.first()
        {
            let v = self.eval(one)?;
            return Ok(self.to_str(&v).as_ref().clone());
        }
        let sep = self.string_of(V_SUBSEP);
        let mut out = Str::new();
        for (i, s) in subs.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&sep);
            }
            let v = self.eval(s)?;
            out.extend_from_slice(&self.to_str(&v));
        }
        Ok(out)
    }

    fn get_field(&mut self, n: usize) -> Value {
        if n == 0 {
            return Value::from_input(self.record().clone());
        }
        self.ensure_split();
        match self.f.fields.get(n.saturating_sub(1)) {
            Some(s) => Value::from_input(s.clone()),
            // Past NF is the empty string, and reading it does not extend the
            // record — only assigning does.
            None => Value::Uninit,
        }
    }

    fn set_field(&mut self, n: usize, s: Str) {
        if n == 0 {
            self.set_record(s);
            return;
        }
        self.ensure_split();
        if self.f.fields.len() < n {
            self.f.fields.resize(n, Str::new());
        }
        if let Some(slot) = self.f.fields.get_mut(n.saturating_sub(1)) {
            *slot = s;
        }
        self.f.record_valid = false;
    }

    fn set_record(&mut self, rec: Str) {
        self.f.record = rec;
        self.f.record_valid = true;
        self.f.split_valid = false;
    }

    fn ensure_split(&mut self) {
        if self.f.split_valid {
            return;
        }
        let rec = self.record().clone();
        self.f.fields = self.split_record(&rec);
        self.f.split_valid = true;
    }

    /// Split a record into fields by the current `FS`.
    fn split_record(&self, rec: &[u8]) -> Vec<Str> {
        // In paragraph mode a newline separates fields whatever FS says, which
        // is what makes `RS=""` useful for stanza-structured files.
        let paragraph = matches!(self.rs, Rs::Paragraph);
        split_with(&self.fs, rec, paragraph)
    }

    /// Rebuild `FS` and `RS` from their variables. Called after either is
    /// assigned, and cheap enough to be called when neither changed.
    fn refresh_separators(&mut self) {
        let fs = self.string_of(V_FS);
        if fs != self.fs_src {
            self.fs = make_fs(&fs);
            self.fs_src = fs;
            // The fields were split by the old FS; POSIX says a change to FS
            // takes effect on the *next* record, so the current split stands.
        }
        let rs = self.string_of(V_RS);
        if rs != self.rs_src {
            self.rs = make_rs(&rs);
            self.rs_src = rs;
        }
    }

    fn string_of(&self, slot: usize) -> Str {
        let convfmt = match self.globals.get(V_CONVFMT) {
            Some(Cell::Val(v)) => v.to_str(b"%.6g"),
            _ => Rc::new(b"%.6g".to_vec()),
        };
        match self.globals.get(slot) {
            Some(Cell::Val(v)) => v.to_str(&convfmt).as_ref().clone(),
            _ => Str::new(),
        }
    }

    fn to_str(&self, v: &Value) -> Rc<Str> {
        let convfmt = match self.globals.get(V_CONVFMT) {
            Some(Cell::Val(v)) => v.to_str(b"%.6g"),
            _ => Rc::new(b"%.6g".to_vec()),
        };
        v.to_str(&convfmt)
    }

    // ---- functions --------------------------------------------------------

    fn call(&mut self, slot: usize, args: &[Expr]) -> R<Value> {
        let Some(func) = self.prog.funcs.get(slot).cloned() else {
            return Err(Fatal("calling an undefined function".to_string()));
        };
        if args.len() > func.params.len() {
            return Err(Fatal(format!(
                "function {}: called with {} arguments but declared with {}",
                func.name,
                args.len(),
                func.params.len()
            )));
        }
        if self.depth >= MAX_DEPTH {
            return Err(Fatal(format!("function {}: recursion too deep", func.name)));
        }

        let is_array = self.prog.param_is_array.get(slot).cloned().unwrap_or_default();
        let mut frame: Vec<Cell> = Vec::with_capacity(func.params.len());
        for i in 0..func.params.len() {
            let wants_array = is_array.get(i).copied().unwrap_or(false);
            match args.get(i) {
                None if wants_array => {
                    // An argument the caller did not pass is a fresh local
                    // array — this is how awk programs declare local arrays.
                    frame.push(Cell::Arr(Rc::new(RefCell::new(HashMap::new()))));
                }
                None => frame.push(Cell::Val(Value::Uninit)),
                Some(Expr::Get(Lvalue::Var(v))) if wants_array => {
                    // By reference: the callee's changes are the caller's.
                    frame.push(Cell::Arr(self.array_ref(*v)));
                }
                Some(a) => {
                    let v = self.eval(a)?;
                    frame.push(Cell::Val(v));
                }
            }
        }

        self.frames.push(frame);
        self.depth = self.depth.saturating_add(1);
        let flow = self.exec_all(&func.body);
        self.depth = self.depth.saturating_sub(1);
        self.frames.pop();
        match flow? {
            Flow::Return(v) => Ok(v),
            // `next` and `exit` inside a function propagate out of it, but a
            // tree walk cannot carry them through an expression; a function
            // that ends any other way simply returns the empty value.
            Flow::Exit => {
                if self.exit_code.is_none() {
                    self.exit_code = Some(0);
                }
                Err(Fatal(String::new()))
            }
            _ => Ok(Value::Uninit),
        }
    }

    // ---- getline ----------------------------------------------------------

    fn getline(&mut self, g: &crate::ast::Getline) -> R<Value> {
        let rs = self.rs.clone();
        match &g.src {
            GetlineSrc::Main => {
                let rec = match self.next_main_record() {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(Value::Num(0.0)),
                    Err(_) => return Ok(Value::Num(-1.0)),
                };
                self.bump(V_NR);
                self.bump(V_FNR);
                match &g.into {
                    // Into a variable: NR and FNR move, but the fields do not,
                    // because `$0` was not touched.
                    Some(lv) => self.assign(lv, Value::from_input(rec))?,
                    None => self.set_record(rec),
                }
                Ok(Value::Num(1.0))
            }
            GetlineSrc::File(e) => {
                let v = self.eval(e)?;
                let name = self.to_str(&v).as_ref().clone();
                let rec = match self.inputs.file(&name).and_then(|r| r.next(&rs)) {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(Value::Num(0.0)),
                    // A file that will not open is -1, not a fatal error: that
                    // is what lets `while ((getline < f) > 0)` be written
                    // against a file that may not be there.
                    Err(_) => return Ok(Value::Num(-1.0)),
                };
                match &g.into {
                    Some(lv) => self.assign(lv, Value::from_input(rec))?,
                    None => self.set_record(rec),
                }
                Ok(Value::Num(1.0))
            }
            GetlineSrc::Cmd(e) => {
                let v = self.eval(e)?;
                let name = self.to_str(&v).as_ref().clone();
                // The child inherits our standard output, so anything buffered
                // has to be flushed before it can write.
                let _ = self.out.flush(None);
                let rec = match self.inputs.command(&name).and_then(|r| r.next(&rs)) {
                    Ok(Some(r)) => r,
                    Ok(None) => return Ok(Value::Num(0.0)),
                    Err(_) => return Ok(Value::Num(-1.0)),
                };
                self.bump(V_NR);
                match &g.into {
                    Some(lv) => self.assign(lv, Value::from_input(rec))?,
                    None => self.set_record(rec),
                }
                Ok(Value::Num(1.0))
            }
        }
    }

    // ---- built-in functions ----------------------------------------------

    /// The compiled form of a regex argument.
    ///
    /// A `/re/` literal was compiled when the program was parsed. Anything else
    /// is a *dynamic* regex — its text is computed at run time — so it is
    /// compiled on first use and cached, because the usual shape is a pattern
    /// held in a variable and used on every record.
    fn regex_of(&mut self, e: &Expr) -> R<Rc<Regex>> {
        if let Expr::Regex(re) = e {
            return Ok(Rc::clone(re));
        }
        let v = self.eval(e)?;
        let text = self.to_str(&v).as_ref().clone();
        if let Some(re) = self.re_cache.get(&text) {
            return Ok(Rc::clone(re));
        }
        let re = crate::parse::compile_regex(&text).map_err(Fatal)?;
        let re = Rc::new(re);
        // The cache is per distinct pattern text; a program that builds a new
        // pattern from every record would otherwise grow it without bound.
        if self.re_cache.len() < 1000 {
            self.re_cache.insert(text, Rc::clone(&re));
        }
        Ok(re)
    }

    #[allow(clippy::too_many_lines)]
    fn builtin(&mut self, b: Builtin, args: &[Expr]) -> R<Value> {
        match b {
            Builtin::Length => {
                let Some(a) = args.first() else {
                    let rec = self.record().clone();
                    return Ok(Value::Num(count_chars(&rec)));
                };
                // `length(arr)` is the element count. It is not POSIX, but it
                // is in every awk in use and the alternative — the length of
                // the empty string — is never what anyone meant.
                if let Expr::Get(Lvalue::Var(v)) = a
                    && matches!(self.cell(*v), Some(Cell::Arr(_)))
                {
                    let arr = self.array_ref(*v);
                    let n = arr.borrow().len();
                    return Ok(Value::Num(f64::from(u32::try_from(n).unwrap_or(u32::MAX))));
                }
                let v = self.eval(a)?;
                let s = self.to_str(&v);
                Ok(Value::Num(count_chars(&s)))
            }
            Builtin::Substr => {
                let sv = self.eval_arg(args, 0)?;
                let s = self.to_str(&sv);
                let chars: Vec<ch::Ch> = ch::chars(&s).collect();
                let len = chars.len();
                // The start and the length are *rounded*, not truncated, so
                // `substr(s, 1.5, 2.4)` takes two characters from the second.
                //
                // A start below 1 becomes 1 and the length is kept, so
                // `substr("Alpha1", 0, 3)` is `Alp` rather than `Al`. The awks
                // are split on this — mawk measures the length from the
                // out-of-range start and drops the part that falls off the
                // front — and POSIX's wording ("the at most n-character
                // substring that begins at position m") does not settle it. This
                // follows gawk and the one true awk, which are the two a script
                // is most likely to have been written against.
                let m = round_half_up(self.eval_arg(args, 1)?.to_num());
                let n = match args.get(2) {
                    None => f64::INFINITY,
                    Some(e) => round_half_up(self.eval(e)?.to_num()),
                };
                let lo = m.max(1.0);
                let end = if n.is_infinite() { f64::INFINITY } else { lo + n };
                #[allow(clippy::cast_precision_loss)]
                let hi = if end.is_infinite() {
                    (len as f64) + 1.0
                } else {
                    end.min((len as f64) + 1.0)
                };
                // Not `hi <= lo`: either may be NaN — `substr($0, "x")` is a
                // legal call — and an empty result is the right answer then.
                if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
                    return Ok(Value::str(Str::new()));
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let (a, bnd) = ((lo as usize).saturating_sub(1), (hi as usize).saturating_sub(1));
                let mut out = Str::new();
                for c in chars.get(a..bnd).unwrap_or_default() {
                    c.push_to(&mut out);
                }
                Ok(Value::str(out))
            }
            Builtin::Index => {
                let hv = self.eval_arg(args, 0)?;
                let nv = self.eval_arg(args, 1)?;
                let hay = self.to_str(&hv);
                let needle = self.to_str(&nv);
                Ok(Value::Num(index_of(&hay, &needle)))
            }
            Builtin::Split => {
                let sv = self.eval_arg(args, 0)?;
                let s = self.to_str(&sv).as_ref().clone();
                let Some(Expr::Get(Lvalue::Var(arr))) = args.get(1) else {
                    return Err(Fatal("split: the second argument must be an array".to_string()));
                };
                let arr = *arr;
                let fs = match args.get(2) {
                    None => None,
                    Some(Expr::Regex(re)) => Some(Fs::Regex(Rc::clone(re))),
                    Some(e) => {
                        let v = self.eval(e)?;
                        let text = self.to_str(&v);
                        Some(make_fs(&text))
                    }
                };
                let parts = match &fs {
                    None => self.split_record(&s),
                    Some(f) => split_with(f, &s, false),
                };
                let a = self.array_ref(arr);
                {
                    let mut m = a.borrow_mut();
                    m.clear();
                    for (i, p) in parts.iter().enumerate() {
                        let key = format!("{}", i.saturating_add(1)).into_bytes();
                        m.insert(key, Value::from_input(p.clone()));
                    }
                }
                Ok(Value::Num(f64::from(u32::try_from(parts.len()).unwrap_or(u32::MAX))))
            }
            Builtin::Sub | Builtin::Gsub => self.substitute(b == Builtin::Gsub, args),
            Builtin::Match => {
                let sv = self.eval_arg(args, 0)?;
                let s = self.to_str(&sv);
                let Some(pat) = args.get(1) else {
                    return Err(Fatal("match: missing pattern".to_string()));
                };
                let re = self.regex_of(pat)?;
                match re.find(&s) {
                    Some((a, b2)) => {
                        // RSTART and RLENGTH are in characters, so the byte
                        // offsets the engine gives have to be converted.
                        let start = count_chars(s.get(..a).unwrap_or_default()) + 1.0;
                        let len = count_chars(s.get(a..b2).unwrap_or_default());
                        self.set_global(V_RSTART, Value::Num(start));
                        self.set_global(V_RLENGTH, Value::Num(len));
                        Ok(Value::Num(start))
                    }
                    None => {
                        self.set_global(V_RSTART, Value::Num(0.0));
                        self.set_global(V_RLENGTH, Value::Num(-1.0));
                        Ok(Value::Num(0.0))
                    }
                }
            }
            Builtin::Sprintf => {
                let vals = self.eval_all(args)?;
                let Some(fmt) = vals.first() else {
                    return Err(Fatal("sprintf: no format string".to_string()));
                };
                let fmt = self.to_str(fmt);
                let convfmt = self.string_of(V_CONVFMT);
                let text = crate::fmt::sprintf(&fmt, vals.get(1..).unwrap_or_default(), &convfmt)
                    .map_err(Fatal)?;
                Ok(Value::str(text))
            }
            Builtin::Sin => Ok(Value::Num(self.eval_arg(args, 0)?.to_num().sin())),
            Builtin::Cos => Ok(Value::Num(self.eval_arg(args, 0)?.to_num().cos())),
            Builtin::Atan2 => {
                let y = self.eval_arg(args, 0)?.to_num();
                let x = self.eval_arg(args, 1)?.to_num();
                Ok(Value::Num(y.atan2(x)))
            }
            Builtin::Exp => Ok(Value::Num(self.eval_arg(args, 0)?.to_num().exp())),
            Builtin::Log => Ok(Value::Num(self.eval_arg(args, 0)?.to_num().ln())),
            Builtin::Sqrt => Ok(Value::Num(self.eval_arg(args, 0)?.to_num().sqrt())),
            Builtin::Int => Ok(Value::Num(self.eval_arg(args, 0)?.to_num().trunc())),
            Builtin::Rand => Ok(Value::Num(self.next_random())),
            Builtin::Srand => {
                let previous = self.seed;
                let new = match args.first() {
                    Some(e) => self.eval(e)?.to_num(),
                    None => seconds_since_epoch(),
                };
                self.seed = new;
                let bits = new.to_bits();
                // Any odd, non-zero state will do; xorshift is dead at zero.
                self.rng = bits ^ 0x9e37_79b9_7f4a_7c15 | 1;
                Ok(Value::Num(previous))
            }
            Builtin::Tolower | Builtin::Toupper => {
                let v = self.eval_arg(args, 0)?;
                let s = self.to_str(&v);
                let mut out = Str::new();
                for c in ch::chars(&s) {
                    let mapped = if b == Builtin::Tolower { c.to_lowercase() } else { c.to_uppercase() };
                    for m in mapped {
                        m.push_to(&mut out);
                    }
                }
                Ok(Value::str(out))
            }
            Builtin::System => {
                let v = self.eval_arg(args, 0)?;
                let cmd = self.to_str(&v);
                // Everything buffered must be written before the child runs, or
                // the child's output and ours come out in the wrong order.
                self.out
                    .flush(None)
                    .map_err(|e| Fatal(format!("flush: {}", coreutils::errmsg::strerror(&e))))?;
                match crate::io::shell(&cmd).status() {
                    Ok(s) => Ok(Value::Num(f64::from(s.code().unwrap_or(0)))),
                    Err(_) => Ok(Value::Num(-1.0)),
                }
            }
            Builtin::Close => {
                let v = self.eval_arg(args, 0)?;
                let name = self.to_str(&v);
                if let Some(code) = self.inputs.close(&name) {
                    return Ok(Value::Num(f64::from(code)));
                }
                match self.out.close(&name) {
                    Some(code) => Ok(Value::Num(f64::from(code))),
                    None => Ok(Value::Num(-1.0)),
                }
            }
            Builtin::Fflush => {
                let name = match args.first() {
                    None => None,
                    Some(e) => {
                        let v = self.eval(e)?;
                        Some(self.to_str(&v).as_ref().clone())
                    }
                };
                let res = self.out.flush(name.as_deref());
                Ok(Value::Num(if res.is_ok() { 0.0 } else { -1.0 }))
            }
        }
    }

    fn eval_arg(&mut self, args: &[Expr], i: usize) -> R<Value> {
        match args.get(i) {
            Some(e) => self.eval(e),
            None => Ok(Value::Uninit),
        }
    }

    /// `sub` and `gsub`.
    ///
    /// The replacement's `&` stands for the matched text and `\&` for a literal
    /// ampersand — the one piece of syntax in awk where a backslash has to be
    /// interpreted at *substitution* time rather than when the string was read.
    fn substitute(&mut self, global: bool, args: &[Expr]) -> R<Value> {
        let Some(pat) = args.first() else {
            return Err(Fatal("sub: missing pattern".to_string()));
        };
        let re = self.regex_of(pat)?;
        let rv = self.eval_arg(args, 1)?;
        let repl = self.to_str(&rv).as_ref().clone();
        let target = match args.get(2) {
            Some(Expr::Get(lv)) => lv.clone(),
            None => Lvalue::Field(Box::new(Expr::Num(0.0))),
            Some(_) => {
                return Err(Fatal(
                    "sub: the third argument must be a variable, a field or an array element"
                        .to_string(),
                ))
            }
        };
        let sv = self.load(&target)?;
        let hay = self.to_str(&sv).as_ref().clone();

        let mut out = Str::new();
        let mut last = 0usize;
        let mut count = 0u32;
        for (s, e) in re.find_iter(&hay) {
            out.extend_from_slice(hay.get(last..s).unwrap_or_default());
            expand_ampersand(&repl, hay.get(s..e).unwrap_or_default(), &mut out);
            last = e;
            count = count.saturating_add(1);
            if !global {
                break;
            }
        }
        if count == 0 {
            return Ok(Value::Num(0.0));
        }
        out.extend_from_slice(hay.get(last..).unwrap_or_default());
        self.assign(&target, Value::str(out))?;
        Ok(Value::Num(f64::from(count)))
    }

    /// A uniform value in `[0, 1)`, from a xorshift generator.
    ///
    /// awk's `rand` is not a cryptographic primitive and never was — the same
    /// program must give the same numbers on every run unless it calls `srand`,
    /// which is the opposite of what a secure generator does.
    fn next_random(&mut self) -> f64 {
        let mut x = self.rng | 1;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        // 53 bits is exactly what an f64 mantissa holds, so every value in the
        // range is reachable and none is reachable twice as often.
        #[allow(clippy::cast_precision_loss)]
        let v = (x >> 11) as f64 / f64::from(1u32 << 22) / f64::from(1u32 << 31);
        v.fract().abs()
    }
}

impl Interp {
    /// `$0`, rebuilt from the fields first if one of them was assigned.
    ///
    /// Every reader of the record goes through here rather than touching
    /// `self.f.record`, because `$2 = "x"` leaves the stored record stale on
    /// purpose: rebuilding on assignment would join with whatever `OFS` was at
    /// that moment, and awk joins with `OFS` as it is when `$0` is *read*.
    fn record(&mut self) -> &Str {
        self.rebuild_record();
        &self.f.record
    }

    /// Rebuild `$0` from the fields, joined by `OFS`.
    fn rebuild_record(&mut self) {
        if self.f.record_valid {
            return;
        }
        let ofs = self.string_of(V_OFS);
        let mut out = Str::new();
        for (i, f) in self.f.fields.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&ofs);
            }
            out.extend_from_slice(f);
        }
        self.f.record = out;
        self.f.record_valid = true;
    }
}

/// Split `text` by `fs`.
fn split_with(fs: &Fs, text: &[u8], paragraph_mode: bool) -> Vec<Str> {
    match fs {
        Fs::Whitespace => text
            .split(|b| matches!(b, b' ' | b'\t' | b'\n'))
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect(),
        Fs::Chars => ch::chars(text).map(ch::Ch::to_str).collect(),
        Fs::Char(c) => {
            if text.is_empty() {
                return Vec::new();
            }
            if paragraph_mode {
                return text
                    .split(|b| b == c || *b == b'\n')
                    .map(<[u8]>::to_vec)
                    .collect();
            }
            text.split(|b| b == c).map(<[u8]>::to_vec).collect()
        }
        Fs::Regex(re) => {
            if text.is_empty() {
                return Vec::new();
            }
            let mut out = Vec::new();
            let mut last = 0usize;
            for (s, e) in re.find_iter(text) {
                // A separator that matches nothing would split between every
                // pair of characters and never advance; skip it.
                if e == s {
                    continue;
                }
                out.push(text.get(last..s).unwrap_or_default().to_vec());
                last = e;
            }
            out.push(text.get(last..).unwrap_or_default().to_vec());
            if paragraph_mode {
                return out
                    .into_iter()
                    .flat_map(|p| p.split(|b| *b == b'\n').map(<[u8]>::to_vec).collect::<Vec<_>>())
                    .collect();
            }
            out
        }
    }
}

/// Build the field splitter `FS` describes.
///
/// The single-character case is *literal*, not a one-character regex: POSIX
/// says so, and it is why `FS = "."` splits on dots rather than on everything.
fn make_fs(fs: &[u8]) -> Fs {
    match fs {
        b" " => Fs::Whitespace,
        b"" => Fs::Chars,
        one if one.len() == 1 => match one.first() {
            Some(c) => Fs::Char(*c),
            None => Fs::Whitespace,
        },
        // A two-character escape like `\t` reaching here unprocessed would be a
        // regex that means "a tab", which is the same thing, so no special case
        // is needed.
        other => match Regex::new(other) {
            Ok(re) => Fs::Regex(Rc::new(re)),
            // An FS that will not compile is not worth killing the run over;
            // treating it literally is what the single-character case does and
            // is the least surprising fallback.
            Err(_) => Fs::Char(other.first().copied().unwrap_or(b' ')),
        },
    }
}

fn make_rs(rs: &[u8]) -> Rs {
    match rs {
        b"" => Rs::Paragraph,
        one if one.len() == 1 => Rs::Char(one.first().copied().unwrap_or(b'\n')),
        other => match Regex::new(other) {
            Ok(re) => Rs::Regex(Rc::new(re)),
            Err(_) => Rs::Char(other.first().copied().unwrap_or(b'\n')),
        },
    }
}

fn arith(op: BinOp, l: f64, r: f64) -> R<f64> {
    Ok(match op {
        BinOp::Add => l + r,
        BinOp::Sub => l - r,
        BinOp::Mul => l * r,
        BinOp::Div => {
            if r == 0.0 {
                return Err(Fatal("division by zero".to_string()));
            }
            l / r
        }
        BinOp::Mod => {
            if r == 0.0 {
                return Err(Fatal("division by zero in %".to_string()));
            }
            // C's `fmod`, which keeps the sign of the left operand — awk is
            // specified in terms of it, so `-7 % 3` is -1 and not 2.
            l % r
        }
        BinOp::Pow => l.powf(r),
    })
}

/// The number of characters in a byte string, counting an undecodable byte as
/// one character.
fn count_chars(s: &[u8]) -> f64 {
    f64::from(u32::try_from(ch::chars(s).count()).unwrap_or(u32::MAX))
}

/// `index()`: the 1-based character position of `needle` in `hay`, or 0.
fn index_of(hay: &[u8], needle: &[u8]) -> f64 {
    if needle.is_empty() {
        // Every awk answers 1 here, including for an empty haystack.
        return 1.0;
    }
    if needle.len() > hay.len() {
        return 0.0;
    }
    match hay.windows(needle.len()).position(|w| w == needle) {
        Some(byte) => count_chars(hay.get(..byte).unwrap_or_default()) + 1.0,
        None => 0.0,
    }
}

/// POSIX rounds `substr`'s arguments to the nearest integer, halves away from
/// zero — not C's truncation, which is why this is not `trunc`.
fn round_half_up(v: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    v.round()
}

/// Write `repl` into `out`, with `&` replaced by `matched`.
fn expand_ampersand(repl: &[u8], matched: &[u8], out: &mut Str) {
    let mut i = 0usize;
    while let Some(&c) = repl.get(i) {
        match c {
            b'&' => {
                out.extend_from_slice(matched);
                i = i.saturating_add(1);
            }
            b'\\' => match repl.get(i.saturating_add(1)) {
                Some(b'&') => {
                    out.push(b'&');
                    i = i.saturating_add(2);
                }
                Some(b'\\') => {
                    out.push(b'\\');
                    i = i.saturating_add(2);
                }
                // A backslash before anything else is itself. The string
                // literal's own escapes were already resolved by the lexer, so
                // whatever is left here was written `\\` in the program.
                _ => {
                    out.push(b'\\');
                    i = i.saturating_add(1);
                }
            },
            other => {
                out.push(other);
                i = i.saturating_add(1);
            }
        }
    }
}

/// Split `var=value` if that is what the argument is.
///
/// The name has to be a valid awk identifier — otherwise `file=1.txt` would be
/// an assignment and `1.txt` would never be read.
pub fn command_assignment(arg: &[u8]) -> Option<(String, Str)> {
    let eq = arg.iter().position(|b| *b == b'=')?;
    let name = arg.get(..eq)?;
    if name.is_empty() {
        return None;
    }
    let first_ok = name.first().is_some_and(|c| *c == b'_' || c.is_ascii_alphabetic());
    if !first_ok || !name.iter().all(|c| *c == b'_' || c.is_ascii_alphanumeric()) {
        return None;
    }
    let value = arg.get(eq.saturating_add(1)..)?.to_vec();
    Some((String::from_utf8_lossy(name).into_owned(), value))
}

/// Resolve the escape sequences in a command-line assignment's value, which
/// POSIX requires — `-F '\t'` and `-v sep='\t'` both mean a tab.
#[must_use]
pub fn unescape(s: &[u8]) -> Str {
    let mut out = Str::new();
    let mut i = 0usize;
    while let Some(&c) = s.get(i) {
        if c != b'\\' {
            out.push(c);
            i = i.saturating_add(1);
            continue;
        }
        i = i.saturating_add(1);
        let Some(&n) = s.get(i) else {
            out.push(b'\\');
            break;
        };
        i = i.saturating_add(1);
        match n {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'/' => out.push(b'/'),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'v' => out.push(0x0b),
            b'0'..=b'7' => {
                let mut v = u32::from(n.wrapping_sub(b'0'));
                let mut k = 1u32;
                while k < 3 {
                    match s.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            v = v.saturating_mul(8).saturating_add(u32::from(d.wrapping_sub(b'0')));
                            i = i.saturating_add(1);
                            k = k.saturating_add(1);
                        }
                        _ => break,
                    }
                }
                out.push(u8::try_from(v & 0xff).unwrap_or(0));
            }
            other => {
                out.push(b'\\');
                out.push(other);
            }
        }
    }
    out
}

fn seconds_since_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
