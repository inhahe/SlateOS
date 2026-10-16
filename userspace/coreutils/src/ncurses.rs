//! What ncurses 6.4's `progs/` share -- `tty_settings.c`, `reset_cmd.c`,
//! `clear_cmd.c` and `transform.c` -- for the ports of `tput`, `clear` and
//! `tset` (which is also `reset`).
//!
//! The terminal's description comes from `userspace/terminfo`; what is here
//! is what the programs do with it: find a descriptor whose settings can be
//! read (standard error first, then standard output, then standard input,
//! then `/dev/tty`), put the line back into a sane state, choose the erase,
//! interrupt and kill characters, send the initialization strings -- margins
//! and tab stops among them -- and report the characters a user should know
//! have changed. Each is a transcription of upstream's function of the same
//! name, quirks included: `send_init_strings` sets the line to its *old*
//! settings with every output flag but four cleared before it writes a byte,
//! and the tab stops are spaced with `%*s`.
//!
//! # Two copies of the terminal
//!
//! ncurses keeps a terminal twice: the copy `tigetstr` and `tigetnum` read,
//! and the legacy one the `CUR` macros read -- `clear_screen`,
//! `init_2string`, `lines` in the programs' own code. The copies differ.
//! The legacy copy's numbers are `short`s: a number past 32767 is 32767
//! there, the screen size is put there truncated (`LINES=40000` is
//! -25536), and what `tset` and `tput reset` write -- the window size into
//! `lines` and `columns`, `init_tabs` cut to the width -- goes there alone;
//! [`Legacy`] is that half. Its strings have storage of their own, in which
//! a one-byte `$CC` has replaced the terminal's command character
//! (`terminfo::legacy_copy`); every string sent here is read from those,
//! and passed around as `strings`, beside the terminal itself as `entry`.
//!
//! # The control characters
//!
//! The defaults are glibc's `<sys/ttydefaults.h>`, which the reference's
//! build sees: erase DEL, interrupt `^C`, kill `^U`. Measured, `tset` on a
//! fresh pseudo-terminal reports nothing.

use std::io;

use libcall::termios::{self as tc, Termios};
use terminfo::{Entry, Outc, Padding, TiString, Tparm};

/// `ErrUsage`.
pub const ERR_USAGE: u8 = 2;
/// `ErrTermType`.
pub const ERR_TERM_TYPE: u8 = 3;
/// `ErrCapName`.
pub const ERR_CAP_NAME: u8 = 4;

/// `ErrSystem (n)`: 4 and `n`, as the low byte `exit` keeps.
#[must_use]
pub fn err_system(n: i32) -> u8 {
    low_byte(4i32.wrapping_add(n))
}

/// The low byte of an `int`, as `exit` and `(unsigned char)` keep it.
#[must_use]
pub fn low_byte(n: i32) -> u8 {
    n.to_le_bytes().first().copied().unwrap_or(0)
}

/// `(short) n`: the low 16 bits, sign-extended.
#[must_use]
pub fn as_short(n: i32) -> i32 {
    let [lo, hi, ..] = n.to_le_bytes();
    i32::from(i16::from_le_bytes([lo, hi]))
}

/// `_nc_rootname (path)`: what follows the last `/`.
#[must_use]
pub fn rootname(path: &[u8]) -> &[u8] {
    path.iter().rposition(|&c| c == b'/').map_or(path, |i| {
        path.get(i.saturating_add(1)..).unwrap_or_default()
    })
}

/// `same_program (a, b)`: the names equal -- there is no executable suffix
/// to ignore here.
#[must_use]
pub fn same_program(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// What `-V` prints. Upstream prints `curses_version ()`, `ncurses
/// 6.4.20240113`; every program here names SlateOS's coreutils instead.
#[must_use]
pub fn version_line(progname: &[u8]) -> Vec<u8> {
    [progname, b" (SlateOS coreutils) 0.1.0\n"].concat()
}

/// `strerror (errno)`.
fn strerror(errno: i32) -> String {
    crate::errmsg::strerror(&io::Error::from_raw_os_error(errno))
}

/// A terminal's string capability, when it has one.
#[must_use]
pub fn cap<'e>(entry: &'e Entry, name: &str) -> Option<&'e [u8]> {
    match entry.tigetstr(name.as_bytes()) {
        TiString::Value(s) => Some(s),
        TiString::Absent | TiString::NotAString => None,
    }
}

