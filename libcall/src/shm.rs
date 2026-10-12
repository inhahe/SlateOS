//! System V shared memory (`<sys/shm.h>`), through the linked C library: a
//! segment made, attached, detached and removed.
//!
//! As much of it as a program needs that wants to see where the kernel maps
//! a segment: procps' `pmap` makes a private one, attaches it read-only,
//! finds it in its own `/proc/self/maps` -- the device minor shared memory is
//! given there names every other process's segments too -- and lets it go
//! (`discover_shm_minor`). On SlateOS the calls are `posix::sysv_shm`'s.
//!
//! On a host that is not unix every call answers [`ENOSYS`](crate::ENOSYS).

#[cfg(not(unix))]
use crate::ENOSYS;
#[cfg(unix)]
use crate::last_errno;

/// The key that always makes a new segment, which nothing else can name.
pub const IPC_PRIVATE: i32 = 0;
/// `shmget`'s flag: make the segment if the key names none.
pub const IPC_CREAT: i32 = 0o1000;
/// `shmctl`'s command: remove the segment once nothing is attached.
#[cfg(any(unix, test))]
const IPC_RMID: i32 = 0;
/// `shmat`'s flag: attach for reading only.
#[cfg(any(unix, test))]
const SHM_RDONLY: i32 = 0o10000;

#[cfg(unix)]
mod sys {
    unsafe extern "C" {
        pub fn shmget(key: i32, size: usize, shmflg: i32) -> i32;
        pub fn shmat(shmid: i32, shmaddr: *const u8, shmflg: i32) -> *mut u8;
        pub fn shmdt(shmaddr: *const u8) -> i32;
        pub fn shmctl(shmid: i32, cmd: i32, buf: *mut u8) -> i32;
    }
}

/// `shmget (key, size, flags)`: the segment's id. `flags` holds the mode's
/// permission bits as well as [`IPC_CREAT`].
///
/// # Errors
///
/// `EINVAL` for a size out of range, `ENOENT` for a key that names nothing
/// without [`IPC_CREAT`], `EACCES`, `ENOSPC`, `ENOMEM`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn shmget(key: i32, size: usize, flags: i32) -> Result<i32, i32> {
    shmget_one(key, size, flags)
}

#[cfg(unix)]
fn shmget_one(key: i32, size: usize, flags: i32) -> Result<i32, i32> {
    // SAFETY: three numbers; the call touches no memory of ours.
    let id = unsafe { sys::shmget(key, size, flags) };
    if id < 0 { Err(last_errno()) } else { Ok(id) }
}

#[cfg(not(unix))]
fn shmget_one(_key: i32, _size: usize, _flags: i32) -> Result<i32, i32> {
    Err(ENOSYS)
}

/// A segment attached to this process, until [`detach`] lets it go.
///
/// The pointer is kept rather than its number so that what goes back to
/// `shmdt` is what `shmat` gave. Nothing here reads or writes through it.
#[derive(Debug)]
pub struct Attached {
    #[cfg_attr(not(unix), allow(dead_code))]
    addr: *mut u8,
}

impl Attached {
    /// Where the segment was mapped, as a number to compare addresses with.
    #[must_use]
    pub fn address(&self) -> usize {
        self.addr.addr()
    }
}

/// `shmat (id, NULL, SHM_RDONLY)`: the segment attached for reading, at an
/// address the kernel picks.
///
/// # Errors
///
/// `EINVAL` for an id that names nothing, `EACCES`, `ENOMEM`, `EIDRM`;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn attach_read_only(id: i32) -> Result<Attached, i32> {
    attach_one(id)
}

#[cfg(unix)]
fn attach_one(id: i32) -> Result<Attached, i32> {
    // SAFETY: a null address asks the kernel to choose one; the call maps
    // the segment there and writes nothing of ours.
    let addr = unsafe { sys::shmat(id, core::ptr::null(), SHM_RDONLY) };
    // `(void *) -1` is the failure.
    if addr.addr() == usize::MAX {
        Err(last_errno())
    } else {
        Ok(Attached { addr })
    }
}

#[cfg(not(unix))]
fn attach_one(_id: i32) -> Result<Attached, i32> {
    Err(ENOSYS)
}

/// `shmdt`: the attachment let go.
///
/// # Errors
///
/// `EINVAL`, which an attachment `shmat` made does not give;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn detach(segment: Attached) -> Result<(), i32> {
    detach_one(&segment)
}

#[cfg(unix)]
fn detach_one(segment: &Attached) -> Result<(), i32> {
    // SAFETY: the address `shmat` returned, given back once -- `detach`
    // takes the `Attached` by value -- and nothing of ours points into it.
    let rc = unsafe { sys::shmdt(segment.addr) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn detach_one(_segment: &Attached) -> Result<(), i32> {
    Err(ENOSYS)
}

/// `shmctl (id, IPC_RMID, NULL)`: the segment removed once nothing is
/// attached to it.
///
/// # Errors
///
/// `EINVAL` for an id that names nothing, `EPERM` for someone else's;
/// [`ENOSYS`](crate::ENOSYS) off Unix.
pub fn remove(id: i32) -> Result<(), i32> {
    remove_one(id)
}

#[cfg(unix)]
fn remove_one(id: i32) -> Result<(), i32> {
    // SAFETY: `IPC_RMID` reads no buffer, so a null one is what the
    // interface asks for.
    let rc = unsafe { sys::shmctl(id, IPC_RMID, core::ptr::null_mut()) };
    if rc < 0 { Err(last_errno()) } else { Ok(()) }
}

#[cfg(not(unix))]
fn remove_one(_id: i32) -> Result<(), i32> {
    Err(ENOSYS)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// The restated numbers are the library's.
    #[test]
    fn the_numbers_are_the_librarys() {
        assert_eq!(IPC_PRIVATE, posix::sysv_shm::IPC_PRIVATE);
        assert_eq!(IPC_CREAT, posix::sysv_shm::IPC_CREAT);
        assert_eq!(IPC_RMID, posix::sysv_shm::IPC_RMID);
        assert_eq!(SHM_RDONLY, posix::sysv_shm::SHM_RDONLY);
    }

    /// A private segment made, attached, let go and removed -- `pmap`'s
    /// round trip -- and a removed one is gone.
    #[cfg(unix)]
    #[test]
    fn a_private_segment_round_trip() {
        let id = shmget(IPC_PRIVATE, 42, IPC_CREAT | 0o600).unwrap();
        let seg = attach_read_only(id).unwrap();
        assert_ne!(seg.address(), 0);
        detach(seg).unwrap();
        remove(id).unwrap();
        assert!(attach_read_only(id).is_err(), "removed");
        assert!(remove(id).is_err(), "removed twice");
    }

    #[cfg(not(unix))]
    #[test]
    fn off_unix_every_call_declines() {
        assert_eq!(shmget(IPC_PRIVATE, 42, IPC_CREAT), Err(ENOSYS));
        assert!(attach_read_only(0).is_err());
        assert_eq!(remove(0), Err(ENOSYS));
    }
}
