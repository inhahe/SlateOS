//! POSIX record locks for both ABIs: the Linux `fcntl` `F_GETLK`, `F_SETLK`
//! and `F_SETLKW` (with their `F_OFD_*` forms), and the native
//! [`SYS_FS_RECORD_LOCK`](super::number::SYS_FS_RECORD_LOCK).
//!
//! [`crate::fs::reclock`] is the table. This module is what both syscall
//! layers do around it, written once: read the caller's `struct flock`,
//! resolve its range against the open file, apply POSIX's access-mode rule,
//! take, test or release the lock, and fill in `F_GETLK`'s answer. Each ABI's
//! own layer only finds the open file, says whose lock it is, moves the
//! `struct flock` in and out of user memory, and translates the error.
//!
//! Until 2026-10-01 this lived inside `linux.rs`. So a native program could
//! not reach the lock table at all
//! (`requests/d-a-native-programs-cannot-reach-the-record-lock-table.md`):
//! its libc answered "granted" to every lock. And `F_SETLKW` answered `EAGAIN`
//! instead of waiting.

use crate::error::{KernelError, KernelResult};
use crate::fs::reclock::{self, LockKey, RecordLock, RecordLockType};

/// `l_type`: a read (shared) lock.
pub const F_RDLCK: i16 = 0;
/// `l_type`: a write (exclusive) lock.
pub const F_WRLCK: i16 = 1;
/// `l_type`: a release, or `F_GETLK`'s "nothing in the way".
pub const F_UNLCK: i16 = 2;

/// `l_whence`: from the start of the file.
pub const SEEK_SET: i16 = 0;
/// `l_whence`: from the description's offset.
pub const SEEK_CUR: i16 = 1;
/// `l_whence`: from the end of the file.
pub const SEEK_END: i16 = 2;

/// `struct flock` as x86-64 Linux lays it out, which the native call takes
/// as well, so libc passes its own structure through:
///
/// ```text
/// 0   i16  l_type
/// 2   i16  l_whence
/// 4   [4]  padding
/// 8   i64  l_start
/// 16  i64  l_len
/// 24  i32  l_pid
/// 28  [4]  padding
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flock {
    pub l_type: i16,
    pub l_whence: i16,
    pub l_start: i64,
    pub l_len: i64,
    pub l_pid: i32,
    /// The padding after `l_whence`, as the caller wrote it.
    pad_whence: [u8; 4],
    /// The padding after `l_pid`, as the caller wrote it.
    pad_end: [u8; 4],
}

impl Flock {
    /// Its size in user memory.
    pub const SIZE: usize = 32;

    /// A request built in the kernel, for the self-tests.
    #[must_use]
    pub const fn new(l_type: i16, l_whence: i16, l_start: i64, l_len: i64) -> Self {
        Self {
            l_type,
            l_whence,
            l_start,
            l_len,
            l_pid: 0,
            pad_whence: [0; 4],
            pad_end: [0; 4],
        }
    }

    /// The same request with `l_pid` set: the field `F_GETLK` reports, and
    /// one an OFD request must leave 0. For the self-tests.
    #[must_use]
    pub const fn with_pid(self, l_pid: i32) -> Self {
        Self { l_pid, ..self }
    }

    /// Decode the user's bytes. Every bit pattern is a `Flock`; validity is
    /// [`apply`]'s question.
    #[must_use]
    pub fn from_bytes(bytes: [u8; Self::SIZE]) -> Self {
        let [
            t0,
            t1,
            w0,
            w1,
            a0,
            a1,
            a2,
            a3,
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            n0,
            n1,
            n2,
            n3,
            n4,
            n5,
            n6,
            n7,
            p0,
            p1,
            p2,
            p3,
            e0,
            e1,
            e2,
            e3,
        ] = bytes;
        Self {
            l_type: i16::from_le_bytes([t0, t1]),
            l_whence: i16::from_le_bytes([w0, w1]),
            l_start: i64::from_le_bytes([s0, s1, s2, s3, s4, s5, s6, s7]),
            l_len: i64::from_le_bytes([n0, n1, n2, n3, n4, n5, n6, n7]),
            l_pid: i32::from_le_bytes([p0, p1, p2, p3]),
            pad_whence: [a0, a1, a2, a3],
            pad_end: [e0, e1, e2, e3],
        }
    }

