//! `<sys/capability.h>` — POSIX capabilities (Linux implementation).
//!
//! Defines Linux capability constants and the capability header/data
//! structures used by `capget()` / `capset()`.

use crate::errno;

// ---------------------------------------------------------------------------
// Capability version
// ---------------------------------------------------------------------------
//
// Linux's `<linux/capability.h>` defines three versions:
//
//   * V1 (`0x19980330`) — the original 32-bit ABI.  Each capability set
//     fits in a single u32 (`_LINUX_CAPABILITY_U32S_1 = 1`); only
//     capabilities 0..=31 are addressable.
//   * V2 (`0x20071026`) — added a second u32 for the high bits.  This
//     version had a bug with 64-bit file capabilities and was deprecated
//     in favour of V3.  Wire format is identical to V3
//     (`_LINUX_CAPABILITY_U32S_2 = 2`), so V2 and V3 are interchangeable
//     for read/write purposes.
//   * V3 (`0x20080522`) — current preferred version; supports the full
//     64-bit capability set.
//
// `kernel/capability.c::cap_validate_magic` accepts all three.  When it
// sees an unknown version, it writes the kernel's preferred version
// (`_LINUX_CAPABILITY_VERSION_3`) into the caller's header and returns
// `-EINVAL`.  The libcap idiom for version discovery is to call
// `capget(&hdr, NULL)` with `hdr.version = 0`:
//
//   * NULL dataptr + EFAULT (NULL header)         → propagate EFAULT
//   * NULL dataptr + unknown version              → return 0 (probe
//     succeeded; preferred version was written to the header)
//   * NULL dataptr + valid version                → return 0
//   * non-NULL dataptr + any error                → propagate error
//
// We mirror that here so libcap, glibc's `cap_get_proc`, and shell
// utilities like `setpriv(1)` and `capsh(1)` can negotiate the version
// before issuing the real call.

/// Version 1 capability header (original 32-bit ABI; Linux 2.2+).
pub const _LINUX_CAPABILITY_VERSION_1: u32 = 0x19980330;

/// Number of u32 words for capability sets in v1 (low 32 bits only).
pub const _LINUX_CAPABILITY_U32S_1: usize = 1;

/// Version 2 capability header (deprecated; superseded by v3 but wire-
/// compatible with it).
pub const _LINUX_CAPABILITY_VERSION_2: u32 = 0x20071026;

/// Number of u32 words for capability sets in v2 (low + high 32 bits).
pub const _LINUX_CAPABILITY_U32S_2: usize = 2;

/// Version 3 capability header (Linux 2.6.26+, supports 64-bit sets).
pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x20080522;

/// Number of u32 words for capability sets in v3.
pub const _LINUX_CAPABILITY_U32S_3: usize = 2;

// ---------------------------------------------------------------------------
// Capability header
// ---------------------------------------------------------------------------

/// Capability header for `capget()`/`capset()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CapUserHeader {
    /// Capability version (`_LINUX_CAPABILITY_VERSION_3`).
    pub version: u32,
    /// PID (0 = calling process).
    pub pid: i32,
}

/// Capability data for `capget()`/`capset()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CapUserData {
    /// Effective capability set.
    pub effective: u32,
    /// Permitted capability set.
    pub permitted: u32,
    /// Inheritable capability set.
    pub inheritable: u32,
}

// ---------------------------------------------------------------------------
// Capability constants
// ---------------------------------------------------------------------------

/// Override DAC read/search.
pub const CAP_DAC_READ_SEARCH: u32 = 2;

/// Override DAC write.
pub const CAP_DAC_OVERRIDE: u32 = 1;

/// Bypass file ownership checks.
pub const CAP_FOWNER: u32 = 3;

/// Set file SUID/SGID.
pub const CAP_FSETID: u32 = 4;

/// Kill processes.
pub const CAP_KILL: u32 = 5;

/// Set UID/GID.
pub const CAP_SETUID: u32 = 7;

/// Set GID.
pub const CAP_SETGID: u32 = 6;

/// Set process capabilities.
pub const CAP_SETPCAP: u32 = 8;

/// Bypass file read/write/execute permission checks.
pub const CAP_CHOWN: u32 = 0;

/// Bind to privileged ports (< 1024).
pub const CAP_NET_BIND_SERVICE: u32 = 10;

/// Various network admin operations.
pub const CAP_NET_ADMIN: u32 = 12;

/// Use RAW and PACKET sockets.
pub const CAP_NET_RAW: u32 = 13;

/// Lock memory (mlock, mlockall).
pub const CAP_IPC_LOCK: u32 = 14;

/// Override IPC ownership checks.
pub const CAP_IPC_OWNER: u32 = 15;

/// Load and unload kernel modules.
pub const CAP_SYS_MODULE: u32 = 16;

/// Perform I/O port operations (ioperm, iopl).
pub const CAP_SYS_RAWIO: u32 = 17;

/// Use chroot.
pub const CAP_SYS_CHROOT: u32 = 18;

/// Trace arbitrary processes (ptrace).
pub const CAP_SYS_PTRACE: u32 = 19;

/// Accounting.
pub const CAP_SYS_PACCT: u32 = 20;

/// Various system admin operations.
pub const CAP_SYS_ADMIN: u32 = 21;

/// Use reboot.
pub const CAP_SYS_BOOT: u32 = 22;

/// Raise process nice value, change scheduling.
pub const CAP_SYS_NICE: u32 = 23;

/// Override resource limits.
pub const CAP_SYS_RESOURCE: u32 = 24;

/// Manipulate system clock.
pub const CAP_SYS_TIME: u32 = 25;

/// Configure tty devices.
pub const CAP_SYS_TTY_CONFIG: u32 = 26;

/// Create special files (mknod).
pub const CAP_MKNOD: u32 = 27;

/// Set file capabilities.
pub const CAP_SETFCAP: u32 = 31;

/// Audit control.
pub const CAP_AUDIT_CONTROL: u32 = 30;

/// Write audit log entries.
pub const CAP_AUDIT_WRITE: u32 = 29;

/// Configure MAC (Mandatory Access Control).
pub const CAP_MAC_ADMIN: u32 = 33;

/// Override MAC.
pub const CAP_MAC_OVERRIDE: u32 = 32;

