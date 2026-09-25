//! `dircolors` — output commands to set the `LS_COLORS` environment variable.
//!
//! ```text
//! Usage: dircolors [OPTION]... [FILE]
//! ```
//!
//! A port of GNU coreutils 9.4's `src/dircolors.c`. There was none, and every
//! stock shell start-up file runs `eval "$(dircolors -b)"`; with no
//! `dircolors` that line fails at every login and `ls --color` gets no
//! palette.
//!
//! # The database
//!
//! Without a FILE, the built-in database is read: GNU's `dircolors.hin` as
//! upstream's build compiles it (blank lines dropped, runs of white space
//! collapsed), which is also exactly what `--print-database` prints. It is
//! kept beside the crate as `src/dircolors.db`, taken from `dircolors -p` of
//! GNU coreutils 9.4 built from source, and embedded verbatim -- its own
//! header carries the permission notice it asks to have preserved.
//!
//! # Reading one
//!
//! Upstream's `dc_parse_stream`, line by line:
//!
//! * a line is a KEYWORD and an ARGUMENT, white space around each, `#` to the
//!   end of the line a comment; a KEYWORD with no ARGUMENT is `invalid line;
//!   missing second token` (two spaces after the semicolon, upstream's);
//! * `TERM` and `COLORTERM` lines are globs over `$TERM` (`none` if unset or
//!   empty) and `$COLORTERM` (empty if unset); a run of them opens a section
//!   that applies if any matched, and the entries before the first run apply
//!   everywhere;
//! * `.ext` becomes `*.ext`, `*glob` stays, `COLOR`/`OPTIONS`/`EIGHTBIT` are
//!   ignored, and the file-type names map to `ls`'s two-letter codes
//!   (`DIR` → `di`, `SETUID` → `su`, …);
//! * a keyword it does not know is an error only inside a section that
//!   applies -- before the first `TERM` line, and in a section that does not,
//!   upstream says nothing, and neither does this.
//!
//! Nothing is printed unless the whole file was good.
//!
//! # Checked against GNU
//!
//! `scripts/dircolors-diff.sh`.

use coreutils::fnmatch::{Flags, fnmatch};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::pathname::last_component;
use coreutils::quote::{os_bytes, quote};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const DIRCOLORS: Program = Program::new("dircolors", 1);

/// Upstream's short options.
const SHORT_OPTIONS: &str = "bcp";

/// Upstream's `long_options[]`, in declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("bourne-shell", Takes::Nothing),
    ("sh", Takes::Nothing),
    ("csh", Takes::Nothing),
    ("c-shell", Takes::Nothing),
    ("print-database", Takes::Nothing),
    ("print-ls-colors", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Two spellings of one option each: glibc does not call a prefix ambiguous
/// when every candidate means the same thing, so `--c` is `--csh`.
const LONG_ALIASES: &[(&str, &str)] = &[("sh", "bourne-shell"), ("c-shell", "csh")];

/// The built-in database, as `dircolors -p` of GNU 9.4 prints it.
const DATABASE: &[u8] = include_bytes!("../dircolors.db");

/// `slack_codes[]` and `ls_codes[]`, zipped: the file-type keywords a
/// database may use, and the two-letter code `ls` reads for each.
const TYPE_CODES: &[(&str, &str)] = &[
    ("NORMAL", "no"),
    ("NORM", "no"),
    ("FILE", "fi"),
    ("RESET", "rs"),
    ("DIR", "di"),
    ("LNK", "ln"),
    ("LINK", "ln"),
    ("SYMLINK", "ln"),
    ("ORPHAN", "or"),
    ("MISSING", "mi"),
    ("FIFO", "pi"),
    ("PIPE", "pi"),
    ("SOCK", "so"),
    ("BLK", "bd"),
    ("BLOCK", "bd"),
    ("CHR", "cd"),
    ("CHAR", "cd"),
    ("DOOR", "do"),
    ("EXEC", "ex"),
    ("LEFT", "lc"),
    ("LEFTCODE", "lc"),
    ("RIGHT", "rc"),
    ("RIGHTCODE", "rc"),
    ("END", "ec"),
    ("ENDCODE", "ec"),
    ("SUID", "su"),
    ("SETUID", "su"),
    ("SGID", "sg"),
    ("SETGID", "sg"),
    ("STICKY", "st"),
    ("OTHER_WRITABLE", "ow"),
    ("OWR", "ow"),
    ("STICKY_OTHER_WRITABLE", "tw"),
    ("OWT", "tw"),
    ("CAPABILITY", "ca"),
    ("MULTIHARDLINK", "mh"),
    ("CLRTOEOL", "cl"),
];

/// Which shell's syntax to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Syntax {
    Bourne,
    C,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    PrintDatabase,
    Run {
        /// `-b`/`-c`, or neither.
        syntax: Option<Syntax>,
        /// `--print-ls-colors`: escaped samples for a terminal, not shell code.
        print_ls_colors: bool,
        /// The database to read instead of the built-in one.
        file: Option<OsString>,
    },
}

