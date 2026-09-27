//! libmount's `tab_parse.c`: reading fstab, `/proc/mounts`, mountinfo,
//! `/proc/swaps` and utab files into tables.
//!
//! A file is read a line at a time (`getline`): a `\r` before the newline
//! is dropped, blank lines and `#` lines are skipped, and a table of unknown
//! kind takes its kind from its first line -- two numbers is mountinfo,
//! `Filename\t` is swaps, anything else fstab. A line that does not parse
//! goes to the error callback -- `findmnt` prints `FILE: parse error at line
//! N -- ignored` -- and the rest of the file is read.
//!
//! Upstream's quirks that show in output are kept:
//!
//! * A number field ends at a blank, a tab or the end of the line; one that
//!   runs into anything else is an error. fstab's `freq` and `passno` are
//!   the exception at the end of the line, where one that overflows `long`
//!   is taken anyway, truncated into an `int`; and anything after `passno`
//!   is ignored.
//! * A line with a NUL in it ends at the NUL, but only the file's last line
//!   may: elsewhere the newline cannot be found and the line is an error.
//! * A swaps line that ends after its used size, but with a blank, has
//!   priority 0.
//! * A utab `ID=` that ends the line is no ID, not an error (before a
//!   blank, `strtol` skips the blank, finds no digits, and it is one).
//! * `/dev/root` in mountinfo is replaced by the device its number names in
//!   `/sys/dev/block`, or failing that by the kernel's `root=`.
//!
//! Errors are `errno` values: what `findmnt` passes to `warn` is the global
//! `errno` at the time, not the parser's return code, so that is what these
//! functions report. [`UNCHANGED`] stands for a failure that set no
//! `errno`.

use crate::cache::Cache;
use crate::fs::{Fs, MNT_FS_KERNEL, MNT_FS_MERGED, MNT_FS_SWAP, makedev};
use crate::mangle::unmangle;
use crate::tab::{Direction, Fmt, Table};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

/// `_PATH_PROC_MOUNTINFO`.
pub const PATH_PROC_MOUNTINFO: &[u8] = b"/proc/self/mountinfo";
/// `_PATH_PROC_MOUNTS`.
pub const PATH_PROC_MOUNTS: &[u8] = b"/proc/mounts";
/// A failure upstream returns without setting `errno`: the caller's
/// `warn` then prints whatever `errno` already held.
pub const UNCHANGED: i32 = 0;
/// `EINVAL`.
const EINVAL: i32 = 22;
/// `PATH_DELETED_SUFFIX`: what the kernel appends to a deleted swap file.
const PATH_DELETED_SUFFIX: &[u8] = b" (deleted)";

/// A path from bytes.
fn path_of(b: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(b))
}

/// The environment variable (`safe_getenv`: SlateOS has no setuid
/// programs reading these).
fn env(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|v| quoting::os_bytes(&v).into_owned())
}

/// `mnt_get_fstab_path()`: `LIBMOUNT_FSTAB`, or `/etc/fstab`.
#[must_use]
pub fn fstab_path() -> Vec<u8> {
    env("LIBMOUNT_FSTAB").unwrap_or_else(|| b"/etc/fstab".to_vec())
}

/// `mnt_get_swaps_path()`: `LIBMOUNT_SWAPS`, or `/proc/swaps`.
#[must_use]
pub fn swaps_path() -> Vec<u8> {
    env("LIBMOUNT_SWAPS").unwrap_or_else(|| b"/proc/swaps".to_vec())
}

/// `mnt_get_utab_path()`: `LIBMOUNT_UTAB`, or `/run/mount/utab`.
#[must_use]
pub fn utab_path() -> Vec<u8> {
    env("LIBMOUNT_UTAB").unwrap_or_else(|| b"/run/mount/utab".to_vec())
}

/// The `errno` of an I/O error (`EIO` if it has none).
fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(5)
}

/// `skip_separator(p)`: past blanks and tabs.
fn skip_separator(s: &[u8], at: usize) -> usize {
    let mut i = at;
    while matches!(s.get(i), Some(b' ' | b'\t')) {
        i = i.saturating_add(1);
    }
    i
}

/// `skip_nonspearator(p)`: past a field.
fn skip_nonseparator(s: &[u8], at: usize) -> usize {
    let mut i = at;
    while s.get(i).is_some_and(|&b| b != 0 && b != b' ' && b != b'\t') {
        i = i.saturating_add(1);
    }
    i
}

/// What `next_s32` / `next_u64` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Num<T> {
    /// The string was empty: nothing converted, and `rc` left as it was.
    Empty,
    /// No digits: `rc` is `-EINVAL`, and the position has not moved.
    Invalid,
    /// A number, stored whether or not it is acceptable; `ok` is `rc == 0`
    /// -- it did not overflow `strtol`'s type and ends at a blank, a tab or
    /// the end.
    Value(T, bool),
}

/// The number at `at` as `strtol` (`i64`, saturating) or `strtoumax`
/// (`u64`, saturating, a minus sign negating in two's complement) reads it:
/// the value, whether it overflowed, and where it ended.
fn strto(s: &[u8], at: usize, signed: bool) -> Option<(i128, bool, usize)> {
    let sc = ulstrutils::scan_integer(s.get(at..).unwrap_or_default(), 10)?;
    let end = at.saturating_add(sc.end);
    let m = i128::try_from(sc.magnitude).unwrap_or(i128::MAX);
    let (v, erange) = if signed {
        let limit = if sc.negative {
            1i128 << 63
        } else {
            (1i128 << 63) - 1
        };
        if sc.saturated || m > limit {
            (
                if sc.negative {
                    -(1i128 << 63)
                } else {
                    (1i128 << 63) - 1
                },
                true,
            )
        } else {
            (if sc.negative { m.saturating_neg() } else { m }, false)
        }
    } else if sc.saturated || m > i128::from(u64::MAX) {
        (i128::from(u64::MAX), true)
    } else {
        let u = u64::try_from(m).unwrap_or(u64::MAX);
        (
            i128::from(if sc.negative { u.wrapping_neg() } else { u }),
            false,
        )
    };
    Some((v, erange, end))
}

