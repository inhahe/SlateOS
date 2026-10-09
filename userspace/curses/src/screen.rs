//! The screen: `SCREEN`, and what is done to it as a whole -- setting it up
//! (`lib_newterm.c`'s `newterm`, `lib_set_term.c`'s `_nc_setupscreen`,
//! `lib_setup.c`'s screen size), the terminal's modes (`lib_ttyflags.c`,
//! `lib_raw.c`, `lib_nl.c`, `lib_echo.c`, `lib_options.c`'s `_nc_keypad`),
//! the line-drawing characters (`lib_acs.c`, `lib_wacs.c`), refreshing
//! (`lib_refresh.c`, and `tty_update.c`'s `doupdate` around the update
//! itself), `endwin` (`lib_endwin.c`), resizing (`resizeterm.c`) and the
//! colour calls that reach past the terminal (`lib_color.c`'s
//! `_nc_change_pair`, `lib_dft_fgbg.c`).
//!
//! A [`Screen`] is a value: [`Screen::newterm`] makes one and its caller
//! keeps it. The one screen the C interface implies -- `initscr`'s, which
//! the signal handlers reach -- is the crate root's.
//!
//! What this crate leaves out of `SCREEN` it leaves out because it reads no
//! input: there is no `getch`, so no key table, no input queue, no
//! `ESCDELAY`, no mouse; and so `KEY_RESIZE`, which upstream queues as input
//! when the window changes size, is not queued anywhere. Nor are there soft
//! labels or lines ripped off the screen, so `stdscr` is always the whole
//! screen.

use libcall::termios::{self as tc, Termios};
use terminfo::{Entry, Padding};

use crate::addch::{AddCtx, Ctype};
use crate::caps::{number, string};
use crate::cell::{A_ALTCHARSET, A_BOLD, A_COLOR, Attr, Cell, XMC_CONFLICT};
use crate::hashmap::HashState;
use crate::term::{ACS_LEN, COLOR_DEFAULT, Sink, Term};
use crate::update;
use crate::window::{HASMOVED, NOCHANGE, Size, Window, short};

/// `_endwin`: where a screen stands with respect to `endwin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndWin {
    /// `ewInitial`: set up, and neither ended nor refreshed since.
    Initial,
    /// `ewRunning`: refreshed after an `endwin`.
    Running,
    /// `ewSuspend`: `endwin` was called; the next refresh comes back.
    Suspend,
}

/// Why a screen could not be set up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoTerminal {
    /// `setupterm` refused the terminal; what it would have said.
    Setup(Vec<u8>),
    /// The screen is a size no window can be.
    Size,
}

/// How a screen is set up: `newterm`'s arguments, and `use_env`, which a
/// program calls before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Where output goes: `newterm`'s output stream, standard output for
    /// `initscr`.
    pub output: Sink,
    /// `newterm`'s input stream's descriptor: whose typed-ahead input a
    /// return from a suspension throws away.
    pub input: i32,
    /// `use_env`: whether the size of the window is asked for, and `LINES`
    /// and `COLUMNS` read, rather than the terminal description's `lines`
    /// and `cols` believed.
    pub use_env: bool,
}

/// The terminal's modes: `TERMINAL`'s `Filedes`, `Ottyb` and `Nttyb`.
#[derive(Clone, Copy, Debug)]
pub struct Tty {
    /// `Filedes`: the descriptor whose modes are read and set -- the output
    /// descriptor, or standard error when standard output is that and is
    /// not a terminal; -1 for a screen kept in memory.
    pub fd: i32,
    /// `Ottyb`: the shell's modes; all zeros when they could not be read.
    pub ottyb: Termios,
    /// `Nttyb`: the program's.
    pub nttyb: Termios,
    /// `_notty`: setting the modes failed with `ENOTTY`.
    pub notty: bool,
}

impl Tty {
    /// `_nc_get_tty_mode (buf)`: the terminal's modes, read again after an
    /// interruption; `None` when they cannot be read at all, which upstream
    /// reports by zeroing the buffer.
    fn get_mode(&self) -> Option<Termios> {
        if self.fd < 0 {
            return None;
        }
        loop {
            match tc::get_attr(self.fd) {
                Ok(t) => return Some(t),
                Err(libcall::EINTR) => {}
                Err(_) => return None,
            }
        }
    }

    /// `_nc_set_tty_mode (buf)`: `tcsetattr (Filedes, TCSADRAIN, buf)`,
    /// again after an interruption.
    fn set_mode(&mut self, buf: &Termios) -> bool {
        if self.fd < 0 {
            return false;
        }
        loop {
            match tc::set_attr(self.fd, tc::TCSADRAIN, buf) {
                Ok(()) => return true,
                Err(libcall::EINTR) => {}
                Err(e) => {
                    if e == libcall::pty::ENOTTY {
                        self.notty = true;
                    }
                    return false;
                }
            }
        }
    }

    /// `def_shell_mode ()`: the shell's modes kept -- and the terminal's
    /// `ht` and `cbt` taken away when it expands tabs itself, since curses
    /// would then be moving with characters the terminal changes.
    fn def_shell_mode(&mut self, entry: &mut Entry) -> bool {
        if let Some(t) = self.get_mode() {
            self.ottyb = t;
            if t.c_oflag & tc::TABDLY != 0 {
                entry.set_string(string::TAB, None);
                entry.set_string(string::BACK_TAB, None);
            }
            true
        } else {
            self.ottyb = Termios::default();
            false
        }
    }

    /// `def_prog_mode ()`: the program's modes kept, without the terminal's
    /// tab expansion.
    fn def_prog_mode(&mut self) -> bool {
        if let Some(mut t) = self.get_mode() {
            t.c_oflag &= !tc::TABDLY;
            self.nttyb = t;
            true
        } else {
            self.nttyb = Termios::default();
            false
        }
    }
}

/// What `_nc_signal_handler` keeps of `SIGTSTP` between its calls: whether
/// it left the signal alone for good, and what it put in place and took
/// out.
#[derive(Clone, Copy, Default)]
pub struct Tstp {
    /// `ignore_tstp`: the program had its own action for `SIGTSTP` when the
    /// screen was set up, so curses never touches it.
    pub ignore: bool,
    /// `new_sigaction.sa_handler`: what curses last installed.
    pub installed: Installed,
    /// `old_sigaction`: what "ignore" replaced, to be put back.
    pub replaced: Option<libcall::signal::SavedAction>,
}

/// What curses last installed for `SIGTSTP`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Installed {
    /// Nothing yet: `SIG_DFL`.
    #[default]
    Nothing,
    /// The suspending handler.
    Handler,
    /// `SIG_IGN`, while the screen is being updated.
    Ignore,
}

/// `SCREEN`.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's SCREEN, field for field"
)]
pub struct Screen {
    /// The terminal side: description, output, costs, attributes, colours.
    pub t: Term,
    /// The terminal's modes.
    pub tty: Tty,
    /// The locale.
    pub ctype: Box<dyn Ctype + Send>,
    /// `stdscr`: the window programs draw in.
    pub stdscr: Window,
    /// `curscr`: what the terminal shows.
    pub curscr: Window,
    /// `newscr`: what it is to show after the next update.
    pub newscr: Window,
    /// The scroll optimiser's tables (`oldhash`, `newhash`, ...).
    pub hash: HashState,
    /// `_endwin`.
    pub endwin: EndWin,
    /// `_nl`, `_raw`, `_cbreak`, `_echo`: the modes curses keeps for itself.
    pub nl: bool,
    pub raw: bool,
    pub cbreak: i32,
    pub echo: bool,
    /// `_keypad_on`: `smkx` was sent.
    pub keypad_on: bool,
    /// `_use_meta`: the terminal passes eight-bit input.
    pub use_meta: bool,
    /// `_lines_avail`: the lines `stdscr` has.
    pub lines_avail: i32,
    /// `_topstolen`: lines taken off the top (always 0 here).
    pub topstolen: Size,
    /// `_ifd`, `_checkfd`: the input descriptor, and the one `typeahead`
    /// watches.
    pub ifd: i32,
    pub checkfd: i32,
    /// `TABSIZE`.
    pub tabsize: i32,
    /// `_sig_winch`: a `SIGWINCH` caught and not yet acted on.
    pub sig_winch: bool,
    /// `_use_env`.
    pub use_env: bool,
    /// `LINES`, `COLS`.
    pub lines: i32,
    pub cols: i32,
    /// `resizeterm.c`'s `current_lines` and `current_cols`: the size being
    /// resized from.
    resize_from: (i32, i32),
    /// What `_nc_signal_handler` keeps of `SIGTSTP`.
    pub tstp: Tstp,
}

