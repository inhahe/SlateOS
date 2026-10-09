//! Run a command again and again, its output shown full screen: procps-ng
//! 4.0.4's `watch`, ported -- built as Debian builds it, with
//! `--enable-watch8bit`, so the output is read as the locale's multibyte
//! characters and drawn through ncursesw's wide calls, and with
//! `--enable-colorwatch`, so colour is on unless `-C` turns it off. Ubuntu
//! ships `src/watch.c` unpatched, so this is the program Ubuntu ships too.
//!
//! ```text
//! watch [options] command
//! ```
//!
//! A transcription of `src/watch.c`, drawn through the curses crate
//! (`userspace/curses`), which is ncursesw's screen library ported. What
//! upstream does and this keeps:
//!
//! - Options stop at the first operand (`+` in the option string), and the
//!   command is the rest of the arguments joined by single spaces, run by
//!   `sh -c` through the C library's `system` in a child of its own -- or with
//!   `-x`, the arguments themselves through `execvp`. The child's standard
//!   output and error are one pipe, which is read as the screen fills and
//!   closed when it is full, so a command that writes more than fits may die
//!   of `SIGPIPE`, which counts as a failure.
//! - `COLUMNS` and `LINES` are read once, through `strtol` at base 0: a
//!   positive number is the size; anything else is -1, exported as such, and
//!   then the size of the terminal on *standard error* replaces it. The size
//!   is exported back for the command. (`COLUMNS=abc` and no terminal leaves
//!   a width of -1, and an empty screen.)
//! - The title is `Every N.Ns: `, the command, and the host and `ctime`'s
//!   date -- its newline included, which moves the cursor to the next line --
//!   cut to fit as upstream cuts them, the command by the columns of its wide
//!   characters.
//! - The output is read a character at a time as `my_getwc` reads it:
//!   `mbtowc` over one more byte at a time, a character that takes sixteen
//!   bytes given back but the first and made the end of the input for that
//!   read, and the byte 0xFF -- stored in a `char` and compared with `EOF` --
//!   taken for the end too. A tab moves to the next multiple of eight; a bell
//!   beeps and skips a column; a wide character that will not fit at the end
//!   of a line moves to the next; a character of no width backs up a column.
//! - `-d` stands out what changed since the last screen (with `=permanent`,
//!   everything that ever changed); `-g` ends the program when anything
//!   changed, `-q N` when nothing changed for N screens; `-e` waits for a key
//!   after a failure, then exits with status 8; `-b` beeps after one. The
//!   SGR colour and style sequences the command writes are drawn, as far as
//!   upstream understands them, on a terminal with colours and without `-C`;
//!   otherwise they are drawn as text, the escape as `^[`.
//! - `SIGINT`, `SIGTERM` and `SIGHUP` put the terminal back and exit with
//!   status 0; `SIGWINCH` makes the next screen measure the terminal again,
//!   resize and redraw. The wait between screens ends at a signal.
//!
//! # Deliberately different
//!
//! - `-v`/`--version` names this build.
//! - A refused number goes through `quoteaf` (`free`'s divergence 6), and one
//!   refused with no reason of its own is reported with none, where upstream
//!   appends whatever `errno` was left over (`free`'s divergence 3).
//! - An SGR sequence of a hundred parameter bytes with no `m`: upstream reads
//!   past the end of its buffer looking for the end of the string; this
//!   reads the hundred bytes as the parameters.

use std::ffi::{CString, OsString};
use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

use coreutils::cfmt::{self, Spec, Value};
use coreutils::extfloat::ExtF80;
use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::cvt;
use coreutils::procps::strutils;
use coreutils::quote::{os_bytes, quoteaf};
use coreutils::stdfd::{self, Stream};
use curses::{
    A_ATTRIBUTES, A_BLINK, A_BOLD, A_DIM, A_ITALIC, A_NORMAL, A_REVERSE, A_UNDERLINE, Attr, WChar,
};
use libcall::locale::{self, Mb, MbState};

/// The parser. Its name is never printed: getopt's complaints carry
/// `argv[0]`.
const WATCH: Program = Program::new("watch", 1);

/// Upstream's `getopt_long` string: options end at the first operand.
const SHORT_OPTIONS: &str = "+bCced::ghq:n:prtwvx";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("color", Takes::Nothing),
    ("no-color", Takes::Nothing),
    ("differences", Takes::Optional),
    ("help", Takes::Nothing),
    ("interval", Takes::Required),
    ("beep", Takes::Nothing),
    ("errexit", Takes::Nothing),
    ("chgexit", Takes::Nothing),
    ("equexit", Takes::Required),
    ("exec", Takes::Nothing),
    ("precise", Takes::Nothing),
    ("no-rerun", Takes::Nothing),
    ("no-title", Takes::Nothing),
    ("no-wrap", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The usage text after the program's name.
const USAGE_REST: &str = concat!(
    " [options] command\n",
    "\n",
    "Options:\n",
    "  -b, --beep             beep if command has a non-zero exit\n",
    "  -c, --color            interpret ANSI color and style sequences\n",
    "  -C, --no-color         do not interpret ANSI color and style sequences\n",
    "  -d, --differences[=<permanent>]\n",
    "                         highlight changes between updates\n",
    "  -e, --errexit          exit if command has a non-zero exit\n",
    "  -g, --chgexit          exit when output from command changes\n",
    "  -q, --equexit <cycles>\n",
    "                         exit when output from command does not change\n",
    "  -n, --interval <secs>  seconds to wait between updates\n",
    "  -p, --precise          attempt run command in precise intervals\n",
    "  -r, --no-rerun         do not rerun program on window resize\n",
    "  -t, --no-title         turn off header\n",
    "  -w, --no-wrap          turn off line wrapping\n",
    "  -x, --exec             pass command to exec instead of \"sh -c\"\n",
    "\n",
    " -h, --help     display this help and exit\n",
    " -v, --version  output version information and exit\n",
    "\n",
    "For more details see watch(1).\n",
);

/// The flags.
const WATCH_DIFF: u32 = 1 << 1;
const WATCH_CUMUL: u32 = 1 << 2;
const WATCH_EXEC: u32 = 1 << 3;
const WATCH_BEEP: u32 = 1 << 4;
const WATCH_COLOR: u32 = 1 << 5;
const WATCH_ERREXIT: u32 = 1 << 6;
const WATCH_CHGEXIT: u32 = 1 << 7;
const WATCH_EQUEXIT: u32 = 1 << 8;
const WATCH_NORERUN: u32 = 1 << 9;

/// `MAX_ANSIBUF`.
const MAX_ANSIBUF: usize = 100;
/// `MAX_ENC_BYTES`: the most bytes `my_getwc` takes for one character.
const MAX_ENC_BYTES: usize = 16;
/// `USECS_PER_SEC`.
const USECS_PER_SEC: u64 = 1_000_000;
/// `UINT_MAX`, the interval's ceiling.
const UINT_MAX: f64 = 4_294_967_295.0;
/// `WEOF`.
const WEOF: u32 = u32::MAX;
/// `EOF`.
const EOF: i32 = -1;
/// The size of glibc's buffer for a pipe or a terminal read through stdio.
const STDIO_PIPE_BUFFER: usize = 4096;

/// `screen_size_changed`: a `SIGWINCH` came.
static SCREEN_SIZE_CHANGED: AtomicBool = AtomicBool::new(false);

/// `die`: the terminal put back, then `exit (EXIT_SUCCESS)`.
extern "C" fn die(_sig: i32) {
    let _errno = libcall::signal::ErrnoGuard::save();
    curses::end_and_exit(0);
}

/// `winch_handler`.
extern "C" fn winch_handler(_sig: i32) {
    SCREEN_SIZE_CHANGED.store(true, Ordering::Relaxed);
}

/// `program_invocation_short_name`: what follows `argv[0]`'s last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or(argv0)
}

