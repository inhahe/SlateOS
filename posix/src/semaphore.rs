//! POSIX semaphore implementation.
//!
//! Implements both unnamed semaphores (`sem_init`) and named semaphores
//! (`sem_open`/`sem_close`/`sem_unlink`, backed by a process-local pool).
//!
//! ## Implementation
//!
//! A semaphore is a single 32-bit atomic counter: positive means the
//! resource is available, zero means callers must block.  The counter
//! doubles as a kernel **futex word** — `sem_wait` blocks via
//! `SYS_FUTEX_WAIT` (no CPU spin) and `sem_post` wakes a waiter via
//! `SYS_FUTEX_WAKE`.  The uncontended fast path is a pure userspace CAS
//! with no syscall.  `sem_timedwait` uses `SYS_FUTEX_WAIT_TIMEOUT`.
//!
//! On the host build (unit tests) there is no kernel futex, so the
//! blocking helpers fall back to a cooperative `spin_loop`; the test
//! suite only exercises the non-blocking paths (CAS success, trywait,
//! null-pointer validation), so this fallback is never hit in practice.
//!
//! Functions: `sem_init`, `sem_destroy`, `sem_wait`, `sem_trywait`,
//! `sem_timedwait`, `sem_post`, `sem_getvalue`, `sem_open`,
//! `sem_close`, `sem_unlink`.
//!
//! LIMITATION: named semaphores are stored in a per-process static pool,
//! so two *different* processes that `sem_open` the same name get
//! independent counters (no cross-process sharing).  True cross-process
//! named semaphores need the pool to live in shared memory keyed by name;
//! tracked in `todo.txt`.

use crate::errno;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// POSIX semaphore.
///
/// We use a simple i32 counter.  Positive means the resource is
/// available; zero or negative means waiters are blocked.
#[repr(C)]
pub struct SemT {
    /// Semaphore value (atomic).
    value: core::sync::atomic::AtomicI32,
}

/// Failed return value for sem_open.
pub const SEM_FAILED: *mut SemT = core::ptr::null_mut();

// ---------------------------------------------------------------------------
// Unnamed semaphores
// ---------------------------------------------------------------------------

/// Initialize an unnamed semaphore.
///
/// `pshared` is ignored (cross-process semaphores not supported).
/// `value` is the initial semaphore count.
///
/// Returns 0 on success, -1 on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_init(sem: *mut SemT, _pshared: i32, value: u32) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Guard against u32 values that would wrap to negative when cast
    // to i32.  Our SEM_VALUE_MAX is i32::MAX.
    if value > i32::MAX as u32 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // SAFETY: Caller guarantees sem is valid and writable.
    unsafe {
        core::ptr::addr_of_mut!((*sem).value)
            .write(core::sync::atomic::AtomicI32::new(value as i32));
    }

    0
}

/// Destroy an unnamed semaphore.
///
/// No-op in our implementation (no resources to free).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_destroy(_sem: *mut SemT) -> i32 {
    0
}

/// Reinterpret the signed counter as the kernel's unsigned 32-bit futex
/// word (a bit-for-bit reinterpret, not a value conversion — avoids a
/// sign-loss cast).
#[cfg(target_os = "none")]
fn futex_word(expected: i32) -> u64 {
    u64::from(u32::from_ne_bytes(expected.to_ne_bytes()))
}

/// Block the calling task until the semaphore value is observed to be
/// non-zero.  `expected` is the value the caller just read as `<= 0`;
/// the kernel only blocks if `*word` still equals it, closing the
/// wait/wake race (a poster that increments and wakes between our load
/// and this call makes `*word != expected`, so we return immediately and
/// re-check).
#[cfg(target_os = "none")]
fn sem_block(atomic: &core::sync::atomic::AtomicI32, expected: i32) {
    let addr = atomic.as_ptr() as u64;
    // SYS_FUTEX_WAIT returns 1 (woken), 0 (value mismatch), or a negative
    // error.  In every case we simply re-loop and re-evaluate the counter,
    // so the return value is intentionally ignored.
    let _ = crate::syscall::syscall2(crate::syscall::SYS_FUTEX_WAIT, addr, futex_word(expected));
}

/// Host fallback: no kernel futex in the unit-test environment.  The test
/// suite never blocks (see module docs), so a cooperative spin is fine.
#[cfg(not(target_os = "none"))]
fn sem_block(_atomic: &core::sync::atomic::AtomicI32, _expected: i32) {
    core::hint::spin_loop();
}

/// Like [`sem_block`] but bounded: block for at most `timeout_ns`
/// nanoseconds via `SYS_FUTEX_WAIT_TIMEOUT`.
#[cfg(target_os = "none")]
fn sem_block_timeout(atomic: &core::sync::atomic::AtomicI32, expected: i32, timeout_ns: u64) {
    let addr = atomic.as_ptr() as u64;
    let _ = crate::syscall::syscall3(
        crate::syscall::SYS_FUTEX_WAIT_TIMEOUT,
        addr,
        futex_word(expected),
        timeout_ns,
    );
}

