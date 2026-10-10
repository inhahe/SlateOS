//! `lastlog` -- report the most recent login of every user, or of some:
//! shadow-utils 4.13's, ported.
//!
//! ```text
//! lastlog [options]
//! ```
//!
//! A transcription of `src/lastlog.c` as Ubuntu 24.04 builds it, none of
//! whose patches touch it. `/var/log/lastlog` is an array of 292-byte records
//! indexed by uid -- a 32-bit time, a 32-byte line and a 256-byte host, as
//! `login` writes them -- and this reads the record of every account in
//! `/etc/passwd`, in the file's order, or of the ones `-u` names. What
//! upstream does and this keeps:
//!
//! - **The listing.** A header before the first line and not at all when
//!   nothing is listed; `%-16s %-8.8s %-42s` and the time as
//!   `%a %b %e %H:%M:%S %z %Y` in the local zone, or `**Never logged in**`
//!   for a record whose time is zero -- one past the file's end included, as
//!   the sparse file reads.
//! - **`-u`**: a login name, else a uid or a range (`N`, `N-`, `-N`, `N-M`),
//!   as `getrange` reads one; a single uid that is nobody's lists nothing.
//!   **`-t DAYS`** and **`-b DAYS`**: only records newer, or older, than that
//!   many days, each read by `getulong` -- `strtoul` with base 0, so `0x10`
//!   is sixteen days and `010` eight.
//! - **`LASTLOG_UID_MAX`** from `/etc/login.defs`, read as shadow-utils reads
//!   the file -- `fgets` into 1024 bytes, the first two fields, quotes
//!   trimmed -- with its complaints: an item it does not know, and a value it
//!   cannot parse (that one without a newline, as upstream prints it). Uids
//!   above it are skipped, compared as a `uid_t`, and a `-u` above it is
//!   warned about.
//! - **`-S` and `-C` with `-u`** set each named record to now (line
//!   `lastlog`, host `localhost`) or clear it, through a stdio stream opened
//!   `r+`, then `fflush` and `fsync`. A seek that fails there fails
//!   upstream's `assert`, and so it does here.
//! - **`-R DIR`** chroots before anything else is read: every argument is
//!   scanned for `-R`, `--root` and `--root=`, as `process_root_flag` scans
//!   them -- so `-RDIR`, which `getopt` accepts, changes no root at all.
//! - **Standard output is stdio's**: written at exit, and a failure to write
//!   it unreported, as upstream never checks it.
//!
//! # Deliberate differences
//!
//! - **No audit records.** Ubuntu builds `lastlog` with libaudit, and `-S` and
//!   `-C` send an `ACCT_UNLOCK` record; SlateOS has no audit subsystem.
//! - **A host that fills its 256 bytes with no terminator** is printed to
//!   the end of the field; upstream's `%s` reads on past it.
//! - The privilege drop before `-R` is `setgid` and `setuid` of the real ids
//!   where upstream calls `setregid` and `setreuid`: the same drop for a
//!   program that is not set-id, which is the only way it is installed here.

use std::ffi::{CString, OsString};
use std::io::SeekFrom;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd;
use coreutils::stdio::{StdioFile, StdioReader};

coreutils::guard_std_fds!();

/// The parser. glibc's getopt names the program by `argv[0]` in its own
/// complaints, and shadow-utils by `argv[0]`'s last component.
const LASTLOG: Program = Program::new("lastlog", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "b:ChR:St:u:";

/// Upstream's `longopts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("before", Takes::Required),
    ("clear", Takes::Nothing),
    ("help", Takes::Nothing),
    ("root", Takes::Required),
    ("set", Takes::Nothing),
    ("time", Takes::Required),
    ("user", Takes::Required),
];

/// `LASTLOG_FILE`.
const LASTLOG_FILE: &[u8] = b"/var/log/lastlog";
/// `LOGINDEFS`.
const LOGIN_DEFS: &str = "/etc/login.defs";

/// `sizeof (struct lastlog)`: `int32_t ll_time`, `char ll_line[32]`, `char
/// ll_host[256]`.
const LL_SIZE: usize = 292;
const LL_SIZE_I64: i64 = 292;
const LL_SIZE_U64: u64 = 292;
/// `ll_line` and `ll_host` within a record.
const LL_LINE: std::ops::Range<usize> = 4..36;
const LL_HOST: std::ops::Range<usize> = 36..292;
/// What `-S` writes into them, `strcpy`'s way.
const SET_LINE: std::ops::Range<usize> = 4..11;
const SET_HOST: std::ops::Range<usize> = 36..45;

/// `DAY`.
const DAY: i64 = 86_400;

/// `25 + 1 + IFNAMSIZ`: the widest link-local IPv6 address and its interface,
/// the host column's width.
const MAX_IPV6_ADDRLEN: usize = 42;
/// The header's `%*s` before `Latest`: `maxIPv6Addrlen - 3`.
const HEADER_PAD: usize = 39;

