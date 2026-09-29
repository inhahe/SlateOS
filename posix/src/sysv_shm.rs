// Sizes, page counts and indices here are bounded -- a segment by `SHMMAX`,
// the total by `SHMALL`, an index by `SHMMNI` -- and every sum is checked
// against its bound before it is made, or undoes one an earlier sum made.
// Clippy cannot see across the checks.
#![allow(clippy::arithmetic_side_effects)]
//! System V shared memory (`<sys/shm.h>`): Linux 6.6's semantics (ipc/shm.c)
//! -- inside one process.
//!
//! Segments are kept in this process: a key names the same segment for every
//! thread of the program, but another program using the key gets a segment of
//! its own. Sharing them between programs is open question D-Q3
//! (`open-questions.md`), as for the message queues and semaphores; the rest
//! is Linux's:
//!
//! - **Memory** is the kernel's: a segment is a shared-memory region
//!   (`SYS_SHM_CREATE`), and every `shmat` maps it afresh (`SYS_SHM_MAP`), so
//!   two attaches are two addresses of the same bytes, as on Linux. A segment
//!   is any size from [`SHMMIN`] to [`SHMMAX`], [`SHMALL`] pages in all, and
//!   [`SHMMNI`] segments at most -- Linux's defaults; its memory is committed
//!   when it is made, as all memory here is.
//! - **Permissions** are Linux's `ipcperms` ([`crate::sysv_ipc`]): `shmget` on
//!   an existing key asks for its flags' bits, `shmat` for read, or read and
//!   write, and `IPC_STAT` for read; `IPC_SET` and `IPC_RMID` are the owner's
//!   or the creator's, or need `CAP_SYS_ADMIN`.
//! - **Removal** is Linux's: `IPC_RMID` on an attached segment marks it
//!   (`SHM_DEST`) and makes its key private, and the memory goes when the last
//!   attach is detached; until then its id still attaches.
//! - **Errors come in the kernel's order.** `shmget` on an existing key judges
//!   the size before the permission; `IPC_SET` reads its buffer before it
//!   looks the segment up, and `IPC_STAT` writes it after.
//! - **`IPC_INFO`, `SHM_INFO`, `SHM_STAT` and `SHM_STAT_ANY`** answer as
//!   Linux's do, which is what `ipcs -m` needs.
//!
//! ## Not Linux's
//!
//! - `shmat` at an address the caller chooses (`shmaddr` non-NULL) is refused
//!   with `EINVAL`: the kernel's `SYS_SHM_MAP` picks the address itself.
//! - `SHM_EXEC` is refused with `EACCES`: the kernel never maps shared memory
//!   executable.
//! - `SHM_HUGETLB` is `ENOMEM`, as on a Linux system with no huge pages set
//!   aside -- Ubuntu's default.
//! - `SHM_LOCK` records the lock and changes nothing else: this memory is
//!   never paged out.
//!
//! ## What changed on 2026-09-26
//!
//! The segments were a static pool: four of at most 64 KiB, every `shmat` of
//! a segment returned the same address, permissions were stored and never
//! checked, `IPC_SET` looked the segment up before reading its buffer, and
//! `IPC_INFO`, `SHM_INFO` and `SHM_STAT` were `EINVAL`
//! (`known-issues.md` -> `B-D-SYSV-SHM-WAS-A-STATIC-POOL`).