/// glibc's `error (status, errnum, ...)`: standard output delivered, then
/// `watch: MESSAGE[: REASON]`, then `exit (status)` -- in curses' mode or
/// not, as upstream's `xerr` leaves it.
fn xerr(status: i32, msg: &[u8], errnum: Option<i32>) -> ! {
    let mut m = b"watch: ".to_vec();
    m.extend_from_slice(msg);
    if let Some(e) = errnum.filter(|&e| e != 0) {
        m.extend_from_slice(b": ");
        m.extend_from_slice(
            coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(e)).as_bytes(),
        );
    }
    m.push(b'\n');
    stdfd::diag_bytes(&m);
    std::process::exit(status)
}

/// `(int) v` of a `long`: its low 32 bits.
fn int(v: i64) -> i32 {
    let [a, b, c, d, ..] = v.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// `wcwidth`, of a `wint_t` as C passes it.
fn wcwidth(c: u32) -> i32 {
    locale::wcwidth(c.cast_signed())
}

/// `wcswidth (s, n)`: the columns of the first `n` characters (all, for a
/// negative `n`), or -1 when one of them has none to show.
fn wcswidth(s: &[WChar], n: i64) -> i32 {
    let take = usize::try_from(n).unwrap_or(usize::MAX);
    let mut total: i32 = 0;
    for &c in s.iter().take(take) {
        let w = locale::wcwidth(c);
        if w < 0 {
            return -1;
        }
        total = total.wrapping_add(w);
    }
    total
}

/// `mbstowcs`: the command's characters in the selected locale, or `None`
/// where it is not text in it.
fn mbstowcs(s: &[u8]) -> Option<Vec<WChar>> {
    let mut out = Vec::new();
    let mut state = MbState::new();
    let mut rest = s;
    while !rest.is_empty() {
        match locale::mbrtowc(rest, &mut state) {
            Mb::Char(wc, n) if n > 0 => {
                out.push(wc);
                rest = rest.get(n..).unwrap_or_default();
            }
            // A NUL ends the string; there is none in an argument.
            Mb::Char(..) => break,
            // What C's string would end with -- the NUL -- does not finish
            // a character, so an incomplete one is as bad as a wrong one.
            Mb::Incomplete | Mb::Invalid => return None,
        }
    }
    Some(out)
}

/// The child's output, read as stdio reads a pipe: a buffer of what one
/// `read` gave, bytes given back ahead of it, and the end of the input kept
/// once seen.
struct Pipe {
    fd: i32,
    buf: Vec<u8>,
    pos: usize,
    pushed: Vec<u8>,
    eof: bool,
}

impl Pipe {
    fn new(fd: i32) -> Self {
        Self {
            fd,
            buf: Vec::new(),
            pos: 0,
            pushed: Vec::new(),
            eof: false,
        }
    }

    /// `getc`: a byte as an `unsigned char`, or `EOF`.
    fn getc(&mut self) -> i32 {
        if let Some(b) = self.pushed.pop() {
            return i32::from(b);
        }
        if let Some(&b) = self.buf.get(self.pos) {
            self.pos = self.pos.saturating_add(1);
            return i32::from(b);
        }
        if self.eof {
            return EOF;
        }
        let mut chunk = vec![0u8; STDIO_PIPE_BUFFER];
        loop {
            match libcall::fd::read(self.fd, &mut chunk) {
                Ok(0) => {
                    self.eof = true;
                    return EOF;
                }
                Ok(n) => {
                    chunk.truncate(n);
                    self.buf = chunk;
                    self.pos = 1;
                    return self.buf.first().map_or(EOF, |&b| i32::from(b));
                }
                // The handlers restart what they interrupt.
                Err(libcall::EINTR) => {}
                // A read that fails is the end of the input to stdio.
                Err(_) => {
                    self.eof = true;
                    return EOF;
                }
            }
        }
    }

    /// `ungetc (c)`: `c` read again next, and the end of the input
    /// forgotten; `EOF` is not given back.
    fn ungetc(&mut self, c: i32) {
        if let Ok(b) = u8::try_from(c) {
            self.pushed.push(b);
            self.eof = false;
        }
    }

    /// `my_getwc`: the next character, `mbtowc` over one more byte at a
    /// time. `getc`'s answer is kept in a `char`, so 0xFF compares equal to
    /// `EOF` and ends the character as the end of the input does.
    fn my_getwc(&mut self) -> u32 {
        let mut bytes = [0u8; MAX_ENC_BYTES];
        let mut byte = 0usize;
        loop {
            let c = self.getc();
            // `(char) c == EOF`: the end, or the byte 0xFF.
            let [stored, ..] = c.to_le_bytes();
            if stored == 0xff {
                return WEOF;
            }
            if let Some(slot) = bytes.get_mut(byte) {
                *slot = stored;
            }
            byte = byte.saturating_add(1);
            // `mbtowc (NULL, NULL, 0)`, then `mbtowc (&rval, i, byte)`.
            let have = bytes.get(..byte).unwrap_or_default();
            if let Mb::Char(wc, n) = locale::mbrtowc(have, &mut MbState::new())
                && n > 0
            {
                return wc.cast_unsigned();
            }
            if byte == MAX_ENC_BYTES {
                // "at least *try* to fix up": all but the first given back.
                while byte > 1 {
                    byte = byte.saturating_sub(1);
                    self.ungetc(i32::from(bytes.get(byte).copied().unwrap_or(0)));
                }
                return WEOF;
            }
        }
    }

    /// `fclose`: the read end closed, whatever was left unread with it.
    fn close(self) {
        // Nothing to report: upstream does not look at `fclose`'s answer.
        let _ = libcall::fd::close(self.fd);
    }
}

/// The program's state: upstream's file-scope variables.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's file-scope variables, one for one"
)]
struct Watch {
    flags: u32,
    height: i64,
    width: i64,
    first_screen: bool,
    show_title: i64,
    precise_timekeeping: bool,
    line_wrap: bool,
    nr_of_colors: i32,
    attributes: Attr,
    fg_col: i32,
    bg_col: i32,
    more_colors: bool,
    /// `int`s, which `strtol`'s `long` is cut to: `COLUMNS=4294967376` is
    /// 80 columns, and `2147483648` a negative number.
    incoming_cols: i32,
    incoming_rows: i32,
    /// The locale's decimal point, for `%.1f`.
    radix: Vec<u8>,
    /// The time zone `ctime` reads.
    zone: localtime::Zone,
}

