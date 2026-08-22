//! stat — display file or filesystem status.
//!
//! ```text
//! stat [-L] [-t] [-c FORMAT | --printf=FORMAT] [--] FILE...
//! stat -f [-t] [-c FORMAT | --printf=FORMAT] [--] FILE...
//!   -L, --dereference    follow symbolic links (default: report the link)
//!   -f, --file-system    report the filesystem the file lives on, not the file
//!   -c, --format=FORMAT  print FORMAT, then a newline, for each operand
//!       --printf=FORMAT  as -c, but interpret backslash escapes and add no
//!                        trailing newline
//!   -t, --terse          one line of bare numbers, for scripts
//!   --                   end of options
//! ```
//!
//! # This program is mostly read by scripts, so `-c` is not a nicety
//!
//! The previous version parsed **no options at all**: every argument was a file
//! name. The single most common use of `stat` anywhere —
//! `size=$(stat -c %s file)` — therefore stat'ed three files called `-c`, `%s`
//! and `file`, failed on the first two, and printed the whole human-readable
//! block for the third. The caller's `$size` came out as a paragraph of text.
//! `-L` and `-t` were broken the same way. See `known-issues.md` →
//! `B-stat-HAS-NO-OPTIONS-AND-CANNOT-READ-A-CLOCK`.
//!
//! # Two things worth knowing about the output
//!
//! **Symbolic links are reported, not followed** — `lstat`, matching GNU. That
//! is what makes `stat` the tool you reach for when you want to know whether
//! something *is* a link, and `-L` is how you ask about the target instead. It
//! also means the default can never be steered somewhere else by a link.
//!
//! **Times are UTC.** There is no timezone database on this OS yet, and a
//! wrong local time is worse than an honestly-labelled UTC one, so every
//! timestamp is printed with an explicit `+0000`. When zoneinfo lands this
//! should follow `TZ` like GNU does.

#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::quote::quoteaf_os;

// ===========================================================================
// Platform-neutral model
// ===========================================================================
//
// Everything below the syscalls is expressed over these two plain structs
// rather than over `std::fs::Metadata`. `Metadata`'s unix accessors only exist
// behind `cfg(unix)`, and the build host is Windows — so anything written
// against them is invisible to `cargo test --workspace` and is, in practice,
// untested. That is how a `stat` with no argument parser survived.

/// The fields of `struct stat` this program can print.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct StatInfo {
    dev: u64,
    ino: u64,
    /// Full mode word: file-type bits *and* permission bits.
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    rdev: u64,
    size: u64,
    blksize: u64,
    /// 512-byte blocks actually allocated, which is not `size / 512` for a
    /// sparse or a compressed file — that difference is the reason `%b` exists.
    blocks: u64,
    atime: i64,
    atime_nsec: u32,
    mtime: i64,
    mtime_nsec: u32,
    ctime: i64,
    ctime_nsec: u32,
}

/// The fields of `struct statvfs` this program can print.
#[derive(Clone, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct FsInfo {
    fsid: u64,
    namelen: u64,
    bsize: u64,
    frsize: u64,
    blocks: u64,
    bfree: u64,
    bavail: u64,
    files: u64,
    ffree: u64,
}

// ===========================================================================
// Mode formatting
// ===========================================================================

/// The file-type bits of a mode word (`S_IFMT`).
const S_IFMT: u32 = 0o170000;

/// Format a POSIX mode word as a 10-character permission string like
/// `-rwxr-xr--`, with setuid/setgid/sticky rendered in the standard way.
fn format_mode(mode: u32) -> String {
    let file_type = match mode & S_IFMT {
        0o140000 => 's',
        0o120000 => 'l',
        0o100000 => '-',
        0o060000 => 'b',
        0o040000 => 'd',
        0o020000 => 'c',
        0o010000 => 'p',
        _ => '?',
    };

    let mut s = String::with_capacity(10);
    s.push(file_type);

    // Each triple is (read, write, execute-or-special). The execute slot is
    // where setuid/setgid/sticky are shown, in the standard overload: the
    // letter is lowercase when the execute bit is also set and uppercase when
    // it is not, so both bits survive one character.
    let triples = [
        (0o400, 0o200, 0o100, mode & 0o4000 != 0, 's'),
        (0o040, 0o020, 0o010, mode & 0o2000 != 0, 's'),
        (0o004, 0o002, 0o001, mode & 0o1000 != 0, 't'),
    ];
    for (r, w, x, special, letter) in triples {
        s.push(if mode & r != 0 { 'r' } else { '-' });
        s.push(if mode & w != 0 { 'w' } else { '-' });
        s.push(match (mode & x != 0, special) {
            (true, false) => 'x',
            (false, false) => '-',
            (true, true) => letter,
            (false, true) => letter.to_ascii_uppercase(),
        });
    }

    s
}

/// The human-readable file type, as GNU's `%F` prints it.
fn file_type_name(mode: u32) -> &'static str {
    match mode & S_IFMT {
        0o140000 => "socket",
        0o120000 => "symbolic link",
        0o100000 => "regular file",
        0o060000 => "block special file",
        0o040000 => "directory",
        0o020000 => "character special file",
        0o010000 => "fifo",
        _ => "weird file",
    }
}

/// `%F` for a regular file of zero length is `regular empty file`, which is
/// GNU's one special case and the one people grep for.
fn file_type_name_sized(mode: u32, size: u64) -> &'static str {
    if mode & S_IFMT == 0o100000 && size == 0 {
        "regular empty file"
    } else {
        file_type_name(mode)
    }
}

