// The arithmetic here is on indices into one line's buffer, each step
// guarded by the buffer's bound or a NUL found inside it, and on sscanf's
// numbers, which saturate before they are cut to an int.
#![allow(clippy::arithmetic_side_effects)]

//! Mount tables (`<mntent.h>`): `setmntent`, `getmntent`, `getmntent_r`,
//! `addmntent`, `endmntent` and `hasmntopt`, as glibc 2.39's
//! `misc/mntent_r.c` and `misc/mntent.c` write them.
//!
//! The kernel serves the mount table in Linux's format at `/proc/mounts`,
//! `/proc/self/mounts` and `/proc/<pid>/mounts` (`kernel/src/fs/procfs.rs`),
//! with the octal escapes this undoes. Until 2026-09-27 every call was a stub
//! in `unistd.rs` -- `setmntent` answered NULL "until /proc/mounts exists" --
//! so `df`, `mount -l`, `findmnt` and every gnulib `read_file_system_list`
//! saw no filesystems.
//!
//! The parser reads through [`MntLines`], an `fgets`-shaped source: a C
//! stream in use, a byte slice in the tests, so every rule of glibc's reader
//! is tested on the host.

use crate::errno;

/// Mount table entry (matches `struct mntent`).
#[repr(C)]
pub struct Mntent {
    /// Name of mounted filesystem.
    pub mnt_fsname: *mut u8,
    /// Filesystem path prefix (mount point).
    pub mnt_dir: *mut u8,
    /// Mount type.
    pub mnt_type: *mut u8,
    /// Mount options.
    pub mnt_opts: *mut u8,
    /// Dump frequency.
    pub mnt_freq: i32,
    /// Pass number for fsck.
    pub mnt_passno: i32,
}

/// What a missing field points at: glibc's `(char *) ""`.  Callers do not
/// write through it; nothing here does either.
static EMPTY_FIELD: [u8; 1] = [0];

/// `getmntent`'s own buffer, as glibc's `struct mntent_buffer`: allocated on
/// first use and kept, and shared by every caller -- `getmntent` is not
/// reentrant, here or in glibc; `getmntent_r` is the reentrant one.
#[repr(C)]
struct MntentBuffer {
    m: Mntent,
    buffer: [u8; 4096],
}

static MNTENT_BUFFER: core::sync::atomic::AtomicPtr<MntentBuffer> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Where an entry's line comes from: `fgets`'s contract -- up to
/// `buf.len() - 1` bytes, through the first newline, then a NUL; `false` at
/// end of input or on an error.  A stream in use, a slice in the tests.
trait MntLines {
    fn fgets(&mut self, buf: &mut [u8]) -> bool;
}

/// A C stream.
struct MntStream(*mut u8);

impl MntLines for MntStream {
    fn fgets(&mut self, buf: &mut [u8]) -> bool {
        let Ok(n) = i32::try_from(buf.len()) else {
            return false;
        };
        !crate::stdio::fgets(buf.as_mut_ptr(), n, self.0).is_null()
    }
}

/// The index of the first NUL in `buf`, or its length.
fn nul_at(buf: &[u8], from: usize) -> usize {
    buf.get(from..)
        .and_then(|rest| rest.iter().position(|&b| b == 0))
        .map_or(buf.len(), |i| from + i)
}