use crate::errno;
use crate::linux_ipc::{IPC_INFO, IpcPerm};
use crate::objtable::Slots;
use crate::perprocess::process_global;
use crate::sysv_ipc::{
    Caller, Perm, S_IRUGO, S_IWUGO, SEQ_MASK, caller, decode_id, encode_id, may_control, now_secs,
    permits,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key.
pub const IPC_PRIVATE: i32 = 0;

/// Attach read-only.
pub const SHM_RDONLY: i32 = 0o10000;
/// Round attach address down to `SHMLBA`.
pub const SHM_RND: i32 = 0o20000;
/// Take-over region on attach (remove on last detach).
pub const SHM_REMAP: i32 = 0o40000;
/// Executable mapping.
pub const SHM_EXEC: i32 = 0o100000;

/// `shmget`: back the segment with huge pages.
pub const SHM_HUGETLB: i32 = 0o4000;
/// `shmget`: do not reserve swap for it.  `0o10000`, as musl's `<sys/shm.h>`
/// and the kernel's `ipc.h` have it -- the same bits as [`SHM_RDONLY`], which
/// is `shmat`'s; this was `0o10000000` until 2026-09-27.  `shmget` ignores
/// the flag (nothing is reserved here), as it ignores every bit it does not
/// know.
pub const SHM_NORESERVE: i32 = 0o10000;

/// Lock pages in memory.
pub const SHM_LOCK: i32 = 11;
/// Unlock pages.
pub const SHM_UNLOCK: i32 = 12;
/// `shmctl`: the segment at an index of the table, not an id -- what `ipcs`
/// walks. Linux-specific.
pub const SHM_STAT: i32 = 13;
/// `shmctl`: the totals in use. Linux-specific.
pub const SHM_INFO: i32 = 14;
/// `shmctl`: `SHM_STAT` without its read-permission check (Linux 4.17).
pub const SHM_STAT_ANY: i32 = 15;

/// In `shm_perm.mode`: removed while attached, freed at the last detach.
pub const SHM_DEST: u32 = 0o1000;
/// In `shm_perm.mode`: `SHM_LOCK`ed.
pub const SHM_LOCKED: u32 = 0o2000;

/// Segment low boundary address multiple (page size).
///
/// An alias of [`crate::unistd::PAGE_SIZE`]; `shmat` addresses must be a
/// multiple of this, and the number is written down once, in `unistd`.
pub const SHMLBA: usize = crate::unistd::PAGE_SIZE;

/// The smallest segment, in bytes.
pub const SHMMIN: usize = 1;
/// The largest segment, in bytes (Linux's `kernel.shmmax`:
/// `ULONG_MAX - (1UL << 24)`).
pub const SHMMAX: usize = usize::MAX - (1 << 24);
/// The most pages of segments in all (`kernel.shmall`, the same figure).
pub const SHMALL: usize = usize::MAX - (1 << 24);
/// The most segments (`kernel.shmmni`).
pub const SHMMNI: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct shmid_ds` — shared memory segment data structure.
#[repr(C)]
pub struct ShmidDs {
    /// Permissions, as a nested `struct ipc_perm` — see [`MsqidDs::msg_perm`]
    /// for why these are no longer flattened.
    ///
    /// [`MsqidDs::msg_perm`]: crate::sysv_msg::MsqidDs::msg_perm
    pub shm_perm: crate::linux_ipc::IpcPerm,
    /// Segment size in bytes.
    pub shm_segsz: usize,
    /// Time of the last attach.
    pub shm_atime: i64,
    /// Time of the last detach.
    pub shm_dtime: i64,
    /// Time of the last change.
    pub shm_ctime: i64,
    /// PID of the creator.
    pub shm_cpid: i32,
    /// PID of the last attach or detach.
    pub shm_lpid: i32,
    /// Number of current attaches.
    pub shm_nattch: usize,
    /// musl's trailing `unsigned long __unused[2]`.
    __unused: [u64; 2],
}

/// `struct shminfo` — what `shmctl(IPC_INFO)` writes: the limits.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shminfo {
    /// [`SHMMAX`].
    pub shmmax: usize,
    /// [`SHMMIN`].
    pub shmmin: usize,
    /// [`SHMMNI`].
    pub shmmni: usize,
    /// Segments a process may attach: [`SHMMNI`], as Linux reports it.
    pub shmseg: usize,
    /// [`SHMALL`].
    pub shmall: usize,
    /// `__unused[4]`.
    pub __unused: [usize; 4],
}

/// `struct shm_info` — what `shmctl(SHM_INFO)` writes: what is in use.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShmInfo {
    /// Segments that exist.
    pub used_ids: i32,
    /// Pages of segments that exist.
    pub shm_tot: usize,
    /// Of them, resident: all of them, memory here being committed.
    pub shm_rss: usize,
    /// Of them, swapped out: none.
    pub shm_swp: usize,
    /// Unused since Linux 2.4.
    pub swap_attempts: usize,
    /// Unused since Linux 2.4.
    pub swap_successes: usize,
}

// ---------------------------------------------------------------------------
// The memory behind a segment
// ---------------------------------------------------------------------------

/// Where a segment's memory lives: the kernel's shared-memory regions on
/// SlateOS. On the host, which has no such calls, a zeroed `malloc` block
/// that every attach gets itself -- enough to test the bookkeeping.
mod backing {
    /// A region of `size` bytes, rounded up to pages: its handle.
    #[cfg(target_os = "none")]
    pub(super) fn create(size: usize) -> Result<u64, i32> {
        let r = crate::syscall::syscall1(crate::syscall::SYS_SHM_CREATE, size as u64);
        if r < 0 {
            return Err(crate::errno::errno_for(r));
        }
        Ok(r as u64)
    }

    /// A new mapping of `handle`'s region: its address.
    #[cfg(target_os = "none")]
    pub(super) fn map(handle: u64, write: bool) -> Result<usize, i32> {
        let flags = crate::syscall::SHM_MAP_READ
            | if write {
                crate::syscall::SHM_MAP_WRITE
            } else {
                0
            };
        let r = crate::syscall::syscall2(crate::syscall::SYS_SHM_MAP, handle, flags);
        if r <= 0 {
            return Err(if r == 0 {
                crate::errno::ENOMEM
            } else {
                crate::errno::errno_for(r)
            });
        }
        Ok(r as usize)
    }

    /// Unmap the `size` bytes at `addr`.
    #[cfg(target_os = "none")]
    pub(super) fn unmap(addr: usize, size: usize) {
        // The mapping is this module's own record of one it made, so the
        // unmap cannot be refused for a reason the caller could act on.
        let _ = crate::syscall::syscall2(crate::syscall::SYS_SHM_UNMAP, addr as u64, size as u64);
    }

    /// Give the region back; mappings of it keep its memory until unmapped.
    #[cfg(target_os = "none")]
    pub(super) fn close(handle: u64) {
        // The handle is this module's own, closed once.
        let _ = crate::syscall::syscall1(crate::syscall::SYS_SHM_CLOSE, handle);
    }

    #[cfg(not(target_os = "none"))]
    pub(super) fn create(size: usize) -> Result<u64, i32> {
        // Page-aligned, as the kernel's mappings are: `shmdt` refuses any
        // other address.
        let p = crate::malloc::aligned_alloc(super::SHMLBA, size);
        if p.is_null() {
            return Err(crate::errno::ENOMEM);
        }
        // SAFETY: a fresh block of `size` bytes.
        unsafe { core::ptr::write_bytes(p, 0, size) };
        // Exposed: every attach turns the address back into a pointer.
        Ok(p.expose_provenance() as u64)
    }

    #[cfg(not(target_os = "none"))]
    pub(super) fn map(handle: u64, _write: bool) -> Result<usize, i32> {
        Ok(handle as usize)
    }

    #[cfg(not(target_os = "none"))]
    pub(super) fn unmap(_addr: usize, _size: usize) {}

    #[cfg(not(target_os = "none"))]
    pub(super) fn close(handle: u64) {
        // SAFETY: the block `create` returned, freed once.
        unsafe { crate::malloc::free(core::ptr::with_exposed_provenance_mut(handle as usize)) };
    }
}

// ---------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------

struct Seg {
    live: bool,
    /// Key, the slot's reuse count, owner, creator and mode.
    perm: Perm,
    /// `SHM_DEST`: removed while attached; its key is private.
    dest: bool,
    /// `SHM_LOCKED`.
    locked: bool,
    /// `shm_segsz`: the size asked for.
    size: usize,
    /// The kernel region.
    handle: u64,
    nattch: usize,
    atime: i64,
    dtime: i64,
    ctime: i64,
    cpid: i32,
    lpid: i32,
}

impl Seg {
    const EMPTY: Self = Self {
        live: false,
        perm: Perm::EMPTY,
        dest: false,
        locked: false,
        size: 0,
        handle: 0,
        nattch: 0,
        atime: 0,
        dtime: 0,
        ctime: 0,
        cpid: 0,
        lpid: 0,
    };

    /// `shm_perm.mode` as `IPC_STAT` reports it: the permission bits with
    /// `SHM_DEST` and `SHM_LOCKED`.
    fn mode(&self) -> u32 {
        self.perm.mode
            | if self.dest { SHM_DEST } else { 0 }
            | if self.locked { SHM_LOCKED } else { 0 }
    }
}

/// One `shmat`: where, how much, and of which segment.
#[derive(Clone, Copy)]
struct Attach {
    live: bool,
    addr: usize,
    len: usize,
    slot: usize,
}

impl Attach {
    const EMPTY: Self = Self {
        live: false,
        addr: 0,
        len: 0,
        slot: 0,
    };
}

/// The segments, this process's attaches, and the pages in use.
struct Tables {
    segs: Slots<Seg>,
    attaches: Slots<Attach>,
    pages: usize,
}

process_global! {
    /// This process's segments.
    ///
    /// Per-thread on the host, for test isolation: a test that counts
    /// segments is broken by any concurrent one, and no lock fixes that.
    fn tables() -> Tables = Tables { segs: Slots::EMPTY, attaches: Slots::EMPTY, pages: 0 };

    /// Serialises every use of the tables, with the same scope as they have
    /// (see [`crate::perprocess::PoolLock`]).
    fn shm_lock() -> crate::perprocess::PoolLock = crate::perprocess::PoolLock::new();
}

/// Holds the tables' lock; the tables are reached through it.
struct Locked {
    _guard: crate::perprocess::PoolGuard<'static>,
    t: *mut Tables,
}

fn lock() -> Locked {
    Locked {
        // SAFETY: `shm_lock()` is this context's lock, valid as long as the
        // tables it guards.
        _guard: unsafe { crate::perprocess::lock_pool(shm_lock()) },
        t: tables(),
    }
}

/// Pages a segment of `size` bytes takes.
fn pages_of(size: usize) -> Option<usize> {
    size.checked_add(SHMLBA - 1).map(|n| n / SHMLBA)
}

impl Locked {
    fn tables(&mut self) -> &mut Tables {
        // SAFETY: the lock is held, and `t` is this context's tables.
        unsafe { &mut *self.t }
    }

    fn seg(&mut self, slot: usize) -> Option<&mut Seg> {
        self.tables().segs.get(slot)
    }

    /// The id naming the segment in `slot` now.
    fn id_of(&mut self, slot: usize) -> i32 {
        let seq = self.seg(slot).map_or(0, |s| s.perm.seq);
        encode_id(slot, seq)
    }

    /// The slot of the live segment `shmid` names -- removed while attached
    /// or not, as Linux's `shm_obtain_object_check` finds it.
    fn resolve(&mut self, shmid: i32) -> Option<usize> {
        let (slot, seq) = decode_id(shmid)?;
        self.seg(slot)
            .is_some_and(|s| s.live && s.perm.seq & SEQ_MASK == seq)
            .then_some(slot)
    }

    /// The slot at table index `index`, if a segment is there (`SHM_STAT`).
    fn at_index(&mut self, index: i32) -> Option<usize> {
        let slot = usize::try_from(index).ok()?;
        self.seg(slot).is_some_and(|s| s.live).then_some(slot)
    }

    /// A segment `shmget` can find by `key`: one removed while attached has
    /// a private key, and is not found.
    fn find_key(&mut self, key: i32) -> Option<usize> {
        let cap = self.tables().segs.cap();
        (0..cap).find(|&i| {
            self.seg(i)
                .is_some_and(|s| s.live && !s.dest && s.perm.key == key)
        })
    }

    /// Linux's `newseg`: a segment keyed `key` of `size` bytes, with
    /// `shmflg`'s permission bits, owned and created by `who`.
    fn new_segment(
        &mut self,
        key: i32,
        size: usize,
        shmflg: i32,
        who: Caller,
    ) -> Result<usize, i32> {
        if !(SHMMIN..=SHMMAX).contains(&size) {
            return Err(errno::EINVAL);
        }
        let pages = pages_of(size).ok_or(errno::ENOSPC)?;
        let total = self.tables().pages.checked_add(pages);
        if total.is_none_or(|t| t > SHMALL) {
            return Err(errno::ENOSPC);
        }
        if shmflg & SHM_HUGETLB != 0 {
            // No huge pages are set aside, as on a default Linux system.
            return Err(errno::ENOMEM);
        }
        if self.tables().segs.live() >= SHMMNI {
            return Err(errno::ENOSPC);
        }
        let handle = backing::create(pages * SHMLBA)?;
        let Some(slot) = self.tables().segs.claim(SHMMNI, || Seg::EMPTY) else {
            backing::close(handle);
            return Err(errno::ENOMEM);
        };
        let pid = crate::process::getpid();
        let seg = self.seg(slot).ok_or(errno::ENOMEM)?;
        let seq = seg.perm.seq;
        *seg = Seg {
            live: true,
            perm: Perm::new(key, seq, shmflg, who),
            size,
            handle,
            ctime: now_secs(),
            cpid: pid,
            ..Seg::EMPTY
        };
        self.tables().pages += pages;
        Ok(slot)
    }

    /// Free the segment in `slot`; its id stops resolving.
    fn destroy(&mut self, slot: usize) {
        let Some(seg) = self.seg(slot) else { return };
        let (handle, size) = (seg.handle, seg.size);
        let seq = seg.perm.seq.wrapping_add(1);
        *seg = Seg {
            perm: Perm { seq, ..Perm::EMPTY },
            ..Seg::EMPTY
        };
        backing::close(handle);
        let t = self.tables();
        t.segs.release(slot);
        t.pages -= pages_of(size).unwrap_or(0);
    }

    /// The highest index in use, or 0 if none (`ipc_get_maxidx`).
    fn max_index(&mut self) -> i32 {
        let cap = self.tables().segs.cap();
        (0..cap)
            .rev()
            .find(|&i| self.seg(i).is_some_and(|s| s.live))
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// ftok
// ---------------------------------------------------------------------------

/// The System V IPC key for file `path` and project `id` -- shared by
/// message queues, semaphores and shared memory -- as glibc and musl make it:
/// the low 16 bits of the file's inode number, the low 8 of its device
/// number above them, and the low 8 of `id` on top. -1, with `stat`'s
/// `errno`, if the file cannot be examined.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ftok(path: *const u8, id: i32) -> i32 {
    // SAFETY: an all-zero `struct stat` is a valid value to be overwritten.
    let mut st: crate::stat::Stat = unsafe { core::mem::zeroed() };
    if crate::file::stat(path, &raw mut st) != 0 {
        return -1;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let key = ((st.st_ino & 0xFFFF) | ((st.st_dev & 0xFF) << 16)) as i32 | ((id & 0xFF) << 24);
    key
}

// ---------------------------------------------------------------------------
// shmget
// ---------------------------------------------------------------------------

/// `shmget` — get a shared memory segment identifier.
///
/// Returns the shmid, or `-1` with `errno`, in Linux's `ipcget` order:
/// `ENOENT` for a key with no segment and no `IPC_CREAT`; `EEXIST` for one
/// with a segment and `IPC_CREAT | IPC_EXCL`; `EINVAL` when that segment is
/// smaller than `size`, then `EACCES` when its mode refuses the bits in
/// `shmflg`. A new segment is `EINVAL` below [`SHMMIN`] or above [`SHMMAX`],
/// `ENOSPC` past [`SHMALL`] pages or [`SHMMNI`] segments, `ENOMEM` when its
/// memory cannot be had.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmget(key: i32, size: usize, shmflg: i32) -> i32 {
    let who = caller();
    let mut t = lock();
    let result = if key == IPC_PRIVATE {
        t.new_segment(key, size, shmflg, who)
    } else if let Some(slot) = t.find_key(key) {
        if shmflg & IPC_CREAT != 0 && shmflg & IPC_EXCL != 0 {
            Err(errno::EEXIST)
        } else if t.seg(slot).is_some_and(|s| s.size < size) {
            // `shm_more_checks`, before the permission.
            Err(errno::EINVAL)
        } else if t
            .seg(slot)
            .is_some_and(|s| permits(&s.perm, who, shmflg as u32))
        {
            Ok(slot)
        } else {
            Err(errno::EACCES)
        }
    } else if shmflg & IPC_CREAT == 0 {
        Err(errno::ENOENT)
    } else {
        t.new_segment(key, size, shmflg, who)
    };
    match result {
        Ok(slot) => t.id_of(slot),
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

// ---------------------------------------------------------------------------
// shmat, shmdt
// ---------------------------------------------------------------------------

/// `shmat` — attach a shared memory segment.
///
/// Every call maps the segment afresh and returns the new address. Errors,
/// in Linux's `do_shmat` order: `EINVAL` for a negative id, a `shmaddr` not
/// on a `SHMLBA` boundary without `SHM_RND`, `SHM_REMAP` with no address;
/// `EINVAL` for an id naming no segment; `EACCES` without the permission the
/// flags ask for; `ENOMEM`. A segment removed while attached still attaches
/// by its id, as on Linux.
///
/// Not Linux's (see the module docs): a non-NULL `shmaddr` that passes those
/// checks is `EINVAL`, and `SHM_EXEC` is `EACCES`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmat(shmid: i32, shmaddr: *const u8, shmflg: i32) -> *mut u8 {
    match attach(shmid, shmaddr.addr(), shmflg) {
        Ok(addr) => core::ptr::with_exposed_provenance_mut(addr),
        Err(e) => {
            errno::set_errno(e);
            // `(void *) -1`.
            core::ptr::without_provenance_mut(usize::MAX)
        }
    }
}

fn attach(shmid: i32, shmaddr: usize, shmflg: i32) -> Result<usize, i32> {
    if shmid < 0 {
        return Err(errno::EINVAL);
    }
    let mut addr = shmaddr;
    if addr != 0 {
        if addr % SHMLBA != 0 {
            if shmflg & SHM_RND == 0 {
                return Err(errno::EINVAL);
            }
            addr -= addr % SHMLBA;
            if addr == 0 && shmflg & SHM_REMAP != 0 {
                return Err(errno::EINVAL);
            }
        }
    } else if shmflg & SHM_REMAP != 0 {
        return Err(errno::EINVAL);
    }
    let write = shmflg & SHM_RDONLY == 0;
    let mut wanted = if write { S_IRUGO | S_IWUGO } else { S_IRUGO };
    if shmflg & SHM_EXEC != 0 {
        wanted |= 0o111;
    }

    let who = caller();
    let mut t = lock();
    let slot = t.resolve(shmid).ok_or(errno::EINVAL)?;
    let seg = t.seg(slot).ok_or(errno::EINVAL)?;
    if !permits(&seg.perm, who, wanted) {
        return Err(errno::EACCES);
    }
    if shmflg & SHM_EXEC != 0 {
        return Err(errno::EACCES);
    }
    if addr != 0 {
        return Err(errno::EINVAL);
    }
    let (handle, len) = (seg.handle, pages_of(seg.size).unwrap_or(0) * SHMLBA);
    let mapped = backing::map(handle, write)?;
    let Some(a) = t.tables().attaches.claim(usize::MAX, || Attach::EMPTY) else {
        backing::unmap(mapped, len);
        return Err(errno::ENOMEM);
    };
    if let Some(rec) = t.tables().attaches.get(a) {
        *rec = Attach {
            live: true,
            addr: mapped,
            len,
            slot,
        };
    }
    let pid = crate::process::getpid();
    if let Some(seg) = t.seg(slot) {
        seg.nattch += 1;
        seg.atime = now_secs();
        seg.lpid = pid;
    }
    Ok(mapped)
}

/// `shmdt` — detach the segment attached at `shmaddr`.
///
/// `EINVAL` for an address that is not where a `shmat` attached one. A
/// segment removed while attached is freed at its last detach.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmdt(shmaddr: *const u8) -> i32 {
    let addr = shmaddr.addr();
    let mut t = lock();
    let cap = t.tables().attaches.cap();
    let found = (addr % SHMLBA == 0)
        .then(|| {
            (0..cap).find(|&i| {
                t.tables()
                    .attaches
                    .get(i)
                    .is_some_and(|a| a.live && a.addr == addr)
            })
        })
        .flatten();
    let Some(a) = found else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let Some(rec) = t
        .tables()
        .attaches
        .get(a)
        .map(|r| core::mem::replace(r, Attach::EMPTY))
    else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    t.tables().attaches.release(a);
    backing::unmap(rec.addr, rec.len);
    let pid = crate::process::getpid();
    let gone = t.seg(rec.slot).is_some_and(|seg| {
        seg.nattch -= 1;
        seg.dtime = now_secs();
        seg.lpid = pid;
        seg.dest && seg.nattch == 0
    });
    if gone {
        t.destroy(rec.slot);
    }
    0
}

// ---------------------------------------------------------------------------
// shmctl
// ---------------------------------------------------------------------------

/// `shmctl` — shared memory control operations.
///
///   * `IPC_STAT` — the segment's state, into `buf`.
///   * `IPC_SET` — its owner, group and permission bits, from `buf`.
///   * `IPC_RMID` — remove it: now, or when its last attach is detached.
///   * `SHM_LOCK`, `SHM_UNLOCK` — lock it in memory (recorded only: this
///     memory is never paged out); the owner's or creator's, or needs
///     `CAP_IPC_LOCK`, and `SHM_LOCK` a non-zero `RLIMIT_MEMLOCK`.
///   * `IPC_INFO` — the limits, as a [`Shminfo`]; `SHM_INFO` — the totals, as
///     a [`ShmInfo`]; both return the highest table index in use.
///   * `SHM_STAT`, `SHM_STAT_ANY` — `IPC_STAT` of the segment at table index
///     `shmid`, returning its id; `SHM_STAT_ANY` needs no permission.
///
/// `EINVAL` first for a negative `shmid` or `cmd`, and for an unknown `cmd`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn shmctl(shmid: i32, cmd: i32, buf: *mut ShmidDs) -> i32 {
    match control(shmid, cmd, buf) {
        Ok(n) => n,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// Linux's `ksys_shmctl`.
fn control(shmid: i32, cmd: i32, buf: *mut ShmidDs) -> Result<i32, i32> {
    if shmid < 0 || cmd < 0 {
        return Err(errno::EINVAL);
    }
    match cmd {
        IPC_INFO => {
            let max_idx = lock().max_index();
            let info = Shminfo {
                shmmax: SHMMAX,
                shmmin: SHMMIN,
                shmmni: SHMMNI,
                shmseg: SHMMNI,
                shmall: SHMALL,
                __unused: [0; 4],
            };
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: for this command `buf` is the caller's `struct
            // shminfo`, whatever its declared type.
            unsafe { buf.cast::<Shminfo>().write_unaligned(info) };
            Ok(max_idx)
        }
        SHM_INFO => {
            let mut t = lock();
            let max_idx = t.max_index();
            let tab = t.tables();
            let info = ShmInfo {
                used_ids: i32::try_from(tab.segs.live()).unwrap_or(i32::MAX),
                shm_tot: tab.pages,
                shm_rss: tab.pages,
                ..ShmInfo::default()
            };
            drop(t);
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: for this command `buf` is the caller's `struct
            // shm_info`, whatever its declared type.
            unsafe { buf.cast::<ShmInfo>().write_unaligned(info) };
            Ok(max_idx)
        }
        IPC_STAT | SHM_STAT | SHM_STAT_ANY => {
            let (ds, ret) = stat(shmid, cmd)?;
            if buf.is_null() {
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's buffer is a `struct shmid_ds`.
            unsafe { buf.write_unaligned(ds) };
            Ok(ret)
        }
        IPC_SET => {
            if buf.is_null() {
                // `copy_shmid_from_user`, before the segment is looked up.
                return Err(errno::EFAULT);
            }
            // SAFETY: the caller's buffer is a `struct shmid_ds`.
            let ds = unsafe { buf.read_unaligned() };
            set(shmid, &ds.shm_perm).map(|()| 0)
        }
        IPC_RMID => rmid(shmid).map(|()| 0),
        SHM_LOCK | SHM_UNLOCK => lock_segment(shmid, cmd == SHM_LOCK).map(|()| 0),
        _ => Err(errno::EINVAL),
    }
}