/// The `short` copy of the terminal's numbers that `CUR`'s macros read,
/// as far as the programs here read it: see the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Legacy {
    /// `lines`.
    pub lines: i32,
    /// `columns`.
    pub columns: i32,
    /// `init_tabs`.
    pub init_tabs: i32,
}

impl Legacy {
    /// As `setupterm` leaves it: `_nc_export_termtype2`'s copy, every number
    /// past 32767 made 32767, and then the screen size put into `lines` and
    /// `columns` as `short`s.
    #[must_use]
    pub fn of(entry: &Entry) -> Self {
        /// `cols`: number 0 of `term.h`.
        const COLUMNS: usize = 0;
        /// `it`: number 1.
        const INIT_TABS: usize = 1;
        /// `lines`: number 2.
        const LINES: usize = 2;
        Self {
            lines: as_short(entry.number(LINES)),
            columns: as_short(entry.number(COLUMNS)),
            init_tabs: entry.number(INIT_TABS).min(i32::from(i16::MAX)),
        }
    }
}

/// `tty_settings.c`'s state: the descriptor the program set up on, and the
/// settings it found there to put back.
#[derive(Clone, Copy, Debug)]
pub struct TtySettings {
    /// `my_fd`.
    fd: i32,
    /// `original_settings`, when `can_restore`.
    original: Option<Termios>,
}

impl TtySettings {
    /// `save_tty_settings (&settings, need_tty)`: the first of standard
    /// error, output and input whose settings can be read -- kept, to be put
    /// back -- else, when a terminal is needed, `/dev/tty` (whose are not
    /// kept), else standard output with no settings at all.
    ///
    /// # Errors
    ///
    /// A needed terminal that cannot be had: `failed ("terminal
    /// attributes")`, whose complaint is written and whose status is
    /// returned.
    pub fn save(need_tty: bool, progname: &[u8]) -> Result<(Self, Option<Termios>), u8> {
        for fd in [2, 1, 0] {
            if let Ok(settings) = tc::get_attr(fd) {
                let saved = Self {
                    fd,
                    original: Some(settings),
                };
                return Ok((saved, Some(settings)));
            }
        }
        if !need_tty {
            let saved = Self {
                fd: 1,
                original: None,
            };
            return Ok((saved, None));
        }
        // Never closed, as upstream never closes it.
        let attempt = open_tty().and_then(|fd| tc::get_attr(fd).map(|s| (fd, s)));
        match attempt {
            Ok((fd, settings)) => Ok((Self { fd, original: None }, Some(settings))),
            Err(errno) => {
                // `failed`: nothing to restore yet, then a newline.
                let mut m = crate::quote::escape_unprintable(progname).into_bytes();
                m.extend_from_slice(b": terminal attributes: ");
                m.extend_from_slice(strerror(errno).as_bytes());
                m.extend_from_slice(b"\n\n");
                ulclosestream::stderr_write(&m);
                Err(err_system(errno))
            }
        }
    }

    /// The descriptor: `save_tty_settings`'s answer.
    #[must_use]
    pub fn fd(&self) -> i32 {
        self.fd
    }

    /// `restore_tty_settings`.
    pub fn restore(&self) {
        if let Some(original) = &self.original {
            // Unchecked, as upstream's `SET_TTY` is.
            let _ = tc::set_attr(self.fd, tc::TCSADRAIN, original);
        }
    }

    /// `update_tty_settings (old, new)`: the new settings set, if they
    /// differ from the old.
    pub fn update(&self, old: &Termios, new: &Termios) {
        if old != new {
            // Unchecked, as upstream's `SET_TTY` is.
            let _ = tc::set_attr(self.fd, tc::TCSADRAIN, new);
        }
    }
}

/// `open ("/dev/tty", O_RDWR)`: a descriptor of our own, or the `errno`.
#[cfg(unix)]
fn open_tty() -> Result<i32, i32> {
    use std::os::fd::IntoRawFd;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map(IntoRawFd::into_raw_fd)
        .map_err(|e| e.raw_os_error().unwrap_or(0))
}

/// A host with no `/dev/tty` of ours: `ENXIO`, what a process with no
/// controlling terminal is told.
#[cfg(not(unix))]
fn open_tty() -> Result<i32, i32> {
    Err(6)
}