/// Host fallback for the bounded wait.
#[cfg(not(target_os = "none"))]
fn sem_block_timeout(_atomic: &core::sync::atomic::AtomicI32, _expected: i32, _timeout_ns: u64) {
    core::hint::spin_loop();
}

/// Wake one task blocked on the semaphore's futex word after a post.
#[cfg(target_os = "none")]
fn sem_wake_one(atomic: &core::sync::atomic::AtomicI32) {
    let addr = atomic.as_ptr() as u64;
    // Wake at most one waiter; a no-op (returns 0) if none are blocked.
    let _ = crate::syscall::syscall2(crate::syscall::SYS_FUTEX_WAKE, addr, 1);
}

/// Host fallback: no futex, nothing to wake.
#[cfg(not(target_os = "none"))]
fn sem_wake_one(_atomic: &core::sync::atomic::AtomicI32) {}

/// Lock (decrement) a semaphore, blocking if the value is zero.
///
/// The uncontended path is a pure userspace CAS.  When the count is
/// exhausted the caller blocks in the kernel via `SYS_FUTEX_WAIT` rather
/// than spinning, so a blocked waiter consumes no CPU.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_wait(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    loop {
        let current = atomic.load(core::sync::atomic::Ordering::Acquire);
        if current > 0 {
            if atomic
                .compare_exchange_weak(
                    current,
                    current.wrapping_sub(1),
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
            {
                return 0;
            }
            // CAS lost a race; retry immediately without blocking.
            continue;
        }
        // Count exhausted: block until a poster wakes us, then re-check.
        sem_block(atomic, current);
    }
}

/// Try to lock a semaphore without blocking.
///
/// Returns 0 if the semaphore was decremented, -1 with EAGAIN if
/// the semaphore is already zero.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_trywait(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    // Retry loop: a failed CAS doesn't mean value is zero — another
    // thread may have concurrently modified it.  Only give up when the
    // value is genuinely non-positive.
    loop {
        let current = atomic.load(core::sync::atomic::Ordering::Acquire);
        if current <= 0 {
            errno::set_errno(errno::EAGAIN);
            return -1;
        }
        if atomic
            .compare_exchange_weak(
                current,
                current.wrapping_sub(1),
                core::sync::atomic::Ordering::AcqRel,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            return 0;
        }
        // CAS failed — value changed. Retry with fresh load.
    }
}

/// Unlock (increment) a semaphore.
///
/// If threads are waiting, one will be woken.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_post(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    // POSIX: return EOVERFLOW if incrementing would exceed SEM_VALUE_MAX.
    // Without this check, wrapping past i32::MAX produces a negative value,
    // which sem_wait interprets as "no resources", causing deadlock.
    loop {
        let current = atomic.load(core::sync::atomic::Ordering::Relaxed);
        if current == i32::MAX {
            errno::set_errno(errno::EOVERFLOW);
            return -1;
        }
        if atomic
            .compare_exchange_weak(
                current,
                current.wrapping_add(1),
                core::sync::atomic::Ordering::Release,
                core::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
        {
            // A resource became available; wake one blocked waiter (if any).
            sem_wake_one(atomic);
            return 0;
        }
    }
}

/// Lock a semaphore with a timeout.
///
/// Like `sem_wait` but returns `ETIMEDOUT` if the absolute time
/// `abstime` passes before the semaphore can be decremented.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_timedwait(sem: *mut SemT, abstime: *const crate::stat::Timespec) -> i32 {
    if sem.is_null() || abstime.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };

    loop {
        // Try to decrement.
        let current = atomic.load(core::sync::atomic::Ordering::Acquire);
        if current > 0 {
            if atomic
                .compare_exchange_weak(
                    current,
                    current.wrapping_sub(1),
                    core::sync::atomic::Ordering::AcqRel,
                    core::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
            {
                return 0;
            }
            // CAS lost a race; retry immediately.
            continue;
        }

        // Count exhausted: compute the time remaining until the deadline.
        let mut now = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let _ = crate::time::clock_gettime(crate::time::CLOCK_REALTIME, &raw mut now);
        let deadline = unsafe { &*abstime };
        if now.tv_sec > deadline.tv_sec
            || (now.tv_sec == deadline.tv_sec && now.tv_nsec >= deadline.tv_nsec)
        {
            errno::set_errno(errno::ETIMEDOUT);
            return -1;
        }

        // Remaining nanoseconds = deadline - now.  Use i128 + saturating
        // arithmetic so a malformed (huge) deadline can't overflow or
        // panic; we already know now < deadline, so the result is > 0.
        let secs = deadline.tv_sec.saturating_sub(now.tv_sec);
        let total_ns = i128::from(secs)
            .saturating_mul(1_000_000_000)
            .saturating_add(i128::from(deadline.tv_nsec))
            .saturating_sub(i128::from(now.tv_nsec));
        let timeout_ns = u64::try_from(total_ns).unwrap_or(0);

        // Block (bounded) until a poster wakes us or the timeout elapses,
        // then re-loop to re-check the counter and the deadline.
        sem_block_timeout(atomic, current, timeout_ns);
    }
}

