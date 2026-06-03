//! System V semaphores — `<sys/sem.h>`.
//!
//! A real in-memory implementation of `semget`, `semop`, `semtimedop`,
//! and `semctl`, modeled on the `mqueue` precedent.
//!
//! ## Design
//!
//! All state lives in a single static pool of [`MAX_SETS`] semaphore
//! sets, each holding up to [`MAX_SEMS_PER_SET`] semaphores.  A single
//! global spinlock ([`SEM_LOCK`]) protects all mutations — coarse but
//! adequate, since each operation is a bounded array update.
//!
//! Keys are i32 values supplied by the caller.  A key of [`IPC_PRIVATE`]
//! (0) always allocates a fresh anonymous set on every call.  Non-zero
//! keys are looked up in the table; with [`IPC_CREAT`] set the caller
//! gets back either the existing matching set (unless [`IPC_EXCL`]) or
//! a freshly allocated one.  Without `IPC_CREAT`, a missing key returns
//! `-1` / `ENOENT`.
//!
//! The semid value handed to userspace is the slot index plus the
//! per-slot generation counter packed in the high bits:
//! `semid = (generation << 16) | (slot + 1)`.  This rejects stale
//! `semid` values after a slot is `IPC_RMID`'d and reused.
//!
//! ### Blocking semantics
//!
//! `semop` with `sem_op < 0` (acquire) blocks when the value is too
//! small, `sem_op == 0` (wait-for-zero) blocks while the value is
//! non-zero, `sem_op > 0` always succeeds immediately (release).  The
//! blocking loop drops the lock between checks and spin-yields so other
//! threads can release the semaphore.  `IPC_NOWAIT` in `sem_flg` turns
//! the would-block path into an immediate `EAGAIN`.
//!
//! `semtimedop` uses the same blocking loop but consults
//! `clock_gettime(CLOCK_REALTIME)` against the absolute-deadline
//! `timespec` argument and returns `EAGAIN` (mapped from POSIX
//! `ETIMEDOUT`-style timeout) when the deadline expires.  POSIX
//! requires `EAGAIN` here, not `ETIMEDOUT`, because semop unifies
//! "timed out" with "would block" — see `man 2 semop` "ERRORS / EAGAIN".
//!
//! ### Atomicity
//!
//! A multi-op `semop` is all-or-nothing: if any op would block, the
//! kernel rolls back any earlier ops in the same batch before sleeping.
//! Our implementation matches this — we apply ops to a scratch
//! `values` array, and only commit (writing back to the set + bumping
//! `sempid`) once every op succeeded.  If one would block we either
//! return `EAGAIN` (NOWAIT) or release the lock and retry the whole
//! batch from scratch (blocking).
//!
//! ### Limitations
//!
//! * Single-process only — the table lives in this process's address
//!   space.  Cross-process Sys V IPC would need a kernel-side named
//!   object namespace (deferred).
//! * `SEM_UNDO` is accepted but ignored — we have no per-process undo
//!   tracking yet.  Programs that depend on rollback on crash (like
//!   classic Sys V mutex idioms with `sem_op = -1, sem_flg = SEM_UNDO`)
//!   will not get their semaphore restored.
//! * Permission bits (the low 9 bits of `semflg`) are stored but not
//!   enforced — getuid() returns 0 in our world so all checks succeed
//!   trivially anyway.
//! * `IPC_INFO`/`SEM_INFO`/`SEM_STAT` (Linux extensions) return
//!   `EINVAL`.

use crate::errno;
use crate::stat::Timespec;
use core::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Constants (shared with sysv_msg)
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;
/// No wait.
pub const IPC_NOWAIT: i32 = 0o4000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key.
pub const IPC_PRIVATE: i32 = 0;

// Semaphore control commands.
/// Get value of semaphore.
pub const GETVAL: i32 = 12;
/// Set value of semaphore.
pub const SETVAL: i32 = 16;
/// Get all semaphore values.
pub const GETALL: i32 = 13;
/// Set all semaphore values.
pub const SETALL: i32 = 17;
/// Get number of processes waiting for increase.
pub const GETNCNT: i32 = 14;
/// Get number of processes waiting for zero.
pub const GETZCNT: i32 = 15;
/// Get PID of last operation.
pub const GETPID: i32 = 11;

/// Undo flag — semaphore operations are undone on process exit.
pub const SEM_UNDO: i32 = 0x1000;