/// `_nc_getenv_num (name)`: the variable as `strtol (s, &end, 0)` reads all
/// of it, or -1 for none, a negative, trailing bytes or more than an `int`.
fn getenv_num(name: &str) -> i32 {
    let Some(v) = std::env::var_os(name) else {
        return -1;
    };
    let bytes = os_bytes(&v);
    let (n, used) = cstrtol::strtol(&bytes, 0);
    if n < 0 || used == 0 || used != bytes.len() {
        return -1;
    }
    i32::try_from(n).unwrap_or(-1)
}

/// The bytes of an environment value.
#[cfg(unix)]
pub(crate) fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

/// The bytes of an environment value, as near as a host without them comes.
#[cfg(not(unix))]
pub(crate) fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

/// `_nc_get_screensize`: the screen's size -- the description's `lines` and
/// `cols`, then the window's size, then `LINES` and `COLUMNS`, then the
/// description again and at last 24 by 80 for anything still unknown --
/// written back into `lines` and `cols` "so tigetnum () and tgetnum () will
/// do the right thing"; and `TABSIZE`, from `it`.
///
/// With `use_env` off nothing is asked: the description is believed.
/// (Upstream then asks the terminal where its cursor lands at 9999,9999 --
/// but only when built with `USE_CHECK_SIZE`, which Debian's is not.)
fn get_screensize(entry: &mut Entry, fd: i32, use_env: bool) -> (i32, i32, i32) {
    let mut linep = entry.number(number::LINES);
    let mut colp = entry.number(number::COLUMNS);
    if use_env {
        // "try asking the OS"
        if fd >= 0 && libcall::fd::is_terminal(fd) {
            loop {
                match libcall::pty::window_size(fd) {
                    Ok(size) => {
                        linep = i32::from(size.rows);
                        colp = i32::from(size.cols);
                        break;
                    }
                    Err(libcall::EINTR) => {}
                    Err(_) => break,
                }
            }
        }
        // "Finally, look for environment variables."
        let value = getenv_num("LINES");
        if value > 0 {
            linep = value;
        }
        let value = getenv_num("COLUMNS");
        if value > 0 {
            colp = value;
        }
        // `_nc_default_screensize`.
        if linep <= 0 {
            linep = entry.number(number::LINES);
        }
        if colp <= 0 {
            colp = entry.number(number::COLUMNS);
        }
        if linep <= 0 {
            linep = 24;
        }
        if colp <= 0 {
            colp = 80;
        }
        entry.set_number(number::LINES, linep);
        entry.set_number(number::COLUMNS, colp);
    }
    let init_tabs = entry.number(number::INIT_TABS);
    let tabsize = if init_tabs >= 0 { init_tabs } else { 8 };
    (linep, colp, tabsize)
}

/// `SGR0_TEST (mode)`: the terminal has the string, and it is not `sgr0`.
fn sgr0_test(entry: &Entry, mode: usize) -> bool {
    entry
        .string(mode)
        .is_some_and(|m| entry.string(string::EXIT_ATTRIBUTE_MODE) != Some(m))
}

/// `_nc_init_acs`'s fallbacks: what each `acsc` letter is drawn as on a
/// terminal that does not map it.
const ACS_FALLBACK: [(u8, u8); 54] = [
    (b'l', b'+'),
    (b'm', b'+'),
    (b'k', b'+'),
    (b'j', b'+'),
    (b'u', b'+'),
    (b't', b'+'),
    (b'v', b'+'),
    (b'w', b'+'),
    (b'q', b'-'),
    (b'x', b'|'),
    (b'n', b'+'),
    (b'o', b'~'),
    (b's', b'_'),
    (b'`', b'+'),
    (b'a', b':'),
    (b'f', b'\''),
    (b'g', b'#'),
    (b'~', b'o'),
    (b',', b'<'),
    (b'+', b'>'),
    (b'.', b'v'),
    (b'-', b'^'),
    (b'h', b'#'),
    (b'i', b'#'),
    (b'0', b'#'),
    (b'p', b'-'),
    (b'r', b'-'),
    (b'y', b'<'),
    (b'z', b'>'),
    (b'{', b'*'),
    (b'|', b'!'),
    (b'}', b'f'),
    (b'L', b'+'),
    (b'M', b'+'),
    (b'K', b'+'),
    (b'J', b'+'),
    (b'T', b'+'),
    (b'U', b'+'),
    (b'V', b'+'),
    (b'W', b'+'),
    (b'Q', b'-'),
    (b'X', b'|'),
    (b'N', b'+'),
    (b'C', b'+'),
    (b'D', b'+'),
    (b'B', b'+'),
    (b'A', b'+'),
    (b'G', b'+'),
    (b'F', b'+'),
    (b'H', b'+'),
    (b'I', b'+'),
    (b'R', b'-'),
    (b'Y', b'|'),
    (b'E', b'+'),
];

/// `_nc_init_wacs`'s table: each letter, its ASCII stand-in, and the
/// Unicode character for it.
const WACS_TABLE: [(u8, u8, u32); 54] = [
    (b'l', b'+', 0x250c),
    (b'm', b'+', 0x2514),
    (b'k', b'+', 0x2510),
    (b'j', b'+', 0x2518),
    (b't', b'+', 0x251c),
    (b'u', b'+', 0x2524),
    (b'v', b'+', 0x2534),
    (b'w', b'+', 0x252c),
    (b'q', b'-', 0x2500),
    (b'x', b'|', 0x2502),
    (b'n', b'+', 0x253c),
    (b'o', b'~', 0x23ba),
    (b's', b'_', 0x23bd),
    (b'`', b'+', 0x25c6),
    (b'a', b':', 0x2592),
    (b'f', b'\'', 0x00b0),
    (b'g', b'#', 0x00b1),
    (b'~', b'o', 0x00b7),
    (b',', b'<', 0x2190),
    (b'+', b'>', 0x2192),
    (b'.', b'v', 0x2193),
    (b'-', b'^', 0x2191),
    (b'h', b'#', 0x2592),
    (b'i', b'#', 0x2603),
    (b'0', b'#', 0x25ae),
    (b'p', b'-', 0x23bb),
    (b'r', b'-', 0x23bc),
    (b'y', b'<', 0x2264),
    (b'z', b'>', 0x2265),
    (b'{', b'*', 0x03c0),
    (b'|', b'!', 0x2260),
    (b'}', b'f', 0x00a3),
    (b'L', b'+', 0x250f),
    (b'M', b'+', 0x2517),
    (b'K', b'+', 0x2513),
    (b'J', b'+', 0x251b),
    (b'T', b'+', 0x2523),
    (b'U', b'+', 0x252b),
    (b'V', b'+', 0x253b),
    (b'W', b'+', 0x2533),
    (b'Q', b'-', 0x2501),
    (b'X', b'|', 0x2503),
    (b'N', b'+', 0x254b),
    (b'C', b'+', 0x2554),
    (b'D', b'+', 0x255a),
    (b'B', b'+', 0x2557),
    (b'A', b'+', 0x255d),
    (b'G', b'+', 0x2563),
    (b'F', b'+', 0x2560),
    (b'H', b'+', 0x2569),
    (b'I', b'+', 0x2566),
    (b'R', b'-', 0x2550),
    (b'Y', b'|', 0x2551),
    (b'E', b'+', 0x256c),
];

