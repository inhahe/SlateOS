//! GNU bc 1.07.1, ported, as Ubuntu 24.04 builds it: the arbitrary-precision
//! calculator language, over [`bcnumber`] -- bc's own `number.c` -- for its
//! arithmetic.
//!
//! The program is coreutils' `bc` (`userspace/coreutils/src/bin/bc.rs`,
//! upstream's `main.c`), coreutils being the one home of the command-line
//! tools (design-decisions §1005); this library is the rest of upstream's
//! source, everything `main` calls.
//!
//! Upstream compiles each line to a byte code and runs it, and the port does
//! the same, file for file:
//!
//! | Upstream | Here |
//! |---|---|
//! | `scan.l` (flex) | `scan`: the same rules, matched as flex matches them -- the longest match, the earlier rule on a tie |
//! | `bc.y`, `bc.c` (Bison) | `parse`: Bison's parser, driven by `bc.c`'s own tables ([`tables`]), with `bc.y`'s actions |
//! | `load.c`, `util.c` | `code`: the code generator, the loader, names and arguments |
//! | `execute.c`, `storage.c` | `exec`: the machine, its variables, arrays and functions |
//! | `libmath.h` | `libmath`: the `-l` library as upstream's build compiles it |
//! | glibc's `random()` | `random` |
//!
//! Driving the parse with Bison's tables rather than a parser of its own is
//! what makes a syntax error answer as upstream's does: which tokens are
//! read before the error is seen -- and so the line it names -- and how the
//! parse recovers, are the tables' and the skeleton's.
//!
//! # Where this differs from upstream, on purpose
//!
//! - **No line editing.** Ubuntu's bc reads a terminal through readline;
//!   this reads it line by line, without editing or history. (`history`
//!   is still the variable readline's build has.)
//! - **A file name in a diagnostic is escaped** where it holds a byte that
//!   is not printable (`quoting::escape_unprintable`).
//!
//! # Upstream's faults, kept
//!
//! Each is what Ubuntu's bc does, measured; a program written against it
//! meets the same.
//!
//! - **`read()` at the end of its input never returns**: upstream's reader
//!   takes the end for a blank and skips blanks.
//! - **The first character after a constant's point is a digit**, whatever
//!   it is: `read()` of `1.` and a newline reads the newline as the base's
//!   largest digit and goes on to the next line.
//! - **A parameter given the wrong kind of argument** (an array for a
//!   value, or the reverse) is named by the next variable's name (or
//!   `(null)`), from upstream's names table being one off; and the call then
//!   ends in "Parameter number mismatch", or -- when an argument follows it
//!   -- dies of `SIGSEGV`, its buffered output unwritten, where upstream
//!   follows a pointer past the list.
//! - **Number output errors say `dc:`** (`dc: could not write output
//!   file`): the message is `number.c`'s, which bc shares with dc.

pub mod tables;

mod code;
mod exec;
mod libmath;
mod parse;
mod random;
mod scan;

use std::collections::{BTreeMap, VecDeque};

use bcnumber::Num;

pub use cstdio as io;
pub use exec::{on_interrupt, use_quit};

use crate::io::{Input, Output};

/// `MAX_STORE`: how many variables, arrays or functions there may be.
const MAX_STORE: i32 = 32767;
/// `BC_DIM_MAX`: the largest array index.
const BC_DIM_MAX: i64 = 16_777_215;
/// `BC_BASE_MAX`, `BC_SCALE_MAX`, `BC_STRING_MAX`: `INT_MAX`.
const BC_INT_MAX: i32 = i32::MAX;

/// One argument, parameter or auto: `arg_list`'s node. A list is kept in
/// upstream's order, its head first -- the most recently added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Arg {
    /// `av_name`: a variable's number, an array's negated -- or, in a call's
    /// list, 0 for a value and 1 for an array.
    name: i32,
    /// `arg_is_var`: an array passed by reference (`*a[]`).
    is_var: bool,
}

/// `bc_function`.
#[derive(Clone, Debug, Default)]
struct Function {
    defined: bool,
    void: bool,
    /// `f_body`: every byte ever loaded, the current code first -- a
    /// redefinition leaves the old code's tail past the new code's end,
    /// where a read that runs on (a constant's first fraction digit) finds
    /// it, as upstream's does.
    body: Vec<u8>,
    /// `f_code_size`: how much of `body` is the current code.
    code_size: usize,
    /// `f_label`: the label groups, 64 labels each; `None` for a label the
    /// group does not define.
    labels: Vec<[Option<usize>; 64]>,
    params: Vec<Arg>,
    autos: Vec<Arg>,
}

/// An array's elements: shared between a level and the levels that take it
/// by reference.
type ArrayValue = std::rc::Rc<std::cell::RefCell<BTreeMap<u32, Num>>>;

