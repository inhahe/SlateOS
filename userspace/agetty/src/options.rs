//! `struct options` and `parse_args`: agetty's command line, read as
//! upstream reads it, with `--help`, `--version`, `--list-speeds` and the
//! line, speed and terminal-type operands.

use std::ffi::OsString;

use getoptlong::{Opt, Program, Takes};
use quoting::{os_bytes, os_from_bytes};

use crate::speeds;
use crate::{Diag, Exit};

/// The parser; it prints nothing itself.
const AGETTY: Program = Program::new("agetty", 1);

/// Upstream's option string.
const SHORTS: &str = "8a:cC:d:Ef:hH:iI:Jl:L::mnNo:pP:r:Rst:Uw";

/// Upstream's `longopts`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("8bits", Takes::Nothing),
    ("autologin", Takes::Required),
    ("noreset", Takes::Nothing),
    ("chdir", Takes::Required),
    ("delay", Takes::Required),
    ("remote", Takes::Nothing),
    ("issue-file", Takes::Required),
    ("show-issue", Takes::Nothing),
    ("flow-control", Takes::Nothing),
    ("host", Takes::Required),
    ("noissue", Takes::Nothing),
    ("init-string", Takes::Required),
    ("noclear", Takes::Nothing),
    ("login-program", Takes::Required),
    ("local-line", Takes::Optional),
    ("extract-baud", Takes::Nothing),
    ("list-speeds", Takes::Nothing),
    ("skip-login", Takes::Nothing),
    ("nonewline", Takes::Nothing),
    ("login-options", Takes::Required),
    ("login-pause", Takes::Nothing),
    ("nice", Takes::Required),
    ("chroot", Takes::Required),
    ("hangup", Takes::Nothing),
    ("keep-baud", Takes::Nothing),
    ("timeout", Takes::Required),
    ("detect-case", Takes::Nothing),
    ("wait-cr", Takes::Nothing),
    ("nohints", Takes::Nothing),
    ("nohostname", Takes::Nothing),
    ("long-hostname", Takes::Nothing),
    ("reload", Takes::Nothing),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
    ("erase-chars", Takes::Required),
    ("kill-chars", Takes::Required),
];

/// `F_PARSE`: process modem status messages.
pub const F_PARSE: u32 = 1 << 0;
/// `F_ISSUE`: display the issue file or directories.
pub const F_ISSUE: u32 = 1 << 1;
/// `F_RTSCTS`: enable RTS/CTS flow control.
pub const F_RTSCTS: u32 = 1 << 2;
/// `F_INITSTRING`: an init string is set.
pub const F_INITSTRING: u32 = 1 << 4;
/// `F_WAITCRLF`: wait for CR or LF.
pub const F_WAITCRLF: u32 = 1 << 5;
/// `F_NOPROMPT`: do not ask for a login name.
pub const F_NOPROMPT: u32 = 1 << 7;
/// `F_LCUC`: support for the `*LCUC` stty modes.
pub const F_LCUC: u32 = 1 << 8;
/// `F_KEEPSPEED`: follow the baud rate from the kernel.
pub const F_KEEPSPEED: u32 = 1 << 9;
/// `F_KEEPCFLAGS`: reuse the kernel's `c_cflag`.
pub const F_KEEPCFLAGS: u32 = 1 << 10;
/// `F_EIGHTBITS`: assume an 8-bit-clean tty.
pub const F_EIGHTBITS: u32 = 1 << 11;
/// `F_VCONSOLE`: a virtual console.
pub const F_VCONSOLE: u32 = 1 << 12;
/// `F_HANGUP`: call `vhangup(2)`.
pub const F_HANGUP: u32 = 1 << 13;
/// `F_UTF8`: the line can do UTF-8.
pub const F_UTF8: u32 = 1 << 14;
/// `F_LOGINPAUSE`: wait for a key before the login prompt.
pub const F_LOGINPAUSE: u32 = 1 << 15;
/// `F_NOCLEAR`: do not clear the screen before prompting.
pub const F_NOCLEAR: u32 = 1 << 16;
/// `F_NONL`: no newline before the issue.
pub const F_NONL: u32 = 1 << 17;
/// `F_NOHOSTNAME`: do not show the host name.
pub const F_NOHOSTNAME: u32 = 1 << 18;
/// `F_LONGHNAME`: show the fully qualified host name.
pub const F_LONGHNAME: u32 = 1 << 19;
/// `F_NOHINTS`: do not print hints.
pub const F_NOHINTS: u32 = 1 << 20;
/// `F_REMOTE`: add `-h fakehost` to login's command line.
pub const F_REMOTE: u32 = 1 << 21;