/// `shmctl_stat`: the state of the segment `shmid` names -- or, for
/// `SHM_STAT` and `SHM_STAT_ANY`, the one at index `shmid` -- and what the
/// call returns.
fn stat(shmid: i32, cmd: i32) -> Result<(ShmidDs, i32), i32> {
    let who = caller();
    let mut t = lock();
    let slot = if cmd == IPC_STAT {
        t.resolve(shmid)
    } else {
        t.at_index(shmid)
    }
    .ok_or(errno::EINVAL)?;
    let seg = t.seg(slot).ok_or(errno::EINVAL)?;
    if cmd != SHM_STAT_ANY && !permits(&seg.perm, who, S_IRUGO) {
        return Err(errno::EACCES);
    }
    let ds = ShmidDs {
        shm_perm: IpcPerm {
            mode: seg.mode(),
            ..seg.perm.to_ipc_perm()
        },
        shm_segsz: seg.size,
        shm_atime: seg.atime,
        shm_dtime: seg.dtime,
        shm_ctime: seg.ctime,
        shm_cpid: seg.cpid,
        shm_lpid: seg.lpid,
        shm_nattch: seg.nattch,
        __unused: [0; 2],
    };
    let ret = if cmd == IPC_STAT { 0 } else { t.id_of(slot) };
    Ok((ds, ret))
}