/// Whether a number ending at `end` ends where upstream wants it to.
fn ends_field(s: &[u8], end: usize) -> bool {
    matches!(s.get(end), None | Some(b' ' | b'\t' | 0))
}

/// `next_s32(s, &num, &rc)`: a `strtol` number at `at`, stored in an `int`
/// (truncated). Returns where the scan stopped and what it found.
fn next_s32(s: &[u8], at: usize) -> (usize, Num<i32>) {
    if s.get(at).is_none_or(|&b| b == 0) {
        return (at, Num::Empty);
    }
    let Some((v, erange, end)) = strto(s, at, true) else {
        return (at, Num::Invalid);
    };
    // `*num = strtol(...)`: a long, truncated into an int.
    let v = i64::try_from(v).unwrap_or(0) as i32;
    (end, Num::Value(v, !erange && ends_field(s, end)))
}

/// `next_u64(s, &num, &rc)`: `strtoumax`, likewise.
fn next_u64(s: &[u8], at: usize) -> (usize, Num<u64>) {
    if s.get(at).is_none_or(|&b| b == 0) {
        return (at, Num::Empty);
    }
    let Some((v, erange, end)) = strto(s, at, false) else {
        return (at, Num::Invalid);
    };
    let v = u64::try_from(v).unwrap_or(u64::MAX);
    (end, Num::Value(v, !erange && ends_field(s, end)))
}

/// The field at `at`, unmangled, and where it ended.
fn field(s: &[u8], at: usize) -> (Option<Vec<u8>>, usize) {
    let (v, len) = unmangle(s.get(at..).unwrap_or_default());
    (v, at.saturating_add(len))
}

/// Whether the line has anything at `at`.
fn more(s: &[u8], at: usize) -> bool {
    s.get(at).is_some_and(|&b| b != 0)
}

/// `mnt_parse_table_line(fs, s)`: `SOURCE TARGET TYPE [OPTIONS [FREQ
/// [PASSNO]]]`.
fn parse_table_line(fs: &mut Fs, s: &[u8]) -> Result<(), ()> {
    fs.passno = 0;
    fs.freq = 0;
    let (src, at) = field(s, 0);
    fs.set_source(Some(src.ok_or(())?));
    let at = skip_separator(s, at);
    let (target, at) = field(s, at);
    fs.target = Some(target.ok_or(())?);
    let at = skip_separator(s, at);
    let (ty, at) = field(s, at);
    fs.set_fstype(Some(ty.ok_or(())?));
    let at = skip_separator(s, at);
    let (opts, at) = field(s, at);
    let Some(opts) = opts else {
        return Ok(());
    };
    fs.set_options(Some(&opts)).map_err(|_| ())?;
    // `freq` then `passno`: each optional, and taken even when it overflowed,
    // if it ends the line.
    let mut at = at;
    for slot in [&mut fs.freq, &mut fs.passno] {
        at = skip_separator(s, at);
        if !more(s, at) {
            return Ok(());
        }
        let (end, num) = next_s32(s, at);
        let ok = match num {
            Num::Value(v, ok) => {
                *slot = v;
                ok
            }
            Num::Invalid => {
                *slot = 0;
                false
            }
            Num::Empty => true,
        };
        if more(s, end) && !ok {
            return Err(());
        }
        at = end;
    }
    Ok(())
}

/// `sscanf(s, "%u:%u", &maj, &min) == 2`: each `%u` is `strtoul` into an
/// `unsigned int`, after any white space.
fn scan_majmin(s: &[u8]) -> Option<(u32, u32)> {
    let (maj, _, end) = strto(s, 0, false)?;
    if s.get(end) != Some(&b':') {
        return None;
    }
    let (min, _, _) = strto(s, end.saturating_add(1), false)?;
    // Truncated into an `unsigned int`, as `%u` stores it.
    let t = |v: i128| u64::try_from(v).unwrap_or(u64::MAX) as u32;
    Some((t(maj), t(min)))
}

