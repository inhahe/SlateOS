//! getopt -- parse command options for a shell script.
//!
//! A port of util-linux 2.39.3's `misc-utils/getopt.c`, function by function
//! and with upstream's names; measured against `getopt from util-linux
//! 2.39.3` by `scripts/getopt-diff.sh`.
//!
//! getopt(1) is glibc's `getopt_long` handed to a shell script, so what it
//! does with the script's arguments is [`getoptlong`]'s, with the four
//! switches glibc has that no utility of ours had needed: it keeps going
//! after an error ([`getoptlong::Parser::keep_going`]), `-a` is
//! `getopt_long_only` ([`getoptlong::Parser::long_only`]), every `-l` entry
//! is its own option ([`getoptlong::Parser::distinct_entries`]), and `W;` in
//! an option string makes `-W foo` mean `--foo`.
//!
//! This replaces a hand-written approximation that read argv as `String`
//! (so a script passing a file name that is not UTF-8 made it panic), had
//! its own idea of long-option matching, and neither permuted like glibc
//! nor quoted for tcsh.
//!
//! # Exit status, as upstream's
//!
//! | status | meaning |
//! |---|---|
//! | 0 | parsed |
//! | 1 | `getopt(3)` refused something in the script's arguments |
//! | 2 | getopt(1)'s own arguments were wrong |
//! | 3 | the output could not be written, or a diagnostic could not be |
//! | 4 | `-T`: this is the enhanced getopt |
//!
//! Status 3 is `close_stdout`'s (`CLOSE_EXIT_CODE`, which getopt sets to its
//! `XALLOC_EXIT_CODE`), and so it is util-linux's rule, through
//! `ulclosestream`: a write that failed before the end, or a final one that
//! failed other than on a closed stdout, and a diagnostic that could not be
//! written -- `getopt -o a -- -x 2>&-` is 3, not 1. A stdout the process was
//! started without stays closed (`stdfdguard`), as upstream sees it.
//!
//! # What is not upstream's
//!
//! * **A name in a diagnostic** -- an option the script's caller typed, or
//!   `-n`'s name -- has its control bytes escaped, where glibc writes them
//!   raw: an argument holding a newline must not be able to print a line of
//!   its own into the script's stderr. Printable text is glibc's.
//! * **A long option whose name is not UTF-8** (`-l` with such bytes) can
//!   never be matched, since [`getoptlong`]'s table holds text; glibc
//!   compares bytes. It is still counted where glibc would count it, in the
//!   table's positions.

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use std::ffi::{OsStr, OsString};
use std::process::ExitCode;
use ulclosestream::{Stdout, stderr_write, warnx};

/// `GETOPT_EXIT_CODE`: `getopt(3)` refused an option in the script's
/// arguments.
const GETOPT_EXIT_CODE: u8 = 1;
/// `PARAMETER_EXIT_CODE`: getopt(1)'s own arguments were wrong.
const PARAMETER_EXIT_CODE: u8 = 2;
/// `CLOSE_EXIT_CODE` (= `XALLOC_EXIT_CODE`): stdout could not be written.
const CLOSE_EXIT_CODE: u8 = 3;
/// `TEST_EXIT_CODE`: `-T`.
const TEST_EXIT_CODE: u8 = 4;

/// getopt(1)'s own options, and the parse of the script's. Only the
/// sentences are used: each is printed after a name this program chooses.
const GETOPT: Program = Program::new("getopt", 2);

/// Upstream's own option string: `+`, so that the first word that is not an
/// option -- the script's option string, in the old form -- ends the scan.
const SHORTS: &str = "+ao:l:n:qQs:TuhV";

/// Upstream's `longopts[]`, in its order (which the ambiguity message
/// shows).
const LONGS: &[(&str, Takes)] = &[
    ("options", Takes::Required),
    ("longoptions", Takes::Required),
    ("quiet", Takes::Nothing),
    ("quiet-output", Takes::Nothing),
    ("shell", Takes::Required),
    ("test", Takes::Nothing),
    ("unquoted", Takes::Nothing),
    ("help", Takes::Nothing),
    ("alternative", Takes::Nothing),
    ("name", Takes::Required),
    ("version", Takes::Nothing),
];

/// `shell_t`: whose quoting the output follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shell {
    Bash,
    Tcsh,
}

/// `struct getopt_control`.
#[derive(Debug)]
struct Ctl {
    shell: Shell,
    /// The script's option string, as given (with any `+` getopt added).
    optstr: Option<Vec<u8>>,
    /// `-n`: the name the script's errors are reported under.
    name: Option<Vec<u8>>,
    /// `-l`, in the order given: each entry is its own option.
    long_options: Vec<(Vec<u8>, Takes)>,
    /// `-a`: `getopt_long_only`.
    long_only: bool,
    quiet_errors: bool,
    quiet_output: bool,
    quote: bool,
}