/// One level of an array name's stack: `bc_var_array`.
#[derive(Clone, Debug)]
struct ArrayLevel {
    /// `a_value`: `None` until an element is first reached.
    value: Option<ArrayValue>,
    /// `a_param`: the elements are another level's, taken by reference.
    param: bool,
}

/// A name's numbers: `id_rec`. 0 is "none yet".
#[derive(Clone, Copy, Debug, Default)]
struct Ids {
    a_name: i32,
    f_name: i32,
    v_name: i32,
}

/// The machine and its front end: upstream's globals, in one place.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's globals, flag for flag"
)]
pub struct Bc {
    // ---- options (global.h)
    /// `interactive`: standard input and output are terminals, or `-i`.
    pub interactive: bool,
    /// `compile_only`: `-c`, print the byte code.
    pub compile_only: bool,
    /// `use_math`: `-l`.
    pub use_math: bool,
    /// `warn_not_std`: `-w`.
    pub warn_not_std: bool,
    /// `std_only`: `-s`, or `POSIXLY_CORRECT`.
    pub std_only: bool,
    /// `quiet`: `-q`.
    pub quiet: bool,
    /// `file_names`: the files still to read.
    pub file_names: VecDeque<Vec<u8>>,
    /// `file_name`: the file being read.
    file_name: Vec<u8>,
    /// `is_std_in`: standard input is being read.
    is_std_in: bool,
    /// `first_file`: no file has been opened yet.
    first_file: bool,
    /// `line_size`: `BC_LINE_LENGTH`, 70 by default, 0 for no breaks.
    pub line_size: i32,

    // ---- the scanner
    scan: scan::Scanner,
    /// `line_no`.
    line_no: i32,

    // ---- the code generator (bc.y, util.c, load.c)
    break_label: i32,
    if_label: i32,
    continue_label: i32,
    next_label: i32,
    out_count: usize,
    did_gen: bool,
    had_error: bool,
    cur_func: i32,
    load_func: usize,
    load_addr: usize,
    save_func: usize,
    save_addr: usize,
    load_str: bool,
    load_const: bool,
    /// `name_tree`: each name's numbers as a variable, an array and a
    /// function.
    names: BTreeMap<Vec<u8>, Ids>,
    next_array: i32,
    next_func: i32,
    next_var: i32,

    // ---- storage (storage.c)
    functions: Vec<Function>,
    /// `f_names`, by function number.
    f_names: Vec<Option<Vec<u8>>>,
    /// Each variable's stack of values, top last; empty for none yet.
    variables: Vec<Vec<Num>>,
    /// `v_names`, indexed as upstream fills it: variable `n`'s name at
    /// `n - 1`.
    v_names: Vec<Option<Vec<u8>>>,
    /// Each array name's stack of levels, top last.
    arrays: Vec<Vec<ArrayLevel>>,
    /// `a_names`, by array number.
    a_names: Vec<Option<Vec<u8>>>,
    /// The execution stack, top last.
    ex_stack: Vec<Num>,
    /// The function return stack, top last.
    fn_stack: Vec<i32>,
    i_base: i32,
    o_base: i32,
    scale: i32,
    n_history: i32,
    c_code: bool,
    runtime_error: bool,
    pc_func: usize,
    pc_addr: usize,
    /// `out_col`.
    out_col: i32,
    random: random::Random,

    // ---- input and output
    /// Standard output: line-buffered, as `main`'s `setvbuf` makes it.
    pub out: Output,
    /// Standard error.
    pub err: Output,
    /// Standard input through stdio, which `read()` reads.
    pub stdin: Input,
}

impl Default for Bc {
    fn default() -> Self {
        Self::new(
            Output::stdout().line_buffered(),
            Output::stderr(),
            Input::stdin(),
        )
    }
}

impl Bc {
    /// A bc writing to `out` and `err`, `read()` reading `stdin`, with
    /// upstream's defaults.
    #[must_use]
    pub fn new(out: Output, err: Output, stdin: Input) -> Self {
        Self {
            interactive: false,
            compile_only: false,
            use_math: false,
            warn_not_std: false,
            std_only: false,
            quiet: false,
            file_names: VecDeque::new(),
            file_name: Vec::new(),
            is_std_in: false,
            first_file: true,
            line_size: 70,
            scan: scan::Scanner::default(),
            line_no: 1,
            break_label: 0,
            if_label: 0,
            continue_label: 0,
            next_label: 1,
            out_count: 2,
            did_gen: false,
            had_error: false,
            cur_func: -1,
            load_func: 0,
            load_addr: 0,
            save_func: 0,
            save_addr: 0,
            load_str: false,
            load_const: false,
            names: BTreeMap::new(),
            next_array: 1,
            next_func: 1,
            next_var: 5,
            functions: vec![Function::default()],
            f_names: vec![Some(b"(main)".to_vec())],
            variables: vec![Vec::new(); 5],
            v_names: vec![None; 5],
            arrays: vec![Vec::new()],
            a_names: vec![None],
            ex_stack: Vec::new(),
            fn_stack: Vec::new(),
            i_base: 10,
            o_base: 10,
            scale: 0,
            n_history: -1,
            c_code: false,
            runtime_error: false,
            pc_func: 0,
            pc_addr: 0,
            out_col: 0,
            random: random::Random::new(),
            out,
            err,
            stdin,
        }
    }

