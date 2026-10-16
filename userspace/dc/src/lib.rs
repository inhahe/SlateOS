//! GNU dc 1.4.1 (bc 1.07.1's `dc/`), ported, as Ubuntu 24.04 builds it:
//! the reverse-Polish desk calculator, over [`bcnumber`] -- bc's `number.c`
//! -- for its arithmetic, so that every digit it prints is upstream's.
//!
//! The modules follow upstream's files: [`eval`](crate) the input language
//! (`eval.c`), the stacks and registers (`stack.c`, `array.c`), the numbers'
//! glue and output (`numeric.c`); the C library's `FILE`s, whose buffering
//! dc's output shows, are `cstdio`'s ([`io`]).
//!
//! # Where this differs from upstream, on purpose
//!
//! - **A file name in a diagnostic is escaped** where it holds a byte that
//!   is not printable (`quoting::escape_unprintable`): upstream prints it
//!   raw, so a crafted name could write lines dc never said.
//! - **Recursion too deep for the stack** ends the program as upstream's
//!   does, but by Rust's stack-overflow abort (`SIGABRT` and a line saying
//!   so) where C's ends in `SIGSEGV`, at a depth that differs with the frame
//!   sizes.

/// The C library's `FILE`s, as dc meets them.
pub use cstdio as io;

mod eval;
mod numeric;
mod stack;

use std::collections::BTreeMap;
use std::rc::Rc;

use bcnumber::Num;

use crate::io::{Input, Output};

/// `dc_data`: a value -- a number, or a string. (Upstream's third kind,
/// `DC_UNINITIALIZED`, is never a value on a stack here: it is the absent
/// value of a register level, [`Level::value`]'s `None`. Upstream checks
/// for it wherever a value is used and calls `dc_garbage` -- "a bug in dc"
/// -- if it is found; with the type unable to hold it, there is nothing to
/// check and no `dc_garbage`.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Data {
    /// A number.
    Num(Rc<Num>),
    /// A string of bytes, any bytes.
    Str(Rc<[u8]>),
}

impl Data {
    /// A number value.
    #[must_use]
    pub fn num(n: Num) -> Self {
        Self::Num(Rc::new(n))
    }

    /// A string value: `dc_makestring`.
    #[must_use]
    pub fn string(s: &[u8]) -> Self {
        Self::Str(Rc::from(s))
    }
}

/// `DC_REGCOUNT`: one register per byte value.
const REGCOUNT: usize = 256;

/// One level of a register's stack: `struct dc_list`, its value and the
/// array that goes with it.
#[derive(Clone, Debug, Default)]
struct Level {
    /// `None` for `DC_UNINITIALIZED`: a level made only to hold an array.
    value: Option<Data>,
    /// The array's elements, by index (upstream's sorted sparse list).
    array: BTreeMap<i32, Data>,
}

/// What dc's `!` command runs a line through: `system(3)`, or a test's
/// stand-in.
pub trait Shell {
    /// Run `command` with `/bin/sh -c` and wait for it. Nothing is flushed
    /// first: `system` does not flush stdio.
    fn system(&mut self, command: &[u8]);
}

/// `system(3)`.
pub struct System;

impl Shell for System {
    fn system(&mut self, command: &[u8]) {
        // An interior NUL cannot be in it: the command ends at the first NUL,
        // as upstream's C string does, before it gets here.
        let Ok(c) = std::ffi::CString::new(command) else {
            return;
        };
        // dc ignores what the command returned, as upstream does.
        let _ = libcall::process::system(&c);
    }
}

/// `dc_evalfile`'s and `dc_evalstr`'s verdict: whether the program goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// `DC_SUCCESS`.
    Success,
    /// `DC_FAIL`: a quit, or an input that could not be read.
    Fail,
}

/// The interpreter: upstream's file-scope statics, in one place.
pub struct Dc {
    /// `progname`: the base name of `argv[0]`.
    progname: Vec<u8>,
    /// Standard output.
    pub out: Output,
    /// Standard error.
    pub err: Output,
    /// Standard input, which `?` reads, and which is evaluated when no
    /// file or expression is given.
    pub stdin: Input,
    /// The evaluation stack, top last.
    stack: Vec<Data>,
    /// The registers' stacks, top last.
    registers: Vec<Vec<Level>>,
    ibase: i32,
    obase: i32,
    scale: i32,
    /// How many levels of evaluation a quit still has to leave.
    unwind_depth: i32,
    /// Whether the quit under way may not end the program (`Q`'s).
    unwind_noexit: bool,
    /// The lookahead byte of standard input while it is being evaluated,
    /// for `?` to give back before it reads (`None` for EOF: used up).
    stdin_lookahead: Option<u8>,
    /// Columns written on the current output line, for the line breaks.
    out_col: i32,
    /// The line length; negative until `DC_LINE_LENGTH` has been read.
    line_max: i32,
    /// `DC_LINE_LENGTH` as the environment held it at the start.
    line_length: Option<Vec<u8>>,
    shell: Box<dyn Shell>,
}