/// `_nc_init_acs ()`: the screen's line-drawing map -- the ASCII fallbacks,
/// the "PC ROM" characters where the Linux console's coincidence allows,
/// then the terminal's own `acsc` -- and `enacs` sent.
fn init_acs(t: &mut Term) {
    for j in 1..ACS_LEN {
        if let Some(m) = t.acs_map.get_mut(j) {
            *m = 0;
        }
        if let Some(s) = t.screen_acs_map.get_mut(j) {
            *s = false;
        }
    }
    for &(k, v) in &ACS_FALLBACK {
        if let Some(m) = t.acs_map.get_mut(usize::from(k)) {
            *m = Attr::from(v);
        }
    }
    t.putp_cap(string::ENA_ACS);
    // "Linux console "supports" the "PC ROM" character set by the
    // coincidence that smpch/rmpch and smacs/rmacs have the same values."
    let same = |a: usize, b: usize| match (t.entry.string(a), t.entry.string(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    };
    if same(
        string::ENTER_PC_CHARSET_MODE,
        string::ENTER_ALT_CHARSET_MODE,
    ) && same(string::EXIT_PC_CHARSET_MODE, string::EXIT_ALT_CHARSET_MODE)
    {
        for i in 1..ACS_LEN {
            if t.acs_map.get(i) == Some(&0) {
                if let Some(m) = t.acs_map.get_mut(i) {
                    *m = Attr::try_from(i).unwrap_or(0);
                }
                if let Some(s) = t.screen_acs_map.get_mut(i) {
                    *s = true;
                }
            }
        }
    }
    if let Some(acsc) = t.entry.string(string::ACS_CHARS) {
        // `strlen (acs_chars)`.
        let length = acsc.iter().position(|&b| b == 0).unwrap_or(acsc.len());
        let mut i = 0usize;
        while i.saturating_add(1) < length {
            let k = acsc.get(i).copied().unwrap_or(0);
            if k != 0 && usize::from(k) < ACS_LEN {
                let v = acsc.get(i.saturating_add(1)).copied().unwrap_or(0);
                if let Some(m) = t.acs_map.get_mut(usize::from(k)) {
                    *m = Attr::from(v) | A_ALTCHARSET;
                }
                if let Some(s) = t.screen_acs_map.get_mut(usize::from(k)) {
                    *s = true;
                }
            }
            i = i.saturating_add(2);
        }
    }
}

/// `_nc_init_wacs ()`: the wide line-drawing characters -- in a UTF-8
/// locale the Unicode ones, where they take one column; else the letter
/// itself in the alternate set. (Upstream's third choice, the ASCII
/// stand-in, is for a map nobody filled in, and `_nc_init_acs` has always
/// filled it in by the time a screen asks.)
fn init_wacs(t: &mut Term, ctype: &dyn Ctype) {
    let active = t.screen_unicode;
    for &(m, ascii, uni) in &WACS_TABLE {
        let value = if active {
            uni.cast_signed()
        } else {
            i32::from(ascii)
        };
        let wide = ctype.wcwidth(value);
        let Some(slot) = t.wacs.get_mut(usize::from(m)) else {
            continue;
        };
        if active && wide == 1 {
            slot.set_char(uni.cast_signed(), 0);
        } else {
            slot.set_char(i32::from(m), A_ALTCHARSET);
        }
    }
}

/// `_nc_locale_breaks_acs (term)`: whether the terminal is known to lose
/// its line drawing in a UTF-8 locale -- as `NCURSES_NO_UTF8_ACS` says, or
/// else the terminal's `U8`, or else for the Linux console, and for a
/// `screen` whose `TERMCAP` shows it shifting into the line-drawing set.
fn locale_breaks_acs(entry: &Entry) -> bool {
    if std::env::var_os("NCURSES_NO_UTF8_ACS").is_some() {
        // -1, true, for a value that is no number.
        return getenv_num("NCURSES_NO_UTF8_ACS") != 0;
    }
    let u8_value = entry.tigetnum(b"U8");
    if u8_value >= 0 {
        return u8_value != 0;
    }
    let Some(term) = std::env::var_os("TERM").map(|v| os_bytes(&v)) else {
        return false;
    };
    let contains = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
    if contains(&term, b"linux") {
        return true;
    }
    if contains(&term, b"screen")
        && let Some(termcap) = std::env::var_os("TERMCAP").map(|v| os_bytes(&v))
        && contains(&termcap, b"screen")
        && contains(&termcap, b"hhII00")
    {
        let shifts = |cap: usize| {
            entry
                .string(cap)
                .is_some_and(|s| s.contains(&0x0e) || s.contains(&0x0f))
        };
        return shifts(string::ENTER_ALT_CHARSET_MODE) || shifts(string::SET_ATTRIBUTES);
    }
    false
}

/// `sscanf (env, "%d%c%d%c", &fg, &sep1, &bg, &sep2)`, as
/// `NCURSES_ASSUMED_COLORS` is read: how many it converted, and the two
/// numbers.
fn scan_assumed(s: &[u8]) -> (i32, i32, i32) {
    let mut at = 0usize;
    let Some(fg) = scan_int(s, &mut at) else {
        return (0, 0, 0);
    };
    // `%c` takes any byte, the end excepted.
    if at >= s.len() {
        return (1, fg, 0);
    }
    at = at.saturating_add(1);
    let Some(bg) = scan_int(s, &mut at) else {
        return (2, fg, 0);
    };
    let count = if at < s.len() { 4 } else { 3 };
    (count, fg, bg)
}

/// `%d`: white space skipped, a sign, digits -- read as `strtol` reads them,
/// clamped to a `long`, and kept to an `int` as glibc's `scanf` stores one.
/// `None` without a digit.
fn scan_int(s: &[u8], at: &mut usize) -> Option<i32> {
    while s.get(*at).is_some_and(u8::is_ascii_whitespace) {
        *at = at.saturating_add(1);
    }
    let negative = s.get(*at) == Some(&b'-');
    if matches!(s.get(*at), Some(b'-' | b'+')) {
        *at = at.saturating_add(1);
    }
    let mut value: i64 = 0;
    let mut digits = 0usize;
    while let Some(d) = s.get(*at).filter(|b| b.is_ascii_digit()) {
        let d = i64::from(d.wrapping_sub(b'0'));
        value = value
            .checked_mul(10)
            .and_then(|v| {
                if negative {
                    v.checked_sub(d)
                } else {
                    v.checked_add(d)
                }
            })
            .unwrap_or(if negative { i64::MIN } else { i64::MAX });
        digits = digits.saturating_add(1);
        *at = at.saturating_add(1);
    }
    if digits == 0 {
        return None;
    }
    let [a, b, c, d, ..] = value.to_le_bytes();
    Some(i32::from_le_bytes([a, b, c, d]))
}

impl Screen {
    /// `newterm (name, out, in)`: the terminal called `name` set up, then a
    /// screen on it ([`Screen::setup`]).
    ///
    /// # Errors
    ///
    /// [`NoTerminal`] when `setupterm` would have failed -- an unknown
    /// terminal, a generic or a hard-copy one -- or the screen is too large.
    pub fn newterm(
        name: &[u8],
        opts: &Options,
        ctype: Box<dyn Ctype + Send>,
    ) -> Result<Self, NoTerminal> {
        let env = terminfo::Env::from_process();
        let fd = match opts.output {
            Sink::Fd(fd) => fd,
            Sink::Memory(_) => -1,
        };
        let setup = terminfo::setupterm_with(
            Some(name),
            None,
            &env,
            &terminfo::Options {
                fd,
                use_env: opts.use_env,
                use_tioctl: false,
            },
        );
        if let Some(complaint) = setup.complaint {
            return Err(NoTerminal::Setup(complaint));
        }
        let padding = setup.padding();
        let entry = setup.entry.ok_or(NoTerminal::Setup(Vec::new()))?;
        Self::setup(entry, padding, opts, ctype)
    }

    /// The rest of `newterm`, given the terminal `setupterm` found: the
    /// screen made (`_nc_setupscreen`), the cursor's costs, the terminal put
    /// in a known state (`_nc_screen_init`), and its modes set for curses
    /// (`_nc_initscr`). Signal handlers are the caller's to install.
    ///
    /// # Errors
    ///
    /// [`NoTerminal::Size`] for a screen too large for a window.
    #[allow(
        clippy::too_many_lines,
        reason = "`_nc_setupscreen` and `newterm`, step for step, in their order"
    )]
    pub fn setup(
        entry: Entry,
        padding: Padding,
        opts: &Options,
        ctype: Box<dyn Ctype + Send>,
    ) -> Result<Self, NoTerminal> {
        // "Allow output redirection. ... If stdout is directed to a file,
        // screen updates go to standard error."
        let (tty_fd, out_is_tty) = match opts.output {
            Sink::Fd(1) if !libcall::fd::is_terminal(1) => (2, false),
            Sink::Fd(fd) => (fd, libcall::fd::is_terminal(fd)),
            Sink::Memory(_) => (-1, false),
        };
        let mut t = Term::new(entry, padding, opts.output.clone());
        let mut tty = Tty {
            fd: tty_fd,
            ottyb: Termios::default(),
            nttyb: Termios::default(),
            notty: false,
        };
        // `_nc_setupterm`: on a terminal, its modes kept now.
        if tty_fd >= 0 && libcall::fd::is_terminal(tty_fd) {
            tty.def_shell_mode(&mut t.entry);
            tty.def_prog_mode();
        }

        // ---- `_nc_setupscreen`
        let (slines, scolumns, tabsize) = get_screensize(&mut t.entry, tty.fd, opts.use_env);
        t.lines = slines;
        t.columns = scolumns;
        let limit = i64::from(slines)
            .saturating_add(2)
            .saturating_mul(i64::from(scolumns).saturating_add(6));
        t.set_out_limit(usize::try_from(limit).unwrap_or(0));
        t.no_padding = std::env::var_os("NCURSES_NO_PADDING").is_some();
        // "Allow those assumed/default color assumptions to be overridden at
        // runtime".
        if let Some(v) = std::env::var_os("NCURSES_ASSUMED_COLORS") {
            let max_colors = t.num(number::MAX_COLORS);
            let (count, fg, bg) = scan_assumed(&os_bytes(&v));
            if count >= 1 {
                t.default_fg = if fg >= 0 && fg < max_colors {
                    fg
                } else {
                    COLOR_DEFAULT
                };
                if count >= 3 {
                    t.default_bg = if bg >= 0 && bg < max_colors {
                        bg
                    } else {
                        COLOR_DEFAULT
                    };
                }
            }
        }
        // "If we've no magic cookie support, we suppress attributes that xmc
        // would affect, i.e., the attributes that affect the rendition of a
        // space." This build has none.
        t.ok_attributes = t.termattrs();
        if t.has_colors() {
            t.ok_attributes |= A_COLOR;
        }
        let glitch = t.num(number::MAGIC_COOKIE_GLITCH);
        if glitch > 0 {
            t.xmc_triggers = t.ok_attributes & XMC_CONFLICT;
            t.xmc_suppress = t.xmc_triggers & !A_BOLD;
            for cap in [
                string::ACS_CHARS,
                string::ENA_ACS,
                string::ENTER_ALT_CHARSET_MODE,
                string::EXIT_ALT_CHARSET_MODE,
            ] {
                t.entry.set_string(cap, None);
            }
        }
        if glitch >= 0 {
            t.entry.set_number(number::MAGIC_COOKIE_GLITCH, -1);
            for cap in [
                string::SET_ATTRIBUTES,
                string::ENTER_BLINK_MODE,
                string::ENTER_BOLD_MODE,
                string::ENTER_DIM_MODE,
                string::ENTER_REVERSE_MODE,
                string::ENTER_STANDOUT_MODE,
                string::ENTER_UNDERLINE_MODE,
            ] {
                t.entry.set_string(cap, None);
            }
        }
        init_acs(&mut t);
        t.screen_unicode = ctype.unicode_locale();
        init_wacs(&mut t, &*ctype);
        t.screen_acs_fix = t.screen_unicode && locale_breaks_acs(&t.entry);
        t.legacy_coding = i32::from(ctype.legacy_locale());
        t.idcok = true;
        t.idlok = false;

        let screen = (slines, scolumns);
        let mut newscr =
            Window::newwin(slines, scolumns, 0, 0, screen, 0).ok_or(NoTerminal::Size)?;
        let mut curscr =
            Window::newwin(slines, scolumns, 0, 0, screen, 0).ok_or(NoTerminal::Size)?;
        newscr.clear = true;
        curscr.clear = false;
        // "Get the current tty-modes. setupterm() may already have done
        // this".
        if tty.ottyb == Termios::default() {
            tty.def_shell_mode(&mut t.entry);
            tty.def_prog_mode();
        }
        let lines_avail = slines;
        let stdscr =
            Window::newwin(lines_avail, scolumns, 0, 0, screen, 0).ok_or(NoTerminal::Size)?;

        // ---- `newterm`, after `_nc_setupscreen`
        let use_meta =
            (tty.ottyb.c_cflag & tc::CSIZE) == tc::CS8 && tty.ottyb.c_iflag & tc::ISTRIP == 0;
        let has = |c: usize| t.entry.string(c).is_some();
        t.scrolling = (has(string::SCROLL_FORWARD) && has(string::SCROLL_REVERSE))
            || ((has(string::PARM_RINDEX)
                || has(string::PARM_INSERT_LINE)
                || has(string::INSERT_LINE))
                && (has(string::PARM_INDEX)
                    || has(string::PARM_DELETE_LINE)
                    || has(string::DELETE_LINE)));
        // `baudrate ()`, which "sets a field in the screen structure": the
        // output speed of the program's modes.
        t.padding.baud = terminfo::baudrate(tc::output_speed(&tty.nttyb));
        t.use_rmso = sgr0_test(&t.entry, string::EXIT_STANDOUT_MODE);
        t.use_rmul = sgr0_test(&t.entry, string::EXIT_UNDERLINE_MODE);
        t.use_ritm = sgr0_test(&t.entry, string::EXIT_ITALICS_MODE);
        // "compute movement costs so we can do better move optimization"
        t.mvcur_init(out_is_tty);
        // "initialize terminal to a sane state"
        update::screen_resume(&mut t, &mut newscr);

        let mut sp = Self {
            t,
            tty,
            ctype,
            stdscr,
            curscr,
            newscr,
            hash: HashState::default(),
            endwin: EndWin::Initial,
            nl: true,
            raw: false,
            cbreak: 0,
            echo: true,
            keypad_on: false,
            use_meta,
            lines_avail,
            topstolen: 0,
            ifd: opts.input,
            checkfd: opts.input,
            tabsize,
            sig_winch: false,
            use_env: opts.use_env,
            lines: lines_avail,
            cols: scolumns,
            resize_from: (slines, scolumns),
            tstp: Tstp::default(),
        };
        // "Initialize the terminal line settings."
        sp.initscr_modes();
        Ok(sp)
    }

    /// `stdscr`, and what adding characters to it needs to know.
    pub fn stdscr_ctx(&mut self) -> (&mut Window, AddCtx<'_>) {
        let ctx = AddCtx {
            ctype: &*self.ctype,
            legacy_coding: self.t.legacy_coding,
            tabsize: self.tabsize,
        };
        (&mut self.stdscr, ctx)
    }

    // ---- the terminal's modes

    /// `def_shell_mode ()`.
    pub fn def_shell_mode(&mut self) -> bool {
        self.tty.def_shell_mode(&mut self.t.entry)
    }

    /// `def_prog_mode ()`.
    pub fn def_prog_mode(&mut self) -> bool {
        self.tty.def_prog_mode()
    }

    /// `reset_prog_mode ()`: the program's modes back, and keypad mode with
    /// them if it was on.
    pub fn reset_prog_mode(&mut self) -> bool {
        let buf = self.tty.nttyb;
        if self.tty.set_mode(&buf) {
            if self.keypad_on {
                self.keypad(true);
            }
            true
        } else {
            false
        }
    }

    /// `reset_shell_mode ()`: keypad mode off -- `rmkx` goes out whether or
    /// not it was ever on -- the output flushed, the shell's modes back.
    pub fn reset_shell_mode(&mut self) -> bool {
        self.keypad(false);
        self.t.flush();
        let buf = self.tty.ottyb;
        self.tty.set_mode(&buf)
    }

    /// `_nc_keypad (sp, flag)`: `smkx`, or else `rmkx`, each flushed when
    /// the terminal has it.
    pub fn keypad(&mut self, flag: bool) {
        let cap = if flag {
            string::KEYPAD_XMIT
        } else {
            string::KEYPAD_LOCAL
        };
        if self.t.putp_cap(cap) {
            self.t.flush();
        }
        self.keypad_on = flag;
    }

    /// `cbreak ()`: input a character at a time, interrupt characters still
    /// signals, carriage return not made newline.
    pub fn cbreak(&mut self) -> bool {
        let mut buf = self.tty.nttyb;
        buf.c_lflag &= !tc::ICANON;
        buf.c_iflag &= !tc::ICRNL;
        buf.c_lflag |= tc::ISIG;
        if let Some(c) = buf.c_cc.get_mut(tc::VMIN) {
            *c = 1;
        }
        if let Some(c) = buf.c_cc.get_mut(tc::VTIME) {
            *c = 0;
        }
        let ok = self.tty.set_mode(&buf);
        if ok {
            self.cbreak = 1;
            self.tty.nttyb = buf;
        }
        ok
    }

    /// `nocbreak ()`.
    pub fn nocbreak(&mut self) -> bool {
        let mut buf = self.tty.nttyb;
        buf.c_lflag |= tc::ICANON;
        buf.c_iflag |= tc::ICRNL;
        let ok = self.tty.set_mode(&buf);
        if ok {
            self.cbreak = 0;
            self.tty.nttyb = buf;
        }
        ok
    }

    /// `_nc_initscr ()`: `cbreak`, and then echo, `ICRNL`, `INLCR`, `IGNCR`
    /// and `ONLCR` off -- the terminal's own echo and newline mapping, which
    /// curses does for itself.
    fn initscr_modes(&mut self) -> bool {
        if !self.cbreak() {
            return false;
        }
        let mut buf = self.tty.nttyb;
        buf.c_lflag &= !(tc::ECHO | tc::ECHONL);
        buf.c_iflag &= !(tc::ICRNL | tc::INLCR | tc::IGNCR);
        buf.c_oflag &= !tc::ONLCR;
        let ok = self.tty.set_mode(&buf);
        if ok {
            self.tty.nttyb = buf;
        }
        ok
    }

    /// `flushinp ()`: what has been typed and not read, thrown away.
    pub fn flushinp(&mut self) {
        // A descriptor that is no terminal has no typed-ahead input to lose;
        // upstream does not look at the result either.
        let _ = tc::flush_input(self.ifd);
    }

    // ---- refreshing

    /// `wnoutrefresh (stdscr)`: `stdscr`'s changes copied into `newscr`,
    /// whole wide characters at either edge, and its cursor with them.
    pub fn wnoutrefresh(&mut self) {
        let win = &mut self.stdscr;
        let newscr = &mut self.newscr;
        let begx = i32::from(win.begx);
        let begy = i32::from(win.begy);
        newscr.bkgd = win.bkgd;
        newscr.attrs = win.attrs;
        win.flags &= !HASMOVED;
        let mut limit_x = i32::from(win.maxx);
        if limit_x > i32::from(newscr.maxx).wrapping_sub(begx) {
            limit_x = i32::from(newscr.maxx).wrapping_sub(begx);
        }
        let mut src_row = 0i32;
        let mut dst_row = begy.wrapping_add(i32::from(win.yoffset));
        while src_row <= i32::from(win.maxy) && dst_row <= i32::from(newscr.maxy) {
            let (first, last) = win
                .line(src_row)
                .map_or((NOCHANGE, NOCHANGE), |l| (l.firstchar, l.lastchar));
            if first != NOCHANGE {
                let changed = (i32::from(first), i32::from(last));
                copy_changes(win, newscr, (src_row, dst_row), changed, limit_x);
            }
            if let Some(oline) = win.line_mut(src_row) {
                oline.firstchar = NOCHANGE;
                oline.lastchar = NOCHANGE;
            }
            src_row = src_row.wrapping_add(1);
            dst_row = dst_row.wrapping_add(1);
        }
        if win.clear {
            win.clear = false;
            newscr.clear = true;
        }
        if !win.leaveok {
            newscr.cury = short(
                i32::from(win.cury)
                    .wrapping_add(begy)
                    .wrapping_add(i32::from(win.yoffset)),
            );
            newscr.curx = short(i32::from(win.curx).wrapping_add(begx));
        }
        newscr.leaveok = win.leaveok;
    }

    /// `doupdate ()`: the terminal brought into line with `newscr` -- back
    /// in curses' modes first if `endwin` left them, at a new size first if
    /// the window changed -- with `SIGTSTP` held off meanwhile.
    pub fn doupdate(&mut self) {
        crate::signals::tstp(&mut self.tstp, false);
        if self.endwin == EndWin::Suspend || self.handle_sigwinch() {
            // "Check if the terminal size has changed while curses was off
            // (this can happen in an xterm, for example), and resize the
            // ncurses data structures accordingly."
            self.update_screensize();
        }
        if self.endwin == EndWin::Suspend {
            // "coming back from shell mode"
            self.reset_prog_mode();
            self.t.mvcur_resume();
            update::screen_resume(&mut self.t, &mut self.newscr);
            self.endwin = EndWin::Running;
        }
        let bkgd = self.stdscr.bkgd;
        update::do_update(
            &mut self.t,
            &mut self.newscr,
            &mut self.curscr,
            &mut self.hash,
            &*self.ctype,
            &bkgd,
        );
        crate::signals::tstp(&mut self.tstp, true);
    }

    /// `wrefresh (stdscr)`.
    pub fn refresh(&mut self) {
        self.wnoutrefresh();
        if self.stdscr.clear {
            self.newscr.clear = true;
        }
        self.doupdate();
        // "Reset the clearok() flag in case it was set for the special case
        // in hardscroll.c".
        self.stdscr.clear = false;
    }

    /// `wrefresh (curscr)`: the whole screen repainted.
    pub fn refresh_curscr(&mut self) {
        self.curscr.clear = true;
        self.doupdate();
    }

    /// `_nc_handle_sigwinch (sp)`: a `SIGWINCH` the handler caught noted
    /// against the screen; whether one is noted.
    fn handle_sigwinch(&mut self) -> bool {
        if crate::signals::take_sigwinch() {
            self.sig_winch = true;
        }
        self.sig_winch
    }

    /// `_nc_update_screensize (sp)`: the screen's size asked for again, and
    /// the screen resized to it if it changed.
    fn update_screensize(&mut self) {
        let old_lines = self.t.num(number::LINES);
        let old_cols = self.t.num(number::COLUMNS);
        let (new_lines, new_cols, tabsize) =
            get_screensize(&mut self.t.entry, self.tty.fd, self.use_env);
        self.tabsize = tabsize;
        // Upstream queues `KEY_RESIZE` when the size is the same but a
        // `SIGWINCH` came; this crate reads no input (module docs).
        if new_lines != old_lines || new_cols != old_cols {
            self.resizeterm(new_lines, new_cols);
        }
        self.sig_winch = false;
    }

    /// `endwin ()`: attributes, colours and the cursor put right for the
    /// shell, the alternate screen left, and the shell's modes back.
    pub fn endwin(&mut self) -> bool {
        let mut code = false;
        if self.endwin != EndWin::Suspend {
            self.endwin = EndWin::Suspend;
            update::screen_wrap(&mut self.t, &self.newscr, &mut self.curscr, &*self.ctype);
            self.t.mvcur_wrap(&self.newscr, &*self.ctype);
            code = true;
        }
        if !self.reset_shell_mode() {
            code = false;
        }
        code
    }

    // ---- resizing

    /// `is_term_resized (lines, cols)`.
    #[must_use]
    pub fn is_term_resized(&self, to_lines: i32, to_cols: i32) -> bool {
        to_lines > 0 && to_cols > 0 && (to_lines != self.t.lines || to_cols != self.t.columns)
    }

    /// Every window adjusted, in the order upstream's window list holds
    /// them -- the newest first: `stdscr`, `curscr`, `newscr`. (With no
    /// subwindows, `increase_size` and `decrease_size` visit each once.)
    fn adjust_all(&mut self, to: (i32, i32), stolen: i32) -> bool {
        let from = self.resize_from;
        let top = i32::from(self.topstolen);
        adjust_window(&mut self.stdscr, to, stolen, from, top)
            && adjust_window(&mut self.curscr, to, stolen, from, top)
            && adjust_window(&mut self.newscr, to, stolen, from, top)
    }

    /// `resize_term (lines, cols)`: the screen and its windows made the new
    /// size -- grown in lines, then in columns, then shrunk -- with no
    /// repainting.
    pub fn resize_term(&mut self, to_lines: i32, to_cols: i32) -> bool {
        if to_lines <= 0 || to_cols <= 0 {
            return false;
        }
        let mut ok = true;
        let was_stolen = self.t.lines.wrapping_sub(self.lines_avail);
        if self.is_term_resized(to_lines, to_cols) {
            let mut my_lines = self.t.lines;
            let mut my_cols = self.t.columns;
            self.resize_from = (my_lines, my_cols);
            if to_lines > self.t.lines {
                my_lines = to_lines;
                ok = self.adjust_all((my_lines, my_cols), was_stolen);
                self.resize_from = (my_lines, my_cols);
            }
            if ok && to_cols > self.t.columns {
                my_cols = to_cols;
                ok = self.adjust_all((my_lines, my_cols), was_stolen);
                self.resize_from = (my_lines, my_cols);
            }
            if ok && (to_lines < my_lines || to_cols < my_cols) {
                ok = self.adjust_all((to_lines, to_cols), was_stolen);
            }
            if ok {
                self.t.lines = to_lines;
                self.t.columns = to_cols;
                self.t.entry.set_number(number::LINES, to_lines);
                self.t.entry.set_number(number::COLUMNS, to_cols);
                self.lines_avail = to_lines.wrapping_sub(was_stolen);
                self.hash.oldhash = None;
                self.hash.newhash = None;
            }
        }
        if ok {
            // "Always update LINES, to allow for call from lib_doupdate.c
            // which needs to have the count adjusted by the stolen (ripped
            // off) lines."
            self.lines = to_lines.wrapping_sub(was_stolen);
            self.cols = to_cols;
        }
        ok
    }

    /// `resizeterm (lines, cols)`: [`Screen::resize_term`], and the whole
    /// screen repainted at the next refresh, since "screen contents are
    /// unknown".
    pub fn resizeterm(&mut self, to_lines: i32, to_cols: i32) -> bool {
        if to_lines <= 0 || to_cols <= 0 {
            return false;
        }
        self.sig_winch = false;
        let mut ok = true;
        if self.is_term_resized(to_lines, to_cols) {
            ok = self.resize_term(to_lines, to_cols);
            self.curscr.clear = true;
        }
        // Upstream queues `KEY_RESIZE` here (module docs).
        ok
    }

    // ---- colours

    /// `init_pair (pair, f, b)`, and the cells drawn in a pair whose colours
    /// changed made to be drawn again (`_nc_change_pair`).
    pub fn init_pair(&mut self, pair: i32, f: i32, b: i32) -> bool {
        match self.t.init_pair(pair, f, b) {
            None => false,
            Some(repaint) => {
                if repaint {
                    self.change_pair(pair);
                }
                true
            }
        }
    }

    /// `_nc_change_pair (sp, pair)`: every `curscr` cell in `pair` made
    /// nothing at all, which no refresh can leave as it is.
    fn change_pair(&mut self, pair: i32) {
        if self.curscr.clear {
            return;
        }
        for y in 0..=i32::from(self.curscr.maxy) {
            let mut changed = false;
            if let Some(line) = self.curscr.line_mut(y) {
                let width = i32::try_from(line.text.len()).unwrap_or(0);
                for x in 0..width {
                    if line.at(x).pair() == pair {
                        if let Some(c) = line.at_mut(x) {
                            c.set_char(0, 0);
                        }
                        line.changed_cell(x);
                        changed = true;
                    }
                }
            }
            if changed {
                self.hash.make_oldhash(&self.curscr, y);
            }
        }
    }

    /// `assume_default_colors (fg, bg)`: what colour pair 0 is taken to be,
    /// -1 meaning the terminal's own colour.
    pub fn assume_default_colors(&mut self, fg: i32, bg: i32) -> bool {
        if !self.t.assume_default_colors(fg, bg) {
            return false;
        }
        if !self.t.color_pairs.is_empty() {
            let save = self.t.default_color;
            self.t.assumed_color = true;
            self.t.default_color = true;
            // `init_pair` takes `short`s.
            self.init_pair(0, i32::from(short(fg)), i32::from(short(bg)));
            self.t.default_color = save;
        }
        true
    }

    /// `beep ()`: `bel`, else `flash`, sent and flushed.
    pub fn beep(&mut self) -> bool {
        if self.t.tputs_cap_always(string::BELL, 1, true) {
            self.t.flush();
            true
        } else if self.t.tputs_cap_always(string::FLASH_SCREEN, 1, true) {
            self.t.flush();
            self.t.flush();
            true
        } else {
            false
        }
    }
}