impl Default for Ctl {
    fn default() -> Self {
        Ctl {
            shell: Shell::Bash,
            optstr: None,
            name: None,
            long_options: Vec::new(),
            long_only: false,
            quiet_errors: false,
            quiet_output: false,
            quote: true,
        }
    }
}

/// The program's end: nothing more to print, only a status to exit with.
#[derive(Debug, PartialEq, Eq)]
struct Exit(u8);

stdfdguard::guard_std_fds!();

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the process was
    // started without is closed again, as upstream would find it.
    stdfdguard::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let short = short_name(
        argv.first()
            .map_or(OsStr::new("getopt"), OsString::as_os_str),
    );
    let mut stdout = Stdout::new(CLOSE_EXIT_CODE);
    let Exit(code) = run(&argv, &mut stdout);
    // `close_stdout`, which upstream registers with `atexit`: every way out
    // passes it.
    ExitCode::from(stdout.close(code, &short))
}

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// A name or argument in a diagnostic: as upstream prints it, except that
/// what is not printable is escaped (module docs).
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `parse_error`: MESSAGE (if any), then `errtryhelp(PARAMETER_EXIT_CODE)`.
fn parse_error(short: &[u8], message: Option<&str>) -> Exit {
    if let Some(message) = message {
        warnx(short, message);
    }
    let line = format!("Try '{} --help' for more information.\n", shown(short));
    stderr_write(line.as_bytes());
    Exit(PARAMETER_EXIT_CODE)
}

/// What was printed, into stdout as upstream's `printf`s put it there, and
/// the status earned; `main`'s `close_stdout` judges both.
///
/// All at once rather than piece by piece is the same to the verdict: what
/// decides it is whether the output outgrew stdio's buffer before the end,
/// which the total decides.
fn finish(stdout: &mut Stdout, out: &[u8], status: u8) -> Exit {
    stdout.write(out);
    Exit(status)
}

/// `usage()`, with upstream's text and the short name where it puts it.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut out = b"\nUsage:\n".to_vec();
    for form in [
        &b" <optstring> <parameters>\n"[..],
        b" [options] [--] <optstring> <parameters>\n",
        b" [options] -o|--options <optstring> [options] [--] <parameters>\n",
    ] {
        out.push(b' ');
        out.extend_from_slice(short);
        out.extend_from_slice(form);
    }
    out.extend_from_slice(
        b"\nParse command options.\n\
\nOptions:\n\
\x20-a, --alternative             allow long options starting with single -\n\
\x20-l, --longoptions <longopts>  the long options to be recognized\n\
\x20-n, --name <progname>         the name under which errors are reported\n\
\x20-o, --options <optstring>     the short options to be recognized\n\
\x20-q, --quiet                   disable error reporting by getopt(3)\n\
\x20-Q, --quiet-output            no normal output\n\
\x20-s, --shell <shell>           set quoting conventions to those of <shell>\n\
\x20-T, --test                    test for getopt(1) version\n\
\x20-u, --unquoted                do not quote the output\n\
\n\
\x20-h, --help                    display this help\n\
\x20-V, --version                 display version\n\
\nFor more details see getopt(1).\n",
    );
    out
}

