//! lockfile -- procmail 3.24's `lockfile(1)`, ported.
//!
//! `lockfile a.lock` makes `a.lock` exist, atomically, waiting while another
//! process holds it, so shell scripts can take turns at something. This is
//! `src/lockfile.c` from procmail 3.24 as Debian and Ubuntu build it
//! (3.24-1ubuntu2), with what it calls from `exopen.c`, `acommon.c`,
//! `authenticate.c` and `mcommon.c`, function by function. It replaced an
//! approximation (known-issues TD-B-LOCKFILE-IS-NOT-PROCMAILS) whose `-l` was a
//! deadline rather than the age of a stale lock, whose bad numbers became
//! defaults, and whose messages and exit statuses were its own.
//! `scripts/lockfile-diff.sh` runs this and Ubuntu's lockfile side by side.
//!
//! # How a lock is taken
//!
//! Not by `open(O_EXCL)` on the lock's name. procmail creates a uniquely named
//! file beside it -- `_`, the pid, one of `.,+%`, the time, `.`, the host
//! name, the numbers in procmail's base 64 -- writes `0` into it (a pid of 0,
//! which "works across networks"), checks with `fstat` and `lstat` that the
//! file it opened is the one at that name, then hard-links it to the lock's
//! name and removes the temporary. The `link` fails if the lock exists, which
//! is the test; a `link` that reports failure but did succeed, as NFS can, is
//! caught by comparing the two names' inodes. So a lock file is mode 0444,
//! holds `0`, and has one link.
//!
//! # The second pass
//!
//! On any failure lockfile goes back over the arguments *before* the one that
//! failed: options are parsed again, and every file among them is removed --
//! the locks this run already took are released, so a script is never left
//! holding some of several. Its quirks are part of the interface and kept:
//! `-r x` prints the usage line twice, once per pass; `-!` inverts the status
//! only when it comes before the failing argument; a usage error is never
//! inverted; a missing directory gives up at once ("Try praying") whatever the
//! NFS comment beside that code says it does.
//!
//! # Where it differs, deliberately
//!
//! * `-v` for a caller with no passwd entry: procmail dereferences a null
//!   pointer and dies; this prints an empty mailbox name.
//! * Allocation failure, which procmail reports as "Out of memory": Rust
//!   aborts instead, so that message is never printed.
//! * `-l` when no temporary was ever made (every attempt at a unique name
//!   failed with EEXIST): procmail compares against an uninitialised time;
//!   this does not force.

// The host build stops at the `main` that refuses to run; everything the real
// one calls is then unreachable there.
#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(test)]
mod tests;

#[cfg(unix)]
mod unix;

/// `lockfile`'s answers to its caller: sysexits.h, as procmail returns them.
pub(crate) const EX_USAGE: i32 = 64;
pub(crate) const EX_UNAVAILABLE: i32 = 69;
pub(crate) const EX_OSERR: i32 = 71;
pub(crate) const EX_CANTCREAT: i32 = 73;
pub(crate) const EX_TEMPFAIL: i32 = 75;

/// The errno values the lock loop tells apart: Linux's numbers, which are
/// SlateOS's C library's too.
pub(crate) const ENOENT: i32 = 2;
pub(crate) const EIO: i32 = 5;
pub(crate) const EEXIST: i32 = 17;
pub(crate) const EXDEV: i32 = 18;
pub(crate) const ENOTDIR: i32 = 20;
pub(crate) const ENOSPC: i32 = 28;
pub(crate) const ENAMETOOLONG: i32 = 36;
pub(crate) const EDQUOT: i32 = 122;

/// config.h: seconds between attempts, and after forcing a stale lock.
const DEF_LOCKSLEEP: i32 = 8;
const DEF_SUSPEND: i32 = 16;
/// config.h `nfsTRY`: one more than the NFS errors tolerated.
const NFS_TRY: i32 = 7 + 1;
/// config.h `RETRYunique`: further attempts at an unused temporary name.
const RETRY_UNIQUE: i32 = 8;
/// exopen.h: room for the temporary's name, and how short ENAMETOOLONG cuts it.
pub(crate) const UNIQ_NAME_LEN: usize = 30;
const MIN_NAME_LEN: usize = 14;
/// config.h `LOCKperm` = `READperm`: a lock is read-only for everyone.
pub(crate) const LOCK_PERM: u32 = 0o444;
/// authenticate.c: the mail spool, as Debian builds it.
const MAILSPOOLDIR: &[u8] = b"/var/mail/";
const LOCKEXT: &[u8] = b".lock";
/// exopen.c `s2c`: the separators that tell four names in one second apart.
pub(crate) const S2C: &[u8; 4] = b".,+%";
/// acommon.c `ultoan`'s digits, in their collating order.
const DIGITS64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

