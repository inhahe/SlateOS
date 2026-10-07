//! `chpasswd` — set many passwords at once, from standard input.
//!
//! shadow-utils 4.13's `chpasswd` (`src/chpasswd.c`), as Ubuntu 24.04 builds
//! it, over SlateOS's account database. It reads `user:password` lines, and
//! checks every line before it writes anything: one bad line leaves every
//! account as it was.
//!
//! ```text
//! $ printf 'alice:s3cret\nnobody-here:x\n' | chpasswd -c SHA512
//! chpasswd: line 2: user 'nobody-here' does not exist
//! chpasswd: error detected, changes ignored
//! ```
//!
//! # Where the passwords go
//!
//! Into `/etc/users.yaml`, the account database (design-decisions §353).
//! `/etc/passwd` and `/etc/shadow` are regenerated from it when it is saved.
//! Upstream edits those two directly. Here they are copies, and an edit to a
//! copy would be undone by the next change any other tool makes -- which is
//! what this program did until 2026-10-07: it wrote `/etc/shadow`, nothing
//! that checks a password reads it, and every password it set was ignored.
//!
//! The database is held (`userdb::Lock`) from before it is read until after it
//! is saved, so two account tools run at once cannot erase each other's
//! changes.
//!
//! # Deliberate differences from shadow-utils 4.13
//!
//! 1. **The database**, above. Every diagnostic that names the database
//!    names `/etc/users.yaml` where upstream's names `/etc/passwd` or
//!    `/etc/shadow`.
//! 2. **No PAM.** SlateOS has none, so every run takes the path upstream
//!    takes when `-c`, `-e` or `-m` is given. A run with none of them is
//!    where Ubuntu's `chpasswd` asks PAM instead, and where the two can
//!    differ: PAM's own messages, and its own choice of method.
//! 3. **No `/etc/login.defs`.** SlateOS does not have one (`useradd`'s notes
//!    say why). A password with no `-c` therefore gets the method every new
//!    password here gets, `userdb::PASSWORD_METHOD` (SHA-512). Upstream with
//!    no `login.defs` falls back to DES, whose eight-character, 56-bit key a
//!    laptop recovers. The rounds the `login.defs` variables would choose are
//!    upstream's defaults.
//! 4. **A fresh salt for every password.** Upstream draws one salt when it
//!    starts and gives it to every password in the run. So two accounts given
//!    the same password in one run get the same hash, and one precomputed
//!    table covers the whole batch. Each setting here is otherwise exactly
//!    upstream's: libxcrypt's `crypt_gensalt`, as `posix` answers it, given
//!    the prefix and the cost shadow-utils gives it.
//! 5. **A locked account stays locked.** Upstream's lock is a `!` in front of
//!    the password field, so writing a new password over the field unlocks
//!    the account as a side effect. The database here keeps the lock apart
//!    from the password, and design-decisions §1003 keeps it: after a run
//!    that saved, a note is printed for each such account, in `passwd`'s
//!    words.
//! 6. **A value that is not UTF-8**, which the database cannot hold, fails on
//!    its own line, with upstream's per-line "failed to prepare" message.
//!    Upstream writes it. (A value with a `:` in it fails the save, `failure
//!    while writing changes to ...`, as upstream's does: that one is not a
//!    difference.)
//! 7. **A lock refused for any reason but another holder** -- the caller is
//!    not privileged -- fails at once. Upstream retries it fifteen times, a
//!    second apart, and then says the same thing.
//! 8. **Names in diagnostics are quoted for the terminal** (`quoting`), so a
//!    line of input cannot put escape sequences on the operator's terminal
//!    through an error message. An ordinary name prints as upstream's does.
//!    What upstream prints between quote marks of its own -- `invalid numeric
//!    argument '%s'`, `option '%s' requires an argument`, `invalid chroot path
//!    '%s'` -- keeps those marks, with what is not printable inside them
//!    octal-escaped, as `logger` does (design-decisions §1033); so does every
//!    other value upstream prints bare, the program's own name included.
//! 9. **A value that is not a hash** -- plain text given to `-e`, the password
//!    `-c NONE` stores, the `*0` a passphrase of 512 bytes leaves under DES --
//!    is kept in the database as given. `/etc/shadow`, generated from it,
//!    shows `*` there (`userdb`'s `shadow_entry`, design-decisions §329),
//!    because written through, `crypt` would read it as a DES setting.
//!    Upstream writes it into `/etc/shadow` itself. No password matches
//!    either.

#![cfg_attr(not(test), no_main)]

use std::ffi::OsString;
use std::io::{self, BufReader, Read, Write};
use std::path::Path;

use getoptlong::{Opt, Program, Takes};
// Names and values from the command line, the environment and the input are
// printed in upstream's own sentences, with upstream's own quote marks where
// it has them, and with what is not printable octal-escaped: a name holding a
// newline must not forge a second line of this program's output. That is
// `logger`'s answer to the same question (design-decisions §1033), and the
// only difference from shadow-utils', which prints them raw.
use quoting::{escape_unprintable, escaped_in_quotes, os_bytes, quoteaf};
use userdb::UserDb;

/// shadow-utils' `E_USAGE`: a command line that cannot be run.
const E_USAGE: i32 = 2;

/// shadow-utils' `E_BAD_ARG`, which `--root`'s own failures exit with.
const E_BAD_ARG: i32 = 3;