/// Major device number, in the encoding Linux has used since 2.6.
///
/// The obvious `rdev >> 8` is the *pre*-2.6 encoding and silently returns the
/// wrong number for any minor above 255 — which is most of them on a modern
/// system, `/dev/sda17` included.
const fn major(rdev: u64) -> u64 {
    ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff)
}

/// Minor device number; see [`major`] for why this is not `rdev & 0xff`.
const fn minor(rdev: u64) -> u64 {
    (rdev & 0xff) | ((rdev >> 12) & !0xff)
}

// ===========================================================================
// Time formatting
// ===========================================================================

/// Convert a count of days since 1970-01-01 to `(year, month, day)`.
///
/// Howard Hinnant's `civil_from_days`, which is exact for the whole proleptic
/// Gregorian range and, more to the point here, runs in constant time. The
/// version this replaces counted forward a year at a time from 1970, so a file
/// whose mtime was set far into the future — `touch -d` will do that, and so
/// will a corrupt filesystem — made `stat` spin for minutes on a single file.
/// It also gave up entirely on anything at or before the epoch, printing the
/// string `0` for a timestamp of exactly `1970-01-01 00:00:00` and for every
/// legitimate pre-1970 date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the epoch to 0000-03-01 so that the leap day lands at the end of
    // the year and the month-length pattern becomes a simple linear formula.
    let z = days.saturating_add(719_468);
    let era = if z >= 0 { z } else { z.saturating_sub(146_096) } / 146_097;
    let doe = z.saturating_sub(era.saturating_mul(146_097)); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March = 0
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y.saturating_add(1) } else { y };
    // The ranges above are proven by the algorithm; the casts cannot lose data.
    let month = u32::try_from(m).unwrap_or(1);
    let day = u32::try_from(d).unwrap_or(1);
    (year, month, day)
}

/// Format an epoch-seconds/nanoseconds pair the way GNU `stat` does:
/// `YYYY-MM-DD HH:MM:SS.nnnnnnnnn +0000`.
///
/// Negative seconds are real dates, not errors — a file may legitimately carry
/// a pre-1970 timestamp, and an archive tool that restores one is doing its
/// job. The floor division below is what makes them come out right: for
/// `-0.5s` the day must round *down* and the time-of-day must stay positive.
fn format_timestamp(secs: i64, nsec: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (h, m, s) = (rem / 3600, (rem / 60) % 60, rem % 60);
    format!("{year}-{month:02}-{day:02} {h:02}:{m:02}:{s:02}.{nsec:09} +0000")
}

// ===========================================================================
// Argument parsing
// ===========================================================================

/// What to do with the `%` sequences in a format string.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Escapes {
    /// `-c`/`--format`: `\n` is two characters, and a newline is appended.
    Literal,
    /// `--printf`: `\n` is a newline, and nothing is appended.
    Interpret,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct StatArgs {
    dereference: bool,
    filesystem: bool,
    terse: bool,
    format: Option<(String, Escapes)>,
    paths: Vec<String>,
}

/// Parse stat's argv.
///
/// Options are permuted, as GNU's are, so `stat file -L` follows the link.
/// `--` stops that. Anything else beginning with `-` is an error rather than a
/// file name: a `stat` that quietly accepts `-c` as a path is how the previous
/// version turned every scripted use into garbage output plus exit 1.
fn parse_args(args: &[String]) -> Result<StatArgs, String> {
    let mut out = StatArgs {
        dereference: false,
        filesystem: false,
        terse: false,
        format: None,
        paths: Vec::new(),
    };
    let mut end_of_options = false;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if end_of_options {
            out.paths.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--" => end_of_options = true,
            "-L" | "--dereference" => out.dereference = true,
            "-f" | "--file-system" => out.filesystem = true,
            "-t" | "--terse" => out.terse = true,
            "-c" | "--format" => {
                let Some(fmt) = iter.next() else {
                    return Err(format!("option '{arg}' requires an argument"));
                };
                out.format = Some((fmt.clone(), Escapes::Literal));
            }
            other => {
                if let Some(fmt) = other.strip_prefix("--format=") {
                    out.format = Some((fmt.to_string(), Escapes::Literal));
                } else if let Some(fmt) = other.strip_prefix("--printf=") {
                    out.format = Some((fmt.to_string(), Escapes::Interpret));
                } else if other.len() > 1 && other.starts_with('-') {
                    return Err(format!("unrecognized option '{other}'"));
                } else {
                    // A bare `-` is a file named `-`. stat reads no streams,
                    // so it has no reason to give it any other meaning.
                    out.paths.push(other.to_string());
                }
            }
        }
    }

    if out.paths.is_empty() {
        return Err("missing operand".to_string());
    }
    Ok(out)
}

// ===========================================================================
// Format expansion
// ===========================================================================

/// GNU's `-t` format for a file. Bare numbers in a fixed order, for scripts
/// that would otherwise have to parse the human-readable block.
const TERSE_FILE: &str = "%n %s %b %f %u %g %D %i %h %t %T %X %Y %Z %W %o";

/// GNU's `-t` format for `-f`.
const TERSE_FS: &str = "%n %i %l %t %s %S %b %f %a %c %d";