/// `E_BAD_ARG`: an option's argument refused.
const E_BAD_ARG: u8 = 3;

/// shadow-utils' `def_table`, as Ubuntu's configuration builds it (SHA and
/// yescrypt, syslog; no bcrypt, no TCB): the items `login.defs` may set.
const DEF_TABLE: &[&str] = &[
    "CHFN_RESTRICT",
    "CONSOLE_GROUPS",
    "CONSOLE",
    "CREATE_HOME",
    "DEFAULT_HOME",
    "ENCRYPT_METHOD",
    "ENV_PATH",
    "ENV_SUPATH",
    "ERASECHAR",
    "FAIL_DELAY",
    "FAKE_SHELL",
    "GID_MAX",
    "GID_MIN",
    "HOME_MODE",
    "HUSHLOGIN_FILE",
    "KILLCHAR",
    "LASTLOG_UID_MAX",
    "LOGIN_RETRIES",
    "LOGIN_TIMEOUT",
    "LOG_OK_LOGINS",
    "LOG_UNKFAIL_ENAB",
    "MAIL_DIR",
    "MAIL_FILE",
    "MAX_MEMBERS_PER_GROUP",
    "MD5_CRYPT_ENAB",
    "NONEXISTENT",
    "PASS_MAX_DAYS",
    "PASS_MIN_DAYS",
    "PASS_WARN_AGE",
    "SHA_CRYPT_MAX_ROUNDS",
    "SHA_CRYPT_MIN_ROUNDS",
    "YESCRYPT_COST_FACTOR",
    "SUB_GID_COUNT",
    "SUB_GID_MAX",
    "SUB_GID_MIN",
    "SUB_UID_COUNT",
    "SUB_UID_MAX",
    "SUB_UID_MIN",
    "SULOG_FILE",
    "SU_NAME",
    "SYS_GID_MAX",
    "SYS_GID_MIN",
    "SYS_UID_MAX",
    "SYS_UID_MIN",
    "TTYGROUP",
    "TTYPERM",
    "TTYTYPE_FILE",
    "UID_MAX",
    "UID_MIN",
    "UMASK",
    "USERDEL_CMD",
    "USERGROUPS_ENAB",
    "SYSLOG_SG_ENAB",
    "SYSLOG_SU_ENAB",
    "FORCE_SHADOW",
    "GRANT_AUX_GROUP_SUBIDS",
    "PREVENT_NO_AUTH",
];

/// `knowndef_table`: items PAM and other packages read from the same file,
/// accepted and ignored.
const KNOWN_DEFS: &[&str] = &[
    "CHFN_AUTH",
    "CHSH_AUTH",
    "CRACKLIB_DICTPATH",
    "ENV_HZ",
    "ENVIRON_FILE",
    "ENV_TZ",
    "FAILLOG_ENAB",
    "FTMP_FILE",
    "HMAC_CRYPTO_ALGO",
    "ISSUE_FILE",
    "LASTLOG_ENAB",
    "LOGIN_STRING",
    "MAIL_CHECK_ENAB",
    "MOTD_FILE",
    "NOLOGINS_FILE",
    "OBSCURE_CHECKS_ENAB",
    "PASS_ALWAYS_WARN",
    "PASS_CHANGE_TRIES",
    "PASS_MAX_LEN",
    "PASS_MIN_LEN",
    "PORTTIME_CHECKS_ENAB",
    "QUOTAS_ENAB",
    "SU_WHEEL_ONLY",
    "ULIMIT",
    "ALWAYS_SET_PATH",
    "ENV_ROOTPATH",
    "LOGIN_KEEP_USERNAME",
    "LOGIN_PLAIN_PROMPT",
    "MOTD_FIRSTONLY",
];

/// A run that has ended with this status, its diagnostic already out.
struct Die(u8);

/// `fprintf (stderr, ...)`: unbuffered, standard output left as it is.
fn say(m: &[u8]) {
    // Nothing to do about a diagnostic that cannot be written; upstream
    // does not look either.
    drop(stdfd::write_all(2, m));
}

/// `"%s: " MESSAGE "\n"` under the program's name.
fn complain(prog: &[u8], parts: &[&[u8]]) {
    let mut m = prog.to_vec();
    m.extend_from_slice(b": ");
    for p in parts {
        m.extend_from_slice(p);
    }
    m.push(b'\n');
    say(&m);
}