    /// Encode for the user, the padding as it came in: `F_GETLK` rewrites
    /// the fields it answers and nothing else.
    #[must_use]
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        let fields = self
            .l_type
            .to_le_bytes()
            .into_iter()
            .chain(self.l_whence.to_le_bytes())
            .chain(self.pad_whence)
            .chain(self.l_start.to_le_bytes())
            .chain(self.l_len.to_le_bytes())
            .chain(self.l_pid.to_le_bytes())
            .chain(self.pad_end);
        for (dst, src) in out.iter_mut().zip(fields) {
            *dst = src;
        }
        out
    }

    /// Read the caller's `struct flock`.
    ///
    /// # Errors
    ///
    /// `InvalidAddress` for a null pointer or memory the caller cannot read.
    pub fn read_user(ptr: u64) -> KernelResult<Self> {
        if ptr == 0 {
            return Err(KernelError::InvalidAddress);
        }
        crate::mm::user::read_user_value::<[u8; Self::SIZE]>(ptr).map(Self::from_bytes)
    }

    /// Write it back to the caller.
    ///
    /// # Errors
    ///
    /// `InvalidAddress` for memory the caller cannot write.
    pub fn write_user(self, ptr: u64) -> KernelResult<()> {
        crate::mm::user::write_user_value(ptr, self.to_bytes())
    }
}

/// What a request does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `F_GETLK`: would this lock be granted, and if not, whose is in the way?
    Get,
    /// `F_SETLK`: take or release it now, or `WouldBlock`.
    Set,
    /// `F_SETLKW`: take it, waiting while another owner's lock is in the way.
    SetWait,
}

/// Whose lock a request takes, or asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// A process, by pid (0 for a kernel task): `F_SETLK` and the native
    /// call. Its locks end when it exits, or when it closes any descriptor
    /// for the file.
    Process(u64),
    /// The open file description the request names: Linux's `F_OFD_*`. Its
    /// locks end at the description's final close.
    Description,
}

/// The open file a request is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A VFS file, by its `fs::handle` handle.
    File(u64),
    /// A memfd, by its id.
    MemFd(u64),
}

impl Target {
    fn memfd(id: u64) -> crate::ipc::memfd::MemFdHandle {
        crate::ipc::memfd::MemFdHandle::from_raw(id)
    }

    /// Which file the table keys it by.
    fn key(self) -> KernelResult<LockKey> {
        match self {
            Self::File(handle) => LockKey::for_handle(handle),
            Self::MemFd(id) => {
                // Asked so that a memfd closed meanwhile is `InvalidHandle`
                // here, as a closed file handle is.
                crate::ipc::memfd::size(Self::memfd(id))?;
                Ok(LockKey::for_memfd(id))
            }
        }
    }

    /// The description's offset: `SEEK_CUR`'s base. Read, not moved: asking
    /// about a lock must not seek the caller's descriptor.
    fn offset(self) -> KernelResult<u64> {
        match self {
            Self::File(handle) => {
                crate::fs::handle::seek(handle, crate::fs::handle::SeekFrom::Current(0))
            }
            Self::MemFd(id) => crate::ipc::memfd::offset(Self::memfd(id)),
        }
    }

    /// The file's size: `SEEK_END`'s base.
    fn size(self) -> KernelResult<u64> {
        match self {
            Self::File(handle) => crate::fs::handle::fstat(handle).map(|meta| meta.size),
            Self::MemFd(id) => crate::ipc::memfd::size(Self::memfd(id)),
        }
    }