/// `MAX_SPEED`: the most baud rates to try.
pub const MAX_SPEED: usize = 10;

/// `--local-line`: whether `CLOCAL` is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Clocal {
    /// `CLOCAL_MODE_AUTO`.
    #[default]
    Auto,
    /// `CLOCAL_MODE_ALWAYS`.
    Always,
    /// `CLOCAL_MODE_NEVER`.
    Never,
}

/// `struct options`, with upstream's file-scope `fakehost`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Toggle switches, `F_*`.
    pub flags: u32,
    /// `-t`: the time-out, in seconds.
    pub timeout: u32,
    /// `-a`: log this user in automatically.
    pub autolog: Option<Vec<u8>>,
    /// `--chdir`.
    pub chdir: Option<Vec<u8>>,
    /// `-r`: change root before the login.
    pub chroot: Option<Vec<u8>>,
    /// `-l`: the login program.
    pub login: Vec<u8>,
    /// `-o`: login's options.
    pub logopt: Option<Vec<u8>>,
    /// The line's name.
    pub tty: Vec<u8>,
    /// A virtual console's number, `tty` without its `tty`.
    pub vcline: Option<Vec<u8>>,
    /// The terminal type.
    pub term: Option<Vec<u8>>,
    /// `-I`: the modem init string, octal escapes decoded.
    pub initstring: Option<Vec<u8>>,
    /// `-f`: the issue files or directories, `:`-separated.
    pub issue: Option<Vec<u8>>,
    /// `--erase-chars`.
    pub erasechars: Option<Vec<u8>>,
    /// `--kill-chars`.
    pub killchars: Option<Vec<u8>>,
    /// `--delay`: seconds to sleep before the prompt.
    pub delay: u32,
    /// `--nice`.
    pub nice: i32,
    /// The baud rates to try, as `B*` codes.
    pub speeds: Vec<u32>,
    /// `-L`.
    pub clocal: Clocal,
    /// Whether the line was given as `-`, standard input.
    pub tty_is_stdin: bool,
    /// `-H`: the host login is told of (upstream's `fakehost`).
    pub fakehost: Option<Vec<u8>>,
    /// A virtual console's keyboard mode (`KDGKBMODE`), `K_RAW` on a serial
    /// line.
    pub kbmode: i32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            flags: F_ISSUE,
            timeout: 0,
            autolog: None,
            chdir: None,
            chroot: None,
            login: crate::PATH_LOGIN.to_vec(),
            logopt: None,
            tty: b"tty1".to_vec(),
            vcline: None,
            term: None,
            initstring: None,
            issue: None,
            erasechars: None,
            killchars: None,
            delay: 0,
            nice: 0,
            speeds: Vec::new(),
            clocal: Clocal::Auto,
            tty_is_stdin: false,
            fakehost: None,
            kbmode: 0,
        }
    }
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Run on the line the options name.
    Run(Options),
    /// `--show-issue`: the issue file, evaluated, and exit.
    ShowIssue(Options),
    /// `--help`, `--version`, `--list-speeds`: this, on standard output,
    /// and exit 0.
    Print(Vec<u8>),
    /// `--reload`.
    Reload,
}

/// `is_speed (str)`: nothing but digits and commas.
fn is_speed(s: &[u8]) -> bool {
    s.iter().all(|&b| b.is_ascii_digit() || b == b',')
}