/// Get the current value of a semaphore.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_getvalue(sem: *mut SemT, sval: *mut i32) -> i32 {
    if sem.is_null() || sval.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let atomic = unsafe { &(*sem).value };
    let val = atomic.load(core::sync::atomic::Ordering::Relaxed);
    unsafe {
        *sval = val;
    }
    0
}

// ---------------------------------------------------------------------------
// Named semaphores
// ---------------------------------------------------------------------------
//
// Named semaphores live in a static pool, identified by a leading-slash
// name (POSIX convention).  `sem_open` either looks up an existing slot
// or, with `O_CREAT`, allocates and initialises a new one.  The pointer
// returned to userspace is the address of the slot's `SemT` field, so
// `sem_wait`/`sem_post`/`sem_getvalue` work unchanged.
//
// A separate refcount tracks how many open handles reference each slot;
// `sem_close` decrements it and `sem_unlink` marks the slot for removal.
// A slot is reclaimed only after it has been unlinked *and* its
// refcount drops to zero, matching POSIX unlink-while-open semantics.

/// Maximum number of distinct named semaphores live at once.
const MAX_NAMED_SEMS: usize = 16;

/// Maximum length of a semaphore name (including the leading `/`).
const MAX_SEM_NAME: usize = 64;

#[repr(C)]
struct NamedSem {
    in_use: bool,
    unlinked: bool,
    name: [u8; MAX_SEM_NAME],
    name_len: usize,
    refcount: u32,
    sem: SemT,
}

// SAFETY: the table is only mutated under `SEM_LOCK`; readers also hold
// the lock.  `NamedSem` itself contains `AtomicI32` for the value, so
// once the lock is dropped concurrent `sem_wait`/`sem_post` are safe.
static SEM_LOCK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

static mut NAMED_SEMS: [NamedSem; MAX_NAMED_SEMS] = [const {
    NamedSem {
        in_use: false,
        unlinked: false,
        name: [0u8; MAX_SEM_NAME],
        name_len: 0,
        refcount: 0,
        sem: SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        },
    }
}; MAX_NAMED_SEMS];

/// RAII guard that releases `SEM_LOCK` on drop.
struct SemLockGuard;

impl Drop for SemLockGuard {
    fn drop(&mut self) {
        SEM_LOCK.store(false, core::sync::atomic::Ordering::Release);
    }
}

fn acquire_sem_lock() -> SemLockGuard {
    while SEM_LOCK
        .compare_exchange_weak(
            false,
            true,
            core::sync::atomic::Ordering::Acquire,
            core::sync::atomic::Ordering::Relaxed,
        )
        .is_err()
    {
        core::hint::spin_loop();
    }
    SemLockGuard
}

/// Validate a POSIX semaphore name: starts with `/`, no further `/`,
/// fits in `MAX_SEM_NAME` bytes.  Returns the name length on success.
fn validate_sem_name(name: *const u8) -> Result<usize, i32> {
    if name.is_null() {
        return Err(errno::EFAULT);
    }
    let mut len: usize = 0;
    while len <= MAX_SEM_NAME {
        // SAFETY: caller contract — `name` is NUL-terminated.
        let b = unsafe { *name.add(len) };
        if b == 0 {
            break;
        }
        len = len.wrapping_add(1);
    }
    if len == 0 || len > MAX_SEM_NAME {
        return Err(errno::EINVAL);
    }
    // SAFETY: bounded above.
    let first = unsafe { *name };
    if first != b'/' {
        return Err(errno::EINVAL);
    }
    let mut i: usize = 1;
    while i < len {
        // SAFETY: i < len.
        let b = unsafe { *name.add(i) };
        if b == b'/' {
            return Err(errno::EINVAL);
        }
        i = i.wrapping_add(1);
    }
    Ok(len)
}

/// Find the slot index whose name matches `name[..len]`, considering
/// only live (not-yet-fully-reclaimed) entries.  Returns `None` if no
/// match.  Caller must hold `SEM_LOCK`.
fn find_named_sem(name: *const u8, len: usize) -> Option<usize> {
    for i in 0..MAX_NAMED_SEMS {
        // SAFETY: SEM_LOCK is held.
        let slot = unsafe { &*core::ptr::addr_of!(NAMED_SEMS[i]) };
        if !slot.in_use || slot.unlinked {
            continue;
        }
        if slot.name_len != len {
            continue;
        }
        // SAFETY: caller-provided name has at least `len` bytes.
        let in_name = unsafe { core::slice::from_raw_parts(name, len) };
        let Some(stored) = slot.name.get(..len) else { continue };
        if stored == in_name {
            return Some(i);
        }
    }
    None
}