const NAMEPREFIX: &[u8] = b"lockfile: ";
pub(crate) const USAGE: &[u8] =
    b"Usage: lockfile -v | -nnn | -r nnn | -l nnn | -s nnn | -! | -ml | -mu | file ...\n";
pub(crate) const HELP: &[u8] = b"\t-v\tdisplay the version number and exit\
\n\t-nnn\twait nnn seconds between locking attempts\
\n\t-r nnn\tmake at most nnn retries before giving up on a lock\
\n\t-l nnn\tset locktimeout to nnn seconds\
\n\t-s nnn\tsuspend nnn seconds after a locktimeout occurred\
\n\t-!\tinvert the exitcode of lockfile\
\n\t-ml\tlock your system mail-spool file\
\n\t-mu\tunlock your system mail-spool file\n";
/// patchlevel.h's VERSION, which begins with its space on purpose.
pub(crate) const VERSION: &[u8] = b" v3.24 2022/03/02\n    \
Copyright (c) 1990-2022, Stephen R. van den Berg\t<srb@cuci.nl>\n\
\n\
Submit questions/answers to the procmail-related mailinglist by sending to:\n\
\t<procmail-users@procmail.org>\n\
\n\
And of course, subscription and information requests for this list to:\n\
\t<procmail-users-request@procmail.org>\n";

/// What `stat` reports, as much of it as lockfile reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u64,
    pub size: u64,
    pub mtime: i64,
    pub mode: u32,
}

const S_IFMT: u32 = 0o170_000;
pub(crate) const S_IFLNK: u32 = 0o120_000;

/// Everything lockfile asks of the system. Each failing call returns its
/// errno, which [`Lockfile`] keeps the way C keeps `errno`: the last failure,
/// untouched by later successes -- procmail reads it after calls that did
/// not set it, and the port must read the same value.
pub(crate) trait System {
    type File;
    fn elog(&mut self, bytes: &[u8]);
    fn getuid(&mut self) -> u32;
    fn geteuid(&mut self) -> u32;
    fn setuid(&mut self, uid: u32) -> bool;
    fn getgid(&mut self) -> u32;
    fn getegid(&mut self) -> u32;
    fn setgid(&mut self, gid: u32) -> bool;
    fn getpid(&mut self) -> u64;
    fn time(&mut self) -> i64;
    fn nodename(&mut self) -> Vec<u8>;
    fn logname(&mut self) -> Option<Vec<u8>>;
    /// `(uid, login name)` of the passwd entry with this name.
    fn user_by_name(&mut self, name: &[u8]) -> Option<(u32, Vec<u8>)>;
    /// The login name of the passwd entry with this uid.
    fn user_by_uid(&mut self, uid: u32) -> Option<Vec<u8>>;
    fn lstat(&mut self, path: &[u8]) -> Result<Stat, i32>;
    fn stat(&mut self, path: &[u8]) -> Result<Stat, i32>;
    /// `open(path, O_WRONLY|O_CREAT|O_EXCL, mode)`.
    fn create_excl(&mut self, path: &[u8], mode: u32) -> Result<Self::File, i32>;
    fn fstat(&mut self, file: &Self::File) -> Result<Stat, i32>;
    fn write(&mut self, file: &mut Self::File, bytes: &[u8]);
    fn close(&mut self, file: Self::File);
    fn link(&mut self, old: &[u8], new: &[u8]) -> Result<(), i32>;
    fn unlink(&mut self, path: &[u8]) -> Result<(), i32>;
    /// C's `sleep`, which a caught signal cuts short.
    fn sleep(&mut self, seconds: u32);
    /// procmail's `exitflag`: 0, or 2 once a terminating signal has arrived.
    fn exitflag(&mut self) -> i32;
    /// `qsignal` for SIGHUP, SIGINT, SIGQUIT and SIGTERM.
    fn catch_signals(&mut self);
    fn ignore_sigpipe(&mut self);
}