/// `mnt_parse_mountinfo_line(fs, s)`.
fn parse_mountinfo_line(fs: &mut Fs, s: &[u8]) -> Result<(), ()> {
    fs.flags |= MNT_FS_KERNEL;
    let mut at = 0usize;
    for slot in [&mut fs.id, &mut fs.parent] {
        let (end, num) = next_s32(s, at);
        match num {
            Num::Value(v, true) if more(s, end) => *slot = v,
            _ => return Err(()),
        }
        at = skip_separator(s, end);
    }
    let (maj, min) = scan_majmin(s.get(at..).unwrap_or_default()).ok_or(())?;
    fs.devno = makedev(maj, min);
    let at = skip_nonseparator(s, at);
    let at = skip_separator(s, at);
    let (root, at) = field(s, at);
    fs.root = Some(root.ok_or(())?);
    let at = skip_separator(s, at);
    let (target, at) = field(s, at);
    fs.target = Some(target.ok_or(())?);
    let at = skip_separator(s, at);
    let (vfs, at) = field(s, at);
    fs.vfs_optstr = Some(vfs.ok_or(())?);
    // The optional fields end at " - ".
    let rest = s.get(at..).unwrap_or_default();
    let sep = rest.windows(3).position(|w| w == b" - ").ok_or(())?;
    if sep > 1 {
        fs.opt_fields = Some(rest.get(1..sep).unwrap_or_default().to_vec());
    }
    let at = skip_separator(s, at.saturating_add(sep).saturating_add(3));
    let (ty, at) = field(s, at);
    fs.set_fstype(Some(ty.ok_or(())?));
    // The source, which is empty when two blanks follow the type.
    let at = if !more(s, at) {
        return Err(());
    } else if s.get(at..).unwrap_or_default().starts_with(b"  ") {
        fs.set_source(Some(Vec::new()));
        at
    } else {
        let at = skip_separator(s, at);
        let (src, at) = field(s, at);
        fs.set_source(Some(src.ok_or(())?));
        at
    };
    let at = skip_separator(s, at);
    let (fsopts, _) = field(s, at);
    fs.fs_optstr = Some(fsopts.ok_or(())?);
    fs.optstr = Some(fs.strdup_options().ok_or(())?);
    Ok(())
}

/// `mnt_parse_utab_line(fs, s)`: `NAME=value` pairs, the first of each
/// name taken and the rest skipped as unknown.
fn parse_utab_line(fs: &mut Fs, s: &[u8]) -> Result<(), ()> {
    let mut p = 0usize;
    while more(s, p) {
        while s.get(p) == Some(&b' ') {
            p = p.saturating_add(1);
        }
        if !more(s, p) {
            break;
        }
        let rest = s.get(p..).unwrap_or_default();
        let value = |prefix: &[u8]| -> Option<(Option<Vec<u8>>, usize)> {
            rest.starts_with(prefix)
                .then(|| field(s, p.saturating_add(prefix.len())))
        };
        let end: Option<usize> = if fs.id == 0 && rest.starts_with(b"ID=") {
            let (end, num) = next_s32(s, p.saturating_add(3));
            match num {
                Num::Value(v, true) => fs.id = v,
                Num::Empty => {}
                Num::Value(..) | Num::Invalid => return Err(()),
            }
            Some(end)
        } else if fs.source.is_none()
            && let Some((v, end)) = value(b"SRC=")
        {
            fs.set_source(Some(v.ok_or(())?));
            Some(end)
        } else if fs.target.is_none()
            && let Some((v, end)) = value(b"TARGET=")
        {
            fs.target = Some(v.ok_or(())?);
            Some(end)
        } else if fs.root.is_none()
            && let Some((v, end)) = value(b"ROOT=")
        {
            fs.root = Some(v.ok_or(())?);
            Some(end)
        } else if fs.bindsrc.is_none()
            && let Some((v, end)) = value(b"BINDSRC=")
        {
            fs.bindsrc = Some(v.ok_or(())?);
            Some(end)
        } else if fs.user_optstr.is_none()
            && let Some((v, end)) = value(b"OPTS=")
        {
            fs.user_optstr = Some(v.ok_or(())?);
            Some(end)
        } else if fs.attrs.is_none()
            && let Some((v, end)) = value(b"ATTRS=")
        {
            fs.attrs = Some(v.ok_or(())?);
            Some(end)
        } else {
            // An unknown variable (or a second of a known one).
            while s.get(p).is_some_and(|&b| b != 0 && b != b' ') {
                p = p.saturating_add(1);
            }
            None
        };
        if let Some(end) = end {
            p = end;
        }
    }
    Ok(())
}

/// `mnt_parse_swaps_line(fs, s)`: `Filename Type Size Used Priority`.
fn parse_swaps_line(fs: &mut Fs, s: &[u8]) -> Result<(), ()> {
    let (src, at) = field(s, 0);
    let mut src = src.ok_or(())?;
    if src.ends_with(PATH_DELETED_SUFFIX) {
        src.truncate(src.len().saturating_sub(PATH_DELETED_SUFFIX.len()));
    }
    fs.set_source(Some(src));
    let at = skip_separator(s, at);
    let (ty, at) = field(s, at);
    fs.swaptype = Some(ty.ok_or(())?);
    let mut at = skip_separator(s, at);
    for slot in [&mut fs.size, &mut fs.usedsize] {
        let (end, num) = next_u64(s, at);
        match num {
            // `off_t`, from a `uint64_t`.
            Num::Value(v, true) if more(s, end) => *slot = v as i64,
            _ => return Err(()),
        }
        at = skip_separator(s, end);
    }
    // The priority: with nothing left, `next_s32` leaves `rc` at the used
    // size's 0, and the priority at 0.
    match next_s32(s, at).1 {
        Num::Value(v, true) => fs.priority = v,
        Num::Empty => {}
        Num::Value(..) | Num::Invalid => return Err(()),
    }
    fs.set_fstype(Some(b"swap".to_vec()));
    Ok(())
}

/// `sscanf(line, "%u %u", &a, &b) == 2`: two numbers, with any white space
/// (or none, where a sign starts the second) between them.
fn two_numbers(line: &[u8]) -> bool {
    let Some((_, _, end)) = strto(line, 0, false) else {
        return false;
    };
    strto(line, end, false).is_some()
}

/// `guess_table_format(line)`.
fn guess_table_format(line: &[u8]) -> Fmt {
    if two_numbers(line) {
        return Fmt::Mountinfo;
    }
    if line.starts_with(b"Filename\t") {
        return Fmt::Swaps;
    }
    Fmt::Fstab
}