/// `parse_args (argc, argv, op)`, `argv[0]` not among `args`.
///
/// # Errors
///
/// How the run ends, having said why: a bad option, a bad number, a speed
/// the table does not have, no line.
pub fn parse_args(args: &[OsString], short: &[u8], diag: &mut dyn Diag) -> Result<Parsed, Exit> {
    let mut op = Options::default();
    let mut show_issue = false;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let parser = AGETTY.parse(args, SHORTS, LONGS);
    for item in parser {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                diag.getopt(e.sentence.as_bytes());
                return Err(diag.errtryhelp());
            }
        };
        let value =
            |v: &Option<OsString>| v.as_deref().map(os_bytes).unwrap_or_default().into_owned();
        match opt {
            Opt::Operand(w) => operands.push(os_bytes(w).into_owned()),
            Opt::Short(b'8', _) | Opt::Long("8bits", _) => op.flags |= F_EIGHTBITS,
            Opt::Short(b'a', v) | Opt::Long("autologin", v) => op.autolog = Some(value(&v)),
            Opt::Short(b'c', _) | Opt::Long("noreset", _) => op.flags |= F_KEEPCFLAGS,
            Opt::Short(b'C', v) | Opt::Long("chdir", v) => op.chdir = Some(value(&v)),
            Opt::Short(b'd', v) | Opt::Long("delay", v) => {
                op.delay = strtou32_or_err(&value(&v), "invalid delay argument", diag)?;
            }
            Opt::Short(b'E', _) | Opt::Long("remote", _) => op.flags |= F_REMOTE,
            Opt::Short(b'f', v) | Opt::Long("issue-file", v) => op.issue = Some(value(&v)),
            Opt::Short(b'h', _) | Opt::Long("flow-control", _) => op.flags |= F_RTSCTS,
            Opt::Short(b'H', v) | Opt::Long("host", v) => op.fakehost = Some(value(&v)),
            Opt::Short(b'i', _) | Opt::Long("noissue", _) => op.flags &= !F_ISSUE,
            Opt::Short(b'I', v) | Opt::Long("init-string", v) => {
                op.initstring = Some(init_special_char(&value(&v)));
                op.flags |= F_INITSTRING;
            }
            Opt::Short(b'J', _) | Opt::Long("noclear", _) => op.flags |= F_NOCLEAR,
            Opt::Short(b'l', v) | Opt::Long("login-program", v) => op.login = value(&v),
            Opt::Short(b'L', v) | Opt::Long("local-line", v) => {
                // `-L` and `-L=always` mean the same. The `=` is part of
                // the value: `-L=never`, `--local-line==never`.
                op.clocal = match v.as_deref().map(os_bytes).as_deref() {
                    None | Some(b"=always") => Clocal::Always,
                    Some(b"=never") => Clocal::Never,
                    Some(b"=auto") => Clocal::Auto,
                    Some(_) => return Err(diag.log_err(b"invalid argument of --local-line")),
                };
            }
            Opt::Short(b'm', _) | Opt::Long("extract-baud", _) => op.flags |= F_PARSE,
            Opt::Short(b'n', _) | Opt::Long("skip-login", _) => op.flags |= F_NOPROMPT,
            Opt::Short(b'N', _) | Opt::Long("nonewline", _) => op.flags |= F_NONL,
            Opt::Short(b'o', v) | Opt::Long("login-options", v) => op.logopt = Some(value(&v)),
            Opt::Short(b'p', _) | Opt::Long("login-pause", _) => op.flags |= F_LOGINPAUSE,
            Opt::Short(b'P', v) | Opt::Long("nice", v) => {
                op.nice = strtos32_or_err(&value(&v), "invalid nice argument", diag)?;
            }
            Opt::Short(b'r', v) | Opt::Long("chroot", v) => op.chroot = Some(value(&v)),
            Opt::Short(b'R', _) | Opt::Long("hangup", _) => op.flags |= F_HANGUP,
            Opt::Short(b's', _) | Opt::Long("keep-baud", _) => op.flags |= F_KEEPSPEED,
            Opt::Short(b't', v) | Opt::Long("timeout", v) => {
                op.timeout = strtou32_or_err(&value(&v), "invalid timeout argument", diag)?;
            }
            Opt::Short(b'U', _) | Opt::Long("detect-case", _) => op.flags |= F_LCUC,
            Opt::Short(b'w', _) | Opt::Long("wait-cr", _) => op.flags |= F_WAITCRLF,
            Opt::Long("nohints", _) => op.flags |= F_NOHINTS,
            Opt::Long("nohostname", _) => op.flags |= F_NOHOSTNAME,
            Opt::Long("long-hostname", _) => op.flags |= F_LONGHNAME,
            Opt::Long("erase-chars", v) => op.erasechars = Some(value(&v)),
            Opt::Long("kill-chars", v) => op.killchars = Some(value(&v)),
            Opt::Long("reload", _) => return Ok(Parsed::Reload),
            Opt::Long("list-speeds", _) => return Ok(Parsed::Print(speeds::list())),
            Opt::Long("show-issue", _) => show_issue = true,
            Opt::Long("version", _) => return Ok(Parsed::Print(version(short))),
            Opt::Long("help", _) => return Ok(Parsed::Print(usage(short))),
            // Unreachable: every option in the tables has its case.
            Opt::Short(..) | Opt::Long(..) => return Err(diag.errtryhelp()),
        }
    }

    if show_issue {
        return Ok(Parsed::ShowIssue(op));
    }

    // "Accept "tty", "baudrate tty", and "tty baudrate"."
    let mut operands = operands.into_iter();
    let Some(first) = operands.next() else {
        return Err(not_enough(diag));
    };
    let speed_first = is_speed(&first);
    if speed_first {
        parse_speeds(&mut op, &first, diag)?;
        let Some(tty) = operands.next() else {
            return Err(not_enough(diag));
        };
        op.tty = tty;
    } else {
        op.tty = first;
    }
    let mut rest = operands.peekable();
    // A speed after the line, if the line came first.
    if !speed_first && let Some(v) = rest.next_if(|v| is_speed(v)) {
        parse_speeds(&mut op, &v, diag)?;
    }

    if op.tty == b"-" {
        op.tty_is_stdin = true;
        match terminal_name() {
            Ok(name) => op.tty = name,
            // Upstream has set the name to NULL by now, and reads it as a
            // string a moment later; here the line stays `-`.
            Err(code) => diag.log_warn(format!("could not get terminal name: {code}").as_bytes()),
        }
    }

    // "On virtual console remember the line which is used for".
    if let Some(n) = op.tty.strip_prefix(b"tty")
        && n.iter().all(u8::is_ascii_digit)
    {
        op.vcline = Some(n.to_vec());
    }

    op.term = rest.next();
    Ok(Parsed::Run(op))
}