/// glibc's `<sys/ttydefaults.h>`, which the reference's build reads.
pub mod defaults {
    /// `CEOF`: `^D`.
    pub const CEOF: u8 = 0o4;
    /// `CERASE`: DEL.
    pub const CERASE: u8 = 0o177;
    /// `CINTR`: `^C`.
    pub const CINTR: u8 = 0o3;
    /// `CKILL`: `^U`.
    pub const CKILL: u8 = 0o25;
    /// `CLNEXT`: `^V`.
    pub const CLNEXT: u8 = 0o26;
    /// `CRPRNT`: `^R`.
    pub const CRPRNT: u8 = 0o22;
    /// `CQUIT`: `^\`.
    pub const CQUIT: u8 = 0o34;
    /// `CSTART`: `^Q`.
    pub const CSTART: u8 = 0o21;
    /// `CSTOP`: `^S`.
    pub const CSTOP: u8 = 0o23;
    /// `CSUSP`: `^Z`.
    pub const CSUSP: u8 = 0o32;
    /// `CWERASE`: `^W`.
    pub const CWERASE: u8 = 0o27;
    /// `CDISCARD`: `^O`.
    pub const CDISCARD: u8 = 0o17;
}

/// `DISABLED (val)`: `_POSIX_VDISABLE`, which is NUL on Linux.
fn disabled(val: u8) -> bool {
    val == 0
}

/// `reset_char (item, value)`: a disabled slot given its default.
fn reset_char(settings: &mut Termios, item: usize, value: u8) {
    if let Some(slot) = settings.c_cc.get_mut(item)
        && disabled(*slot)
    {
        *slot = value;
    }
}

/// `reset_tty_settings (fd, settings, noset)`: the settings read again --
/// unchecked, as upstream reads them -- and made sane: each disabled
/// control character its default, parity, stripping and delays off, eight
/// bits, canonical input with echo, and `CLOCAL` off unless the line has
/// modem lines. Set on `fd` unless `noset`.
pub fn reset_tty_settings(fd: i32, settings: &mut Termios, noset: bool) {
    use defaults as d;
    if let Ok(current) = tc::get_attr(fd) {
        *settings = current;
    }
    reset_char(settings, tc::VDISCARD, d::CDISCARD);
    reset_char(settings, tc::VEOF, d::CEOF);
    reset_char(settings, tc::VERASE, d::CERASE);
    reset_char(settings, tc::VINTR, d::CINTR);
    reset_char(settings, tc::VKILL, d::CKILL);
    reset_char(settings, tc::VLNEXT, d::CLNEXT);
    reset_char(settings, tc::VQUIT, d::CQUIT);
    reset_char(settings, tc::VREPRINT, d::CRPRNT);
    reset_char(settings, tc::VSTART, d::CSTART);
    reset_char(settings, tc::VSTOP, d::CSTOP);
    reset_char(settings, tc::VSUSP, d::CSUSP);
    reset_char(settings, tc::VWERASE, d::CWERASE);

    settings.c_iflag &= !(tc::IGNBRK
        | tc::PARMRK
        | tc::INPCK
        | tc::ISTRIP
        | tc::INLCR
        | tc::IGNCR
        | tc::IUCLC
        | tc::IXANY
        | tc::IXOFF);
    settings.c_iflag |= tc::BRKINT | tc::IGNPAR | tc::ICRNL | tc::IXON | tc::IMAXBEL;
    settings.c_oflag &= !(tc::OLCUC
        | tc::OCRNL
        | tc::ONOCR
        | tc::ONLRET
        | tc::OFILL
        | tc::OFDEL
        | tc::NLDLY
        | tc::CRDLY
        | tc::TABDLY
        | tc::BSDLY
        | tc::VTDLY
        | tc::FFDLY);
    settings.c_oflag |= tc::OPOST | tc::ONLCR;
    let mut mask = tc::CSIZE | tc::CSTOPB | tc::PARENB | tc::PARODD;
    // "leave clocal alone if this appears to use a modem"
    if libcall::pty::modem_bits(fd).is_err() {
        mask |= tc::CLOCAL;
    }
    settings.c_cflag &= !mask;
    settings.c_cflag |= tc::CS8 | tc::CREAD;
    settings.c_lflag &= !(tc::ECHONL | tc::NOFLSH | tc::TOSTOP | tc::ECHOPRT | tc::XCASE);
    settings.c_lflag |=
        tc::ISIG | tc::ICANON | tc::ECHO | tc::ECHOE | tc::ECHOK | tc::ECHOCTL | tc::ECHOKE;
    if !noset {
        // Unchecked, as upstream's `SET_TTY` is.
        let _ = tc::set_attr(fd, tc::TCSADRAIN, settings);
    }
}