/// glibc's `strspn(s, " \t")` from `at`.
fn skip_blanks(buf: &[u8], mut at: usize) -> usize {
    while matches!(buf.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    at
}

/// glibc's `strsep(&head, " \t")`: the token at `*head` -- ended in place by
/// a NUL where the next blank or tab was -- and `*head` moved past it, or to
/// `None` if the line ended first.
fn strsep_blank(buf: &mut [u8], head: &mut Option<usize>) -> Option<usize> {
    let start = (*head)?;
    let end = nul_at(buf, start);
    let cut = buf
        .get(start..end)
        .and_then(|t| t.iter().position(|&b| b == b' ' || b == b'\t'))
        .map(|i| start + i);
    match cut.and_then(|c| buf.get_mut(c).map(|slot| (c, slot))) {
        Some((c, slot)) => {
            *slot = 0;
            *head = Some(c + 1);
        }
        None => *head = None,
    }
    Some(start)
}

/// glibc's `decode_name`, in place from `at` to its NUL: `\040` a space,
/// `\011` a tab, `\012` a newline, and `\\` or `\134` a backslash.
fn decode_name(buf: &mut [u8], at: usize) {
    let end = nul_at(buf, at);
    let (mut r, mut w) = (at, at);
    while r < end {
        let rest = buf.get(r..end).unwrap_or_default();
        let (byte, took) = match rest {
            [b'\\', b'0', b'4', b'0', ..] => (b' ', 4),
            [b'\\', b'0', b'1', b'1', ..] => (b'\t', 4),
            [b'\\', b'0', b'1', b'2', ..] => (b'\n', 4),
            [b'\\', b'\\', ..] => (b'\\', 2),
            [b'\\', b'1', b'3', b'4', ..] => (b'\\', 4),
            [b, ..] => (*b, 1),
            [] => break,
        };
        if let Some(slot) = buf.get_mut(w) {
            *slot = byte;
        }
        w += 1;
        r += took;
    }
    if let Some(slot) = buf.get_mut(w) {
        *slot = 0;
    }
}

/// `sscanf(s, " %d %d ", &a, &b)`: how many converted -- or -1 (`EOF`) if the
/// input ended before the first -- and the values.  A value is `strtol`'s,
/// saturated to 64 bits, then cut to an `int` as C's conversion does.
fn scan_two_ints(buf: &[u8], at: usize) -> (i32, i32, i32) {
    let end = nul_at(buf, at);
    let s = buf.get(at..end).unwrap_or_default();
    let mut i = 0;
    let mut got = [0i32; 2];
    for k in 0..2 {
        while matches!(s.get(i), Some(b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')) {
            i += 1;
        }
        let [a, b] = got;
        if i >= s.len() {
            // Input ran out: before the first conversion that is EOF.
            return (if k == 0 { -1 } else { 1 }, a, b);
        }
        let negative = s.get(i) == Some(&b'-');
        if matches!(s.get(i), Some(b'-' | b'+')) {
            i += 1;
        }
        let digits = s
            .get(i..)
            .map_or(0, |r| r.iter().take_while(|c| c.is_ascii_digit()).count());
        if digits == 0 {
            // A matching failure: what converted so far.
            return (if k == 0 { 0 } else { 1 }, a, b);
        }
        let mut v: i64 = 0;
        for &d in s.get(i..i + digits).unwrap_or_default() {
            let d = i64::from(d - b'0');
            v = if negative {
                v.checked_mul(10)
                    .and_then(|x| x.checked_sub(d))
                    .unwrap_or(i64::MIN)
            } else {
                v.checked_mul(10)
                    .and_then(|x| x.checked_add(d))
                    .unwrap_or(i64::MAX)
            };
        }
        i += digits;
        if let Some(slot) = got.get_mut(k) {
            // C's `(int)` of a `long`: the low 32 bits.
            #[allow(clippy::cast_possible_truncation)]
            let low = v as i32;
            *slot = low;
        }
    }
    let [a, b] = got;
    (2, a, b)
}

/// glibc's `get_mnt_entry`: the next entry -- blank and `#` lines skipped --
/// read into `buf` and split there, in place, into the four strings `mp`
/// points at and the two numbers.  `false` at the end of the input.
fn mnt_entry(src: &mut dyn MntLines, buf: &mut [u8], mp: &mut Mntent) -> bool {
    let start = loop {
        if !src.fgets(buf) {
            return false;
        }
        let len = nul_at(buf, 0);
        match buf
            .get(..len)
            .and_then(|l| l.iter().position(|&b| b == b'\n'))
        {
            Some(nl) => {
                // Chop the newline and any blanks before it.
                let mut end = nl;
                while end > 0 && matches!(buf.get(end - 1), Some(b' ' | b'\t')) {
                    end -= 1;
                }
                if let Some(slot) = buf.get_mut(end) {
                    *slot = 0;
                }
            }
            None => {
                // Not the whole line was read: read the rest, and forget it.
                let mut tmp = [0u8; 1024];
                while src.fgets(&mut tmp) {
                    if tmp
                        .get(..nul_at(&tmp, 0))
                        .is_some_and(|l| l.contains(&b'\n'))
                    {
                        break;
                    }
                }
            }
        }
        let head = skip_blanks(buf, 0);
        if !matches!(buf.get(head), None | Some(0 | b'#')) {
            break head;
        }
    };
    let base = buf.as_mut_ptr();
    let mut head = Some(start);
    let mut fields = [EMPTY_FIELD.as_ptr().cast_mut(); 4];
    for (k, field) in fields.iter_mut().enumerate() {
        if let Some(cp) = strsep_blank(buf, &mut head) {
            decode_name(buf, cp);
            // SAFETY: `cp` is an index into `buf`, which `base` starts.
            *field = unsafe { base.add(cp) };
        }
        if k < 3 {
            head = head.map(|h| skip_blanks(buf, h));
        }
    }
    let [fsname, dir, kind, opts] = fields;
    mp.mnt_fsname = fsname;
    mp.mnt_dir = dir;
    mp.mnt_type = kind;
    mp.mnt_opts = opts;
    let (count, freq, passno) = head.map_or((0, 0, 0), |h| scan_two_ints(buf, h));
    match count {
        0 => {
            mp.mnt_freq = 0;
            mp.mnt_passno = 0;
        }
        1 => {
            mp.mnt_freq = freq;
            mp.mnt_passno = 0;
        }
        2 => {
            mp.mnt_freq = freq;
            mp.mnt_passno = passno;
        }
        // EOF before a number: glibc's switch has no case for it, and leaves
        // both as they were.
        _ => {}
    }
    true
}

/// glibc's `__getmntent_r` loop: the next entry, skipping any `autofs` one
/// whose options say `ignore` -- zeroed, as glibc leaves it, and passed over.
fn mnt_next(src: &mut dyn MntLines, buf: &mut [u8], entry: &mut Mntent) -> bool {
    loop {
        if !mnt_entry(src, buf, entry) {
            return false;
        }
        // SAFETY: `mnt_type` points into `buf` or at `EMPTY_FIELD`.
        let is_autofs = unsafe { c_bytes(entry.mnt_type) } == b"autofs";
        // SAFETY: `entry` is the entry just read.
        if is_autofs && !unsafe { hasmntopt(entry, b"ignore\0".as_ptr()) }.is_null() {
            *entry = Mntent {
                mnt_fsname: core::ptr::null_mut(),
                mnt_dir: core::ptr::null_mut(),
                mnt_type: core::ptr::null_mut(),
                mnt_opts: core::ptr::null_mut(),
                mnt_freq: 0,
                mnt_passno: 0,
            };
            continue;
        }
        return true;
    }
}

/// The bytes of the C string at `p` (none for NULL).
///
/// # Safety
///
/// `p` is NULL or a valid NUL-terminated string that outlives the result.
unsafe fn c_bytes<'a>(p: *const u8) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    // SAFETY: the caller's contract.
    unsafe { core::ffi::CStr::from_ptr(p.cast()) }.to_bytes()
}

/// Open a mount table: `fopen(file, mode)` with glibc's `"ce"` appended --
/// no cancellation points, and close-on-exec.  NULL with `errno` as `fopen`
/// leaves it; a NULL `mode` is `EFAULT` (glibc reads it first, and faults).
///
/// # Safety
///
/// `filename` and `mode` are NULL or valid NUL-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn setmntent(filename: *const u8, mode: *const u8) -> *mut u8 {
    if mode.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's contract.
    let m = unsafe { c_bytes(mode) };
    let Some(len) = m.len().checked_add(3) else {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    };
    let newmode = crate::malloc::malloc(len);
    if newmode.is_null() {
        errno::set_errno(errno::ENOMEM);
        return core::ptr::null_mut();
    }
    // SAFETY: `newmode` holds `m.len() + 3` bytes: the mode, "ce" and a NUL.
    let stream = unsafe {
        core::ptr::copy_nonoverlapping(m.as_ptr(), newmode, m.len());
        core::ptr::copy_nonoverlapping(b"ce\0".as_ptr(), newmode.add(m.len()), 3);
        crate::stdio::fopen(filename, newmode)
    };
    // SAFETY: `newmode` came from `malloc` above; fopen does not keep it.
    unsafe { crate::malloc::free(newmode) };
    stream
}