fn help_text() -> String {
    "\
Usage: dircolors [OPTION]... [FILE]
Output commands to set the LS_COLORS environment variable.

Determine format of output:
  -b, --sh, --bourne-shell    output Bourne shell code to set LS_COLORS
  -c, --csh, --c-shell        output C shell code to set LS_COLORS
  -p, --print-database        output defaults
      --print-ls-colors       output fully escaped colors for display
      --help        display this help and exit
      --version     output version information and exit

If FILE is specified, read it to determine which colors to use for which
file types and extensions.  Otherwise, a precompiled database is used.
For details on the format of these files, run 'dircolors --print-database'.
"
    .to_string()
}

/// Upstream's option loop, then its three checks in its order.
///
/// # Errors
///
/// An unknown option; a shell syntax with `-p` or `--print-ls-colors`; both of
/// those; or an operand too many.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut syntax: Option<Syntax> = None;
    let mut print_database = false;
    let mut print_ls_colors = false;
    let mut operands: Vec<OsString> = Vec::new();
    for item in DIRCOLORS.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, LONG_ALIASES) {
        match item? {
            Opt::Short(b'b', _) | Opt::Long("bourne-shell" | "sh", _) => {
                syntax = Some(Syntax::Bourne);
            }
            Opt::Short(b'c', _) | Opt::Long("csh" | "c-shell", _) => syntax = Some(Syntax::C),
            Opt::Short(b'p', _) | Opt::Long("print-database", _) => print_database = true,
            Opt::Long("print-ls-colors", _) => print_ls_colors = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(x) => operands.push(x.clone()),
            // Unreachable: every table entry is handled above.
            Opt::Long(other, _) => {
                return Err(DIRCOLORS.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(DIRCOLORS.invalid_option(c)),
        }
    }
    if (print_database || print_ls_colors) && syntax.is_some() {
        return Err(DIRCOLORS.usage_referring(
            "the options to output non shell syntax,\n\
             and to select a shell syntax are mutually exclusive"
                .to_string(),
        ));
    }
    if print_database && print_ls_colors {
        return Err(DIRCOLORS.usage_referring(
            "options --print-database and --print-ls-colors are mutually exclusive".to_string(),
        ));
    }
    // One FILE, or none with -p; the extra one named is the first past that.
    let allowed = usize::from(!print_database);
    if let Some(extra) = operands.get(allowed) {
        let mut message = format!("extra operand {}", quote(&os_bytes(extra)));
        if print_database {
            message.push_str("\nfile operands cannot be combined with --print-database (-p)");
        }
        return Err(DIRCOLORS.usage_referring(message));
    }
    if print_database {
        return Ok(Request::PrintDatabase);
    }
    Ok(Request::Run {
        syntax,
        print_ls_colors,
        file: operands.into_iter().next(),
    })
}

/// Upstream's `guess_shell_syntax`: C-shell syntax if `$SHELL`'s last
/// component is `csh` or `tcsh`, Bourne otherwise, and `None` when `$SHELL`
/// is unset or empty.
fn guess_shell_syntax(shell: Option<&[u8]>) -> Option<Syntax> {
    let shell = shell.filter(|s| !s.is_empty())?;
    match last_component(shell) {
        b"csh" | b"tcsh" => Some(Syntax::C),
        _ => Some(Syntax::Bourne),
    }
}

/// C's `isspace` in the C locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// Upstream's `parse_line`: the KEYWORD and ARGUMENT of one line, either of
/// which may be missing. The line ends at its first NUL, as a C string does.
fn parse_line(line: &[u8]) -> (Option<&[u8]>, Option<&[u8]>) {
    let line = line.split(|&b| b == 0).next().unwrap_or_default();
    let mut p = line
        .iter()
        .position(|&b| !is_c_space(b))
        .unwrap_or(line.len());
    // Blank lines and shell-style comments.
    if matches!(line.get(p), None | Some(b'#')) {
        return (None, None);
    }
    let keyword_start = p;
    while line.get(p).is_some_and(|&b| !is_c_space(b)) {
        p = p.saturating_add(1);
    }
    let keyword = line.get(keyword_start..p);
    if p >= line.len() {
        return (keyword, None);
    }
    // `do ++p; while (isspace (*p))`: step over the separator, then the rest.
    p = p.saturating_add(1);
    while line.get(p).is_some_and(|&b| is_c_space(b)) {
        p = p.saturating_add(1);
    }
    if matches!(line.get(p), None | Some(b'#')) {
        return (keyword, None);
    }
    let arg_start = p;
    while line.get(p).is_some_and(|&b| b != b'#') {
        p = p.saturating_add(1);
    }
    // Trailing white space off the argument; it cannot empty it, since it
    // starts with a byte that is not space.
    let arg = line.get(arg_start..p).unwrap_or_default();
    let end = arg
        .iter()
        .rposition(|&b| !is_c_space(b))
        .map_or(0, |i| i.saturating_add(1));
    (keyword, arg.get(..end))
}

/// The `LS_COLORS` value, or the `--print-ls-colors` listing, being built.
struct Accumulator {
    out: Vec<u8>,
    print_ls_colors: bool,
}

impl Accumulator {
    /// Upstream's `append_quoted`: shell-quote for single quotes, and put a
    /// backslash before a `:` or `=` that is not already escaped (`\` and `^`
    /// being `ls`'s own escape characters). Verbatim for
    /// `--print-ls-colors`.
    fn append_quoted(&mut self, text: &[u8]) {
        let mut need_backslash = true;
        for &c in text {
            if !self.print_ls_colors {
                match c {
                    b'\'' => {
                        self.out.extend_from_slice(b"'\\'");
                        need_backslash = true;
                    }
                    b'\\' | b'^' => need_backslash = !need_backslash,
                    b':' | b'=' => {
                        if need_backslash {
                            self.out.push(b'\\');
                        }
                        need_backslash = true;
                    }
                    _ => need_backslash = true,
                }
            }
            self.out.push(c);
        }
    }

    /// Upstream's `append_entry`.
    fn append_entry(&mut self, prefix: Option<u8>, item: &[u8], arg: &[u8]) {
        if self.print_ls_colors {
            self.append_quoted(b"\x1b[");
            self.append_quoted(arg);
            self.out.push(b'm');
        }
        if let Some(prefix) = prefix {
            self.out.push(prefix);
        }
        self.append_quoted(item);
        self.out
            .push(if self.print_ls_colors { b'\t' } else { b'=' });
        self.append_quoted(arg);
        if self.print_ls_colors {
            self.append_quoted(b"\x1b[0m");
        }
        self.out
            .push(if self.print_ls_colors { b'\n' } else { b':' });
    }
}

/// Where the lines come from, for diagnostics: a file's name as typed, or the
/// built-in database.
#[derive(Clone, Copy)]
enum Source<'a> {
    File(&'a [u8]),
    Internal,
}

/// Upstream's `dc_parse_stream` over lines already read, with its
/// diagnostics collected rather than printed. `true` if every line was good.
struct Parser<'a> {
    term: &'a [u8],
    colorterm: &'a [u8],
    source: Source<'a>,
    acc: Accumulator,
    diags: Vec<String>,
    ok: bool,
    line_number: u64,
    state: State,
}