/// `usage (status)`: to standard output for `-h`, else to standard error.
fn usage_text(prog: &[u8]) -> Vec<u8> {
    let mut s = b"Usage: ".to_vec();
    s.extend_from_slice(prog);
    s.extend_from_slice(b" [options]\n\nOptions:\n");
    s.extend_from_slice(
        b"  -b, --before DAYS             print only lastlog records older than DAYS\n",
    );
    s.extend_from_slice(
        b"  -C, --clear                   clear lastlog record of an user (usable only with -u)\n",
    );
    s.extend_from_slice(b"  -h, --help                    display this help message and exit\n");
    s.extend_from_slice(b"  -R, --root CHROOT_DIR         directory to chroot into\n");
    s.extend_from_slice(
        b"  -S, --set                     set lastlog record to current time (usable only with -u)\n",
    );
    s.extend_from_slice(
        b"  -t, --time DAYS               print only lastlog records more recent than DAYS\n",
    );
    s.extend_from_slice(
        b"  -u, --user LOGIN              print lastlog record of the specified LOGIN\n",
    );
    s.extend_from_slice(b"\n");
    s
}

/// `usage (EXIT_FAILURE)`.
fn usage_error(prog: &[u8]) -> Die {
    say(&usage_text(prog));
    Die(1)
}

/// `strerror (errno)` for an `io::Error`.
fn reason(e: &std::io::Error) -> Vec<u8> {
    coreutils::errmsg::strerror(e).into_bytes()
}

/// `process_root_flag ("-R", argc, argv)`: every argument, the program's
/// name included, scanned for `--root`, `--root=DIR` and `-R` -- whole
/// words only -- and the root changed to the one found.
fn process_root_flag(prog: &[u8], argv: &[Vec<u8>]) -> Result<(), Die> {
    let mut newroot: Option<&[u8]> = None;
    let mut i = 0usize;
    while let Some(arg) = argv.get(i) {
        let val = arg.strip_prefix(b"--root=");
        if arg.as_slice() == b"--root" || val.is_some() || arg.as_slice() == b"-R" {
            if newroot.is_some() {
                complain(prog, &[b"multiple --root options"]);
                return Err(Die(E_BAD_ARG));
            }
            if let Some(v) = val {
                newroot = Some(v);
            } else if let Some(next) = argv.get(i.saturating_add(1)) {
                newroot = Some(next);
                i = i.saturating_add(1);
            } else {
                complain(prog, &[b"option '", arg, b"' requires an argument"]);
                return Err(Die(E_BAD_ARG));
            }
        }
        i = i.saturating_add(1);
    }
    match newroot {
        Some(root) => change_root(prog, root),
        None => Ok(()),
    }
}

/// `change_root`: privileges dropped, then the checks and the `chroot`.
fn change_root(prog: &[u8], newroot: &[u8]) -> Result<(), Die> {
    #[cfg(unix)]
    {
        let ids = coreutils::grouplist::current_ids();
        let dropped =
            libcall::process::set_gid(ids.rgid).and_then(|()| libcall::process::set_uid(ids.ruid));
        if let Err(errno) = dropped {
            let e = std::io::Error::from_raw_os_error(errno);
            complain(prog, &[b"failed to drop privileges (", &reason(&e), b")"]);
            return Err(Die(1));
        }
    }
    if newroot.first() != Some(&b'/') {
        complain(
            prog,
            &[
                b"invalid chroot path '",
                newroot,
                b"', only absolute paths are supported.",
            ],
        );
        return Err(Die(E_BAD_ARG));
    }
    let path = os_from_bytes(newroot);
    // `access (newroot, F_OK)`: whether it exists at all.
    if let Err(e) = std::fs::metadata(&path) {
        complain(
            prog,
            &[
                b"cannot access chroot directory ",
                newroot,
                b": ",
                &reason(&e),
            ],
        );
        return Err(Die(E_BAD_ARG));
    }
    if let Err(e) = std::env::set_current_dir(&path) {
        complain(
            prog,
            &[
                b"cannot chdir to chroot directory ",
                newroot,
                b": ",
                &reason(&e),
            ],
        );
        return Err(Die(E_BAD_ARG));
    }
    let chrooted = CString::new(newroot.to_vec())
        .map_err(|_| 2)
        .and_then(|c| libcall::process::change_root(&c));
    if let Err(errno) = chrooted {
        let e = std::io::Error::from_raw_os_error(errno);
        complain(
            prog,
            &[
                b"unable to chroot to directory ",
                newroot,
                b": ",
                &reason(&e),
            ],
        );
        return Err(Die(E_BAD_ARG));
    }
    Ok(())
}

/// `getulong`: `strtoul (numstr, &end, 0)`, the whole of it, in range.
fn getulong(s: &[u8]) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let (v, used, overflow) = cstrtol::strtoull(s, 0);
    (used == s.len() && !overflow).then_some(v)
}

/// What `-u` selects: `umin`, `umax`, and whether each was given.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Range {
    min: Option<u64>,
    max: Option<u64>,
}