/// Set an environment variable, as upstream's `putenv` of a static buffer
/// sets it: `NAME=VALUE` formatted into 24 bytes, so cut to 23.
fn putenv_num(name: &str, value: i64) {
    let mut text = format!("{name}={value}").into_bytes();
    text.truncate(23);
    let value = text.get(name.len().saturating_add(1)..).unwrap_or_default();
    let value = coreutils::quote::os_from_bytes(value);
    // SAFETY: watch has one thread -- its command runs in a process of its
    // own -- so nothing reads the environment while it changes.
    unsafe { std::env::set_var(name, value) };
}

/// `strtol (s, &end, 0)` read whole and positive, as `get_terminal_size`
/// takes `COLUMNS` and `LINES`; -1 otherwise.
fn incoming(s: &[u8]) -> i64 {
    let (t, used) = cstrtol::strtol(s, 0);
    if used == s.len() && t > 0 { t } else { -1 }
}

impl Watch {
    /// `reset_ansi`.
    fn reset_ansi(&mut self) {
        self.attributes = A_NORMAL;
        self.fg_col = 0;
        self.bg_col = 0;
    }

    /// `init_ansi_colors`.
    fn init_ansi_colors(&mut self) {
        self.nr_of_colors = 9;
        self.more_colors = curses::colors() >= 16 && curses::color_pairs() >= 16 * 16;
        if self.more_colors {
            // "Initialize using ANSI SGR 8-bit specified colors": what
            // `init_color` answers is not looked at.
            for (n, rgb) in [
                (8, (333, 333, 333)),
                (9, (1000, 333, 333)),
                (10, (333, 1000, 333)),
                (11, (1000, 1000, 333)),
                (12, (333, 333, 1000)),
                (13, (1000, 333, 1000)),
                (14, (333, 1000, 1000)),
            ] {
                let _ = curses::init_color(n, rgb.0, rgb.1, rgb.2);
            }
            self.nr_of_colors = self.nr_of_colors.wrapping_add(7);
        }
        let n = self.nr_of_colors;
        let short = |v: i32| {
            let [a, b, ..] = v.to_le_bytes();
            i16::from_le_bytes([a, b])
        };
        for bg in 0..n {
            for fg in 0..n {
                let pair = bg.wrapping_mul(n).wrapping_add(fg).wrapping_add(1);
                let _ = curses::init_pair(
                    short(pair),
                    short(fg.wrapping_sub(1)),
                    short(bg.wrapping_sub(1)),
                );
            }
        }
        self.reset_ansi();
    }

    /// `process_ansi_color_escape_sequence`: the colour `;5;N` names, as a
    /// colour index plus one; 0 for anything not understood. `seq` is the
    /// parameters from the one after `38` or `48`, and is left after what
    /// was read.
    fn ansi_colour(&self, seq: &[u8], at: &mut usize) -> i32 {
        let rest = seq.get(*at..).unwrap_or_default();
        if rest.first() != Some(&b';') {
            return 0;
        }
        if rest.get(1) == Some(&b'5') {
            if rest.get(2) != Some(&b';') {
                return 0;
            }
            let digits = rest.get(3..).unwrap_or_default();
            let (num, used) = cstrtol::strtol(digits, 10);
            *at = at.saturating_add(3).saturating_add(used);
            let num = int(num);
            if (0..=7).contains(&num) {
                return num.wrapping_add(1);
            }
            if (8..=15).contains(&num) {
                return if self.more_colors {
                    num.wrapping_add(1)
                } else {
                    num.wrapping_sub(8).wrapping_add(1)
                };
            }
        }
        0
    }

