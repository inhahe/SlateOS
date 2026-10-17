//! File-backed `mmap`: the kernel's (`SYS_MMAP_FILE`), after the refusals
//! only this library can make.
//!
//! # What a mapping is
//!
//! The kernel's, as the Linux ABI's `mmap` makes it: a private mapping is
//! demand-paged from the file, each page read at its first touch; a shared
//! one maps the file's pages in the page cache, so it shows what others
//! write to the file. A page past the end of the file reads as zeros. The
//! mapping holds its own reference to the file, so the descriptor may be
//! closed after.
//!
//! A shared, writable mapping is the kernel's to grant: one that saves its
//! stores to the file maps it (and `msync` saves them at once,
//! `SYS_MSYNC`); one that cannot yet answers `ENOSYS`, which `mmap` never
//! answers on Linux, so it is reported as `ENODEV` -- "the file system does
//! not support memory mapping" -- and a program is not told its data are
//! saved when they are not.
//!
//! Until 2026-10-17 this library copied the file into anonymous memory at
//! `mmap`, all of it at once, and refused every shared, writable mapping:
//! the native kernel had no way to map a file until 2026-10-07
//! (`requests/d-a-a-native-program-cannot-map-a-file.md`).
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
//! The descriptor's kind and its access mode are this library's to know --
//! the kernel sees a handle -- so these are decided here, and only a mapping
//! that passes them reaches the kernel, which answers the rest.

use core::ffi::c_void;

use super::{MAP_FAILED, MAP_PRIVATE, MAP_TYPE, PROT_GROWSDOWN, PROT_GROWSUP, PROT_WRITE};
use crate::errno;
use crate::fcntl::{O_ACCMODE, O_RDONLY, O_RDWR, O_WRONLY, S_IFDIR, S_IFMT};
use crate::fdtable::{self, HandleKind};
use crate::types::{Fd, OffT, SizeT};

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

/// What a mapping is made with: the kernel, or the tests' stand-in.
trait Calls {
    /// `SYS_MMAP_FILE`: the address mapped, or Linux's errno as `-errno` (or
    /// a native code from past the call's body).
    fn map_file(
        &self,
        addr: *mut c_void,
        length: SizeT,
        prot: i32,
        flags: i32,
        handle: u64,
        offset: OffT,
    ) -> i64;
    /// Whether `fd` is a directory.
    fn is_directory(&self, fd: Fd) -> bool;
}

/// The kernel.
struct System;

impl Calls for System {
    fn map_file(
        &self,
        addr: *mut c_void,
        length: SizeT,
        prot: i32,
        flags: i32,
        handle: u64,
        offset: OffT,
    ) -> i64 {
        // `prot` and `flags` are Linux's bits, which the kernel reads as
        // they are; their `u32` is the same bits. `offset` is non-negative
        // ([`refusal`]).
        crate::syscall::syscall6(
            crate::syscall::SYS_MMAP_FILE,
            addr as u64,
            length as u64,
            u64::from(prot.cast_unsigned()),
            u64::from(flags.cast_unsigned()),
            handle,
            offset.cast_unsigned(),
        )
    }

    fn is_directory(&self, fd: Fd) -> bool {
        let mut st = core::mem::MaybeUninit::<crate::stat::Stat>::zeroed();
        // A file that cannot be asked about is taken for one that is not a
        // directory: the kernel then answers for it.
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
    let handle = match refusal(calls, length, prot, flags, fd, offset) {
        Ok(handle) => handle,
        Err(e) => {
            errno::set_errno(e);
            return MAP_FAILED;
        }
    };
    match mapped_at(calls.map_file(addr, length, prot, flags, handle, offset)) {
        Ok(at) => at as *mut c_void,
        Err(e) => {
            errno::set_errno(e);
            MAP_FAILED
        }
    }
}

/// `SYS_MMAP_FILE`'s answer `ret`: the address, or the errno -- its Linux
/// errnos as they are, but `ENOSYS`, a kernel's that cannot save a shared
/// mapping's stores yet (or has no such call), as `ENODEV`.
fn mapped_at(ret: i64) -> Result<usize, i32> {
    if ret >= 0 {
        return usize::try_from(ret).map_err(|_| errno::ENOMEM);
    }
    match errno::door_errno(ret) {
        errno::ENOSYS => Err(errno::ENODEV),
        e => Err(e),
    }
}

/// Why `fd` cannot be mapped so, in Linux's order (the module's doc); or the
/// handle of the file to map.
fn refusal(
    calls: &dyn Calls,
    length: SizeT,
    prot: i32,
    flags: i32,
    fd: Fd,
    offset: OffT,
) -> Result<u64, i32> {
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
    Ok(entry.handle)
}

#[cfg(test)]
mod tests;