/// `strtoul (s, &end, 10)`: the value, how much was used, and `ERANGE`.
fn strtoul10(s: &[u8]) -> (u64, usize, bool) {
    cstrtol::strtoull(s, 10)
}

/// `getrange`: `-N`, `N`, `N-` or `N-M`.
fn getrange(range: &[u8]) -> Option<Range> {
    if range.first() == Some(&b'-') {
        if !range.get(1).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        let rest = range.get(1..).unwrap_or_default();
        let (n, used, overflow) = strtoul10(rest);
        if used != rest.len() || overflow {
            return None;
        }
        return Some(Range {
            min: None,
            max: Some(n),
        });
    }
    let (n, used, overflow) = strtoul10(range);
    if overflow {
        return None;
    }
    match range.get(used) {
        None => Some(Range {
            min: Some(n),
            max: Some(n),
        }),
        Some(b'-') => {
            let rest = range.get(used.saturating_add(1)..).unwrap_or_default();
            if rest.is_empty() {
                Some(Range {
                    min: Some(n),
                    max: None,
                })
            } else if !rest.first().is_some_and(u8::is_ascii_digit) {
                None
            } else {
                let (m, used2, overflow2) = strtoul10(rest);
                if used2 != rest.len() || overflow2 {
                    return None;
                }
                Some(Range {
                    min: Some(n),
                    max: Some(m),
                })
            }
        }
        Some(_) => None,
    }
}

/// `(uid_t) n`: an `unsigned long` cut to 32 bits.
fn as_uid(n: u64) -> u32 {
    u32::try_from(n & 0xffff_ffff).unwrap_or(u32::MAX)
}

/// `isspace` in the C locale, which is a UTF-8 one's for single bytes.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | 0x09..=0x0d)
}

/// `def_load` and `getdef_ulong ("LASTLOG_UID_MAX", 0xFFFFFFFF)`: the
/// login.defs file read once, its complaints printed, and the one value
/// this program asks for. `Err` where the file cannot be opened or read for
/// a reason other than its absence -- upstream logs that to syslog and exits
/// 1 without a word.
fn lastlog_uid_max() -> Result<u64, Die> {
    const DEFAULT: u64 = 0xFFFF_FFFF;
    let file = match std::fs::File::open(LOGIN_DEFS) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(DEFAULT),
        Err(_) => return Err(Die(1)),
    };
    let mut fp = StdioReader::from_file(file);
    let mut value: Option<Vec<u8>> = None;
    // `fgets (buf, 1024, fp)`: a line, or its first 1023 bytes.
    loop {
        let mut line = Vec::new();
        let mut eof = false;
        while line.len() < 1023 {
            match fp.getc() {
                Ok(Some(c)) => {
                    line.push(c);
                    if c == b'\n' {
                        break;
                    }
                }
                Ok(None) => {
                    eof = true;
                    break;
                }
                Err(_) => return Err(Die(1)),
            }
        }
        if line.is_empty() && eof {
            break;
        }
        if let Some((name, val)) = defs_line(&line) {
            if DEF_TABLE.iter().any(|d| d.as_bytes() == name) {
                if name == b"LASTLOG_UID_MAX" {
                    value = Some(val.to_vec());
                }
            } else if !KNOWN_DEFS.iter().any(|d| d.as_bytes() == name) {
                let mut m = b"configuration error - unknown item '".to_vec();
                m.extend_from_slice(name);
                m.extend_from_slice(b"' (notify administrator)\n");
                say(&m);
            }
        }
        if eof {
            break;
        }
    }
    // `fclose`, unchecked: the file was only read.
    drop(fp.close());
    let Some(v) = value else {
        return Ok(DEFAULT);
    };
    match getulong(&v) {
        Some(n) => Ok(n),
        None => {
            // Upstream's message has no newline.
            let mut m = b"configuration error - cannot parse LASTLOG_UID_MAX value: '".to_vec();
            m.extend_from_slice(&v);
            m.push(b'\'');
            say(&m);
            Ok(DEFAULT)
        }
    }
}

/// One `fgets` line of login.defs, as `def_load` splits it: the name and the
/// value, or `None` for a comment, a blank line or a single field.
fn defs_line(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    // `strlen`, then the trailing white space trimmed.
    let buf = buf.get(..buf.iter().position(|&b| b == 0).unwrap_or(buf.len()))?;
    let end = buf
        .iter()
        .rposition(|&c| !is_space(c))
        .map_or(0, |i| i.saturating_add(1));
    let buf = buf.get(..end)?;
    let start = buf.iter().position(|&c| c != b' ' && c != b'\t')?;
    let name_and_rest = buf.get(start..)?;
    if name_and_rest.first() == Some(&b'#') {
        return None;
    }
    let name_len = name_and_rest
        .iter()
        .position(|&c| c == b' ' || c == b'\t')?;
    let name = name_and_rest.get(..name_len)?;
    let rest = name_and_rest.get(name_len.saturating_add(1)..)?;
    let skip = rest
        .iter()
        .position(|&c| c != b' ' && c != b'"' && c != b'\t')
        .unwrap_or(rest.len());
    let value = rest.get(skip..)?;
    let value = value.get(..value.iter().position(|&c| c == b'"').unwrap_or(value.len()))?;
    Some((name, value))
}