/// `shmctl_down` for `IPC_SET`: the owner, group and permission bits.
fn set(shmid: i32, perm: &IpcPerm) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(shmid).ok_or(errno::EINVAL)?;
    let seg = t.seg(slot).ok_or(errno::EINVAL)?;
    if !may_control(&seg.perm, who) {
        return Err(errno::EPERM);
    }
    seg.perm.update(perm.uid, perm.gid, perm.mode)?;
    seg.ctime = now_secs();
    Ok(())
}

/// `shmctl_down` for `IPC_RMID`, then `do_shm_rmid`: free the segment now if
/// nothing is attached, else mark it and make its key private.
fn rmid(shmid: i32) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(shmid).ok_or(errno::EINVAL)?;
    let seg = t.seg(slot).ok_or(errno::EINVAL)?;
    if !may_control(&seg.perm, who) {
        return Err(errno::EPERM);
    }
    if seg.nattch == 0 {
        t.destroy(slot);
    } else {
        seg.dest = true;
        seg.perm.key = IPC_PRIVATE;
    }
    Ok(())
}

/// `shmctl_do_lock`.
fn lock_segment(shmid: i32, lock_it: bool) -> Result<(), i32> {
    let who = caller();
    let mut t = lock();
    let slot = t.resolve(shmid).ok_or(errno::EINVAL)?;
    let seg = t.seg(slot).ok_or(errno::EINVAL)?;
    if !crate::sys_capability::has_capability(crate::sys_capability::CAP_IPC_LOCK) {
        if who.euid != seg.perm.uid && who.euid != seg.perm.cuid {
            return Err(errno::EPERM);
        }
        if lock_it && memlock_limit() == 0 {
            return Err(errno::EPERM);
        }
    }
    seg.locked = lock_it;
    Ok(())
}