/// Maximum value a single semaphore can hold (POSIX `SEMVMX`).  Matches
/// Linux: any individual sem op may not push a value above this.
pub const SEMVMX: i16 = 32_767;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct sembuf` — semaphore operation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sembuf {
    /// Semaphore number (index in the set).
    pub sem_num: u16,
    /// Semaphore operation: positive (release), negative (acquire), or zero (wait).
    pub sem_op: i16,
    /// Operation flags (e.g., `IPC_NOWAIT`, `SEM_UNDO`).
    pub sem_flg: i16,
}

// ---------------------------------------------------------------------------
// Pool sizing
// ---------------------------------------------------------------------------

const MAX_SETS: usize = 16;
const MAX_SEMS_PER_SET: usize = 32;

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct SemSet {
    in_use: bool,
    /// Caller-supplied key (or 0 for IPC_PRIVATE sets).
    key: i32,
    /// Number of semaphores actually in use (≤ MAX_SEMS_PER_SET).
    nsems: usize,
    /// Permission bits (low 9 bits of `semflg`); stored but unenforced.
    mode: u16,
    /// Generation counter, bumped each time the slot is reused.  Mixed
    /// into the user-visible semid to detect use-after-RMID.
    generation: u32,
    /// Current semaphore values.
    values: [i16; MAX_SEMS_PER_SET],
    /// PID of the last process that performed a successful op on each
    /// semaphore (GETPID).  Our world has no real PIDs; we record 0
    /// (the only process) so the field is at least non-garbage.
    sempid: [i32; MAX_SEMS_PER_SET],
}

impl SemSet {
    const EMPTY: Self = Self {
        in_use: false,
        key: 0,
        nsems: 0,
        mode: 0,
        generation: 0,
        values: [0i16; MAX_SEMS_PER_SET],
        sempid: [0i32; MAX_SEMS_PER_SET],
    };
}

// ---------------------------------------------------------------------------
// Static state
// ---------------------------------------------------------------------------

static SEM_LOCK: AtomicBool = AtomicBool::new(false);
static mut SEM_SETS: [SemSet; MAX_SETS] = [const { SemSet::EMPTY }; MAX_SETS];

fn lock_acquire() {
    while SEM_LOCK
        .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn lock_release() {
    SEM_LOCK.store(false, Ordering::Release);
}

/// RAII guard that releases the global lock on drop.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        lock_release();
    }
}

fn lock() -> Guard {
    lock_acquire();
    Guard
}

// ---------------------------------------------------------------------------
// Set ID encoding
// ---------------------------------------------------------------------------

/// Pack a (slot, generation) pair into a user-visible `semid`.
fn encode_semid(slot: usize, generation: u32) -> i32 {
    // slot in low 16 bits (as slot+1, so 0 never escapes),
    // generation in the next 15 bits.
    let s = ((slot as u32) & 0xFFFF).wrapping_add(1);
    let g = generation & 0x7FFF;
    ((g << 16) | s) as i32
}

/// Decode a `semid` into a (slot, generation) pair.  Returns `None` if
/// the encoding is malformed (e.g., zero, negative, or out of range).
fn decode_semid(semid: i32) -> Option<(usize, u32)> {
    if semid <= 0 {
        return None;
    }
    let u = semid as u32;
    let s = (u & 0xFFFF) as usize;
    if s == 0 {
        return None;
    }
    let slot = s - 1;
    if slot >= MAX_SETS {
        return None;
    }
    let generation = (u >> 16) & 0x7FFF;
    Some((slot, generation))
}

// ---------------------------------------------------------------------------
// Helpers (all callers hold the lock)
// ---------------------------------------------------------------------------

/// SAFETY: Caller must hold `SEM_LOCK`.
unsafe fn sets_ptr() -> *mut SemSet {
    core::ptr::addr_of_mut!(SEM_SETS).cast::<SemSet>()
}

/// Find a set by key.  Returns `Some(slot)` or `None`.
///
/// SAFETY: Caller must hold the lock.
unsafe fn find_set_by_key(key: i32) -> Option<usize> {
    if key == IPC_PRIVATE {
        return None; // IPC_PRIVATE never matches an existing set.
    }
    let sets = unsafe { sets_ptr() };
    let mut i: usize = 0;
    while i < MAX_SETS {
        let s = unsafe { sets.add(i) };
        if unsafe { (*s).in_use } && unsafe { (*s).key } == key {
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// Allocate an unused set slot.  Returns `Some(slot)` or `None` if the
/// pool is exhausted.
///
/// SAFETY: Caller must hold the lock.
unsafe fn alloc_set(key: i32, nsems: usize, mode: u16) -> Option<usize> {
    let sets = unsafe { sets_ptr() };
    let mut i: usize = 0;
    while i < MAX_SETS {
        let s = unsafe { sets.add(i) };
        if !unsafe { (*s).in_use } {
            unsafe {
                (*s).in_use = true;
                (*s).key = key;
                (*s).nsems = nsems;
                (*s).mode = mode;
                // Initialize values & sempid (don't zero generation —
                // it persists across reuse to invalidate stale semids).
                let mut k: usize = 0;
                while k < MAX_SEMS_PER_SET {
                    (*s).values[k] = 0;
                    (*s).sempid[k] = 0;
                    k = k.wrapping_add(1);
                }
            }
            return Some(i);
        }
        i = i.wrapping_add(1);
    }
    None
}

/// Look up a set by semid, validating the generation tag.
///
/// SAFETY: Caller must hold the lock.
unsafe fn resolve_semid(semid: i32) -> Option<usize> {
    let (slot, gen_) = decode_semid(semid)?;
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    if !unsafe { (*s).in_use } {
        return None;
    }
    if unsafe { (*s).generation } & 0x7FFF != gen_ {
        return None;
    }
    Some(slot)
}

// ---------------------------------------------------------------------------
// semget
// ---------------------------------------------------------------------------

/// `semget` — get a semaphore set identifier.
///
/// Allocates a new set (with `IPC_CREAT`) or returns an existing one
/// keyed by `key`.  `IPC_PRIVATE` always allocates fresh.
///
/// Returns the semid on success, or `-1` with errno set on failure
/// (EINVAL, ENOENT, EEXIST, ENOSPC).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semget(key: i32, nsems: i32, semflg: i32) -> i32 {
    // Validate nsems first.  Linux/POSIX:
    //   - For an existing set looked up by key, nsems == 0 is permitted
    //     (caller just wants the id).
    //   - For a new set, nsems must be in 1..=SEMMSL (our MAX_SEMS_PER_SET).
    if nsems < 0 || (nsems as usize) > MAX_SEMS_PER_SET {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mode = (semflg & 0o777) as u16;
    let _g = lock();
    // IPC_PRIVATE: always allocate a fresh set.
    if key == IPC_PRIVATE {
        if nsems < 1 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        let Some(slot) = (unsafe { alloc_set(IPC_PRIVATE, nsems as usize, mode) }) else {
            errno::set_errno(errno::ENOSPC);
            return -1;
        };
        // SAFETY: lock held; slot is the freshly-allocated slot.
        let gen_ = unsafe {
            let sets = sets_ptr();
            (*sets.add(slot)).generation & 0x7FFF
        };
        return encode_semid(slot, gen_);
    }
    // Non-private key: look up first.
    let existing = unsafe { find_set_by_key(key) };
    if let Some(slot) = existing {
        if semflg & IPC_CREAT != 0 && semflg & IPC_EXCL != 0 {
            errno::set_errno(errno::EEXIST);
            return -1;
        }
        // If caller passed a nonzero nsems and the existing set is
        // smaller, Linux returns EINVAL.
        if nsems > 0 {
            let cur = unsafe {
                let sets = sets_ptr();
                (*sets.add(slot)).nsems
            };
            if (nsems as usize) > cur {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
        }
        let gen_ = unsafe {
            let sets = sets_ptr();
            (*sets.add(slot)).generation & 0x7FFF
        };
        return encode_semid(slot, gen_);
    }
    // Not found.
    if semflg & IPC_CREAT == 0 {
        errno::set_errno(errno::ENOENT);
        return -1;
    }
    if nsems < 1 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Some(slot) = (unsafe { alloc_set(key, nsems as usize, mode) }) else {
        errno::set_errno(errno::ENOSPC);
        return -1;
    };
    let gen_ = unsafe {
        let sets = sets_ptr();
        (*sets.add(slot)).generation & 0x7FFF
    };
    encode_semid(slot, gen_)
}

// ---------------------------------------------------------------------------
// semop / semtimedop
// ---------------------------------------------------------------------------

/// Outcome of attempting a batch of ops against a set.
enum BatchResult {
    /// All ops applied to the scratch values successfully (caller
    /// commits).  Carries the new values and the per-semaphore touched
    /// flag (true if that index was modified).
    Ok([i16; MAX_SEMS_PER_SET], [bool; MAX_SEMS_PER_SET]),
    /// One op would block.  Carries the index of the offending op so
    /// caller decides whether to wait or to return EAGAIN.
    WouldBlock,
    /// One op was malformed (bad sem_num, overflow, etc.); errno
    /// already set.  Carries `-1` to be returned to the user directly.
    Error,
}

/// Try to apply a batch of ops to a scratch copy of the set's values.
/// Returns the new values + touched-mask, `WouldBlock`, or `Error`.
///
/// SAFETY: Caller must hold the lock; `slot` must be a valid in-use slot.
unsafe fn try_apply_batch(slot: usize, sops: &[Sembuf]) -> BatchResult {
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    let nsems = unsafe { (*s).nsems };
    let mut values: [i16; MAX_SEMS_PER_SET] = unsafe { (*s).values };
    let mut touched: [bool; MAX_SEMS_PER_SET] = [false; MAX_SEMS_PER_SET];
    for op in sops {
        let idx = op.sem_num as usize;
        if idx >= nsems {
            errno::set_errno(errno::EFBIG);
            return BatchResult::Error;
        }
        let cur = values[idx];
        let delta = op.sem_op;
        if delta == 0 {
            if cur != 0 {
                return BatchResult::WouldBlock;
            }
            // Zero — no value change but caller still gets sempid update.
            touched[idx] = true;
            continue;
        }
        let new_val = i32::from(cur) + i32::from(delta);
        if delta > 0 {
            if new_val > i32::from(SEMVMX) {
                errno::set_errno(errno::ERANGE);
                return BatchResult::Error;
            }
            values[idx] = new_val as i16;
            touched[idx] = true;
            continue;
        }
        // delta < 0 (acquire).
        if new_val < 0 {
            return BatchResult::WouldBlock;
        }
        values[idx] = new_val as i16;
        touched[idx] = true;
    }
    BatchResult::Ok(values, touched)
}

/// Commit the result of a successful batch back to the set.
///
/// SAFETY: Caller must hold the lock; `slot` must be a valid in-use slot.
unsafe fn commit_batch(
    slot: usize,
    new_values: &[i16; MAX_SEMS_PER_SET],
    touched: &[bool; MAX_SEMS_PER_SET],
) {
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    let nsems = unsafe { (*s).nsems };
    let mut i: usize = 0;
    while i < nsems {
        unsafe {
            (*s).values[i] = new_values[i];
            if touched[i] {
                (*s).sempid[i] = 0; // no real pids in our world
            }
        }
        i = i.wrapping_add(1);
    }
}

/// Shared implementation of `semop` / `semtimedop`.
///
/// `deadline_ns`: `None` for unbounded blocking, `Some` for a
/// `CLOCK_REALTIME` absolute deadline in nanoseconds.
fn semop_common(semid: i32, sops: *const Sembuf, nsops: usize, deadline_ns: Option<u64>) -> i32 {
    if sops.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if nsops == 0 || nsops > MAX_SEMS_PER_SET {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    // Snapshot the op array into a fixed-size buffer so we can release
    // the lock during blocking without holding a pointer to caller mem.
    let mut buf: [Sembuf; MAX_SEMS_PER_SET] = [Sembuf {
        sem_num: 0,
        sem_op: 0,
        sem_flg: 0,
    }; MAX_SEMS_PER_SET];
    for (i, slot) in buf.iter_mut().enumerate().take(nsops) {
        // SAFETY: caller contract; we just bounded `nsops` above.
        *slot = unsafe { *sops.add(i) };
    }
    let sops_slice = &buf[..nsops];
    // Detect IPC_NOWAIT: present on any op of the batch.  Linux's
    // behaviour: if any sembuf in the array sets IPC_NOWAIT, the
    // entire op is non-blocking (the flag is per-op semantically but
    // any-blocker → fail).
    let nowait = sops_slice
        .iter()
        .any(|op| op.sem_flg & IPC_NOWAIT as i16 != 0);

    loop {
        {
            let _g = lock();
            let Some(slot) = (unsafe { resolve_semid(semid) }) else {
                errno::set_errno(errno::EINVAL);
                return -1;
            };
            match unsafe { try_apply_batch(slot, sops_slice) } {
                BatchResult::Ok(new_values, touched) => {
                    unsafe { commit_batch(slot, &new_values, &touched) };
                    return 0;
                }
                BatchResult::Error => {
                    // errno already set.
                    return -1;
                }
                BatchResult::WouldBlock => {
                    if nowait {
                        errno::set_errno(errno::EAGAIN);
                        return -1;
                    }
                    if let Some(dl) = deadline_ns
                        && now_realtime_ns() >= dl
                    {
                        errno::set_errno(errno::EAGAIN);
                        return -1;
                    }
                    // Drop the lock and try again.  Fall through to
                    // hint::spin_loop outside the guard.
                }
            }
        }
        core::hint::spin_loop();
    }
}

/// `semop` — perform semaphore operations (blocking).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semop(semid: i32, sops: *const Sembuf, nsops: usize) -> i32 {
    semop_common(semid, sops, nsops, None)
}

/// `semtimedop` — perform semaphore operations with absolute timeout.
///
/// `timeout` is a `*const Timespec` interpreted against `CLOCK_REALTIME`.
/// A NULL `timeout` means "wait forever" (same as `semop`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semtimedop(
    semid: i32,
    sops: *const Sembuf,
    nsops: usize,
    timeout: *const u8, // *const Timespec — kept as opaque for the public ABI
) -> i32 {
    let deadline = if timeout.is_null() {
        None
    } else {
        // The public C ABI exposes `*const u8` (opaque), so the pointer
        // may not be Timespec-aligned.  Use `read_unaligned` rather than
        // a typed deref.
        // SAFETY: caller contract — non-null pointer to a Timespec.
        let ts = unsafe { core::ptr::read_unaligned(timeout.cast::<Timespec>()) };
        if ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 || ts.tv_sec < 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        Some(timespec_to_ns(&ts))
    };
    semop_common(semid, sops, nsops, deadline)
}

// ---------------------------------------------------------------------------
// semctl
// ---------------------------------------------------------------------------

/// `semctl` — semaphore control operations.
///
/// Three-arg form: the variadic union argument is omitted.  For
/// `SETVAL` (which needs a value) and `SETALL`/`GETALL`/`IPC_STAT`
/// (which need a pointer) callers must use the four-arg variants
/// below.  The three-arg form supports:
///   * `IPC_RMID` — remove the set, invalidating all outstanding semids
///   * `GETVAL` — return the current value of `sem_num`
///   * `GETPID` / `GETNCNT` / `GETZCNT` — currently always return 0
///     (no per-process wait tracking; we have no real PIDs)
///   * Anything else → `EINVAL`
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semctl(semid: i32, semnum: i32, cmd: i32) -> i32 {
    let _g = lock();
    let Some(slot) = (unsafe { resolve_semid(semid) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    // SAFETY: lock held, slot validated.
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    let nsems = unsafe { (*s).nsems };
    match cmd {
        IPC_RMID => {
            // Bump generation so any outstanding semid becomes invalid.
            unsafe {
                (*s).in_use = false;
                (*s).generation = (*s).generation.wrapping_add(1);
                (*s).key = 0;
                (*s).nsems = 0;
            }
            0
        }
        GETVAL => {
            if semnum < 0 || (semnum as usize) >= nsems {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            unsafe { i32::from((*s).values[semnum as usize]) }
        }
        GETPID | GETNCNT | GETZCNT => {
            if semnum < 0 || (semnum as usize) >= nsems {
                errno::set_errno(errno::EINVAL);
                return -1;
            }
            // We don't track waiters; sempid is always 0.
            0
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// `semctl_setval` — four-arg form for `SETVAL`.
///
/// Sets semaphore `semnum` in `semid` to `val`.  Returns 0 on success
/// or -1 with errno on failure.  This is a deliberate helper rather
/// than a C-variadic shim because Rust can't safely emit variadics on
/// stable.  Callers that need POSIX `int semctl(int, int, int, ...)`
/// can write a tiny C shim.
pub extern "C" fn semctl_setval(semid: i32, semnum: i32, val: i32) -> i32 {
    if !(0..=i32::from(SEMVMX)).contains(&val) {
        errno::set_errno(errno::ERANGE);
        return -1;
    }
    let _g = lock();
    let Some(slot) = (unsafe { resolve_semid(semid) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    let nsems = unsafe { (*s).nsems };
    if semnum < 0 || (semnum as usize) >= nsems {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    unsafe {
        (*s).values[semnum as usize] = val as i16;
        (*s).sempid[semnum as usize] = 0;
    }
    0
}

/// `semctl_setall` — four-arg form for `SETALL`.
///
/// `array` points to a `u16` array of length `nsems` (the set's size).
/// Each value must be in `0..=SEMVMX`.
///
/// # Safety
///
/// `array` must point to at least `nsems` consecutive `u16` values
/// where `nsems` is the size of the set identified by `semid`.
pub unsafe extern "C" fn semctl_setall(semid: i32, array: *const u16) -> i32 {
    if array.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let _g = lock();
    let Some(slot) = (unsafe { resolve_semid(semid) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    let nsems = unsafe { (*s).nsems };
    // Read & validate first so a bad value doesn't leave the set
    // half-updated.
    let mut buf: [i16; MAX_SEMS_PER_SET] = [0; MAX_SEMS_PER_SET];
    let mut i: usize = 0;
    while i < nsems {
        // SAFETY: caller contract.
        let v = unsafe { *array.add(i) };
        if v > SEMVMX as u16 {
            errno::set_errno(errno::ERANGE);
            return -1;
        }
        buf[i] = v as i16;
        i = i.wrapping_add(1);
    }
    let mut j: usize = 0;
    while j < nsems {
        unsafe {
            (*s).values[j] = buf[j];
            (*s).sempid[j] = 0;
        }
        j = j.wrapping_add(1);
    }
    0
}

/// `semctl_getall` — four-arg form for `GETALL`.
///
/// `array` is filled with the current values of all semaphores in the
/// set.
///
/// # Safety
///
/// `array` must point to at least `nsems` consecutive writable `u16`
/// slots where `nsems` is the size of the set.
pub unsafe extern "C" fn semctl_getall(semid: i32, array: *mut u16) -> i32 {
    if array.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let _g = lock();
    let Some(slot) = (unsafe { resolve_semid(semid) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let sets = unsafe { sets_ptr() };
    let s = unsafe { sets.add(slot) };
    let nsems = unsafe { (*s).nsems };
    let mut i: usize = 0;
    while i < nsems {
        // SAFETY: caller contract.
        unsafe { *array.add(i) = (*s).values[i] as u16 };
        i = i.wrapping_add(1);
    }
    0
}

// ---------------------------------------------------------------------------
// Timespec helpers
// ---------------------------------------------------------------------------

fn now_realtime_ns() -> u64 {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let r = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut ts);
    if r != 0 {
        return 0;
    }
    timespec_to_ns(&ts)
}

fn timespec_to_ns(ts: &Timespec) -> u64 {
    let sec = ts.tv_sec.max(0) as u64;
    let nsec = ts.tv_nsec.max(0) as u64;
    sec.saturating_mul(1_000_000_000).saturating_add(nsec)
}

// ---------------------------------------------------------------------------
// Test-only helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
fn test_reset_all() {
    // Wipe the table.  Safe because tests serialize via TEST_LOCK below.
    let _g = lock();
    // SAFETY: lock held.
    let sets = unsafe { sets_ptr() };
    let mut i: usize = 0;
    while i < MAX_SETS {
        unsafe {
            // Preserve generation so any leftover stale semids in other
            // tests still get rejected.
            (*sets.add(i)).in_use = false;
            (*sets.add(i)).key = 0;
            (*sets.add(i)).nsems = 0;
            (*sets.add(i)).generation = (*sets.add(i)).generation.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialize tests that touch the global table — cargo runs tests
    /// in parallel and our table is process-global.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean_table<F: FnOnce()>(f: F) {
        let _g = TEST_LOCK.lock().unwrap();
        test_reset_all();
        f();
    }

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipc_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
        assert_eq!(IPC_NOWAIT, 0o4000);
    }

    #[test]
    fn test_sem_commands_distinct() {
        assert_ne!(GETVAL, SETVAL);
        assert_ne!(GETALL, SETALL);
        assert_ne!(GETNCNT, GETZCNT);
    }

    #[test]
    fn test_sem_undo() {
        assert_ne!(SEM_UNDO, 0);
    }

    #[test]
    fn test_sembuf_size() {
        assert_eq!(core::mem::size_of::<Sembuf>(), 6);
    }

    #[test]
    fn test_sembuf_fields() {
        let sb = Sembuf {
            sem_num: 0,
            sem_op: -1,
            sem_flg: SEM_UNDO as i16,
        };
        assert_eq!(sb.sem_num, 0);
        assert_eq!(sb.sem_op, -1);
        assert_eq!(sb.sem_flg, SEM_UNDO as i16);
    }

    // -----------------------------------------------------------------------
    // semid encoding
    // -----------------------------------------------------------------------

    #[test]
    fn test_semid_encode_decode_roundtrip() {
        for slot in 0..MAX_SETS {
            for gen_ in [0u32, 1, 7, 0x7FFE, 0x7FFF] {
                let id = encode_semid(slot, gen_);
                let (s, g) = decode_semid(id).unwrap();
                assert_eq!(s, slot);
                assert_eq!(g, gen_);
            }
        }
    }

    #[test]
    fn test_semid_decode_rejects_zero() {
        assert!(decode_semid(0).is_none());
    }

    #[test]
    fn test_semid_decode_rejects_negative() {
        assert!(decode_semid(-1).is_none());
    }

    #[test]
    fn test_semid_decode_rejects_out_of_range_slot() {
        // slot+1 = MAX_SETS+1 → slot = MAX_SETS, out of range.
        let id = encode_semid(MAX_SETS, 0);
        // encode_semid mod-16's the slot, so this actually wraps.  Test
        // a directly malformed encoding instead.
        let bad = (MAX_SETS as u32 + 1) as i32;
        assert!(decode_semid(bad).is_none());
        let _ = id;
    }

    // -----------------------------------------------------------------------
    // semget
    // -----------------------------------------------------------------------

    #[test]
    fn test_semget_private_creates_new_set() {
        with_clean_table(|| {
            let id1 = semget(IPC_PRIVATE, 4, IPC_CREAT | 0o666);
            let id2 = semget(IPC_PRIVATE, 4, IPC_CREAT | 0o666);
            assert_ne!(id1, -1);
            assert_ne!(id2, -1);
            assert_ne!(id1, id2);
        });
    }

    #[test]
    fn test_semget_keyed_lookup() {
        with_clean_table(|| {
            let id1 = semget(0x1234, 3, IPC_CREAT | 0o666);
            assert_ne!(id1, -1);
            // Same key, no creat — should find the existing.
            let id2 = semget(0x1234, 3, 0);
            assert_eq!(id1, id2);
        });
    }

    #[test]
    fn test_semget_missing_no_creat_enoent() {
        with_clean_table(|| {
            errno::set_errno(0);
            let id = semget(0x9999, 1, 0);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::ENOENT);
        });
    }

    #[test]
    fn test_semget_excl_existing_eexist() {
        with_clean_table(|| {
            let _ = semget(0x4321, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let id = semget(0x4321, 1, IPC_CREAT | IPC_EXCL | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EEXIST);
        });
    }

    #[test]
    fn test_semget_bad_nsems_einval() {
        with_clean_table(|| {
            errno::set_errno(0);
            let id = semget(IPC_PRIVATE, -1, IPC_CREAT);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
            errno::set_errno(0);
            let id = semget(IPC_PRIVATE, (MAX_SEMS_PER_SET + 1) as i32, IPC_CREAT);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semget_private_zero_nsems_einval() {
        with_clean_table(|| {
            errno::set_errno(0);
            let id = semget(IPC_PRIVATE, 0, IPC_CREAT);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semget_lookup_too_small_einval() {
        with_clean_table(|| {
            let _ = semget(0x55aa, 2, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let id = semget(0x55aa, 5, 0);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semget_pool_exhaustion_enospc() {
        with_clean_table(|| {
            let mut ids = std::vec::Vec::new();
            for _ in 0..MAX_SETS {
                let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
                assert_ne!(id, -1);
                ids.push(id);
            }
            errno::set_errno(0);
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            assert_eq!(id, -1);
            assert_eq!(errno::get_errno(), errno::ENOSPC);
        });
    }

    // -----------------------------------------------------------------------
    // semop — happy path
    // -----------------------------------------------------------------------

    #[test]
    fn test_semop_release_then_acquire() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            assert_ne!(id, -1);
            // Release (V).
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 3,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, v.as_ptr(), 1), 0);
            assert_eq!(semctl(id, 0, GETVAL), 3);
            // Acquire (P).
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, p.as_ptr(), 1), 0);
            assert_eq!(semctl(id, 0, GETVAL), 2);
        });
    }

    #[test]
    fn test_semop_acquire_blocking_with_nowait_eagain() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: IPC_NOWAIT as i16,
            }];
            assert_eq!(semop(id, p.as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EAGAIN);
        });
    }

    #[test]
    fn test_semop_zero_op_succeeds_when_zero() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let z = [Sembuf {
                sem_num: 0,
                sem_op: 0,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, z.as_ptr(), 1), 0);
        });
    }

    #[test]
    fn test_semop_zero_op_blocks_when_nonzero_nowait() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 1,
                sem_flg: 0,
            }];
            let _ = semop(id, v.as_ptr(), 1);
            errno::set_errno(0);
            let z = [Sembuf {
                sem_num: 0,
                sem_op: 0,
                sem_flg: IPC_NOWAIT as i16,
            }];
            assert_eq!(semop(id, z.as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EAGAIN);
        });
    }

    #[test]
    fn test_semop_multi_op_atomic() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 2, IPC_CREAT | 0o600);
            // Pre-populate: sem0=5, sem1=0.
            let init = [Sembuf {
                sem_num: 0,
                sem_op: 5,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, init.as_ptr(), 1), 0);
            // Batch: acquire on sem0 (succeeds), then acquire on sem1
            // (would block).  Whole batch must roll back.
            let ops = [
                Sembuf {
                    sem_num: 0,
                    sem_op: -1,
                    sem_flg: IPC_NOWAIT as i16,
                },
                Sembuf {
                    sem_num: 1,
                    sem_op: -1,
                    sem_flg: IPC_NOWAIT as i16,
                },
            ];
            assert_eq!(semop(id, ops.as_ptr(), 2), -1);
            assert_eq!(errno::get_errno(), errno::EAGAIN);
            // sem0 must still be 5 (rollback worked).
            assert_eq!(semctl(id, 0, GETVAL), 5);
        });
    }

    // -----------------------------------------------------------------------
    // semop — error paths
    // -----------------------------------------------------------------------

    #[test]
    fn test_semop_null_sops_efault() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(semop(id, core::ptr::null(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    #[test]
    fn test_semop_zero_nsops_e2big() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 0,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, v.as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::E2BIG);
        });
    }

    #[test]
    fn test_semop_too_many_ops_e2big() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 0,
                sem_flg: 0,
            }];
            errno::set_errno(0);
            assert_eq!(semop(id, v.as_ptr(), MAX_SEMS_PER_SET + 1), -1);
            assert_eq!(errno::get_errno(), errno::E2BIG);
        });
    }

    #[test]
    fn test_semop_bad_semid_einval() {
        with_clean_table(|| {
            errno::set_errno(0);
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 0,
                sem_flg: 0,
            }];
            assert_eq!(semop(0xDEAD, v.as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semop_bad_sem_num_efbig() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let v = [Sembuf {
                sem_num: 5,
                sem_op: 1,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, v.as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::EFBIG);
        });
    }

    #[test]
    fn test_semop_overflow_semvmx_erange() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            // Push value to SEMVMX.
            let _ = semctl_setval(id, 0, SEMVMX as i32);
            errno::set_errno(0);
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 1,
                sem_flg: 0,
            }];
            assert_eq!(semop(id, v.as_ptr(), 1), -1);
            assert_eq!(errno::get_errno(), errno::ERANGE);
            // Value unchanged.
            assert_eq!(semctl(id, 0, GETVAL), SEMVMX as i32);
        });
    }

    // -----------------------------------------------------------------------
    // semtimedop
    // -----------------------------------------------------------------------

    #[test]
    fn test_semtimedop_past_deadline_eagain() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: 0,
            }];
            let past = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            errno::set_errno(0);
            assert_eq!(
                semtimedop(id, p.as_ptr(), 1, &raw const past as *const u8),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EAGAIN);
        });
    }

    #[test]
    fn test_semtimedop_immediate_success_no_timeout_check() {
        // If the op can succeed immediately, the timeout never matters
        // (even a past deadline shouldn't cause a failure).
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let _ = semctl_setval(id, 0, 5);
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: 0,
            }];
            let past = Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            assert_eq!(
                semtimedop(id, p.as_ptr(), 1, &raw const past as *const u8),
                0
            );
        });
    }

    #[test]
    fn test_semtimedop_null_timeout_blocks_forever_then_nowait() {
        // NULL timeout with NOWAIT in sembuf should still return EAGAIN
        // when blocked.
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: IPC_NOWAIT as i16,
            }];
            errno::set_errno(0);
            assert_eq!(semtimedop(id, p.as_ptr(), 1, core::ptr::null()), -1);
            assert_eq!(errno::get_errno(), errno::EAGAIN);
        });
    }

    #[test]
    fn test_semtimedop_bad_timespec_einval() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: 0,
            }];
            let bad = Timespec {
                tv_sec: 0,
                tv_nsec: -1,
            };
            errno::set_errno(0);
            assert_eq!(
                semtimedop(id, p.as_ptr(), 1, &raw const bad as *const u8),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
            let bad2 = Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            };
            errno::set_errno(0);
            assert_eq!(
                semtimedop(id, p.as_ptr(), 1, &raw const bad2 as *const u8),
                -1
            );
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    // -----------------------------------------------------------------------
    // semctl
    // -----------------------------------------------------------------------

    #[test]
    fn test_semctl_rmid_invalidates_id() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            assert_eq!(semctl(id, 0, IPC_RMID), 0);
            // Stale id should now fail.
            errno::set_errno(0);
            assert_eq!(semctl(id, 0, GETVAL), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semctl_rmid_frees_slot() {
        with_clean_table(|| {
            // Fill table.
            let mut ids = std::vec::Vec::new();
            for _ in 0..MAX_SETS {
                ids.push(semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600));
            }
            // RMID one — should free a slot.
            let id = ids[0];
            assert_eq!(semctl(id, 0, IPC_RMID), 0);
            // Allocate again — should succeed now.
            let new_id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            assert_ne!(new_id, -1);
            // And the old id should be different (different generation
            // tag, even if the same slot).
            assert_ne!(new_id, id);
        });
    }

    #[test]
    fn test_semctl_getval_after_setval() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 3, IPC_CREAT | 0o600);
            assert_eq!(semctl_setval(id, 1, 42), 0);
            assert_eq!(semctl(id, 1, GETVAL), 42);
            assert_eq!(semctl(id, 0, GETVAL), 0);
            assert_eq!(semctl(id, 2, GETVAL), 0);
        });
    }

    #[test]
    fn test_semctl_setval_out_of_range_erange() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(semctl_setval(id, 0, -1), -1);
            assert_eq!(errno::get_errno(), errno::ERANGE);
            errno::set_errno(0);
            assert_eq!(semctl_setval(id, 0, SEMVMX as i32 + 1), -1);
            assert_eq!(errno::get_errno(), errno::ERANGE);
        });
    }

    #[test]
    fn test_semctl_setall_getall() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 4, IPC_CREAT | 0o600);
            let init: [u16; 4] = [10, 20, 30, 40];
            let r = unsafe { semctl_setall(id, init.as_ptr()) };
            assert_eq!(r, 0);
            let mut out: [u16; 4] = [0; 4];
            let r = unsafe { semctl_getall(id, out.as_mut_ptr()) };
            assert_eq!(r, 0);
            assert_eq!(out, init);
        });
    }

    #[test]
    fn test_semctl_setall_value_out_of_range_erange() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 2, IPC_CREAT | 0o600);
            // Pre-populate so we can detect that no value was applied
            // before the bad one.
            let _ = semctl_setval(id, 0, 7);
            let bad: [u16; 2] = [5, (SEMVMX as u16) + 1];
            errno::set_errno(0);
            let r = unsafe { semctl_setall(id, bad.as_ptr()) };
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ERANGE);
            // Nothing changed.
            assert_eq!(semctl(id, 0, GETVAL), 7);
        });
    }

    #[test]
    fn test_semctl_bad_cmd_einval() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(semctl(id, 0, 9999), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semctl_bad_semnum_getval_einval() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 2, IPC_CREAT | 0o600);
            errno::set_errno(0);
            assert_eq!(semctl(id, 5, GETVAL), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
            errno::set_errno(0);
            assert_eq!(semctl(id, -1, GETVAL), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semctl_getpid_returns_zero() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            let _ = semctl_setval(id, 0, 3);
            // GETPID always returns 0 (no real pids).
            assert_eq!(semctl(id, 0, GETPID), 0);
            assert_eq!(semctl(id, 0, GETNCNT), 0);
            assert_eq!(semctl(id, 0, GETZCNT), 0);
        });
    }

    #[test]
    fn test_semctl_setval_null_check_via_bad_semid() {
        with_clean_table(|| {
            errno::set_errno(0);
            assert_eq!(semctl_setval(0xCAFE, 0, 1), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        });
    }

    #[test]
    fn test_semctl_getall_null_efault() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let r = unsafe { semctl_getall(id, core::ptr::null_mut()) };
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    #[test]
    fn test_semctl_setall_null_efault() {
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            errno::set_errno(0);
            let r = unsafe { semctl_setall(id, core::ptr::null()) };
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        });
    }

    // -----------------------------------------------------------------------
    // Workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_classic_mutex_workflow() {
        // Create a 1-semaphore set, initialize to 1 (binary mutex),
        // acquire, release, remove.
        with_clean_table(|| {
            let id = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
            assert_ne!(id, -1);
            assert_eq!(semctl_setval(id, 0, 1), 0);
            // Acquire.
            let p = [Sembuf {
                sem_num: 0,
                sem_op: -1,
                sem_flg: SEM_UNDO as i16,
            }];
            assert_eq!(semop(id, p.as_ptr(), 1), 0);
            assert_eq!(semctl(id, 0, GETVAL), 0);
            // Release.
            let v = [Sembuf {
                sem_num: 0,
                sem_op: 1,
                sem_flg: SEM_UNDO as i16,
            }];
            assert_eq!(semop(id, v.as_ptr(), 1), 0);
            assert_eq!(semctl(id, 0, GETVAL), 1);
            // Remove.
            assert_eq!(semctl(id, 0, IPC_RMID), 0);
        });
    }
}