/// `time (NULL)`.
fn now() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => i64::try_from(e.duration().as_secs())
            .unwrap_or(i64::MAX)
            .saturating_neg(),
    }
}

/// `strftime ("%a %b %e %H:%M:%S %z %Y")` of `localtime (t)`.
fn ptime(zone: &localtime::Zone, t: i64) -> Vec<u8> {
    let tm = zone.localtime(t, 0);
    let wday = usize::try_from(tm.wday)
        .ok()
        .and_then(|w| localtime::WDAY_ABBR.get(w))
        .copied()
        .unwrap_or(b"?");
    let mon = usize::try_from(tm.month)
        .ok()
        .and_then(|m| m.checked_sub(1))
        .and_then(|m| localtime::MON_ABBR.get(m))
        .copied()
        .unwrap_or(b"?");
    let sign = if tm.gmtoff < 0 { '-' } else { '+' };
    let minutes = tm.gmtoff.unsigned_abs() / 60;
    let mut s = wday.to_vec();
    s.push(b' ');
    s.extend_from_slice(mon);
    s.extend_from_slice(
        format!(
            " {:2} {:02}:{:02}:{:02} {sign}{:02}{:02} {}",
            tm.day,
            tm.hour,
            tm.minute,
            tm.second,
            minutes / 60,
            minutes % 60,
            tm.year
        )
        .as_bytes(),
    );
    s
}

/// The C string at the start of `s`.
fn c_str(s: &[u8]) -> &[u8] {
    s.get(..s.iter().position(|&b| b == 0).unwrap_or(s.len()))
        .unwrap_or_default()
}

/// glibc's `assert` failing: `PROG: FILE:LINE: FUNCTION: Assertion `EXPR'
/// failed.`, then `abort`.
fn assertion_failed(prog: &[u8], line: u32, function: &str) -> ! {
    let mut m = prog.to_vec();
    m.extend_from_slice(
        format!(": lastlog.c:{line}: {function}: Assertion `0 == err' failed.\n").as_bytes(),
    );
    say(&m);
    std::process::abort()
}

/// The run's state: upstream's file-scope statics.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's flags, one each, kept as it has them"
)]
struct Lastlog<'o> {
    prog: Vec<u8>,
    zone: localtime::Zone,
    db: pwdb::Db,
    range: Range,
    uflg: bool,
    tflg: bool,
    bflg: bool,
    cflg: bool,
    sflg: bool,
    seconds: i64,
    inverse_seconds: i64,
    /// `statbuf.st_size`.
    size: u64,
    /// Whether the header has been printed: `print_one`'s `once`.
    once: bool,
    out: &'o mut ulclosestream::Stdout,
}