/// `default_erase`: on an overstriking terminal its `kbs`, when that is one
/// byte; else `CERASE`. `strings` is the terminal's legacy copy
/// ([`terminfo::legacy_copy`]), which is what upstream's macros read, here
/// and in everything below that takes one.
fn default_erase(strings: &Entry) -> u8 {
    if strings.tigetflag(b"os") == 1
        && let Some([only]) = cap(strings, "kbs")
    {
        return *only;
    }
    defaults::CERASE
}

/// `set_control_chars (settings, erase, intr, kill)`: each of the three set
/// to the one asked for (a non-negative value), and otherwise -- only when
/// it is disabled -- to its default.
pub fn set_control_chars(
    settings: &mut Termios,
    strings: &Entry,
    erase: i32,
    intr: i32,
    kill: i32,
) {
    let erase_default = default_erase(strings);
    for (slot, asked, default) in [
        (tc::VERASE, erase, erase_default),
        (tc::VINTR, intr, defaults::CINTR),
        (tc::VKILL, kill, defaults::CKILL),
    ] {
        if let Some(c) = settings.c_cc.get_mut(slot)
            && (disabled(*c) || asked >= 0)
        {
            *c = if asked >= 0 { low_byte(asked) } else { default };
        }
    }
}

/// `set_conversions (settings)`: newline to CR-LF and back, echo, and the
/// erase and kill echoes -- the first two not on a terminal whose `nel` is a
/// bare newline.
pub fn set_conversions(settings: &mut Termios, strings: &Entry) {
    settings.c_oflag |= tc::ONLCR;
    settings.c_iflag |= tc::ICRNL;
    settings.c_lflag |= tc::ECHO;
    // `OXTABS` is 0 on Linux: nothing to set, nothing to clear.
    if cap(strings, "nel") == Some(b"\n") {
        // "Newline, not linefeed."
        settings.c_oflag &= !tc::ONLCR;
        settings.c_iflag &= !tc::ICRNL;
    }
    settings.c_lflag |= tc::ECHOE | tc::ECHOK;
}

/// `my_file`: where `reset_cmd.c` writes. Standard output through its
/// buffer (`tput`), or standard error, unbuffered (`tset`).
pub enum MyFile<'a> {
    /// `stdout`.
    Stdout(&'a mut ulclosestream::Stdout),
    /// `stderr`.
    Stderr,
}

impl MyFile<'_> {
    /// `fflush (my_file)`: `reset_flush`.
    pub fn fflush(&mut self) {
        if let MyFile::Stdout(out) = self {
            out.flush();
        }
    }

    /// `fwrite (bytes, 1, len, my_file)`, its answer looked at: standard
    /// output fails only when a write the buffer forces out fails, standard
    /// error -- unbuffered -- when this write does.
    ///
    /// # Errors
    ///
    /// The failed write's.
    pub fn fwrite(&mut self, bytes: &[u8]) -> io::Result<()> {
        match self {
            MyFile::Stdout(out) => out.fwrite(bytes),
            MyFile::Stderr => ulclosestream::stderr_raw(bytes),
        }
    }
}

impl Outc for MyFile<'_> {
    /// `putc (c, my_file)`.
    fn put(&mut self, bytes: &[u8]) {
        match self {
            MyFile::Stdout(out) => out.write(bytes),
            MyFile::Stderr => ulclosestream::stderr_write(bytes),
        }
    }

    /// `_nc_flush ()`: standard output. Standard error holds nothing, and
    /// `tset` has written nothing to standard output by then.
    fn flush(&mut self) {
        self.fflush();
    }
}

/// `reset_cmd.c`'s writing half: `reset_start`'s three, and what the rest
/// reads of the terminal.
pub struct Reset<'r, 'o> {
    /// `my_file`.
    pub file: &'r mut MyFile<'o>,
    /// `use_reset`: invoked as `reset`.
    pub use_reset: bool,
    /// `use_init`: invoked as `init`.
    pub use_init: bool,
    /// The terminal, as `tigetstr` -- and `tparm`'s own checks -- read it.
    pub entry: &'r Entry,
    /// Its legacy copy ([`terminfo::legacy_copy`]): the strings sent here
    /// are upstream's macros, which read that.
    pub strings: &'r Entry,
    /// Its `short` numbers.
    pub legacy: &'r mut Legacy,
    /// Its `tparm` state.
    pub tparm: &'r mut Tparm,
    /// How `tputs` pads on it.
    pub padding: Padding,
    /// `_nc_progname`, for `failed`.
    pub progname: &'r [u8],
}