impl Dc {
    /// A dc called `progname`, writing to `out` and `err`, reading `stdin`,
    /// running `!` commands through `shell`, with `DC_LINE_LENGTH` as given.
    #[must_use]
    pub fn new(
        progname: &[u8],
        out: Output,
        err: Output,
        stdin: Input,
        shell: Box<dyn Shell>,
        line_length: Option<Vec<u8>>,
    ) -> Self {
        Self {
            progname: progname.to_vec(),
            out,
            err,
            stdin,
            stack: Vec::new(),
            registers: vec![Vec::new(); REGCOUNT],
            ibase: 10,
            obase: 10,
            scale: 0,
            unwind_depth: 0,
            unwind_noexit: false,
            stdin_lookahead: None,
            out_col: 0,
            line_max: -1,
            line_length,
            shell,
        }
    }

    /// `progname`.
    #[must_use]
    pub fn progname(&self) -> &[u8] {
        &self.progname
    }

    /// `fprintf (stderr, "%s: %s", progname, text)`, unchecked.
    pub fn say(&mut self, text: &[u8]) {
        let mut line = self.progname.clone();
        line.extend_from_slice(b": ");
        line.extend_from_slice(text);
        self.err.write(&line);
    }

    /// [`Dc::say`], then `checkferror_output (stderr)`: upstream's usual
    /// way to complain.
    pub fn complain(&mut self, text: &[u8]) {
        self.say(text);
        self.check_stderr();
    }

    /// `perror (what)`: `what`, then the words for `errno`.
    pub fn perror(&mut self, what: &[u8], errno: i32) {
        let mut line = what.to_vec();
        line.extend_from_slice(b": ");
        line.extend_from_slice(
            errmsg::strerror(&std::io::Error::from_raw_os_error(errno)).as_bytes(),
        );
        line.push(b'\n');
        self.err.write(&line);
    }

    /// `exit (code)`: standard output flushed, as the C library's `exit`
    /// does, and the process ended.
    pub fn exit(&mut self, code: i32) -> ! {
        self.out.flush();
        std::process::exit(code)
    }

    /// `checkferror_output (stdout)`.
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

    /// `checkferror_input (fp)`, for an input whose error is `errno` if it
    /// has one.
    fn check_input(&mut self, error: Option<i32>) {
        if let Some(e) = error {
            self.perror(b"dc: could not read input file", e);
            self.exit(1);
        }
    }
}

/// `dc_show_id`'s text: `'X' (0130)` and the suffix for a printable byte
/// (C's `isgraph` in the C locale), else the octal alone (`%#o`, so 0 is
/// `0`).
#[must_use]
pub fn show_id(id: u8, suffix: &[u8]) -> Vec<u8> {
    let octal = if id == 0 {
        "0".to_owned()
    } else {
        format!("0{id:o}")
    };
    let mut text = Vec::new();
    if id.is_ascii_graphic() {
        // A register's name, one graphic byte: `'%c'` is dc's own spelling.
        text.extend_from_slice(&[b'\'', id, b'\'', b' ', b'(']);
        text.extend_from_slice(octal.as_bytes());
        text.push(b')');
    } else {
        text.extend_from_slice(octal.as_bytes());
    }
    text.extend_from_slice(suffix);
    text
}

/// `r1bindex (s, '/')`: what follows the last `/`, or all of `s`.
#[must_use]
pub fn base_name(s: &[u8]) -> &[u8] {
    match s.iter().rposition(|&b| b == b'/') {
        Some(i) => s.get(i.saturating_add(1)..).unwrap_or_default(),
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_shown_as_printf_shows_it() {
        assert_eq!(show_id(b'G', b" x"), b"'G' (0107) x");
        assert_eq!(show_id(b' ', b""), b"040");
        assert_eq!(show_id(0, b""), b"0");
        assert_eq!(show_id(0xff, b""), b"0377");
        assert_eq!(show_id(b'~', b""), b"'~' (0176)");
    }

    #[test]
    fn the_program_name_is_what_follows_the_last_slash() {
        assert_eq!(base_name(b"/usr/bin/dc"), b"dc");
        assert_eq!(base_name(b"dc"), b"dc");
        assert_eq!(base_name(b"dir/"), b"");
    }
}