/// The error callback: given the file name and line number, whether the
/// bad line is forgiven (a positive return) or fatal (negative). Without
/// one every error is forgiven.
pub type ErrCb<'a> = &'a mut dyn FnMut(&[u8], usize) -> i32;

/// `struct libmnt_parser`.
struct Parser<'c, R: BufRead> {
    reader: R,
    filename: Vec<u8>,
    line: usize,
    /// `feof(f)`: the last read reached the end of the file.
    eof: bool,
    /// `sysroot_rc`: whether `/dev/root` has been guessed at.
    sysroot_guessed: bool,
    /// `sysroot`: the guess, if it found anything.
    sysroot: Option<Vec<u8>>,
    errcb: Option<ErrCb<'c>>,
}

/// What `mnt_table_parse_next` returned.
enum Next {
    /// An entry.
    Fs(Box<Fs>),
    /// A bad line the callback forgave.
    Forgiven,
    /// End of file, a read error (`errno`), or a fatal bad line.
    Fatal(i32),
}

/// `mnt_table_parse_next(pa, tb, fs)`.
fn parse_next<R: BufRead>(pa: &mut Parser<'_, R>, tb: &mut Table) -> Next {
    loop {
        let mut buf = Vec::new();
        match pa.reader.read_until(b'\n', &mut buf) {
            Ok(0) => {
                pa.eof = true;
                return Next::Fatal(EINVAL);
            }
            Ok(_) => {}
            Err(e) => return Next::Fatal(errno_of(&e)),
        }
        pa.line = pa.line.saturating_add(1);
        // `getline` stops at a newline; without one it reached the end.
        if buf.last() != Some(&b'\n') {
            pa.eof = true;
        }
        // `strchr(buf, '\n')`, which a NUL before the newline hides; then,
        // only at the end of the file, `memchr(buf, '\0')`.
        let first = buf.iter().position(|&b| b == b'\n' || b == 0);
        let end = match first {
            Some(i) if buf.get(i) == Some(&b'\n') => i,
            _ if pa.eof => crate::c_str(&buf).len(),
            _ => return report(pa),
        };
        buf.truncate(end);
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        let start = buf
            .iter()
            .position(|&b| b != b' ' && b != b'\t')
            .unwrap_or(buf.len());
        let s = buf.get(start..).unwrap_or_default();
        if s.is_empty() || s.first() == Some(&b'#') {
            continue;
        }
        if tb.fmt == Fmt::Guess {
            tb.fmt = guess_table_format(s);
            if tb.fmt == Fmt::Swaps {
                // The header.
                continue;
            }
        }
        let mut fs = Box::default();
        let rc = match tb.fmt {
            Fmt::Fstab | Fmt::Mtab => parse_table_line(&mut fs, s),
            Fmt::Mountinfo => parse_mountinfo_line(&mut fs, s),
            Fmt::Utab => parse_utab_line(&mut fs, s),
            Fmt::Swaps => {
                if s.starts_with(b"Filename\t") {
                    continue;
                }
                parse_swaps_line(&mut fs, s)
            }
            Fmt::Guess => Err(()),
        };
        return match rc {
            Ok(()) => Next::Fs(fs),
            Err(()) => report(pa),
        };
    }
}

/// A line that did not parse, to the callback.
fn report<R: BufRead>(pa: &mut Parser<'_, R>) -> Next {
    let rc = match pa.errcb.as_mut() {
        Some(cb) => cb(&pa.filename, pa.line),
        None => 1,
    };
    if rc > 0 {
        Next::Forgiven
    } else {
        Next::Fatal(UNCHANGED)
    }
}

/// `path_to_tid(filename)`: the number in `/proc/<tid>/mountinfo`'s
/// canonical path, or 0.
fn path_to_tid(filename: &[u8]) -> i32 {
    let Some(path) = crate::cache::canonicalize_path(filename) else {
        return 0;
    };
    let Some(slash) = path.iter().rposition(|&b| b == b'/') else {
        return 0;
    };
    let dir = path.get(..slash).unwrap_or_default();
    let Some(slash) = dir.iter().rposition(|&b| b == b'/') else {
        return 0;
    };
    let num = dir.get(slash.saturating_add(1)..).unwrap_or_default();
    // `strtol` into a `pid_t`, refusing an overflow or anything left over.
    match strto(num, 0, true) {
        Some((v, false, end)) if end == num.len() => i64::try_from(v).unwrap_or(0) as i32,
        _ => 0,
    }
}

/// `sysfs_devno_to_devpath(devno)`: `/dev/NAME` for a block device number,
/// from where `/sys/dev/block/MAJ:MIN` links -- if that file is a block
/// device with that number.
pub(crate) fn devno_to_devpath(devno: u64) -> Option<Vec<u8>> {
    // `"%d:%d"`: the two `unsigned int`s printed signed.
    let link = format!(
        "/sys/dev/block/{}:{}",
        crate::fs::major(devno) as i32,
        crate::fs::minor(devno) as i32
    );
    let target = std::fs::read_link(link).ok()?;
    let target = quoting::os_bytes(target.as_os_str()).into_owned();
    let slash = target.iter().rposition(|&b| b == b'/')?;
    let name = target.get(slash.saturating_add(1)..)?;
    let mut path = b"/dev/".to_vec();
    // `sysfs_devname_sys_to_dev`: sysfs spells a `/` in a name `!`.
    path.extend(name.iter().map(|&b| if b == b'!' { b'/' } else { b }));
    is_block_device_numbered(&path, devno).then_some(path)
}

