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
//! ## A field is one value per record
//!
//! Each field, and `$0`, is made into a [`Value`] the first time it is read
//! and the same value is handed out after, until the record or that field
//! changes -- gawk's one node per field. It matters because a value from input
//! remembers whether it has been used as a number (see `value.rs`), and a
//! program that tests `$1 > 0` and then stores `a[$1]` stores what gawk
//! stores.
//!
//! What a field holds is what was put there, as in gawk: a field split from
//! input is input (a strnum when it looks like a number), but `$2 = "10.0"`
//! makes `$2` the string `"10.0"` -- so `$2 == 10` is false -- and `$2 = 10`
//! the number. `$0` likewise: input when read, whatever was assigned when
//! assigned, and a plain string when rebuilt from its fields.
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
use crate::value::{FieldNode, Str, Value, c_long, num_to_str};
use ere::awk::{self as escape, Warnings};
use ere::{Regex, ch};
use run::{Ctx, ForIter, Frame, Item};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;

/// An error that stops the program. awk has no exceptions; every one of these
/// ends the run.
pub enum Fatal {
    /// gawk's `fatal:` and its kin: said after `awk: ` and where the program
    /// was, and the run ends with status 2. Bytes, because most of them name
    /// a file or a command, and gawk prints those with `%s`.
    Said(Str),
    /// Standard output's reader went away. gawk dies of `SIGPIPE` there; this
    /// system does not use signals for process control, so -- as everywhere
    /// in coreutils (`stdfd::reader_gone`) -- the run ends quietly, with the
    /// status it had already earned.
    ReaderGone,
}

impl Fatal {
    /// [`Fatal::Said`] from this file's own sentences, built as text.
    pub fn said(message: impl Into<Str>) -> Fatal {
        Fatal::Said(message.into())
    }
}

impl From<String> for Fatal {
    fn from(s: String) -> Fatal {
        Fatal::Said(s.into_bytes())
    }
}

impl From<Str> for Fatal {
    fn from(s: Str) -> Fatal {
        Fatal::Said(s)
    }
}

/// `head NAME tail`, the name as it is: gawk prints file and command names
/// with `%s`, and a name need not be text.
fn named(head: &str, name: &[u8], tail: &str) -> Str {
    let mut said = Vec::with_capacity(
        head.len()
            .saturating_add(name.len())
            .saturating_add(tail.len()),
    );
    said.extend_from_slice(head.as_bytes());
    said.extend_from_slice(name);
    said.extend_from_slice(tail.as_bytes());
    said
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
        Fatal::said(e.to_string())
    }
}

type R<T> = Result<T, Fatal>;

/// An awk array: shared, because arrays are passed to functions by reference.
/// Laid out as gawk lays them out, for gawk's `for (k in a)` order; see
/// [`crate::array`].
type Array = Rc<RefCell<crate::array::Array>>;