/// `fail_exit (1)`, and `EXIT_FAILURE`.
const FAILURE: i32 = 1;

/// `BUFSIZ`: the buffer upstream reads each line into with `fgets`. A line of
/// `BUFSIZ - 1` bytes or more, before its newline, is "too long".
const BUFSIZ: usize = 8192;

/// SHA-crypt's rounds, as shadow-utils clamps them (`libmisc/salt.c`).
const SHA_ROUNDS_MIN: i64 = 1000;
const SHA_ROUNDS_MAX: i64 = 999_999_999;
const SHA_ROUNDS_DEFAULT: i64 = 5000;

/// yescrypt's cost factor, as shadow-utils clamps it.
const Y_COST_MIN: i64 = 1;
const Y_COST_MAX: i64 = 11;
const Y_COST_DEFAULT: i64 = 5;

/// `GENSALT_SETTING_SIZE`. For DES shadow-utils hands `crypt_gensalt` a
/// "prefix" of this many dots less one, which libxcrypt reads as a DES
/// setting.
const GENSALT_SETTING_SIZE: usize = 100;

/// Ubuntu's build has SHA-crypt and yescrypt but not bcrypt, so these are the
/// methods `-c` takes -- spelled exactly, in capitals: upstream compares with
/// `strcmp`.
const METHODS: &[&str] = &["DES", "MD5", "NONE", "SHA256", "SHA512", "YESCRYPT"];

/// For a getopt error: the walk's sentences, and its status.
const CHPASSWD: Program = Program::new("chpasswd", E_USAGE);

/// The short options, as upstream's `getopt_long` is given them.
const SHORTS: &str = "c:ehmR:s:";

/// The long options.
const LONGS: &[(&str, Takes)] = &[
    ("crypt-method", Takes::Required),
    ("encrypted", Takes::Nothing),
    ("help", Takes::Nothing),
    ("md5", Takes::Nothing),
    ("root", Takes::Required),
    ("sha-rounds", Takes::Required),
];

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    /// `-e`: the passwords are hashes already, stored as given.
    encrypted: bool,
    /// `-m`: MD5, which `-c MD5` also asks for.
    md5: bool,
    /// `-c`: the method, exactly as typed. `None` without `-c`.
    method: Option<Vec<u8>>,
    /// `-s` was given, whatever became of its value. See [`parse`].
    sflg: bool,
    /// `-s`'s value for SHA-crypt, or upstream's starting value.
    sha_rounds: i64,
    /// `-s`'s value for yescrypt, or upstream's starting value.
    yescrypt_cost: i64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            encrypted: false,
            md5: false,
            method: None,
            sflg: false,
            sha_rounds: SHA_ROUNDS_DEFAULT,
            yescrypt_cost: Y_COST_DEFAULT,
        }
    }
}

/// Why the command line ends the run before any line is read.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stop {
    /// `--help`: the usage text on standard output, status 0.
    Help,
    /// A diagnostic, then the usage text, both on standard error: upstream's
    /// `usage (E_USAGE)` after the message.
    Usage(String),
}

impl Config {
    /// Upstream's `IS_CRYPT_METHOD`: `-c` given, and exactly `name`.
    fn method_is(&self, name: &str) -> bool {
        self.method.as_deref() == Some(name.as_bytes())
    }
}

/// The method `-c` named, as one of [`METHODS`], or `None` for a name upstream
/// does not know -- which `check_flags` has refused before anything asks.
fn known_method(typed: &[u8]) -> Option<&'static str> {
    METHODS
        .iter()
        .copied()
        .find(|name| name.as_bytes() == typed)
}

/// Upstream's `process_flags` and `check_flags`.
///
/// `-s` is read when it is met, against whatever `-c` said *before* it. So
/// `-c SHA512 -s 9000` asks for 9000 rounds, and `-s 9000 -c SHA512` takes
/// the default and never looks at `9000` -- not even to refuse `-s banana`.
/// That is upstream's, measured, and kept: the order is part of the command
/// line's meaning.
///
/// `argv0` is the program as it was invoked, for getopt's sentences, which
/// glibc prefixes with `argv[0]` whole; `prog` is its last component, for
/// everything upstream says itself.
fn parse(args: &[OsString], argv0: &[u8], prog: &[u8]) -> Result<Config, Stop> {
    let mut cfg = Config::default();
    for item in CHPASSWD.parse(args, SHORTS, LONGS) {
        let item = item
            .map_err(|e| Stop::Usage(format!("{}: {}", escape_unprintable(argv0), e.sentence)))?;
        let (letter, value) = match &item {
            // Operands are not looked at: upstream never reads past `optind`.
            Opt::Operand(_) => continue,
            Opt::Short(c, v) => (*c, v.as_ref()),
            Opt::Long(name, v) => (long_letter(name), v.as_ref()),
        };
        let value = value.map(|v| os_bytes(v).into_owned()).unwrap_or_default();
        match letter {
            b'c' => cfg.method = Some(value),
            b'e' => cfg.encrypted = true,
            b'h' => return Err(Stop::Help),
            b'm' => cfg.md5 = true,
            // Handled by `root_flag`, after the rest: upstream's no-op here.
            b'R' => {}
            b's' => {
                cfg.sflg = true;
                let target = if cfg.method_is("SHA256") || cfg.method_is("SHA512") {
                    Some(&mut cfg.sha_rounds)
                } else if cfg.method_is("YESCRYPT") {
                    Some(&mut cfg.yescrypt_cost)
                } else {
                    None
                };
                if let Some(target) = target {
                    let Some(n) = getlong(&value) else {
                        return Err(Stop::Usage(format!(
                            "{}: invalid numeric argument {}",
                            escape_unprintable(prog),
                            escaped_in_quotes(&value)
                        )));
                    };
                    *target = n;
                }
            }
            _ => {}
        }
    }
    check_flags(&cfg, prog)?;
    Ok(cfg)
}