    /// `set_ansi_attribute (attrib, escape_sequence)`: whether it was
    /// understood -- and if it was, the window's attributes and pair set.
    fn set_ansi_attribute(&mut self, attrib: i32, seq: Option<(&[u8], &mut usize)>) -> bool {
        match attrib {
            -1 => {}
            0 => self.reset_ansi(),
            1 => self.attributes |= A_BOLD,
            2 => self.attributes |= A_DIM,
            3 => self.attributes |= A_ITALIC,
            4 => self.attributes |= A_UNDERLINE,
            5 => self.attributes |= A_BLINK,
            7 => self.attributes |= A_REVERSE,
            21 => self.attributes &= !A_BOLD,
            22 => self.attributes &= !(A_BOLD | A_DIM),
            23 => self.attributes &= !A_ITALIC,
            24 => self.attributes &= !A_UNDERLINE,
            25 => self.attributes &= !A_BLINK,
            27 => self.attributes &= !A_REVERSE,
            38 => {
                self.fg_col = match seq {
                    Some((s, at)) => self.ansi_colour(s, at),
                    None => 0,
                };
                if self.fg_col == 0 {
                    return false;
                }
            }
            39 => self.fg_col = 0,
            48 => {
                self.bg_col = match seq {
                    Some((s, at)) => self.ansi_colour(s, at),
                    None => 0,
                };
                if self.bg_col == 0 {
                    return false;
                }
            }
            49 => self.bg_col = 0,
            30..=37 => self.fg_col = attrib.wrapping_sub(30).wrapping_add(1),
            40..=47 => self.bg_col = attrib.wrapping_sub(40).wrapping_add(1),
            90..=97 => {
                self.fg_col = if self.more_colors {
                    attrib.wrapping_sub(90).wrapping_add(9)
                } else {
                    attrib.wrapping_sub(90).wrapping_add(1)
                };
            }
            100..=107 => {
                self.bg_col = if self.more_colors {
                    attrib.wrapping_sub(100).wrapping_add(9)
                } else {
                    attrib.wrapping_sub(100).wrapping_add(1)
                };
            }
            _ => return false,
        }
        let pair = self
            .bg_col
            .wrapping_mul(self.nr_of_colors)
            .wrapping_add(self.fg_col)
            .wrapping_add(1);
        let [a, b, ..] = pair.to_le_bytes();
        let _ = curses::attr_set(self.attributes, i16::from_le_bytes([a, b]));
        true
    }

    /// `process_ansi`: an escape sequence the command wrote, read and acted
    /// on if it is SGR (`ESC [ ... m`).
    fn process_ansi(&mut self, p: &mut Pipe) {
        let mut c = p.getc();
        if c == i32::from(b'(') {
            // The two bytes of a character set designation, dropped.
            let _ = p.getc();
            c = p.getc();
        }
        if c != i32::from(b'[') {
            p.ungetc(c);
            return;
        }
        let mut buf: Vec<u8> = Vec::with_capacity(MAX_ANSIBUF);
        for _ in 0..MAX_ANSIBUF {
            let c = p.getc();
            if c == i32::from(b'm') {
                break;
            }
            match u8::try_from(c) {
                Ok(b) if b.is_ascii_digit() || b == b';' => buf.push(b),
                _ => return,
            }
        }
        // "Special case of <ESC>[m".
        if buf.is_empty() {
            self.set_ansi_attribute(0, None);
        }
        // `for (endptr = numstart = buf; *endptr != '\0'; numstart = endptr
        // + 1)`, the string ending where the parameters do.
        // The test is of where the last number ended, so a `;` at the very
        // end reads one more, empty, number: two resets.
        let at = |i: usize| buf.get(i).copied().unwrap_or(0);
        let mut numstart = 0usize;
        let mut end = 0usize;
        while at(end) != 0 {
            let (value, used) = cstrtol::strtol(buf.get(numstart..).unwrap_or_default(), 10);
            end = numstart.saturating_add(used);
            let understood = self.set_ansi_attribute(int(value), Some((&buf, &mut end)));
            if !understood {
                break;
            }
            if numstart == end {
                // "[m treated as [0m".
                self.set_ansi_attribute(0, None);
            }
            numstart = end.saturating_add(1);
        }
    }

    /// `get_terminal_size`: `COLUMNS` and `LINES` read the first time, and
    /// the terminal on standard error asked for what they did not give.
    fn get_terminal_size(&mut self) {
        let var = |name: &str| std::env::var_os(name).map(|v| os_bytes(&v).into_owned());
        if self.incoming_cols == 0 {
            self.incoming_cols = -1;
            if let Some(s) = var("COLUMNS").filter(|s| !s.is_empty()) {
                self.incoming_cols = int(incoming(&s));
                self.width = i64::from(self.incoming_cols);
                putenv_num("COLUMNS", self.width);
            }
        }
        if self.incoming_rows == 0 {
            self.incoming_rows = -1;
            if let Some(s) = var("LINES").filter(|s| !s.is_empty()) {
                self.incoming_rows = int(incoming(&s));
                self.height = i64::from(self.incoming_rows);
                putenv_num("LINES", self.height);
            }
        }
        if let Ok(w) = libcall::pty::window_size(2)
            && (self.incoming_cols < 0 || self.incoming_rows < 0)
        {
            if self.incoming_rows < 0 && w.rows > 0 {
                self.height = i64::from(w.rows);
                putenv_num("LINES", self.height);
            }
            if self.incoming_cols < 0 && w.cols > 0 {
                self.width = i64::from(w.cols);
                putenv_num("COLUMNS", self.width);
            }
        }
    }

    /// `%.1f`, in the locale's decimal point.
    fn fixed1(&self, v: f64) -> Vec<u8> {
        let spec = Spec {
            minus: false,
            plus: false,
            space: false,
            hash: false,
            zero: false,
            width: 0,
            precision: Some(1),
            conv: b'f',
        };
        let text = cfmt::render(&spec, Value::Float(ExtF80::from_f64(v)));
        let mut out = Vec::with_capacity(text.len());
        for &b in &text {
            if b == b'.' {
                out.extend_from_slice(&self.radix);
            } else {
                out.push(b);
            }
        }
        out
    }

