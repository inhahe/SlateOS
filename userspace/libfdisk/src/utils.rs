//! `utils.c`: reading the device and its first sector, partition device
//! names -- and the small pieces of C the port needs said once: `strtol`,
//! `strcasecmp`, `ul_stralnumcmp`, the truncating casts, a division that
//! does not trap.

use std::cell::RefCell;
use std::io::{Read, Seek, SeekFrom, Write};
use std::rc::Rc;

use crate::context::Context;
use crate::errno::{EINVAL, ENOMEM};

/// `a / b`, and 0 where C would trap on a zero divisor.
#[must_use]
pub fn div(a: u64, b: u64) -> u64 {
    a.checked_div(b).unwrap_or(0)
}

/// `a % b`, and 0 where C would trap.
#[must_use]
pub fn rem(a: u64, b: u64) -> u64 {
    a.checked_rem(b).unwrap_or(0)
}

/// `(unsigned int) v`: the low 32 bits.
#[must_use]
pub fn trunc_u32(v: i64) -> u32 {
    let [b0, b1, b2, b3, ..] = v.to_le_bytes();
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// `(int) v`: the low 32 bits, signed.
#[must_use]
pub fn trunc_i32(v: i64) -> i32 {
    let [b0, b1, b2, b3, ..] = v.to_le_bytes();
    i32::from_le_bytes([b0, b1, b2, b3])
}

/// `(int) v` for an unsigned 64-bit value.
#[must_use]
pub fn u64_as_i32(v: u64) -> i32 {
    let [b0, b1, b2, b3, ..] = v.to_le_bytes();
    i32::from_le_bytes([b0, b1, b2, b3])
}

/// `(unsigned int) v` for an unsigned 64-bit value.
#[must_use]
pub fn u64_as_u32(v: u64) -> u32 {
    let [b0, b1, b2, b3, ..] = v.to_le_bytes();
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// C's `isspace` in the C locale.
#[must_use]
pub fn c_isspace(c: u8) -> bool {
    cstrtol::isspace(c)
}

/// glibc's `strtol (s, &end, base)`: the value, the index `end` points at,
/// and the errno (`ERANGE` on overflow, the value then clamped). With no
/// digits, 0 and `end` 0 -- the start of the string, whitespace and sign
/// included, as glibc leaves it.
#[must_use]
pub fn strtol(s: &[u8], base: u32) -> (i64, usize, i32) {
    let (v, end, overflow) = cstrtol::strtol_overflow(s, base);
    (v, end, if overflow { crate::errno::ERANGE } else { 0 })
}

/// glibc's `strtoul (s, &end, base)`: as [`strtol`], but unsigned -- a
/// leading `-` negates in unsigned arithmetic, and overflow is `ULONG_MAX`
/// with `ERANGE`.
#[must_use]
pub fn strtoul(s: &[u8], base: u32) -> (u64, usize, i32) {
    let (v, end, overflow) = cstrtol::strtoull(s, base);
    (v, end, if overflow { crate::errno::ERANGE } else { 0 })
}

/// `strcasecmp`, in the C locale: the sign of the first difference.
#[must_use]
pub fn strcasecmp(a: &[u8], b: &[u8]) -> i32 {
    let mut i = 0;
    loop {
        let x = a.get(i).copied().unwrap_or(0).to_ascii_lowercase();
        let y = b.get(i).copied().unwrap_or(0).to_ascii_lowercase();
        if x != y || x == 0 {
            return i32::from(x).wrapping_sub(i32::from(y));
        }
        i = i.wrapping_add(1);
    }
}

/// `ul_stralnumcmp (p1, p2)`: compares only the letters and digits, in any
/// case.
#[must_use]
pub fn stralnumcmp(p1: &[u8], p2: &[u8]) -> i32 {
    let mut a = p1
        .iter()
        .copied()
        .filter(u8::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase());
    let mut b = p2
        .iter()
        .copied()
        .filter(u8::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase());
    loop {
        let c1 = a.next().unwrap_or(0);
        let c2 = b.next().unwrap_or(0);
        if c1 == 0 || c1 != c2 {
            return i32::from(c1).wrapping_sub(i32::from(c2));
        }
    }
}

/// The C string at the front of `b`: up to its first NUL.
#[must_use]
pub fn c_str(b: &[u8]) -> &[u8] {
    b.iter()
        .position(|&c| c == 0)
        .map_or(b, |n| b.get(..n).unwrap_or(b))
}

/// A little-endian field of `n` (at most 8) bytes at `at`; 0 for one past
/// the end of `b`.
#[must_use]
pub fn le_get(b: &[u8], at: usize, n: usize) -> u64 {
    let mut v = [0u8; 8];
    if let (Some(s), Some(d)) = (b.get(at..at.wrapping_add(n)), v.get_mut(..n)) {
        d.copy_from_slice(s);
    }
    u64::from_le_bytes(v)
}

/// Writes the low `n` (at most 8) bytes of `val` at `at`, little-endian;
/// nothing for a field past the end of `b`.
pub fn le_put(b: &mut [u8], at: usize, n: usize, val: u64) {
    let v = val.to_le_bytes();
    if let (Some(d), Some(s)) = (b.get_mut(at..at.wrapping_add(n)), v.get(..n)) {
        d.copy_from_slice(s);
    }
}

/// The `n`-byte (1 to 4) big-endian field at `at` of an on-disk structure
/// -- the SGI and Sun labels are big-endian throughout -- or 0 where the
/// field would run past `b`.
#[must_use]
pub fn be_get(b: &[u8], at: usize, n: usize) -> u32 {
    let mut v = [0u8; 4];
    if let (Some(s), Some(d)) = (
        b.get(at..at.wrapping_add(n)),
        v.get_mut(4usize.wrapping_sub(n)..),
    ) {
        d.copy_from_slice(s);
    }
    u32::from_be_bytes(v)
}

/// Stores the low `n` bytes of `val` big-endian at `at`, as `cpu_to_be16`
/// or `cpu_to_be32` into a field does; a field past `b` is not stored.
pub fn be_put(b: &mut [u8], at: usize, n: usize, val: u32) {
    let v = val.to_be_bytes();
    if let (Some(d), Some(s)) = (
        b.get_mut(at..at.wrapping_add(n)),
        v.get(4usize.wrapping_sub(n)..),
    ) {
        d.copy_from_slice(s);
    }
}

/// The errno an I/O error carries, or EIO.
#[must_use]
pub fn errno_of(e: &std::io::Error) -> i32 {
    e.raw_os_error().unwrap_or(crate::errno::EIO)
}

/// `-errno` for an I/O error: what libfdisk returns.
#[must_use]
pub fn neg_errno(e: &std::io::Error) -> i32 {
    errno_of(e).wrapping_neg()
}

impl Context {
    /// The big-endian field at `at` of the in-memory first sector, where
    /// the SGI and Sun labels live; 0 without a first sector.
    pub(crate) fn firstsector_be(&self, at: usize, n: usize) -> u32 {
        self.with_firstsector(|b| be_get(b, at, n)).unwrap_or(0)
    }

    /// Stores a big-endian field into the in-memory first sector.
    pub(crate) fn set_firstsector_be(&self, at: usize, n: usize, val: u32) {
        self.with_firstsector(|b| be_put(b, at, n, val));
    }

    /// `read_from_device (cxt, buf, start, size)`: one `read` of
    /// `buf.len()` bytes at `start`. A short read is -EINVAL ("probably too
    /// small a device").
    pub fn read_from_device(&self, buf: &mut [u8], start: u64) -> i32 {
        let Some(dev) = self.dev.as_ref() else {
            return -EINVAL;
        };
        let mut f = &**dev;
        if let Err(e) = f.seek(SeekFrom::Start(start)) {
            return neg_errno(&e);
        }
        match f.read(buf) {
            Ok(n) if n == buf.len() => 0,
            Ok(_) => -EINVAL,
            Err(e) => neg_errno(&e),
        }
    }

    /// `lseek (fd, off, SEEK_SET)` and `write_all (fd, buf, len)`: 0, or
    /// the negated errno.
    pub fn write_to_device(&self, buf: &[u8], off: u64) -> i32 {
        let Some(dev) = self.dev.as_ref() else {
            return -EINVAL;
        };
        let mut f = &**dev;
        if let Err(e) = f.seek(SeekFrom::Start(off)) {
            return neg_errno(&e);
        }
        match f.write_all(buf) {
            Ok(()) => 0,
            Err(e) => neg_errno(&e),
        }
    }

    /// `fdisk_init_firstsector_buffer (cxt, protect_off, protect_size)`:
    /// a zeroed in-memory first sector -- a new buffer when there is none
    /// of the right size, else the old one cleared and, when bytes are to be
    /// protected, those re-read from the device.
    pub fn init_firstsector_buffer(&mut self, protect_off: u32, protect_size: u32) -> i32 {
        let ss = usize::try_from(self.sector_size).unwrap_or(0);
        let fits = self
            .firstsector
            .as_ref()
            .is_some_and(|b| u64::try_from(b.borrow().len()).ok() == Some(self.sector_size));
        if !fits {
            // A nested context that shares the parent's buffer lets go of
            // it rather than freeing it: the parent's stays as it was.
            let mut v = Vec::new();
            if v.try_reserve_exact(ss).is_err() {
                return -ENOMEM;
            }
            v.resize(ss, 0);
            self.firstsector = Some(Rc::new(RefCell::new(v)));
            return 0;
        }
        if let Some(b) = self.firstsector.clone() {
            b.borrow_mut().fill(0);
            if protect_size != 0 {
                let off = usize::try_from(protect_off).unwrap_or(0);
                let len = usize::try_from(protect_size).unwrap_or(0);
                let mut part = vec![0u8; len];
                if self.read_from_device(&mut part, u64::from(protect_off)) == 0 {
                    if let Some(dst) = b.borrow_mut().get_mut(off..off.saturating_add(len)) {
                        dst.copy_from_slice(&part);
                    }
                }
            }
        }
        0
    }

    /// `fdisk_read_firstsector (cxt)`: the device's first sector into the
    /// in-memory buffer.
    pub fn read_firstsector(&mut self) -> i32 {
        let rc = self.init_firstsector_buffer(0, 0);
        if rc != 0 {
            return rc;
        }
        let Some(b) = self.firstsector.clone() else {
            return -EINVAL;
        };
        let mut buf = b.borrow().clone();
        let rc = self.read_from_device(&mut buf, 0);
        // A short read leaves the buffer as far as it read; C's `read`
        // fills it in place, so do the same.
        *b.borrow_mut() = buf;
        rc
    }

    /// The in-memory first sector, copied out.
    #[must_use]
    pub fn firstsector(&self) -> Vec<u8> {
        self.firstsector
            .as_ref()
            .map(|b| b.borrow().clone())
            .unwrap_or_default()
    }

    /// Changes the in-memory first sector in place.
    pub fn with_firstsector<R>(&self, f: impl FnOnce(&mut Vec<u8>) -> R) -> Option<R> {
        self.firstsector.as_ref().map(|b| f(&mut b.borrow_mut()))
    }
}

/// `fdisk_partname (dev, partno)`: the device name of partition `partno`
/// (from 1) of `dev`: `sda1`, `nvme0n1p1`, `...-part1` for udev's links.
#[must_use]
pub fn partname(dev: Option<&[u8]>, partno: usize) -> Option<Vec<u8>> {
    let Some(dev0) = dev.filter(|d| !d.is_empty()) else {
        return Some(partno.to_string().into_bytes());
    };
    let mut dev: Vec<u8> = dev0.to_vec();
    // It is impossible to predict /dev/dm-N partition names.
    if dev.starts_with(b"/dev/dm-") {
        if let Some(mapped) = dev
            .get(5..)
            .and_then(ulsysfs::canonicalize::canonicalize_dm_name)
        {
            dev = mapped;
        }
    }
    let mut w = dev.len();
    let mut p: &[u8] = b"";
    if dev.last().is_some_and(u8::is_ascii_digit) {
        p = b"p";
    }
    // The devfs kludge.
    if dev.ends_with(b"disc") {
        w = w.saturating_sub(4);
        p = b"part";
    }
    let head = dev.get(..w).unwrap_or(&dev);
    let with = |sep: &[u8]| -> Vec<u8> {
        let mut r = head.to_vec();
        r.extend_from_slice(sep);
        r.extend_from_slice(partno.to_string().as_bytes());
        r
    };
    // udev names partitions by appending -partN; multipath's rules too.
    if dev.starts_with(b"/dev/disk/by-id")
        || dev.starts_with(b"/dev/disk/by-path")
        || dev.starts_with(b"/dev/mapper")
    {
        let r = with(b"");
        if exists(&r) {
            return Some(r);
        }
        let r = with(b"p");
        if exists(&r) {
            return Some(r);
        }
        p = b"-part";
    }
    Some(with(p))
}

/// `access (path, F_OK) == 0`.
fn exists(path: &[u8]) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        std::fs::metadata(std::ffi::OsStr::from_bytes(path)).is_ok()
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(path).is_ok_and(|p| std::fs::metadata(p).is_ok())
    }
}
