//! The interpreter: the state a running awk program has, and the operations
//! on it -- records and fields, variables, input and output. The loop that runs
//! the compiled program is [`run`]; see [`crate::compile`] for why the program
//! is compiled rather than walked.
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

mod run;

use crate::ast::{
    Loc, Program, RedirMode, V_ARGC, V_ARGV, V_CONVFMT, V_ENVIRON, V_FILENAME, V_FNR, V_FS, V_NF,
    V_NR, V_OFMT, V_OFS, V_ORS, V_RLENGTH, V_RS, V_RSTART, V_SUBSEP, VarRef,
};
use crate::compile::Code;
use crate::io::{Inputs, Outputs, Records, Rs};
use crate::value::{Str, Value, c_long, num_to_str};
use ere::awk::{self as escape, Warnings};
use ere::{Regex, ch};
use run::{Ctx, ForIter, Frame, Item};
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

/// A regex search that gave up is fatal, not a non-match.
///
/// Only a pattern with a backreference can produce one. Folding it into "did
/// not match" would make `$0 !~ /re/` true for a line we never actually
/// examined — a silently wrong answer in a program whose whole output is
/// selected by such tests. Stopping with a diagnostic is the only honest
/// reading, and it is what `gawk` does with its own regex-cost limits.
impl From<ere::MatchLimit> for Fatal {
    fn from(e: ere::MatchLimit) -> Fatal {
        Fatal(e.to_string())
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

/// How control left a stretch of code: by reaching its end, or by `next`,
/// `nextfile` or `exit` -- from wherever they were run, a function included.
enum Flow {
    Normal,
    Next,
    NextFile,
    Exit,
}

/// What reading the next record of the main input came to. A file that would
/// not open is not one of these: that is a fatal error wherever it happens.
enum MainRead {
    Record(Str),
    Eof,
    /// The open file could not be read.
    Failed(std::io::Error),
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
        Fields {
            record: Str::new(),
            fields: Vec::new(),
            split_valid: true,
            record_valid: true,
        }
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
    /// The program, compiled; shared so the loop can hold it while it runs.
    code: Rc<Code>,
    globals: Vec<Cell>,
    /// The call frames, on the heap rather than the native stack. The last is
    /// the running function's.
    frames: Vec<Frame>,
    /// The values, arrays and assignment targets the instructions work on.
    stack: Vec<Item>,
    /// The `for (k in a)` loops in progress.
    iters: Vec<ForIter>,
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
    rng: u64,
    seed: f64,
    /// The run's escape warnings: gawk says each once per run, so this is the
    /// same table the program text was lexed with ([`Interp::adopt_warnings`]).
    warnings: Warnings,
    /// The line of the instruction running, as a diagnostic names it --
    /// gawk's `sourceline`, set by every instruction that has a line. `None`
    /// until the first one runs.
    loc: Option<Loc>,
}

/// Write every escape warning said and not yet written, as gawk's `warning()`
/// does: `awk: `, then `prefix` -- where the program was, `cmd. line:1: ` and
/// at run time the input file and record, as [`Interp::diagnostic_prefix`]
/// gives it, or nothing for the command line's `-v`, `-F` and `var=value`,
/// which gawk does not place either -- then `warning: ` and the message.
pub fn emit_warnings(w: &mut Warnings, prefix: &[u8]) {
    for message in w.take() {
        let mut line = b"awk: ".to_vec();
        line.extend_from_slice(prefix);
        line.extend_from_slice(b"warning: ");
        line.extend_from_slice(&message);
        line.push(b'\n');
        coreutils::stdfd::diag_bytes(&line);
    }
}

impl Interp {
    /// Build an interpreter for `prog`, with `argv` as awk's `ARGV[1..]`.
    #[must_use]
    pub fn new(prog: Program, argv: &[Str], env: &[(Str, Str)]) -> Interp {
        let globals = vec![Cell::default(); prog.globals];
        let code = Rc::new(crate::compile::compile(&prog));
        let mut it = Interp {
            prog,
            code,
            globals,
            frames: Vec::new(),
            stack: Vec::new(),
            iters: Vec::new(),
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
            rng: 0,
            seed: 0.0,
            warnings: Warnings::default(),
            loc: None,
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
        let argc = u32::try_from(argv.len())
            .unwrap_or(u32::MAX)
            .saturating_add(1);
        it.set_global(V_ARGC, Value::Num(f64::from(argc)));
        // Seed the generator the way awk does: deterministically, so a program
        // that never calls `srand` gives the same answers on every run.
        it.rng = 0x2545_f491_4f6c_dd1d;
        it
    }

    /// Carry on with the escape warnings the command line and the program text
    /// were read with, so that what they already said is not said again.
    pub fn adopt_warnings(&mut self, warnings: Warnings) {
        self.warnings = warnings;
    }

    /// Write whatever the escape layers have said since last time, each with
    /// where the program was, as gawk's `warning()` does.
    fn say_warnings(&mut self) {
        if self.warnings.is_empty() {
            return;
        }
        self.flush_before_diagnostic();
        let prefix = self.diagnostic_prefix();
        emit_warnings(&mut self.warnings, &prefix);
    }

    /// Flush standard output before a diagnostic, as gawk's `err()` does, so
    /// that when both go to one place the warning follows what was printed
    /// before it rather than jumping the queue. A failure here is not this
    /// diagnostic's to report: the next write, or the exit, meets it again.
    fn flush_before_diagnostic(&mut self) {
        let _ = self.out.flush_stdout();
    }

    /// What gawk's `err()` puts between `awk: ` and a diagnostic: the running
    /// statement's `cmd. line:3: ` (or `prog.awk:3: `), then, once input has
    /// been read, `(FILENAME=... FNR=...) `. Measured, gawk 5.2.1 `--posix`:
    /// `awk: cmd. line:2: (FILENAME=f1 FNR=1) fatal: division by zero
    /// attempted` from an END after one line of `f1`; nothing after the colon
    /// in a BEGIN that has read nothing but its location.
    #[must_use]
    pub fn diagnostic_prefix(&self) -> Str {
        let mut out = match self.loc {
            Some(loc) => crate::source::prefix(&self.prog.sources, loc),
            None => Str::new(),
        };
        // gawk keeps FNR in a C `long`, truncated from whatever was assigned.
        let fnr = self.get_global(V_FNR).to_num().trunc();
        if fnr.is_finite() && fnr > 0.0 {
            out.extend_from_slice(b"(FILENAME=");
            out.extend_from_slice(&self.string_of(V_FILENAME));
            out.extend_from_slice(format!(" FNR={fnr:.0}) ").as_bytes());
        }
        out
    }

    /// Say a warning of the interpreter's own, placed as gawk's `warning()`
    /// places it.
    fn warning(&mut self, message: &[u8]) {
        self.flush_before_diagnostic();
        let mut line = b"awk: ".to_vec();
        line.extend_from_slice(&self.diagnostic_prefix());
        line.extend_from_slice(b"warning: ");
        line.extend_from_slice(message);
        line.push(b'\n');
        coreutils::stdfd::diag_bytes(&line);
    }

    /// Write whatever the escape layers have said since last time with no
    /// location: what gawk does for a `var=value` operand, whose escapes are
    /// resolved between records rather than by a statement (measured: `awk:
    /// warning: escape sequence ...`, even after a file has been read).
    fn say_warnings_unplaced(&mut self) {
        if self.warnings.is_empty() {
            return;
        }
        self.flush_before_diagnostic();
        emit_warnings(&mut self.warnings, b"");
    }

    /// Set a variable named on the command line by `-v` or as `var=value`.
    ///
    /// The value is a strnum, so `-v n=10` compares numerically — which is what
    /// makes `awk -v n=10 '$1 == n'` work on a numeric column. Its escapes are
    /// already resolved: [`escape::string`] is the caller's, since `-v` is
    /// resolved before the program is even parsed.
    ///
    /// # Errors
    /// Returns a diagnostic if the name is one of the built-in arrays, or if
    /// the variable is `FS` or `RS` and its value will not compile as a regex.
    pub fn assign_cli(&mut self, name: &str, value: Str) -> Result<(), String> {
        let Some(slot) = self.prog.global_names.iter().position(|n| n == name) else {
            // A name the program never mentions still has to be settable: a
            // script that reads `-v debug=1` only in a branch it does not have
            // must not fail, and a later `-f` file might use it.
            return Ok(());
        };
        if matches!(self.globals.get(slot), Some(Cell::Arr(_))) {
            return Err(format!(
                "{name} is an array and cannot be assigned on the command line"
            ));
        }
        // Through `set_var`, not `set_global`: `-F:` and `-v RS=;` have to take
        // effect, and it is `set_var` that recompiles the splitter when `FS` or
        // `RS` changes. Writing the slot directly stored the new value where
        // nothing would read it until the next assignment.
        self.set_var(VarRef::Global(slot), Value::from_input(value))
            .map_err(|e| e.0)?;
        Ok(())
    }

    /// Run the whole program, returning the process exit status.
    ///
    /// # Errors
    /// Returns the fatal diagnostic; the caller prints it and exits 2.
    ///
    /// # What the program already printed is printed, fatal or not
    ///
    /// The buffered output is flushed on *every* way out of here, because a
    /// fatal error does not retract the lines that were written before it.
    /// `awk '{print}' good.txt missing.txt` prints `good.txt` and then fails;
    /// so does any program that prints for an hour and then divides by zero.
    ///
    /// It did not, and the bug was invisible for as long as it existed. The
    /// body below is one `?`-chain, so a fatal skipped the `finish_all` at the
    /// end of it; `die` then reached `process::exit`, which does not run
    /// destructors, so the `BufWriter` holding an entire run's output was
    /// dropped without being written. Nothing reported it — the output simply
    /// was not there. Measured against gawk, which prints it.
    ///
    /// The old harness could not have caught this: it compared stdout through
    /// `$(...)` for cases that were all a single line, and it had no file
    /// operands at all, so the two-file case that shows it plainly could not
    /// be written.
    pub fn run(&mut self) -> R<i32> {
        let outcome = self.run_to_end();
        let flushed = self
            .out
            .finish_all()
            .map_err(|e| Fatal(format!("write error: {}", coreutils::errmsg::strerror(&e))));
        match outcome {
            // The program's own failure is the cause and wins the report; a
            // flush that also failed is a consequence of the same full disk.
            Err(e) => Err(e),
            Ok(code) => {
                flushed?;
                Ok(code)
            }
        }
    }

    /// The program itself, without the flush that has to happen either way.
    fn run_to_end(&mut self) -> R<i32> {
        let flow = self.run_code(self.code.begin, Ctx::Begin)?;

        let needs_input = !self.prog.rules.is_empty() || !self.prog.end.is_empty();
        if !matches!(flow, Flow::Exit) && needs_input {
            self.main_loop()?;
        }

        // POSIX: `exit` in BEGIN or in a rule still runs END -- that is what
        // lets a program bail out early and still print its totals. An `exit`
        // inside END ends END.
        self.run_code(self.code.end, Ctx::End)?;
        Ok(self.exit_code.unwrap_or(0))
    }

    fn main_loop(&mut self) -> R<()> {
        loop {
            let rec = match self.next_main_record()? {
                MainRead::Record(r) => r,
                MainRead::Eof => return Ok(()),
                MainRead::Failed(e) => {
                    return Err(Fatal(format!(
                        "fatal: error reading input file `{}': {}",
                        String::from_utf8_lossy(&self.string_of(V_FILENAME)),
                        coreutils::errmsg::strerror(&e)
                    )));
                }
            };
            self.bump(V_NR);
            self.bump(V_FNR);
            self.set_record(rec);
            match self.run_code(self.code.rules, Ctx::Rule)? {
                Flow::Exit => return Ok(()),
                Flow::NextFile => self.main.current = None,
                Flow::Normal | Flow::Next => {}
            }
        }
    }

    // ---- the main input ---------------------------------------------------

    /// The next record of the main input, moving on through `ARGV` as each
    /// file ends.
    ///
    /// # Errors
    /// gawk's fatal error for a file that will not open, wherever the read
    /// came from -- the main loop or a plain `getline`.
    fn next_main_record(&mut self) -> R<MainRead> {
        loop {
            if let Some(reader) = self.main.current.as_mut() {
                let rs = self.rs.clone();
                match reader.next(&rs) {
                    Ok(Some(rec)) => return Ok(MainRead::Record(rec)),
                    Ok(None) => self.main.current = None,
                    Err(e) => return Ok(MainRead::Failed(e)),
                }
            }
            if !self.open_next_input()? {
                return Ok(MainRead::Eof);
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
                // gawk names standard input `-` here, `--posix` or not:
                // `echo x | gawk --posix '{print FILENAME}'` prints `-`
                // (measured), and so does its diagnostics' `(FILENAME=- FNR=1)`.
                self.set_global(V_FILENAME, Value::str(b"-".to_vec()));
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
                // gawk's `arg_assign`: escapes resolved with `ELIDE_BACK_NL`,
                // the same as `-v`.
                let value = escape::string(&value, true, &mut self.warnings);
                self.say_warnings_unplaced();
                // A refusal (an array named, or an `FS` that will not compile)
                // is gawk's fatal error here too.
                self.assign_cli(&name, value).map_err(Fatal)?;
                continue;
            }
            self.main.opened_any = true;
            self.set_global(V_FILENAME, Value::str(text.as_ref().clone()));
            self.set_global(V_FNR, Value::Num(0.0));
            let src: Box<dyn Read> = if text.as_ref() == b"-" || text.as_ref() == b"/dev/stdin" {
                self.main.stdin_used = true;
                Box::new(std::io::stdin())
            } else {
                match open_input(&text) {
                    Ok(f) => Box::new(f),
                    Err(e) => {
                        // An input file that cannot be opened stops the run. It
                        // is tempting to skip it and carry on, but an awk
                        // program is usually computing a total over the files it
                        // was given, and a total that silently omits one of them
                        // is worse than no answer at all.
                        //
                        // The wording is gawk's, quotes and all: this is not a
                        // rendering of a parser's internals but a plain report
                        // about a file, and a script that greps awk's stderr
                        // should not have to know which awk it got.
                        return Err(Fatal(format!(
                            "fatal: cannot open file `{}' for reading: {}",
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

    // ---- variables and fields --------------------------------------------

    fn get_var(&mut self, v: VarRef) -> R<Value> {
        if v == VarRef::Global(V_NF) {
            self.ensure_split()?;
            let n = self.f.fields.len();
            return Ok(Value::Num(f64::from(u32::try_from(n).unwrap_or(u32::MAX))));
        }
        Ok(match self.cell(v) {
            Some(Cell::Val(val)) => val.clone(),
            // Using an array where a scalar was wanted is a program bug, but it
            // is not worth killing the run over: the empty value is what an
            // unset variable gives and it keeps the diagnostic in one place.
            _ => Value::Uninit,
        })
    }

    fn set_var(&mut self, v: VarRef, val: Value) -> R<()> {
        if let VarRef::Global(slot) = v {
            match slot {
                V_NF => {
                    self.ensure_split()?;
                    // gawk's `set_NF`: the count as a C `long`, and a negative
                    // one refused rather than read as 0.
                    let n = c_long(val.to_num());
                    if n < 0 {
                        return Err(Fatal("fatal: NF set to negative value".to_string()));
                    }
                    let n = usize::try_from(n).unwrap_or(usize::MAX).min(16_000_000);
                    self.f.fields.resize(n, Str::new());
                    self.f.record_valid = false;
                    self.f.split_valid = true;
                    return Ok(());
                }
                V_FS | V_RS => {
                    self.set_global(slot, val);
                    return self.refresh_separators();
                }
                _ => {}
            }
        }
        self.set_cell(v, Cell::Val(val));
        Ok(())
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
            VarRef::Local(s) => self.frames.last().and_then(|f| f.locals.get(s)),
        }
    }

    fn set_cell(&mut self, v: VarRef, c: Cell) {
        let slot = match v {
            VarRef::Global(s) => self.globals.get_mut(s),
            VarRef::Local(s) => self.frames.last_mut().and_then(|f| f.locals.get_mut(s)),
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

    fn get_field(&mut self, n: usize) -> R<Value> {
        if n == 0 {
            return Ok(Value::from_input(self.record().clone()));
        }
        self.ensure_split()?;
        Ok(match self.f.fields.get(n.saturating_sub(1)) {
            Some(s) => Value::from_input(s.clone()),
            // Past NF is the empty string, and reading it does not extend the
            // record — only assigning does.
            None => Value::Uninit,
        })
    }

    fn set_field(&mut self, n: usize, s: Str) -> R<()> {
        if n == 0 {
            self.set_record(s);
            return Ok(());
        }
        self.ensure_split()?;
        if self.f.fields.len() < n {
            self.f.fields.resize(n, Str::new());
        }
        if let Some(slot) = self.f.fields.get_mut(n.saturating_sub(1)) {
            *slot = s;
        }
        self.f.record_valid = false;
        Ok(())
    }

    fn set_record(&mut self, rec: Str) {
        self.f.record = rec;
        self.f.record_valid = true;
        self.f.split_valid = false;
    }

    fn ensure_split(&mut self) -> R<()> {
        if self.f.split_valid {
            return Ok(());
        }
        let rec = self.record().clone();
        self.f.fields = self.split_record(&rec)?;
        self.f.split_valid = true;
        Ok(())
    }

    /// Split a record into fields by the current `FS`.
    ///
    /// Fallible because `FS` is a regular expression the program chose, and one
    /// containing a backreference can exhaust the matcher's budget. Splitting
    /// on "as much as we managed to match" would silently misnumber every
    /// field, so the run stops instead.
    fn split_record(&self, rec: &[u8]) -> R<Vec<Str>> {
        // In paragraph mode a newline separates fields whatever FS says, which
        // is what makes `RS=""` useful for stanza-structured files.
        let paragraph = matches!(self.rs, Rs::Paragraph);
        split_with(&self.fs, rec, paragraph)
    }

    /// Rebuild `FS` and `RS` from their variables. Called after either is
    /// assigned, and cheap enough to be called when neither changed.
    ///
    /// # Errors
    /// gawk's fatal error for a separator that will not compile as a regex.
    fn refresh_separators(&mut self) -> R<()> {
        let fs = self.string_of(V_FS);
        if fs != self.fs_src {
            self.fs = self.make_fs(&fs)?;
            self.fs_src = fs;
            // The fields were split by the old FS; POSIX says a change to FS
            // takes effect on the *next* record, so the current split stands.
        }
        let rs = self.string_of(V_RS);
        if rs != self.rs_src {
            self.rs = self.make_rs(&rs)?;
            self.rs_src = rs;
        }
        Ok(())
    }

    /// Build the field splitter `FS` describes.
    ///
    /// The single-character case is *literal*, not a one-character regex: POSIX
    /// says so, and it is why `FS = "."` splits on dots rather than on
    /// everything. Anything longer is a regex, through gawk's `make_regexp` --
    /// escape layer included, so `FS = "\\|"` splits on a bar -- and one that
    /// will not compile is gawk's fatal error: `FS = "a("` stops the run with
    /// `invalid regexp: Unmatched ( or \(: /a(/`, measured. (It used to fall
    /// back to splitting on the first character, which answered a question
    /// the program had not asked.)
    fn make_fs(&mut self, fs: &[u8]) -> R<Fs> {
        Ok(match fs {
            b" " => Fs::Whitespace,
            b"" => Fs::Chars,
            &[c] => Fs::Char(c),
            other => Fs::Regex(Rc::new(self.dynamic_regex(other)?)),
        })
    }

    /// Build the record reader `RS` describes: empty for paragraphs, one
    /// character literally, anything longer a regex as [`Self::make_fs`]'s is.
    fn make_rs(&mut self, rs: &[u8]) -> R<Rs> {
        Ok(match rs {
            b"" => Rs::Paragraph,
            &[c] => Rs::Char(c),
            other => Rs::Regex(Rc::new(self.dynamic_regex(other)?)),
        })
    }

    /// Compile a regex whose text was computed at run time, saying any
    /// escape warnings it earned.
    fn dynamic_regex(&mut self, text: &[u8]) -> R<Regex> {
        let compiled = crate::parse::compile_dynamic(text, &mut self.warnings);
        self.say_warnings();
        compiled.map_err(Fatal)
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
fn split_with(fs: &Fs, text: &[u8], paragraph_mode: bool) -> R<Vec<Str>> {
    Ok(match fs {
        Fs::Whitespace => text
            .split(|b| matches!(b, b' ' | b'\t' | b'\n'))
            .filter(|p| !p.is_empty())
            .map(<[u8]>::to_vec)
            .collect(),
        Fs::Chars => ch::chars(text).map(ch::Ch::to_str).collect(),
        Fs::Char(c) => {
            if text.is_empty() {
                return Ok(Vec::new());
            }
            if paragraph_mode {
                return Ok(text
                    .split(|b| b == c || *b == b'\n')
                    .map(<[u8]>::to_vec)
                    .collect());
            }
            text.split(|b| b == c).map(<[u8]>::to_vec).collect()
        }
        Fs::Regex(re) => {
            if text.is_empty() {
                return Ok(Vec::new());
            }
            let mut out = Vec::new();
            let mut last = 0usize;
            for span in re.find_iter(text) {
                let (s, e) = span?;
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
                return Ok(out
                    .into_iter()
                    .flat_map(|p| {
                        p.split(|b| *b == b'\n')
                            .map(<[u8]>::to_vec)
                            .collect::<Vec<_>>()
                    })
                    .collect());
            }
            out
        }
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

/// Open a main-input operand, refusing a directory as gawk does when it opens
/// one (`iop_alloc` checks), rather than letting the first read fail.
fn open_input(name: &[u8]) -> std::io::Result<std::fs::File> {
    let f = std::fs::File::open(crate::io::os_path(name))?;
    if f.metadata()?.is_dir() {
        return Err(std::io::Error::from(std::io::ErrorKind::IsADirectory));
    }
    Ok(f)
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
    let first_ok = name
        .first()
        .is_some_and(|c| *c == b'_' || c.is_ascii_alphabetic());
    if !first_ok || !name.iter().all(|c| *c == b'_' || c.is_ascii_alphanumeric()) {
        return None;
    }
    let value = arg.get(eq.saturating_add(1)..)?.to_vec();
    Some((String::from_utf8_lossy(name).into_owned(), value))
}

fn seconds_since_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}