    /// `output_header`: the interval, the command and the host and date,
    /// cut to fit as upstream cuts them.
    fn output_header(&self, wcommand: &[WChar], interval: f64) {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        // `ctime`: `asctime (localtime (&t))` and its newline, or a null
        // that `%s` prints as `(null)` for a year it cannot write.
        let ts = localtime::asctime(&self.zone.localtime(t, 0)).map_or_else(
            || b"(null)".to_vec(),
            |mut s| {
                s.push(b'\n');
                s
            },
        );
        // `char hostname[(int) sysconf (_SC_HOST_NAME_MAX) + 1]`: 65 bytes on
        // Linux, 256 on SlateOS. A failing `gethostname` leaves upstream's
        // buffer as it found it, uninitialised; here the name is empty.
        let max = libcall::conf::sysconf(libcall::conf::SC_HOST_NAME_MAX);
        let mut host = vec![0u8; usize::try_from(int(max).wrapping_add(1)).unwrap_or(0)];
        let hostname = libcall::hostname_into(&mut host)
            .ok()
            .and_then(|n| host.get(..n))
            .unwrap_or_default()
            .to_vec();
        let mut header = b"Every ".to_vec();
        header.extend_from_slice(&self.fixed1(interval));
        header.extend_from_slice(b"s: ");
        let mut right = hostname;
        right.extend_from_slice(b": ");
        right.extend_from_slice(&ts);
        let hlen = i64::try_from(header.len()).unwrap_or(i64::MAX);
        let rhlen = i64::try_from(right.len()).unwrap_or(i64::MAX);
        let width = self.width;
        if width < rhlen {
            return;
        }
        // Every place below is an `int` made from a `long`.
        let at = |v: i64| int(v);
        if rhlen.saturating_add(hlen).saturating_add(1) <= width {
            let _ = curses::mvaddstr(0, 0, &header);
            if rhlen.saturating_add(hlen).saturating_add(2) <= width {
                if width < rhlen.saturating_add(hlen).saturating_add(4) {
                    let _ =
                        curses::mvaddstr(0, at(width.wrapping_sub(rhlen).wrapping_sub(4)), b"... ");
                } else {
                    let command_columns = wcswidth(wcommand, -1);
                    if width
                        < rhlen
                            .saturating_add(hlen)
                            .saturating_add(i64::from(command_columns))
                    {
                        // "print truncated"
                        let available = at(width.wrapping_sub(rhlen).wrapping_sub(hlen));
                        let mut in_use = command_columns;
                        let mut wcomm_len = i64::try_from(wcommand.len()).unwrap_or(i64::MAX);
                        while available.wrapping_sub(4) < in_use {
                            wcomm_len = wcomm_len.wrapping_sub(1);
                            in_use = wcswidth(wcommand, wcomm_len);
                        }
                        let _ = curses::mvaddnwstr(0, at(hlen), wcommand, at(wcomm_len));
                        let _ = curses::mvaddstr(
                            0,
                            at(width.wrapping_sub(rhlen).wrapping_sub(4)),
                            b"... ",
                        );
                    } else {
                        let _ = curses::mvaddwstr(0, at(hlen), wcommand);
                    }
                }
            }
        }
        let _ = curses::mvaddstr(0, at(width.wrapping_sub(rhlen).wrapping_add(1)), &right);
    }

    /// `find_eol`: the rest of a line the screen had no room for, read and
    /// dropped.
    fn find_eol(p: &mut Pipe) {
        loop {
            let c = p.my_getwc();
            if c == WEOF || c == u32::from(b'\n') {
                break;
            }
        }
    }