/// `stat(path)` is a block device and its `st_rdev` is `devno`.
fn is_block_device_numbered(path: &[u8], devno: u64) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};
        std::fs::metadata(path_of(path))
            .is_ok_and(|m| m.file_type().is_block_device() && m.rdev() == devno)
    }
    #[cfg(not(unix))]
    {
        // No block devices to find where there is no `/sys`.
        let _ = (path, devno);
        false
    }
}

/// `mnt_get_kernel_cmdline_option(name)` for a `name=` option: its value
/// on the kernel command line's first line, the last one given.
fn kernel_cmdline_option(name: &[u8]) -> Option<Vec<u8>> {
    let data = std::fs::read("/proc/cmdline").ok()?;
    // `fgets(buf, BUFSIZ)`: the first line, at most 8191 bytes, as a C string.
    let line_end = data
        .iter()
        .position(|&b| b == b'\n')
        .map_or(data.len(), |i| i.saturating_add(1));
    let mut buf = crate::c_str(data.get(..line_end.min(8191)).unwrap_or_default()).to_vec();
    if buf.is_empty() || buf.first() == Some(&b'\n') {
        return None;
    }
    match buf.windows(4).position(|w| w == b" -- ") {
        Some(i) => buf.truncate(i),
        // The last byte, which it takes to be the newline.
        None => {
            buf.pop();
        }
    }
    let blank = |b: u8| b == b' ' || b == b'\t';
    let mut res = None;
    let mut p = 0usize;
    while p < buf.len() {
        let Some(off) = buf
            .get(p..)
            .and_then(|t| t.windows(name.len()).position(|w| w == name))
        else {
            break;
        };
        p = p.saturating_add(off);
        if p != 0 && !buf.get(p.wrapping_sub(1)).copied().is_some_and(blank) {
            p = p.saturating_add(1);
            continue;
        }
        let v = p.saturating_add(name.len());
        let mut q = p;
        while buf.get(q).is_some_and(|&b| !blank(b)) {
            q = q.saturating_add(1);
        }
        res = Some(buf.get(v..q).unwrap_or_default().to_vec());
        if q >= buf.len() {
            break;
        }
        // The blank becomes the value's NUL -- so an option straight after
        // it has no blank before it any more.
        if let Some(b) = buf.get_mut(q) {
            *b = 0;
        }
        p = q.saturating_add(1);
    }
    res
}

/// `mnt_guess_system_root(devno, cache, &path)`: what `/dev/root` really
/// is -- from its device number, or from the kernel's `root=`.
fn guess_system_root(devno: u64) -> Option<Vec<u8>> {
    if crate::fs::major(devno) > 0
        && let Some(dev) = devno_to_devpath(devno)
    {
        return Some(dev);
    }
    let spec = kernel_cmdline_option(b"root=")?;
    if let Some((x, y)) = scan_majmin(&spec) {
        return devno_to_devpath(makedev(x, y));
    }
    if !spec.is_empty() && spec.iter().all(u8::is_ascii_hexdigit) {
        // `strtoul(spec, 16)` into a `uint32_t`; an overflow is no guess.
        let sc = ulstrutils::scan_integer(&spec, 16)?;
        if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
            return None;
        }
        let n = u64::try_from(sc.magnitude).unwrap_or(0) as u32;
        // The kernel's `new_decode_dev()`.
        let x = (n & 0xfff00) >> 8;
        let y = (n & 0xff) | ((n >> 12) & 0xfff00);
        return devno_to_devpath(makedev(x, y));
    }
    // A device name or a tag. The table has no cache while it is parsed.
    Cache::new().resolve_spec(&spec)
}

/// `mnt_table_parse_stream(tb, f, filename)`: every entry of the stream
/// appended to the table.
fn parse_stream<R: BufRead>(
    tb: &mut Table,
    reader: R,
    filename: &[u8],
    errcb: Option<ErrCb<'_>>,
) -> Result<(), i32> {
    let mut pa = Parser {
        reader,
        filename: filename.to_vec(),
        line: 0,
        eof: false,
        sysroot_guessed: false,
        sysroot: None,
        errcb,
    };
    // Decided before a guessed format is known, as upstream decides it.
    let flags = if tb.fmt == Fmt::Swaps {
        MNT_FS_SWAP
    } else if filename == PATH_PROC_MOUNTS {
        MNT_FS_KERNEL
    } else {
        0
    };
    let mut tid: Option<i32> = None;
    loop {
        if pa.eof {
            return Ok(());
        }
        let mut fs = match parse_next(&mut pa, tb) {
            Next::Fs(fs) => fs,
            Next::Forgiven => continue,
            // A fatal error at the end of the file is the end of the file.
            Next::Fatal(_) if pa.eof => return Ok(()),
            Next::Fatal(errno) => return Err(errno),
        };
        fs.flags |= flags;
        if tb.fmt == Fmt::Mountinfo {
            fs.tid = *tid.get_or_insert_with(|| path_to_tid(filename));
            if fs.srcpath() == Some(b"/dev/root") {
                // Guessed once: a container's mountinfo can have many.
                if !pa.sysroot_guessed {
                    pa.sysroot = guess_system_root(fs.devno);
                    pa.sysroot_guessed = true;
                }
                if let Some(real) = pa.sysroot.clone() {
                    fs.set_source(Some(real));
                }
            }
        }
        tb.ents.push(*fs);
    }
}