/// The soft `RLIMIT_MEMLOCK`.
fn memlock_limit() -> u64 {
    let mut r = crate::resource::Rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // A refusal leaves the limit reading 0, which refuses the lock -- the
    // answer Linux gives with no limit to spend.
    let _ = crate::resource::getrlimit(crate::resource::RLIMIT_MEMLOCK, &raw mut r);
    r.rlim_cur
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_wrap
)]
mod tests {
    use super::*;

    const FAIL: *mut u8 = core::ptr::without_provenance_mut(usize::MAX);

    fn err<T: PartialEq + core::fmt::Debug>(got: T, fail: T, want: i32) {
        assert_eq!(got, fail);
        assert_eq!(errno::get_errno(), want);
    }

    fn get(key: i32, size: usize, flags: i32) -> i32 {
        errno::set_errno(0);
        shmget(key, size, flags)
    }

    fn stat_of(id: i32) -> ShmidDs {
        let mut ds: ShmidDs = unsafe { core::mem::zeroed() };
        assert_eq!(shmctl(id, IPC_STAT, &raw mut ds), 0);
        ds
    }

    fn rm(id: i32) {
        assert_eq!(shmctl(id, IPC_RMID, core::ptr::null_mut()), 0);
    }

    // -- shmget --

    #[test]
    fn a_segment_is_any_size_linux_allows() {
        // It was four segments of at most 64 KiB.
        let big = get(IPC_PRIVATE, 1 << 20, 0o600);
        assert!(big > 0, "a mebibyte");
        assert_eq!(stat_of(big).shm_segsz, 1 << 20);
        let ids: Vec<i32> = (0..10).map(|_| get(IPC_PRIVATE, 100, 0o600)).collect();
        assert!(ids.iter().all(|&id| id > 0), "more than four");
        for id in ids {
            rm(id);
        }
        rm(big);
    }

