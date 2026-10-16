//! The device door: the sound card's ALSA devices, reached natively.
//!
//! Lane A's `SYS_DEVICE_OPEN` .. `SYS_DEVICE_CLOSE` (1119-1123) carry the
//! Linux device ABI to a native program. A native open of
//! `/dev/snd/pcmC0D0p` (playback), `/dev/snd/pcmC0D0c` (capture) or
//! `/dev/snd/controlC0` (the control device, which alsa-lib opens first to
//! find the card) yields a handle, and `ioctl`, `read` and `write` on it
//! carry exactly the requests and argument layouts a Linux program's do,
//! answered by the kernel's own ALSA handlers -- so this library passes them
//! through untouched (`requests/e-ad-no-application-can-reach-the-sound-
//! device.md`). Before it, a native program could open neither device, and
//! `ioctl` answered every request but a terminal's with `ENOTTY`.
//!
//! # What is the device's, and what is the descriptor's
//!
//! As on Linux, the device answers its own requests and transfers, and the
//! descriptor's state is the descriptor's: `FIONBIO`, `FIOCLEX` and
//! `FIONCLEX` never reach the device (`crate::ioctl`), the access mode an
//! `open` asked for decides whether `read` and `write` reach it at all
//! (`vfs_read`'s and `vfs_write`'s `EBADF`, before the driver), and neither
//! device seeks (`ESPIPE`, as `no_llseek` answers).
//!
//! # Answers
//!
//! Every call in the family answers as the device's ABI does: a value of 0
//! or more, or a negated Linux errno, which goes to `errno` unchanged -- this
//! library's errno numbers are Linux's. An interrupted wait is the kernel's
//! restart, resolved as for every slow native call: with no handler run the
//! call resumes by itself, and when a native handler runs the call answers
//! `Interrupted` in the native code, which [`crate::interrupt::restarting`]
//! turns into a restart or `EINTR` as the handler's `SA_RESTART` says. (That
//! one native code shares its number with a negated `ENOEXEC`, which no
//! sound device answers.)
//!
//! # Descriptors
//!
//! [`HandleKind::DevicePcm`] and [`HandleKind::DeviceControl`], one kind per
//! device class as the pty's ends are, so a dispatch site that forgets one is
//! a non-exhaustive `match`. A PCM handle is the process's: closed at its
//! exit, one more holder at its `fork`, waitable through `SYS_WAIT_MULTIPLE`
//! (kind 22). The control device keeps nothing per open; its handle is
//! always 1. A `dup` shares the handle, as it shares an open file
//! description. The path a descriptor was opened by is kept, as a file's is,
//! so that `fstat` can say which node it is and `/proc/self/fd` can name it;
//! opening that name again opens the device again, as Linux's reopen does.
//!
//! Neither kind crosses `exec` or spawn yet: the kernel's descriptor map has
//! no handle type for them, so a child that needs the card opens it itself,
//! and a handle an `exec` leaves behind is the process's until it exits
//! (`known-issues/D-POSIX-A-SOUND-DEVICE-DOES-NOT-CROSS-EXEC.md`).
//!
//! # Where this is not Linux
//!
//! - `readv` and `writev` on a PCM are a `read` or a `write` per segment, as
//!   lane A's own Linux `readv` and `writev` are. Linux's are the
//!   non-interleaved transfer -- one segment per channel, for a stream
//!   configured `SNDRV_PCM_ACCESS_RW_NONINTERLEAVED`, `EINVAL` otherwise --
//!   which no door call carries; alsa-lib transfers through
//!   `SNDRV_PCM_IOCTL_WRITEN_FRAMES` and its kin instead, which are the
//!   device's and pass through.
//! - The control device polls as having nothing to report, which is Linux's
//!   answer for one subscribed to no events (`snd_ctl_poll`); one that is
//!   subscribed would be readable when an event waits, and the kernel has
//!   no readiness to ask for it.