/// One line of `wnoutrefresh`: the changed columns `changed` of `win`'s row
/// copied into `newscr`'s, `rows` being the two, widened to whole wide
/// characters, and the rest of any wide character the copy cuts into
/// blanked.
fn copy_changes(
    win: &Window,
    newscr: &mut Window,
    rows: (i32, i32),
    changed: (i32, i32),
    limit_x: i32,
) {
    let (src_row, dst_row) = rows;
    let begx = i32::from(win.begx);
    let (first, mut last_src) = changed;
    if last_src > limit_x {
        last_src = limit_x;
    }
    let mut src_col = first;
    let mut dst_col = src_col.wrapping_add(begx);
    // "Ensure that we will copy complete multi-column characters on the
    // left-boundary."
    let here = win.cell(src_row, src_col);
    if here.is_widec_ext() {
        let j = 1i32
            .wrapping_add(dst_col)
            .wrapping_sub(here.widec_ext())
            .max(0);
        if dst_col > j {
            src_col = src_col.wrapping_sub(dst_col.wrapping_sub(j));
            dst_col = j;
        }
    }
    // "Ensure that we will copy complete multi-column characters on the
    // right-boundary."
    let mut j = last_src;
    if win.cell(src_row, j).widec_ext() != 0 {
        j = j.wrapping_add(1);
        while j <= limit_x {
            if win.cell(src_row, j).is_widec_base() {
                break;
            }
            last_src = j;
            j = j.wrapping_add(1);
        }
    }
    let last_dst = begx.wrapping_add(last_src.min(i32::from(win.maxx)));
    let mut fix_left = dst_col;
    let mut fix_right = last_dst;
    // "Check for boundary cases where we may overwrite part of a
    // multi-column character. For those, wipe the remainder of the
    // character to blanks."
    let there = newscr.cell(dst_row, dst_col);
    if there.is_widec_ext() {
        fix_left = 1i32
            .wrapping_add(dst_col)
            .wrapping_sub(there.widec_ext())
            .max(0);
    }
    if newscr.cell(dst_row, last_dst).widec_ext() != 0 {
        let mut j = last_dst.wrapping_add(1);
        while j <= i32::from(newscr.maxx) && newscr.cell(dst_row, j).is_widec_ext() {
            fix_right = j;
            j = j.wrapping_add(1);
        }
    }
    let Some(nline) = newscr.line_mut(dst_row) else {
        return;
    };
    if fix_left < dst_col || fix_right > last_dst {
        for k in fix_left..=fix_right {
            if let Some(c) = nline.at_mut(k) {
                *c = Cell::blank();
            }
            nline.changed_cell(k);
        }
    }
    // "Copy the changed text."
    while src_col <= last_src {
        let src = win.cell(src_row, src_col);
        if nline.at(dst_col) != src {
            if let Some(c) = nline.at_mut(dst_col) {
                *c = src;
            }
            nline.changed_cell(dst_col);
        }
        src_col = src_col.wrapping_add(1);
        dst_col = dst_col.wrapping_add(1);
    }
}