/// The parser's state: `ST_GLOBAL` before any `TERM`/`COLORTERM` line, and
/// then whether the current section applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Before the first `TERM`/`COLORTERM` line: entries apply everywhere.
    Global,
    /// In a run of `TERM` lines, none of which has matched yet.
    TermNo,
    /// A `TERM` line of the current run matched.
    TermSure,
    /// Past the run, in a section that applies.
    TermYes,
}

impl<'a> Parser<'a> {
    fn new(term: &'a [u8], colorterm: &'a [u8], source: Source<'a>, print_ls_colors: bool) -> Self {
        Parser {
            term,
            colorterm,
            source,
            acc: Accumulator {
                out: Vec::new(),
                print_ls_colors,
            },
            diags: Vec::new(),
            ok: true,
            line_number: 0,
            state: State::Global,
        }
    }

    fn where_(&self) -> String {
        match self.source {
            Source::File(name) => coreutils::quote::quotef(name),
            Source::Internal => "<internal>".to_string(),
        }
    }

    /// One line, in upstream's words.
    fn line(&mut self, line: &[u8]) {
        self.line_number = self.line_number.saturating_add(1);
        let (Some(keyword), arg) = parse_line(line) else {
            return;
        };
        let Some(arg) = arg else {
            self.diags.push(format!(
                "{}:{}: invalid line;  missing second token",
                self.where_(),
                self.line_number
            ));
            self.ok = false;
            return;
        };
        let mut unrecognized = false;
        if keyword.eq_ignore_ascii_case(b"TERM") {
            if self.state != State::TermSure {
                self.state = if fnmatch(arg, self.term, Flags::NONE) {
                    State::TermSure
                } else {
                    State::TermNo
                };
            }
        } else if keyword.eq_ignore_ascii_case(b"COLORTERM") {
            if self.state != State::TermSure {
                self.state = if fnmatch(arg, self.colorterm, Flags::NONE) {
                    State::TermSure
                } else {
                    State::TermNo
                };
            }
        } else {
            if self.state == State::TermSure {
                // Another run of TERM lines can close this section.
                self.state = State::TermYes;
            }
            if self.state == State::TermNo {
                unrecognized = true;
            } else if keyword.first() == Some(&b'.') {
                self.acc.append_entry(Some(b'*'), keyword, arg);
            } else if keyword.first() == Some(&b'*') {
                self.acc.append_entry(None, keyword, arg);
            } else if keyword.eq_ignore_ascii_case(b"OPTIONS")
                || keyword.eq_ignore_ascii_case(b"COLOR")
                || keyword.eq_ignore_ascii_case(b"EIGHTBIT")
            {
                // Recognised, and ignored.
            } else if let Some((_, code)) = TYPE_CODES
                .iter()
                .find(|(name, _)| keyword.eq_ignore_ascii_case(name.as_bytes()))
            {
                self.acc.append_entry(None, code.as_bytes(), arg);
            } else {
                unrecognized = true;
            }
        }
        if unrecognized && matches!(self.state, State::TermSure | State::TermYes) {
            // The keyword is printed as it is, unquoted: upstream's `%s`. It
            // holds no white space, which is all that ended it.
            let keyword: String = keyword.iter().map(|&b| char::from(b)).collect();
            self.diags.push(format!(
                "{}:{}: unrecognized keyword {keyword}",
                self.where_(),
                self.line_number
            ));
            self.ok = false;
        }
    }
}