impl<'r> Reset<'r, '_> {
    /// `out_char`.
    fn out_char(&mut self, c: u8) {
        self.file.put(&[c]);
    }

    /// `tputs (s, 0, out_char)`.
    fn tputs0(&mut self, s: &[u8]) {
        terminfo::tputs_to(s, 0, &self.padding, &mut *self.file);
    }

    /// `sent_string`: `tputs (s, 0, out_char)` for a string there is.
    fn sent_string(&mut self, s: Option<&[u8]>) -> bool {
        match s {
            Some(s) => {
                self.tputs0(s);
                true
            }
            None => false,
        }
    }

    /// `to_left_margin`.
    fn left_margin(&mut self) -> bool {
        match cap(self.strings, "cr") {
            Some(cr) => {
                self.sent_string(Some(cr));
            }
            None => self.out_char(b'\r'),
        }
        true
    }

    /// `reset_tabstops (wide)`: unless `it` is 8 (or absent), every stop
    /// cleared and one set each `it` columns -- `it` cut to the width first,
    /// in the `short` copy, where it stays.
    fn reset_tabstops(&mut self, wide: i32) -> bool {
        let (Some(hts), Some(tbc)) = (cap(self.strings, "hts"), cap(self.strings, "tbc")) else {
            return false;
        };
        if self.legacy.init_tabs == 8 || self.legacy.init_tabs < 0 {
            return false;
        }
        self.left_margin();
        self.tputs0(tbc);
        if self.legacy.init_tabs > 1 {
            if self.legacy.init_tabs > wide {
                self.legacy.init_tabs = as_short(wide);
            }
            let step = self.legacy.init_tabs;
            let mut c = step;
            while c < wide {
                // `fprintf (my_file, "%*s", init_tabs, " ")`.
                let width = usize::try_from(step).unwrap_or(0).max(1);
                self.file.put(&vec![b' '; width]);
                self.tputs0(hts);
                c = c.saturating_add(step);
            }
            self.left_margin();
        }
        true
    }

    /// `cat_file (file)`: a file's bytes to `my_file`, `BUFSIZ` at a time.
    /// A file that will not open, or a write that fails, is `failed`; a read
    /// that fails ends the copy as the end of the file does -- so a
    /// directory, which opens, sends nothing and says nothing.
    fn cat_file(&mut self, file: Option<&[u8]>, tty: &TtySettings) -> Result<bool, u8> {
        /// `BUFSIZ`.
        const BUFSIZ: usize = 8192;
        let Some(file) = file else {
            return Ok(false);
        };
        let mut fp = match std::fs::File::open(crate::quote::os_from_bytes(file)) {
            Ok(f) => f,
            Err(e) => return Err(self.failed(file, &e, tty)),
        };
        let mut sent = false;
        let mut buf = vec![0u8; BUFSIZ];
        loop {
            let nr = fread(&mut fp, &mut buf);
            let Some(chunk) = buf.get(..nr).filter(|c| !c.is_empty()) else {
                break;
            };
            if let Err(e) = self.file.fwrite(chunk) {
                return Err(self.failed(file, &e, tty));
            }
            sent = true;
        }
        Ok(sent)
    }

    /// `failed (msg)`: the complaint, the settings put back, a newline to
    /// `my_file`, and `ErrSystem (errno)`.
    fn failed(&mut self, what: &[u8], e: &io::Error, tty: &TtySettings) -> u8 {
        let mut m = crate::quote::escape_unprintable(self.progname).into_bytes();
        m.extend_from_slice(b": ");
        m.extend_from_slice(crate::quote::escape_unprintable(what).as_bytes());
        m.extend_from_slice(b": ");
        m.extend_from_slice(crate::errmsg::strerror(e).as_bytes());
        m.push(b'\n');
        ulclosestream::stderr_write(&m);
        tty.restore();
        self.file.put(b"\n");
        self.file.fflush();
        err_system(e.raw_os_error().unwrap_or(0))
    }

    /// The reset string where there is one and this is `reset`, else the
    /// init string: `(use_reset && (reset_1string != 0)) ? reset_1string :
    /// init_1string`.
    fn reset_or_init(&self, reset: &str, init: &str) -> Option<&'r [u8]> {
        let strings: &'r Entry = self.strings;
        match cap(strings, reset) {
            Some(r) if self.use_reset => Some(r),
            _ => cap(strings, init),
        }
    }

    /// `send_init_strings (fd, old_settings)`: whether anything was sent.
    ///
    /// # Errors
    ///
    /// An init or reset file that cannot be read: the status to exit with,
    /// the complaint already written.
    pub fn send_init_strings(
        &mut self,
        fd: i32,
        old_settings: Option<&mut Termios>,
        tty: &TtySettings,
    ) -> Result<bool, u8> {
        let mut need_flush = false;
        // `TAB3` exists, so: the old settings with every output flag but
        // these four cleared -- the mask is ANDed in, not out -- set before
        // anything is written.
        if let Some(old) = old_settings {
            let keep = tc::TAB3 | tc::ONLCR | tc::OCRNL | tc::ONLRET;
            if old.c_oflag & keep != 0 {
                old.c_oflag &= keep;
                // Unchecked, as upstream's `SET_TTY` is.
                let _ = tc::set_attr(fd, tc::TCSADRAIN, old);
            }
        }
        if !(self.use_reset || self.use_init) {
            return Ok(need_flush);
        }
        // The strings are the macros' -- the legacy copy's; `tparm` checks
        // them against the terminal's own.
        let entry = self.entry;
        let strings = self.strings;
        if let Some(prog) = cap(strings, "iprog") {
            // `system (init_prog)`, its status ignored -- and what this
            // process holds in its buffer not flushed first, as `system`
            // does not flush it.
            let _ = crate::shell::shell_bytes(prog).status();
        }
        let s = self.reset_or_init("rs1", "is1");
        need_flush |= self.sent_string(s);
        let s = self.reset_or_init("rs2", "is2");
        need_flush |= self.sent_string(s);

        let last = self.legacy.columns.wrapping_sub(1);
        if let Some(mgc) = cap(strings, "mgc") {
            need_flush |= self.sent_string(Some(mgc));
        } else if let Some(smglr) = cap(strings, "smglr") {
            let s = self.tparm.nc_tiparm(entry, 2, smglr, &[0, last]);
            need_flush |= self.sent_string(s.as_deref());
        } else if let (Some(smglp), Some(smgrp)) = (cap(strings, "smglp"), cap(strings, "smgrp")) {
            let s = self.tparm.nc_tiparm(entry, 1, smglp, &[0]);
            need_flush |= self.sent_string(s.as_deref());
            let s = self.tparm.nc_tiparm(entry, 1, smgrp, &[last]);
            need_flush |= self.sent_string(s.as_deref());
        } else if let (Some(smgl), Some(smgr)) = (cap(strings, "smgl"), cap(strings, "smgr")) {
            need_flush |= self.left_margin();
            need_flush |= self.sent_string(Some(smgl));
            if let Some(cuf) = cap(strings, "cuf") {
                let s = self.tparm.nc_tiparm(entry, 1, cuf, &[last]);
                need_flush |= self.sent_string(s.as_deref());
            } else {
                for _ in 0..last.max(0) {
                    self.out_char(b' ');
                    need_flush = true;
                }
            }
            need_flush |= self.sent_string(Some(smgr));
            need_flush |= self.left_margin();
        }

        let columns = self.legacy.columns;
        need_flush |= self.reset_tabstops(columns);

        let file = self.reset_or_init("rf", "if");
        need_flush |= self.cat_file(file, tty)?;

        let s = self.reset_or_init("rs3", "is3");
        need_flush |= self.sent_string(s);
        Ok(need_flush)
    }
}