/// `rewind(f)` and `mnt_table_parse_stream(tb, f, filename)`: `file` read
/// again from its start into the table -- `findmnt --poll`'s reading of the
/// mount table each time it changes.
///
/// # Errors
///
/// The seek, or a read, failed: the `errno`.
pub fn parse_stream_from_start(
    tb: &mut Table,
    file: &std::fs::File,
    filename: &[u8],
    errcb: Option<ErrCb<'_>>,
) -> Result<(), i32> {
    use std::io::Seek;
    let mut f = file;
    f.seek(std::io::SeekFrom::Start(0))
        .map_err(|e| errno_of(&e))?;
    parse_stream(tb, BufReader::new(f), filename, errcb)
}

/// `mnt_table_parse_file(tb, filename)`.
///
/// # Errors
///
/// The file will not open, or a read of it failed: the `errno`.
pub fn parse_file(tb: &mut Table, filename: &[u8], errcb: Option<ErrCb<'_>>) -> Result<(), i32> {
    let file = std::fs::File::open(path_of(filename)).map_err(|e| errno_of(&e))?;
    parse_stream(tb, BufReader::new(file), filename, errcb)
}

/// `mnt_table_parse_dir(tb, dirname)`: every `*.fstab` file of the
/// directory not starting with `.`, in `versionsort` order, each named in
/// errors by its name alone. A file that will not open or read is skipped.
///
/// # Errors
///
/// The directory will not open.
pub fn parse_dir(tb: &mut Table, dirname: &[u8], mut errcb: Option<ErrCb<'_>>) -> Result<(), i32> {
    let dir = std::fs::read_dir(path_of(dirname)).map_err(|e| errno_of(&e))?;
    let mut names: Vec<Vec<u8>> = Vec::new();
    for e in dir {
        let Ok(e) = e else {
            continue;
        };
        let name = quoting::os_bytes(&e.file_name()).into_owned();
        // `mnt_table_parse_dir_filter`: a regular file, a link or unknown.
        let kind_ok = e.file_type().is_ok_and(|t| t.is_file() || t.is_symlink());
        if !kind_ok
            || name.first() == Some(&b'.')
            || name.len() < b".fstab".len().saturating_add(1)
            || !name.ends_with(b".fstab")
        {
            continue;
        }
        names.push(name);
    }
    names.sort_by(|a, b| ulstrutils::strverscmp(a, b));
    for name in names {
        let mut p = dirname.to_vec();
        p.push(b'/');
        p.extend_from_slice(&name);
        let path = path_of(&p);
        if !std::fs::metadata(&path).is_ok_and(|m| m.is_file()) {
            continue;
        }
        if let Ok(f) = std::fs::File::open(&path) {
            let cb: Option<ErrCb<'_>> = match errcb.as_mut() {
                Some(cb) => Some(&mut **cb),
                None => None,
            };
            // Upstream ignores what a file's parse returns.
            let _ = parse_stream(tb, BufReader::new(f), &name, cb);
        }
    }
    Ok(())
}

/// `mnt_table_parse_fstab(tb, filename)`: a file, or a directory of them;
/// `None` for `LIBMOUNT_FSTAB` or `/etc/fstab`.
///
/// # Errors
///
/// It cannot be `stat`ed (its `errno`), is neither a file nor a directory
/// ([`UNCHANGED`]), or will not open.
pub fn parse_fstab(
    tb: &mut Table,
    filename: Option<&[u8]>,
    errcb: Option<ErrCb<'_>>,
) -> Result<(), i32> {
    let filename = filename.map_or_else(fstab_path, <[u8]>::to_vec);
    let meta = std::fs::metadata(path_of(&filename)).map_err(|e| errno_of(&e))?;
    tb.fmt = Fmt::Fstab;
    if meta.is_file() {
        parse_file(tb, &filename, errcb)
    } else if meta.is_dir() {
        parse_dir(tb, &filename, errcb)
    } else {
        Err(UNCHANGED)
    }
}

/// `mnt_table_parse_swaps(tb, filename)`: `None` for `LIBMOUNT_SWAPS` or
/// `/proc/swaps`.
///
/// # Errors
///
/// As [`parse_file`].
pub fn parse_swaps(
    tb: &mut Table,
    filename: Option<&[u8]>,
    errcb: Option<ErrCb<'_>>,
) -> Result<(), i32> {
    let filename = filename.map_or_else(swaps_path, <[u8]>::to_vec);
    tb.fmt = Fmt::Swaps;
    parse_file(tb, &filename, errcb)
}

/// `mnt_table_merge_user_fs(tb, uf)`: a utab entry's userspace options,
/// attributes and bind source onto the last kernel entry for the same
/// mount -- by ID when both have one, else by root, mount point and source.
fn merge_user_fs(tb: &mut Table, uf: &Fs) {
    let (Some(src), Some(target), Some(root)) =
        (uf.srcpath(), uf.target.as_deref(), uf.root.as_deref())
    else {
        return;
    };
    if uf.attrs.is_none() && uf.user_optstr.is_none() {
        return;
    }
    let id = uf.id;
    let found = tb.order(Direction::Backward).into_iter().find(|&i| {
        let Some(fs) = tb.ents.get(i) else {
            return false;
        };
        if fs.flags & MNT_FS_MERGED != 0 {
            return false;
        }
        if id > 0 && fs.id != 0 {
            fs.id == id
        } else {
            fs.root.as_deref() == Some(root)
                && fs.streq_target(Some(target))
                && fs.streq_srcpath(Some(src))
        }
    });
    if let Some(i) = found
        && let Some(fs) = tb.ents.get_mut(i)
    {
        // Upstream ignores a malformed option string here.
        let _ = fs.append_options(uf.user_optstr.as_deref());
        if let Some(a) = &uf.attrs {
            crate::optstr::append_option(&mut fs.attrs, a, None);
        }
        fs.bindsrc.clone_from(&uf.bindsrc);
        fs.flags |= MNT_FS_MERGED;
    }
}