/// Close a mount table.  Always 1, as glibc (and SunOS before it); a NULL
/// stream is allowed.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn endmntent(stream: *mut u8) -> i32 {
    if !stream.is_null() {
        // The stream's own error cannot change the answer (glibc ignores it).
        let _ = crate::stdio::fclose(stream);
    }
    1
}

/// Read the next entry into the caller's `mp` and `buf`, as glibc's
/// `getmntent_r`: blank and `#` lines skipped; a line longer than `bufsiz`
/// cut there, the rest read and dropped; the fields split at blanks and tabs
/// and their `\040`-style escapes undone; missing fields "" and missing
/// numbers 0.  An `autofs` entry whose options say `ignore` is skipped.  NULL
/// at the end of the table.
///
/// `bufsiz` 0 or less is `EINVAL` (glibc's `fgets`); a NULL `mp` or `buffer`
/// is `EFAULT`, where glibc faults.
///
/// # Safety
///
/// `stream` is a stream, `mp` NULL or valid to write, and `buffer` NULL or
/// valid for `bufsiz` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getmntent_r(
    stream: *mut u8,
    mp: *mut Mntent,
    buffer: *mut u8,
    bufsiz: i32,
) -> *mut Mntent {
    let Some(len) = usize::try_from(bufsiz).ok().filter(|&n| n > 0) else {
        errno::set_errno(errno::EINVAL);
        return core::ptr::null_mut();
    };
    if mp.is_null() || buffer.is_null() {
        errno::set_errno(errno::EFAULT);
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's contract: `buffer` holds `bufsiz` bytes and `mp`
    // is valid to write; neither aliases the other.
    let (buf, entry) = unsafe { (core::slice::from_raw_parts_mut(buffer, len), &mut *mp) };
    let mut src = MntStream(stream);
    crate::stdio::flockfile(stream.cast());
    let found = mnt_next(&mut src, buf, entry);
    crate::stdio::funlockfile(stream.cast());
    if found { mp } else { core::ptr::null_mut() }
}

/// Read the next entry into a buffer of the library's -- one for the whole
/// process, overwritten by the next call, as glibc's is.  NULL at the end of
/// the table, or if the buffer cannot be had.
///
/// # Safety
///
/// `stream` is a stream.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn getmntent(stream: *mut u8) -> *mut Mntent {
    use core::sync::atomic::Ordering;
    let mut buffer = MNTENT_BUFFER.load(Ordering::Acquire);
    if buffer.is_null() {
        // Zeroed, so that the entry is valid before the first read fills it.
        let fresh = crate::malloc::calloc(1, size_of::<MntentBuffer>()).cast::<MntentBuffer>();
        if fresh.is_null() {
            return core::ptr::null_mut();
        }
        buffer = match MNTENT_BUFFER.compare_exchange(
            core::ptr::null_mut(),
            fresh,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => fresh,
            Err(theirs) => {
                // SAFETY: `fresh` came from `calloc` above and nothing else has it.
                unsafe { crate::malloc::free(fresh.cast()) };
                theirs
            }
        };
    }
    // SAFETY: `buffer` is the live, zero-initialised `MntentBuffer`.
    unsafe {
        let m = core::ptr::addr_of_mut!((*buffer).m);
        let b = core::ptr::addr_of_mut!((*buffer).buffer).cast::<u8>();
        getmntent_r(stream, m, b, 4096)
    }
}