/// `fread (buf, 1, buf.len (), fp)`: as many bytes as fill `buf`, or as were
/// there before the end of the file or a read that failed -- which `fread`
/// does not tell apart from it, as `cat_file` does not ask.
fn fread(fp: &mut std::fs::File, buf: &mut [u8]) -> usize {
    let mut got = 0usize;
    while let Some(rest) = buf.get_mut(got..).filter(|r| !r.is_empty()) {
        match io::Read::read(fp, rest) {
            Ok(0) => break,
            Ok(n) => got = got.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    got
}

/// `show_tty_change`: what became of one control character, unless it is
/// what it was and what it should be.
fn show_tty_change(
    old: &Termios,
    new: &Termios,
    name: &str,
    which: usize,
    def: u8,
    strings: &Entry,
) {
    let newer = new.c_cc.get(which).copied().unwrap_or(0);
    let older = old.c_cc.get(which).copied().unwrap_or(0);
    if older == newer && older == def {
        return;
    }
    let mut m = format!("{name} {} ", if older == newer { "is" } else { "set to" }).into_bytes();
    // "Check 'delete' before 'backspace', since the key_backspace value is
    // ambiguous."
    if disabled(newer) {
        m.extend_from_slice(b"undef.\n");
    } else if newer == 0o177 {
        m.extend_from_slice(b"delete.\n");
    } else if cap(strings, "kbs") == Some(&[newer][..]) {
        m.extend_from_slice(b"backspace.\n");
    } else if newer < 0o40 {
        let shown = newer ^ 0o100;
        m.extend_from_slice(b"control-");
        m.push(shown);
        m.extend_from_slice(b" (^");
        m.push(shown);
        m.extend_from_slice(b").\n");
    } else {
        m.push(newer);
        m.extend_from_slice(b".\n");
    }
    ulclosestream::stderr_write(&m);
}

/// `print_tty_chars (old, new)`: erase, kill and interrupt.
pub fn print_tty_chars(old: &Termios, new: &Termios, strings: &Entry) {
    show_tty_change(old, new, "Erase", tc::VERASE, defaults::CERASE, strings);
    show_tty_change(old, new, "Kill", tc::VKILL, defaults::CKILL, strings);
    show_tty_change(old, new, "Interrupt", tc::VINTR, defaults::CINTR, strings);
}

/// `set_window_size (fd, &lines, &columns)`: a window with no size given
/// the terminal's; the size of one that has one taken, into the `short`
/// copy.
pub fn set_window_size(fd: i32, legacy: &mut Legacy) {
    // Unchecked, as upstream's `ioctl` is; what a failed one leaves is
    // whatever was on the stack, read here as no size at all.
    let win = libcall::pty::window_size(fd).unwrap_or_default();
    if win.rows == 0 && win.cols == 0 {
        if legacy.lines > 0 && legacy.columns > 0 {
            let size = libcall::pty::WinSize {
                rows: u16::try_from(legacy.lines).unwrap_or(u16::MAX),
                cols: u16::try_from(legacy.columns).unwrap_or(u16::MAX),
                ..win
            };
            // Unchecked, as upstream's `ioctl` is.
            let _ = libcall::pty::set_window_size(fd, size);
        }
    } else if win.rows > 0 && win.cols > 0 {
        legacy.lines = as_short(i32::from(win.rows));
        legacy.columns = as_short(i32::from(win.cols));
    }
}

/// `putchar` through the program's standard output -- `clear_cmd`'s `putch`
/// and `putp`'s `_nc_putchar` -- with `_nc_flush`'s `fflush (stdout)`.
pub struct PutChar<'a>(pub &'a mut ulclosestream::Stdout);

impl Outc for PutChar<'_> {
    fn put(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }

    fn flush(&mut self) {
        self.0.flush();
    }
}