/// The short letter a long option stands for.
fn long_letter(name: &str) -> u8 {
    match name {
        "crypt-method" => b'c',
        "encrypted" => b'e',
        "help" => b'h',
        "md5" => b'm',
        "root" => b'R',
        "sha-rounds" => b's',
        _ => 0,
    }
}

/// Upstream's `check_flags`.
fn check_flags(cfg: &Config, prog: &[u8]) -> Result<(), Stop> {
    let prog = escape_unprintable(prog);
    let cflg = cfg.method.is_some();
    if cfg.sflg && !cflg {
        return Err(Stop::Usage(format!(
            "{prog}: -s flag is only allowed with the -c flag"
        )));
    }
    if (cfg.encrypted && (cfg.md5 || cflg)) || (cfg.md5 && cflg) {
        return Err(Stop::Usage(format!(
            "{prog}: the -c, -e, and -m flags are exclusive"
        )));
    }
    if let Some(method) = &cfg.method
        && known_method(method).is_none()
    {
        return Err(Stop::Usage(format!(
            "{prog}: unsupported crypt method: {}",
            escape_unprintable(method)
        )));
    }
    Ok(())
}

/// shadow-utils' `getlong`: `strtol (numstr, &end, 0)`, refused if it read
/// nothing, stopped before the end, or overflowed.
///
/// Base 0, so `0x10` is sixteen and `010` is eight, and `08` is refused --
/// `strtol` reads the `0` as octal and stops at the `8`. Leading blanks and a
/// sign are `strtol`'s to take.
fn getlong(text: &[u8]) -> Option<i64> {
    let mut at = text
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .unwrap_or(text.len());
    let negative = match text.get(at) {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };
    let rest = text.get(at..).unwrap_or_default();
    let (base, digits): (u32, &[u8]) = match rest {
        [b'0', b'x' | b'X', next, ..] if next.is_ascii_hexdigit() => {
            (16, rest.get(2..).unwrap_or_default())
        }
        [b'0', ..] => (8, rest),
        _ => (10, rest),
    };
    let len = digits
        .iter()
        .take_while(|&&b| char::from(b).is_digit(base))
        .count();
    if len == 0 || len != digits.len() {
        return None;
    }
    let mut magnitude: i128 = 0;
    for &b in digits {
        let d = char::from(b).to_digit(base)?;
        magnitude = magnitude
            .checked_mul(i128::from(base))?
            .checked_add(i128::from(d))?;
        // Past `LONG_MIN`'s magnitude is `ERANGE` whichever the sign.
        if magnitude > i128::from(i64::MAX).checked_add(1)? {
            return None;
        }
    }
    let value = if negative {
        magnitude.checked_neg()?
    } else {
        magnitude
    };
    i64::try_from(value).ok()
}

/// Upstream's `usage`.
///
/// The method list is Ubuntu's build's (no bcrypt), while the `-s` line names
/// bcrypt anyway: upstream prints that line whenever any of the three is
/// built.
fn usage_text(prog: &[u8]) -> String {
    format!(
        "Usage: {} [options]\n\
         \n\
         Options:\n\
         \x20 -c, --crypt-method METHOD     the crypt method (one of NONE DES MD5 SHA256 SHA512 YESCRYPT)\n\
         \x20 -e, --encrypted               supplied passwords are encrypted\n\
         \x20 -h, --help                    display this help message and exit\n\
         \x20 -m, --md5                     encrypt the clear text password using\n\
         \x20                               the MD5 algorithm\n\
         \x20 -R, --root CHROOT_DIR         directory to chroot into\n\
         \x20 -s, --sha-rounds              number of rounds for the SHA, BCRYPT\n\
         \x20                               or YESCRYPT crypt algorithms\n\
         \n",
        escape_unprintable(prog)
    )
}

/// shadow-utils' `Basename`: everything after the last `/`, which is nothing
/// for a name that ends in one.
fn basename(argv0: &[u8]) -> &[u8] {
    match argv0.iter().rposition(|&b| b == b'/') {
        Some(slash) => argv0.get(slash.saturating_add(1)..).unwrap_or_default(),
        None => argv0,
    }
}

// ---------------------------------------------------------------------------
// --root
// ---------------------------------------------------------------------------

/// Why `--root` ends the run: the message, and `E_BAD_ARG` or `FAILURE`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RootFailure {
    message: String,
    status: i32,
}