    /// `run_command`: the command run once and its output drawn. Whether
    /// the program should now end (`-g`, `-q`).
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's run_command, statement for statement"
    )]
    fn run_command(&mut self, command: &[u8], command_argv: &[CString]) -> bool {
        let mut oldeolseen = true;
        let mut exit_early = false;
        let mut buffer_size: i32 = 0;
        let mut unchanged_buffer: i32 = 0;
        let (read_end, write_end) = match libcall::fd::pipe() {
            Ok(p) => p,
            Err(e) => xerr(7, b"unable to create IPC pipes", Some(e)),
        };
        // "flush stdout and stderr, since we're about to do fd stuff":
        // nothing is waiting on either, curses writing past stdio.
        // SAFETY: watch has one thread, so the child may do anything a
        // process may.
        let child = match unsafe { libcall::process::fork() } {
            Err(e) => xerr(2, b"unable to fork process", Some(e)),
            Ok(libcall::process::Forked::Child) => {
                self.child(read_end, write_end, command, command_argv)
            }
            Ok(libcall::process::Forked::Parent(pid)) => pid,
        };
        // The child has its own copy of the write end.
        let _ = libcall::fd::close(write_end);
        let mut p = Pipe::new(read_end);
        let color = self.flags & WATCH_COLOR != 0;

        self.reset_ansi();
        let mut y = self.show_title;
        while y < self.height {
            let mut eolseen = false;
            let mut tabpending = false;
            let mut tabwaspending = false;
            if color {
                self.set_ansi_attribute(-1, None);
            }
            let mut carry = WEOF;
            let mut x: i64 = 0;
            while x < self.width {
                let mut c: u32 = u32::from(b' ');
                let mut attr = false;
                if tabwaspending && color {
                    self.set_ansi_attribute(-1, None);
                }
                tabwaspending = false;
                if !eolseen {
                    // "if there is a tab pending, just spit spaces until the
                    // next stop instead of reading characters"
                    if !tabpending {
                        loop {
                            if carry == WEOF {
                                c = p.my_getwc();
                            } else {
                                c = carry;
                                carry = WEOF;
                            }
                            let skip = c != WEOF
                                && !locale::iswprint(c.cast_signed())
                                && c < 128
                                && wcwidth(c) == 0
                                && c != 0x07
                                && c != u32::from(b'\n')
                                && c != u32::from(b'\t')
                                && (c != 0x1b || !color);
                            if !skip {
                                break;
                            }
                        }
                    }
                    if c == 0x1b && color {
                        self.process_ansi(&mut p);
                        // `x--; continue`: the loop's `x++` puts it back.
                        continue;
                    }
                    if c == u32::from(b'\n') {
                        if !oldeolseen && x == 0 {
                            // `x = -1; continue`: this line again, from 0.
                            continue;
                        }
                        eolseen = true;
                    } else if c == u32::from(b'\t') {
                        tabpending = true;
                    } else if c == 0x07 {
                        let _ = curses::beep();
                        x = x.wrapping_add(1);
                        continue;
                    }
                    if x == self.width.wrapping_sub(1) && wcwidth(c) == 2 {
                        // "process this double-width character on the next
                        // line because it won't fit here"
                        y = y.wrapping_add(1);
                        x = 0;
                        carry = c;
                        continue;
                    }
                    if c == WEOF || c == u32::from(b'\n') || c == u32::from(b'\t') {
                        c = u32::from(b' ');
                        if color {
                            let _ = curses::attrset(A_NORMAL);
                        }
                    }
                    if tabpending && x.wrapping_add(1) % 8 == 0 {
                        tabpending = false;
                        tabwaspending = true;
                    }
                }
                let _ = curses::mv(int(y), int(x));
                let wc = c.cast_signed();
                if !self.first_screen && !exit_early && self.flags & WATCH_CHGEXIT != 0 {
                    let old = curses::in_wch().unwrap_or_default();
                    exit_early = wc != old.chars[0];
                }
                if !self.first_screen && !exit_early && self.flags & WATCH_EQUEXIT != 0 {
                    buffer_size = buffer_size.wrapping_add(1);
                    let old = curses::in_wch().unwrap_or_default();
                    if wc == old.chars[0] {
                        unchanged_buffer = unchanged_buffer.wrapping_add(1);
                    }
                }
                if self.flags & WATCH_DIFF != 0 {
                    let old = curses::in_wch().unwrap_or_default();
                    attr = !self.first_screen
                        && (wc != old.chars[0]
                            || (self.flags & WATCH_CUMUL != 0 && old.attr & A_ATTRIBUTES != 0));
                }
                if attr {
                    let _ = curses::standout();
                }
                let _ = curses::addnwstr(&[wc], 1);
                if attr {
                    let _ = curses::standend();
                }
                if wcwidth(c) == 0 {
                    x = x.wrapping_sub(1);
                }
                if wcwidth(c) == 2 {
                    x = x.wrapping_add(1);
                }
                x = x.wrapping_add(1);
            }
            oldeolseen = eolseen;
            if !self.line_wrap {
                self.reset_ansi();
                if color {
                    let _ = curses::attrset(A_NORMAL);
                }
            }
            if !self.line_wrap && !eolseen {
                Self::find_eol(&mut p);
            }
            y = y.wrapping_add(1);
        }

        p.close();
        // "harvest child process and get status, propagated from command"
        let status = match libcall::process::wait(child) {
            Ok(s) => s,
            Err(e) => xerr(8, b"waitpid", Some(e)),
        };
        // "if child process exited in error, beep if option_beep is set"
        if status.exit_code() != Some(0) {
            if self.flags & WATCH_BEEP != 0 {
                let _ = curses::beep();
            }
            if self.flags & WATCH_ERREXIT != 0 {
                let _ = curses::mvaddstr(
                    int(self.height.wrapping_sub(1)),
                    0,
                    b"command exit with a non-zero status, press a key to exit",
                );
                let _ = curses::refresh();
                // `fgetc (stdin)`: one read of what stdio would buffer.
                let mut key = [0u8; 1024];
                let _ = libcall::fd::read(0, &mut key);
                let _ = curses::endwin();
                std::process::exit(8);
            }
        }
        if unchanged_buffer == buffer_size && self.flags & WATCH_EQUEXIT != 0 {
            exit_early = true;
        }
        self.first_screen = false;
        let _ = curses::refresh();
        exit_early
    }

    /// The child of `run_command`: its standard output and error the pipe,
    /// then the command.
    fn child(&self, read_end: i32, write_end: i32, command: &[u8], command_argv: &[CString]) -> ! {
        let _ = libcall::fd::close(read_end);
        let _ = libcall::fd::close(1);
        if let Err(e) = libcall::fd::dup2(write_end, 1) {
            xerr(3, b"dup2 failed", Some(e));
        }
        let _ = libcall::fd::close(write_end);
        // "stderr should default to stdout"
        let _ = libcall::fd::dup2(1, 2);
        if self.flags & WATCH_EXEC != 0 {
            let argv: Vec<&std::ffi::CStr> = command_argv.iter().map(CString::as_c_str).collect();
            let mut slots = vec![core::ptr::null(); argv.len().saturating_add(1)];
            let e = libcall::process::execvp(&argv, &mut slots);
            let mut m = b"unable to execute '".to_vec();
            m.extend_from_slice(command_argv.first().map_or(&b""[..], |a| a.as_bytes()));
            m.push(b'\'');
            xerr(4, &m, Some(e));
        }
        // "watch manpage promises sh quoting"
        let line = CString::new(command.to_vec()).unwrap_or_default();
        let status = libcall::process::system(&line);
        std::process::exit(status.exit_code().unwrap_or(1))
    }
}

/// `strtod_nol_or_err (str, errmesg)`: the number, or the refusal and
/// `exit (EXIT_FAILURE)`.
fn strtod_nol_or_err(text: &[u8], errmesg: &str) -> f64 {
    match strutils::strtod_nol(text) {
        Ok(v) => v,
        Err(fault) => {
            let mut m = errmesg.as_bytes().to_vec();
            m.extend_from_slice(b": ");
            m.extend_from_slice(quoteaf(text).as_bytes());
            m.extend_from_slice(fault.suffix().as_bytes());
            // The suffix is the reason already; `xerr` adds none.
            xerr(1, &m, None)
        }
    }
}

/// `usage (out)`, and the status `exit` is then given.
fn usage(name: &[u8], to_stdout: bool, out: &mut Stream) -> u8 {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(name);
    text.extend_from_slice(USAGE_REST.as_bytes());
    if to_stdout {
        // `Stream` never fails a write; a failure is `close_stdout`'s to say.
        let _ = out.write_all(&text);
        0
    } else {
        stdfd::diag_bytes_ahead_of_stdout(&text);
        1
    }
}