use crate::errno;
use crate::fcntl::{O_ACCMODE, O_APPEND, O_CREAT, O_DIRECTORY, O_EXCL, O_NONBLOCK, O_PATH};
use crate::fcntl::{O_CLOEXEC, O_RDONLY, O_RDWR, O_SYNC, O_WRONLY};
use crate::fdtable::{self, FdEntry, HandleKind};
use crate::syscall::{syscall2, syscall3, syscall5};

/// The kernel's device kind for an ALSA PCM substream.
const DEVICE_KIND_PCM: u64 = 1;
/// The kernel's device kind for the ALSA control device.
const DEVICE_KIND_CONTROL: u64 = 2;
/// Call flag: the descriptor is non-blocking (`O_NONBLOCK`): a write or a
/// `DRAIN` that would wait answers `EAGAIN` instead.
const DEVICE_NONBLOCK: u64 = 1;
/// `SYS_WAIT_MULTIPLE`'s kind for a PCM handle: the kernel's
/// `ResourceType::AlsaPcm`.
const WAIT_KIND_ALSA_PCM: u32 = 22;

/// ALSA's major device number, which every one of its nodes has.
const ALSA_MAJOR: u32 = 116;

/// Whether a descriptor of `kind` is one of the door's devices.
pub(crate) const fn is_device(kind: HandleKind) -> bool {
    matches!(kind, HandleKind::DevicePcm | HandleKind::DeviceControl)
}

/// The descriptor kind for a path the door serves, and the node's minor
/// number as Linux numbers it without dynamic minors (card 0: the control
/// device 0, playback device 0 16, capture device 0 24); `None` for any
/// other path.
pub(crate) fn node(resolved: &[u8]) -> Option<(HandleKind, u32)> {
    match resolved {
        b"/dev/snd/controlC0" => Some((HandleKind::DeviceControl, 0)),
        b"/dev/snd/pcmC0D0p" => Some((HandleKind::DevicePcm, 16)),
        b"/dev/snd/pcmC0D0c" => Some((HandleKind::DevicePcm, 24)),
        _ => None,
    }
}

/// The kernel's device kind for a descriptor kind; `None` for a kind that is
/// no device's.
fn door_kind(kind: HandleKind) -> Option<u64> {
    match kind {
        HandleKind::DevicePcm => Some(DEVICE_KIND_PCM),
        HandleKind::DeviceControl => Some(DEVICE_KIND_CONTROL),
        _ => None,
    }
}

/// A door call's answer `ret`: the value, or the errno -- a negated Linux
/// errno's own number, `EINTR` for the native `Interrupted` a handler's run
/// leaves, `ENOSYS` from a kernel without the door.
fn answer(ret: i64) -> Result<i64, i32> {
    if ret >= 0 {
        return Ok(ret);
    }
    Err(errno::door_errno(ret))
}

/// The call flags a descriptor's status flags ask for.
fn call_flags(entry: &FdEntry) -> u64 {
    if entry.status_flags & O_NONBLOCK != 0 {
        DEVICE_NONBLOCK
    } else {
        0
    }
}