/// The directory `--root` names, found as upstream's `process_root_flag`
/// finds it: by its own scan of argv, after getopt has run.
///
/// The scan matches `--root`, `--root=DIR` and `-R` *exactly*, so the
/// spellings getopt also accepts -- `--ro DIR`, `-RDIR`, `-eR DIR` -- reach
/// getopt and are ignored there, and the run does not change root. It also
/// reads every word, `argv[0]` and those after `--` included. All of that is
/// upstream's, and kept for the same reason as `-s`'s order.
fn root_flag(args: &[OsString], prog: &[u8]) -> Result<Option<Vec<u8>>, RootFailure> {
    let prog = escape_unprintable(prog);
    let mut newroot: Option<Vec<u8>> = None;
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        let arg = os_bytes(arg);
        let value = arg.strip_prefix(b"--root=".as_slice());
        if arg.as_ref() == b"--root" || value.is_some() || arg.as_ref() == b"-R" {
            if newroot.is_some() {
                return Err(RootFailure {
                    message: format!("{prog}: multiple --root options"),
                    status: E_BAD_ARG,
                });
            }
            if let Some(value) = value {
                newroot = Some(value.to_vec());
            } else {
                i = i.saturating_add(1);
                let Some(next) = args.get(i) else {
                    return Err(RootFailure {
                        message: format!(
                            "{prog}: option {} requires an argument",
                            escaped_in_quotes(&arg)
                        ),
                        status: E_BAD_ARG,
                    });
                };
                newroot = Some(os_bytes(next).into_owned());
            }
        }
        i = i.saturating_add(1);
    }
    Ok(newroot)
}

/// Upstream's `change_root`: drop privileges, then enter `newroot`.
#[cfg(unix)]
fn change_root(newroot: &[u8], prog: &[u8]) -> Result<(), RootFailure> {
    use std::os::unix::ffi::OsStrExt as _;
    unsafe extern "C" {
        fn getuid() -> u32;
        fn getgid() -> u32;
        fn setreuid(ruid: u32, euid: u32) -> i32;
        fn setregid(rgid: u32, egid: u32) -> i32;
    }
    let prog = escape_unprintable(prog);
    let shown = escape_unprintable(newroot);
    let bad_arg = |message: String| RootFailure {
        message,
        status: E_BAD_ARG,
    };
    // SAFETY: the four calls take and return plain integers and touch no
    // memory of this process's; `setre[ug]id` to the real ids gives up a
    // set-id program's privilege and is a no-op otherwise.
    let dropped = unsafe { setregid(getgid(), getgid()) == 0 && setreuid(getuid(), getuid()) == 0 };
    if !dropped {
        return Err(RootFailure {
            message: format!(
                "{prog}: failed to drop privileges ({})",
                errmsg::strerror(&io::Error::last_os_error())
            ),
            status: FAILURE,
        });
    }
    if newroot.first() != Some(&b'/') {
        return Err(bad_arg(format!(
            "{prog}: invalid chroot path {}, only absolute paths are supported.",
            escaped_in_quotes(newroot)
        )));
    }
    let path = Path::new(std::ffi::OsStr::from_bytes(newroot));
    // `access (newroot, F_OK)`: whether it can be reached, through links.
    if let Err(e) = std::fs::metadata(path) {
        return Err(bad_arg(format!(
            "{prog}: cannot access chroot directory {shown}: {}",
            errmsg::strerror(&e)
        )));
    }
    if let Err(e) = std::env::set_current_dir(path) {
        return Err(bad_arg(format!(
            "{prog}: cannot chdir to chroot directory {shown}: {}",
            errmsg::strerror(&e)
        )));
    }
    if let Err(e) = std::os::unix::fs::chroot(path) {
        return Err(bad_arg(format!(
            "{prog}: unable to chroot to directory {shown}: {}",
            errmsg::strerror(&e)
        )));
    }
    Ok(())
}

/// The development host has no `chroot`; the command is SlateOS's and Linux's.
#[cfg(not(unix))]
fn change_root(newroot: &[u8], prog: &[u8]) -> Result<(), RootFailure> {
    Err(RootFailure {
        message: format!(
            "{}: unable to chroot to directory {}: Function not implemented",
            escape_unprintable(prog),
            escape_unprintable(newroot)
        ),
        status: E_BAD_ARG,
    })
}

// ---------------------------------------------------------------------------
// The setting a new password is hashed under
// ---------------------------------------------------------------------------

/// What `crypt_gensalt` is asked for: shadow-utils' `crypt_make_salt`, before
/// it calls it. `None` stores the password as given (`-e`, `-c NONE`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct SettingRequest {
    /// The prefix shadow-utils builds, rounds and cost included.
    prefix: Vec<u8>,
    /// The `count` it passes beside it.
    count: u64,
    /// libxcrypt's own random-byte count for the method, so that a salt here
    /// is as long as one libxcrypt draws for itself.
    nrbytes: usize,
}