/// A cursor into the options: an argument, and an offset in it. C walks a
/// `char*` across them; two cursors are equal when the pointers would be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Pos {
    arg: usize,
    off: usize,
}

impl Pos {
    fn plus(self, n: usize) -> Self {
        Self { arg: self.arg, off: self.off.saturating_add(n) }
    }
}

/// A file being locked: an argument, or the mailbox lock (`-ml`).
#[derive(Clone, Copy, Debug)]
enum Target {
    Arg(usize),
    Mailbox,
}

/// Where control goes after one option letter, as procmail's `goto`s send it.
enum Then {
    /// The next letter of the same argument.
    Next,
    /// `checkrdec:` -- a number had to be read here.
    CheckDigits,
    /// `eusg:` -- print the usage line, then as `Xusg`.
    Eusg,
    /// `xusg:` -- the status is a usage error; then the second pass.
    Xusg,
    /// `nfailure:` -- the second pass.
    Nfailure,
    /// The argument is finished.
    Done,
    /// `-ml` in the first pass: lock the mailbox.
    LockMailbox,
}

/// procmail's `main`'s locals and statics, and C's `errno`.
pub(crate) struct Lockfile<'s, S: System> {
    sys: &'s mut S,
    argv: Vec<Vec<u8>>,
    sleepsec: i32,
    retries: i32,
    invert: bool,
    force: i32,
    suspend: i32,
    retval: i32,
    virgin: bool,
    /// The mailbox's lock file, once `-m` has worked it out (`static char*ma`).
    ma: Option<Vec<u8>>,
    errno: i32,
    /// exopen.c's statics: the separator in use, and the second it is for.
    serial: usize,
    serial_time: i64,
    /// The last temporary's modification time, for `-l` (`time_t t`).
    t: Option<i64>,
}

/// `strtol(s, &end, 10)`: the value as an `int` keeps it, and how many bytes
/// the number took -- 0 when there was none, as `end == s` says in C.
pub(crate) fn strtol(s: &[u8]) -> (i32, usize) {
    let mut i = 0usize;
    while s.get(i).is_some_and(|&c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')) {
        i = i.saturating_add(1);
    }
    let negative = match s.get(i) {
        Some(b'-') => {
            i = i.saturating_add(1);
            true
        }
        Some(b'+') => {
            i = i.saturating_add(1);
            false
        }
        _ => false,
    };
    let start = i;
    let mut value: i64 = 0;
    let mut overflow = false;
    while let Some(&c) = s.get(i).filter(|c| c.is_ascii_digit()) {
        let digit = i64::from(c.wrapping_sub(b'0'));
        match value.checked_mul(10).and_then(|v| v.checked_add(digit)) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i = i.saturating_add(1);
    }
    if i == start {
        return (0, 0);
    }
    // strtol clamps to LONG_MIN/LONG_MAX; storing that long in an int keeps
    // its low 32 bits, as GCC's conversion does.
    let long = if overflow {
        if negative { i64::MIN } else { i64::MAX }
    } else if negative {
        value.wrapping_neg()
    } else {
        value
    };
    #[allow(clippy::cast_possible_truncation)]
    let int = long as i32;
    (int, i)
}

/// acommon.c `ultoan`: `val` in base 64, least significant digit first, then
/// a NUL. Returns the NUL's index.
pub(crate) fn ultoan(mut val: u64, buf: &mut [u8], mut at: usize) -> usize {
    loop {
        #[allow(clippy::cast_possible_truncation)]
        let digit = (val & 0x3f) as usize;
        put(buf, at, DIGITS64.get(digit).copied().unwrap_or(b'_'));
        at = at.saturating_add(1);
        val = val.checked_shr(6).unwrap_or(0);
        if val == 0 {
            break;
        }
    }
    put(buf, at, 0);
    at
}

/// exopen.c `safehost`: the host name with `/`, `:` and `\` -- which do not
/// belong in a file name -- each written as `\` and three octal digits.
pub(crate) fn safehost(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    for &c in name {
        if matches!(c, b'/' | b':' | b'\\') {
            out.extend_from_slice(&[
                b'\\',
                b'0'.wrapping_add(c >> 6),
                b'0'.wrapping_add((c >> 3) & 7),
                b'0'.wrapping_add(c & 7),
            ]);
        } else {
            out.push(c);
        }
    }
    out
}