/// The built-in database, line by line: `G_line`'s strings.
fn database_lines() -> impl Iterator<Item = &'static [u8]> {
    DATABASE
        .strip_suffix(b"\n")
        .unwrap_or(DATABASE)
        .split(|&b| b == b'\n')
}

/// Wrap the accumulated value as shell code, or not.
fn finish(value: &[u8], syntax: Option<Syntax>, print_ls_colors: bool) -> Vec<u8> {
    if print_ls_colors {
        return value.to_vec();
    }
    let (prefix, suffix): (&[u8], &[u8]) = match syntax {
        Some(Syntax::C) => (b"setenv LS_COLORS '", b"'\n"),
        _ => (b"LS_COLORS='", b"';\nexport LS_COLORS\n"),
    };
    let mut out = prefix.to_vec();
    out.extend_from_slice(value);
    out.extend_from_slice(suffix);
    out
}

fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(run(), 1)
}

fn run() -> std::process::ExitCode {
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::stdfd::{self, Stream};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::ExitCode;

    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(r) => r,
        Err(e) => {
            DIRCOLORS.report(&e);
            return ExitCode::FAILURE;
        }
    };
    let mut out = Stream::stdout();
    let (syntax, print_ls_colors, file) = match request {
        Request::Help => {
            // Deliberately unread: `close_stdout` reports a failed write.
            let _ = out.write_all(help_text().as_bytes());
            return stdfd::close_stdout("dircolors", out, ExitCode::SUCCESS);
        }
        Request::Version => {
            let _ = out.write_all(b"dircolors (SlateOS coreutils) 0.1.0\n");
            return stdfd::close_stdout("dircolors", out, ExitCode::SUCCESS);
        }
        Request::PrintDatabase => {
            let _ = out.write_all(DATABASE);
            return stdfd::close_stdout("dircolors", out, ExitCode::SUCCESS);
        }
        Request::Run {
            syntax,
            print_ls_colors,
            file,
        } => (syntax, print_ls_colors, file),
    };

    let syntax = match syntax {
        Some(s) => Some(s),
        None if print_ls_colors => None,
        None => {
            let shell = std::env::var_os("SHELL");
            match guess_shell_syntax(shell.as_deref().map(os_bytes).as_deref()) {
                Some(s) => Some(s),
                None => {
                    diag!(
                        "dircolors: no SHELL environment variable, and no shell type option given"
                    );
                    return ExitCode::FAILURE;
                }
            }
        }
    };

    let term_env = std::env::var_os("TERM");
    let term: Vec<u8> = match term_env.as_deref().map(os_bytes) {
        Some(t) if !t.is_empty() => t.into_owned(),
        _ => b"none".to_vec(),
    };
    let colorterm: Vec<u8> = std::env::var_os("COLORTERM")
        .as_deref()
        .map(|c| os_bytes(c).into_owned())
        .unwrap_or_default();

    let name_bytes = file.as_ref().map(|f| os_bytes(f).into_owned());
    let source = match &name_bytes {
        Some(name) => Source::File(name),
        None => Source::Internal,
    };
    let mut parser = Parser::new(&term, &colorterm, source, print_ls_colors);

    match &file {
        None => {
            for line in database_lines() {
                parser.line(line);
            }
        }
        Some(name) => {
            let name_b = name_bytes.as_deref().unwrap_or_default();
            // `freopen` for a name, the standard input for `-`.
            let reader: Box<dyn Read> = if name_b == b"-" {
                Box::new(std::io::stdin())
            } else {
                match std::fs::File::open(name) {
                    Ok(f) => Box::new(f),
                    Err(e) => {
                        diag!(
                            "dircolors: {}: {}",
                            coreutils::quote::quotef(name_b),
                            strerror(&e)
                        );
                        return ExitCode::FAILURE;
                    }
                }
            };
            let mut reader = BufReader::new(reader);
            let mut line = Vec::new();
            loop {
                line.clear();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        parser.line(&line);
                        // Diagnostics as they arise, as upstream prints them.
                        for message in parser.diags.drain(..) {
                            diag!("dircolors: {message}");
                        }
                    }
                    Err(e) => {
                        diag!(
                            "dircolors: {}: read error: {}",
                            coreutils::quote::quotef(name_b),
                            strerror(&e)
                        );
                        parser.ok = false;
                        break;
                    }
                }
            }
        }
    }
    for message in parser.diags.drain(..) {
        diag!("dircolors: {message}");
    }
    if !parser.ok {
        return stdfd::close_stdout("dircolors", out, ExitCode::FAILURE);
    }
    let _ = out.write_all(&finish(&parser.acc.out, syntax, print_ls_colors));
    stdfd::close_stdout("dircolors", out, ExitCode::SUCCESS)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn parse(lines: &[&str], term: &str, colorterm: &str) -> (String, Vec<String>, bool) {
        let mut p = Parser::new(
            term.as_bytes(),
            colorterm.as_bytes(),
            Source::File(b"db"),
            false,
        );
        for line in lines {
            p.line(line.as_bytes());
        }
        (String::from_utf8(p.acc.out).unwrap(), p.diags, p.ok)
    }

    #[test]
    fn lines_split_into_keyword_and_argument() {
        assert_eq!(
            parse_line(b"DIR 01;34 # directory\n"),
            (Some(&b"DIR"[..]), Some(&b"01;34"[..]))
        );
        assert_eq!(
            parse_line(b"  .tar\t01;31  \r\n"),
            (Some(&b".tar"[..]), Some(&b"01;31"[..]))
        );
        assert_eq!(parse_line(b"# comment\n"), (None, None));
        assert_eq!(parse_line(b"   \n"), (None, None));
        assert_eq!(parse_line(b"DIR\n"), (Some(&b"DIR"[..]), None));
        assert_eq!(parse_line(b"DIR   # nothing\n"), (Some(&b"DIR"[..]), None));
        assert_eq!(parse_line(b"DIR"), (Some(&b"DIR"[..]), None));
        // A NUL ends the line, as it ends a C string.
        assert_eq!(
            parse_line(b"DIR 01\0;34\n"),
            (Some(&b"DIR"[..]), Some(&b"01"[..]))
        );
    }

    #[test]
    fn entries_before_any_term_apply_everywhere() {
        let (out, diags, ok) = parse(&["DIR 01;34", ".tar 01;31", "*~ 00;90"], "dumb", "");
        assert_eq!(out, "di=01;34:*.tar=01;31:*~=00;90:");
        assert!(diags.is_empty() && ok);
    }

    #[test]
    fn a_term_section_applies_only_when_a_glob_matches() {
        let db = [
            "TERM xterm*",
            "TERM linux",
            "DIR 01;34",
            "TERM dumb",
            "FILE 00",
        ];
        assert_eq!(parse(&db, "xterm-256color", "").0, "di=01;34:");
        assert_eq!(parse(&db, "linux", "").0, "di=01;34:");
        assert_eq!(parse(&db, "dumb", "").0, "fi=00:");
        assert_eq!(parse(&db, "vt100", "").0, "");
        // COLORTERM opens a section the same way.
        let db = ["COLORTERM ?*", "DIR 01;34"];
        assert_eq!(parse(&db, "none", "truecolor").0, "di=01;34:");
        assert_eq!(parse(&db, "none", "").0, "");
    }

    #[test]
    fn an_unknown_keyword_is_an_error_only_where_the_section_applies() {
        let (_, diags, ok) = parse(&["BOGUS 1"], "x", "");
        assert!(diags.is_empty() && ok, "before any TERM line: silent");
        let (_, diags, ok) = parse(&["TERM y", "BOGUS 1"], "x", "");
        assert!(
            diags.is_empty() && ok,
            "in a section that does not apply: silent"
        );
        let (_, diags, ok) = parse(&["TERM x", "BOGUS 1"], "x", "");
        assert_eq!(diags, vec!["db:2: unrecognized keyword BOGUS"]);
        assert!(!ok);
    }

    #[test]
    fn a_missing_argument_is_always_an_error() {
        let (_, diags, ok) = parse(&["# c", "", "DIR"], "x", "");
        assert_eq!(diags, vec!["db:3: invalid line;  missing second token"]);
        assert!(!ok);
    }

    #[test]
    fn keywords_are_case_insensitive_and_three_are_ignored() {
        assert_eq!(
            parse(
                &["dir 1", "Setuid 2", "color all", "OPTIONS -F", "EIGHTBIT 1"],
                "x",
                ""
            )
            .0,
            "di=1:su=2:"
        );
    }

    #[test]
    fn values_are_quoted_for_the_shell() {
        let mut acc = Accumulator {
            out: Vec::new(),
            print_ls_colors: false,
        };
        acc.append_quoted(b"a'b");
        assert_eq!(acc.out, b"a'\\''b");
        let mut acc = Accumulator {
            out: Vec::new(),
            print_ls_colors: false,
        };
        // `:` and `=` gain a backslash unless one is already in force.
        acc.append_quoted(b"a:b=c\\:d^=e\\\\:");
        assert_eq!(acc.out, b"a\\:b\\=c\\:d^=e\\\\\\:");
    }

    #[test]
    fn print_ls_colors_shows_each_entry_in_its_own_colour() {
        let mut acc = Accumulator {
            out: Vec::new(),
            print_ls_colors: true,
        };
        acc.append_entry(Some(b'*'), b".tar", b"01;31");
        assert_eq!(acc.out, b"\x1b[01;31m*.tar\t01;31\x1b[0m\n");
    }

    #[test]
    fn the_shell_is_guessed_from_its_last_component() {
        assert_eq!(guess_shell_syntax(Some(b"/bin/tcsh")), Some(Syntax::C));
        assert_eq!(guess_shell_syntax(Some(b"csh")), Some(Syntax::C));
        assert_eq!(guess_shell_syntax(Some(b"/bin/bash")), Some(Syntax::Bourne));
        assert_eq!(guess_shell_syntax(Some(b"/bin/cshx")), Some(Syntax::Bourne));
        assert_eq!(guess_shell_syntax(Some(b"")), None);
        assert_eq!(guess_shell_syntax(None), None);
    }

    #[test]
    fn output_is_wrapped_for_the_shell_asked_for() {
        assert_eq!(
            finish(b"di=1:", Some(Syntax::Bourne), false),
            b"LS_COLORS='di=1:';\nexport LS_COLORS\n"
        );
        assert_eq!(
            finish(b"di=1:", Some(Syntax::C), false),
            b"setenv LS_COLORS 'di=1:'\n"
        );
        assert_eq!(finish(b"x", None, true), b"x");
    }

    #[test]
    fn the_built_in_database_parses_cleanly_for_common_terminals() {
        for term in ["xterm-256color", "linux", "none", "dumb", "screen"] {
            let mut p = Parser::new(term.as_bytes(), b"", Source::Internal, false);
            for line in database_lines() {
                p.line(line);
            }
            assert!(p.ok && p.diags.is_empty(), "{term}: {:?}", p.diags);
        }
        assert!(DATABASE.ends_with(b"\n"));
    }

    #[test]
    fn options() {
        assert_eq!(parse_args(&argv(&["-p"])).unwrap(), Request::PrintDatabase);
        assert_eq!(
            parse_args(&argv(&["--c", "f"])).unwrap(),
            Request::Run {
                syntax: Some(Syntax::C),
                print_ls_colors: false,
                file: Some("f".into())
            }
        );
        let e = parse_args(&argv(&["-p", "-b"])).unwrap_err();
        assert!(
            e.message()
                .starts_with("the options to output non shell syntax,\nand to select")
        );
        let e = parse_args(&argv(&["-p", "--print-ls-colors"])).unwrap_err();
        assert!(
            e.message()
                .starts_with("options --print-database and --print-ls-colors")
        );
        let e = parse_args(&argv(&["-p", "f"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand ‘f’\nfile operands cannot be combined with --print-database (-p)\n\
             Try 'dircolors --help' for more information."
        );
        let e = parse_args(&argv(&["a", "b"])).unwrap_err();
        assert!(e.message().starts_with("extra operand ‘b’\nTry"));
    }
}
