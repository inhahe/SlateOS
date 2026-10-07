//! File-backed `mmap`, made the one way the native kernel allows until it
//! maps files itself: anonymous memory where the mapping goes, filled from
//! the file, then given the protection asked for.
//!
//! # Why
//!
//! The native `SYS_MMAP` makes anonymous memory, or maps device registers; it
//! has no file to map from. (The Linux ABI's `mmap` maps files, a page at a
//! time as they are touched -- known-issues TD22 -- but a native program does
//! not reach it.) Until 2026-10-06 this library passed a file mapping's
//! descriptor to that call anyway, where nothing read it: a native program
//! that mapped a file was given memory full of zeros, and no error. Lane A
//! is asked for native file mappings
//! (`requests/d-a-a-native-program-cannot-map-a-file.md`).
//!
//! # What a mapping is here
//!
//! Linux without an MMU maps a private file the same way, by copying it in.
//!
//! - `MAP_PRIVATE`: the file's bytes from `offset`, as they are when it is
//!   mapped. Past the end of the file it reads as zeros, and writes stay in
//!   the mapping, as on Linux.
//! - `MAP_SHARED` without `PROT_WRITE`: the same. Unlike Linux's, it does
//!   not show writes made to the file after it was mapped.
//! - `MAP_SHARED` with `PROT_WRITE`: refused, `ENODEV` -- "the file system
//!   does not support memory mapping". Writes to the mapping would never
//!   reach the file, so a program is not told its data are saved when they
//!   are not; it uses `write`, or says what failed.
//!
//! Where it differs from Linux besides:
//! - A page wholly past the end of the file reads as zeros, where Linux
//!   raises SIGBUS.
//! - The whole range is read when it is mapped, where Linux reads each page
//!   at its first touch: mapping a large file costs its size in memory, and
//!   the time to read it, at once.
//! - `madvise(MADV_DONTNEED)` is refused (`EINVAL`), as it is for all memory
//!   here, where Linux reads the file again; and an `mremap` that grows the
//!   mapping is refused too (`ENOSYS`).
//!
//! # The refusals, in Linux's order
//!
//! As `do_mmap` (mm/mmap.c, Linux 6.6) takes a file mapping:
//! 1. `fd` not open: `EBADF` (`ksys_mmap_pgoff`; `O_PATH` is refused the same
//!    way by the caller, `mmap`).
//! 2. A negative `offset`, or `offset + length` past the largest file:
//!    `EOVERFLOW` (`file_mmap_ok`).
//! 3. `MAP_SHARED` with `PROT_WRITE`, on a descriptor not open for writing:
//!    `EACCES`.
//! 4. A descriptor not open for reading: `EACCES`.
//! 5. Something no file system maps -- a pipe, a socket, a terminal, a
//!    directory: `ENODEV`.
//! 6. `PROT_GROWSDOWN` or `PROT_GROWSUP`: `EINVAL`.
//!
//! Then this library's own: `MAP_SHARED` with `PROT_WRITE`, `ENODEV`.

use core::ffi::c_void;

use super::{
    MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE, MAP_TYPE, PROT_GROWSDOWN, PROT_GROWSUP, PROT_READ,
    PROT_WRITE,
};
use crate::errno;
use crate::fcntl::{O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY, S_IFDIR, S_IFMT};
use crate::fdtable::{self, HandleKind};
use crate::types::{Fd, OffT, SizeT, SsizeT};

/// The page a mapping is made of: the kernel maps whole ones, so a mapping's
/// last page is all there, past `length`.
const PAGE: SizeT = crate::unistd::PAGE_SIZE;

/// Map `length` bytes of `fd`'s file from `offset` (`mmap` with
/// `MAP_ANONYMOUS` clear, past its own checks).
pub(super) fn map(
    addr: *mut c_void,
    length: SizeT,
    prot: i32,
    flags: i32,
    fd: Fd,
    offset: OffT,
) -> *mut c_void {
    map_with(&System, addr, length, prot, flags, fd, offset)
}

/// What a mapping is made with: the library's own calls, or the tests'
/// stand-ins.
trait Calls {
    /// Anonymous memory, readable and writable, as `flags` asks for it;
    /// `MAP_FAILED` with `errno` set if not.
    fn anonymous(&self, addr: *mut c_void, length: SizeT, flags: i32) -> *mut c_void;
    /// `pread`.
    fn read_at(&self, fd: Fd, buf: *mut u8, count: SizeT, offset: OffT) -> SsizeT;
    /// `mprotect`.
    fn protect(&self, p: *mut c_void, length: SizeT, prot: i32) -> i32;
    /// `munmap`.
    fn unmap(&self, p: *mut c_void, length: SizeT) -> i32;
    /// Whether `fd` is a directory.
    fn is_directory(&self, fd: Fd) -> bool;
}

/// The library's own calls.
struct System;

impl Calls for System {
    fn anonymous(&self, addr: *mut c_void, length: SizeT, flags: i32) -> *mut c_void {
        super::mmap(addr, length, PROT_READ | PROT_WRITE, flags, -1, 0)
    }