/// `clear_cmd (legacy)`: `clear` with `lines` as the count affected, then --
/// unless `legacy` (`-x`) -- the scrollback's `E3`. `false` for a terminal
/// with no `clear`, whose `tputs` is `ERR`; `E3` is still sent.
///
/// `clear` is the macro `clear_screen`, so it comes from `strings`, the
/// legacy copy; `E3` is asked of `tigetstr`, so it comes from `entry`.
pub fn clear_cmd(
    out: &mut ulclosestream::Stdout,
    strings: &Entry,
    entry: &Entry,
    lines: i32,
    padding: &Padding,
    legacy: bool,
) -> bool {
    let affcnt = if lines > 0 { lines } else { 1 };
    let mut putch = PutChar(out);
    let sent = match cap(strings, "clear") {
        Some(clear) => {
            terminfo::tputs_to(clear, affcnt, padding, &mut putch);
            true
        }
        None => false,
    };
    if !legacy && let Some(e3) = cap(entry, "E3") {
        terminfo::tputs_to(e3, affcnt, padding, &mut putch);
    }
    sent
}

/// `fgets (buf, size, stream)`: a line of at most `size - 1` bytes, its
/// newline kept; `None` at the end with nothing read, or after a read error.
pub fn fgets(stream: &mut crate::stdio::StdioReader, size: usize) -> Option<Vec<u8>> {
    let mut line = Vec::new();
    while line.len().saturating_add(1) < size {
        match stream.getc() {
            Ok(Some(b)) => {
                line.push(b);
                if b == b'\n' {
                    break;
                }
            }
            Ok(None) => break,
            // glibc's `fgets` answers NULL on an error, read bytes or not.
            Err(_) => return None,
        }
    }
    if line.is_empty() { None } else { Some(line) }
}