    /// `bc_exit (val)`: standard output flushed, as `exit` flushes it, and
    /// the process ended.
    pub fn exit(&mut self, code: i32) -> ! {
        self.out.flush();
        std::process::exit(code)
    }

    /// `perror (what)`.
    fn perror(&mut self, what: &[u8], errno: i32) {
        let mut line = what.to_vec();
        line.extend_from_slice(b": ");
        line.extend_from_slice(
            errmsg::strerror(&std::io::Error::from_raw_os_error(errno)).as_bytes(),
        );
        line.push(b'\n');
        self.err.write(&line);
    }

    /// `checkferror_output (stdout)`: number.c's, whose words say `dc`
    /// whichever program calls it.
    pub fn check_stdout(&mut self) {
        if self.out.error() {
            let e = self.out.errno();
            self.perror(b"dc: could not write output file", e);
            self.exit(1);
        }
    }

    /// `checkferror_output (stderr)`.
    pub fn check_stderr(&mut self) {
        if self.err.error() {
            let e = self.err.errno();
            self.perror(b"dc: could not write output file", e);
            self.exit(1);
        }
    }

    /// `checkferror_input (stdin)`.
    fn check_stdin(&mut self) {
        if self.stdin.error() {
            let e = self.stdin.errno();
            self.perror(b"dc: could not read input file", e);
            self.exit(1);
        }
    }

    /// The name `yyerror` and `ct_warn` give the input: `(standard_in)`, or
    /// the file's, escaped.
    fn input_name(&self) -> Vec<u8> {
        if self.is_std_in {
            b"(standard_in)".to_vec()
        } else {
            quoting::escape_unprintable(&self.file_name).into_bytes()
        }
    }

    /// `yyerror (msg)`: a compile error, said with the input's name and line.
    fn yyerror(&mut self, msg: &[u8]) {
        let mut line = self.input_name();
        line.extend_from_slice(format!(" {}: ", self.line_no).as_bytes());
        line.extend_from_slice(msg);
        line.push(b'\n');
        self.err.write(&line);
        self.check_stderr();
        self.had_error = true;
    }

    /// `ct_warn (msg)`: a non-standard construct -- an error under `-s`, a
    /// warning under `-w`, nothing otherwise.
    fn ct_warn(&mut self, msg: &[u8]) {
        let tag: &[u8] = if self.std_only {
            b"Error: "
        } else if self.warn_not_std {
            b"(Warning) "
        } else {
            return;
        };
        let mut line = self.input_name();
        line.extend_from_slice(format!(" {}: ", self.line_no).as_bytes());
        line.extend_from_slice(tag);
        line.extend_from_slice(msg);
        line.push(b'\n');
        self.err.write(&line);
        self.check_stderr();
        if self.std_only {
            self.had_error = true;
        }
    }

    /// The name of function `f` as `rt_error` prints it: `(null)` for none.
    fn f_name(&self, f: usize) -> Vec<u8> {
        self.f_names
            .get(f)
            .cloned()
            .flatten()
            .unwrap_or_else(|| b"(null)".to_vec())
    }

    /// `rt_error (msg)`: a runtime error, which stops the machine.
    fn rt_error(&mut self, msg: &[u8]) {
        let mut line = b"Runtime error (func=".to_vec();
        line.extend_from_slice(&self.f_name(self.pc_func));
        line.extend_from_slice(format!(", adr={}): ", self.pc_addr).as_bytes());
        line.extend_from_slice(msg);
        line.push(b'\n');
        self.err.write(&line);
        self.check_stderr();
        self.runtime_error = true;
    }

    /// `rt_warn (msg)`: a runtime warning.
    fn rt_warn(&mut self, msg: &[u8]) {
        let mut line = b"Runtime warning (func=".to_vec();
        line.extend_from_slice(&self.f_name(self.pc_func));
        line.extend_from_slice(format!(", adr={}): ", self.pc_addr).as_bytes());
        line.extend_from_slice(msg);
        line.push(b'\n');
        self.err.write(&line);
        self.check_stderr();
    }