    #[test]
    fn ftok_of_a_missing_file_is_minus_one() {
        assert_eq!(
            super::ftok(c"/no/such/file/for/ftok".as_ptr().cast(), 1),
            -1
        );
    }

    #[test]
    fn shmget_sizes() {
        err(get(IPC_PRIVATE, 0, 0o600), -1, errno::EINVAL);
        err(get(IPC_PRIVATE, SHMMAX + 1, 0o600), -1, errno::EINVAL);
        let id = get(IPC_PRIVATE, 1, 0o600);
        assert!(id > 0, "SHMMIN is 1");
        rm(id);
        err(get(IPC_PRIVATE, 16, 0o600 | SHM_HUGETLB), -1, errno::ENOMEM);
    }

    #[test]
    fn shmget_by_key_in_linuxs_order() {
        let key = 0x5A11_0001;
        err(get(key, 100, 0o600), -1, errno::ENOENT);
        let id = get(key, 100, IPC_CREAT | 0o600);
        assert!(id > 0);
        assert_eq!(get(key, 100, 0), id, "found");
        assert_eq!(get(key, 50, 0), id, "a smaller size is fine");
        err(
            get(key, 100, IPC_CREAT | IPC_EXCL | 0o600),
            -1,
            errno::EEXIST,
        );
        err(get(key, 200, 0), -1, errno::EINVAL);
        rm(id);
    }