/// `get_terminal_name (NULL, &name, NULL)`: the name of the terminal on the
/// first of standard input, output and error that is one, without its
/// `/dev/`.
///
/// # Errors
///
/// `-EINVAL` when none of the three is a terminal, -1 when `ttyname` fails.
fn terminal_name() -> Result<Vec<u8>, i32> {
    let fd = [0, 1, 2]
        .into_iter()
        .find(|&fd| libcall::fd::is_terminal(fd))
        .ok_or(-libcall::EINVAL)?;
    let mut buf = [0u8; 4096];
    let len = libcall::termios::ttyname_into(fd, &mut buf).map_err(|_| -1)?;
    let path = buf.get(..len).ok_or(-1)?;
    Ok(path.strip_prefix(b"/dev/").unwrap_or(path).to_vec())
}

/// `log_warn` and `errx (EXIT_FAILURE, "not enough arguments")`.
fn not_enough(diag: &mut dyn Diag) -> Exit {
    diag.log_warn(b"not enough arguments");
    diag.errx(b"not enough arguments")
}

/// `parse_speeds (op, arg)`: the comma-separated speeds, each by `bcode`.
fn parse_speeds(op: &mut Options, arg: &[u8], diag: &mut dyn Diag) -> Result<(), Exit> {
    // `strtok (str, ",")`: empty fields are skipped.
    for cp in arg.split(|&b| b == b',').filter(|f| !f.is_empty()) {
        let code = speeds::bcode(cp);
        op.speeds.push(code);
        if code == 0 {
            let mut msg = b"bad speed: ".to_vec();
            msg.extend_from_slice(cp);
            return Err(diag.log_err(&msg));
        }
        if op.speeds.len() >= MAX_SPEED {
            return Err(diag.log_err(b"too many alternate speeds"));
        }
    }
    Ok(())
}