/// glibc's `write_string`: `s` with a blank, tab, newline or backslash as
/// its three-digit octal escape, then a blank.
fn mnt_field(s: &[u8], put: &mut dyn FnMut(u8)) {
    for &c in s {
        if matches!(c, b' ' | b'\t' | b'\n' | b'\\') {
            put(b'\\');
            put(((c & 0xC0) >> 6) + b'0');
            put(((c & 0x38) >> 3) + b'0');
            put((c & 0x07) + b'0');
        } else {
            put(c);
        }
    }
    put(b' ');
}

/// `n` in decimal, as `%d` prints it.
fn mnt_decimal(n: i32, put: &mut dyn FnMut(u8)) {
    let mut digits = [0u8; 11];
    let mut k = digits.len();
    let mut v = n.unsigned_abs();
    loop {
        k -= 1;
        if let Some(slot) = digits.get_mut(k) {
            #[allow(clippy::cast_possible_truncation)]
            {
                *slot = b'0' + (v % 10) as u8;
            }
        }
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if n < 0 {
        put(b'-');
    }
    for &d in digits.get(k..).unwrap_or_default() {
        put(d);
    }
}

/// An entry as `addmntent` writes it: four escaped fields and the two numbers.
fn mnt_line(fields: [&[u8]; 4], freq: i32, passno: i32, put: &mut dyn FnMut(u8)) {
    for f in fields {
        mnt_field(f, put);
    }
    mnt_decimal(freq, put);
    put(b' ');
    mnt_decimal(passno, put);
    put(b'\n');
}

/// Append `mnt` to the table open on `stream`, as glibc's `addmntent`: at
/// the end of the file, the fields escaped, then flushed.  0 on success, 1 if
/// the seek, a write or the flush failed.  A NULL `mnt` is `EFAULT`; a NULL
/// field is written as "".
///
/// # Safety
///
/// `stream` is a stream; `mnt` is NULL or a valid entry whose fields are NULL
/// or valid strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn addmntent(stream: *mut u8, mnt: *const Mntent) -> i32 {
    if mnt.is_null() {
        errno::set_errno(errno::EFAULT);
        return 1;
    }
    if crate::stdio::fseek(stream, 0, crate::stdio::SEEK_END) != 0 {
        return 1;
    }
    // SAFETY: the caller's contract.
    let m = unsafe { &*mnt };
    // SAFETY: the caller's contract: each field is NULL or a string.
    let fields = unsafe {
        [
            c_bytes(m.mnt_fsname),
            c_bytes(m.mnt_dir),
            c_bytes(m.mnt_type),
            c_bytes(m.mnt_opts),
        ]
    };
    crate::stdio::flockfile(stream.cast());
    mnt_line(fields, m.mnt_freq, m.mnt_passno, &mut |b| {
        // Errors are read from the stream below, as glibc does.
        let _ = crate::stdio::fputc(i32::from(b), stream);
    });
    let failed = crate::stdio::ferror(stream) != 0 || crate::stdio::fflush(stream) != 0;
    crate::stdio::funlockfile(stream.cast());
    i32::from(failed)
}