/// `main()`.
fn run(argv: &[OsString], stdout: &mut Stdout) -> Exit {
    let arg0: &OsStr = argv
        .first()
        .map_or(OsStr::new("getopt"), OsString::as_os_str);
    let short = short_name(arg0);
    let mut ctl = Ctl::default();
    let compatible = std::env::var_os("GETOPT_COMPATIBLE").is_some();

    let Some(first) = argv.get(1) else {
        if compatible {
            // "For some reason, the original getopt gave no error when there
            // were no arguments."
            return finish(stdout, b" --\n", 0);
        }
        return parse_error(&short, Some("missing optstring argument"));
    };

    // The old form, `getopt OPTSTRING PARAMETERS`: anything not starting
    // with `-`, or everything under GETOPT_COMPATIBLE. The option string loses
    // its leading run of `-` and `+`, and the output is not quoted.
    let first_bytes = os_bytes(first);
    if first_bytes.first() != Some(&b'-') || compatible {
        ctl.quote = false;
        let skip = first_bytes
            .iter()
            .take_while(|&&b| b == b'-' || b == b'+')
            .count();
        ctl.optstr = Some(first_bytes.get(skip..).unwrap_or_default().to_vec());
        let params = argv.get(2..).unwrap_or_default();
        return generate_output(&ctl, &os_bytes(arg0), params, stdout);
    }

    let own = argv.get(1..).unwrap_or_default();
    let mut parser = GETOPT.parse(own, SHORTS, LONGS);
    // Where the script's part of argv begins, in `own`: past the last option,
    // and past a `--` (glibc's `optind`).
    let mut optind = own.len();
    while let Some(item) = parser.next() {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given; the referral
                // that follows uses the short name.
                let line = format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence);
                stderr_write(line.as_bytes());
                return parse_error(&short, None);
            }
        };
        let flag = match opt {
            Opt::Short(c, value) => (c, value),
            Opt::Long(name, value) => (long_flag(name), value),
            Opt::Operand(_) => {
                // `+`: the first word that is not an option ends the scan,
                // and belongs to the script.
                optind = parser.optind().saturating_sub(1);
                break;
            }
        };
        match flag {
            (b'a', _) => ctl.long_only = true,
            (b'o', Some(v)) => add_short_options(&mut ctl, &os_bytes(&v)),
            (b'l', Some(v)) => {
                if let Err(e) = add_long_options(&mut ctl, &os_bytes(&v)) {
                    return parse_error(&short, Some(e));
                }
            }
            (b'n', Some(v)) => ctl.name = Some(os_bytes(&v).into_owned()),
            (b'q', _) => ctl.quiet_errors = true,
            (b'Q', _) => ctl.quiet_output = true,
            (b's', Some(v)) => match shell_type(&os_bytes(&v)) {
                Some(shell) => ctl.shell = shell,
                None => {
                    return parse_error(&short, Some("unknown shell after -s or --shell argument"));
                }
            },
            (b'T', _) => return Exit(TEST_EXIT_CODE),
            (b'u', _) => ctl.quote = false,
            (b'V', _) => {
                let mut out = short.clone();
                out.extend_from_slice(b" from util-linux 2.39.3\n");
                return finish(stdout, &out, 0);
            }
            (b'h', _) => return finish(stdout, &usage(&short), 0),
            _ => {
                return parse_error(&short, Some("internal error, contact the author."));
            }
        }
    }
    if optind == own.len() {
        // The walk ended without an operand: everything was an option, or a
        // `--` came last. glibc's `optind` is then the end, or past the `--`.
        optind = parser.optind();
    }
    let mut params = own.get(optind..).unwrap_or_default();

    if ctl.optstr.is_none() {
        let Some((optstr, rest)) = params.split_first() else {
            return parse_error(&short, Some("missing optstring argument"));
        };
        add_short_options(&mut ctl, &os_bytes(optstr));
        params = rest;
    }

    let name = ctl
        .name
        .clone()
        .unwrap_or_else(|| os_bytes(arg0).into_owned());
    generate_output(&ctl, &name, params, stdout)
}

/// The short option a long one of getopt's own stands for.
fn long_flag(name: &str) -> u8 {
    match name {
        "options" => b'o',
        "longoptions" => b'l',
        "quiet" => b'q',
        "quiet-output" => b'Q',
        "shell" => b's',
        "test" => b'T',
        "unquoted" => b'u',
        "help" => b'h',
        "alternative" => b'a',
        "name" => b'n',
        "version" => b'V',
        // Every name in LONGS is above; this is the `default:` upstream has.
        _ => 0,
    }
}

/// `add_short_options`: under `POSIXLY_CORRECT` an option string that does
/// not already begin with `+` gets one, so glibc stops at the first operand.
fn add_short_options(ctl: &mut Ctl, options: &[u8]) {
    let mut optstr = Vec::with_capacity(options.len().saturating_add(1));
    if options.first() != Some(&b'+') && std::env::var_os("POSIXLY_CORRECT").is_some() {
        optstr.push(b'+');
    }
    optstr.extend_from_slice(options);
    ctl.optstr = Some(optstr);
}

/// `add_long_options`: names separated by commas or whitespace (`strtok`'s
/// `", \t\n"`), each ending in `:` for a required argument or `::` for an
/// optional one.
///
/// # Errors
///
/// A name that is nothing but its colons: "empty long option after -l or
/// --long argument".
fn add_long_options(ctl: &mut Ctl, options: &[u8]) -> Result<(), &'static str> {
    for token in options
        .split(|&b| matches!(b, b',' | b' ' | b'\t' | b'\n'))
        .filter(|t| !t.is_empty())
    {
        let (name, takes) = if let Some(name) = token.strip_suffix(b"::") {
            (name, Takes::Optional)
        } else if let Some(name) = token.strip_suffix(b":") {
            (name, Takes::Required)
        } else {
            (token, Takes::Nothing)
        };
        if takes != Takes::Nothing && name.is_empty() {
            return Err("empty long option after -l or --long argument");
        }
        ctl.long_options.push((name.to_vec(), takes));
    }
    Ok(())
}