/// Upstream's `get_salt` and `crypt_make_salt`, up to the `crypt_gensalt`
/// call.
fn setting_request(cfg: &Config) -> Option<SettingRequest> {
    if cfg.encrypted || cfg.method_is("NONE") {
        return None;
    }
    let method = if cfg.md5 {
        "MD5"
    } else {
        // No `-c`: see deliberate difference 3.
        cfg.method
            .as_deref()
            .and_then(known_method)
            .unwrap_or(match userdb::PASSWORD_METHOD {
                posix::crypt::Method::Sha256 => "SHA256",
                posix::crypt::Method::Yescrypt => "YESCRYPT",
                posix::crypt::Method::Md5 => "MD5",
                _ => "SHA512",
            })
    };
    // `-s`'s value is given only with `-s`, and read as an `int` through a
    // pointer to the `long` it was stored in: on x86-64 that is the low 32
    // bits, so `-s 4294967297` asks for one round. Reproduced as `as i32`.
    #[allow(clippy::cast_possible_truncation)]
    let preferred = |value: i64| cfg.sflg.then_some(i64::from(value as i32));
    match method {
        "MD5" => Some(SettingRequest {
            prefix: b"$1$".to_vec(),
            count: 0,
            nrbytes: 9,
        }),
        "SHA256" | "SHA512" => {
            let rounds = sha_rounds(preferred(cfg.sha_rounds));
            let mut prefix = if method == "SHA256" {
                b"$5$".to_vec()
            } else {
                b"$6$".to_vec()
            };
            if rounds != SHA_ROUNDS_DEFAULT {
                prefix.extend_from_slice(format!("rounds={rounds}$").as_bytes());
            }
            Some(SettingRequest {
                prefix,
                count: u64::try_from(rounds).unwrap_or(0),
                nrbytes: 15,
            })
        }
        "YESCRYPT" => {
            let cost = yescrypt_cost(preferred(cfg.yescrypt_cost));
            let mut prefix = b"$y$".to_vec();
            prefix.extend_from_slice(&yescrypt_cost_text(cost));
            Some(SettingRequest {
                prefix,
                count: u64::try_from(cost).unwrap_or(0),
                nrbytes: 16,
            })
        }
        // DES, which upstream also reaches from a method it does not know --
        // unreachable here, since `check_flags` refused it.
        _ => Some(SettingRequest {
            prefix: vec![b'.'; GENSALT_SETTING_SIZE.saturating_sub(1)],
            count: 0,
            nrbytes: 2,
        }),
    }
}

/// `SHA_get_salt_rounds`: `-s`'s value, 0 meaning the default, clamped; or
/// the default with no `-s` and no `login.defs`.
///
/// Upstream holds the count in an `unsigned long`, so a negative one is a huge
/// one and is clamped to the *most* rounds: `-s -5` hashes with 999,999,999,
/// which takes minutes. Measured, and kept.
fn sha_rounds(preferred: Option<i64>) -> i64 {
    clamp_as_unsigned(
        preferred,
        SHA_ROUNDS_DEFAULT,
        SHA_ROUNDS_MIN,
        SHA_ROUNDS_MAX,
    )
}

/// `YESCRYPT_get_salt_cost`, as [`sha_rounds`]: a negative cost is the
/// highest.
fn yescrypt_cost(preferred: Option<i64>) -> i64 {
    clamp_as_unsigned(preferred, Y_COST_DEFAULT, Y_COST_MIN, Y_COST_MAX)
}

/// `value`, read as the `unsigned long` upstream assigns it to (0 meaning
/// `default`), clamped to `min..=max`.
fn clamp_as_unsigned(value: Option<i64>, default: i64, min: i64, max: i64) -> i64 {
    match value {
        None | Some(0) => default,
        Some(n) if n < 0 => max,
        Some(n) => n.clamp(min, max),
    }
}

/// `YESCRYPT_salt_cost_to_buf`: `j`, a letter for the cost, `T` or `5`, `$`.
fn yescrypt_cost_text(cost: i64) -> [u8; 4] {
    // The cost is clamped to 1..=11 before this, so each sum is a letter.
    let offset = u8::try_from(cost).unwrap_or(5);
    let middle = if offset < 3 {
        0x36_u8.saturating_add(offset)
    } else if offset < 6 {
        0x34_u8.saturating_add(offset)
    } else {
        0x3b_u8.saturating_add(offset)
    };
    [b'j', middle, if offset >= 3 { b'T' } else { b'5' }, b'$']
}

/// libxcrypt's `crypt_gensalt_rn`, as `posix` answers it: the setting
/// `request` names, salted with `random`. `None` if it is refused.
fn gensalt(request: &SettingRequest, random: &[u8]) -> Option<Vec<u8>> {
    let mut prefix = request.prefix.clone();
    prefix.push(0);
    let mut out = [0u8; posix::gensalt::CRYPT_GENSALT_OUTPUT_SIZE];
    let nrbytes = i32::try_from(random.len()).ok()?;
    let room = i32::try_from(out.len()).ok()?;
    // SAFETY: `prefix` is NUL-terminated; `random` is readable for its
    // `nrbytes` bytes; `out` is writable for its `room` bytes, and nothing
    // else refers to any of the three while this runs.
    let made = unsafe {
        posix::gensalt::crypt_gensalt_rn(
            prefix.as_ptr(),
            request.count,
            random.as_ptr(),
            nrbytes,
            out.as_mut_ptr(),
            room,
        )
    };
    if made.is_null() {
        return None;
    }
    let len = out.iter().position(|&b| b == 0)?;
    out.get(..len).map(<[u8]>::to_vec)
}