/// `adjust_window (win, to_lines, to_cols, stolen)`: one window made to fit
/// a screen going from `from` lines and columns to `to`.
fn adjust_window(
    win: &mut Window,
    to: (i32, i32),
    stolen: i32,
    from: (i32, i32),
    topstolen: i32,
) -> bool {
    let (to_lines, to_cols) = to;
    let (cur_lines, cur_cols) = from;
    let bottom = cur_lines.wrapping_add(topstolen).wrapping_sub(stolen);
    let mut my_lines = i32::from(win.maxy).wrapping_add(1);
    let mut my_cols = i32::from(win.maxx).wrapping_add(1);
    if i32::from(win.begy) >= bottom {
        // "If it is below the bottom of the new screen, move up by the same
        // amount that the screen shrank."
        win.begy = short(i32::from(win.begy).wrapping_add(to_lines.wrapping_sub(cur_lines)));
    } else if my_lines == cur_lines.wrapping_sub(stolen) && to_lines != cur_lines {
        my_lines = to_lines.wrapping_sub(stolen);
    } else if my_lines == cur_lines && to_lines != cur_lines {
        my_lines = to_lines;
    }
    if my_lines > to_lines {
        my_lines = to_lines;
    }
    if my_cols > to_cols {
        my_cols = to_cols;
    }
    if my_cols == cur_cols && to_cols != cur_cols {
        my_cols = to_cols;
    }
    win.wresize(my_lines, my_cols)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::addch;
    use crate::cell::{A_NORMAL, A_REVERSE, A_UNDERLINE};
    use crate::testing::{CLocale, Utf8};

    /// A screen on xterm-256color -- from the database, or from the entry
    /// built in where there is none -- kept in memory, at the size the
    /// entry gives (24 by 80).
    fn xterm(ctype: Box<dyn Ctype + Send>) -> Screen {
        let opts = Options {
            output: Sink::Memory(Vec::new()),
            input: -1,
            use_env: false,
        };
        Screen::newterm(b"xterm-256color", &opts, ctype).unwrap()
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    /// What a signal handler does to a screen the program has drawn --
    /// `endwin` for `SIGINT`, `SIGTERM` and a program's own handler, and for
    /// `SIGTSTP` that, then the modes kept and the screen repainted on the
    /// way back -- allocates and frees nothing: a handler that interrupted
    /// the program inside the allocator would wait on its lock for good
    /// (TD-B-CURSES-SIGNAL-WORK-ALLOCATES-IN-A-HANDLER). Written to a
    /// closed descriptor, so the bytes go nowhere and only the library's own
    /// work is counted; what it writes is the business of the other tests
    /// and of `scripts/curses-diff.sh`.
    #[test]
    fn the_work_a_signal_handler_does_allocates_nothing() {
        use crate::testing::allocations_in;
        let locales: [(&str, Box<dyn Ctype + Send>); 2] =
            [("C", Box::new(CLocale)), ("UTF-8", Box::new(Utf8))];
        for (name, ctype) in locales {
            let opts = Options {
                output: Sink::Fd(-1),
                input: -1,
                use_env: false,
            };
            let mut sp = Screen::newterm(b"xterm-256color", &opts, ctype).unwrap();
            assert!(sp.t.start_color());
            assert!(sp.init_pair(1, 1, 4));
            // A colour of the program's own, which the repaint restores
            // (`_nc_screen_resume`), as `watch` defines eight.
            assert!(sp.t.init_color(9, 1000, 333, 333));
            let text: &[u8] = if name == "C" {
                b"plain text"
            } else {
                "wide \u{65e5}\u{672c} text".as_bytes()
            };
            {
                let (w, ctx) = sp.stdscr_ctx();
                for y in 0..20 {
                    assert!(w.wmove(y, y));
                    assert!(addch::waddnstr(w, &ctx, text, -1));
                }
                w.wattrset(A_BOLD | A_REVERSE);
                assert!(w.wmove(21, 0));
                assert!(addch::waddnstr(w, &ctx, b"bold and reversed", -1));
                w.wattr_set(A_UNDERLINE, 1);
                assert!(addch::waddnstr(w, &ctx, b" in colour", -1));
                w.wattrset(A_NORMAL);
                assert!(w.wmove(22, 0));
                for _ in 0..30 {
                    assert!(addch::waddch(w, &ctx, Attr::from(b'q') | A_ALTCHARSET));
                }
            }
            sp.refresh();
            // A second frame, so the update has something to compare and
            // every table its size.
            {
                let (w, ctx) = sp.stdscr_ctx();
                assert!(w.wmove(5, 0));
                assert!(addch::waddnstr(w, &ctx, b"changed", -1));
            }
            sp.refresh();

            let ending = allocations_in(|| {
                sp.endwin();
            });
            let repainting = allocations_in(|| {
                sp.def_prog_mode();
                sp.flushinp();
                sp.def_shell_mode();
                sp.doupdate();
            });
            let ending_again = allocations_in(|| {
                sp.endwin();
            });
            assert_eq!(
                (ending, repainting, ending_again),
                (0, 0, 0),
                "allocator calls in endwin, the repaint after a suspension, and \
                 endwin again, in the {name} locale"
            );
        }
    }

    #[test]
    fn a_screen_is_the_entrys_size_and_knows_its_locale() {
        let sp = xterm(Box::new(CLocale));
        assert_eq!((sp.t.lines, sp.t.columns), (24, 80));
        assert_eq!((sp.lines, sp.cols, sp.tabsize), (24, 80, 8));
        assert_eq!(
            (i32::from(sp.stdscr.maxy), i32::from(sp.stdscr.maxx)),
            (23, 79)
        );
        assert!(!sp.t.screen_unicode);
        assert_eq!(sp.t.legacy_coding, 1);
        assert_eq!(sp.endwin, EndWin::Initial);
        assert!(sp.newscr.clear && !sp.curscr.clear);
        // No terminal to set modes on: nothing was read, nothing set.
        assert_eq!(sp.tty.fd, -1);
        assert_eq!(sp.tty.ottyb, Termios::default());

        let sp = xterm(Box::new(Utf8));
        assert!(sp.t.screen_unicode);
        assert_eq!(sp.t.legacy_coding, 0);
        // The line-drawing characters are Unicode's in a UTF-8 locale.
        assert_eq!(sp.t.wacs[usize::from(b'q')].ch(), 0x2500);
        assert_eq!(sp.t.wacs[usize::from(b'q')].attr, 0);
    }

    #[test]
    fn line_drawing_falls_back_and_then_follows_acsc() {
        let sp = xterm(Box::new(CLocale));
        // xterm maps every letter through `acsc` into the alternate set.
        assert_eq!(
            sp.t.acs_map[usize::from(b'q')],
            Attr::from(b'q') | A_ALTCHARSET
        );
        assert!(sp.t.screen_acs_map[usize::from(b'q')]);
        // Outside a UTF-8 locale the wide map is the letter, alternate.
        assert_eq!(sp.t.wacs[usize::from(b'q')].ch(), i32::from(b'q'));
        assert_eq!(sp.t.wacs[usize::from(b'q')].attr, A_ALTCHARSET);
    }

    #[test]
    fn the_first_refresh_enters_the_screen_and_endwin_leaves_it() {
        let mut sp = xterm(Box::new(CLocale));
        {
            let (w, ctx) = sp.stdscr_ctx();
            assert!(w.wmove(3, 7));
            assert!(addch::waddnstr(w, &ctx, b"hello", -1));
        }
        sp.refresh();
        let out = sp.t.take_written();
        // smcup first; the text where it was put.
        assert!(out.starts_with(b"\x1b[?1049h"), "{out:?}");
        assert!(contains(&out, b"\x1b[4;8Hhello"), "{out:?}");
        assert_eq!(sp.endwin, EndWin::Initial);

        // With no terminal to give the shell's modes back to, `endwin`
        // reports failure, as upstream's does -- after doing the rest.
        assert!(!sp.endwin());
        assert_eq!(sp.endwin, EndWin::Suspend);
        let out = sp.t.take_written();
        // rmcup, and rmkx last, whether or not keypad mode was ever on.
        assert!(contains(&out, b"\x1b[?1049l"), "{out:?}");
        assert!(out.ends_with(b"\x1b[?1l\x1b>"), "{out:?}");

        // A second endwin has nothing to wrap up, only the modes to reset.
        assert!(!sp.endwin());
        assert_eq!(sp.t.take_written(), b"\x1b[?1l\x1b>");

        // The next refresh comes back: smcup again, and a repaint.
        sp.refresh();
        assert_eq!(sp.endwin, EndWin::Running);
        let out = sp.t.take_written();
        assert!(contains(&out, b"\x1b[?1049h"), "{out:?}");
        assert!(contains(&out, b"hello"), "{out:?}");
    }

    #[test]
    fn an_unchanged_screen_refreshes_to_nothing() {
        let mut sp = xterm(Box::new(CLocale));
        sp.refresh();
        let _ = sp.t.take_written();
        sp.refresh();
        assert_eq!(sp.t.take_written(), b"");
    }

    #[test]
    fn resizing_resizes_every_window_and_repaints() {
        let mut sp = xterm(Box::new(CLocale));
        sp.refresh();
        assert!(!sp.is_term_resized(24, 80));
        assert!(!sp.is_term_resized(0, 100));
        assert!(sp.is_term_resized(30, 100));
        assert!(sp.resizeterm(30, 100));
        for w in [&sp.stdscr, &sp.curscr, &sp.newscr] {
            assert_eq!((i32::from(w.maxy), i32::from(w.maxx)), (29, 99));
        }
        assert_eq!((sp.lines, sp.cols, sp.lines_avail), (30, 100, 30));
        assert_eq!(
            (sp.t.num(number::LINES), sp.t.num(number::COLUMNS)),
            (30, 100)
        );
        assert!(sp.curscr.clear);
        // Shrinking works the same way; a size of nothing is refused.
        assert!(sp.resizeterm(10, 20));
        assert_eq!(
            (i32::from(sp.stdscr.maxy), i32::from(sp.stdscr.maxx)),
            (9, 19)
        );
        assert!(!sp.resizeterm(0, 20));
        assert!(!sp.resize_term(10, -1));
    }

    #[test]
    fn a_wide_character_cut_into_on_newscr_is_blanked_whole() {
        let mut sp = xterm(Box::new(Utf8));
        {
            let (w, ctx) = sp.stdscr_ctx();
            assert!(w.wmove(0, 0));
            // U+4E00, two columns, at columns 0 and 1.
            assert!(addch::waddnwstr(w, &ctx, &[0x4e00, 0x61], -1));
        }
        sp.wnoutrefresh();
        assert_eq!(sp.newscr.cell(0, 0).ch(), 0x4e00);
        assert!(sp.newscr.cell(0, 1).is_widec_ext());
        // Now column 1 alone is written over: the character's first half,
        // left behind, goes too.
        {
            let (w, ctx) = sp.stdscr_ctx();
            assert!(w.wmove(0, 1));
            assert!(addch::waddnstr(w, &ctx, b"z", -1));
        }
        sp.wnoutrefresh();
        assert_eq!(sp.newscr.cell(0, 1).ch(), i32::from(b'z'));
        assert!(!sp.newscr.cell(0, 0).is_widec_ext());
        assert_ne!(sp.newscr.cell(0, 0).ch(), 0x4e00);
    }

    #[test]
    fn colours_start_and_pair_zero_can_be_the_terminals_own() {
        let mut sp = xterm(Box::new(CLocale));
        assert!(sp.t.has_colors());
        assert!(sp.t.start_color());
        assert_eq!((sp.t.color_count, sp.t.pair_count), (256, 65536));
        assert!(sp.assume_default_colors(-1, -1));
        assert!(sp.init_pair(1, 1, -1));
        // A pair outside the table is refused.
        assert!(!sp.init_pair(-1, 1, 2));
    }

    #[test]
    fn assumed_colours_are_read_as_sscanf_reads_them() {
        assert_eq!(scan_assumed(b""), (0, 0, 0));
        assert_eq!(scan_assumed(b"x"), (0, 0, 0));
        assert_eq!(scan_assumed(b"7"), (1, 7, 0));
        assert_eq!(scan_assumed(b"7,"), (2, 7, 0));
        assert_eq!(scan_assumed(b"7,x"), (2, 7, 0));
        assert_eq!(scan_assumed(b"7,0"), (3, 7, 0));
        assert_eq!(scan_assumed(b" -1;4\n"), (4, -1, 4));
        // `%d` reads every digit there is; `%c` then has nothing.
        assert_eq!(scan_assumed(b"12"), (1, 12, 0));
        // Out of a `long`: clamped, then cut to an `int`, as glibc's is.
        assert_eq!(scan_assumed(b"99999999999999999999"), (1, -1, 0));
        assert_eq!(scan_assumed(b"4294967298"), (1, 2, 0));
    }
}