/// lastdirsep.c: the index just past the last `/`, 0 if there is none.
pub(crate) fn lastdirsep(name: &[u8]) -> usize {
    name.iter().rposition(|&c| c == b'/').map_or(0, |i| i.saturating_add(1))
}

/// The C string in `buf`: everything before the first NUL.
pub(crate) fn cstr(buf: &[u8]) -> &[u8] {
    match buf.iter().position(|&c| c == 0) {
        Some(n) => buf.get(..n).unwrap_or(buf),
        None => buf,
    }
}

/// `buf[at] = byte`, where C's buffer is sized so it is always in bounds.
fn put(buf: &mut [u8], at: usize, byte: u8) {
    if let Some(slot) = buf.get_mut(at) {
        *slot = byte;
    }
}

/// `strlcpy(&buf[at..], src, buf.len() - at)`.
fn strlcpy_at(buf: &mut [u8], at: usize, src: &[u8]) {
    let room = buf.len().saturating_sub(at).saturating_sub(1);
    let n = src.len().min(room);
    if let (Some(dst), Some(from)) = (buf.get_mut(at..at.saturating_add(n)), src.get(..n)) {
        dst.copy_from_slice(from);
    }
    put(buf, at.saturating_add(n), 0);
}

impl<'s, S: System> Lockfile<'s, S> {
    pub(crate) fn new(sys: &'s mut S, argv: Vec<Vec<u8>>) -> Self {
        Self {
            sys,
            argv,
            sleepsec: DEF_LOCKSLEEP,
            retries: -1,
            invert: false,
            force: 0,
            suspend: DEF_SUSPEND,
            retval: 0,
            virgin: true,
            ma: None,
            errno: 0,
            serial: S2C.len(),
            serial_time: 0,
            t: None,
        }
    }

    fn elog(&mut self, bytes: &[u8]) {
        self.sys.elog(bytes);
    }

    fn nlog(&mut self, bytes: &[u8]) {
        self.sys.elog(NAMEPREFIX);
        self.sys.elog(bytes);
    }

    fn byte_at(&self, pos: Pos) -> u8 {
        self.argv.get(pos.arg).and_then(|a| a.get(pos.off)).copied().unwrap_or(0)
    }

    fn rest(&self, pos: Pos) -> &[u8] {
        self.argv.get(pos.arg).and_then(|a| a.get(pos.off..)).unwrap_or(&[])
    }

    fn target_name(&self, target: Target) -> Vec<u8> {
        match target {
            Target::Arg(p) => self.argv.get(p).cloned().unwrap_or_default(),
            Target::Mailbox => self.ma.clone().unwrap_or_default(),
        }
    }

    fn truncate_target(&mut self, target: Target, len: usize) {
        let name = match target {
            Target::Arg(p) => self.argv.get_mut(p),
            Target::Mailbox => self.ma.as_mut(),
        };
        if let Some(name) = name {
            name.truncate(len);
        }
    }

    /// authenticate.c `auth_mailboxname`: where a login's mail is spooled.
    fn mailbox_of(login: &[u8]) -> Vec<u8> {
        let mut m = MAILSPOOLDIR.to_vec();
        m.extend_from_slice(login);
        m
    }

    fn set_errno<T>(&mut self, result: Result<T, i32>) -> Result<T, i32> {
        if let Err(e) = result {
            self.errno = e;
        }
        result
    }