/// `get_time_usec`: microseconds since the epoch, as an `unsigned long
/// long`.
fn get_time_usec() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| {
            USECS_PER_SEC
                .wrapping_mul(d.as_secs())
                .wrapping_add(u64::from(d.subsec_micros()))
        })
}

/// `usleep (us)`: one sleep, which a signal ends.
fn usleep(us: u32) {
    let secs = u64::from(us) / USECS_PER_SEC;
    let nanos = (us % 1_000_000).saturating_mul(1000);
    // A signal ending it early is the point; nothing else can go wrong.
    let _ = libcall::clock::nanosleep(secs, nanos);
}

/// Everything `main` does: the status, for a run that returns.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, statement for statement"
)]
fn run(argv: &[OsString], out: &mut Stream) -> u8 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let name = short_name(&argv0).to_vec();
    let mut locale_name = [0u8; 256];
    // `setlocale (LC_ALL, "")`: a locale that does not exist leaves `C`.
    let _ = locale::select_from_env(locale::LC_ALL, &mut locale_name);
    let mut point = [0u8; 16];
    let radix = locale::radix(&mut point)
        .and_then(|n| point.get(..n))
        .unwrap_or(b".")
        .to_vec();
    let mut w = Watch {
        // `WITH_COLORWATCH`: Debian configures `--enable-colorwatch`, so
        // colour is on until `-C` turns it off.
        flags: WATCH_COLOR,
        height: 24,
        width: 80,
        first_screen: true,
        show_title: 2,
        precise_timekeeping: false,
        line_wrap: true,
        nr_of_colors: 0,
        attributes: A_NORMAL,
        fg_col: 0,
        bg_col: 0,
        more_colors: false,
        incoming_cols: 0,
        incoming_rows: 0,
        radix,
        zone: localtime::Zone::from_env(),
    };
    let mut interval: f64 = 2.0;
    let mut max_cycles: i32 = 1;
    let mut cycle_count: i32 = 0;
    if let Some(s) = std::env::var_os("WATCH_INTERVAL") {
        interval = strtod_nol_or_err(
            &os_bytes(&s),
            "Could not parse interval from WATCH_INTERVAL",
        );
    }
    let words = argv.get(1..).unwrap_or(&[]);
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in WATCH.parse(words, SHORT_OPTIONS, LONG_OPTIONS) {
        match item {
            Ok(Opt::Short(b'b', _) | Opt::Long("beep", _)) => w.flags |= WATCH_BEEP,
            Ok(Opt::Short(b'c', _) | Opt::Long("color", _)) => w.flags |= WATCH_COLOR,
            Ok(Opt::Short(b'C', _) | Opt::Long("no-color", _)) => w.flags &= !WATCH_COLOR,
            Ok(Opt::Short(b'd', v) | Opt::Long("differences", v)) => {
                w.flags |= WATCH_DIFF;
                if v.is_some() {
                    w.flags |= WATCH_CUMUL;
                }
            }
            Ok(Opt::Short(b'e', _) | Opt::Long("errexit", _)) => w.flags |= WATCH_ERREXIT,
            Ok(Opt::Short(b'g', _) | Opt::Long("chgexit", _)) => w.flags |= WATCH_CHGEXIT,
            Ok(Opt::Short(b'q', Some(v)) | Opt::Long("equexit", Some(v))) => {
                w.flags |= WATCH_EQUEXIT;
                max_cycles =
                    cvt::cvt_i32(strtod_nol_or_err(&os_bytes(&v), "failed to parse argument"));
            }
            Ok(Opt::Short(b'r', _) | Opt::Long("no-rerun", _)) => w.flags |= WATCH_NORERUN,
            Ok(Opt::Short(b't', _) | Opt::Long("no-title", _)) => w.show_title = 0,
            Ok(Opt::Short(b'w', _) | Opt::Long("no-wrap", _)) => w.line_wrap = false,
            Ok(Opt::Short(b'x', _) | Opt::Long("exec", _)) => w.flags |= WATCH_EXEC,
            Ok(Opt::Short(b'n', Some(v)) | Opt::Long("interval", Some(v))) => {
                interval = strtod_nol_or_err(&os_bytes(&v), "failed to parse argument");
            }
            Ok(Opt::Short(b'p', _) | Opt::Long("precise", _)) => w.precise_timekeeping = true,
            Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => return usage(&name, true, out),
            Ok(Opt::Short(b'v', _) | Opt::Long("version", _)) => {
                let mut v = name.clone();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                // `Stream` never fails a write; see `usage`.
                let _ = out.write_all(&v);
                return 0;
            }
            Ok(Opt::Operand(o)) => operands.push(os_bytes(o).into_owned()),
            Ok(Opt::Short(..) | Opt::Long(..)) => return usage(&name, false, out),
            Err(e) => {
                let mut m = argv0.clone();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                stdfd::diag_bytes_ahead_of_stdout(&m);
                return usage(&name, false, out);
            }
        }
    }
    // `if (interval < 0.1) ...; if (interval > UINT_MAX) ...`.
    interval = interval.clamp(0.1, UINT_MAX);
    if operands.is_empty() {
        return usage(&name, false, out);
    }
    let command = operands.join(&b' ');
    // Arguments hold no NUL, so each is a C string.
    let command_argv: Vec<CString> = operands
        .iter()
        .map(|a| CString::new(a.clone()).unwrap_or_default())
        .collect();
    let Some(wcommand) = mbstowcs(&command) else {
        stdfd::diag_bytes_ahead_of_stdout(b"unicode handling error\n");
        return 1;
    };

    w.get_terminal_size();
    // "Catch keyboard interrupts so we can put tty back in a sane state."
    // `signal ()`, glibc's: what they interrupt is restarted. None of these
    // numbers can be refused.
    for sig in [
        libcall::signal::SIGINT,
        libcall::signal::SIGTERM,
        libcall::signal::SIGHUP,
    ] {
        let _ = libcall::signal::set_handler(sig, die, true);
    }
    let _ = libcall::signal::set_handler(libcall::signal::SIGWINCH, winch_handler, true);

    // "Set up tty for curses use." (`curses_started`, which `die` reads,
    // is whether curses has a screen to put back.)
    curses::initscr();
    if w.flags & WATCH_COLOR != 0 {
        if curses::has_colors() {
            let _ = curses::start_color();
            let _ = curses::use_default_colors();
            w.init_ansi_colors();
        } else {
            w.flags &= !WATCH_COLOR;
        }
    }
    let _ = curses::nonl();
    let _ = curses::noecho();
    let _ = curses::cbreak();

    let mut last_run: u64 = 0;
    let mut next_loop: u64 = if w.precise_timekeeping {
        get_time_usec()
    } else {
        0
    };
    loop {
        if SCREEN_SIZE_CHANGED.load(Ordering::Relaxed) {
            w.get_terminal_size();
            let _ = curses::resizeterm(int(w.height), int(w.width));
            let _ = curses::clear();
            SCREEN_SIZE_CHANGED.store(false, Ordering::Relaxed);
            w.first_screen = true;
        }
        if w.show_title != 0 {
            w.output_header(&wcommand, interval);
        }
        let usec = interval * cvt::dbl(USECS_PER_SEC);
        if w.flags & WATCH_NORERUN == 0 || cvt::dbl(get_time_usec().wrapping_sub(last_run)) > usec {
            last_run = get_time_usec();
            let exit = w.run_command(&command, &command_argv);
            if w.flags & WATCH_EQUEXIT != 0 {
                if cycle_count == max_cycles && exit {
                    break;
                } else if exit {
                    cycle_count = cycle_count.wrapping_add(1);
                } else {
                    cycle_count = 0;
                }
            } else if exit {
                break;
            }
        } else {
            let _ = curses::refresh();
        }
        if w.precise_timekeeping {
            let cur_time = get_time_usec();
            next_loop = cvt::cvt_u64(cvt::dbl(next_loop) + usec);
            if cur_time < next_loop {
                let [a, b, c, d, ..] = next_loop.wrapping_sub(cur_time).to_le_bytes();
                usleep(u32::from_le_bytes([a, b, c, d]));
            }
        } else if interval < UINT_MAX / cvt::dbl(USECS_PER_SEC) {
            usleep(cvt::cvt_u32(usec));
        } else {
            // `sleep (interval)`: whole seconds.
            let _ = libcall::clock::nanosleep(u64::from(cvt::cvt_u32(interval)), 0);
        }
    }
    let _ = curses::endwin();
    0
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = Stream::stdout();
    let status = run(&argv, &mut out);
    let name = argv
        .first()
        .map_or_else(Vec::new, |a| short_name(&os_bytes(a)).to_vec());
    stdfd::close_stdout_procps(&name, out, ExitCode::from(status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_size_from_the_environment_is_read_as_strtol_reads_it() {
        assert_eq!(incoming(b"80"), 80);
        assert_eq!(incoming(b"0x50"), 80);
        assert_eq!(incoming(b"010"), 8);
        assert_eq!(incoming(b"0"), -1);
        assert_eq!(incoming(b"-5"), -1);
        assert_eq!(incoming(b"80x"), -1);
        assert_eq!(incoming(b" 80"), 80);
    }

    #[test]
    fn widths_follow_wcwidth() {
        assert_eq!(wcswidth(&[0x41, 0x42], -1), 2);
        assert_eq!(wcswidth(&[0x41, 0x42], 1), 1);
        assert_eq!(wcswidth(&[0x41, 0x7], -1), -1);
        assert_eq!(wcswidth(&[], -1), 0);
    }

    #[test]
    fn a_c_string_of_the_c_locale() {
        assert_eq!(mbstowcs(b"ls -l"), Some(vec![0x6c, 0x73, 0x20, 0x2d, 0x6c]));
        // The C locale is ASCII.
        assert_eq!(mbstowcs(b"caf\xc3\xa9"), None);
    }

    /// What `my_getwc` gives for a pipe holding `bytes`, called two more
    /// times than there are bytes: every call takes a byte or meets the end,
    /// so the last two are always the end.
    #[cfg(unix)]
    #[allow(clippy::unwrap_used)]
    fn read_through_a_pipe(bytes: &[u8]) -> Vec<u32> {
        let (r, w) = libcall::fd::pipe().unwrap();
        assert_eq!(libcall::fd::write(w, bytes), Ok(bytes.len()));
        libcall::fd::close(w).unwrap();
        let mut p = Pipe::new(r);
        let got = (0..bytes.len().saturating_add(2))
            .map(|_| p.my_getwc())
            .collect();
        p.close();
        got
    }

    /// Only ASCII and NUL, whose reading no locale changes: the test runs
    /// in whichever one the process has.
    #[cfg(unix)]
    #[test]
    fn the_reader_takes_what_my_getwc_takes() {
        let ch = |s: &str| s.bytes().map(u32::from).collect::<Vec<_>>();
        // Plain bytes, then the end, and the end again.
        let mut want = ch("ab");
        want.extend([WEOF, WEOF]);
        assert_eq!(read_through_a_pipe(b"ab"), want);
        // 0xFF in a `char` is `EOF`: it ends that character and no more.
        assert_eq!(
            read_through_a_pipe(b"a\xffb"),
            vec![0x61, WEOF, 0x62, WEOF, WEOF]
        );
        // `mbtowc` answers 0 for a NUL, which is not a character read, so
        // the bytes after it are taken in looking for one -- here to the
        // end, all of them lost.
        assert_eq!(read_through_a_pipe(b"\0abc"), vec![WEOF; 6]);
        // Sixteen bytes without a character: all but the first given back,
        // and read again one at a time.
        let mut want = vec![WEOF];
        want.extend(ch("abcdefghijklmnopqrst"));
        want.extend([WEOF, WEOF]);
        assert_eq!(read_through_a_pipe(b"\0abcdefghijklmnopqrst"), want);
    }
}