/// Expand a file format string.
///
/// Returns bytes, not a `String`, because `%n` is the file's name and a name is
/// an arbitrary byte string on this OS — forcing it through UTF-8 would corrupt
/// exactly the names a user most needs `stat` to be honest about. `%N` is the
/// quoted form and is safe to render as text by construction.
fn apply_format(fmt: &str, escapes: &Escapes, st: &StatInfo, name: &[u8], link: Option<&[u8]>) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(fmt.len().saturating_mul(2));
    let mut chars = fmt.chars();

    while let Some(c) = chars.next() {
        if c == '\\' && matches!(escapes, Escapes::Interpret) {
            match chars.next() {
                Some('n') => out.push(b'\n'),
                Some('t') => out.push(b'\t'),
                Some('r') => out.push(b'\r'),
                Some('0') => out.push(0),
                Some('\\') => out.push(b'\\'),
                Some(other) => {
                    out.push(b'\\');
                    push_char(&mut out, other);
                }
                None => out.push(b'\\'),
            }
            continue;
        }
        if c != '%' {
            push_char(&mut out, c);
            continue;
        }
        let Some(spec) = chars.next() else {
            // A trailing bare `%`. GNU keeps it.
            out.push(b'%');
            break;
        };
        match spec {
            '%' => out.push(b'%'),
            'a' => push_str(&mut out, &format!("{:o}", st.mode & 0o7777)),
            'A' => push_str(&mut out, &format_mode(st.mode)),
            'b' => push_str(&mut out, &st.blocks.to_string()),
            'B' => push_str(&mut out, "512"),
            'd' => push_str(&mut out, &st.dev.to_string()),
            'D' => push_str(&mut out, &format!("{:x}", st.dev)),
            'f' => push_str(&mut out, &format!("{:x}", st.mode)),
            'F' => push_str(&mut out, file_type_name_sized(st.mode, st.size)),
            'g' => push_str(&mut out, &st.gid.to_string()),
            // %U/%G are the *names*, which need /etc/users.yaml (§353). Until
            // that lookup exists, printing the number is the honest answer: it
            // is right, just less friendly. Printing a guessed name would not.
            'G' => push_str(&mut out, &st.gid.to_string()),
            'h' => push_str(&mut out, &st.nlink.to_string()),
            'i' => push_str(&mut out, &st.ino.to_string()),
            'n' => out.extend_from_slice(name),
            'N' => push_str(&mut out, &quoted_name(name, link)),
            'o' => push_str(&mut out, &st.blksize.to_string()),
            's' => push_str(&mut out, &st.size.to_string()),
            't' => push_str(&mut out, &format!("{:x}", major(st.rdev))),
            'T' => push_str(&mut out, &format!("{:x}", minor(st.rdev))),
            'u' => push_str(&mut out, &st.uid.to_string()),
            'U' => push_str(&mut out, &st.uid.to_string()),
            'w' | 'W' => {
                // Birth time. The VFS does not record one, and GNU prints `-`
                // and `0` respectively when it is unknown rather than inventing
                // one — a fabricated creation date is worse than none.
                push_str(&mut out, if spec == 'w' { "-" } else { "0" });
            }
            'x' => push_str(&mut out, &format_timestamp(st.atime, st.atime_nsec)),
            'X' => push_str(&mut out, &st.atime.to_string()),
            'y' => push_str(&mut out, &format_timestamp(st.mtime, st.mtime_nsec)),
            'Y' => push_str(&mut out, &st.mtime.to_string()),
            'z' => push_str(&mut out, &format_timestamp(st.ctime, st.ctime_nsec)),
            'Z' => push_str(&mut out, &st.ctime.to_string()),
            other => {
                // Unknown specifier: pass it through unchanged, as GNU does,
                // so a format string written for a newer stat degrades to
                // visibly-wrong rather than silently-empty.
                out.push(b'%');
                push_char(&mut out, other);
            }
        }
    }

    out
}

/// Expand a filesystem format string (`-f`).
fn apply_fs_format(fmt: &str, escapes: &Escapes, fs: &FsInfo, name: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(fmt.len().saturating_mul(2));
    let mut chars = fmt.chars();

    while let Some(c) = chars.next() {
        if c == '\\' && matches!(escapes, Escapes::Interpret) {
            match chars.next() {
                Some('n') => out.push(b'\n'),
                Some('t') => out.push(b'\t'),
                Some('\\') => out.push(b'\\'),
                Some(other) => {
                    out.push(b'\\');
                    push_char(&mut out, other);
                }
                None => out.push(b'\\'),
            }
            continue;
        }
        if c != '%' {
            push_char(&mut out, c);
            continue;
        }
        let Some(spec) = chars.next() else {
            out.push(b'%');
            break;
        };
        match spec {
            '%' => out.push(b'%'),
            'a' => push_str(&mut out, &fs.bavail.to_string()),
            'b' => push_str(&mut out, &fs.blocks.to_string()),
            'c' => push_str(&mut out, &fs.files.to_string()),
            'd' => push_str(&mut out, &fs.ffree.to_string()),
            'f' => push_str(&mut out, &fs.bfree.to_string()),
            'i' => push_str(&mut out, &format!("{:x}", fs.fsid)),
            'l' => push_str(&mut out, &fs.namelen.to_string()),
            'n' => out.extend_from_slice(name),
            's' => push_str(&mut out, &fs.bsize.to_string()),
            'S' => push_str(&mut out, &fs.frsize.to_string()),
            // statvfs carries no filesystem-type field, so there is nothing
            // truthful to print for %t/%T. GNU prints `0` and `UNKNOWN` when
            // it cannot tell either.
            't' => push_str(&mut out, "0"),
            'T' => push_str(&mut out, "UNKNOWN"),
            other => {
                out.push(b'%');
                push_char(&mut out, other);
            }
        }
    }

    out
}