/// `open` of a path the door serves: `None` for any other path, and for
/// every path on a kernel without the door (or, in the host build, none) --
/// which then opens through the filesystem as before, and is told there is
/// no such file. `None` too for `O_PATH`, which names a node without opening
/// its device, as Linux's never reaches the driver. Otherwise the
/// descriptor, or -1 with errno: `ENOTDIR` for `O_DIRECTORY` and `EEXIST` for
/// `O_CREAT | O_EXCL`, as Linux's `do_open` answers for a node that exists
/// before the device is asked; `ENODEV` when there is no sound card to play
/// through, as the device's own open answers.
pub(crate) fn open(resolved: &[u8], flags: i32) -> Option<i32> {
    let (kind, _) = node(resolved)?;
    if flags & O_PATH != 0 {
        return None;
    }
    if flags & O_DIRECTORY != 0 {
        errno::set_errno(errno::ENOTDIR);
        return Some(-1);
    }
    if flags & (O_CREAT | O_EXCL) == O_CREAT | O_EXCL {
        errno::set_errno(errno::EEXIST);
        return Some(-1);
    }
    let ret = syscall2(
        crate::syscall::SYS_DEVICE_OPEN,
        resolved.as_ptr() as u64,
        resolved.len() as u64,
    );
    if crate::syscall::lacks_call(ret) {
        return None;
    }
    let handle = match answer(ret) {
        Ok(h) => h.unsigned_abs(),
        Err(e) => {
            errno::set_errno(e);
            return Some(-1);
        }
    };
    // What `F_GETFL` reports, as a file's open keeps it.
    let stored = flags & (O_ACCMODE | O_APPEND | O_NONBLOCK | O_SYNC);
    let Some(fd) = fdtable::alloc_fd_with_flags(kind, handle, stored) else {
        // The handle was made for this call and no descriptor holds it: let
        // it go, as its last close would. Its answer cannot change EMFILE.
        let _ = close(kind, handle);
        errno::set_errno(errno::EMFILE);
        return Some(-1);
    };
    if flags & O_CLOEXEC != 0 {
        // `fd` was allocated just above, so it is open: the flag is set.
        let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
    }
    fdtable::store_fd_path(fd, resolved.as_ptr(), resolved.len());
    Some(fd)
}

/// `open` of descriptor `n`'s `/proc/self/fd` or `/dev/fd` name, `n` being a
/// device's: the device opened again by the path `n` was opened by, as
/// Linux's reopen of the node opens it again -- a playback stream of its
/// own, not `n`'s shared. -1 with errno on failure: `ENOENT` should `n` have
/// no path, which a device descriptor always has.
pub(crate) fn reopen(n: i32, flags: i32) -> i32 {
    let mut path = [0u8; crate::unistd::PATH_MAX];
    let len = fdtable::get_fd_path(n, &mut path);
    match path
        .get(..len)
        .filter(|p| !p.is_empty())
        .and_then(|p| open(p, flags))
    {
        Some(fd) => fd,
        None => {
            errno::set_errno(errno::ENOENT);
            -1
        }
    }
}

/// Whether an open with access mode `acc` may read (`FMODE_READ`, as
/// Linux's `OPEN_FMODE` derives it: `O_RDONLY` and `O_RDWR`, not the mode
/// 3 that opens for `ioctl` alone).
const fn may_read(acc: i32) -> bool {
    acc == O_RDONLY || acc == O_RDWR
}

/// Whether an open with access mode `acc` may write (`FMODE_WRITE`).
const fn may_write(acc: i32) -> bool {
    acc == O_WRONLY || acc == O_RDWR
}