/// Find a free slot index.  Caller must hold `SEM_LOCK`.
fn find_free_sem_slot() -> Option<usize> {
    for i in 0..MAX_NAMED_SEMS {
        // SAFETY: SEM_LOCK is held.
        let slot = unsafe { &*core::ptr::addr_of!(NAMED_SEMS[i]) };
        if !slot.in_use {
            return Some(i);
        }
    }
    None
}

/// Find the slot that owns the given `SemT*`.  Caller must hold
/// `SEM_LOCK`.  Returns `None` if the pointer isn't a named-semaphore
/// address (i.e. it's an unnamed semaphore).
fn slot_for_ptr(sem: *mut SemT) -> Option<usize> {
    for i in 0..MAX_NAMED_SEMS {
        // SAFETY: SEM_LOCK is held; stable addresses in `static mut`.
        let p = unsafe { core::ptr::addr_of!(NAMED_SEMS[i].sem) }.cast_mut();
        if p == sem {
            return Some(i);
        }
    }
    None
}

/// Open a named semaphore.
///
/// Two-arg form (`sem_open(name, oflag)`) looks up an existing
/// semaphore.  Four-arg form with `O_CREAT` set in `oflag` creates the
/// semaphore if it doesn't exist, initialising its value to `value`.
/// `O_CREAT | O_EXCL` fails with `EEXIST` if the semaphore already
/// exists.  `mode` is accepted but unused (we have no per-object
/// permission model yet).
///
/// On x86_64 SysV ABI the variadic `mode`/`value` args are passed in
/// registers regardless of caller-side declaration, so a 2-arg caller
/// leaves them undefined: we read them only when `O_CREAT` is set, at
/// which point the caller is contractually required to supply them.
///
/// # Errors
///
/// - `EFAULT` — `name` is NULL.
/// - `EINVAL` — name doesn't start with `/`, contains internal `/`, is
///   empty, or exceeds `MAX_SEM_NAME` bytes; or `value` exceeds
///   `i32::MAX`.
/// - `EEXIST` — `O_CREAT | O_EXCL` and the name already exists.
/// - `ENOENT` — name doesn't exist and `O_CREAT` is not set.
/// - `ENOSPC` — the named-sem pool is exhausted.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_open(name: *const u8, oflag: i32, mode: u32, value: u32) -> *mut SemT {
    let _ = mode;
    let name_len = match validate_sem_name(name) {
        Ok(n) => n,
        Err(e) => {
            errno::set_errno(e);
            return SEM_FAILED;
        }
    };

    let _guard = acquire_sem_lock();

    let existing = find_named_sem(name, name_len);
    let create = oflag & crate::fcntl::O_CREAT != 0;
    let exclusive = oflag & crate::fcntl::O_EXCL != 0;

    if let Some(idx) = existing {
        if create && exclusive {
            errno::set_errno(errno::EEXIST);
            return SEM_FAILED;
        }
        // Bump refcount; return slot's SemT pointer.
        // SAFETY: SEM_LOCK is held.
        unsafe {
            NAMED_SEMS[idx].refcount = NAMED_SEMS[idx].refcount.wrapping_add(1);
        }
        // SAFETY: stable static address.
        return unsafe { core::ptr::addr_of_mut!(NAMED_SEMS[idx].sem) };
    }
    // Not found.
    if !create {
        errno::set_errno(errno::ENOENT);
        return SEM_FAILED;
    }
    if value > i32::MAX as u32 {
        errno::set_errno(errno::EINVAL);
        return SEM_FAILED;
    }
    let Some(idx) = find_free_sem_slot() else {
        errno::set_errno(errno::ENOSPC);
        return SEM_FAILED;
    };
    // SAFETY: SEM_LOCK is held; idx is in bounds.
    unsafe {
        let slot = &raw mut NAMED_SEMS[idx];
        (*slot).in_use = true;
        (*slot).unlinked = false;
        (*slot).name_len = name_len;
        (*slot).refcount = 1;
        let name_dst: *mut u8 = core::ptr::addr_of_mut!((*slot).name).cast::<u8>();
        // Copy name bytes.
        for j in 0..name_len {
            *name_dst.add(j) = *name.add(j);
        }
        // Zero-fill the rest of the name buffer for tidiness.
        for j in name_len..MAX_SEM_NAME {
            *name_dst.add(j) = 0;
        }
        (*slot)
            .sem
            .value
            .store(value as i32, core::sync::atomic::Ordering::Relaxed);
        core::ptr::addr_of_mut!((*slot).sem)
    }
}