/// Fill `out` from the kernel's random source, or say it could not.
///
/// `getrandom`, as upstream's `read_random_bytes` prefers, rather than a read
/// of `/dev/urandom`: under `--root` the new root need not have one, and
/// upstream draws its salt before it changes root.
#[cfg(unix)]
fn random_bytes(out: &mut [u8]) -> bool {
    unsafe extern "C" {
        fn getrandom(buf: *mut u8, len: usize, flags: u32) -> isize;
    }
    let mut filled = 0;
    while let Some(rest) = out.get_mut(filled..) {
        if rest.is_empty() {
            return true;
        }
        // SAFETY: `rest` is writable for its length, and `getrandom` writes
        // at most that many bytes into it.
        let got = unsafe { getrandom(rest.as_mut_ptr(), rest.len(), 0) };
        match usize::try_from(got) {
            Ok(n) => filled = filled.saturating_add(n),
            Err(_) if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
    true
}

/// The development host draws none, and says so.
#[cfg(not(unix))]
fn random_bytes(_out: &mut [u8]) -> bool {
    false
}

/// Hash `password` under a fresh setting for `request`, as upstream's
/// `pw_encrypt` does, or say why not.
fn hash_new(
    password: &[u8],
    request: &SettingRequest,
    random: &mut dyn FnMut(&mut [u8]) -> bool,
) -> Result<Vec<u8>, String> {
    // Ubuntu's shadow-utils leaves the random bytes to libxcrypt's
    // `crypt_gensalt`, so a source that fails is that call failing, and is
    // reported as one: no program name, and the run ends.
    let mut rbytes = vec![0u8; request.nrbytes];
    let made = if random(&mut rbytes) {
        gensalt(request, &rbytes)
    } else {
        None
    };
    let Some(setting) = made else {
        return Err(format!(
            "Unable to generate a salt from setting \"{}\", check your settings in \
             ENCRYPT_METHOD and the corresponding configuration for your selected hash method.",
            escape_unprintable(&request.prefix)
        ));
    };
    let mut hash_buf = posix::crypt::buf();
    let Some(hashed) = posix::crypt::hash_into(password, &setting, &mut hash_buf) else {
        return crypt_failed(&setting);
    };
    Ok(hashed.as_bytes().to_vec())
}

/// What upstream does when `crypt` fails -- a passphrase over libxcrypt's 512
/// bytes, say.
///
/// libxcrypt's `crypt` does not return NULL, so `pw_encrypt`'s "failed to
/// crypt password" is never reached. It returns a failure token, `*0` (`*1`
/// when the setting itself begins `*0`), and `pw_encrypt` judges that by its
/// length. Under a `$` setting a result of thirteen bytes or fewer means the
/// method is missing, and the run ends saying so, with no program name.
/// Under DES thirteen bytes is a hash, so the token is not caught and is
/// returned -- and stored as the password, which nothing can match.
fn crypt_failed(setting: &[u8]) -> Result<Vec<u8>, String> {
    if setting.first() == Some(&b'$') {
        let method = match setting.get(1) {
            Some(b'1') => "MD5".to_string(),
            Some(b'2') => "BCRYPT".to_string(),
            Some(b'5') => "SHA256".to_string(),
            Some(b'6') => "SHA512".to_string(),
            Some(b'y') => "YESCRYPT".to_string(),
            Some(&other) => format!("${}$", char::from(other)),
            None => "$$".to_string(),
        };
        return Err(format!(
            "crypt method not supported by libcrypt? ({method})"
        ));
    }
    Ok(if setting.starts_with(b"*0") {
        b"*1".to_vec()
    } else {
        b"*0".to_vec()
    })
}

// ---------------------------------------------------------------------------
// The day a password changed
// ---------------------------------------------------------------------------

/// shadow-utils' `gettime`: now, or `$SOURCE_DATE_EPOCH` when it holds a time
/// no later than now. A value that does not is reported, on standard error and
/// with no program name, and now is used instead.
fn gettime(now: u64, epoch_var: Option<&[u8]>, err: &mut dyn Write) -> u64 {
    let Some(text) = epoch_var else {
        return now;
    };
    let complain = |err: &mut dyn Write, what: String| {
        // Unchecked: a diagnostic that cannot be written cannot be reported.
        let _ = writeln!(err, "Environment variable $SOURCE_DATE_EPOCH: {what}");
    };
    match strtoull(text) {
        Strtoull::Overflow => complain(err, "strtoull: Numerical result out of range".to_string()),
        Strtoull::NoDigits => complain(
            err,
            format!("No digits were found: {}", escape_unprintable(text)),
        ),
        Strtoull::Trailing(rest) => complain(
            err,
            format!("Trailing garbage: {}", escape_unprintable(rest)),
        ),
        Strtoull::Value(epoch) if epoch > now => complain(
            err,
            format!(
                "value must be smaller than or equal to the current time ({now}) but was found to be: {epoch}"
            ),
        ),
        Strtoull::Value(epoch) => return epoch,
    }
    now
}

/// What `strtoull (text, &end, 10)` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strtoull<'a> {
    /// No digits: `end` is `text`.
    NoDigits,
    /// More than `ULLONG_MAX`: `ERANGE`.
    Overflow,
    /// A number, then these bytes.
    Trailing(&'a [u8]),
    /// A number and nothing after it -- negated modulo 2^64 after a `-`, as
    /// `strtoull` negates.
    Value(u64),
}

/// `strtoull` in base 10: blanks, a sign, digits.
fn strtoull(text: &[u8]) -> Strtoull<'_> {
    let start = text
        .iter()
        .position(|&b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .unwrap_or(text.len());
    let (negative, digits_at) = match text.get(start) {
        Some(b'-') => (true, start.saturating_add(1)),
        Some(b'+') => (false, start.saturating_add(1)),
        _ => (false, start),
    };
    let rest = text.get(digits_at..).unwrap_or_default();
    let len = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if len == 0 {
        return Strtoull::NoDigits;
    }
    let mut value: u64 = 0;
    for &b in rest.get(..len).unwrap_or_default() {
        let Some(next) = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u64::from(b.wrapping_sub(b'0'))))
        else {
            return Strtoull::Overflow;
        };
        value = next;
    }
    if negative {
        value = value.wrapping_neg();
    }
    match rest.get(len..) {
        Some(tail) if !tail.is_empty() => Strtoull::Trailing(tail),
        _ => Strtoull::Value(value),
    }
}