/// Find `opt` among `mnt`'s options, as glibc's `hasmntopt`: a match must
/// start the options or follow a comma, and end them or come before `=` or a
/// comma.  The address of the match within `mnt_opts`, or NULL.
///
/// # Safety
///
/// `mnt` is NULL or a valid entry; `opt` is NULL or a valid string.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn hasmntopt(mnt: *const Mntent, opt: *const u8) -> *mut u8 {
    if mnt.is_null() || opt.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's contract.
    let base = unsafe { (*mnt).mnt_opts };
    if base.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the caller's contract.
    let (opts, o) = unsafe { (c_bytes(base), c_bytes(opt)) };
    let mut rest = 0;
    loop {
        // `strstr(rest, opt)`: an empty `opt` matches where it starts.
        let found = opts.get(rest..).and_then(|r| {
            if o.is_empty() {
                Some(0)
            } else {
                r.windows(o.len()).position(|w| w == o)
            }
        });
        let Some(p) = found.map(|i| rest + i) else {
            return core::ptr::null_mut();
        };
        let before = p == rest || opts.get(p.wrapping_sub(1)) == Some(&b',');
        let after = opts.get(p + o.len()).copied().unwrap_or(0);
        if before && matches!(after, 0 | b'=' | b',') {
            // SAFETY: `p` is within the string `base` starts.
            return unsafe { base.add(p) };
        }
        match opts
            .get(p..)
            .and_then(|r| r.iter().position(|&b| b == b','))
        {
            Some(c) => rest = p + c + 1,
            None => return core::ptr::null_mut(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A mount table in memory, read with `fgets`'s contract -- as a stream
    /// would be: up to `buf.len() - 1` bytes, through the first newline.
    struct Lines<'a>(&'a [u8]);

    impl MntLines for Lines<'_> {
        fn fgets(&mut self, buf: &mut [u8]) -> bool {
            if self.0.is_empty() || buf.is_empty() {
                return false;
            }
            let room = buf.len() - 1;
            let take = match self.0.iter().position(|&b| b == b'\n') {
                Some(nl) => (nl + 1).min(room),
                None => self.0.len().min(room),
            };
            buf[..take].copy_from_slice(&self.0[..take]);
            buf[take] = 0;
            self.0 = &self.0[take..];
            true
        }
    }

    fn blank_entry() -> Mntent {
        Mntent {
            mnt_fsname: core::ptr::null_mut(),
            mnt_dir: core::ptr::null_mut(),
            mnt_type: core::ptr::null_mut(),
            mnt_opts: core::ptr::null_mut(),
            mnt_freq: 0,
            mnt_passno: 0,
        }
    }

    /// The four strings and two numbers of `m`.
    fn fields(m: &Mntent) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i32, i32) {
        let s = |p: *mut u8| unsafe { c_bytes(p) }.to_vec();
        (
            s(m.mnt_fsname),
            s(m.mnt_dir),
            s(m.mnt_type),
            s(m.mnt_opts),
            m.mnt_freq,
            m.mnt_passno,
        )
    }

    fn entry(
        fsname: &[u8],
        dir: &[u8],
        kind: &[u8],
        opts: &[u8],
        freq: i32,
        passno: i32,
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, i32, i32) {
        (
            fsname.to_vec(),
            dir.to_vec(),
            kind.to_vec(),
            opts.to_vec(),
            freq,
            passno,
        )
    }

    /// /proc/mounts' shape: comments, blank lines and lines of blanks are
    /// skipped, blanks before the newline dropped, the numbers read.
    #[test]
    fn mntent_reads_a_mount_table() {
        let table = b"# a comment\n\n \t \n  proc /proc proc rw,nosuid 0 0\n/dev/vda\t/  ext4 rw,relatime 1 2 \t\n";
        let mut src = Lines(table);
        let mut buf = [0u8; 256];
        let mut m = blank_entry();
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(
            fields(&m),
            entry(b"proc", b"/proc", b"proc", b"rw,nosuid", 0, 0)
        );
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(
            fields(&m),
            entry(b"/dev/vda", b"/", b"ext4", b"rw,relatime", 1, 2)
        );
        assert!(!mnt_next(&mut src, &mut buf, &mut m));
    }

    /// The kernel's escapes, undone: `\040` a space, `\011` a tab, `\012` a
    /// newline, `\\` and `\134` a backslash.
    #[test]
    fn mntent_undoes_the_escapes() {
        let mut src = Lines(b"a\\040b /mnt/x\\011y\\012z t o\\\\p\\134q 0 0\n");
        let mut buf = [0u8; 256];
        let mut m = blank_entry();
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(
            fields(&m),
            entry(b"a b", b"/mnt/x\ty\nz", b"t", b"o\\p\\q", 0, 0)
        );
    }

    /// Missing fields are "", missing numbers 0 -- one number, the pass 0.
    #[test]
    fn mntent_fills_what_is_missing() {
        let mut src = Lines(b"dev\ndev dir type opts 7\ndev dir type opts x\n");
        let mut buf = [0u8; 256];
        let mut m = blank_entry();
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"dev", b"", b"", b"", 0, 0));
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"dev", b"dir", b"type", b"opts", 7, 0));
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"dev", b"dir", b"type", b"opts", 0, 0));
    }

    /// A line longer than the buffer is cut there and the rest dropped; the
    /// next line is read whole.
    #[test]
    fn mntent_cuts_a_long_line_and_drops_the_rest() {
        let mut src = Lines(b"abcdefghij klmnopqrst u v 1 2\nnext / t o 3 4\n");
        let mut buf = [0u8; 16];
        let mut m = blank_entry();
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"abcdefghij", b"klmn", b"", b"", 0, 0));
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"next", b"/", b"t", b"o", 3, 4));
    }

    /// glibc's quirk, kept: a cut line whose last bytes are blanks leaves the
    /// numbers as they were -- `sscanf` answers EOF, which its switch has no
    /// case for.
    #[test]
    fn mntent_keeps_the_numbers_when_a_cut_line_ends_in_blanks() {
        let mut src = Lines(b"a b c d        tail 1 2\n");
        let mut buf = [0u8; 12];
        let mut m = blank_entry();
        m.mnt_freq = 7;
        m.mnt_passno = 9;
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"a", b"b", b"c", b"d", 7, 9));
    }

    /// An `autofs` entry is skipped if its options say `ignore`, not otherwise.
    #[test]
    fn mntent_skips_an_ignored_autofs() {
        let mut src = Lines(b"auto /a autofs rw,ignore 0 0\nauto /b autofs rw 0 0\n");
        let mut buf = [0u8; 256];
        let mut m = blank_entry();
        assert!(mnt_next(&mut src, &mut buf, &mut m));
        assert_eq!(fields(&m), entry(b"auto", b"/b", b"autofs", b"rw", 0, 0));
        assert!(!mnt_next(&mut src, &mut buf, &mut m));
    }

    #[test]
    fn scan_two_ints_is_sscanf() {
        let scan = |s: &[u8]| {
            let mut v = s.to_vec();
            v.push(0);
            scan_two_ints(&v, 0)
        };
        assert_eq!(scan(b" 1 2 "), (2, 1, 2));
        assert_eq!(scan(b"-3 +4"), (2, -3, 4));
        assert_eq!(scan(b"5"), (1, 5, 0));
        assert_eq!(scan(b"5 x"), (1, 5, 0));
        assert_eq!(scan(b"x"), (0, 0, 0));
        assert_eq!(scan(b"- 5"), (0, 0, 0));
        assert_eq!(scan(b""), (-1, 0, 0));
        assert_eq!(scan(b" \t "), (-1, 0, 0));
        // strtol's 64 bits, then an int's 32: 99999999999 is 0x17_4876_E7FF.
        assert_eq!(scan(b"99999999999 0"), (2, 0x4876_E7FF, 0));
        // Past 64 bits strtol saturates, and LONG_MAX's low half is -1.
        assert_eq!(scan(b"99999999999999999999 0"), (2, -1, 0));
    }

    #[test]
    fn hasmntopt_is_glibcs() {
        let mut opts = b"rw,noatime,size=10,x\0".to_vec();
        let m = Mntent {
            mnt_opts: opts.as_mut_ptr(),
            ..blank_entry()
        };
        let at = |o: &[u8]| {
            let mut z = o.to_vec();
            z.push(0);
            let p = unsafe { hasmntopt(&m, z.as_ptr()) };
            (!p.is_null()).then(|| p as usize - m.mnt_opts as usize)
        };
        assert_eq!(at(b"rw"), Some(0));
        assert_eq!(at(b"noatime"), Some(3));
        assert_eq!(at(b"size"), Some(11));
        assert_eq!(at(b"x"), Some(19));
        // Not a whole option: inside one, or a prefix of one.
        assert_eq!(at(b"atime"), None);
        assert_eq!(at(b"w"), None);
        assert_eq!(at(b"siz"), None);
        assert_eq!(at(b""), None);
        assert!(unsafe { hasmntopt(core::ptr::null(), b"rw\0".as_ptr()) }.is_null());
        assert!(unsafe { hasmntopt(&m, core::ptr::null()) }.is_null());
    }

    /// `addmntent`'s line: the four fields with blanks, tabs, newlines and
    /// backslashes as octal escapes, then the numbers.
    #[test]
    fn addmntent_writes_glibcs_line() {
        let mut out = Vec::new();
        mnt_line([b"a b", b"/m\tn\no", b"t", b"o\\p"], 1, -2, &mut |b| {
            out.push(b)
        });
        assert_eq!(out, b"a\\040b /m\\011n\\012o t o\\134p 1 -2\n".to_vec());
        let mut out = Vec::new();
        mnt_line([b"", b"", b"", b""], i32::MIN, 0, &mut |b| out.push(b));
        assert_eq!(out, b"    -2147483648 0\n".to_vec());
    }

    #[test]
    fn mntent_calls_refuse_what_glibc_would_fault_on() {
        assert_eq!(endmntent(core::ptr::null_mut()), 1);
        errno::set_errno(0);
        assert!(unsafe { setmntent(b"/proc/mounts\0".as_ptr(), core::ptr::null()) }.is_null());
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let mut m = blank_entry();
        let mut buf = [0u8; 16];
        errno::set_errno(0);
        assert!(
            unsafe { getmntent_r(core::ptr::null_mut(), &mut m, buf.as_mut_ptr(), 0) }.is_null()
        );
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        let null_entry = core::ptr::null_mut();
        assert!(
            unsafe { getmntent_r(core::ptr::null_mut(), null_entry, buf.as_mut_ptr(), 16) }
                .is_null()
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert_eq!(
            unsafe { addmntent(core::ptr::null_mut(), core::ptr::null()) },
            1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_mntent_size() {
        // Four pointers and two ints: musl's `struct mntent`, which
        // abi_layout.rs checks field by field.
        assert_eq!(core::mem::size_of::<Mntent>(), 40);
    }
}