    /// procmail's `main`; returns the exit status.
    pub(crate) fn run(&mut self) -> i32 {
        let mut argc = self.argv.len();
        if argc <= 1 {
            self.elog(USAGE);
            return EX_USAGE;
        }
        self.sys.ignore_sigpipe();
        let uid = self.sys.getuid();
        if !self.sys.setuid(uid) || self.sys.geteuid() != uid {
            return self.cannot_drop_privileges();
        }

        'again: loop {
            self.invert = false;
            self.sys.catch_signals();
            let mut p = 0usize;
            'args: loop {
                argc = argc.saturating_sub(1);
                if argc == 0 {
                    break 'again;
                }
                p = p.saturating_add(1);
                if self.byte_at(Pos { arg: p, off: 0 }) == b'-' {
                    let mut cp = Pos { arg: p, off: 1 };
                    loop {
                        let cp2 = cp;
                        let c = self.byte_at(cp);
                        cp = cp.plus(1);
                        let then = match c {
                            b'!' => {
                                self.invert = !self.invert;
                                Then::Next
                            }
                            b'r' | b'l' | b's' => {
                                if self.byte_at(cp) == 0 {
                                    // The number is the next argument.
                                    p = p.saturating_add(1);
                                    argc = argc.saturating_sub(1);
                                    if argc == 0 {
                                        p = p.saturating_sub(1);
                                        self.elog(USAGE);
                                        self.retval = EX_USAGE;
                                        argc = self.second_pass(p);
                                        continue 'again;
                                    }
                                    cp = Pos { arg: p, off: 0 };
                                }
                                let (i, used) = strtol(self.rest(cp));
                                cp = cp.plus(used);
                                match c {
                                    b'r' => {
                                        self.retries = i;
                                        Then::CheckDigits
                                    }
                                    b'l' => {
                                        self.force = i;
                                        Then::CheckDigits
                                    }
                                    _ if i < 0 => Then::Eusg,
                                    _ => {
                                        self.suspend = i;
                                        Then::CheckDigits
                                    }
                                }
                            }
                            b'v' => {
                                self.version();
                                Then::Xusg
                            }
                            b'h' | b'?' => {
                                self.elog(USAGE);
                                self.elog(HELP);
                                Then::Xusg
                            }
                            b'm' => self.mail_option(cp, uid),
                            0 => Then::Done,
                            _ => {
                                let (v, used) = strtol(self.rest(cp2));
                                cp = cp2.plus(used);
                                if self.sleepsec >= 0 {
                                    self.sleepsec = v;
                                    if v < 0 { Then::Eusg } else { Then::CheckDigits }
                                } else {
                                    // The second pass reads it again, and
                                    // discards it.
                                    Then::Next
                                }
                            }
                        };
                        match then {
                            Then::Next => {}
                            Then::CheckDigits => {
                                if cp2 == cp {
                                    self.elog(USAGE);
                                    self.retval = EX_USAGE;
                                    argc = self.second_pass(p);
                                    continue 'again;
                                }
                            }
                            Then::Eusg => {
                                self.elog(USAGE);
                                self.retval = EX_USAGE;
                                argc = self.second_pass(p);
                                continue 'again;
                            }
                            Then::Xusg => {
                                self.retval = EX_USAGE;
                                argc = self.second_pass(p);
                                continue 'again;
                            }
                            Then::Nfailure => {
                                argc = self.second_pass(p);
                                continue 'again;
                            }
                            Then::Done => continue 'args,
                            Then::LockMailbox => {
                                if self.lock(Target::Mailbox) {
                                    continue 'args;
                                }
                                argc = self.second_pass(p);
                                continue 'again;
                            }
                        }
                    }
                } else if self.sleepsec < 0 {
                    // The second pass: release what the first one took.
                    let name = self.target_name(Target::Arg(p));
                    let removed = self.sys.unlink(&name);
                    let _ = self.set_errno(removed);
                } else {
                    let gid = self.sys.getgid();
                    if !self.sys.setgid(gid) || self.sys.getegid() != gid {
                        return self.cannot_drop_privileges();
                    }
                    if !self.lock(Target::Arg(p)) {
                        argc = self.second_pass(p);
                        continue 'again;
                    }
                }
            }
        }

        if self.retval == 0 && self.virgin {
            self.elog(USAGE);
            return EX_USAGE;
        }
        if self.invert {
            match self.retval {
                0 => return EX_CANTCREAT,
                EX_CANTCREAT => return 0,
                _ => {}
            }
        }
        self.retval
    }

    /// `sp:` -- the uid or gid could not be set back to the caller's own.
    fn cannot_drop_privileges(&mut self) -> i32 {
        self.nlog(b"Unable to give up special permissions");
        EX_OSERR
    }

    /// `nfailure:` -- the next pass covers the arguments before the current
    /// one (its argc is the current one's index), as a release pass.
    fn second_pass(&mut self, p: usize) -> usize {
        self.sleepsec = -1;
        p
    }

    /// `-v`: procmail's version, and the caller's mailbox lock.
    fn version(&mut self) {
        self.elog(b"lockfile");
        self.elog(VERSION);
        self.elog(b"\nYour system mailbox's lockfile:\t");
        let uid = self.sys.getuid();
        let mailbox = self.sys.user_by_uid(uid).map(|login| Self::mailbox_of(&login)).unwrap_or_default();
        self.elog(&mailbox);
        self.elog(LOCKEXT);
        self.elog(b"\n");
    }

    /// `-ml`, `-mu`: lock or unlock the caller's mailbox. `cp` is just past
    /// the `m`.
    fn mail_option(&mut self, cp: Pos, uid: u32) -> Then {
        let first = self.byte_at(cp);
        let second = self.byte_at(cp.plus(1));
        if (first != 0 && second != 0) || (self.ma.is_some() && self.sleepsec >= 0) {
            return Then::Eusg;
        }
        if self.ma.is_none() {
            // $LOGNAME is a hint, believed only when its passwd entry has the
            // caller's uid; otherwise the uid's own entry.
            let hinted = match self.sys.logname() {
                Some(name) => self.sys.user_by_name(&name),
                None => None,
            };
            let login = match hinted.filter(|(found, _)| *found == uid) {
                Some((_, login)) => Some(login),
                None => self.sys.user_by_uid(uid),
            };
            let Some(login) = login else {
                self.nlog(b"Can't determine your mailbox, who are you?\n");
                return Then::Nfailure;
            };
            let mut ma = Self::mailbox_of(&login);
            ma.extend_from_slice(LOCKEXT);
            self.ma = Some(ma);
        }
        match first {
            b'l' if self.sleepsec >= 0 => Then::LockMailbox,
            b'l' | b'u' => {
                let ma = self.ma.clone().unwrap_or_default();
                let removed = self.sys.unlink(&ma);
                if self.set_errno(removed).is_ok() {
                    self.virgin = false;
                } else {
                    self.nlog(b"Can't unlock \"");
                    self.elog(&ma);
                    self.elog(b"\"");
                    if first == b'l' {
                        self.elog(b" again,\n already dropped my privileges");
                    }
                    self.elog(b"\n");
                }
                Then::Done
            }
            _ => Then::Eusg,
        }
    }

    /// `stilv:` and the loop after it: take the lock on `target`, or say why
    /// not. False is `lfailure:` -- the reason and "giving up" are printed.
    fn lock(&mut self, target: Target) -> bool {
        self.virgin = false;
        let mut permanent = NFS_TRY;
        loop {
            if self.xcreat(target) {
                return true;
            }
            match self.sys.exitflag() {
                0 => {}
                1 => {
                    self.retval = EX_OSERR;
                    self.nlog(b"Out of memory");
                    return self.give_up(target);
                }
                _ => {
                    self.retval = EX_TEMPFAIL;
                    self.nlog(b"Signal received");
                    return self.give_up(target);
                }
            }
            match self.errno {
                EEXIST => {
                    if self.stale(target) {
                        self.force_unlock(target);
                    } else {
                        match self.retries {
                            0 => {
                                self.nlog(b"Sorry");
                                self.retval = EX_CANTCREAT;
                                return self.give_up(target);
                            }
                            -1 => self.sys.sleep(self.sleepsec.unsigned_abs()),
                            _ => {
                                self.retries = self.retries.wrapping_sub(1);
                                self.sys.sleep(self.sleepsec.unsigned_abs());
                            }
                        }
                    }
                }
                ENOSPC | EDQUOT | ENOENT | ENOTDIR | EIO => {
                    // procmail's comment says these are tolerated nfsTRY times;
                    // its code gives up on the first unless the count has run
                    // out. Kept as it behaves.
                    permanent = permanent.wrapping_sub(1);
                    if permanent == 0 {
                        self.sys.sleep(self.sleepsec.unsigned_abs());
                        continue;
                    }
                    self.nlog(b"Try praying");
                    self.retval = EX_UNAVAILABLE;
                    return self.give_up(target);
                }
                ENAMETOOLONG => {
                    let name = self.target_name(target);
                    let keep = name
                        .len()
                        .checked_sub(1)
                        .filter(|&n| n > 0 && name.get(n.saturating_sub(1)).is_some_and(|&c| c != b'/'));
                    let Some(n) = keep else {
                        self.nlog(b"Filename too long");
                        self.retval = EX_UNAVAILABLE;
                        return self.give_up(target);
                    };
                    self.nlog(b"Truncating \"");
                    self.elog(&name);
                    self.elog(b"\" and retrying lock\n");
                    self.truncate_target(target, n);
                }
                _ => {
                    self.nlog(b"Try praying");
                    self.retval = EX_UNAVAILABLE;
                    return self.give_up(target);
                }
            }
            permanent = NFS_TRY;
        }
    }

    /// `-l`: is the lock that is in the way older than the timeout?
    fn stale(&mut self, target: Target) -> bool {
        if self.force == 0 {
            return false;
        }
        let name = self.target_name(target);
        let found = self.sys.lstat(&name);
        match (self.set_errno(found), self.t) {
            (Ok(st), Some(t)) => i64::from(self.force) < t.saturating_sub(st.mtime),
            _ => false,
        }
    }

    /// Remove a stale lock, say so, and pause.
    fn force_unlock(&mut self, target: Target) {
        let name = self.target_name(target);
        let removed = self.sys.unlink(&name);
        let message: &[u8] = if self.set_errno(removed).is_ok() {
            b"Forcing lock on \""
        } else {
            b"Forced unlock denied on \""
        };
        self.nlog(message);
        self.elog(&name);
        self.elog(b"\"\n");
        self.sys.sleep(self.suspend.unsigned_abs());
    }

    /// `lfailure:` -- the end of the reason, and the file given up on.
    fn give_up(&mut self, target: Target) -> bool {
        let name = self.target_name(target);
        self.elog(b", giving up on \"");
        self.elog(&name);
        self.elog(b"\"\n");
        false
    }

    /// lockfile.c `xcreat`: one attempt at the lock.
    fn xcreat(&mut self, target: Target) -> bool {
        let name = self.target_name(target);
        let dir = lastdirsep(&name);
        let mut buf = vec![0u8; dir.saturating_add(UNIQ_NAME_LEN)];
        if let (Some(to), Some(from)) = (buf.get_mut(..dir), name.get(..dir)) {
            to.copy_from_slice(from);
        }
        if !self.unique(&mut buf, dir) {
            return false;
        }
        let temp = cstr(&buf).to_vec();
        let found = self.sys.stat(&temp);
        if let Ok(st) = self.set_errno(found) {
            self.t = Some(st.mtime);
        }
        self.myrename(&temp, &name)
    }

    /// exopen.c `unique`, as lockfile calls it (`doCHECK|doLOCK`): create a
    /// new, uniquely named file in `buf`'s directory, holding `0`.
    fn unique(&mut self, buf: &mut [u8], p: usize) -> bool {
        let end = buf.len();
        if end.saturating_sub(p) < UNIQ_NAME_LEN {
            return false;
        }
        put(buf, p, b'_');
        let pid = self.sys.getpid();
        let dot = ultoan(pid, buf, p.saturating_add(1));
        let mut retry = RETRY_UNIQUE;
        let mut build = self.serial < S2C.len();
        let opened = loop {
            if build || self.serial >= S2C.len() {
                if !build {
                    // Roll over: a second no earlier name was made in.
                    loop {
                        let now = self.sys.time();
                        if now != self.serial_time {
                            self.serial = 0;
                            self.serial_time = now;
                            break;
                        }
                        self.sys.sleep(1);
                    }
                }
                build = false;
                #[allow(clippy::cast_sign_loss)]
                let time = self.serial_time as u64;
                let host = ultoan(time, buf, dot.saturating_add(1)).saturating_add(1);
                put(buf, host.saturating_sub(1), b'.');
                let safe = safehost(&self.sys.nodename());
                strlcpy_at(buf, host, &safe);
            }
            put(buf, dot, S2C.get(self.serial).copied().unwrap_or(b'%'));
            self.serial = self.serial.saturating_add(1);

            let mut probe = self.sys.lstat(cstr(buf));
            if probe == Err(ENAMETOOLONG) {
                let mut op = lastdirsep(cstr(buf));
                let ldp = op.saturating_add(1);
                op = if end.saturating_sub(op) > MIN_NAME_LEN.saturating_add(1) {
                    op.saturating_add(MIN_NAME_LEN).saturating_add(1)
                } else {
                    end.saturating_sub(1)
                };
                loop {
                    op = op.saturating_sub(1);
                    put(buf, op, 0);
                    probe = self.sys.lstat(cstr(buf));
                    if !(probe == Err(ENAMETOOLONG) && op > ldp) {
                        break;
                    }
                }
            }
            let probe = self.set_errno(probe);

            // A name in use, or one that could not be looked at, or one
            // somebody created meanwhile: try the next. Otherwise this is the
            // answer, a file or a failure.
            let try_next = match probe {
                Ok(_) => true,
                Err(e) if e != ENOENT => true,
                Err(_) => {
                    let created = self.sys.create_excl(cstr(buf), LOCK_PERM);
                    match self.set_errno(created) {
                        Ok(file) => break Some(file),
                        Err(e) => e == EEXIST,
                    }
                }
            };
            if !try_next {
                break None;
            }
            let more = retry != 0;
            retry = retry.wrapping_sub(1);
            if !more {
                break None;
            }
        };
        let Some(mut file) = opened else {
            return false;
        };

        // doCHECK: the file opened must be the one at that name, alone and
        // empty, or somebody substituted it.
        let by_fd = self.sys.fstat(&file);
        let by_name = self.sys.lstat(cstr(buf));
        let substituted = match (self.set_errno(by_name), self.set_errno(by_fd)) {
            (Ok(n), Ok(f)) => {
                n.nlink != 1 || n.size != 0 || n.dev != f.dev || n.ino != f.ino || n.uid != f.uid || n.gid != f.gid
            }
            _ => true,
        };
        if substituted {
            self.sys.close(file);
            let removed = self.sys.unlink(cstr(buf));
            let _ = self.set_errno(removed);
            return false;
        }
        // doLOCK: a pid of 0, which "works across networks".
        self.sys.write(&mut file, b"0");
        self.sys.close(file);
        true
    }

    /// exopen.c `myrename`: link `old` to `new`, and remove `old` either way.
    fn myrename(&mut self, old: &[u8], new: &[u8]) -> bool {
        let linked = self.hlink(old, new);
        let saved = self.errno;
        let removed = self.sys.unlink(old);
        let _ = self.set_errno(removed);
        let ok = match linked {
            Ok(Some(file)) => {
                self.sys.close(file);
                true
            }
            Ok(None) => true,
            Err(()) => false,
        };
        self.errno = saved;
        ok
    }

    /// exopen.c `hlink`: a hard link, or -- where the filesystem has none --
    /// an exclusive create of `new`, which is then left empty.
    fn hlink(&mut self, old: &[u8], new: &[u8]) -> Result<Option<S::File>, ()> {
        match self.rlink(old, new) {
            Rlink::Linked => Ok(None),
            Rlink::Failed => Err(()),
            Rlink::Refused(sto) => {
                if sto.nlink < 2 && self.errno == EXDEV {
                    let created = self.sys.create_excl(new, sto.mode);
                    if let Ok(file) = self.set_errno(created) {
                        return Ok(Some(file));
                    }
                }
                Err(())
            }
        }
    }

    /// exopen.c `rlink`: `link`, and when it reports failure, whether it
    /// failed -- NFS can report failure for a link it made.
    fn rlink(&mut self, old: &[u8], new: &[u8]) -> Rlink {
        let Err(serrno) = self.sys.link(old, new) else {
            return Rlink::Linked;
        };
        self.errno = serrno;
        let sto = match self.sys.lstat(old) {
            Ok(st) => st,
            Err(_) => {
                self.errno = serrno;
                return Rlink::Failed;
            }
        };
        let same = match self.sys.lstat(new) {
            Ok(stn) => {
                stn.dev == sto.dev
                    && stn.ino == sto.ino
                    && stn.uid == sto.uid
                    && stn.gid == sto.gid
                    && sto.mode & S_IFMT != S_IFLNK
            }
            Err(_) => false,
        };
        if same {
            Rlink::Linked
        } else {
            self.errno = serrno;
            Rlink::Refused(sto)
        }
    }
}

/// What `rlink` concluded.
enum Rlink {
    Linked,
    Failed,
    /// The link failed and the files differ; `old`'s stat, for `hlink`.
    Refused(Stat),
}

#[cfg(unix)]
fn main() {
    use std::os::unix::ffi::OsStringExt;
    let argv: Vec<Vec<u8>> = std::env::args_os().map(OsStringExt::into_vec).collect();
    let mut sys = unix::Unix::new();
    let status = Lockfile::new(&mut sys, argv).run();
    std::process::exit(status);
}

#[cfg(not(unix))]
fn main() {
    eprintln!("lockfile: this build runs on SlateOS and Unix hosts only");
    std::process::exit(EX_UNAVAILABLE);
}