/// Seconds since the epoch, now.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ---------------------------------------------------------------------------
// Standard input, as `fgets` reads it
// ---------------------------------------------------------------------------

/// Standard input read the way upstream's loop reads it: `fgets` into a
/// `BUFSIZ` buffer, with `feof` beside it.
///
/// Both halves of `fgets`' behaviour show in the output. A piece ends at a
/// newline or at `BUFSIZ - 1` bytes, whichever comes first, which is what
/// makes a line "too long". And the program looks at each piece as a C
/// string, so a NUL byte ends the line there: the newline after it goes
/// unseen, the line is "too long", and the drain that skips the rest of a
/// long line runs on into the line after it. That last part is upstream's
/// accident, and is reproduced because it decides which lines get changed.
struct Fgets<R> {
    inner: BufReader<R>,
    eof: bool,
}

impl<R: Read> Fgets<R> {
    fn new(inner: R) -> Self {
        Self {
            inner: BufReader::new(inner),
            eof: false,
        }
    }

    /// The next piece into `buf`, or `false` where `fgets` returns NULL:
    /// end of input, or a read error, before a byte.
    fn next_piece(&mut self, buf: &mut Vec<u8>) -> bool {
        buf.clear();
        while buf.len() < BUFSIZ.saturating_sub(1) {
            let mut byte = [0u8; 1];
            match self.inner.read(&mut byte) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(_) => {
                    buf.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    // glibc's `fgets` returns NULL on an error, whatever it
                    // read first; the loop then ends as it does at the end.
                    self.eof = true;
                    return false;
                }
            }
        }
        !buf.is_empty()
    }
}

/// `buf` as C sees it: up to its first NUL.
fn c_string(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf.get(..end).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// Everything a run needs from outside itself, so that a test can supply it.
struct World<'a> {
    /// The account database.
    db_path: &'a Path,
    /// `gettime`'s two inputs.
    now: u64,
    source_date_epoch: Option<Vec<u8>>,
    /// The random source settings are salted from.
    random: &'a mut dyn FnMut(&mut [u8]) -> bool,
}