    /// Whether the description was opened for reading, and for writing.
    fn access(self) -> KernelResult<(bool, bool)> {
        match self {
            Self::File(handle) => crate::fs::handle::open_flags(handle)
                .map(|flags| (flags.is_readable(), flags.is_writable())),
            // `memfd_create` opens read-write, and nothing reopens one with
            // less.
            Self::MemFd(id) => crate::ipc::memfd::size(Self::memfd(id)).map(|_| (true, true)),
        }
    }

    /// Whether the open file description still exists.
    fn exists(self) -> bool {
        self.access().is_ok()
    }

    /// The owner value of the description itself (`F_OFD_*`).
    fn description_owner(self) -> u64 {
        match self {
            Self::File(handle) => reclock::ofd_owner(handle),
            Self::MemFd(id) => reclock::memfd_ofd_owner(id),
        }
    }
}

/// Resolve a `struct flock`'s `(l_whence, l_start, l_len)` into an absolute
/// half-open byte range `(start, len)`, `len == 0` meaning to end of file.
///
/// `cur_offset` is read for `SEEK_CUR` and `file_size` for `SEEK_END`; the
/// other is ignored. Split out so it can be tested without a process, a
/// descriptor or a file -- and because two of its cases are easy to get
/// silently wrong on a path that decides who may write to what:
///
/// * `l_len == 0` means **to end of file**, not an empty range. It is passed
///   through as 0, which `RecordLock::end` already reads as `u64::MAX`.
/// * a **negative** `l_len` describes the range *below* the anchor:
///   `[l_start + l_len, l_start)`. Treating it as an empty or forward range
///   locks bytes the caller never named, which is worse than refusing.
///
/// # Errors
///
/// `InvalidArgument` (`EINVAL`) for an unknown `l_whence`, a range starting
/// before byte zero, or one whose bounds do not fit a signed 64-bit offset.
pub fn flock_range(
    whence: i16,
    l_start: i64,
    l_len: i64,
    cur_offset: u64,
    file_size: u64,
) -> KernelResult<(u64, u64)> {
    const EINVAL: KernelError = KernelError::InvalidArgument;
    let base: i64 = match whence {
        SEEK_SET => 0,
        SEEK_CUR => i64::try_from(cur_offset).map_err(|_| EINVAL)?,
        SEEK_END => i64::try_from(file_size).map_err(|_| EINVAL)?,
        _ => return Err(EINVAL),
    };
    let anchor = base.checked_add(l_start).ok_or(EINVAL)?;

    let (start, len) = if l_len < 0 {
        // POSIX: the range is [anchor + l_len, anchor).
        let s = anchor.checked_add(l_len).ok_or(EINVAL)?;
        let n = l_len.checked_neg().ok_or(EINVAL)?;
        (s, n)
    } else {
        // The END of the range must be representable too. The negative branch
        // above gets this free from its own `checked_add`; this branch did not
        // check at all, so `l_start = i64::MAX, l_len = 1` returned a range
        // whose end does not exist -- a lock that looks one byte wide and is
        // not. Found by hand-checking the self-test's own cases against this
        // function before the boot reached them.
        anchor.checked_add(l_len).ok_or(EINVAL)?;
        (anchor, l_len)
    };

    // A range starting before byte zero is EINVAL, not a clamp: clamping would
    // silently widen the lock.
    if start < 0 {
        return Err(EINVAL);
    }
    let start_u = u64::try_from(start).map_err(|_| EINVAL)?;
    let len_u = u64::try_from(len).map_err(|_| EINVAL)?;
    Ok((start_u, len_u))
}