/// `init_special_char (arg, op)`: the init string, `\\` a backslash and
/// `\ddd` (up to three octal digits) a byte.
#[must_use]
pub fn init_special_char(arg: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(arg.len());
    let mut p = arg.iter().copied().peekable();
    while let Some(c) = p.next() {
        if c != b'\\' {
            out.push(c);
            continue;
        }
        if p.next_if_eq(&b'\\').is_some() {
            out.push(b'\\');
            continue;
        }
        // `char ch = 0; ch <<= 3; ch += digit`, for up to three digits: a
        // `char`, so a value past 255 wraps as it does there.
        let mut ch: u8 = 0;
        for _ in 0..3 {
            match p.next_if(|d| (b'0'..=b'7').contains(d)) {
                Some(d) => ch = ch.wrapping_shl(3).wrapping_add(d.wrapping_sub(b'0')),
                None => break,
            }
        }
        out.push(ch);
    }
    out
}

/// `strtou32_or_err (arg, errmesg)`.
fn strtou32_or_err(arg: &[u8], errmesg: &str, diag: &mut dyn Diag) -> Result<u32, Exit> {
    ulstrutils::ul_strtou64(arg, 10)
        .and_then(|n| u32::try_from(n).map_err(|_| ulstrutils::NumErr::Range))
        .map_err(|e| {
            let m = ulstrutils::num_error_message(errmesg, &os_from_bytes(arg), e);
            diag.errx(m.as_bytes())
        })
}

/// `strtos32_or_err (arg, errmesg)`.
fn strtos32_or_err(arg: &[u8], errmesg: &str, diag: &mut dyn Diag) -> Result<i32, Exit> {
    ulstrutils::ul_strtos32(arg, 10).map_err(|e| {
        let m = ulstrutils::num_error_message(errmesg, &os_from_bytes(arg), e);
        diag.errx(m.as_bytes())
    })
}

/// `usage ()`'s text.
#[must_use]
pub fn usage(short: &[u8]) -> Vec<u8> {
    // The name is shown as it was given; a name that is not UTF-8 is
    // shown by its bytes below.
    let name = "\u{0}NAME\u{0}";
    let mut t = format!(
        "\nUsage:\n {name} [options] <line> [<baud_rate>,...] [<termtype>]\n \
         {name} [options] <baud_rate>,... <line> [<termtype>]\n\nOpen a terminal and set its mode.\n\nOptions:\n"
    );
    t.push_str(
        " -8, --8bits                assume 8-bit tty\n \
         -a, --autologin <user>     login the specified user automatically\n \
         -c, --noreset              do not reset control mode\n \
         -E, --remote               use -r <hostname> for login(1)\n \
         -f, --issue-file <list>    display issue files or directories\n     \
         --show-issue           display issue file and exit\n \
         -h, --flow-control         enable hardware flow control\n \
         -H, --host <hostname>      specify login host\n \
         -i, --noissue              do not display issue file\n \
         -I, --init-string <string> set init string\n \
         -J  --noclear              do not clear the screen before prompt\n \
         -l, --login-program <file> specify login program\n \
         -L, --local-line[=<mode>]  control the local line flag\n \
         -m, --extract-baud         extract baud rate during connect\n \
         -n, --skip-login           do not prompt for login\n \
         -N  --nonewline            do not print a newline before issue\n \
         -o, --login-options <opts> options that are passed to login\n \
         -p, --login-pause          wait for any key before the login\n \
         -r, --chroot <dir>         change root to the directory\n \
         -R, --hangup               do virtually hangup on the tty\n \
         -s, --keep-baud            try to keep baud rate after break\n \
         -t, --timeout <number>     login process timeout\n \
         -U, --detect-case          detect uppercase terminal\n \
         -w, --wait-cr              wait carriage-return\n     \
         --nohints              do not print hints\n     \
         --nohostname           no hostname at all will be shown\n     \
         --long-hostname        show full qualified hostname\n     \
         --erase-chars <string> additional backspace chars\n     \
         --kill-chars <string>  additional kill chars\n     \
         --chdir <directory>    chdir before the login\n     \
         --delay <number>       sleep seconds before prompt\n     \
         --nice <number>        run login with this priority\n     \
         --reload               reload prompts on running agetty instances\n     \
         --list-speeds          display supported baud rates\n     \
         --help                 display this help\n     \
         --version              display version\n\
         \nFor more details see agetty(8).\n",
    );
    replace_name(t.into_bytes(), short)
}