/// Close a named semaphore handle.
///
/// Decrements the slot's refcount.  If the slot was previously
/// `sem_unlink`ed and the refcount reaches zero, the slot is reclaimed.
///
/// # Errors
///
/// - `EFAULT` — `sem` is NULL.
/// - `EINVAL` — `sem` doesn't refer to a known named semaphore.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_close(sem: *mut SemT) -> i32 {
    if sem.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let _guard = acquire_sem_lock();
    let Some(idx) = slot_for_ptr(sem) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    // SAFETY: SEM_LOCK is held; idx is in bounds.
    unsafe {
        if NAMED_SEMS[idx].refcount > 0 {
            NAMED_SEMS[idx].refcount = NAMED_SEMS[idx].refcount.wrapping_sub(1);
        }
        if NAMED_SEMS[idx].unlinked && NAMED_SEMS[idx].refcount == 0 {
            // Reclaim.
            NAMED_SEMS[idx].in_use = false;
            NAMED_SEMS[idx].unlinked = false;
            NAMED_SEMS[idx].name_len = 0;
        }
    }
    0
}

/// Remove a named semaphore from the namespace.
///
/// The slot is marked unlinked but stays alive until all open handles
/// are closed, at which point it is reclaimed.  A subsequent
/// `sem_open` with the same name and `O_CREAT` creates a fresh
/// semaphore unrelated to the unlinked one.
///
/// # Errors
///
/// - `EFAULT` — `name` is NULL.
/// - `EINVAL` — invalid name format.
/// - `ENOENT` — no semaphore with this name exists.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn sem_unlink(name: *const u8) -> i32 {
    let name_len = match validate_sem_name(name) {
        Ok(n) => n,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    let _guard = acquire_sem_lock();
    let Some(idx) = find_named_sem(name, name_len) else {
        errno::set_errno(errno::ENOENT);
        return -1;
    };
    // SAFETY: SEM_LOCK is held; idx is in bounds.
    unsafe {
        NAMED_SEMS[idx].unlinked = true;
        if NAMED_SEMS[idx].refcount == 0 {
            NAMED_SEMS[idx].in_use = false;
            NAMED_SEMS[idx].unlinked = false;
            NAMED_SEMS[idx].name_len = 0;
        }
    }
    0
}