/// A new, empty array.
fn new_array() -> Array {
    Rc::new(RefCell::new(crate::array::Array::new()))
}

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
    /// `$0` as a value, once read or assigned: the one node every reader
    /// shares.
    record_value: Option<Value>,
    /// Whether `$0` came from input, which makes it input once read; a `$0`
    /// rebuilt from its fields is a plain string.
    record_input: bool,
    /// Each field as a value, once read; `None` until then. Kept the length
    /// of `fields`.
    field_values: Vec<Option<Value>>,
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
            record_value: None,
            record_input: true,
            field_values: Vec::new(),
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
    /// gawk's C `long`s behind `NR` and `FNR`. The variables keep whatever was
    /// assigned to them, but a read gives the long whenever the two disagree
    /// -- `NR = 2.5` reads back 2, `NR = "7x"` reads back `7x` -- and the
    /// counting is the long's (see [`Interp::counter`]).
    nr: i64,
    fnr: i64,
    /// Set by `exit`; also the process's status.
    exit_code: Option<i32>,
    /// An `exit` statement ran: gawk's `exiting`, which keeps an error writing
    /// standard output at exit from turning a 0 into a 1.
    exited: bool,
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
            nr: 0,
            fnr: 0,
            exit_code: None,
            exited: false,
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

        // ENVIRON is a string array from the start, whatever its first
        // name (gawk's `load_environ`), filled in the environment's order.
        let environ: Array = Rc::new(RefCell::new(crate::array::Array::new_str()));
        it.set_cell(VarRef::Global(V_ENVIRON), Cell::Arr(Rc::clone(&environ)));
        {
            let mut m = environ.borrow_mut();
            for (k, v) in env {
                *m.lookup(&Value::str(k.clone()), b"%.6g") = Value::from_input(v.clone());
            }
        }
        // ARGV's subscripts are numbers, so it is an integer array (gawk's
        // `init_args`).
        let argv_arr = it.array_slot(V_ARGV);
        {
            let mut m = argv_arr.borrow_mut();
            *m.lookup(&Value::Num(0.0), b"%.6g") = Value::str(b"awk".to_vec());
            for (i, a) in argv.iter().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                let k = Value::Num(i.saturating_add(1) as f64);
                *m.lookup(&k, b"%.6g") = Value::from_input(a.clone());
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
        // gawk tests its C `long` FNR, not the variable.
        if self.fnr > 0 {
            out.push(b'(');
            // `FILENAME=%.*s `, printed only when the value has a string at
            // all -- a number assigned to it has none (`(FNR=1)` alone) --
            // and, being C's `%s`, only up to a NUL.
            if let Some(name) = self.filename_shown() {
                out.extend_from_slice(b"FILENAME=");
                out.extend_from_slice(&name);
                out.push(b' ');
            }
            out.extend_from_slice(format!("FNR={}) ", self.fnr).as_bytes());
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

    /// `FILENAME` as gawk's `err()` prints it: nothing for a value that is
    /// only a number (gawk's `stptr` is NULL until something formats it),
    /// else the text up to its first NUL.
    fn filename_shown(&self) -> Option<Str> {
        let name = self.get_global(V_FILENAME);
        // Asked without settling anything: gawk reads the node's `stptr`.
        if !name.has_text() {
            return None;
        }
        let text = self.to_str(&name);
        let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
        Some(text.get(..end).unwrap_or_default().to_vec())
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

    /// A `var=value` operand, as gawk's `arg_assign` takes one: refused if
    /// the name is a keyword, a built-in or a function, or the value holds a
    /// newline; then the value's escapes, said where nothing is placed, and
    /// the assignment.
    ///
    /// # Errors
    /// gawk's fatal error for each refusal, and for an assignment that fails.
    fn assign_operand(&mut self, name: &str, raw: &[u8]) -> R<()> {
        // gawk forgets where it was for this: what it says has no line, and
        // the line stays forgotten until the next instruction sets it. And it
        // zeroes FNR for the whole of it, so nothing said has an input
        // position either; FNR comes back only if the assignment succeeds.
        self.loc = None;
        let saved = self.fnr;
        self.fnr = 0;
        if let Some(refusal) = cli_refusal(name, raw) {
            return Err(Fatal::said(refusal));
        }
        if self.prog.funcs.iter().any(|f| f.name == name) {
            return Err(Fatal::said(format!(
                "fatal: cannot use function `{name}' as variable name"
            )));
        }
        let value = escape::string(raw, true, &mut self.warnings);
        self.say_warnings_unplaced();
        self.assign_value(name, value)?;
        self.fnr = saved;
        Ok(())
    }

    /// Set a variable named by `-v` (or `-F`), its name and value already
    /// checked and its escapes already resolved before the program was
    /// parsed, as gawk does it.
    ///
    /// gawk zeroes `FNR` around the assignment and restores it after, so a
    /// failure is said with no input position, and an assignment *to* `FNR`
    /// is undone: `-v FNR=2.5` leaves BEGIN reading 0, as `awk 'END { print
    /// FNR }' f FNR=10` reads `f`'s count. Measured.
    ///
    /// # Errors
    /// gawk's fatal error for a name that is an array, or a variable whose
    /// assignment fails (`NF` negative, `FS` not compiling).
    pub fn assign_cli(&mut self, name: &str, value: Str) -> R<()> {
        self.loc = None;
        let saved = self.fnr;
        self.fnr = 0;
        self.assign_value(name, value)?;
        self.fnr = saved;
        Ok(())
    }

    /// The assignment itself: refused for a name that holds an array, else
    /// through `set_var` -- a strnum, so `-v n=10` compares numerically, which
    /// is what makes `awk -v n=10 '$1 == n'` work on a numeric column. Already
    /// looked at as a number, as gawk's `arg_assign` leaves it.
    fn assign_value(&mut self, name: &str, value: Str) -> R<()> {
        let Some(slot) = self.prog.global_names.iter().position(|n| n == name) else {
            // A name the program never mentions still has to be settable: a
            // script that reads `-v debug=1` only in a branch it does not have
            // must not fail, and a later `-f` file might use it.
            return Ok(());
        };
        if matches!(self.globals.get(slot), Some(Cell::Arr(_))) {
            return Err(Fatal::said(format!(
                "fatal: attempt to use array `{name}' in a scalar context"
            )));
        }
        // Through `set_var`, not `set_global`: `-F:` and `-v RS=;` have to take
        // effect, and it is `set_var` that recompiles the splitter when `FS` or
        // `RS` changes.
        self.set_var(VarRef::Global(slot), Value::from_assignment(value))
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
    /// (It did not, once: the body was one `?`-chain that skipped its flush on
    /// the error path, and `process::exit` runs no destructors. Measured
    /// against gawk, which prints it.)
    ///
    /// # Exit is gawk's `Op_atexit`
    ///
    /// With the program done, gawk forgets where it was (its line, not its
    /// input position), closes every redirection -- one that will not flush is
    /// a fatal error, `flush to "f" failed` -- and then flushes standard
    /// output, where a failure is only a warning: `error writing standard
    /// output`, and status 1 unless the program said `exit`. After a fatal
    /// error none of that is said; C's `exit` flushes what it can, quietly.
    pub fn run(&mut self) -> R<i32> {
        let outcome = self.run_to_end();
        match outcome {
            Ok(code) => {
                self.loc = None;
                if let Err((name, e)) = self.out.finish_redirects() {
                    self.out.finish_quietly();
                    return Err(Fatal::Said(named(
                        "fatal: flush to \"",
                        &name,
                        &format!("\" failed: {}", coreutils::errmsg::strerror(&e)),
                    )));
                }
                match self.out.finish_stdout() {
                    Ok(()) => Ok(code),
                    Err(e) if coreutils::stdfd::reader_gone(&e) => Err(Fatal::ReaderGone),
                    Err(e) => {
                        let message = format!(
                            "error writing standard output: {}",
                            coreutils::errmsg::strerror(&e)
                        );
                        self.warning(message.as_bytes());
                        Ok(if code == 0 && !self.exited { 1 } else { code })
                    }
                }
            }
            Err(e) => {
                self.out.finish_quietly();
                Err(e)
            }
        }
    }

    /// The status the run had earned: what `exit` said, or 0.
    #[must_use]
    pub fn exit_status(&self) -> i32 {
        self.exit_code.unwrap_or(0)
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
                    return Err(Fatal::Said(named(
                        "fatal: error reading input file `",
                        &self.string_of(V_FILENAME),
                        &format!("': {}", coreutils::errmsg::strerror(&e)),
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
                self.set_counter(V_FNR, 0);
                return Ok(true);
            }
            #[allow(clippy::cast_precision_loss)]
            let key = Value::Num(self.main.argv as f64);
            self.main.argv = self.main.argv.saturating_add(1);
            let arg = {
                let arr = self.array_slot(V_ARGV);
                let convfmt = self.convfmt();
                let v = arr.borrow().get(&key, &convfmt).cloned();
                v.unwrap_or(Value::Uninit)
            };
            let text = self.to_str(&arg);
            if text.is_empty() {
                // An emptied ARGV entry is skipped — that is how a program
                // removes a file from the list.
                continue;
            }
            if let Some((name, value)) = command_assignment(&text) {
                self.assign_operand(&name, &value)?;
                continue;
            }
            self.main.opened_any = true;
            self.set_global(V_FILENAME, Value::str(text.as_ref().clone()));
            self.set_counter(V_FNR, 0);
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
                        return Err(Fatal::Said(named(
                            "fatal: cannot open file `",
                            &text,
                            &format!("' for reading: {}", coreutils::errmsg::strerror(&e)),
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
        self.emit(&line, target, "print")
    }

    fn printf_values(&mut self, vals: &[Value], target: Option<(RedirMode, Str)>) -> R<()> {
        let Some(fmt) = vals.first() else {
            return Err(Fatal::said("printf: no format string".to_string()));
        };
        let fmt = self.to_str(fmt);
        let convfmt = self.string_of(V_CONVFMT);
        let rest = vals.get(1..).unwrap_or_default();
        let text = crate::fmt::sprintf(&fmt, rest, &convfmt).map_err(Fatal::said)?;
        self.emit(&text, target, "printf")
    }

    /// Write a print's output where it goes, failing as gawk's `efwrite`
    /// fails: `from` is the statement, `print` or `printf`, which gawk names.
    fn emit(&mut self, bytes: &[u8], target: Option<(RedirMode, Str)>, from: &str) -> R<()> {
        let Some((mode, name)) = target else {
            return match self.out.write_stdout(bytes) {
                Ok(()) => Ok(()),
                Err(e) if coreutils::stdfd::reader_gone(&e) => Err(Fatal::ReaderGone),
                Err(e) => Err(Fatal::said(format!(
                    "fatal: {from} to \"standard output\" failed: {}",
                    coreutils::errmsg::strerror(&e)
                ))),
            };
        };
        if name.is_empty() {
            return Err(null_redirect(redirect_symbol(mode)));
        }
        match self.out.write_to(&name, mode, bytes) {
            Ok(()) => Ok(()),
            Err(crate::io::OutError::Open(e)) => {
                let why = coreutils::errmsg::strerror(&e);
                Err(Fatal::Said(match mode {
                    RedirMode::Pipe => named(
                        "fatal: cannot open pipe `",
                        &name,
                        &format!("' for output: {why}"),
                    ),
                    RedirMode::Truncate | RedirMode::Append => {
                        named("fatal: cannot redirect to `", &name, &format!("': {why}"))
                    }
                }))
            }
            // A write that reached standard output through `/dev/stdout`
            // meets its reader going away as standard output does.
            Err(crate::io::OutError::Write(e))
                if coreutils::stdfd::reader_gone(&e)
                    && (name.as_slice() == b"/dev/stdout" || name.as_slice() == b"-") =>
            {
                Err(Fatal::ReaderGone)
            }
            Err(crate::io::OutError::Write(e)) => Err(Fatal::Said(named(
                &format!("fatal: {from} to \""),
                &name,
                &format!("\" failed: {}", coreutils::errmsg::strerror(&e)),
            ))),
        }
    }

    /// gawk's `flush_io`: standard output, then every redirection, each
    /// failure fatal as gawk words it. What `fflush()` is, and what `system`
    /// does before it runs anything.
    fn flush_io(&mut self) -> R<()> {
        match self.out.flush_all() {
            Ok(()) => Ok(()),
            Err(crate::io::FlushError::Stdout(e)) if coreutils::stdfd::reader_gone(&e) => {
                Err(Fatal::ReaderGone)
            }
            Err(crate::io::FlushError::Stdout(e)) => Err(Fatal::said(format!(
                "fatal: fflush: cannot flush standard output: {}",
                coreutils::errmsg::strerror(&e)
            ))),
            Err(crate::io::FlushError::Sink { name, pipe, err }) => Err(Fatal::Said(named(
                &format!("fatal: {} flush of `", if pipe { "pipe" } else { "file" }),
                &name,
                &format!("' failed: {}", coreutils::errmsg::strerror(&err)),
            ))),
        }
    }

    // ---- variables and fields --------------------------------------------

    fn get_var(&mut self, v: VarRef) -> R<Value> {
        if let VarRef::Global(slot @ (V_NR | V_FNR)) = v {
            return Ok(self.counter(slot));
        }
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
        // A variable keeps its own copy of a field (gawk's `UNFIELD`).
        let val = val.unfield();
        if let VarRef::Global(slot) = v {
            match slot {
                V_NF => {
                    self.ensure_split()?;
                    // gawk's `set_NF`: the count as a C `long`, and a negative
                    // one refused rather than read as 0.
                    let n = c_long(val.to_num());
                    if n < 0 {
                        return Err(Fatal::said("fatal: NF set to negative value".to_string()));
                    }
                    let n = usize::try_from(n).unwrap_or(usize::MAX).min(16_000_000);
                    self.f.fields.resize(n, Str::new());
                    self.f.field_values.resize(n, None);
                    self.f.record_valid = false;
                    self.f.record_value = None;
                    self.f.split_valid = true;
                    return Ok(());
                }
                V_FS | V_RS => {
                    self.set_global(slot, val);
                    return self.refresh_separators();
                }
                // gawk's `set_OFS`, `set_ORS`, `set_SUBSEP`, `set_CONVFMT`
                // and `set_OFMT` take the new value's text at once
                // (`force_string`), which settles an index from `for (k in
                // a)` into a string there and then. `set_OFS` first rebuilds
                // a `$0` whose fields have changed, with the OFS it is
                // replacing: `$3 = "x"; OFS = "-"; print` prints `a b x`.
                V_OFS | V_ORS | V_SUBSEP | V_CONVFMT | V_OFMT => {
                    if slot == V_OFS {
                        self.rebuild_record();
                    }
                    let _ = self.to_str(&val);
                }
                V_NR | V_FNR => {
                    // gawk's `set_NR`: the long is the value's, truncated.
                    let n = c_long(val.to_num());
                    self.set_global(slot, val);
                    if slot == V_NR {
                        self.nr = n;
                    } else {
                        self.fnr = n;
                    }
                    return Ok(());
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

    /// Count one more record in `NR` or `FNR`: the long, as gawk counts.
    fn bump(&mut self, slot: usize) {
        let n = if slot == V_NR { self.nr } else { self.fnr }.wrapping_add(1);
        self.set_counter(slot, n);
    }

    /// Set `NR` or `FNR` to a count, the variable and the long both.
    fn set_counter(&mut self, slot: usize, n: i64) {
        if slot == V_NR {
            self.nr = n;
        } else {
            self.fnr = n;
        }
        #[allow(clippy::cast_precision_loss)]
        self.set_global(slot, Value::Num(n as f64));
    }

    /// `NR` or `FNR` as a read gives it: gawk's `update_NR` replaces the
    /// variable's value with the long whenever the two disagree, and keeps
    /// what was assigned when they agree -- a string included.
    fn counter(&self, slot: usize) -> Value {
        let long = if slot == V_NR { self.nr } else { self.fnr };
        let stored = self.get_global(slot);
        #[allow(clippy::cast_precision_loss, clippy::float_cmp)]
        if stored.to_num() == long as f64 {
            stored
        } else {
            Value::Num(long as f64)
        }
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

    /// The array a program names, refusing a global the command line made a
    /// scalar first: `-v a=1` with `a[1]` in the program is gawk's `attempt to
    /// use scalar `a' as an array`, said where the array is used.
    fn array_of(&mut self, v: VarRef) -> R<Array> {
        if let VarRef::Global(slot) = v
            && let Some(Cell::Val(val)) = self.globals.get(slot)
            && !matches!(val, Value::Uninit)
        {
            let name = self.prog.global_names.get(slot).map_or("?", String::as_str);
            return Err(Fatal::said(format!(
                "fatal: attempt to use scalar `{name}' as an array"
            )));
        }
        Ok(self.array_ref(v))
    }

    /// The array in a slot, creating it if the slot is still untouched.
    fn array_ref(&mut self, v: VarRef) -> Array {
        if let Some(Cell::Arr(a)) = self.cell(v) {
            return Rc::clone(a);
        }
        let a = new_array();
        self.set_cell(v, Cell::Arr(Rc::clone(&a)));
        a
    }

    fn array_slot(&mut self, slot: usize) -> Array {
        self.array_ref(VarRef::Global(slot))
    }

    /// `$n`: the field's value, made on first reading and the same value
    /// after, so that what has been done to it shows (see the module docs).
    fn get_field(&mut self, n: usize) -> R<Value> {
        if n == 0 {
            self.rebuild_record();
            let f = &mut self.f;
            let v = f.record_value.get_or_insert_with(|| {
                if f.record_input {
                    Value::from_field(f.record.clone(), FieldNode::Record)
                } else {
                    Value::str(f.record.clone())
                }
            });
            return Ok(v.clone());
        }
        self.ensure_split()?;
        let i = n.saturating_sub(1);
        let Some(text) = self.f.fields.get(i) else {
            // Past NF is the empty string, and reading it does not extend the
            // record — only assigning does.
            return Ok(Value::Uninit);
        };
        if self.f.field_values.len() != self.f.fields.len() {
            self.f.field_values.resize(self.f.fields.len(), None);
        }
        let make = || Value::from_field(text.clone(), FieldNode::Field);
        let v = match self.f.field_values.get_mut(i) {
            Some(slot) => slot.get_or_insert_with(make).clone(),
            None => make(),
        };
        Ok(v)
    }

    /// `$n = v`: gawk's `Op_store_field`. The field holds `v` itself -- a
    /// copy, if `v` is another field (`UNFIELD`) -- and its text; `$0` is to
    /// be rebuilt. `$0 = v` makes `v` the record, to be split again.
    fn set_field(&mut self, n: usize, v: Value) -> R<()> {
        let v = v.unfield();
        let text = self.to_str(&v).as_ref().clone();
        if n == 0 {
            self.set_record(text);
            self.f.record_value = Some(v);
            self.f.record_input = false;
            return Ok(());
        }
        self.ensure_split()?;
        if self.f.fields.len() < n {
            self.f.fields.resize(n, Str::new());
        }
        self.f.field_values.resize(self.f.fields.len(), None);
        if let Some(slot) = self.f.fields.get_mut(n.saturating_sub(1)) {
            *slot = text;
        }
        if let Some(slot) = self.f.field_values.get_mut(n.saturating_sub(1)) {
            *slot = Some(v);
        }
        self.f.record_valid = false;
        self.f.record_value = None;
        Ok(())
    }

    /// A record read from input: `$0` and, once split, its fields are input.
    fn set_record(&mut self, rec: Str) {
        self.f.record = rec;
        self.f.record_value = None;
        self.f.record_input = true;
        self.f.record_valid = true;
        self.f.split_valid = false;
    }

    fn ensure_split(&mut self) -> R<()> {
        if self.f.split_valid {
            return Ok(());
        }
        let rec = self.record().clone();
        self.f.fields = self.split_record(&rec)?;
        self.f.field_values.clear();
        self.f.field_values.resize(self.f.fields.len(), None);
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
        compiled.map_err(Fatal::Said)
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
        v.to_str(&self.convfmt())
    }

    /// `CONVFMT`'s text, which turns a number into a string.
    fn convfmt(&self) -> Rc<Str> {
        match self.globals.get(V_CONVFMT) {
            Some(Cell::Val(v)) => v.to_str(b"%.6g"),
            _ => Rc::new(b"%.6g".to_vec()),
        }
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
    /// purpose: gawk rebuilds when `$0` is next *read*, with the `OFS` then in
    /// force -- or, if `OFS` is assigned first, with the `OFS` it replaces
    /// (see `set_var`).
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
        let convfmt = self.convfmt();
        let mut out = Str::new();
        for (i, f) in self.f.fields.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&ofs);
            }
            // A number assigned to a field takes its text now, with the
            // CONVFMT in force now, as gawk's `force_string` re-formats a
            // number whose format has changed since.
            match self.f.field_values.get(i) {
                Some(Some(v @ Value::Num(_))) => out.extend_from_slice(&v.to_str(&convfmt)),
                _ => out.extend_from_slice(f),
            }
        }
        self.f.record = out;
        // Rebuilt, `$0` is a string: gawk makes it with `make_str_node`.
        self.f.record_value = None;
        self.f.record_input = false;
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

/// How gawk writes a redirection's operator in a diagnostic.
fn redirect_symbol(mode: RedirMode) -> &'static str {
    match mode {
        RedirMode::Truncate => ">",
        RedirMode::Append => ">>",
        RedirMode::Pipe => "|",
    }
}

/// gawk's refusal of a redirection to or from the empty string, which
/// `redirect_string` makes before it tries anything.
fn null_redirect(symbol: &str) -> Fatal {
    Fatal::said(format!(
        "fatal: expression for `{symbol}' redirection has null string value"
    ))
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

/// The names gawk refuses as a command-line variable under `--posix`
/// (`check_special`): its keywords and built-in functions, less those marked
/// as extensions (`func` is an ordinary name under `--posix`). `eval` is on the
/// list because gawk's table does not mark it.
const RESERVED: &[&str] = &[
    "BEGIN", "END", "atan2", "break", "close", "continue", "cos", "delete", "do", "else", "eval",
    "exit", "exp", "fflush", "for", "function", "getline", "gsub", "if", "in", "index", "int",
    "length", "log", "match", "next", "nextfile", "print", "printf", "rand", "return", "sin",
    "split", "sprintf", "sqrt", "srand", "sub", "substr", "system", "tolower", "toupper", "while",
];

/// What gawk's `arg_assign` refuses about a command-line assignment before it
/// looks at the program -- a reserved name, a newline in the value -- worded
/// as gawk words it; `None` if nothing.
#[must_use]
pub fn cli_refusal(name: &str, raw: &[u8]) -> Option<String> {
    if RESERVED.contains(&name) {
        return Some(format!(
            "fatal: cannot use gawk builtin `{name}' as variable name"
        ));
    }
    // POSIX allows no newline inside a string; the lexer says so for the
    // program text, and this for the command line.
    if raw.contains(&b'\n') {
        return Some("fatal: POSIX does not allow physical newlines in string values".to_string());
    }
    None
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
    // Checked above to be an ASCII identifier, so the decode cannot fail.
    Some((std::str::from_utf8(name).ok()?.to_string(), value))
}

fn seconds_since_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// gawk's `arg_assign` refusals that need no program: a reserved name
    /// (`check_special`, POSIX names only) and a newline in the value.
    #[test]
    fn a_command_line_assignment_is_refused_as_gawk_refuses_it() {
        assert_eq!(
            cli_refusal("if", b"1").as_deref(),
            Some("fatal: cannot use gawk builtin `if' as variable name")
        );
        assert_eq!(
            cli_refusal("length", b"1").as_deref(),
            Some("fatal: cannot use gawk builtin `length' as variable name")
        );
        // An extension is an ordinary name under `--posix`.
        assert_eq!(cli_refusal("func", b"1"), None);
        assert_eq!(cli_refusal("gensub", b"1"), None);
        assert_eq!(
            cli_refusal("x", b"a\nb").as_deref(),
            Some("fatal: POSIX does not allow physical newlines in string values")
        );
        assert_eq!(cli_refusal("x", b"a b"), None);
    }

    /// A `var=value` operand is one only with an identifier before the `=`;
    /// anything else is a file name.
    #[test]
    fn an_operand_is_an_assignment_only_with_a_name() {
        assert_eq!(
            command_assignment(b"x=1"),
            Some(("x".to_string(), b"1".to_vec()))
        );
        assert_eq!(
            command_assignment(b"_a1=="),
            Some(("_a1".to_string(), b"=".to_vec()))
        );
        assert_eq!(command_assignment(b"1x=1"), None);
        assert_eq!(
            command_assignment(b"file=1.txt/x"),
            Some(("file".to_string(), b"1.txt/x".to_vec()))
        );
        assert_eq!(command_assignment(b"=1"), None);
        assert_eq!(command_assignment(b"a.txt"), None);
    }
}