/// `mnt_table_parse_mtab(tb, filename)` (`__mnt_table_parse_mountinfo`):
/// the kernel's table -- `/proc/self/mountinfo`, or `/proc/mounts` when that
/// will not read and no file was named -- with utab's userspace options
/// merged in when it is mountinfo.
///
/// # Errors
///
/// As [`parse_file`], for the named file or for `/proc/mounts`.
pub fn parse_mtab(
    tb: &mut Table,
    filename: Option<&[u8]>,
    mut errcb: Option<ErrCb<'_>>,
) -> Result<(), i32> {
    let explicit = filename.is_some();
    let filename: &[u8] = match filename {
        None => PATH_PROC_MOUNTINFO,
        Some(f) => f,
    };
    tb.fmt = if filename == PATH_PROC_MOUNTINFO {
        Fmt::Mountinfo
    } else {
        Fmt::Guess
    };
    let cb: Option<ErrCb<'_>> = match errcb.as_mut() {
        Some(cb) => Some(&mut **cb),
        None => None,
    };
    if let Err(e) = parse_file(tb, filename, cb) {
        if explicit {
            return Err(e);
        }
        tb.fmt = Fmt::Mtab;
        return parse_file(tb, PATH_PROC_MOUNTS, errcb);
    }
    if !tb.is_mountinfo() || tb.ents.is_empty() {
        return Ok(());
    }
    let utab = utab_path();
    // `is_file_empty`: missing counts as empty.
    if std::fs::metadata(path_of(&utab)).map_or(true, |m| m.len() == 0) {
        return Ok(());
    }
    let mut u = Table {
        fmt: Fmt::Utab,
        ents: Vec::new(),
    };
    if parse_file(&mut u, &utab, None).is_ok() {
        for i in u.order(Direction::Backward) {
            if let Some(uf) = u.ents.get(i) {
                merge_user_fs(tb, uf);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    reason = "tests index tables they have just built"
)]
mod tests {
    use super::*;

    fn parse(text: &[u8], fmt: Fmt) -> (Table, Vec<usize>) {
        let mut tb = Table {
            fmt,
            ents: Vec::new(),
        };
        let mut errors = Vec::new();
        let mut cb = |_: &[u8], line: usize| {
            errors.push(line);
            1
        };
        let rc = parse_stream(&mut tb, text, b"test", Some(&mut cb));
        assert_eq!(rc, Ok(()));
        (tb, errors)
    }

    #[test]
    fn mountinfo_lines_parse() {
        let (tb, errs) = parse(
            b"21 1 8:3 / / rw,relatime shared:1 - ext4 /dev/sda3 rw,errors=remount-ro\n\
              22 21 0:5 / /proc rw,nosuid - proc proc rw\n\
              23 21 0:6 /x /mnt/my\\040disk ro master:2 - tmpfs  rw,size=10k\n",
            Fmt::Guess,
        );
        assert!(errs.is_empty());
        assert_eq!(tb.fmt, Fmt::Mountinfo);
        assert_eq!(tb.ents.len(), 3);
        let f = &tb.ents[0];
        assert_eq!((f.id, f.parent, f.devno), (21, 1, makedev(8, 3)));
        assert_eq!(f.opt_fields.as_deref(), Some(&b"shared:1"[..]));
        assert_eq!(f.options(), Some(&b"rw,relatime,errors=remount-ro"[..]));
        assert_eq!(tb.ents[1].opt_fields, None);
        assert_eq!(tb.ents[2].target.as_deref(), Some(&b"/mnt/my disk"[..]));
        assert_eq!(tb.ents[2].source.as_deref(), Some(&b""[..]));
        assert_eq!(tb.ents[2].options(), Some(&b"ro,size=10k"[..]));
    }

    #[test]
    fn mountinfo_needs_the_separator_and_clean_numbers() {
        let (tb, errs) = parse(
            b"1 0 8:1 / / rw - ext4 /dev/sda1 rw\n\
              2x 1 8:1 / /a rw - ext4 /dev/sda1 rw\n\
              3 1 8:1 / /b rw ext4 /dev/sda1 rw\n\
              4 1 8 / /c rw - ext4 /dev/sda1 rw\n\
              5 1 8:1 / /d rw - ext4\n",
            Fmt::Guess,
        );
        assert_eq!(errs, vec![2, 3, 4, 5]);
        assert_eq!(tb.ents.len(), 1);
    }

    #[test]
    fn fstab_lines_parse_and_errors_are_reported() {
        let (tb, errs) = parse(
            b"# comment\n\n  UUID=abc / ext4 defaults 0 1\r\n/dev/sdb1 /x xfs\nbad\n/dev/sdc /y ext4 rw x\n",
            Fmt::Guess,
        );
        assert_eq!(tb.fmt, Fmt::Fstab);
        assert_eq!(errs, vec![5, 6]);
        assert_eq!(tb.ents.len(), 2);
        assert_eq!(tb.ents[0].tag(), Some((&b"UUID"[..], &b"abc"[..])));
        assert_eq!(tb.ents[0].passno, 1);
        assert_eq!(tb.ents[1].options(), None);
    }

    #[test]
    fn fstab_numbers_are_taken_as_strtol_leaves_them() {
        let (tb, errs) = parse(
            b"a /1 t o 99999999999999999999
              a /2 t o 4294967297 2 junk after
              a /3 t o 1x
              a /4 t o 1 2x
              a /5 t o 1 99999999999999999999
              a /6 t o 1 2y 3
",
            Fmt::Fstab,
        );
        assert_eq!(errs, vec![3, 4, 6]);
        assert_eq!(tb.ents.len(), 3);
        // An overflowing number is taken, truncated, when it ends the line;
        // anything after passno is ignored.
        assert_eq!(tb.ents[0].freq, -1);
        assert_eq!((tb.ents[1].freq, tb.ents[1].passno), (1, 2));
        assert_eq!((tb.ents[2].freq, tb.ents[2].passno), (1, -1));
    }

    #[test]
    fn a_nul_ends_only_the_last_line() {
        let (tb, errs) = parse(b"a /1 t\0junk\nb /2 t\nc /3 t\0junk", Fmt::Fstab);
        assert_eq!(errs, vec![1]);
        assert_eq!(tb.ents.len(), 2);
        assert_eq!(tb.ents[1].target.as_deref(), Some(&b"/3"[..]));
    }

    #[test]
    fn swaps_parse() {
        let (tb, errs) = parse(
            b"Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority\n\
              /swapfile                               file\t\t4194300\t\t0\t\t-2\n\
              /x\\040(deleted) file 1 2 \n\
              /y file 1 2\n",
            Fmt::Guess,
        );
        assert_eq!(tb.fmt, Fmt::Swaps);
        assert_eq!(errs, vec![4]);
        assert_eq!(tb.ents.len(), 2);
        assert_eq!((tb.ents[0].size, tb.ents[0].priority), (4_194_300, -2));
        assert!(tb.ents[0].is_swaparea());
        assert_eq!(tb.ents[1].source.as_deref(), Some(&b"/x"[..]));
        assert_eq!(tb.ents[1].priority, 0);
    }

    #[test]
    fn utab_lines_parse() {
        let (tb, errs) = parse(
            b"ID=7 SRC=/dev/sda1 TARGET=/mnt ROOT=/ OPTS=user,x-foo ATTRS=a FOO=bar SRC=/dev/other
              SRC=/dev/b TARGET=/b ROOT=/ ID=
              ID=5x SRC=/dev/c
              SRC= TARGET=/d
              ID= SRC=/dev/e
",
            Fmt::Utab,
        );
        // `ID=` is no ID only at the end of the line: before a blank,
        // `strtol` skips the blank and finds no digits.
        assert_eq!(errs, vec![3, 4, 5]);
        assert_eq!(tb.ents.len(), 2);
        let f = &tb.ents[0];
        assert_eq!(f.id, 7);
        assert_eq!(f.source.as_deref(), Some(&b"/dev/sda1"[..]));
        assert_eq!(f.user_optstr.as_deref(), Some(&b"user,x-foo"[..]));
        assert_eq!(f.attrs.as_deref(), Some(&b"a"[..]));
        assert_eq!(tb.ents[1].id, 0);
        assert_eq!(tb.ents[1].root.as_deref(), Some(&b"/"[..]));
    }

    #[test]
    fn utab_options_merge_onto_the_kernel_entry() {
        let (mut tb, _) = parse(
            b"21 1 8:3 / / rw - ext4 /dev/sda3 rw\n30 21 8:4 / /mnt rw - ext4 /dev/sda4 rw\n",
            Fmt::Mountinfo,
        );
        let (u, _) = parse(
            b"SRC=/dev/sda4 TARGET=/mnt ROOT=/ OPTS=x-foo=1\n",
            Fmt::Utab,
        );
        merge_user_fs(&mut tb, &u.ents[0]);
        assert_eq!(tb.ents[1].options(), Some(&b"rw,x-foo=1"[..]));
        assert_eq!(tb.ents[1].user_optstr.as_deref(), Some(&b"x-foo=1"[..]));
        assert!(tb.ents[1].flags & MNT_FS_MERGED != 0);
        assert!(tb.ents[0].flags & MNT_FS_MERGED == 0);
    }

    #[test]
    fn guesses() {
        assert_eq!(guess_table_format(b"12 34 x"), Fmt::Mountinfo);
        assert_eq!(guess_table_format(b"12-3"), Fmt::Mountinfo);
        assert_eq!(guess_table_format(b"1234"), Fmt::Fstab);
        assert_eq!(guess_table_format(b"0x10 5"), Fmt::Fstab);
        assert_eq!(guess_table_format(b"Filename\tType"), Fmt::Swaps);
    }

    #[test]
    fn majmin_is_scanfs() {
        assert_eq!(scan_majmin(b"8:3"), Some((8, 3)));
        assert_eq!(scan_majmin(b"8: 3"), Some((8, 3)));
        assert_eq!(scan_majmin(b"8 :3"), None);
        assert_eq!(scan_majmin(b"-1:4294967297"), Some((u32::MAX, 1)));
    }

    #[test]
    fn a_missing_file_is_its_errno() {
        let not_found = |r: Result<(), i32>| {
            r.is_err_and(|e| {
                std::io::Error::from_raw_os_error(e).kind() == std::io::ErrorKind::NotFound
            })
        };
        let mut tb = Table::new();
        assert!(not_found(parse_file(
            &mut tb,
            b"/nonexistent-ulmount/x",
            None
        )));
        assert!(not_found(parse_fstab(
            &mut tb,
            Some(b"/nonexistent-ulmount/x"),
            None
        )));
        assert!(tb.ents.is_empty());
    }
}