/// `shell_type`: `sh` and `bash` quote alike, as do `csh` and `tcsh`.
fn shell_type(shell: &[u8]) -> Option<Shell> {
    match shell {
        b"bash" | b"sh" => Some(Shell::Bash),
        b"tcsh" | b"csh" => Some(Shell::Tcsh),
        _ => None,
    }
}

/// `isspace` in the C locale.
fn is_c_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `print_normalized`: ` ARG`, quoted for the shell unless `-u`.
///
/// Inside single quotes a shell of the Bourne family needs only `'` handled
/// (`'\''`); tcsh also sees `!` there, and loses whitespace, so a backslash is
/// doubled, `!` becomes `'\!'`, a newline `\n`, and other white space `'\ '`.
fn print_normalized(out: &mut Vec<u8>, ctl: &Ctl, arg: &[u8]) {
    out.push(b' ');
    if !ctl.quote {
        out.extend_from_slice(arg);
        return;
    }
    out.push(b'\'');
    for &c in arg {
        if ctl.shell == Shell::Tcsh {
            match c {
                b'\\' => {
                    out.extend_from_slice(b"\\\\");
                    continue;
                }
                b'!' => {
                    out.extend_from_slice(b"'\\!'");
                    continue;
                }
                b'\n' => {
                    out.extend_from_slice(b"\\n");
                    continue;
                }
                _ => {}
            }
            if is_c_space(c) {
                out.extend_from_slice(b"'\\");
                out.push(c);
                out.push(b'\'');
                continue;
            }
        }
        if c == b'\'' {
            out.extend_from_slice(b"'\\''");
        } else {
            out.push(c);
        }
    }
    out.push(b'\'');
}

/// Whether the option string gives `c` an argument: upstream's
/// `strchr(optstr, c)` then a look at the next byte, on the string as given.
fn short_has_arg(optstr: &[u8], c: u8) -> bool {
    optstr
        .iter()
        .position(|&b| b == c)
        .is_some_and(|at| optstr.get(at.saturating_add(1)) == Some(&b':'))
}