/// Use `syslog()`.
pub const CAP_SYSLOG: u32 = 34;

/// Trigger wake-ups (via `/dev/wakealarm`).
pub const CAP_WAKE_ALARM: u32 = 35;

/// Block suspend.
pub const CAP_BLOCK_SUSPEND: u32 = 36;

/// Read audit log.
pub const CAP_AUDIT_READ: u32 = 37;

/// Perform perfmon operations.
pub const CAP_PERFMON: u32 = 38;

/// Use BPF.
pub const CAP_BPF: u32 = 39;

/// Use checkpoint/restore.
pub const CAP_CHECKPOINT_RESTORE: u32 = 40;

/// Last valid capability number.
pub const CAP_LAST_CAP: u32 = 40;

// ---------------------------------------------------------------------------
// Process capability sets
// ---------------------------------------------------------------------------
//
// Linux capability v3 holds 64 bits per set across two u32 words (the
// `datap[2]` array passed to capget/capset).  We model that as a pair
// of AtomicU32 per set.  The default value is "all caps held" — we
// run as root with no security boundary yet, so dropping a cap means
// the process voluntarily declines a privilege, but querying always
// reports whatever the process previously stored.

use core::sync::atomic::{AtomicU32, Ordering};

/// Initial value with every defined capability bit set (caps 0..=40
/// occupy the low 41 bits of the combined 64-bit set).
const DEFAULT_CAPS_LOW: u32 = u32::MAX;
const DEFAULT_CAPS_HIGH: u32 = (1u32 << 9).wrapping_sub(1); // caps 32..40 → 9 bits

// effective / permitted / inheritable, each (low, high) word.
static CAP_EFF_LO: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_LOW);
static CAP_EFF_HI: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_HIGH);
static CAP_PRM_LO: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_LOW);
static CAP_PRM_HI: AtomicU32 = AtomicU32::new(DEFAULT_CAPS_HIGH);
static CAP_INH_LO: AtomicU32 = AtomicU32::new(0);
static CAP_INH_HI: AtomicU32 = AtomicU32::new(0);

/// Read the currently-held effective capability set as (low, high).
#[must_use]
pub fn current_caps_effective() -> (u32, u32) {
    (
        CAP_EFF_LO.load(Ordering::Relaxed),
        CAP_EFF_HI.load(Ordering::Relaxed),
    )
}

/// Test whether the calling process holds capability `cap`.
///
/// Helper for permission checks elsewhere in the posix layer.  `cap`
/// must be one of the `CAP_*` constants; returns false if `cap >
/// CAP_LAST_CAP`.
#[must_use]
pub fn has_capability(cap: u32) -> bool {
    if cap > CAP_LAST_CAP {
        return false;
    }
    let (lo, hi) = current_caps_effective();
    if cap < 32 {
        lo & (1u32 << cap) != 0
    } else {
        hi & (1u32 << (cap.wrapping_sub(32))) != 0
    }
}