/// Reset the named-semaphore table.  Test-only.
#[cfg(test)]
fn reset_named_sems() {
    let _guard = acquire_sem_lock();
    for i in 0..MAX_NAMED_SEMS {
        // SAFETY: SEM_LOCK is held.
        unsafe {
            NAMED_SEMS[i].in_use = false;
            NAMED_SEMS[i].unlinked = false;
            NAMED_SEMS[i].name_len = 0;
            NAMED_SEMS[i].refcount = 0;
            NAMED_SEMS[i]
                .sem
                .value
                .store(0, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- sem_init --

    #[test]
    fn test_sem_init_zero() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(-1),
        };
        let ret = sem_init(&raw mut sem, 0, 0);
        assert_eq!(ret, 0);
        let mut val: i32 = -1;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 0);
    }

    #[test]
    fn test_sem_init_positive() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        let ret = sem_init(&raw mut sem, 0, 5);
        assert_eq!(ret, 0);
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 5);
    }

    #[test]
    fn test_sem_init_max_valid() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        let ret = sem_init(&raw mut sem, 0, i32::MAX as u32);
        assert_eq!(ret, 0);
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, i32::MAX);
    }

    #[test]
    fn test_sem_init_overflow_rejected() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        // i32::MAX + 1 = 2147483648 — should be rejected
        let ret = sem_init(&raw mut sem, 0, (i32::MAX as u32).wrapping_add(1));
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sem_init_null() {
        let ret = sem_init(core::ptr::null_mut(), 0, 1);
        assert_eq!(ret, -1);
    }

    // -- sem_destroy --

    #[test]
    fn test_sem_destroy_succeeds() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(5),
        };
        assert_eq!(sem_destroy(&raw mut sem), 0);
    }

    // -- sem_post --

    #[test]
    fn test_sem_post_increments() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        let ret = sem_post(&raw mut sem);
        assert_eq!(ret, 0);

        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 1);
    }

    #[test]
    fn test_sem_post_multiple() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        for _ in 0..10 {
            sem_post(&raw mut sem);
        }

        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 10);
    }

    #[test]
    fn test_sem_post_overflow_rejected() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(i32::MAX),
        };
        let ret = sem_post(&raw mut sem);
        assert_eq!(ret, -1);
        // Value should not have changed
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, i32::MAX);
    }

    #[test]
    fn test_sem_post_null() {
        let ret = sem_post(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -- sem_trywait --

    #[test]
    fn test_sem_trywait_decrements() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 3);

        let ret = sem_trywait(&raw mut sem);
        assert_eq!(ret, 0);

        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 2);
    }

    #[test]
    fn test_sem_trywait_fails_at_zero() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        let ret = sem_trywait(&raw mut sem);
        assert_eq!(ret, -1); // EAGAIN
    }

    #[test]
    fn test_sem_trywait_null() {
        let ret = sem_trywait(core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sem_trywait_drain() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 3);

        assert_eq!(sem_trywait(&raw mut sem), 0);
        assert_eq!(sem_trywait(&raw mut sem), 0);
        assert_eq!(sem_trywait(&raw mut sem), 0);
        // Now zero — should fail
        assert_eq!(sem_trywait(&raw mut sem), -1);
    }

    // -- sem_getvalue --

    #[test]
    fn test_sem_getvalue_basic() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 42);

        let mut val: i32 = 0;
        let ret = sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(ret, 0);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_sem_getvalue_null_sem() {
        let mut val: i32 = 0;
        let ret = sem_getvalue(core::ptr::null_mut(), &raw mut val);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_sem_getvalue_null_sval() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(5),
        };
        let ret = sem_getvalue(&raw mut sem, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -- post/trywait round trip --

    #[test]
    fn test_sem_post_trywait_round_trip() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        // Can't trywait on empty
        assert_eq!(sem_trywait(&raw mut sem), -1);

        // Post once
        assert_eq!(sem_post(&raw mut sem), 0);

        // Now can trywait
        assert_eq!(sem_trywait(&raw mut sem), 0);

        // Empty again
        assert_eq!(sem_trywait(&raw mut sem), -1);
    }

    // -- Named semaphores --

    /// Serialise tests that touch the shared NAMED_SEMS pool.
    static NAMED_SEM_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_named_sem_lock<F: FnOnce()>(f: F) {
        let _guard = NAMED_SEM_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reset_named_sems();
        f();
        reset_named_sems();
    }

    #[test]
    fn test_sem_open_missing_no_create_enoent() {
        with_named_sem_lock(|| {
            crate::errno::set_errno(0);
            let p = sem_open(b"/missing\0".as_ptr(), 0, 0, 0);
            assert_eq!(p, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
        });
    }

    #[test]
    fn test_sem_open_creates_and_returns_valid() {
        with_named_sem_lock(|| {
            let p = sem_open(b"/a\0".as_ptr(), crate::fcntl::O_CREAT, 0o600, 3);
            assert!(!p.is_null());
            // Verify the value via sem_getvalue.
            let mut v: i32 = -1;
            assert_eq!(sem_getvalue(p, &raw mut v), 0);
            assert_eq!(v, 3);
            assert_eq!(sem_close(p), 0);
        });
    }

    #[test]
    fn test_sem_open_excl_existing_eexist() {
        with_named_sem_lock(|| {
            let p1 = sem_open(b"/excl\0".as_ptr(), crate::fcntl::O_CREAT, 0, 1);
            assert!(!p1.is_null());
            crate::errno::set_errno(0);
            let p2 = sem_open(
                b"/excl\0".as_ptr(),
                crate::fcntl::O_CREAT | crate::fcntl::O_EXCL,
                0,
                1,
            );
            assert_eq!(p2, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::EEXIST);
            assert_eq!(sem_close(p1), 0);
        });
    }

    #[test]
    fn test_sem_open_same_name_returns_same_pointer() {
        with_named_sem_lock(|| {
            let p1 = sem_open(b"/share\0".as_ptr(), crate::fcntl::O_CREAT, 0, 0);
            let p2 = sem_open(b"/share\0".as_ptr(), 0, 0, 0);
            assert!(!p1.is_null());
            assert_eq!(p1, p2);
            // Cross-handle visibility: post via p1, see via p2.
            assert_eq!(sem_post(p1), 0);
            let mut v: i32 = -1;
            assert_eq!(sem_getvalue(p2, &raw mut v), 0);
            assert_eq!(v, 1);
            assert_eq!(sem_close(p1), 0);
            assert_eq!(sem_close(p2), 0);
        });
    }

    #[test]
    fn test_sem_open_invalid_name() {
        with_named_sem_lock(|| {
            // No leading '/'.
            crate::errno::set_errno(0);
            let p = sem_open(b"bad\0".as_ptr(), crate::fcntl::O_CREAT, 0, 0);
            assert_eq!(p, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            // Embedded '/'.
            let p = sem_open(b"/a/b\0".as_ptr(), crate::fcntl::O_CREAT, 0, 0);
            assert_eq!(p, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
            // Empty.
            let p = sem_open(b"\0".as_ptr(), crate::fcntl::O_CREAT, 0, 0);
            assert_eq!(p, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        });
    }

    #[test]
    fn test_sem_open_null_name_efault() {
        with_named_sem_lock(|| {
            crate::errno::set_errno(0);
            let p = sem_open(core::ptr::null(), crate::fcntl::O_CREAT, 0, 0);
            assert_eq!(p, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
        });
    }

    #[test]
    fn test_sem_open_value_overflow_einval() {
        with_named_sem_lock(|| {
            crate::errno::set_errno(0);
            let p = sem_open(
                b"/big\0".as_ptr(),
                crate::fcntl::O_CREAT,
                0,
                (i32::MAX as u32).wrapping_add(1),
            );
            assert_eq!(p, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        });
    }

    #[test]
    fn test_sem_open_pool_exhaustion_enospc() {
        with_named_sem_lock(|| {
            // Open MAX_NAMED_SEMS distinct semaphores.
            let names: [&[u8]; 16] = [
                b"/s00\0", b"/s01\0", b"/s02\0", b"/s03\0", b"/s04\0", b"/s05\0", b"/s06\0",
                b"/s07\0", b"/s08\0", b"/s09\0", b"/s10\0", b"/s11\0", b"/s12\0", b"/s13\0",
                b"/s14\0", b"/s15\0",
            ];
            let mut ptrs = [SEM_FAILED; 16];
            for (i, n) in names.iter().enumerate() {
                ptrs[i] = sem_open(n.as_ptr(), crate::fcntl::O_CREAT, 0, 0);
                assert!(!ptrs[i].is_null(), "open {i} should succeed");
            }
            crate::errno::set_errno(0);
            let extra = sem_open(b"/overflow\0".as_ptr(), crate::fcntl::O_CREAT, 0, 0);
            assert_eq!(extra, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOSPC);
            for p in &ptrs {
                let _ = sem_close(*p);
            }
        });
    }

    #[test]
    fn test_sem_unlink_basic_then_open_fresh() {
        with_named_sem_lock(|| {
            let p1 = sem_open(b"/u\0".as_ptr(), crate::fcntl::O_CREAT, 0, 5);
            assert!(!p1.is_null());
            assert_eq!(sem_close(p1), 0);
            // No open handles, so unlink should free the slot immediately.
            assert_eq!(sem_unlink(b"/u\0".as_ptr()), 0);
            // Opening the same name with O_CREAT now creates a new slot.
            let p2 = sem_open(b"/u\0".as_ptr(), crate::fcntl::O_CREAT, 0, 7);
            assert!(!p2.is_null());
            let mut v: i32 = -1;
            sem_getvalue(p2, &raw mut v);
            assert_eq!(v, 7);
            assert_eq!(sem_close(p2), 0);
            assert_eq!(sem_unlink(b"/u\0".as_ptr()), 0);
        });
    }

    #[test]
    fn test_sem_unlink_while_open_delays_reclaim() {
        with_named_sem_lock(|| {
            let p1 = sem_open(b"/d\0".as_ptr(), crate::fcntl::O_CREAT, 0, 1);
            assert!(!p1.is_null());
            // Unlink while still open — slot stays alive.
            assert_eq!(sem_unlink(b"/d\0".as_ptr()), 0);
            // sem_open(no O_CREAT) for the same name now returns ENOENT
            // because the slot has been marked unlinked.
            crate::errno::set_errno(0);
            let p2 = sem_open(b"/d\0".as_ptr(), 0, 0, 0);
            assert_eq!(p2, SEM_FAILED);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
            // p1 is still usable.
            let mut v: i32 = -1;
            assert_eq!(sem_getvalue(p1, &raw mut v), 0);
            assert_eq!(v, 1);
            // Closing p1 finishes the reclaim.
            assert_eq!(sem_close(p1), 0);
        });
    }

    #[test]
    fn test_sem_unlink_missing_enoent() {
        with_named_sem_lock(|| {
            crate::errno::set_errno(0);
            let r = sem_unlink(b"/nope\0".as_ptr());
            assert_eq!(r, -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::ENOENT);
        });
    }

    #[test]
    fn test_sem_close_null_efault() {
        crate::errno::set_errno(0);
        assert_eq!(sem_close(core::ptr::null_mut()), -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sem_close_unknown_pointer_einval() {
        // A pointer to an unnamed semaphore on the stack is not a named-
        // semaphore slot; closing it should EINVAL rather than corrupt
        // anything.
        with_named_sem_lock(|| {
            let mut sem = SemT {
                value: core::sync::atomic::AtomicI32::new(0),
            };
            crate::errno::set_errno(0);
            assert_eq!(sem_close(&raw mut sem), -1);
            assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        });
    }

    #[test]
    fn test_sem_open_refcount_close_balance() {
        with_named_sem_lock(|| {
            let p1 = sem_open(b"/rc\0".as_ptr(), crate::fcntl::O_CREAT, 0, 0);
            let p2 = sem_open(b"/rc\0".as_ptr(), 0, 0, 0);
            let p3 = sem_open(b"/rc\0".as_ptr(), 0, 0, 0);
            assert_eq!(p1, p2);
            assert_eq!(p2, p3);
            assert_eq!(sem_close(p1), 0);
            assert_eq!(sem_close(p2), 0);
            // Slot must still be live for p3 (and unlink).
            assert_eq!(sem_unlink(b"/rc\0".as_ptr()), 0);
            // p3 still usable.
            let mut v: i32 = -1;
            assert_eq!(sem_getvalue(p3, &raw mut v), 0);
            assert_eq!(sem_close(p3), 0);
        });
    }

    // -- SemT layout --

    #[test]
    fn test_sem_size() {
        // AtomicI32 = 4 bytes
        assert_eq!(core::mem::size_of::<SemT>(), 4);
    }

    #[test]
    fn test_sem_alignment() {
        assert_eq!(core::mem::align_of::<SemT>(), 4);
    }

    // -- sem_init sets errno for null --

    #[test]
    fn test_sem_init_null_sets_efault() {
        crate::errno::set_errno(0);
        let ret = sem_init(core::ptr::null_mut(), 0, 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sem_init_overflow_sets_einval() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        let ret = sem_init(&raw mut sem, 0, u32::MAX);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- sem_wait null sets errno --

    #[test]
    fn test_sem_wait_null_efault() {
        crate::errno::set_errno(0);
        let ret = sem_wait(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sem_wait_fast_path_decrements() {
        // When the count is positive sem_wait takes the userspace CAS fast
        // path and never reaches the (host-stubbed) blocking helper.  Two
        // waits on a count-of-2 semaphore should both succeed and drain it.
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(2),
        };
        assert_eq!(sem_wait(&raw mut sem), 0);
        assert_eq!(sem_wait(&raw mut sem), 0);
        let mut v: i32 = -1;
        assert_eq!(sem_getvalue(&raw mut sem, &raw mut v), 0);
        assert_eq!(v, 0);
    }

    // -- sem_trywait sets EAGAIN --

    #[test]
    fn test_sem_trywait_zero_sets_eagain() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);
        let ret = sem_trywait(&raw mut sem);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EAGAIN);
    }

    #[test]
    fn test_sem_trywait_null_sets_efault() {
        crate::errno::set_errno(0);
        let ret = sem_trywait(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- sem_post overflow sets EOVERFLOW --

    #[test]
    fn test_sem_post_overflow_sets_eoverflow() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(i32::MAX),
        };
        let ret = sem_post(&raw mut sem);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EOVERFLOW);
    }

    #[test]
    fn test_sem_post_null_sets_efault() {
        crate::errno::set_errno(0);
        let ret = sem_post(core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // (Named-semaphore implementations no longer ENOSYS — see the
    // dedicated section above for full coverage.)

    // -- sem_timedwait null checks --

    #[test]
    fn test_sem_timedwait_null_sem() {
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = sem_timedwait(core::ptr::null_mut(), &raw const ts);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    #[test]
    fn test_sem_timedwait_null_abstime() {
        crate::errno::set_errno(0);
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);
        let ret = sem_timedwait(&raw mut sem, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    // -- sem_init with pshared (ignored but accepted) --

    #[test]
    fn test_sem_init_pshared_nonzero() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        // pshared=1 should still succeed (we ignore it)
        let ret = sem_init(&raw mut sem, 1, 10);
        assert_eq!(ret, 0);
        let mut val: i32 = 0;
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 10);
    }

    // -- SEM_FAILED constant --

    #[test]
    fn test_sem_failed_is_null() {
        assert!(SEM_FAILED.is_null());
    }

    // -- Multiple post/trywait cycles --

    #[test]
    fn test_sem_multiple_cycles() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 0);

        for _ in 0..5 {
            assert_eq!(sem_post(&raw mut sem), 0);
            assert_eq!(sem_post(&raw mut sem), 0);
            assert_eq!(sem_trywait(&raw mut sem), 0);
            assert_eq!(sem_trywait(&raw mut sem), 0);
            assert_eq!(sem_trywait(&raw mut sem), -1); // empty
        }
    }

    // -- sem_getvalue after operations --

    #[test]
    fn test_sem_getvalue_tracks_operations() {
        let mut sem = SemT {
            value: core::sync::atomic::AtomicI32::new(0),
        };
        sem_init(&raw mut sem, 0, 5);
        let mut val: i32 = 0;

        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 5);

        sem_trywait(&raw mut sem);
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 4);

        sem_post(&raw mut sem);
        sem_post(&raw mut sem);
        sem_getvalue(&raw mut sem, &raw mut val);
        assert_eq!(val, 6);
    }
}