/// The text with every `\0NAME\0` marker replaced by `short`, byte for
/// byte.
fn replace_name(text: Vec<u8>, short: &[u8]) -> Vec<u8> {
    const MARK: &[u8] = b"\0NAME\0";
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_slice();
    while let Some(at) = rest.windows(MARK.len()).position(|w| w == MARK) {
        out.extend_from_slice(rest.get(..at).unwrap_or_default());
        out.extend_from_slice(short);
        rest = rest
            .get(at.saturating_add(MARK.len())..)
            .unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}

/// `output_version ()`: the program, the package, and the features this
/// build has -- upstream's but `plymouth`, which SlateOS has no boot splash
/// for.
#[must_use]
pub fn version(short: &[u8]) -> Vec<u8> {
    let mut v = short.to_vec();
    v.extend_from_slice(b" from util-linux 2.39.3 (flow control, hints, issue, issue.d, keyboard mode, reload, syslog, widechar)\n");
    v
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;
    use crate::tests::Said;

    fn parse(words: &[&str]) -> (Result<Parsed, Exit>, Said) {
        let args: Vec<OsString> = words.iter().map(OsString::from).collect();
        let mut said = Said::default();
        let r = parse_args(&args, b"agetty", &mut said);
        (r, said)
    }

    fn run(words: &[&str]) -> Options {
        match parse(words).0 {
            Ok(Parsed::Run(op)) => op,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_line_the_speeds_and_the_type_in_either_order() {
        let op = run(&["tty1"]);
        assert_eq!(
            (op.tty.as_slice(), op.vcline.as_deref(), op.speeds.len()),
            (&b"tty1"[..], Some(&b"1"[..]), 0)
        );
        let op = run(&["115200,9600", "ttyS0", "vt102"]);
        assert_eq!(op.tty, b"ttyS0");
        assert_eq!(op.speeds, [0o10002, 0o15]);
        assert_eq!(op.term.as_deref(), Some(&b"vt102"[..]));
        assert_eq!(op.vcline, None);
        let op = run(&["ttyS1", "38400", "linux"]);
        assert_eq!(
            (op.speeds.as_slice(), op.term.as_deref()),
            (&[0o17][..], Some(&b"linux"[..]))
        );
        // Not a speed after the line: it is the type.
        let op = run(&["tty2", "xterm"]);
        assert_eq!(
            (op.speeds.len(), op.term.as_deref()),
            (0, Some(&b"xterm"[..]))
        );
    }

    #[test]
    fn a_line_is_needed_and_a_speed_must_be_one() {
        let (r, said) = parse(&[]);
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(said.stderr, b"agetty: not enough arguments\n");
        assert_eq!(said.syslog, [b"not enough arguments".to_vec()]);
        let (r, said) = parse(&["9600"]);
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(said.stderr, b"agetty: not enough arguments\n");
        let (r, said) = parse(&["9601", "ttyS0"]);
        assert_eq!((r, said.stderr.len()), (Err(Exit(1)), 0));
        assert_eq!(said.syslog, [b"bad speed: 9601".to_vec()]);
        assert!(said.slow_exit);
    }

    #[test]
    fn options_set_their_fields() {
        let op = run(&[
            "-8",
            "-a",
            "root",
            "-c",
            "-E",
            "-f",
            "/x:/y",
            "-h",
            "-H",
            "host",
            "-i",
            "-J",
            "-l",
            "/bin/login2",
            "-m",
            "-n",
            "-N",
            "-o",
            "-p -- \\u",
            "-p",
            "-P",
            "5",
            "-r",
            "/root",
            "-R",
            "-s",
            "-t",
            "60",
            "-U",
            "-w",
            "--nohints",
            "--nohostname",
            "--long-hostname",
            "--erase-chars",
            "#",
            "--kill-chars",
            "@",
            "--chdir",
            "/tmp",
            "--delay",
            "3",
            "tty3",
        ]);
        for f in [
            F_EIGHTBITS,
            F_KEEPCFLAGS,
            F_REMOTE,
            F_RTSCTS,
            F_NOCLEAR,
            F_PARSE,
            F_NOPROMPT,
            F_NONL,
            F_LOGINPAUSE,
            F_HANGUP,
            F_KEEPSPEED,
            F_LCUC,
            F_WAITCRLF,
            F_NOHINTS,
            F_NOHOSTNAME,
            F_LONGHNAME,
        ] {
            assert_ne!(op.flags & f, 0, "{f:#x}");
        }
        assert_eq!(op.flags & F_ISSUE, 0);
        assert_eq!(op.autolog.as_deref(), Some(&b"root"[..]));
        assert_eq!(op.issue.as_deref(), Some(&b"/x:/y"[..]));
        assert_eq!(op.fakehost.as_deref(), Some(&b"host"[..]));
        assert_eq!(op.login, b"/bin/login2");
        assert_eq!(op.logopt.as_deref(), Some(&b"-p -- \\u"[..]));
        assert_eq!((op.nice, op.timeout, op.delay), (5, 60, 3));
        assert_eq!(op.chroot.as_deref(), Some(&b"/root"[..]));
        assert_eq!(op.chdir.as_deref(), Some(&b"/tmp"[..]));
        assert_eq!(op.erasechars.as_deref(), Some(&b"#"[..]));
        assert_eq!(op.killchars.as_deref(), Some(&b"@"[..]));
    }

    #[test]
    fn local_line_takes_its_value_with_the_equals_sign() {
        assert_eq!(run(&["-L", "tty1"]).clocal, Clocal::Always);
        assert_eq!(run(&["-L=never", "tty1"]).clocal, Clocal::Never);
        assert_eq!(run(&["--local-line==auto", "tty1"]).clocal, Clocal::Auto);
        let (r, said) = parse(&["-Lnever", "tty1"]);
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(said.syslog, [b"invalid argument of --local-line".to_vec()]);
    }

    #[test]
    fn numbers_are_strtou32_s() {
        let (r, said) = parse(&["-t", "x", "tty1"]);
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(said.stderr, b"agetty: invalid timeout argument: 'x'\n");
        let (r, said) = parse(&["--delay", "-1", "tty1"]);
        assert_eq!(r, Err(Exit(1)));
        assert!(
            said.stderr
                .starts_with(b"agetty: invalid delay argument: '-1'")
        );
        assert_eq!(run(&["-P", "-5", "tty1"]).nice, -5);
    }

    #[test]
    fn a_bad_option_refers_to_the_help() {
        let (r, said) = parse(&["--bogus"]);
        assert_eq!(r, Err(Exit(1)));
        assert_eq!(
            said.stderr,
            b"agetty: unrecognized option '--bogus'\nTry 'agetty --help' for more information.\n"
        );
    }

    #[test]
    fn help_version_and_speeds_print_and_end() {
        assert_eq!(parse(&["--help"]).0, Ok(Parsed::Print(usage(b"agetty"))));
        assert_eq!(
            parse(&["--version", "--help"]).0,
            Ok(Parsed::Print(version(b"agetty")))
        );
        assert!(usage(b"getty").starts_with(b"\nUsage:\n getty [options] <line>"));
        assert_eq!(
            parse(&["--list-speeds"]).0,
            Ok(Parsed::Print(speeds::list()))
        );
        assert!(matches!(
            parse(&["--show-issue"]).0,
            Ok(Parsed::ShowIssue(_))
        ));
        assert_eq!(parse(&["--reload", "tty1"]).0, Ok(Parsed::Reload));
    }

    #[test]
    fn an_init_string_decodes_its_octal() {
        assert_eq!(init_special_char(b"ATZ\\015"), b"ATZ\r");
        assert_eq!(init_special_char(b"a\\\\b"), b"a\\b");
        assert_eq!(init_special_char(b"\\7x"), b"\x07x");
        assert_eq!(init_special_char(b"\\777"), [0o777u32 as u8]);
        assert_eq!(init_special_char(b"\\q"), b"\0q");
    }
}