/// `setupterm`'s complaint as it prints it when it has nowhere to put its
/// verdict, but with the unprintable bytes of the terminal's name escaped:
/// what lies between the opening quote and the last `': `, since what
/// follows the name never holds one.
#[must_use]
pub fn setupterm_complaint(complaint: &[u8]) -> Vec<u8> {
    let Some(rest) = complaint.strip_prefix(b"'") else {
        return complaint.to_vec();
    };
    match rest.windows(3).rposition(|w| w == b"': ") {
        Some(end) => {
            let (name, tail) = rest.split_at(end);
            let mut m = b"'".to_vec();
            m.extend_from_slice(crate::quote::escape_unprintable(name).as_bytes());
            m.extend_from_slice(tail);
            m
        }
        None => complaint.to_vec(),
    }
}

/// `getopt`'s complaint, `argv[0]` first -- its unprintable bytes escaped, as
/// every diagnostic here escapes a name (design-decisions §370).
pub fn getopt_complaint(argv0: &[u8], sentence: &str) {
    let mut m = crate::quote::escape_unprintable(argv0).into_bytes();
    m.extend_from_slice(b": ");
    m.extend_from_slice(sentence.as_bytes());
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn rootname_is_the_last_component() {
        assert_eq!(rootname(b"/usr/bin/tput"), b"tput");
        assert_eq!(rootname(b"clear"), b"clear");
        assert_eq!(rootname(b"a/"), b"");
    }

    #[test]
    fn err_system_is_four_and_the_errno() {
        assert_eq!(err_system(0), 4);
        assert_eq!(err_system(6), 10);
        assert_eq!(err_system(300), 48);
    }

    #[test]
    fn a_short_wraps() {
        assert_eq!(as_short(24), 24);
        assert_eq!(as_short(32767), 32767);
        assert_eq!(as_short(40000), -25536);
        assert_eq!(as_short(65536 + 80), 80);
        assert_eq!(as_short(-1), -1);
    }

    #[test]
    fn the_legacy_copy_of_no_numbers_is_absent_numbers() {
        let l = Legacy::of(&Entry::default());
        assert_eq!((l.lines, l.columns, l.init_tabs), (-1, -1, -1));
    }

    #[test]
    fn the_control_characters_default_as_glibcs() {
        let entry = Entry::default();
        let mut t = Termios::default();
        // All disabled: each takes its default.
        set_control_chars(&mut t, &entry, -1, -1, -1);
        assert_eq!(t.c_cc[tc::VERASE], 0o177);
        assert_eq!(t.c_cc[tc::VINTR], 3);
        assert_eq!(t.c_cc[tc::VKILL], 0o25);
        // Asked for: taken, whatever was there, as its low byte; not asked
        // for and set: kept.
        set_control_chars(&mut t, &entry, 8, -1, 0x115);
        assert_eq!(t.c_cc[tc::VERASE], 8);
        assert_eq!(t.c_cc[tc::VINTR], 3);
        assert_eq!(t.c_cc[tc::VKILL], 0x15);
    }

    #[test]
    fn the_conversions_are_set() {
        let entry = Entry::default();
        let mut t = Termios::default();
        set_conversions(&mut t, &entry);
        assert_ne!(t.c_oflag & tc::ONLCR, 0);
        assert_ne!(t.c_iflag & tc::ICRNL, 0);
        assert_eq!(
            t.c_lflag & (tc::ECHO | tc::ECHOE | tc::ECHOK),
            tc::ECHO | tc::ECHOE | tc::ECHOK
        );
    }

    #[test]
    fn the_version_line_names_the_program() {
        assert_eq!(version_line(b"tput"), b"tput (SlateOS coreutils) 0.1.0\n");
    }
}