/// Carry out one record-lock request on `target`. For [`Op::Get`], `flock`
/// is rewritten with the answer.
///
/// `still_open` is asked once a [`Owner::Process`] lock has been taken:
/// "does the caller still hold the descriptor it named?". If it answers no,
/// the descriptor was closed while the lock was being taken. The close's
/// release has already run, so the new lock would outlive it, and it is
/// dropped again with the answer `InvalidHandle`. This is Linux's recovery
/// from the same close/fcntl race (`fcntl_setlk`). An OFD lock is checked
/// against its description instead, which is what ends it.
///
/// # Errors
///
/// As `KernelError`s, with the errno each ABI makes of them:
///
/// - `InvalidArgument` (`EINVAL`):
///   - an `l_type` other than `F_RDLCK`, `F_WRLCK` or `F_UNLCK`. A process's
///     `F_GETLK` must name a lock, as on Linux; an OFD's `F_GETLK` with
///     `F_UNLCK` asks which of its own locks cover the range;
///   - an `l_whence` other than `SEEK_SET`, `SEEK_CUR` or `SEEK_END`;
///   - a range [`flock_range`] refuses;
///   - an OFD request whose `l_pid` is not 0.
/// - `InvalidHandle` (`EBADF`):
///   - a lock the description's mode does not allow: `F_RDLCK` needs it open
///     for reading, `F_WRLCK` for writing (POSIX; Linux
///     `check_fmode_for_setlk`);
///   - a handle closed meanwhile.
/// - `WouldBlock` (`EAGAIN`): [`Op::Set`], with another owner's lock in the way.
/// - `Deadlock` (`EDEADLK`), `Interrupted` (`EINTR`, or a restart):
///   [`Op::SetWait`]; see [`reclock::set_wait`].
/// - `ResourceExhausted` (`ENOLCK`): the lock table is full.
pub fn apply(
    target: Target,
    who: Owner,
    op: Op,
    flock: &mut Flock,
    still_open: impl Fn() -> bool,
) -> KernelResult<()> {
    let ofd = who == Owner::Description;
    let lock_type = match flock.l_type {
        F_RDLCK => Some(RecordLockType::Read),
        F_WRLCK => Some(RecordLockType::Write),
        F_UNLCK if op != Op::Get || ofd => None,
        _ => return Err(KernelError::InvalidArgument),
    };
    let whence = flock.l_whence;
    // Only the base the request names is looked up: a `SEEK_SET` lock on a
    // directory handle must not fail because a directory has no offset.
    let (cur, size) = match whence {
        SEEK_CUR => (target.offset()?, 0),
        SEEK_END => (0, target.size()?),
        _ => (0, 0),
    };
    let (start, len) = flock_range(whence, flock.l_start, flock.l_len, cur, size)?;
    if op != Op::Get {
        let (readable, writable) = target.access()?;
        let allowed = match lock_type {
            Some(RecordLockType::Read) => readable,
            Some(RecordLockType::Write) => writable,
            None => true,
        };
        if !allowed {
            return Err(KernelError::InvalidHandle);
        }
    }
    if ofd && flock.l_pid != 0 {
        return Err(KernelError::InvalidArgument);
    }

    let key = target.key()?;
    let owner = match who {
        Owner::Process(pid) => reclock::posix_owner(pid),
        Owner::Description => target.description_owner(),
    };

    let Some(lock_type) = lock_type else {
        if op == Op::Get {
            // An OFD asking which of its own locks cover the range; with none,
            // `l_type` stays `F_UNLCK` and nothing else changes.
            if let Some(own) = reclock::query_own(&key, owner, start, len) {
                report(flock, &own);
            }
        } else {
            reclock::unlock(&key, owner, start, len);
        }
        return Ok(());
    };

    match op {
        Op::Get => {
            match reclock::query(&key, owner, start, len, lock_type) {
                Some(holder) => report(flock, &holder),
                // "returns F_UNLCK in the l_type field of lock and leaves the
                // other fields of the structure unchanged" (fcntl(2)). Until
                // 2026-10-01 `l_pid` was zeroed as well.
                None => flock.l_type = F_UNLCK,
            }
            Ok(())
        }
        Op::Set => {
            reclock::set(&key, owner, start, len, lock_type)?;
            keep_if_open(&key, owner, target, who, still_open)
        }
        Op::SetWait => {
            reclock::set_wait(&key, owner, start, len, lock_type)?;
            keep_if_open(&key, owner, target, who, still_open)
        }
    }
}