    /// `out_char (ch)`: a byte of a number to standard output, after a
    /// backslash and a newline when the line is full.
    fn out_char(&mut self, ch: u8) {
        if ch == b'\n' {
            self.out_col = 0;
            self.out.write(b"\n");
        } else {
            self.out_col = self.out_col.saturating_add(1);
            if self.out_col == self.line_size.saturating_sub(1) && self.line_size != 0 {
                self.out.write(b"\\\n");
                self.out_col = 1;
            }
            self.out.write(&[ch]);
        }
        self.check_stdout();
    }

    /// `out_schar (ch)`: a byte of a string, broken across lines only where
    /// POSIX bc is not asked for.
    fn out_schar(&mut self, ch: u8) {
        if ch == b'\n' {
            self.out_col = 0;
            self.out.write(b"\n");
        } else {
            if !self.std_only {
                self.out_col = self.out_col.saturating_add(1);
                if self.out_col == self.line_size.saturating_sub(1) && self.line_size != 0 {
                    self.out.write(b"\\\n");
                    self.out_col = 1;
                }
            }
            self.out.write(&[ch]);
        }
        self.check_stdout();
    }

    /// `show_bc_version ()`.
    pub fn show_bc_version(&mut self) {
        self.out.write(
            b"bc 1.07.1\nCopyright 1991-1994, 1997, 1998, 2000, 2004, 2006, 2008, 2012-2017 Free Software Foundation, Inc.\n",
        );
    }

    /// `welcome ()`.
    fn welcome(&mut self) {
        self.out.write(
            b"This is free software with ABSOLUTELY NO WARRANTY.\nFor details type `warranty'. \n",
        );
    }

    /// `warranty (prefix)`.
    fn warranty(&mut self) {
        self.out.write(b"\n");
        self.show_bc_version();
        self.out.write(WARRANTY);
    }

    /// `limits ()`.
    fn limits(&mut self) {
        let text = format!(
            "BC_BASE_MAX     = {BC_INT_MAX}\nBC_DIM_MAX      = {BC_DIM_MAX}\nBC_SCALE_MAX    = {BC_INT_MAX}\n\
             BC_STRING_MAX   = {BC_INT_MAX}\nMAX Exponent    = {}\nNumber of vars  = {MAX_STORE}\n",
            i64::MAX
        );
        self.out.write(text.as_bytes());
        self.check_stdout();
    }

    /// `open_new_file ()`: the next input -- the math library first, if
    /// asked for, then each file named, then standard input; `false` when
    /// standard input has been read. A file that cannot be opened ends the
    /// program.
    pub fn open_new_file(&mut self) -> bool {
        self.line_no = 1;
        if self.is_std_in {
            return false;
        }
        if self.use_math && self.first_file {
            // In the order of each function's first mention in libmath.b,
            // which is how the compiled code numbers them.
            for name in [b"e", b"l", b"s", b"a", b"c", b"j"] {
                self.lookup(name, code::Kind::Funct);
            }
            for chunk in libmath::LIBMATH {
                self.load_code(chunk);
            }
        }
        if let Some(name) = self.file_names.pop_front() {
            match scan::Scanner::open(&name) {
                Ok(source) => {
                    self.new_yy_file(source);
                    self.file_name = name;
                    return true;
                }
                Err(_) => {
                    let mut line = b"File ".to_vec();
                    line.extend_from_slice(&quoting::escape_unprintable(&name).into_bytes());
                    line.extend_from_slice(b" is unavailable.\n");
                    self.err.write(&line);
                    self.exit(1);
                }
            }
        }
        self.new_yy_file(scan::Source::stdin());
        self.is_std_in = true;
        true
    }

    /// `new_yy_file (file)`: the scanner reads `source` from here on.
    fn new_yy_file(&mut self, source: scan::Source) {
        self.scan.switch(source, !self.first_file);
        self.first_file = false;
    }

    /// Run the program: `init_storage`, `init_load`, `init_tree`,
    /// `init_gen`, the first file, then the parse, which runs each line as
    /// it is read. Ends the process, as `main` does.
    pub fn run(&mut self) -> ! {
        self.init_load();
        self.init_gen();
        self.is_std_in = false;
        self.first_file = true;
        if !self.open_new_file() {
            self.exit(1);
        }
        self.yyparse();
        if self.compile_only {
            self.out.write(b"\n");
        }
        self.exit(0)
    }
}

/// `warranty`'s text after the version.
const WARRANTY: &[u8] = b"\n    This program is free software; you can redistribute it and/or modify\n    it under the terms of the GNU General Public License as published by\n    the Free Software Foundation; either version 3 of the License , or\n    (at your option) any later version.\n\n    This program is distributed in the hope that it will be useful,\n    but WITHOUT ANY WARRANTY; without even the implied warranty of\n    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the\n    GNU General Public License for more details.\n\n    You should have received a copy of the GNU General Public License\n    along with this program. If not, write to\n\n       The Free Software Foundation, Inc.\n       51 Franklin Street, Fifth Floor\n       Boston, MA 02110-1335  USA\n\n";