// ---------------------------------------------------------------------------
// Cross-module cap-state test serialisation
// ---------------------------------------------------------------------------
//
// The effective/permitted/inheritable cap sets above are process-global
// `AtomicU32`s. Cargo runs `posix`'s test suite multi-threaded, so any
// test that mutates the cap state (e.g. drops `CAP_SYS_ADMIN` to verify
// an unprivileged code path) races against every other test in every
// other module that does the same — or that merely reads/assumes a
// specific cap layout. The races manifest as ~150 spurious test
// failures per run, all with errno mismatches (EPERM vs ENOSYS, etc.)
// where a concurrent test had reset the caps mid-run.
//
// The fix: every test-only `CapGuard` in the crate acquires this
// global mutex before snapshotting/mutating the cap set and holds it
// until Drop restores. Cap-mutating tests therefore serialise across
// modules, and tests that only read the cap state but rely on it
// staying stable get the same protection by also holding the guard.
//
// Only compiled on host builds (`std::sync::Mutex` is unavailable in
// the no_std `target_os = "none"` build). The lock is intentionally
// `pub` so every module's CapGuard can name it.
#[cfg(not(target_os = "none"))]
pub static CAP_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// `std::sync::Mutex` is not reentrant: a thread that locks it twice
// deadlocks itself. Some tests nest `CapGuard`s on purpose — typically
// an outer snapshot for the whole test plus an inner one for a scoped
// modification (e.g. `test_ioprio_set_phase191_recovery_after_eperm`).
// To make those work under the global lock, we wrap acquisition in a
// thread-local depth counter: only the outermost guard actually owns
// the `MutexGuard`; nested guards are no-ops for the lock but still
// snapshot/restore caps independently.
#[cfg(not(target_os = "none"))]
std::thread_local! {
    static CAP_TEST_LOCK_DEPTH: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// RAII handle returned by [`CapTestLockGuard::acquire`]. Holds the
/// crate-global cap-state mutex on the outermost call per thread and is
/// a no-op for nested calls on the same thread. Drop decrements the
/// depth counter and releases the lock when depth returns to zero.
#[cfg(not(target_os = "none"))]
pub struct CapTestLockGuard {
    // `Some` on the outermost acquire; `None` for re-entrant acquires
    // that piggy-back on an existing hold.
    _inner: Option<std::sync::MutexGuard<'static, ()>>,
}

#[cfg(not(target_os = "none"))]
impl CapTestLockGuard {
    /// Acquire the cap-state test lock with re-entrant semantics.
    /// Poisoned locks are recovered (a prior panicking test holding the
    /// guard would otherwise wedge every subsequent test in the run).
    #[must_use]
    pub fn acquire() -> Self {
        let depth = CAP_TEST_LOCK_DEPTH.with(core::cell::Cell::get);
        CAP_TEST_LOCK_DEPTH.with(|d| d.set(depth.saturating_add(1)));
        let inner = if depth == 0 {
            Some(
                CAP_TEST_LOCK
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )
        } else {
            None
        };
        Self { _inner: inner }
    }
}

#[cfg(not(target_os = "none"))]
impl Drop for CapTestLockGuard {
    fn drop(&mut self) {
        CAP_TEST_LOCK_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        // `_inner` (the real MutexGuard) drops here on the outermost
        // guard, releasing the mutex.
    }
}

// ---------------------------------------------------------------------------
// capget / capset
// ---------------------------------------------------------------------------

/// Validate the header passed to `capget` / `capset`.
///
/// Mirrors Linux's `kernel/capability.c::cap_validate_magic`: returns
/// the per-set u32-word count (`tocopy`) — 1 for V1, 2 for V2/V3.  If
/// `version` is unsupported, writes the preferred version
/// (`_LINUX_CAPABILITY_VERSION_3`) into `*hdrp` and returns
/// `Err(EINVAL)`.  A NULL header pointer yields `Err(EFAULT)`.
///
/// PID-handling is **not** done here — Linux performs it after the
/// short-circuit that supports the probe pattern, so the caller does
/// the pid check itself once we know it is on the non-probe path.
fn validate_cap_header(hdrp: *mut CapUserHeader) -> Result<usize, i32> {
    if hdrp.is_null() {
        return Err(errno::EFAULT);
    }
    // SAFETY: hdrp is non-null by check above; caller contract for layout.
    let version = unsafe { (*hdrp).version };
    match version {
        _LINUX_CAPABILITY_VERSION_1 => Ok(_LINUX_CAPABILITY_U32S_1),
        _LINUX_CAPABILITY_VERSION_2 | _LINUX_CAPABILITY_VERSION_3 => Ok(_LINUX_CAPABILITY_U32S_3),
        _ => {
            // Tell the caller which version we prefer.
            // SAFETY: hdrp non-null.
            unsafe {
                (*hdrp).version = _LINUX_CAPABILITY_VERSION_3;
            }
            Err(errno::EINVAL)
        }
    }
}

/// Get process capabilities.
///
/// Writes the calling process's effective, permitted, and inheritable
/// sets into `datap[0..tocopy)` (1 entry for V1, 2 for V2/V3).  Returns
/// 0 on success, -1 with errno on validation failure.
///
/// # Linux semantics
///
/// `kernel/capability.c::SYSCALL_DEFINE2(capget)`:
///
/// ```c
/// ret = cap_validate_magic(header, &tocopy);
/// if ((dataptr == NULL) || (ret != 0))
///     return ((dataptr == NULL) && (ret == -EINVAL)) ? 0 : ret;
/// ```
///
/// The "probe" idiom — `capget(&hdr, NULL)` with `hdr.version = 0` —
/// must return 0 even when the header's version is unknown, because
/// `cap_validate_magic` has already written the preferred version into
/// the header and the caller's probe has succeeded.  Without this,
/// libcap and glibc's `cap_get_proc` cannot negotiate the version
/// before issuing the real call.
///
/// Errors (Linux-matching priority order):
/// * `EFAULT` — `hdrp` is NULL (header unreadable).  This wins over
///   the probe shortcut: a NULL header has no version field to write
///   the preferred value into, so the probe cannot have succeeded.
/// * `EINVAL` — non-NULL `datap` with an unknown header version.  The
///   header is rewritten with the preferred version regardless.
/// * `EPERM`  — `pid != 0` (real Linux looks up the target task's
///   credentials; our stub has no process model so we reject any
///   non-self request).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn capget(hdrp: *mut CapUserHeader, datap: *mut CapUserData) -> i32 {
    let validation = validate_cap_header(hdrp);

    // Linux's short-circuit:
    //     if ((dataptr == NULL) || (ret != 0))
    //         return ((dataptr == NULL) && (ret == -EINVAL)) ? 0 : ret;
    //
    // Probe path: when datap is NULL, callers want to discover the
    // preferred version.  We return 0 unless the header itself was
    // unreadable (EFAULT), in which case the probe cannot have written
    // the preferred version and we propagate the error.
    if datap.is_null() {
        return match validation {
            Ok(_) => 0,                        // probe with known version
            Err(e) if e == errno::EINVAL => 0, // probe wrote preferred version
            Err(e) => {
                errno::set_errno(e);
                -1
            }
        };
    }

    // Non-NULL datap with a validation error: propagate the error.
    let tocopy = match validation {
        Ok(t) => t,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };

    // SAFETY: hdrp non-null (validate_cap_header would have returned
    // EFAULT otherwise).
    let pid = unsafe { (*hdrp).pid };
    if pid != 0 {
        errno::set_errno(errno::EPERM);
        return -1;
    }

    let eff_lo = CAP_EFF_LO.load(Ordering::Relaxed);
    let eff_hi = CAP_EFF_HI.load(Ordering::Relaxed);
    let prm_lo = CAP_PRM_LO.load(Ordering::Relaxed);
    let prm_hi = CAP_PRM_HI.load(Ordering::Relaxed);
    let inh_lo = CAP_INH_LO.load(Ordering::Relaxed);
    let inh_hi = CAP_INH_HI.load(Ordering::Relaxed);
    // SAFETY: caller guarantees datap points to `tocopy` writable
    // CapUserData entries — 1 for V1 (low word only), 2 for V2/V3.
    unsafe {
        *datap = CapUserData {
            effective: eff_lo,
            permitted: prm_lo,
            inheritable: inh_lo,
        };
        if tocopy == _LINUX_CAPABILITY_U32S_3 {
            *datap.add(1) = CapUserData {
                effective: eff_hi,
                permitted: prm_hi,
                inheritable: inh_hi,
            };
        }
    }
    0
}