/// Expand `fmt`, honouring printf-style widths, and terminate it.
///
/// Widths are applied by expanding the bare specifier on its own and padding
/// the result, rather than by teaching each of the ~30 specifier arms about
/// padding. That keeps them free of formatting concerns and means a new
/// specifier gets width support for nothing.
///
/// `one` is the per-format expander — [`apply_format`] or [`apply_fs_format`]
/// with its data bound — which is what lets this single function serve both
/// modes. Neither expander appends the trailing newline; that decision belongs
/// here, once, so `-c` and `--printf` cannot disagree about it depending on
/// whether the format happened to contain a width.
fn expand(fmt: &str, escapes: &Escapes, one: &dyn Fn(&str, &Escapes) -> Vec<u8>) -> Vec<u8> {
    let mut out = if has_width(fmt) {
        let mut acc = Vec::with_capacity(fmt.len().saturating_mul(2));
        let mut rest = fmt;
        while let Some(pos) = rest.find('%') {
            let (before, after) = rest.split_at(pos);
            acc.extend_from_slice(&one(before, escapes));
            let tail = after.get(1..).unwrap_or("");
            if let Some((left, zero, width, spec, used)) = split_width(tail) {
                let body = one(&format!("%{spec}"), escapes);
                // Padding is a display concern, so it applies only to text. A
                // `%n` holding non-UTF-8 bytes is emitted unpadded rather than
                // mangled — the name matters more than the column.
                match core::str::from_utf8(&body) {
                    Ok(s) => acc.extend_from_slice(pad(s, left, zero, width).as_bytes()),
                    Err(_) => acc.extend_from_slice(&body),
                }
                rest = tail.get(used..).unwrap_or("");
            } else {
                // Not a width qualifier after all. Hand the `%` *and the
                // character after it* back to the expander as one unit, so
                // `%%` and unknown specifiers behave identically on both
                // paths — splitting them would turn `%%` back into two.
                let mut it = tail.chars();
                if let Some(c) = it.next() {
                    acc.extend_from_slice(&one(&format!("%{c}"), escapes));
                    rest = tail.get(c.len_utf8()..).unwrap_or("");
                } else {
                    acc.extend_from_slice(&one("%", escapes));
                    rest = "";
                }
            }
        }
        acc.extend_from_slice(&one(rest, escapes));
        acc
    } else {
        one(fmt, escapes)
    };

    // `-c`/`--format` prints one record per file, so it terminates each;
    // `--printf` prints exactly what it was told to and nothing else.
    if matches!(escapes, Escapes::Literal) {
        out.push(b'\n');
    }
    out
}

/// Whether `fmt` contains at least one width-qualified specifier.
fn has_width(fmt: &str) -> bool {
    let mut rest = fmt;
    while let Some(pos) = rest.find('%') {
        let tail = rest.get(pos.saturating_add(1)..).unwrap_or("");
        if split_width(tail).is_some() {
            return true;
        }
        rest = tail;
    }
    false
}

/// `%N`: the name, shell-quoted, plus `-> target` when it is a symbolic link.
///
/// Uses the shared quoter rather than wrapping the name in `'…'` by hand. A
/// name containing a quote, a newline or a non-UTF-8 byte is exactly the case
/// `%N` exists to make safe, and hand-rolled quoting gets all three wrong.
fn quoted_name(name: &[u8], link: Option<&[u8]>) -> String {
    let head = quoteaf_os(bytes_to_os(name));
    match link {
        Some(t) => format!("{head} -> {}", quoteaf_os(bytes_to_os(t))),
        None => head,
    }
}

#[cfg(unix)]
fn bytes_to_os(b: &[u8]) -> std::ffi::OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(b).to_os_string()
}