/// Upstream's `main`, from `open_files` to the end: read the lines, change
/// the database in memory, and save it only if every line was good.
fn run(
    cfg: &Config,
    prog: &[u8],
    input: impl Read,
    err: &mut dyn Write,
    world: &mut World<'_>,
) -> i32 {
    let prog_text = escape_unprintable(prog);
    let db_name = escape_unprintable(&os_bytes(world.db_path.as_os_str()));
    let request = setting_request(cfg);

    // `open_files`: hold the database, then read it.
    let _lock = match UserDb::lock(world.db_path) {
        Ok(lock) => lock,
        Err(_) => {
            let _ = writeln!(err, "{prog_text}: cannot lock {db_name}; try again later.");
            return FAILURE;
        }
    };
    let Ok(mut db) = UserDb::load(world.db_path) else {
        let _ = writeln!(err, "{prog_text}: cannot open {db_name}");
        return FAILURE;
    };

    let mut errors: u32 = 0;
    let mut line: u64 = 0;
    let mut still_locked: Vec<String> = Vec::new();
    // Whether a stored value holds a byte upstream's `shadow_put` refuses
    // (`valid_field (sp_pwdp, ":\n")`): it accepts the line, and the write of
    // the whole file fails. See the save below.
    let mut unwritable = false;
    let mut lines = Fgets::new(input);
    let mut buf = Vec::with_capacity(BUFSIZ);
    // Upstream's diagnostics say "line %d", an `int`: the count is printed as
    // one, wrapping as C's does, though no input is that long.
    #[allow(clippy::cast_possible_truncation)]
    let line_no = |line: u64| line as i32;

    while lines.next_piece(&mut buf) {
        line = line.saturating_add(1);
        let piece = c_string(&buf);
        let text = if let Some(newline) = piece.iter().rposition(|&b| b == b'\n') {
            piece.get(..newline).unwrap_or_default().to_vec()
        } else if !lines.eof {
            // Drop the rest of this line, piece by piece, until one holds a
            // newline -- as C sees it.
            let mut rest = Vec::new();
            while lines.next_piece(&mut rest) && !c_string(&rest).contains(&b'\n') {}
            let _ = writeln!(err, "{prog_text}: line {}: line too long", line_no(line));
            errors = errors.saturating_add(1);
            continue;
        } else {
            piece.to_vec()
        };

        let Some(colon) = text.iter().position(|&b| b == b':') else {
            let _ = writeln!(
                err,
                "{prog_text}: line {}: missing new password",
                line_no(line)
            );
            errors = errors.saturating_add(1);
            continue;
        };
        let name = text.get(..colon).unwrap_or_default();
        let newpwd = text.get(colon.saturating_add(1)..).unwrap_or_default();

        // The hash comes before the lookup, as upstream's does: a name that
        // does not exist costs what one that does costs.
        let stored = match &request {
            Some(request) => match hash_new(newpwd, request, world.random) {
                Ok(hash) => hash,
                Err(message) => {
                    let _ = writeln!(err, "{message}");
                    return FAILURE;
                }
            },
            None => newpwd.to_vec(),
        };

        let found = std::str::from_utf8(name).ok().and_then(|n| db.find_mut(n));
        let Some(record) = found else {
            let _ = writeln!(
                err,
                "{prog_text}: line {}: user {} does not exist",
                line_no(line),
                quoteaf(name)
            );
            errors = errors.saturating_add(1);
            continue;
        };

        // `sp_lstchg = gettime () / SCALE`, and a day of 0 is "disable aging"
        // rather than "change it at once": upstream's -1, an empty field.
        // Before the entry is prepared, as upstream's is, so a complaint
        // about `$SOURCE_DATE_EPOCH` comes first.
        let day = gettime(world.now, world.source_date_epoch.as_deref(), err) / 86_400;

        // See deliberate difference 6: a value with a `:` or a control byte
        // is taken here and fails the save, as upstream's does; one the
        // database cannot hold at all fails here.
        let Ok(stored) = std::str::from_utf8(&stored) else {
            let _ = writeln!(
                err,
                "{prog_text}: line {}: failed to prepare the new {db_name} entry {}",
                line_no(line),
                quoteaf(name)
            );
            errors = errors.saturating_add(1);
            continue;
        };

        unwritable |= stored.contains([':', '\n']);
        record.set(userdb::field::PASSWORD_HASH, stored);
        let aging = userdb::Aging {
            changed: i64::try_from(day).ok().filter(|&d| d != 0),
            ..record.aging()
        };
        record.set_aging(&aging);
        // The `locked: true` flag, which the new password leaves standing
        // (§1003) -- not `is_locked`, which also reads a `!` or `*` in front
        // of the entry: a `!` there went with the old entry, as upstream's
        // does, and a `*` is what the new one is.
        if record
            .get(userdb::field::LOCKED)
            .is_some_and(|v| v.trim() == "true")
        {
            still_locked.push(escape_unprintable(name));
        }
    }

    if errors != 0 {
        let _ = writeln!(err, "{prog_text}: error detected, changes ignored");
        return FAILURE;
    }

    // Upstream's `close_files`, where a `:` in a stored value fails the
    // write of `/etc/shadow` and nothing is changed. The generated file here
    // would show `*` for such a value (deliberate difference 9) and could be
    // written, so the refusal is made here, upstream's way.
    if unwritable {
        let _ = writeln!(
            err,
            "{prog_text}: failure while writing changes to {db_name}"
        );
        return FAILURE;
    }

    // `close_files`: the save, which also writes `/etc/passwd` and
    // `/etc/shadow` from the database.
    if db.save(world.db_path).is_err() {
        let _ = writeln!(
            err,
            "{prog_text}: failure while writing changes to {db_name}"
        );
        return FAILURE;
    }

    // See deliberate difference 5: `passwd`'s note, once the change is real.
    for name in still_locked {
        let _ = writeln!(
            err,
            "{prog_text}: note: `{name}' is still locked and will refuse this password; \
             run `passwd -u' to unlock it"
        );
    }
    0
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// The whole program, from argv to the status: the part of `main` a test can
/// reach.
fn chpasswd(argv: &[OsString], input: impl Read, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let prog = basename(&argv0).to_vec();
    let args = argv.get(1..).unwrap_or_default();

    let cfg = match parse(args, &argv0, &prog) {
        Ok(cfg) => cfg,
        Err(Stop::Help) => {
            let _ = out.write_all(usage_text(&prog).as_bytes());
            return 0;
        }
        Err(Stop::Usage(message)) => {
            let _ = writeln!(err, "{message}");
            let _ = err.write_all(usage_text(&prog).as_bytes());
            return E_USAGE;
        }
    };

    match root_flag(argv, &prog) {
        Ok(Some(newroot)) => {
            if let Err(failure) = change_root(&newroot, &prog) {
                let _ = writeln!(err, "{}", failure.message);
                return failure.status;
            }
        }
        Ok(None) => {}
        Err(failure) => {
            let _ = writeln!(err, "{}", failure.message);
            return failure.status;
        }
    }

    let mut random = random_bytes;
    let mut world = World {
        db_path: Path::new(userdb::DEFAULT_PATH),
        now: now_secs(),
        source_date_epoch: std::env::var_os("SOURCE_DATE_EPOCH").map(|v| os_bytes(&v).into_owned()),
        random: &mut random,
    };
    run(&cfg, &prog, input, err, &mut world)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    // `args_os`, not `args`: the latter panics on an argument that is not
    // valid UTF-8.
    let argv: Vec<OsString> = std::env::args_os().collect();
    let stdout = io::stdout();
    let stderr = io::stderr();
    chpasswd(&argv, io::stdin(), &mut stdout.lock(), &mut stderr.lock())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