impl Lastlog<'_> {
    /// `print_one`.
    fn print_one(&mut self, fp: &mut StdioReader, user: Option<(Vec<u8>, u32)>) -> Result<(), Die> {
        let Some((name, uid)) = user else {
            return Ok(());
        };
        let offset = i64::from(uid).saturating_mul(LL_SIZE_I64);
        let mut ll = [0u8; LL_SIZE];
        let end = u64::try_from(offset)
            .unwrap_or(u64::MAX)
            .saturating_add(LL_SIZE_U64);
        if end <= self.size {
            // "fseeko errors are not really relevant for us."
            if fp
                .seek(SeekFrom::Start(u64::try_from(offset).unwrap_or(0)))
                .is_err()
            {
                assertion_failed(&self.prog, 112, "print_one");
            }
            let (got, _) = fp.fread(&mut ll);
            if got != LL_SIZE {
                complain(
                    &self.prog,
                    &[format!("Failed to get the entry for UID {uid}").as_bytes()],
                );
                return Err(Die(1));
            }
        }
        let mut t = [0u8; 4];
        t.copy_from_slice(ll.get(..4).unwrap_or(&[0; 4]));
        let ll_time = i64::from(i32::from_le_bytes(t));

        if self.tflg && now().wrapping_sub(ll_time) > self.seconds {
            return Ok(());
        }
        if self.bflg && now().wrapping_sub(ll_time) < self.inverse_seconds {
            return Ok(());
        }

        if !self.once {
            let mut header = b"Username         Port     From".to_vec();
            header.resize(header.len().saturating_add(HEADER_PAD), b' ');
            header.extend_from_slice(b"Latest\n");
            self.out.write(&header);
            self.once = true;
        }

        let cp = if ll_time == 0 {
            b"**Never logged in**".to_vec()
        } else {
            ptime(&self.zone, ll_time)
        };
        // `"%-16s %-8.8s %*s%s\n"`, the host left-justified in 42.
        let mut line = name.clone();
        line.resize(line.len().max(16), b' ');
        line.push(b' ');
        let port = c_str(ll.get(LL_LINE).unwrap_or_default());
        let port = port.get(..port.len().min(8)).unwrap_or(port);
        line.extend_from_slice(port);
        line.resize(
            line.len().saturating_add(8usize.saturating_sub(port.len())),
            b' ',
        );
        line.push(b' ');
        let host = c_str(ll.get(LL_HOST).unwrap_or_default());
        line.extend_from_slice(host);
        line.resize(
            line.len()
                .saturating_add(MAX_IPV6_ADDRLEN.saturating_sub(host.len())),
            b' ',
        );
        line.extend_from_slice(&cp);
        line.push(b'\n');
        self.out.write(&line);
        Ok(())
    }

    /// `print`.
    fn print(&mut self, fp: &mut StdioReader) -> Result<(), Die> {
        let uid_max = lastlog_uid_max()?;
        if self.range.min.is_some_and(|m| m > uid_max)
            || self.range.max.is_some_and(|m| m > uid_max)
        {
            let mut m = self.prog.clone();
            m.extend_from_slice(
                format!(
                    ": Selected uid(s) are higher than LASTLOG_UID_MAX ({uid_max}),\n\tthe output might be incorrect.\n"
                )
                .as_bytes(),
            );
            say(&m);
        }
        if let (true, Some(min), Some(max)) = (self.uflg, self.range.min, self.range.max)
            && min == max
        {
            let user = self
                .db
                .user_by_uid(as_uid(min))
                .map(|u| (u.name.clone(), u.uid));
            return self.print_one(fp, user);
        }
        let users: Vec<(Vec<u8>, u32)> = self
            .db
            .all_users()
            .iter()
            .map(|u| (u.name.clone(), u.uid))
            .collect();
        for (name, uid) in users {
            if self.uflg {
                if self.range.min.is_some_and(|m| uid < as_uid(m))
                    || self.range.max.is_some_and(|m| uid > as_uid(m))
                {
                    continue;
                }
            } else if uid > as_uid(uid_max) {
                continue;
            }
            self.print_one(fp, Some((name, uid)))?;
        }
        Ok(())
    }

    /// `update_one`.
    fn update_one(&self, fp: &mut StdioFile, user: Option<(Vec<u8>, u32)>) -> Result<(), Die> {
        let Some((_, uid)) = user else {
            return Ok(());
        };
        let offset = u64::from(uid).saturating_mul(LL_SIZE_U64);
        // "fseeko errors are not really relevant for us."
        if fp.seek(SeekFrom::Start(offset)).is_err() {
            assertion_failed(&self.prog, 214, "update_one");
        }
        let mut ll = [0u8; LL_SIZE];
        if self.sflg {
            let t = i32::try_from(now()).unwrap_or(i32::MAX);
            if let Some(f) = ll.get_mut(..4) {
                f.copy_from_slice(&t.to_le_bytes());
            }
            if let Some(f) = ll.get_mut(SET_HOST) {
                f.copy_from_slice(b"localhost");
            }
            if let Some(f) = ll.get_mut(SET_LINE) {
                f.copy_from_slice(b"lastlog");
            }
        }
        if fp.write(&ll).is_err() {
            complain(
                &self.prog,
                &[format!("Failed to update the entry for UID {uid}").as_bytes()],
            );
            return Err(Die(1));
        }
        Ok(())
    }

    /// `update`.
    fn update(&mut self, fp: &mut StdioFile) -> Result<(), Die> {
        if !self.uflg {
            return Ok(());
        }
        let uid_max = lastlog_uid_max()?;
        if self.range.min.is_some_and(|m| m > uid_max)
            || self.range.max.is_some_and(|m| m > uid_max)
        {
            let mut m = self.prog.clone();
            m.extend_from_slice(
                format!(
                    ": Selected uid(s) are higher than LASTLOG_UID_MAX ({uid_max}),\n\tthey will not be updated.\n"
                )
                .as_bytes(),
            );
            say(&m);
            return Ok(());
        }
        if let (Some(min), Some(max)) = (self.range.min, self.range.max)
            && min == max
        {
            let user = self
                .db
                .user_by_uid(as_uid(min))
                .map(|u| (u.name.clone(), u.uid));
            self.update_one(fp, user)?;
        } else {
            let users: Vec<(Vec<u8>, u32)> = self
                .db
                .all_users()
                .iter()
                .map(|u| (u.name.clone(), u.uid))
                .collect();
            for (name, uid) in users {
                if self.range.min.is_some_and(|m| uid < as_uid(m))
                    || self.range.max.is_some_and(|m| uid > as_uid(m))
                {
                    continue;
                }
                self.update_one(fp, Some((name, uid)))?;
            }
        }
        let synced = fp.flush().and_then(|()| match fp.file() {
            Some(f) => f.sync_all(),
            None => Ok(()),
        });
        if synced.is_err() {
            complain(&self.prog, &[b"Failed to update the lastlog file"]);
            return Err(Die(1));
        }
        Ok(())
    }
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    // `Basename (argv[0])`: what follows the last slash.
    let prog = argv0
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(argv0.as_slice(), |i| {
            argv0.get(i.saturating_add(1)..).unwrap_or_default()
        })
        .to_vec();
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(&argv, &argv0, &prog, &mut out) {
        Ok(()) => 0,
        Err(Die(code)) => code,
    };
    // `exit`'s own flush, whose failure nobody hears of.
    out.flush_at_exit();
    ExitCode::from(status)
}