    fn read_at(&self, fd: Fd, buf: *mut u8, count: SizeT, offset: OffT) -> SsizeT {
        crate::file::pread(fd, buf, count, offset)
    }

    fn protect(&self, p: *mut c_void, length: SizeT, prot: i32) -> i32 {
        super::mprotect(p, length, prot)
    }

    fn unmap(&self, p: *mut c_void, length: SizeT) -> i32 {
        super::munmap(p, length)
    }

    fn is_directory(&self, fd: Fd) -> bool {
        let mut st = core::mem::MaybeUninit::<crate::stat::Stat>::zeroed();
        // A file that cannot be asked about is taken for one that is not a
        // directory: a directory's read fails, and the mapping with it.
        crate::file::fstat(fd, st.as_mut_ptr()) == 0
            // SAFETY: `fstat` filled it; all zeros was a valid `Stat` already.
            && unsafe { st.assume_init() }.st_mode & S_IFMT == S_IFDIR
    }
}

/// [`map`], made with `calls`.
fn map_with(
    calls: &dyn Calls,
    addr: *mut c_void,
    length: SizeT,
    prot: i32,
    flags: i32,
    fd: Fd,
    offset: OffT,
) -> *mut c_void {
    if let Err(e) = refusal(calls, length, prot, flags, fd, offset) {
        errno::set_errno(e);
        return MAP_FAILED;
    }
    // Private and anonymous, wherever the caller's placement flags
    // (`MAP_FIXED`, `MAP_FIXED_NOREPLACE`) and hints (`MAP_POPULATE`,
    // `MAP_NORESERVE`) put it.
    let p = calls.anonymous(
        addr,
        length,
        (flags & !MAP_TYPE) | MAP_PRIVATE | MAP_ANONYMOUS,
    );
    if p == MAP_FAILED {
        return MAP_FAILED;
    }
    // Whole pages, as Linux maps them: the bytes from `length` to the end of
    // its last page are the file's too, where the file has bytes there.
    let span = length.checked_next_multiple_of(PAGE).unwrap_or(length);
    let made = fill(calls, p.cast(), span, fd, offset).and_then(|()| {
        if prot == PROT_READ | PROT_WRITE || calls.protect(p, length, prot) == 0 {
            Ok(())
        } else {
            Err(errno::get_errno())
        }
    });
    if let Err(e) = made {
        // What went wrong first is what is reported; a failed unmap has no
        // errno of its own to give, and the memory is the process's either
        // way.
        let _ = calls.unmap(p, length);
        errno::set_errno(e);
        return MAP_FAILED;
    }
    p
}

/// Why `fd` cannot be mapped so, in Linux's order (the module's doc).
fn refusal(
    calls: &dyn Calls,
    length: SizeT,
    prot: i32,
    flags: i32,
    fd: Fd,
    offset: OffT,
) -> Result<(), i32> {
    let entry = fdtable::get_fd(fd).ok_or(errno::EBADF)?;
    let end = u64::try_from(offset)
        .ok()
        .and_then(|o| o.checked_add(u64::try_from(length).ok()?));
    if end.is_none_or(|e| i64::try_from(e).is_err()) {
        return Err(errno::EOVERFLOW);
    }
    let shared_write = flags & MAP_TYPE != MAP_PRIVATE && prot & PROT_WRITE != 0;
    let mode = entry.status_flags & O_ACCMODE;
    if shared_write && mode != O_WRONLY && mode != O_RDWR {
        return Err(errno::EACCES);
    }
    if mode != O_RDONLY && mode != O_RDWR {
        return Err(errno::EACCES);
    }
    if entry.kind != HandleKind::File || calls.is_directory(fd) {
        return Err(errno::ENODEV);
    }
    if prot & (PROT_GROWSDOWN | PROT_GROWSUP) != 0 {
        return Err(errno::EINVAL);
    }
    if shared_write {
        return Err(errno::ENODEV);
    }
    Ok(())
}

/// Read the file into `dst` from `offset`, until `length` bytes (`dst`'s
/// whole pages) or the end of the file. What lies past the end stays as the
/// anonymous memory came: zeros.
fn fill(calls: &dyn Calls, dst: *mut u8, length: SizeT, fd: Fd, offset: OffT) -> Result<(), i32> {
    let mut done: SizeT = 0;
    while done < length {
        let at = OffT::try_from(done)
            .ok()
            .and_then(|d| offset.checked_add(d))
            .ok_or(errno::EOVERFLOW)?;
        // SAFETY: `dst` is `length` writable bytes, the mapping just made,
        // and `done < length`.
        let into = unsafe { dst.add(done) };
        let n = calls.read_at(fd, into, length.saturating_sub(done), at);
        match SizeT::try_from(n) {
            Ok(0) => break,
            Ok(n) => done = done.saturating_add(n),
            Err(_) => match errno::get_errno() {
                errno::EINTR => {}
                e => return Err(e),
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