/// The close/fcntl race check [`apply`] makes after taking a lock.
fn keep_if_open(
    key: &LockKey,
    owner: u64,
    target: Target,
    who: Owner,
    still_open: impl Fn() -> bool,
) -> KernelResult<()> {
    let open = match who {
        Owner::Process(_) => still_open(),
        Owner::Description => target.exists(),
    };
    if open {
        Ok(())
    } else {
        reclock::release_on(key, owner);
        Err(KernelError::InvalidHandle)
    }
}

/// Describe a holder in `flock`, as Linux's `posix_lock_to_flock` does:
/// its type, its range from the start of the file (`SEEK_SET`), and its pid,
/// or -1 for an OFD lock, which has none.
fn report(flock: &mut Flock, holder: &RecordLock) {
    flock.l_type = match holder.lock_type {
        RecordLockType::Read => F_RDLCK,
        RecordLockType::Write => F_WRLCK,
    };
    flock.l_whence = 0;
    // Every stored range came through `flock_range`, so it fits.
    flock.l_start = i64::try_from(holder.start).unwrap_or(i64::MAX);
    flock.l_len = i64::try_from(holder.len).unwrap_or(0);
    flock.l_pid = if reclock::owner_is_ofd(holder.owner) {
        -1
    } else {
        i32::try_from(holder.owner).unwrap_or(-1)
    };
}

/// Drop `pid`'s record locks on the file behind a descriptor it is closing.
///
/// POSIX: closing ANY descriptor for a file releases all of the process's
/// locks on it, whichever descriptor took them, including when other
/// descriptors for the file stay open. Every path that removes a descriptor
/// from a process calls this: close, a `dup2` that displaces one,
/// `close_range`, and close-on-exec. It must be called while the handle is
/// still open, since the key is the handle's file.
pub fn release_on_close(pid: u64, target: Target) {
    let owner = reclock::posix_owner(pid);
    // Nearly every close is by a process with no record lock, and that is
    // answered without the filesystem lookup that resolving the key costs.
    if !reclock::holds_any(owner) {
        return;
    }
    // A handle that is already gone has no file left to name. Its locks
    // went with its final close (OFD) or go when the process does (POSIX).
    if let Ok(key) = target.key() {
        reclock::release_on(&key, owner);
    }
}