    #[test]
    fn shmget_judges_the_size_before_the_permission() {
        let key = 0x5A11_0002;
        let id = get(key, 100, IPC_CREAT | 0o600);
        let mut ds = stat_of(id);
        ds.shm_perm.mode = 0o000;
        assert_eq!(shmctl(id, IPC_SET, &raw mut ds), 0);
        // Running as root with CAP_IPC_OWNER the permission always passes,
        // so only the size's EINVAL is observable here, before it.
        err(get(key, 200, 0o600), -1, errno::EINVAL);
        rm(id);
    }

    // -- shmat, shmdt --

    #[test]
    fn attaches_share_the_bytes_and_count() {
        let id = get(IPC_PRIVATE, 4096, 0o600);
        let a = shmat(id, core::ptr::null(), 0);
        assert_ne!(a, FAIL);
        let b = shmat(id, core::ptr::null(), 0);
        assert_ne!(b, FAIL);
        unsafe { a.write(42) };
        assert_eq!(unsafe { b.read() }, 42, "one segment behind both");
        assert_eq!(stat_of(id).shm_nattch, 2);
        assert_ne!(stat_of(id).shm_atime, 0);
        assert_eq!(shmdt(a), 0);
        assert_eq!(stat_of(id).shm_nattch, 1);
        assert_eq!(shmdt(b), 0);
        assert_eq!(stat_of(id).shm_nattch, 0);
        assert_ne!(stat_of(id).shm_dtime, 0);
        rm(id);
    }

    #[test]
    fn shmat_checks_in_linuxs_order() {
        let id = get(IPC_PRIVATE, 100, 0o600);
        err(shmat(-1, core::ptr::null(), 0), FAIL, errno::EINVAL);
        err(shmat(id, 1 as *const u8, 0), FAIL, errno::EINVAL);
        err(shmat(id, core::ptr::null(), SHM_REMAP), FAIL, errno::EINVAL);
        err(
            shmat(id, 1 as *const u8, SHM_RND | SHM_REMAP),
            FAIL,
            errno::EINVAL,
        );
        err(
            shmat(0x7FFF_0001, core::ptr::null(), 0),
            FAIL,
            errno::EINVAL,
        );
        // Not Linux's: the kernel chooses the address, and never grants exec.
        err(shmat(id, SHMLBA as *const u8, 0), FAIL, errno::EINVAL);
        err(shmat(id, core::ptr::null(), SHM_EXEC), FAIL, errno::EACCES);
        rm(id);
    }

    #[test]
    fn shmdt_needs_an_attach_address() {
        err(shmdt(core::ptr::null()), -1, errno::EINVAL);
        err(shmdt(1 as *const u8), -1, errno::EINVAL);
        let id = get(IPC_PRIVATE, 100, 0o600);
        let a = shmat(id, core::ptr::null(), SHM_RDONLY);
        assert_ne!(a, FAIL);
        assert_eq!(shmdt(a), 0);
        err(shmdt(a), -1, errno::EINVAL);
        rm(id);
    }

    // -- removal --

    #[test]
    fn removed_while_attached_lives_until_the_last_detach() {
        let key = 0x5A11_0003;
        let id = get(key, 100, IPC_CREAT | 0o600);
        let a = shmat(id, core::ptr::null(), 0);
        rm(id);
        let ds = stat_of(id);
        assert_ne!(ds.shm_perm.mode & SHM_DEST, 0, "marked");
        assert_eq!(
            ds.shm_perm.__ipc_perm_key, IPC_PRIVATE,
            "its key is private"
        );
        err(get(key, 100, 0), -1, errno::ENOENT);
        let b = shmat(id, core::ptr::null(), 0);
        assert_ne!(b, FAIL, "Linux attaches a marked segment by its id");
        assert_eq!(shmdt(a), 0);
        assert_eq!(shmdt(b), 0);
        let mut ds: ShmidDs = unsafe { core::mem::zeroed() };
        err(shmctl(id, IPC_STAT, &raw mut ds), -1, errno::EINVAL);
    }