/// Host-only fallback. Windows `OsString` is not byte-addressable, so tests on
/// the build host go through UTF-8; the bytes tested there are ASCII, and the
/// real target uses the `cfg(unix)` version above.
#[cfg(not(unix))]
fn bytes_to_os(b: &[u8]) -> std::ffi::OsString {
    std::ffi::OsString::from(String::from_utf8_lossy(b).into_owned())
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

fn push_char(out: &mut Vec<u8>, c: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// The default, human-readable block, as a format string.
///
/// Expressed as a format rather than as a series of `println!`s so that it goes
/// through the same expander as `-c` — one code path means the two cannot drift
/// apart, which is how the old version came to quote the name in its error
/// message but not in its output.
fn default_format() -> String {
    [
        "  File: %N\n",
        "  Size: %-15s Blocks: %-10b IO Block: %-6o %F\n",
        "Device: %Dh/%dd\tInode: %-11i Links: %h\n",
        "Access: (%04a/%A)  Uid: (%5u/%8U)   Gid: (%5g/%8G)\n",
        "Access: %x\nModify: %y\nChange: %z\n Birth: %w",
    ]
    .concat()
}

/// The default `-f` block.
fn default_fs_format() -> String {
    [
        "  File: %N\n",
        "    ID: %-8i Namelen: %-7l Type: %T\n",
        "Block size: %-10s Fundamental block size: %S\n",
        "Blocks: Total: %-10b Free: %-10f Available: %a\n",
        "Inodes: Total: %-10c Free: %d",
    ]
    .concat()
}

// ===========================================================================
// Width handling
// ===========================================================================
//
// The default block above uses `%-15s`-style widths. GNU supports the full
// printf flag/width/precision grammar between the `%` and the specifier; this
// implements the part the default formats need — an optional `-`, an optional
// zero, and a decimal width — and passes anything it does not understand
// through untouched.

/// Split `%[-][0][width]X` into `(flags, width, specifier_char)`.
///
/// Returns `None` when what follows the `%` is not a width-qualified
/// specifier, so the caller can fall back to plain handling.
fn split_width(rest: &str) -> Option<(bool, bool, usize, char, usize)> {
    let mut left = false;
    let mut zero = false;
    let mut width = 0usize;
    let mut saw_digit = false;
    let mut used = 0usize;

    for c in rest.chars() {
        match c {
            // A leading `-` is the left-justify flag only before any digit; a
            // `0` likewise means zero-fill only before any digit, since after
            // one it is part of the width.
            '-' if !left && !zero && !saw_digit => left = true,
            '0' if !zero && !saw_digit => zero = true,
            '0'..='9' => {
                saw_digit = true;
                let d = usize::try_from(c.to_digit(10).unwrap_or(0)).unwrap_or(0);
                width = width.saturating_mul(10).saturating_add(d);
            }
            _ => {
                // Nothing qualified this specifier, so it is an ordinary one
                // and the caller should handle it without padding.
                if !left && !zero && !saw_digit {
                    return None;
                }
                return Some((left, zero, width, c, used.saturating_add(c.len_utf8())));
            }
        }
        used = used.saturating_add(c.len_utf8());
    }
    None
}

/// Pad `body` to `width` per the flags, the way printf would.
fn pad(body: &str, left: bool, zero: bool, width: usize) -> String {
    let len = body.chars().count();
    if len >= width {
        return body.to_string();
    }
    let fill = width.saturating_sub(len);
    if left {
        format!("{body}{}", " ".repeat(fill))
    } else if zero {
        format!("{}{body}", "0".repeat(fill))
    } else {
        format!("{}{body}", " ".repeat(fill))
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("stat: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

#[cfg(unix)]
mod imp {
    use super::{
        Escapes, FsInfo, StatInfo, TERSE_FILE, TERSE_FS, apply_format, apply_fs_format,
        default_format, default_fs_format, expand, parse_args,
    };
    use coreutils::quote::quotef_os;
    use std::env;
    use std::ffi::{CString, OsString};
    use std::fs;
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::process;

    /// The layout the posix crate's `statvfs` writes.
    #[repr(C)]
    #[derive(Default)]
    struct PosixStatvfs {
        f_bsize: u64,
        f_frsize: u64,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_favail: u64,
        f_fsid: u64,
        f_flag: u64,
        f_namemax: u64,
    }

    // SAFETY: `statvfs` is provided by the posix crate with exactly this C
    // signature. It returns 0 on success and -1 with `errno` set on failure.
    unsafe extern "C" {
        fn statvfs(path: *const u8, buf: *mut PosixStatvfs) -> i32;
    }

    pub fn main() {
        let args: Vec<String> = env::args().skip(1).collect();
        let parsed = match parse_args(&args) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("stat: {e}");
                eprintln!("Usage: stat [-L] [-f] [-t] [-c FORMAT] [--] FILE...");
                process::exit(1);
            }
        };

        let (fmt, escapes) = match parsed.format {
            Some((f, e)) => (f, e),
            None if parsed.terse && parsed.filesystem => (TERSE_FS.to_string(), Escapes::Literal),
            None if parsed.terse => (TERSE_FILE.to_string(), Escapes::Literal),
            None if parsed.filesystem => (default_fs_format(), Escapes::Literal),
            None => (default_format(), Escapes::Literal),
        };

        let stdout = io::stdout();
        let mut out = stdout.lock();
        let mut status = 0;

        for path in &parsed.paths {
            let rendered = if parsed.filesystem {
                render_fs(path, &fmt, &escapes, &mut status)
            } else {
                render_file(path, parsed.dereference, &fmt, &escapes, &mut status)
            };
            let Some(bytes) = rendered else { continue };
            // A closed downstream reader — `stat * | head -1` — is an ordinary
            // end to a pipeline, not a failure. Anything else is data the
            // caller will never see and must not be reported as success.
            if let Err(e) = out.write_all(&bytes) {
                if e.kind() == io::ErrorKind::BrokenPipe {
                    process::exit(status);
                }
                eprintln!("stat: write error: {e}");
                process::exit(1);
            }
        }

        if let Err(e) = out.flush()
            && e.kind() != io::ErrorKind::BrokenPipe
        {
            eprintln!("stat: write error: {e}");
            status = 1;
        }
        process::exit(status);
    }

    fn render_file(
        path: &str,
        dereference: bool,
        fmt: &str,
        escapes: &Escapes,
        status: &mut i32,
    ) -> Option<Vec<u8>> {
        let os_path = OsString::from(path);
        // lstat by default. `stat link` answering about the target would make
        // it impossible to ask the one question only `stat` can answer.
        let meta = if dereference {
            fs::metadata(&os_path)
        } else {
            fs::symlink_metadata(&os_path)
        };
        let meta = match meta {
            Ok(m) => m,
            Err(e) => {
                eprintln!("stat: cannot stat {}: {e}", quotef_os(&os_path));
                *status = 1;
                return None;
            }
        };

        let st = StatInfo {
            dev: meta.dev(),
            ino: meta.ino(),
            mode: meta.mode(),
            nlink: meta.nlink(),
            uid: meta.uid(),
            gid: meta.gid(),
            rdev: meta.rdev(),
            size: meta.size(),
            blksize: meta.blksize(),
            blocks: meta.blocks(),
            atime: meta.atime(),
            atime_nsec: u32::try_from(meta.atime_nsec()).unwrap_or(0),
            mtime: meta.mtime(),
            mtime_nsec: u32::try_from(meta.mtime_nsec()).unwrap_or(0),
            ctime: meta.ctime(),
            ctime_nsec: u32::try_from(meta.ctime_nsec()).unwrap_or(0),
        };

        // Read the target only for an actual link. A failure here is not fatal
        // — the rest of the fields are still true — so it degrades to printing
        // the name alone.
        let link: Option<Vec<u8>> = if meta.file_type().is_symlink() {
            fs::read_link(&os_path)
                .ok()
                .map(|t| t.into_os_string().as_bytes().to_vec())
        } else {
            None
        };

        Some(expand(fmt, escapes, &|f, e| {
            apply_format(f, e, &st, os_path.as_bytes(), link.as_deref())
        }))
    }

    fn render_fs(path: &str, fmt: &str, escapes: &Escapes, status: &mut i32) -> Option<Vec<u8>> {
        let os_path = OsString::from(path);
        let Ok(cpath) = CString::new(os_path.as_bytes()) else {
            eprintln!(
                "stat: cannot read filesystem information for {}: path contains a NUL byte",
                quotef_os(&os_path)
            );
            *status = 1;
            return None;
        };
        let mut raw = PosixStatvfs::default();
        // SAFETY: `cpath` is a valid NUL-terminated C string that outlives the
        // call, and `raw` is a valid, writable buffer of the declared layout.
        let ret = unsafe { statvfs(cpath.as_ptr().cast::<u8>(), &raw mut raw) };
        if ret != 0 {
            eprintln!(
                "stat: cannot read filesystem information for {}: {}",
                quotef_os(&os_path),
                io::Error::last_os_error()
            );
            *status = 1;
            return None;
        }
        let fs_info = FsInfo {
            fsid: raw.f_fsid,
            namelen: raw.f_namemax,
            bsize: raw.f_bsize,
            frsize: raw.f_frsize,
            blocks: raw.f_blocks,
            bfree: raw.f_bfree,
            bavail: raw.f_bavail,
            files: raw.f_files,
            ffree: raw.f_ffree,
        };
        Some(expand(fmt, escapes, &|f, e| {
            apply_fs_format(f, e, &fs_info, os_path.as_bytes())
        }))
    }

}

#[cfg(unix)]
fn main() {
    imp::main();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    fn sample() -> StatInfo {
        StatInfo {
            dev: 0x0801,
            ino: 1234,
            mode: 0o100644,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            rdev: 0,
            size: 42,
            blksize: 4096,
            blocks: 8,
            atime: 1_718_454_645,
            atime_nsec: 123_456_789,
            mtime: 1_718_454_645,
            mtime_nsec: 0,
            ctime: 1_718_454_645,
            ctime_nsec: 0,
        }
    }

    /// Expand `f` through the real entry point, the way `main` does.
    fn run(f: &str, escapes: &Escapes, name: &[u8], link: Option<&[u8]>) -> String {
        let st = sample();
        let out = expand(f, escapes, &|piece, e| {
            apply_format(piece, e, &st, name, link)
        });
        String::from_utf8(out).unwrap()
    }

    fn fmt(f: &str) -> String {
        run(f, &Escapes::Interpret, b"file", None)
    }

    // ---- argument parsing --------------------------------------------------
    //
    // The whole of `known-issues.md` → `B-stat-HAS-NO-OPTIONS-AND-CANNOT-READ-
    // A-CLOCK` is that there was no parser here at all, so these are the tests
    // that would have caught it.

    #[test]
    fn dash_c_takes_a_format_and_is_not_a_file() {
        let a = parse_args(&s(&["-c", "%s", "file"])).unwrap();
        assert_eq!(a.format, Some(("%s".to_string(), Escapes::Literal)));
        assert_eq!(a.paths, vec!["file"]);
    }

    #[test]
    fn long_format_and_printf_differ_only_in_escapes() {
        let a = parse_args(&s(&["--format=%s", "f"])).unwrap();
        assert_eq!(a.format, Some(("%s".to_string(), Escapes::Literal)));
        let b = parse_args(&s(&["--printf=%s", "f"])).unwrap();
        assert_eq!(b.format, Some(("%s".to_string(), Escapes::Interpret)));
    }

    #[test]
    fn dash_c_without_an_argument_is_an_error() {
        assert!(parse_args(&s(&["-c"])).unwrap_err().contains("requires"));
    }

    #[test]
    fn dereference_terse_and_filesystem_flags() {
        let a = parse_args(&s(&["-L", "-t", "-f", "x"])).unwrap();
        assert!(a.dereference && a.terse && a.filesystem);
    }

    #[test]
    fn options_may_follow_the_file() {
        let a = parse_args(&s(&["file", "-L"])).unwrap();
        assert!(a.dereference);
        assert_eq!(a.paths, vec!["file"]);
    }

    #[test]
    fn double_dash_makes_a_flag_a_file_name() {
        let a = parse_args(&s(&["--", "-L"])).unwrap();
        assert!(!a.dereference);
        assert_eq!(a.paths, vec!["-L"]);
    }

    #[test]
    fn unknown_option_is_rejected_not_taken_as_a_path() {
        assert!(parse_args(&s(&["-Z", "f"])).unwrap_err().contains("-Z"));
    }

    #[test]
    fn bare_dash_is_a_file_name() {
        assert_eq!(parse_args(&s(&["-"])).unwrap().paths, vec!["-"]);
    }

    #[test]
    fn no_operand_is_an_error() {
        assert!(parse_args(&s(&[])).unwrap_err().contains("missing"));
    }

    // ---- timestamps --------------------------------------------------------

    #[test]
    fn the_epoch_itself_is_a_date_not_the_string_zero() {
        // The old code returned "0" for any `secs <= 0`, so a file stamped at
        // exactly the epoch — which is what a restored archive with no mtime
        // gets — reported no date at all.
        assert_eq!(
            format_timestamp(0, 0),
            "1970-01-01 00:00:00.000000000 +0000"
        );
    }

    #[test]
    fn pre_epoch_timestamps_are_real_dates() {
        assert_eq!(
            format_timestamp(-1, 0),
            "1969-12-31 23:59:59.000000000 +0000"
        );
        assert_eq!(
            format_timestamp(-86_400, 0),
            "1969-12-31 00:00:00.000000000 +0000"
        );
    }

    #[test]
    fn known_dates_round_trip() {
        assert_eq!(
            format_timestamp(1_718_454_645, 0),
            "2024-06-15 12:30:45.000000000 +0000"
        );
        assert_eq!(
            format_timestamp(951_782_400, 0),
            "2000-02-29 00:00:00.000000000 +0000"
        );
        assert_eq!(format_timestamp(1, 0), "1970-01-01 00:00:01.000000000 +0000");
    }

    #[test]
    fn nanoseconds_are_zero_padded_to_nine_digits() {
        assert!(format_timestamp(0, 42).ends_with("00:00:00.000000042 +0000"));
    }

    #[test]
    fn a_far_future_timestamp_returns_immediately() {
        // The old implementation counted forward one year at a time from 1970,
        // so this input spun for minutes. `touch -d` can set it, and so can a
        // corrupt inode; a `stat` that hangs on one file is a denial of service
        // with no attacker cost. If this test ever takes measurable time, the
        // constant-time algorithm has been replaced with a loop again.
        let out = format_timestamp(i64::MAX / 2, 0);
        assert!(out.ends_with(" +0000"));
    }

    #[test]
    fn century_leap_rules() {
        // 2000 is a leap year, 1900 and 2100 are not.
        assert_eq!(civil_from_days(11_017), (2000, 3, 1)); // day after Feb 29
        assert_eq!(civil_from_days(-25_508), (1900, 3, 1)); // day after Feb 28
    }

    // ---- mode --------------------------------------------------------------

    #[test]
    fn mode_basics() {
        assert_eq!(format_mode(0o100755), "-rwxr-xr-x");
        assert_eq!(format_mode(0o040755), "drwxr-xr-x");
        assert_eq!(format_mode(0o120777), "lrwxrwxrwx");
        assert_eq!(format_mode(0o010644), "prw-r--r--");
        assert_eq!(format_mode(0o060660), "brw-rw----");
        assert_eq!(format_mode(0o020660), "crw-rw----");
        assert_eq!(format_mode(0o140755), "srwxr-xr-x");
        assert_eq!(format_mode(0o030755), "?rwxr-xr-x");
        assert_eq!(format_mode(0o100000), "----------");
    }

    #[test]
    fn mode_special_bits_case_encodes_the_execute_bit() {
        assert_eq!(format_mode(0o104755), "-rwsr-xr-x");
        assert_eq!(format_mode(0o104644), "-rwSr--r--");
        assert_eq!(format_mode(0o102755), "-rwxr-sr-x");
        assert_eq!(format_mode(0o102644), "-rw-r-Sr--");
        assert_eq!(format_mode(0o041777), "drwxrwxrwt");
        assert_eq!(format_mode(0o041776), "drwxrwxrwT");
    }

    // ---- device numbers ----------------------------------------------------

    #[test]
    fn device_numbers_use_the_modern_encoding() {
        // /dev/sda1 is 8:1 — the easy case, which the old `>> 8` also got.
        assert_eq!(major(0x0801), 8);
        assert_eq!(minor(0x0801), 1);
        // A minor above 255 is where `rdev & 0xff` silently truncates. Minor
        // 300 = 0x12c: low byte 0x2c, high bits in the 0xfff00 field.
        let rdev = (8u64 << 8) | (300 & 0xff) | ((300 & !0xffu64) << 12);
        assert_eq!(major(rdev), 8);
        assert_eq!(minor(rdev), 300);
    }

    // ---- format expansion --------------------------------------------------

    #[test]
    fn common_specifiers() {
        assert_eq!(fmt("%s"), "42");
        assert_eq!(fmt("%a"), "644");
        assert_eq!(fmt("%A"), "-rw-r--r--");
        assert_eq!(fmt("%i"), "1234");
        assert_eq!(fmt("%h"), "1");
        assert_eq!(fmt("%u %g"), "1000 1000");
        assert_eq!(fmt("%b"), "8");
        assert_eq!(fmt("%B"), "512");
        assert_eq!(fmt("%o"), "4096");
        assert_eq!(fmt("%F"), "regular file");
        assert_eq!(fmt("%n"), "file");
        assert_eq!(fmt("%N"), "'file'");
        assert_eq!(fmt("%Y"), "1718454645");
    }

    #[test]
    fn empty_regular_file_has_its_own_type_name() {
        let mut st = sample();
        st.size = 0;
        let out = apply_format("%F", &Escapes::Interpret, &st, b"f", None);
        assert_eq!(String::from_utf8(out).unwrap(), "regular empty file");
    }

    #[test]
    fn percent_percent_is_a_literal_percent() {
        assert_eq!(fmt("100%%"), "100%");
        // …and stays one when a width elsewhere in the string sends the
        // expansion down the other code path. Splitting `%%` into two pieces
        // there would double it.
        assert_eq!(fmt("%-4s100%%"), "42  100%");
    }

    #[test]
    fn an_unknown_specifier_passes_through() {
        assert_eq!(fmt("%Q"), "%Q");
        assert_eq!(fmt("%-4s%Q"), "42  %Q");
    }

    #[test]
    fn dash_c_appends_a_newline_and_does_not_interpret_escapes() {
        assert_eq!(run("%s\\n", &Escapes::Literal, b"f", None), "42\\n\n");
    }

    #[test]
    fn printf_interprets_escapes_and_appends_nothing() {
        assert_eq!(run("%s\\n", &Escapes::Interpret, b"f", None), "42\n");
    }

    #[test]
    fn escape_handling_does_not_depend_on_whether_a_width_is_present() {
        // The two code paths inside `expand` must agree. They did not at first:
        // the width path interpreted `\n` even under `-c`, so `stat -c '%-4s\n'`
        // and `stat -c '%s\n'` disagreed about their own output format.
        assert_eq!(run("%-4s\\n", &Escapes::Literal, b"f", None), "42  \\n\n");
        assert_eq!(run("%-4s\\n", &Escapes::Interpret, b"f", None), "42  \n");
    }

    #[test]
    fn only_one_newline_is_appended_however_wide_the_format() {
        assert_eq!(run("%-4s", &Escapes::Literal, b"f", None), "42  \n");
    }

    #[test]
    fn percent_n_is_raw_bytes_and_percent_big_n_is_quoted() {
        // A name with a quote in it is exactly what %N exists for, and what
        // hand-rolled `'{name}'` quoting gets wrong.
        let text = run("%N", &Escapes::Interpret, b"it's", None);
        assert_ne!(text, "'it's'", "quoting must not produce an unbalanced quote");
        assert!(text.contains("it"));
    }

    #[test]
    fn a_symlink_shows_its_target() {
        assert_eq!(
            run("%N", &Escapes::Interpret, b"link", Some(b"/etc")),
            "'link' -> '/etc'"
        );
    }

    #[test]
    fn a_non_link_shows_no_arrow() {
        assert!(!fmt("%N").contains("->"));
    }

    // ---- the default block -------------------------------------------------

    #[test]
    fn the_default_block_renders_through_the_same_expander_as_dash_c() {
        // The whole block is one format string precisely so that this can be
        // asserted here rather than only being seen on a running system. The
        // old version built it from six `println!`s, which is why its output
        // quoted nothing while its error messages did.
        assert_eq!(
            run(&default_format(), &Escapes::Literal, b"file", None),
            concat!(
                "  File: 'file'\n",
                "  Size: 42              Blocks: 8          IO Block: 4096   regular file\n",
                "Device: 801h/2049d\tInode: 1234        Links: 1\n",
                "Access: (0644/-rw-r--r--)  Uid: ( 1000/    1000)   Gid: ( 1000/    1000)\n",
                "Access: 2024-06-15 12:30:45.123456789 +0000\n",
                "Modify: 2024-06-15 12:30:45.000000000 +0000\n",
                "Change: 2024-06-15 12:30:45.000000000 +0000\n",
                " Birth: -\n",
            )
        );
    }

    #[test]
    fn the_default_block_shows_where_a_link_points() {
        // `stat link` that does not name the target is answering the one
        // question it was asked with the one fact it withheld.
        let out = run(&default_format(), &Escapes::Literal, b"link", Some(b"/etc"));
        assert!(out.starts_with("  File: 'link' -> '/etc'\n"), "{out}");
    }

    // ---- widths ------------------------------------------------------------

    #[test]
    fn width_parsing() {
        assert_eq!(split_width("-15s rest"), Some((true, false, 15, 's', 4)));
        assert_eq!(split_width("04a"), Some((false, true, 4, 'a', 3)));
        assert_eq!(split_width("s"), None);
        assert_eq!(split_width("%"), None);
    }

    #[test]
    fn padding_directions() {
        assert_eq!(pad("42", true, false, 5), "42   ");
        assert_eq!(pad("42", false, false, 5), "   42");
        assert_eq!(pad("42", false, true, 5), "00042");
        assert_eq!(pad("longer", false, false, 3), "longer");
    }

    // ---- filesystem format -------------------------------------------------

    #[test]
    fn fs_specifiers() {
        let fs = FsInfo {
            fsid: 0xdead,
            namelen: 255,
            bsize: 4096,
            frsize: 4096,
            blocks: 1000,
            bfree: 500,
            bavail: 400,
            files: 100,
            ffree: 90,
        };
        let out = apply_fs_format(
            "%i %l %s %S %b %f %a %c %d",
            &Escapes::Interpret,
            &fs,
            b"/",
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "dead 255 4096 4096 1000 500 400 100 90"
        );
    }

    #[test]
    fn terse_formats_are_the_gnu_ones() {
        // Scripts parse these positionally, so the field order is an interface.
        assert_eq!(TERSE_FILE, "%n %s %b %f %u %g %D %i %h %t %T %X %Y %Z %W %o");
        assert_eq!(TERSE_FS, "%n %i %l %t %s %S %b %f %a %c %d");
    }
}