/// Exercise [`flock_range`] over the cases POSIX defines and the ones that
/// would quietly lock the wrong bytes.
///
/// # Errors
///
/// `InternalError` when a case resolves wrongly.
pub fn self_test_flock_range() -> KernelResult<()> {
    crate::serial_println!("[flock-range] Running range-resolution self-test...");

    // One case: whence, l_start, l_len, the descriptor's offset, the file
    // size, and the absolute range it must resolve to. Named because
    // `clippy::type_complexity` is deny-level here, and because the field
    // order IS the meaning of the table -- a reader should not have to count
    // commas to find which `i64` is the length.
    type Case = (i16, i64, i64, u64, u64, (u64, u64));
    let ok_cases: [Case; 7] = [
        // SEEK_SET, plain forward range.
        (0, 100, 50, 999, 999, (100, 50)),
        // len 0 is to-EOF, preserved as 0 for RecordLock::end to read.
        (0, 100, 0, 999, 999, (100, 0)),
        // SEEK_CUR uses the descriptor's offset as the base.
        (1, 10, 5, 200, 999, (210, 5)),
        // SEEK_END uses the file size.
        (2, 0, 10, 0, 500, (500, 10)),
        // SEEK_END with a negative start: the last 10 bytes.
        (2, -10, 10, 0, 500, (490, 10)),
        // A NEGATIVE length: the range BELOW the anchor.
        (0, 100, -40, 0, 999, (60, 40)),
        // Negative length against a SEEK_CUR base.
        (1, 100, -100, 50, 999, (50, 100)),
    ];
    for (w, st, ln, cur, size, want) in ok_cases {
        match flock_range(w, st, ln, cur, size) {
            Ok(got) if got == want => {}
            other => {
                crate::serial_println!(
                    "[flock-range]   FAIL: (whence {}, start {}, len {}) gave {:?}, wanted Ok({:?})",
                    w,
                    st,
                    ln,
                    other,
                    want
                );
                return Err(KernelError::InternalError);
            }
        }
    }

    // (whence, l_start, l_len, cur, size, what)
    let err_cases: [(i16, i64, i64, u64, u64, &str); 5] = [
        (7, 0, 1, 0, 0, "unknown whence"),
        (0, -1, 1, 0, 0, "range starts before byte zero"),
        (0, 10, -100, 0, 0, "negative length reaches below zero"),
        (0, i64::MAX, 1, 0, 0, "anchor + len overflows i64"),
        (1, i64::MAX, 1, 8, 0, "SEEK_CUR base + start overflows i64"),
    ];
    for (w, st, ln, cur, size, what) in err_cases {
        if let Ok(got) = flock_range(w, st, ln, cur, size) {
            crate::serial_println!("[flock-range]   FAIL: {} was accepted as {:?}", what, got);
            return Err(KernelError::InternalError);
        }
    }

    crate::serial_println!("[flock-range] Self-test passed (12 cases).");
    Ok(())
}

/// [`apply`] against real handles: the access-mode rule, `F_GETLK`'s answer
/// in full, `SEEK_CUR` and `SEEK_END`, an OFD asking about its own locks,
/// and the close/fcntl race.
///
/// Residue-free: it releases both synthetic owners, closes its handles and
/// removes its file on every path out.
///
/// # Errors
///
/// `IoError` when a case answers wrongly; the setup's own errors otherwise.
pub fn self_test() -> KernelResult<()> {
    use crate::fs::handle::{self, OpenFlags};

    const PATH: &str = "/tmp/record-lock-selftest";
    crate::serial_println!("[record-lock] Running the shared-core self-test...");
    let _ = crate::fs::Vfs::remove(PATH);
    crate::fs::Vfs::write_file(PATH, &[0u8; 64])?;
    let rw = handle::open(PATH, OpenFlags::READ.union(OpenFlags::WRITE))?;
    let ro = handle::open(PATH, OpenFlags::READ);
    let wo = handle::open(PATH, OpenFlags::WRITE);
    let result = match (ro, wo) {
        (Ok(ro), Ok(wo)) => {
            let r = core_cases(rw, ro, wo);
            let _ = handle::close(ro);
            let _ = handle::close(wo);
            r
        }
        (ro, wo) => {
            if let Ok(h) = ro {
                let _ = handle::close(h);
            }
            if let Ok(h) = wo {
                let _ = handle::close(h);
            }
            Err(KernelError::IoError)
        }
    };
    reclock::release_all(reclock::posix_owner(TEST_A));
    reclock::release_all(reclock::posix_owner(TEST_B));
    let _ = handle::close(rw);
    let _ = crate::fs::Vfs::remove(PATH);
    result?;
    crate::serial_println!("[record-lock] Self-test passed (8 cases).");
    Ok(())
}

/// The synthetic processes [`self_test`] locks as.
const TEST_A: u64 = 9101;
const TEST_B: u64 = 9102;