    #[test]
    fn a_removed_segments_id_does_not_come_back() {
        let a = get(IPC_PRIVATE, 100, 0o600);
        rm(a);
        let b = get(IPC_PRIVATE, 100, 0o600);
        assert_ne!(a, b, "the slot's reuse count moved");
        let mut ds: ShmidDs = unsafe { core::mem::zeroed() };
        err(shmctl(a, IPC_STAT, &raw mut ds), -1, errno::EINVAL);
        rm(b);
    }

    // -- shmctl --

    #[test]
    fn ipc_set_reads_its_buffer_before_the_lookup() {
        // THE PASS'S NULL SITE: this looked the segment up first, so a bad
        // id with a NULL buffer was EINVAL; Linux copies first.
        err(
            shmctl(0x7FFF_0001, IPC_SET, core::ptr::null_mut()),
            -1,
            errno::EFAULT,
        );
        let id = get(IPC_PRIVATE, 100, 0o600);
        err(
            shmctl(id, IPC_STAT, core::ptr::null_mut()),
            -1,
            errno::EFAULT,
        );
        let mut ds: ShmidDs = unsafe { core::mem::zeroed() };
        err(
            shmctl(0x7FFF_0001, IPC_STAT, &raw mut ds),
            -1,
            errno::EINVAL,
        );
        rm(id);
    }

    #[test]
    fn ipc_set_changes_owner_and_mode_only() {
        let id = get(IPC_PRIVATE, 100, 0o600);
        let mut ds = stat_of(id);
        ds.shm_perm.mode = 0o7644;
        ds.shm_segsz = 1;
        assert_eq!(shmctl(id, IPC_SET, &raw mut ds), 0);
        let now = stat_of(id);
        assert_eq!(now.shm_perm.mode, 0o644, "the permission bits only");
        assert_eq!(now.shm_segsz, 100);
        ds.shm_perm.uid = u32::MAX;
        err(shmctl(id, IPC_SET, &raw mut ds), -1, errno::EINVAL);
        rm(id);
    }

    #[test]
    fn info_stat_and_the_negatives() {
        err(
            shmctl(-1, IPC_STAT, core::ptr::null_mut()),
            -1,
            errno::EINVAL,
        );
        err(shmctl(0, -1, core::ptr::null_mut()), -1, errno::EINVAL);
        err(shmctl(0, 99, core::ptr::null_mut()), -1, errno::EINVAL);

        let id = get(IPC_PRIVATE, 3 * SHMLBA + 1, 0o600);
        let mut limits = Shminfo::default();
        let top = shmctl(0, IPC_INFO, (&raw mut limits).cast());
        assert!(top >= 0);
        assert_eq!(
            (limits.shmmax, limits.shmmin, limits.shmmni),
            (SHMMAX, 1, SHMMNI)
        );
        let mut used = ShmInfo::default();
        assert_eq!(shmctl(0, SHM_INFO, (&raw mut used).cast()), top);
        assert_eq!(
            (used.used_ids, used.shm_tot),
            (1, 4),
            "four pages, one segment"
        );
        err(
            shmctl(0, IPC_INFO, core::ptr::null_mut()),
            -1,
            errno::EFAULT,
        );

        let index = (id & 0xFFFF) - 1;
        let mut ds: ShmidDs = unsafe { core::mem::zeroed() };
        assert_eq!(
            shmctl(index, SHM_STAT, &raw mut ds),
            id,
            "SHM_STAT answers the id"
        );
        assert_eq!(shmctl(index, SHM_STAT_ANY, &raw mut ds), id);
        assert_eq!(ds.shm_segsz, 3 * SHMLBA + 1);
        rm(id);
    }

    #[test]
    fn shm_lock_is_recorded() {
        let id = get(IPC_PRIVATE, 100, 0o600);
        assert_eq!(shmctl(id, SHM_LOCK, core::ptr::null_mut()), 0);
        assert_ne!(stat_of(id).shm_perm.mode & SHM_LOCKED, 0);
        assert_eq!(shmctl(id, SHM_UNLOCK, core::ptr::null_mut()), 0);
        assert_eq!(stat_of(id).shm_perm.mode & SHM_LOCKED, 0);
        err(
            shmctl(0x7FFF_0001, SHM_LOCK, core::ptr::null_mut()),
            -1,
            errno::EINVAL,
        );
        rm(id);
    }

    #[test]
    fn the_totals_follow_the_segments() {
        let a = get(IPC_PRIVATE, SHMLBA, 0o600);
        let b = get(IPC_PRIVATE, SHMLBA + 1, 0o600);
        let mut used = ShmInfo::default();
        shmctl(0, SHM_INFO, (&raw mut used).cast());
        assert_eq!((used.used_ids, used.shm_tot), (2, 3));
        rm(a);
        rm(b);
        shmctl(0, SHM_INFO, (&raw mut used).cast());
        assert_eq!((used.used_ids, used.shm_tot), (0, 0));
    }

    #[test]
    fn the_structures_are_linuxs() {
        assert_eq!(size_of::<Shminfo>(), 9 * 8);
        assert_eq!(size_of::<ShmInfo>(), 6 * 8);
        assert_eq!(core::mem::offset_of!(ShmInfo, shm_tot), 8);
    }
}