/// `read` of a device descriptor: the bytes read, or -1 with errno. `EBADF`
/// for a descriptor not opened for reading, before the device is asked, as
/// `vfs_read` answers; otherwise the device's answer -- a capture PCM's
/// frames, and for the control device whatever its read answers (Linux's
/// is `EBADFD` for one subscribed to no events).
pub(crate) fn read(entry: &FdEntry, buf: *mut u8, len: usize) -> isize {
    if !may_read(entry.status_flags & O_ACCMODE) {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    transfer(entry, crate::syscall::SYS_DEVICE_READ, buf as u64, len)
}

/// `write` of a device descriptor: the bytes written, or -1 with errno.
/// `EBADF` for a descriptor not opened for writing, as `vfs_write` answers.
/// A blocking PCM write waits until all of it is queued; a non-blocking one
/// takes what fits, or `EAGAIN`.
pub(crate) fn write(entry: &FdEntry, buf: *const u8, len: usize) -> isize {
    if !may_write(entry.status_flags & O_ACCMODE) {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    transfer(entry, crate::syscall::SYS_DEVICE_WRITE, buf as u64, len)
}

/// [`read`]'s and [`write`]'s call. The buffer and the count go to the
/// device as they came, a null buffer and a zero count included: the device
/// answers them as Linux's driver does (`EFAULT` where it would copy).
fn transfer(entry: &FdEntry, nr: u64, buf: u64, len: usize) -> isize {
    let Some(kind) = door_kind(entry.kind) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    let flags = call_flags(entry);
    let ret = crate::interrupt::restarting(crate::interrupt::Restart::IfAsked, || {
        syscall5(nr, kind, entry.handle, buf, len as u64, flags)
    });
    match answer(ret) {
        Ok(n) => isize::try_from(n).unwrap_or(isize::MAX),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// `ioctl(fd, request, arg)` on a device descriptor: the request passed
/// through as a Linux `ioctl` on the node -- `request`'s low 32 bits, as
/// Linux reads it -- and the device's answer: 0 or more, or -1 with errno.
/// `ENOTTY` for a request the device does not know, the terminal requests
/// among them. (The descriptor's own requests never reach here.)
pub(crate) fn ioctl(entry: &FdEntry, request: u64, arg: *mut u8) -> i32 {
    let Some(kind) = door_kind(entry.kind) else {
        errno::set_errno(errno::ENOTTY);
        return -1;
    };
    let flags = call_flags(entry);
    let ret = crate::interrupt::restarting(crate::interrupt::Restart::IfAsked, || {
        syscall5(
            crate::syscall::SYS_DEVICE_IOCTL,
            kind,
            entry.handle,
            request & 0xFFFF_FFFF,
            arg as u64,
            flags,
        )
    });
    match answer(ret) {
        Ok(v) => i32::try_from(v).unwrap_or(i32::MAX),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// Let go of a device handle: the descriptor's last close. 0, or -1 with
/// errno.
pub(crate) fn close(kind: HandleKind, handle: u64) -> i32 {
    let Some(door) = door_kind(kind) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    match answer(syscall2(crate::syscall::SYS_DEVICE_CLOSE, door, handle)) {
        Ok(_) => 0,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// Let go of a device handle whose answer nobody reads: the descriptor a
/// `dup2` evicts, whose close Linux reports to no one. `errno` is left as it
/// was, so the `dup2` that succeeded says nothing of it.
pub(crate) fn release(kind: HandleKind, handle: u64) {
    if let Some(door) = door_kind(kind) {
        // Discarded by design: an evicted descriptor's close has no caller
        // to tell, on Linux as here.
        let _ = syscall2(crate::syscall::SYS_DEVICE_CLOSE, door, handle);
    }
}

/// One `SYS_WAIT_MULTIPLE` item, as the kernel lays it out: 24 bytes, the
/// last four padding.
#[repr(C)]
struct WaitItem {
    handle: u64,
    kind: u32,
    events: u32,
    revents: u32,
    pad: u32,
}

/// `poll`'s view of a device descriptor, as `(readable, writable, hangup,
/// error)`. A PCM's comes from the kernel, asked once without waiting
/// (`SYS_WAIT_MULTIPLE` with no time): writable while its ring has room,
/// readable for capture -- what a player waiting for room waits on. The
/// control device has nothing to report (the module doc says why).
pub(crate) fn readiness(kind: HandleKind, handle: u64) -> (bool, bool, bool, bool) {
    if kind != HandleKind::DevicePcm {
        return (false, false, false, false);
    }
    let pollin = crate::poll::POLLIN.unsigned_abs();
    let pollout = crate::poll::POLLOUT.unsigned_abs();
    let mut item = WaitItem {
        handle,
        kind: WAIT_KIND_ALSA_PCM,
        events: u32::from(pollin | pollout),
        revents: 0,
        pad: 0,
    };
    let ret = syscall3(
        crate::syscall::SYS_WAIT_MULTIPLE,
        (&raw mut item) as u64,
        1,
        0,
    );
    if ret < 0 {
        // The kernel could not say: the handle is gone or was never ours.
        // Readable and in error, so a caller blocked on it is released
        // rather than parked on a device that cannot answer.
        return (true, false, true, true);
    }
    readiness_of(item.revents)
}

/// `(readable, writable, hangup, error)` from a `poll` revents word.
fn readiness_of(revents: u32) -> (bool, bool, bool, bool) {
    let bit = |b: i16| revents & u32::from(b.unsigned_abs()) != 0;
    (
        bit(crate::poll::POLLIN),
        bit(crate::poll::POLLOUT),
        bit(crate::poll::POLLHUP),
        bit(crate::poll::POLLERR) || bit(crate::poll::POLLNVAL),
    )
}

/// The `st_mode` and `st_rdev` of the node a device descriptor was opened
/// by: a character device, `crw-rw----` as udev makes the sound nodes, with
/// ALSA's numbers -- major 116, and the minor [`node`] gives, encoded as
/// glibc's `makedev` encodes them. `None` for a path the door does not serve.
pub(crate) fn stat_of(path: &[u8]) -> Option<(u32, u64)> {
    let (_, minor) = node(path)?;
    let mode = crate::fcntl::S_IFCHR | 0o660;
    Some((mode, crate::crt::gnu_dev_makedev(ALSA_MAJOR, minor)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three nodes, their kinds and Linux's numbers; nothing else.
    #[test]
    fn the_door_serves_three_nodes() {
        assert_eq!(
            node(b"/dev/snd/controlC0"),
            Some((HandleKind::DeviceControl, 0))
        );
        assert_eq!(
            node(b"/dev/snd/pcmC0D0p"),
            Some((HandleKind::DevicePcm, 16))
        );
        assert_eq!(
            node(b"/dev/snd/pcmC0D0c"),
            Some((HandleKind::DevicePcm, 24))
        );
        assert_eq!(node(b"/dev/snd/pcmC0D1p"), None);
        assert_eq!(node(b"/dev/snd"), None);
        assert_eq!(node(b"/dev/null"), None);
        assert_eq!(door_kind(HandleKind::DevicePcm), Some(1));
        assert_eq!(door_kind(HandleKind::DeviceControl), Some(2));
        assert_eq!(door_kind(HandleKind::File), None);
        assert!(is_device(HandleKind::DevicePcm) && is_device(HandleKind::DeviceControl));
        assert!(!is_device(HandleKind::File) && !is_device(HandleKind::PtyMaster));
    }

    /// A door call answers a value, a negated Linux errno, or the native
    /// `Interrupted` a handler's run leaves -- `EINTR`.
    #[test]
    fn a_door_answer_is_a_value_or_a_linux_errno() {
        assert_eq!(answer(0), Ok(0));
        assert_eq!(answer(4096), Ok(4096));
        assert_eq!(answer(-i64::from(errno::ENOTTY)), Err(errno::ENOTTY));
        assert_eq!(answer(-i64::from(errno::EAGAIN)), Err(errno::EAGAIN));
        assert_eq!(answer(-i64::from(errno::ENODEV)), Err(errno::ENODEV));
        assert_eq!(answer(-i64::from(errno::EBADFD)), Err(errno::EBADFD));
        assert_eq!(answer(errno::native::INTERRUPTED), Err(errno::EINTR));
        assert_eq!(answer(errno::native::NO_SUCH_SYSCALL), Err(errno::ENOSYS));
    }

    /// The host has no kernel and so no door: its paths open through the
    /// filesystem as before (`None`), and no descriptor is made. `O_PATH`
    /// never asks the door at all.
    #[test]
    fn the_host_has_no_door() {
        assert_eq!(open(b"/dev/snd/pcmC0D0p", O_RDWR), None);
        assert_eq!(open(b"/dev/snd/controlC0", O_RDWR | O_PATH), None);
        assert_eq!(open(b"/tmp/x", O_RDWR), None);
    }

    /// What Linux's `do_open` answers for a node that exists, before the
    /// device is asked: `ENOTDIR` for `O_DIRECTORY`, `EEXIST` for
    /// `O_CREAT | O_EXCL` -- and `O_CREAT` alone opens it.
    #[test]
    fn a_node_is_not_a_directory_and_it_exists() {
        errno::set_errno(0);
        assert_eq!(open(b"/dev/snd/pcmC0D0p", O_RDWR | O_DIRECTORY), Some(-1));
        assert_eq!(errno::get_errno(), errno::ENOTDIR);
        errno::set_errno(0);
        assert_eq!(
            open(b"/dev/snd/controlC0", O_RDWR | O_CREAT | O_EXCL),
            Some(-1)
        );
        assert_eq!(errno::get_errno(), errno::EEXIST);
        // O_CREAT alone reaches the door, which the host has not got.
        assert_eq!(open(b"/dev/snd/controlC0", O_RDWR | O_CREAT), None);
    }

    /// The access mode decides whether a transfer reaches the device:
    /// `EBADF` before the kernel is asked, as `vfs_read`/`vfs_write` answer.
    #[test]
    fn the_access_mode_is_the_descriptors() {
        assert!(may_read(O_RDONLY) && may_read(O_RDWR) && !may_read(O_WRONLY));
        assert!(may_write(O_WRONLY) && may_write(O_RDWR) && !may_write(O_RDONLY));
        // Mode 3 opens for ioctl alone: neither.
        assert!(!may_read(3) && !may_write(3));
        let entry = |status_flags| FdEntry {
            kind: HandleKind::DevicePcm,
            handle: 7,
            flags: 0,
            status_flags,
        };
        let mut byte = 0u8;
        errno::set_errno(0);
        assert_eq!(read(&entry(O_WRONLY), &raw mut byte, 1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        errno::set_errno(0);
        assert_eq!(write(&entry(O_RDONLY), &raw const byte, 1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    /// `poll`'s bits, read as Linux's; the control device reports nothing.
    #[test]
    fn readiness_reads_poll_bits() {
        let r = |b: i16| u32::from(b.unsigned_abs());
        assert_eq!(readiness_of(0), (false, false, false, false));
        assert_eq!(
            readiness_of(r(crate::poll::POLLOUT)),
            (false, true, false, false)
        );
        assert_eq!(
            readiness_of(r(crate::poll::POLLIN)),
            (true, false, false, false)
        );
        assert_eq!(
            readiness_of(r(crate::poll::POLLHUP) | r(crate::poll::POLLNVAL)),
            (false, false, true, true)
        );
        assert_eq!(
            readiness(HandleKind::DeviceControl, 1),
            (false, false, false, false)
        );
        // The kernel's item is 24 bytes.
        assert_eq!(core::mem::size_of::<WaitItem>(), 24);
    }

    /// A node's stat: a character device with Linux's numbers.
    #[test]
    fn a_device_node_stats_as_linux_numbers_it() {
        let (mode, rdev) = stat_of(b"/dev/snd/pcmC0D0p").expect("a node");
        assert_eq!(mode & crate::fcntl::S_IFMT, crate::fcntl::S_IFCHR);
        assert_eq!(mode & 0o777, 0o660);
        assert_eq!(rdev, (116 << 8) | 16);
        assert_eq!(stat_of(b"/dev/snd/controlC0").map(|s| s.1), Some(116 << 8));
        assert_eq!(
            stat_of(b"/dev/snd/pcmC0D0c").map(|s| s.1),
            Some((116 << 8) | 24)
        );
        // Read back as `major(3)` and `minor(3)` read them.
        let rdev = stat_of(b"/dev/snd/pcmC0D0c").map_or(0, |s| s.1);
        assert_eq!(crate::crt::gnu_dev_major(rdev), 116);
        assert_eq!(crate::crt::gnu_dev_minor(rdev), 24);
        assert_eq!(stat_of(b"/etc/passwd"), None);
    }
}