/// `generate_output`: parse `params` against the script's options, and print
/// them normalized -- every option, with its argument, then `--`, then every
/// operand -- or, with `-Q`, nothing. `name` is what the script's errors are
/// reported under.
fn generate_output(ctl: &Ctl, name: &[u8], params: &[OsString], stdout: &mut Stdout) -> Exit {
    let optstr: &[u8] = ctl.optstr.as_deref().unwrap_or_default();
    // glibc prints nothing when the option string, past its `+` or `-`,
    // begins with `:`; `-q` is `opterr = 0`.
    let body = optstr
        .strip_prefix(b"+")
        .or_else(|| optstr.strip_prefix(b"-"))
        .unwrap_or(optstr);
    let silent = ctl.quiet_errors || body.first() == Some(&b':');
    let return_in_order = optstr.first() == Some(&b'-');

    // The table borrows the names; one that is not UTF-8 keeps its place
    // with a name nothing typed can match (module docs).
    let names: Vec<Option<&str>> = ctl
        .long_options
        .iter()
        .map(|(n, _)| std::str::from_utf8(n).ok())
        .collect();
    let table: Vec<(&str, Takes)> = ctl
        .long_options
        .iter()
        .zip(&names)
        .map(|((_, takes), name)| (name.unwrap_or("\u{0}unmatchable"), *takes))
        .collect();

    let mut parser = GETOPT
        .parse_bytes(params, optstr, &table)
        .keep_going(true)
        .distinct_entries(true)
        .long_only(ctl.long_only);
    let mut status = 0;
    let mut out = Vec::new();
    let mut remaining: Vec<&OsString> = Vec::new();
    while let Some(item) = parser.next() {
        match item {
            Err(e) => {
                status = GETOPT_EXIT_CODE;
                if !silent {
                    let line = format!("{}: {}\n", shown(name), e.sentence);
                    stderr_write(line.as_bytes());
                }
            }
            Ok(Opt::Long(long, value)) => {
                if !ctl.quiet_output {
                    out.extend_from_slice(b" --");
                    out.extend_from_slice(long.as_bytes());
                    let takes = table
                        .iter()
                        .find(|(n, _)| *n == long)
                        .map_or(Takes::Nothing, |(_, t)| *t);
                    if takes != Takes::Nothing {
                        print_normalized(
                            &mut out,
                            ctl,
                            &value.as_deref().map(os_bytes).unwrap_or_default(),
                        );
                    }
                }
            }
            Ok(Opt::Short(c, value)) => {
                if !ctl.quiet_output {
                    out.extend_from_slice(&[b' ', b'-', c]);
                    if short_has_arg(optstr, c) {
                        print_normalized(
                            &mut out,
                            ctl,
                            &value.as_deref().map(os_bytes).unwrap_or_default(),
                        );
                    }
                }
            }
            // A `-` option string hands each operand back where it stood
            // (glibc's `NON_OPT`), until a `--`; every other operand is left
            // for after the `--` getopt prints.
            Ok(Opt::Operand(word)) => {
                if return_in_order && !parser.stopped() {
                    if !ctl.quiet_output {
                        print_normalized(&mut out, ctl, &os_bytes(word));
                    }
                } else {
                    remaining.push(word);
                }
            }
        }
    }
    if ctl.quiet_output {
        return finish(stdout, &[], status);
    }
    out.extend_from_slice(b" --");
    for word in remaining {
        print_normalized(&mut out, ctl, &os_bytes(word));
    }
    out.push(b'\n');
    finish(stdout, &out, status)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn normalized(ctl: &Ctl, arg: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        print_normalized(&mut out, ctl, arg);
        out
    }

    #[test]
    fn bash_quoting_handles_only_the_single_quote() {
        let ctl = Ctl::default();
        assert_eq!(normalized(&ctl, b"a b"), b" 'a b'");
        assert_eq!(normalized(&ctl, b"it's"), b" 'it'\\''s'");
        assert_eq!(normalized(&ctl, b"a\\b!\n"), b" 'a\\b!\n'");
        assert_eq!(normalized(&ctl, b""), b" ''");
        assert_eq!(normalized(&ctl, b"\xff"), b" '\xff'");
    }

    #[test]
    fn tcsh_quoting_also_handles_backslash_bang_and_white_space() {
        let ctl = Ctl {
            shell: Shell::Tcsh,
            ..Ctl::default()
        };
        assert_eq!(normalized(&ctl, b"a\\b"), b" 'a\\\\b'");
        assert_eq!(normalized(&ctl, b"hi!"), b" 'hi'\\!''");
        assert_eq!(normalized(&ctl, b"a\nb"), b" 'a\\nb'");
        assert_eq!(normalized(&ctl, b"a b\tc"), b" 'a'\\ 'b'\\\t'c'");
        assert_eq!(normalized(&ctl, b"it's"), b" 'it'\\''s'");
    }

    #[test]
    fn unquoted_output_is_the_bytes() {
        let ctl = Ctl {
            quote: false,
            ..Ctl::default()
        };
        assert_eq!(normalized(&ctl, b"a 'b'"), b" a 'b'");
    }

    #[test]
    fn long_options_are_split_as_strtok_splits_them() {
        let mut ctl = Ctl::default();
        add_long_options(&mut ctl, b",alpha, beta:\tgamma::\n,,delta:::").unwrap();
        assert_eq!(
            ctl.long_options,
            vec![
                (b"alpha".to_vec(), Takes::Nothing),
                (b"beta".to_vec(), Takes::Required),
                (b"gamma".to_vec(), Takes::Optional),
                // Only one strip: `delta:::` is `delta:`, optional.
                (b"delta:".to_vec(), Takes::Optional),
            ]
        );
        for empty in [&b":"[..], b"::", b"a,:"] {
            assert_eq!(
                add_long_options(&mut Ctl::default(), empty),
                Err("empty long option after -l or --long argument")
            );
        }
    }

    #[test]
    fn an_optional_or_required_argument_is_whatever_follows_the_letter() {
        assert!(short_has_arg(b"ab:c::", b'b'));
        assert!(short_has_arg(b"ab:c::", b'c'));
        assert!(!short_has_arg(b"ab:c::", b'a'));
        // The first occurrence decides, as strchr does.
        assert!(!short_has_arg(b"aa:", b'a'));
    }

    #[test]
    fn the_short_name_is_argv0_past_its_last_slash() {
        assert_eq!(short_name(OsStr::new("/usr/bin/getopt")), b"getopt");
        assert_eq!(short_name(OsStr::new("getopt")), b"getopt");
    }

    #[test]
    fn the_shells() {
        assert_eq!(shell_type(b"sh"), Some(Shell::Bash));
        assert_eq!(shell_type(b"csh"), Some(Shell::Tcsh));
        assert_eq!(shell_type(b"zsh"), None);
    }
}