/// Set process capabilities.
///
/// Reads `datap[0..tocopy)` (1 entry for V1, 2 for V2/V3) and atomically
/// updates the effective, permitted, and inheritable sets.  Linux
/// enforces several invariants (effective ⊆ permitted;
/// inheritable ⊆ permitted ∪ inheritable-old; only `CAP_SETPCAP` allows
/// raising permitted) — we currently apply only the basic
/// effective-⊆-permitted check, since the full rules require a real
/// security model.  Returns 0 on success.
///
/// # Linux semantics
///
/// `kernel/capability.c::SYSCALL_DEFINE2(capset)`:
///
/// ```c
/// ret = cap_validate_magic(header, &tocopy);
/// if (ret != 0) return ret;
/// if (get_user(pid, &header->pid)) return -EFAULT;
/// if (pid != 0 && pid != task_pid_vnr(current)) return -EPERM;
/// if (copybytes > sizeof(kdata)) return -EINVAL;
/// if (copy_from_user(&kdata, data, copybytes)) return -EFAULT;
/// ```
///
/// Unlike `capget`, `capset` does **not** have a probe shortcut — the
/// data pointer must be valid.
///
/// Errors (Linux-matching priority order):
/// * `EFAULT` — `hdrp` is NULL.
/// * `EINVAL` — unknown header version (preferred version written back).
/// * `EPERM`  — `pid != 0` (Linux: pid must be 0 or self).  Phase 158:
///   this is checked **before** `datap` validation because Linux's
///   `SYSCALL_DEFINE2(capset)` runs `get_user(pid, &header->pid)` and
///   the pid != 0 check *before* `copy_from_user(&kdata, data, ...)`.
///   A bad pid wins over a NULL data pointer.
/// * `EFAULT` — `datap` is NULL (Linux: `copy_from_user` failure).
/// * `EPERM`  — effective ⊄ permitted (POSIX/Linux invariant).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn capset(hdrp: *mut CapUserHeader, datap: *const CapUserData) -> i32 {
    let tocopy = match validate_cap_header(hdrp) {
        Ok(t) => t,
        Err(e) => {
            errno::set_errno(e);
            return -1;
        }
    };
    // Phase 158: pid check runs before datap validation to match Linux's
    // `kernel/capability.c::SYSCALL_DEFINE2(capset)` ordering — the kernel
    // does `get_user(pid, ...)` and the pid-vs-self comparison *before*
    // `copy_from_user(&kdata, data, copybytes)`.  Pre-Phase-158 we EFAULTed
    // first on a NULL `datap`, which made buggy callers that passed both
    // bad pid and bad data see EFAULT instead of EPERM.
    //
    // SAFETY: hdrp non-null (validate_cap_header succeeded).
    let pid = unsafe { (*hdrp).pid };
    if pid != 0 {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    if datap.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: caller contract — datap points to `tocopy` readable
    // CapUserData entries (1 for V1, 2 for V2/V3).
    let (lo, hi) = unsafe {
        let lo = *datap;
        let hi = if tocopy == _LINUX_CAPABILITY_U32S_3 {
            *datap.add(1)
        } else {
            // V1 only carries the low 32 bits; high words default to 0
            // so any previously-set high bits are cleared on capset.
            CapUserData {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            }
        };
        (lo, hi)
    };
    // Effective must be a subset of permitted (POSIX/Linux invariant).
    if (lo.effective & !lo.permitted) != 0 || (hi.effective & !hi.permitted) != 0 {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    CAP_EFF_LO.store(lo.effective, Ordering::Relaxed);
    CAP_EFF_HI.store(hi.effective, Ordering::Relaxed);
    CAP_PRM_LO.store(lo.permitted, Ordering::Relaxed);
    CAP_PRM_HI.store(hi.permitted, Ordering::Relaxed);
    CAP_INH_LO.store(lo.inheritable, Ordering::Relaxed);
    CAP_INH_HI.store(hi.inheritable, Ordering::Relaxed);
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_header_size() {
        assert_eq!(core::mem::size_of::<CapUserHeader>(), 8);
    }

    #[test]
    fn test_cap_data_size() {
        assert_eq!(core::mem::size_of::<CapUserData>(), 12);
    }

    #[test]
    fn test_cap_constants_in_range() {
        let caps = [
            CAP_CHOWN,
            CAP_DAC_OVERRIDE,
            CAP_DAC_READ_SEARCH,
            CAP_FOWNER,
            CAP_FSETID,
            CAP_KILL,
            CAP_SETGID,
            CAP_SETUID,
            CAP_SETPCAP,
            CAP_NET_BIND_SERVICE,
            CAP_NET_ADMIN,
            CAP_NET_RAW,
            CAP_IPC_LOCK,
            CAP_IPC_OWNER,
            CAP_SYS_MODULE,
            CAP_SYS_RAWIO,
            CAP_SYS_CHROOT,
            CAP_SYS_PTRACE,
            CAP_SYS_PACCT,
            CAP_SYS_ADMIN,
            CAP_SYS_BOOT,
            CAP_SYS_NICE,
            CAP_SYS_RESOURCE,
            CAP_SYS_TIME,
            CAP_SYS_TTY_CONFIG,
            CAP_MKNOD,
            CAP_AUDIT_WRITE,
            CAP_AUDIT_CONTROL,
            CAP_SETFCAP,
            CAP_MAC_OVERRIDE,
            CAP_MAC_ADMIN,
            CAP_SYSLOG,
            CAP_WAKE_ALARM,
            CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ,
            CAP_PERFMON,
            CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        for &c in &caps {
            assert!(c <= CAP_LAST_CAP, "CAP_{c} exceeds CAP_LAST_CAP");
        }
    }

    #[test]
    fn test_cap_constants_distinct() {
        let caps = [
            CAP_CHOWN,
            CAP_DAC_OVERRIDE,
            CAP_DAC_READ_SEARCH,
            CAP_FOWNER,
            CAP_FSETID,
            CAP_KILL,
            CAP_SETGID,
            CAP_SETUID,
            CAP_SETPCAP,
            CAP_NET_BIND_SERVICE,
            CAP_NET_ADMIN,
            CAP_NET_RAW,
            CAP_IPC_LOCK,
            CAP_IPC_OWNER,
            CAP_SYS_MODULE,
            CAP_SYS_RAWIO,
            CAP_SYS_CHROOT,
            CAP_SYS_PTRACE,
            CAP_SYS_PACCT,
            CAP_SYS_ADMIN,
            CAP_SYS_BOOT,
            CAP_SYS_NICE,
            CAP_SYS_RESOURCE,
            CAP_SYS_TIME,
            CAP_SYS_TTY_CONFIG,
            CAP_MKNOD,
            CAP_AUDIT_WRITE,
            CAP_AUDIT_CONTROL,
            CAP_SETFCAP,
            CAP_MAC_OVERRIDE,
            CAP_MAC_ADMIN,
            CAP_SYSLOG,
            CAP_WAKE_ALARM,
            CAP_BLOCK_SUSPEND,
            CAP_AUDIT_READ,
            CAP_PERFMON,
            CAP_BPF,
            CAP_CHECKPOINT_RESTORE,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j], "CAP constants must be distinct");
            }
        }
    }

    #[test]
    fn test_cap_last_cap() {
        assert_eq!(CAP_LAST_CAP, 40);
    }

    #[test]
    fn test_cap_version_3() {
        assert_eq!(_LINUX_CAPABILITY_VERSION_3, 0x20080522);
    }

    /// Restore the capability sets to their cold-boot defaults so tests
    /// that mutate state don't leak into one another.
    fn reset_caps() {
        CAP_EFF_LO.store(DEFAULT_CAPS_LOW, Ordering::Relaxed);
        CAP_EFF_HI.store(DEFAULT_CAPS_HIGH, Ordering::Relaxed);
        CAP_PRM_LO.store(DEFAULT_CAPS_LOW, Ordering::Relaxed);
        CAP_PRM_HI.store(DEFAULT_CAPS_HIGH, Ordering::Relaxed);
        CAP_INH_LO.store(0, Ordering::Relaxed);
        CAP_INH_HI.store(0, Ordering::Relaxed);
    }

    #[test]
    fn test_capget_null_header_efault() {
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(core::ptr::null_mut(), data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_capset_null_header_efault() {
        let data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capset(core::ptr::null_mut(), data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_capset_null_data_efault() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_capget_version_mismatch_rewrites_header() {
        let mut hdr = CapUserHeader {
            version: 0xdeadbeef,
            pid: 0,
        };
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Linux kernel writes the preferred version back into the header.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    #[test]
    fn test_capset_version_mismatch_rewrites_header() {
        let mut hdr = CapUserHeader { version: 1, pid: 0 };
        let data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    #[test]
    fn test_capget_nonzero_pid_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 42,
        };
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_capset_nonzero_pid_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 99,
        };
        let data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    #[test]
    fn test_capget_null_datap_is_probe() {
        // A null datap is a valid "probe" — Linux uses it to discover
        // the supported version. Returns 0; header is left intact
        // since it already matched our preferred version.
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let ret = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    #[test]
    fn test_capget_returns_defaults() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, 0);
        assert_eq!(data[0].effective, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].permitted, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].inheritable, 0);
        assert_eq!(data[1].effective, DEFAULT_CAPS_HIGH);
        assert_eq!(data[1].permitted, DEFAULT_CAPS_HIGH);
        assert_eq!(data[1].inheritable, 0);
        reset_caps();
    }

    #[test]
    fn test_capset_then_capget_roundtrip() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        // Drop everything except CAP_NET_BIND_SERVICE (bit 10) in
        // effective, keep all in permitted.
        let want_eff_lo: u32 = 1u32 << CAP_NET_BIND_SERVICE;
        let want_inh_lo: u32 = 1u32 << CAP_CHOWN;
        let set_data = [
            CapUserData {
                effective: want_eff_lo,
                permitted: DEFAULT_CAPS_LOW,
                inheritable: want_inh_lo,
            },
            CapUserData {
                effective: 0,
                permitted: DEFAULT_CAPS_HIGH,
                inheritable: 0,
            },
        ];
        let ret = capset(&mut hdr, set_data.as_ptr());
        assert_eq!(ret, 0);

        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, 0);
        assert_eq!(data[0].effective, want_eff_lo);
        assert_eq!(data[0].permitted, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].inheritable, want_inh_lo);
        assert_eq!(data[1].effective, 0);
        assert_eq!(data[1].permitted, DEFAULT_CAPS_HIGH);
        assert_eq!(data[1].inheritable, 0);
        reset_caps();
    }

    #[test]
    fn test_capset_rejects_effective_not_subset_of_permitted() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        // Effective claims CAP_KILL (bit 5) but permitted does not.
        let bad = [
            CapUserData {
                effective: 1u32 << CAP_KILL,
                permitted: 0,
                inheritable: 0,
            },
            CapUserData {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
        ];
        let ret = capset(&mut hdr, bad.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
        reset_caps();
    }

    #[test]
    fn test_capset_rejects_effective_not_subset_high_word() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        // High-word violation: claim CAP_BPF (bit 39, → high bit 7) in
        // effective without permitted.
        let bad = [
            CapUserData {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
            CapUserData {
                effective: 1u32 << 7,
                permitted: 0,
                inheritable: 0,
            },
        ];
        let ret = capset(&mut hdr, bad.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
        reset_caps();
    }

    #[test]
    fn test_has_capability_default_holds_known_caps() {
        reset_caps();
        assert!(has_capability(CAP_CHOWN));
        assert!(has_capability(CAP_KILL));
        assert!(has_capability(CAP_SYS_ADMIN));
        // High-word cap defined within DEFAULT_CAPS_HIGH range (cap 39
        // = bit 7 of high; DEFAULT_CAPS_HIGH = 0x1FF covers bits 0..8).
        assert!(has_capability(CAP_BPF));
        assert!(has_capability(CAP_CHECKPOINT_RESTORE));
        reset_caps();
    }

    #[test]
    fn test_has_capability_out_of_range() {
        // Anything past CAP_LAST_CAP is rejected outright.
        assert!(!has_capability(CAP_LAST_CAP + 1));
        assert!(!has_capability(63));
        assert!(!has_capability(u32::MAX));
    }

    #[test]
    fn test_has_capability_follows_capset() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        // Drop everything in effective.
        let zero = [
            CapUserData {
                effective: 0,
                permitted: DEFAULT_CAPS_LOW,
                inheritable: 0,
            },
            CapUserData {
                effective: 0,
                permitted: DEFAULT_CAPS_HIGH,
                inheritable: 0,
            },
        ];
        let ret = capset(&mut hdr, zero.as_ptr());
        assert_eq!(ret, 0);
        assert!(!has_capability(CAP_CHOWN));
        assert!(!has_capability(CAP_SYS_ADMIN));
        assert!(!has_capability(CAP_BPF));
        reset_caps();
        // After reset, defaults should restore visibility.
        assert!(has_capability(CAP_CHOWN));
    }

    #[test]
    fn test_current_caps_effective_default() {
        reset_caps();
        let (lo, hi) = current_caps_effective();
        assert_eq!(lo, DEFAULT_CAPS_LOW);
        assert_eq!(hi, DEFAULT_CAPS_HIGH);
    }

    #[test]
    fn test_cap_known_values() {
        assert_eq!(CAP_CHOWN, 0);
        assert_eq!(CAP_DAC_OVERRIDE, 1);
        assert_eq!(CAP_KILL, 5);
        assert_eq!(CAP_SYS_ADMIN, 21);
        assert_eq!(CAP_NET_BIND_SERVICE, 10);
    }

    // ------------------------------------------------------------------
    // Phase 132 — capget/capset accept V1 / V2 / V3, and the NULL-dataptr
    // probe pattern returns 0 even for unknown versions
    //
    // Linux's `cap_validate_magic` accepts V1 (one u32 per set), V2
    // (two u32, deprecated), and V3 (two u32, current).  The probe
    // idiom — `capget(&hdr, NULL)` with any version — must return 0 so
    // libcap/glibc can negotiate the version field before issuing the
    // real call.  Phases prior to 132 rejected V1/V2 with EINVAL and
    // returned EINVAL on the probe path with an unknown version,
    // breaking libcap's `cap_get_proc`.
    // ------------------------------------------------------------------

    // -- Helper / constant tests -------------------------------------------

    #[test]
    fn test_phase132_capability_v1_constant() {
        assert_eq!(_LINUX_CAPABILITY_VERSION_1, 0x19980330);
        assert_eq!(_LINUX_CAPABILITY_U32S_1, 1);
    }

    #[test]
    fn test_phase132_capability_v2_constant() {
        assert_eq!(_LINUX_CAPABILITY_VERSION_2, 0x20071026);
        assert_eq!(_LINUX_CAPABILITY_U32S_2, 2);
    }

    #[test]
    fn test_phase132_all_versions_distinct() {
        let versions = [
            _LINUX_CAPABILITY_VERSION_1,
            _LINUX_CAPABILITY_VERSION_2,
            _LINUX_CAPABILITY_VERSION_3,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(
                    versions[i], versions[j],
                    "capability versions must be distinct"
                );
            }
        }
    }

    // -- V1 accepted by capget --------------------------------------------

    #[test]
    fn test_phase132_capget_v1_writes_only_low_slot() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_1,
            pid: 0,
        };
        // Sentinel high slot — must remain untouched after V1 capget.
        let mut data = [
            CapUserData {
                effective: 0,
                permitted: 0,
                inheritable: 0,
            },
            CapUserData {
                effective: 0xDEAD_BEEF,
                permitted: 0xCAFE_BABE,
                inheritable: 0xFEED_FACE,
            },
        ];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, 0);
        // Low slot populated with the default caps.
        assert_eq!(data[0].effective, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].permitted, DEFAULT_CAPS_LOW);
        assert_eq!(data[0].inheritable, 0);
        // High slot untouched — V1 only writes one entry.
        assert_eq!(data[1].effective, 0xDEAD_BEEF);
        assert_eq!(data[1].permitted, 0xCAFE_BABE);
        assert_eq!(data[1].inheritable, 0xFEED_FACE);
        // Header version is *not* rewritten — V1 is valid.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_1);
        reset_caps();
    }

    // -- V2 accepted by capget --------------------------------------------

    #[test]
    fn test_phase132_capget_v2_writes_both_slots() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_2,
            pid: 0,
        };
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, 0);
        // Both slots populated — V2 is wire-compatible with V3.
        assert_eq!(data[0].effective, DEFAULT_CAPS_LOW);
        assert_eq!(data[1].effective, DEFAULT_CAPS_HIGH);
        // Header version is *not* rewritten.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_2);
        reset_caps();
    }

    // -- Probe pattern: NULL dataptr with unknown version -----------------

    #[test]
    fn test_phase132_capget_probe_unknown_version_returns_zero() {
        // Probe pattern: caller passes garbage version with NULL datap.
        // Linux: returns 0 after writing the preferred version.  This is
        // libcap's `_cap_get_proc` initial probe.
        let mut hdr = CapUserHeader { version: 0, pid: 0 };
        errno::set_errno(errno::EBADF);
        let ret = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(ret, 0);
        // Header was rewritten with the preferred version.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
        // POSIX: successful syscall must not touch errno.
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_phase132_capget_probe_v1_returns_zero() {
        // Probe with a known version still returns 0 (no rewrite needed).
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_1,
            pid: 0,
        };
        let ret = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_1);
    }

    #[test]
    fn test_phase132_capget_probe_v2_returns_zero() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_2,
            pid: 0,
        };
        let ret = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_2);
    }

    // -- EFAULT wins over probe shortcut ----------------------------------

    #[test]
    fn test_phase132_capget_null_header_efault_even_with_null_datap() {
        // EFAULT from a NULL header pointer is *not* short-circuited by
        // the probe path — without a writable header there's no way to
        // signal the preferred version, so Linux propagates -EFAULT.
        errno::set_errno(0);
        let ret = capget(core::ptr::null_mut(), core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- Non-NULL datap with unknown version still EINVAL -----------------

    #[test]
    fn test_phase132_capget_unknown_version_nonnull_datap_einval() {
        // Real call (non-NULL datap) with unknown version: EINVAL with
        // preferred version written.  This is the post-probe regression
        // path — must continue to work.
        let mut hdr = CapUserHeader {
            version: 0x12345678,
            pid: 0,
        };
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    // -- capset accepts V1 and V2 -----------------------------------------

    #[test]
    fn test_phase132_capset_v2_accepted() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_2,
            pid: 0,
        };
        // V2 is wire-compatible with V3 — set caps and verify they took.
        let want_eff: u32 = 1u32 << CAP_KILL;
        let data = [
            CapUserData {
                effective: want_eff,
                permitted: DEFAULT_CAPS_LOW,
                inheritable: 0,
            },
            CapUserData {
                effective: 0,
                permitted: DEFAULT_CAPS_HIGH,
                inheritable: 0,
            },
        ];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, 0);
        let (lo, hi) = current_caps_effective();
        assert_eq!(lo, want_eff);
        assert_eq!(hi, 0);
        // Header is *not* rewritten when version is accepted.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_2);
        reset_caps();
    }

    #[test]
    fn test_phase132_capset_v1_clears_high_word() {
        // V1 only carries the low 32 bits; the high word defaults to 0,
        // so any previously-set high-bit caps must be cleared.
        reset_caps();
        // Pre-condition: defaults have high bits set (CAP_BPF etc.).
        let (_, hi_before) = current_caps_effective();
        assert_ne!(hi_before, 0, "DEFAULT_CAPS_HIGH should be non-zero");

        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_1,
            pid: 0,
        };
        let data = [CapUserData {
            effective: 1u32 << CAP_KILL,
            permitted: DEFAULT_CAPS_LOW,
            inheritable: 0,
        }];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, 0);
        let (lo, hi) = current_caps_effective();
        assert_eq!(lo, 1u32 << CAP_KILL);
        // High word cleared because V1 carries no high-set data.
        assert_eq!(hi, 0);
        reset_caps();
    }

    // -- Validation-order parity (Linux's flow) ---------------------------

    #[test]
    fn test_phase132_capset_efault_beats_einval_when_header_null() {
        // NULL header → EFAULT before any version check.
        errno::set_errno(0);
        let data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capset(core::ptr::null_mut(), data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase132_capset_einval_beats_eperm_for_pid() {
        // Unknown version with pid != 0: EINVAL (version) wins over EPERM
        // (pid) — version is checked first in cap_validate_magic.
        let mut hdr = CapUserHeader {
            version: 0xBADCAFE,
            pid: 42,
        };
        let data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Preferred version was still written.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    // -- Workflow: libcap probe-then-call ---------------------------------

    #[test]
    fn test_phase132_workflow_libcap_probe_then_real_call() {
        reset_caps();
        // 1. Probe with version=0, NULL datap.  Expect ret 0 and the
        //    preferred version written to hdr.version.
        let mut hdr = CapUserHeader { version: 0, pid: 0 };
        let r1 = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(r1, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);

        // 2. Real call with the discovered version.  Expect populated
        //    data and ret 0.
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let r2 = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(r2, 0);
        assert_eq!(data[0].effective, DEFAULT_CAPS_LOW);
        assert_eq!(data[1].effective, DEFAULT_CAPS_HIGH);
        reset_caps();
    }

    // -- Buggy-caller cases -----------------------------------------------

    #[test]
    fn test_phase132_buggy_caller_uninitialised_version_probe_works() {
        // C: `struct __user_cap_header_struct hdr; hdr.pid = 0;` —
        // hdr.version is uninitialised stack memory.  If the caller
        // immediately probes (NULL datap), Linux returns 0 and writes
        // the preferred version even if the garbage happened to be a
        // valid version.  Test with a deliberately weird value.
        let mut hdr = CapUserHeader {
            version: 0x5A5A_5A5A,
            pid: 0,
        };
        let ret = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(ret, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    // -- Recovery: probe doesn't poison subsequent calls ------------------

    #[test]
    fn test_phase132_recovery_after_unknown_version_probe() {
        reset_caps();
        // 1. Probe with garbage version succeeds.
        let mut hdr = CapUserHeader {
            version: 0xBAD,
            pid: 0,
        };
        errno::set_errno(0);
        let r1 = capget(&mut hdr, core::ptr::null_mut());
        assert_eq!(r1, 0);
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
        // Probe success: errno was NOT clobbered.
        assert_eq!(errno::get_errno(), 0);

        // 2. The very next real capget with the now-correct version must
        //    reach the data-write path, not stale EINVAL.
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let r2 = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(r2, 0);
        assert_eq!(data[0].effective, DEFAULT_CAPS_LOW);
        reset_caps();
    }

    // ------------------------------------------------------------------
    // Phase 158 — capset validation-order fix: pid (EPERM) wins over
    // datap NULL (EFAULT)
    //
    // Linux's `kernel/capability.c::SYSCALL_DEFINE2(capset)`:
    //
    //     ret = cap_validate_magic(header, &tocopy);
    //     if ((ret < 0) && (ret != -EINVAL)) return ret;
    //     if (get_user(pid, &header->pid)) return -EFAULT;
    //     if (pid != 0 && pid != task_pid_vnr(current)) return -EPERM;
    //     copybytes = tocopy * sizeof(struct __user_cap_data_struct);
    //     if (copybytes > sizeof(kdata)) return -EINVAL;
    //     if (copy_from_user(&kdata, data, copybytes)) return -EFAULT;
    //
    // The pid check runs *before* the copy_from_user(data) check.  A bad
    // pid therefore beats a NULL data pointer.  Pre-Phase-158 we EFAULTed
    // first because we tested datap for NULL before reading the pid.
    //
    // Precedence (post-fix), highest to lowest:
    //   1. EFAULT — hdrp is NULL                 (validate_cap_header)
    //   2. EINVAL — unknown header version       (validate_cap_header)
    //   3. EPERM  — pid != 0                     (pid check)
    //   4. EFAULT — datap is NULL                (data NULL check)
    //   5. EPERM  — effective ⊄ permitted        (POSIX invariant)
    // ------------------------------------------------------------------

    // -- Per-error-class --------------------------------------------------

    /// Sanity: bad pid alone (non-NULL data) still yields EPERM.  This
    /// arm of the precedence ladder was already covered by the original
    /// `test_capset_nonzero_pid_eperm`; we include the Phase-158 copy as
    /// a fixed anchor so any future re-ordering shows up here too.
    #[test]
    fn test_phase158_capset_bad_pid_alone_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 1,
        };
        let data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        errno::set_errno(0);
        let ret = capset(&mut hdr, data.as_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Sanity: NULL data alone (good pid) yields EFAULT.  Mirrors the
    /// pre-existing `test_capset_null_data_efault` so the Phase-158 grid
    /// is self-contained.
    #[test]
    fn test_phase158_capset_null_data_alone_efault() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        errno::set_errno(0);
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // -- Ordering matrix --------------------------------------------------

    /// Core Phase-158 fix: pid != 0 with NULL data → EPERM (not EFAULT).
    /// Pre-fix this returned EFAULT because the datap NULL check ran
    /// first.  Post-fix matches Linux's `SYSCALL_DEFINE2(capset)` order.
    #[test]
    fn test_phase158_capset_bad_pid_null_data_yields_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 99,
        };
        errno::set_errno(0);
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        // Phase 158: EPERM (pid) wins over EFAULT (NULL data) because
        // Linux runs the pid check before copy_from_user.
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Symmetric: negative pid with NULL data still yields EPERM (our
    /// stub treats every non-zero pid the same — Linux would split
    /// pid<0 into EINVAL later, but only after the data check).
    #[test]
    fn test_phase158_capset_negative_pid_null_data_yields_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: -7,
        };
        errno::set_errno(0);
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Header EFAULT still wins over everything (NULL header, NULL data,
    /// bad pid implied by garbage header).
    #[test]
    fn test_phase158_capset_null_header_beats_null_data() {
        errno::set_errno(0);
        let ret = capset(core::ptr::null_mut(), core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    /// EINVAL (unknown version) wins over both NULL data and bad pid —
    /// `validate_cap_header` runs first.
    #[test]
    fn test_phase158_capset_einval_beats_eperm_and_efault() {
        let mut hdr = CapUserHeader {
            version: 0xDEAD_BEEF,
            pid: 13,
        };
        errno::set_errno(0);
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // Preferred version written even on this combined-error path.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    // -- Workflow: glibc-style cap_set_proc with stale pid ----------------

    /// Workflow regression: a misconfigured caller that copy-pasted a
    /// child's pid into the header and forgot to allocate a data buffer
    /// (or zero-initialised the pointer) now sees EPERM rather than
    /// EFAULT.  That matches Linux and signals "you can't touch another
    /// task," which is the actionable diagnostic.
    #[test]
    fn test_phase158_workflow_stale_pid_null_data() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 1234,
        };
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
        // Header version is NOT rewritten when validation succeeded —
        // only unknown-version branches touch the version field.
        assert_eq!(hdr.version, _LINUX_CAPABILITY_VERSION_3);
    }

    // -- Buggy-caller cases -----------------------------------------------

    /// V1 caller with bad pid and NULL data: pid check still beats data
    /// check.  Demonstrates the ordering holds for the legacy ABI too.
    #[test]
    fn test_phase158_capset_v1_bad_pid_null_data_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_1,
            pid: 5,
        };
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// V2 caller likewise.
    #[test]
    fn test_phase158_capset_v2_bad_pid_null_data_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_2,
            pid: 5,
        };
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    // -- Recovery: state isn't mutated on the failed paths ----------------

    /// State invariant: the EPERM-from-bad-pid path must not touch the
    /// stored capability sets.  Two passes around a `capset(bad)` call
    /// should leave `current_caps_effective()` unchanged.
    #[test]
    fn test_phase158_capset_failed_call_does_not_mutate_state() {
        reset_caps();
        let (before_lo, before_hi) = current_caps_effective();

        // Phase-158 failure path: bad pid + NULL data.
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 7,
        };
        let _ = capset(&mut hdr, core::ptr::null());

        let (after_lo, after_hi) = current_caps_effective();
        assert_eq!(before_lo, after_lo, "EPERM path must not mutate caps");
        assert_eq!(before_hi, after_hi, "EPERM path must not mutate caps");
        reset_caps();
    }

    /// State invariant: NULL-data with good pid (EFAULT) likewise leaves
    /// state untouched.  Sanity check that pre-existing EFAULT path
    /// hasn't acquired an unintended side-effect.
    #[test]
    fn test_phase158_capset_efault_path_does_not_mutate_state() {
        reset_caps();
        let (before_lo, before_hi) = current_caps_effective();

        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let _ = capset(&mut hdr, core::ptr::null());

        let (after_lo, after_hi) = current_caps_effective();
        assert_eq!(before_lo, after_lo);
        assert_eq!(before_hi, after_hi);
        reset_caps();
    }

    // -- No-side-effect loop ---------------------------------------------

    /// Loop the Phase-158 failure path 200 times.  No state mutation, no
    /// errno desynchronisation: every iteration must return -1 / EPERM.
    #[test]
    fn test_phase158_capset_eperm_loop_is_idempotent() {
        reset_caps();
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 42,
        };
        for _ in 0..200 {
            errno::set_errno(0);
            let r = capset(&mut hdr, core::ptr::null());
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EPERM);
        }
        let (lo, hi) = current_caps_effective();
        assert_eq!(lo, DEFAULT_CAPS_LOW);
        assert_eq!(hi, DEFAULT_CAPS_HIGH);
        reset_caps();
    }

    // -- Sentinel: pre-Phase-158 behaviour no longer holds ----------------

    /// Sentinel: the pre-Phase-158 contract was "NULL data EFAULT beats
    /// pid EPERM."  Asserting the *opposite* here pins the new contract
    /// in place — if anyone restores the old order this test trips.
    #[test]
    fn test_capset_bad_pid_null_data_no_longer_returns_efault_phase158() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 1,
        };
        let ret = capset(&mut hdr, core::ptr::null());
        assert_eq!(ret, -1);
        // Phase 158 reversed this: it used to be EFAULT, now EPERM.
        assert_ne!(errno::get_errno(), errno::EFAULT);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    // -- Cross-checks: capget ordering is independent --------------------

    /// Cross-check: capget's NULL-datap-as-probe shortcut is *not*
    /// affected by Phase 158.  Bad pid with NULL data on capget is the
    /// probe path → returns 0 (the probe succeeded; the pid field isn't
    /// read until after the probe shortcut).
    #[test]
    fn test_phase158_capget_null_datap_still_probe_even_with_bad_pid() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 77,
        };
        errno::set_errno(0);
        let ret = capget(&mut hdr, core::ptr::null_mut());
        // capget's probe short-circuit (datap NULL) returns 0 without
        // ever reaching the pid check.  Phase 158 only adjusted capset.
        assert_eq!(ret, 0);
    }

    /// Cross-check: capget with non-NULL data and bad pid still EPERM
    /// (unchanged by Phase 158).
    #[test]
    fn test_phase158_capget_bad_pid_nonnull_data_still_eperm() {
        let mut hdr = CapUserHeader {
            version: _LINUX_CAPABILITY_VERSION_3,
            pid: 5,
        };
        let mut data = [CapUserData {
            effective: 0,
            permitted: 0,
            inheritable: 0,
        }; 2];
        let ret = capget(&mut hdr, data.as_mut_ptr());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }
}