/// [`self_test`]'s cases, on a file of 64 bytes open read-write (`rw`),
/// read-only (`ro`) and write-only (`wo`).
fn core_cases(rw: u64, ro: u64, wo: u64) -> KernelResult<()> {
    fn fail(what: &str, got: &dyn core::fmt::Debug) -> KernelResult<()> {
        crate::serial_println!("[record-lock]   FAIL: {}: got {:?}", what, got);
        Err(KernelError::IoError)
    }
    let open = || true;
    let (a, b) = (Owner::Process(TEST_A), Owner::Process(TEST_B));

    // 1. The access mode. A write lock through a read-only description and a
    //    read lock through a write-only one are EBADF, as POSIX requires; a
    //    release needs neither.
    let r = apply(
        Target::File(ro),
        a,
        Op::Set,
        &mut Flock::new(F_WRLCK, 0, 0, 1),
        open,
    );
    if r != Err(KernelError::InvalidHandle) {
        return fail("a write lock through a read-only handle", &r);
    }
    let r = apply(
        Target::File(wo),
        a,
        Op::Set,
        &mut Flock::new(F_RDLCK, 0, 0, 1),
        open,
    );
    if r != Err(KernelError::InvalidHandle) {
        return fail("a read lock through a write-only handle", &r);
    }
    let r = apply(
        Target::File(ro),
        a,
        Op::Set,
        &mut Flock::new(F_UNLCK, 0, 0, 0),
        open,
    );
    if r.is_err() {
        return fail("a release through a read-only handle", &r);
    }
    crate::serial_println!("[record-lock]   [1/8] the access mode decides: OK");

    // 2. F_GETLK reports the holder in full: type, range from byte 0
    //    (l_whence rewritten to SEEK_SET), and pid. A's lock is taken through
    //    one handle and found through another, since a lock is on the file.
    apply(
        Target::File(rw),
        a,
        Op::Set,
        &mut Flock::new(F_WRLCK, 0, 10, 20),
        open,
    )?;
    let mut probe = Flock::new(F_RDLCK, SEEK_END, -40, 1);
    apply(Target::File(ro), b, Op::Get, &mut probe, open)?;
    let want = Flock {
        l_pid: 9101,
        ..Flock::new(F_WRLCK, 0, 10, 20)
    };
    if probe != want {
        return fail("F_GETLK's report of A's lock", &probe);
    }
    crate::serial_println!("[record-lock]   [2/8] F_GETLK reports type, range and pid: OK");

    // 3. With nothing in the way, F_GETLK sets l_type to F_UNLCK and leaves
    //    every other field as it came, l_pid included.
    let mut free = Flock {
        l_pid: 1234,
        ..Flock::new(F_WRLCK, SEEK_SET, 40, 5)
    };
    apply(Target::File(rw), b, Op::Get, &mut free, open)?;
    let want = Flock {
        l_pid: 1234,
        ..Flock::new(F_UNLCK, 0, 40, 5)
    };
    if free != want {
        return fail("F_GETLK with nothing in the way", &free);
    }
    crate::serial_println!("[record-lock]   [3/8] F_GETLK leaves a free range's fields alone: OK");

    // 4. A process's F_GETLK must name a lock: F_UNLCK is EINVAL on Linux.
    let r = apply(
        Target::File(rw),
        b,
        Op::Get,
        &mut Flock::new(F_UNLCK, 0, 0, 1),
        open,
    );
    if r != Err(KernelError::InvalidArgument) {
        return fail("a process's F_GETLK of F_UNLCK", &r);
    }
    crate::serial_println!("[record-lock]   [4/8] F_GETLK of F_UNLCK is EINVAL: OK");

    // 5. B is refused while A holds the range, then SEEK_CUR: with the
    //    read-write handle's offset at 50, start 2 length 3 is [52, 55).
    let r = apply(
        Target::File(rw),
        b,
        Op::Set,
        &mut Flock::new(F_WRLCK, 0, 15, 1),
        open,
    );
    if r != Err(KernelError::WouldBlock) {
        return fail("B's write inside A's range", &r);
    }
    crate::fs::handle::seek(rw, crate::fs::handle::SeekFrom::Start(50))?;
    apply(
        Target::File(rw),
        b,
        Op::Set,
        &mut Flock::new(F_WRLCK, SEEK_CUR, 2, 3),
        open,
    )?;
    let mut probe = Flock::new(F_WRLCK, 0, 0, 0);
    apply(Target::File(rw), a, Op::Get, &mut probe, open)?;
    // A asks about the whole file; the first lock in its way is B's.
    if probe.l_start != 52 || probe.l_len != 3 || probe.l_pid != 9102 {
        return fail("B's SEEK_CUR lock", &probe);
    }
    crate::serial_println!(
        "[record-lock]   [5/8] a conflict is WouldBlock; SEEK_CUR is the offset: OK"
    );

    // 6. The close/fcntl race: a lock taken while the descriptor was being
    //    closed is dropped again, and the answer is InvalidHandle.
    let r = apply(
        Target::File(rw),
        a,
        Op::Set,
        &mut Flock::new(F_RDLCK, 0, 60, 2),
        || false,
    );
    let mut probe = Flock::new(F_WRLCK, 0, 60, 2);
    apply(Target::File(rw), b, Op::Get, &mut probe, open)?;
    // All of A's locks on the file went with it, as the close's would have.
    let mut whole = Flock::new(F_WRLCK, 0, 0, 0);
    apply(Target::File(rw), b, Op::Get, &mut whole, open)?;
    if r != Err(KernelError::InvalidHandle) || probe.l_type != F_UNLCK || whole.l_type != F_UNLCK {
        crate::serial_println!(
            "[record-lock]   FAIL: close race: {:?}, then {:?} and {:?}",
            r,
            probe,
            whole
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!("[record-lock]   [6/8] a lock taken across a close is dropped: OK");

    // 7. An OFD lock reports l_pid -1, and an OFD asking with F_UNLCK finds
    //    its own lock -- not B's, which is in the same range.
    apply(
        Target::File(wo),
        Owner::Description,
        Op::Set,
        &mut Flock::new(F_WRLCK, 0, 0, 2),
        open,
    )?;
    let mut probe = Flock::new(F_RDLCK, 0, 0, 1);
    apply(Target::File(ro), b, Op::Get, &mut probe, open)?;
    let mut own = Flock::new(F_UNLCK, 0, 0, 0);
    apply(
        Target::File(wo),
        Owner::Description,
        Op::Get,
        &mut own,
        open,
    )?;
    reclock::release_ofd(wo);
    if probe.l_pid != -1 || own.l_type != F_WRLCK || own.l_len != 2 || own.l_pid != -1 {
        crate::serial_println!(
            "[record-lock]   FAIL: OFD: B saw {:?}, the OFD's own query {:?}",
            probe,
            own
        );
        return Err(KernelError::IoError);
    }
    crate::serial_println!(
        "[record-lock]   [7/8] OFD locks report pid -1 and can ask about their own: OK"
    );

    // 8. A memfd is locked by its id and sizes SEEK_END by its own data.
    let m = crate::ipc::memfd::create_with_flags(alloc::vec::Vec::from(&b"reclock"[..]), false);
    let r = crate::ipc::memfd::write(m, &[0u8; 8]).and_then(|_| {
        apply(
            Target::MemFd(m.raw()),
            a,
            Op::Set,
            &mut Flock::new(F_WRLCK, SEEK_END, -2, 2),
            open,
        )?;
        let mut probe = Flock::new(F_WRLCK, 0, 0, 0);
        apply(Target::MemFd(m.raw()), b, Op::Get, &mut probe, open)?;
        Ok(probe)
    });
    reclock::release_all(reclock::posix_owner(TEST_A));
    crate::ipc::memfd::close(m);
    match r {
        Ok(p) if p.l_start == 6 && p.l_len == 2 && p.l_pid == 9101 => {}
        other => return fail("a memfd's SEEK_END lock", &other),
    }
    crate::serial_println!("[record-lock]   [8/8] a memfd is lockable, SEEK_END by its size: OK");
    Ok(())
}