/// Upstream's `main`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(
    argv: &[OsString],
    argv0: &[u8],
    prog: &[u8],
    out: &mut ulclosestream::Stdout,
) -> Result<(), Die> {
    let words: Vec<Vec<u8>> = argv.iter().map(|a| os_bytes(a).into_owned()).collect();
    process_root_flag(prog, &words)?;

    let zone = localtime::Zone::from_env();
    let db = pwdb::Db::load();
    let mut ll = Lastlog {
        prog: prog.to_vec(),
        zone,
        db,
        range: Range::default(),
        uflg: false,
        tflg: false,
        bflg: false,
        cflg: false,
        sflg: false,
        seconds: 0,
        inverse_seconds: 0,
        size: 0,
        once: false,
        out,
    };
    let days_of = |v: Option<OsString>, ll: &Lastlog<'_>| -> Result<i64, Die> {
        let text = v.as_deref().map(os_bytes).unwrap_or_default().into_owned();
        match getulong(&text) {
            Some(d) => Ok(i64::from_ne_bytes(d.to_ne_bytes()).wrapping_mul(DAY)),
            None => {
                complain(&ll.prog, &[b"invalid numeric argument '", &text, b"'"]);
                Err(Die(1))
            }
        }
    };

    let mut first_operand: Option<Vec<u8>> = None;
    for item in LASTLOG.parse(
        argv.get(1..).unwrap_or_default(),
        SHORT_OPTIONS,
        LONG_OPTIONS,
    ) {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                // glibc's getopt names the program by `argv[0]` as given.
                let mut m = argv0.to_vec();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                say(&m);
                return Err(usage_error(prog));
            }
        };
        match opt {
            Opt::Short(b'b', v) | Opt::Long("before", v) => {
                ll.inverse_seconds = days_of(v, &ll)?;
                ll.bflg = true;
            }
            Opt::Short(b'C', _) | Opt::Long("clear", _) => ll.cflg = true,
            Opt::Short(b'h', _) | Opt::Long("help", _) => {
                ll.out.write(&usage_text(prog));
                return Ok(());
            }
            // Handled by `process_root_flag`, before the options are read.
            Opt::Short(b'R', _) | Opt::Long("root", _) => {}
            Opt::Short(b'S', _) | Opt::Long("set", _) => ll.sflg = true,
            Opt::Short(b't', v) | Opt::Long("time", v) => {
                ll.seconds = days_of(v, &ll)?;
                ll.tflg = true;
            }
            Opt::Short(b'u', v) | Opt::Long("user", v) => {
                ll.uflg = true;
                let who = v.as_deref().map(os_bytes).unwrap_or_default().into_owned();
                if let Some(uid) = ll.db.user_by_name(&who).map(|u| u.uid) {
                    ll.range = Range {
                        min: Some(u64::from(uid)),
                        max: Some(u64::from(uid)),
                    };
                } else {
                    // `getrange` leaves what it does not parse as it was.
                    let Some(r) = getrange(&who) else {
                        complain(prog, &[b"Unknown user or range: ", &who]);
                        return Err(Die(1));
                    };
                    ll.range = r;
                }
            }
            Opt::Operand(v) => {
                if first_operand.is_none() {
                    first_operand = Some(os_bytes(v).into_owned());
                }
            }
            Opt::Short(..) | Opt::Long(..) => return Err(usage_error(prog)),
        }
    }
    if let Some(op) = first_operand {
        complain(prog, &[b"unexpected argument: ", &op]);
        return Err(usage_error(prog));
    }
    if ll.cflg && ll.sflg {
        complain(prog, &[b"Option -C cannot be used together with option -S"]);
        return Err(usage_error(prog));
    }
    if (ll.cflg || ll.sflg) && !ll.uflg {
        complain(
            prog,
            &[b"Options -C and -S require option -u to specify the user"],
        );
        return Err(usage_error(prog));
    }

    let writing = ll.cflg || ll.sflg;
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .write(writing)
        .open(os_from_bytes(LASTLOG_FILE));
    let file = match opened {
        Ok(f) => f,
        Err(e) => {
            // `perror (LASTLOG_FILE)`.
            let mut m = LASTLOG_FILE.to_vec();
            m.extend_from_slice(b": ");
            m.extend_from_slice(&reason(&e));
            m.push(b'\n');
            say(&m);
            return Err(Die(1));
        }
    };
    match file.metadata() {
        Ok(st) => ll.size = st.len(),
        Err(e) => {
            complain(
                prog,
                &[b"Cannot get the size of ", LASTLOG_FILE, b": ", &reason(&e)],
            );
            return Err(Die(1));
        }
    }

    if writing {
        let mut fp = StdioFile::from_file(file);
        ll.update(&mut fp)?;
        // `fclose`, unchecked: `update` has flushed and synced.
        drop(fp.close());
    } else {
        let mut fp = StdioReader::from_file(file);
        ll.print(&mut fp)?;
        drop(fp.close());
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn ranges_read_as_getrange_reads_them() {
        let r = |min, max| Some(Range { min, max });
        assert_eq!(getrange(b"5"), r(Some(5), Some(5)));
        assert_eq!(getrange(b"5-"), r(Some(5), None));
        assert_eq!(getrange(b"-5"), r(None, Some(5)));
        assert_eq!(getrange(b"5-9"), r(Some(5), Some(9)));
        assert_eq!(getrange(b"9-5"), r(Some(9), Some(5)));
        assert_eq!(getrange(b"-"), None);
        assert_eq!(getrange(b"--5"), None);
        assert_eq!(getrange(b"5-x"), None);
        assert_eq!(getrange(b"5-9x"), None);
        assert_eq!(getrange(b"5x"), None);
        assert_eq!(getrange(b"x"), None);
        // `strtoul` reads nothing from "", and the end is the terminator.
        assert_eq!(getrange(b""), r(Some(0), Some(0)));
        // A sign and leading blanks are `strtoul`'s to accept.
        assert_eq!(getrange(b" 7"), r(Some(7), Some(7)));
        assert_eq!(getrange(b"99999999999999999999"), None);
    }

    #[test]
    fn numbers_read_as_getulong_reads_them() {
        assert_eq!(getulong(b"10"), Some(10));
        assert_eq!(getulong(b"0x10"), Some(16));
        assert_eq!(getulong(b"010"), Some(8));
        assert_eq!(getulong(b""), None);
        assert_eq!(getulong(b"1x"), None);
        assert_eq!(getulong(b"-1"), Some(u64::MAX));
        assert_eq!(getulong(b"99999999999999999999"), None);
    }

    #[test]
    fn login_defs_lines_split_as_def_load_splits_them() {
        assert_eq!(
            defs_line(b"LASTLOG_UID_MAX 1000\n"),
            Some((&b"LASTLOG_UID_MAX"[..], &b"1000"[..]))
        );
        assert_eq!(
            defs_line(b"  NAME \t \"quoted value\" trailing\n"),
            Some((&b"NAME"[..], &b"quoted value"[..]))
        );
        assert_eq!(defs_line(b"# comment\n"), None);
        assert_eq!(defs_line(b"   \n"), None);
        assert_eq!(defs_line(b"LONE\n"), None);
        assert_eq!(defs_line(b"NAME   \n"), None);
        assert_eq!(defs_line(b"NAME \"\"\n"), Some((&b"NAME"[..], &b""[..])));
    }

    #[test]
    fn the_root_flag_is_scanned_as_whole_words() {
        let words = |a: &[&str]| a.iter().map(|s| s.as_bytes().to_vec()).collect::<Vec<_>>();
        // No root named: nothing changes.
        assert!(process_root_flag(b"lastlog", &words(&["lastlog", "-RX", "-u", "root"])).is_ok());
        // Twice: refused before anything is changed.
        assert!(matches!(
            process_root_flag(b"lastlog", &words(&["lastlog", "-R", "a", "--root=b"])),
            Err(Die(E_BAD_ARG))
        ));
        // At the end, with nothing after it.
        assert!(matches!(
            process_root_flag(b"lastlog", &words(&["lastlog", "-u", "-R"])),
            Err(Die(E_BAD_ARG))
        ));
    }

    #[test]
    fn times_print_as_strftime_prints_them() {
        let utc = localtime::Zone::utc();
        assert_eq!(
            ptime(&utc, 1_700_000_000),
            b"Tue Nov 14 22:13:20 +0000 2023"
        );
        assert_eq!(ptime(&utc, 86_400 * 3), b"Sun Jan  4 00:00:00 +0000 1970");
    }
}
