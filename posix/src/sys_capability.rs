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
// `datap[2]` array passed to capget/capset).  The default value is "all
// caps held" — we run as root with no security boundary yet, so dropping
// a cap means the process voluntarily declines a privilege, but querying
// always reports whatever the process previously stored.
//
// ## Where the words live
//
// On the real target the sets are process-global, because that is what
// they model: capabilities are a property of the process, and every
// thread in it shares them.  On the host, where this crate exists only to
// be unit-tested, they are `thread_local!` instead — for exactly the
// reason `perthread` gives (known-issues.md `TD-POSIX-TEST-PARALLEL`):
// cargo runs the suite on many threads in one process, so process-global
// cap words make every cap-mutating test a race against every other test
// that reads or writes them.  Giving each test thread its own set makes
// that interference structurally impossible rather than something each
// test has to remember to defend against.

/// Initial value with every defined capability bit set (caps 0..=40
/// occupy the low 41 bits of the combined 64-bit set).
const DEFAULT_CAPS_LOW: u32 = u32::MAX;
const DEFAULT_CAPS_HIGH: u32 = (1u32 << 9).wrapping_sub(1); // caps 32..40 → 9 bits

/// The three capability sets, each as a (low, high) `u32` pair.
///
/// Loaded and stored as one value so the backing store can differ per
/// build without every caller knowing which it got.
#[derive(Clone, Copy)]
pub(crate) struct CapWords {
    pub(crate) eff_lo: u32,
    pub(crate) eff_hi: u32,
    pub(crate) prm_lo: u32,
    pub(crate) prm_hi: u32,
    pub(crate) inh_lo: u32,
    pub(crate) inh_hi: u32,
}

/// Cold-boot capability state: all defined caps effective and permitted,
/// nothing inheritable.
pub(crate) const CAPS_DEFAULT: CapWords = CapWords {
    eff_lo: DEFAULT_CAPS_LOW,
    eff_hi: DEFAULT_CAPS_HIGH,
    prm_lo: DEFAULT_CAPS_LOW,
    prm_hi: DEFAULT_CAPS_HIGH,
    inh_lo: 0,
    inh_hi: 0,
};

// -- target build: process-global, shared by every thread ------------------

#[cfg(target_os = "none")]
mod store {
    use super::{CAPS_DEFAULT, CapWords};
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    // effective / permitted / inheritable, each (low, high) word. Seeded
    // from `CAPS_DEFAULT` so both builds share one statement of the
    // cold-boot set rather than restating it.
    static CAP_EFF_LO: AtomicU32 = AtomicU32::new(CAPS_DEFAULT.eff_lo);
    static CAP_EFF_HI: AtomicU32 = AtomicU32::new(CAPS_DEFAULT.eff_hi);
    static CAP_PRM_LO: AtomicU32 = AtomicU32::new(CAPS_DEFAULT.prm_lo);
    static CAP_PRM_HI: AtomicU32 = AtomicU32::new(CAPS_DEFAULT.prm_hi);
    static CAP_INH_LO: AtomicU32 = AtomicU32::new(CAPS_DEFAULT.inh_lo);
    static CAP_INH_HI: AtomicU32 = AtomicU32::new(CAPS_DEFAULT.inh_hi);

    // The kernel's own answer, projected onto Linux's words (§312).  Absent
    // until `kernel_view::refresh` succeeds, which is why it needs a validity
    // flag rather than a sentinel value: "no capabilities at all" and "never
    // asked" are different states and must not collapse into each other — the
    // first is a true empty set, the second means we still do not know.
    static PROJ_VALID: AtomicBool = AtomicBool::new(false);
    static PROJ_LO: AtomicU32 = AtomicU32::new(0);
    static PROJ_HI: AtomicU32 = AtomicU32::new(0);

    pub(super) fn load_projection() -> Option<(u32, u32)> {
        // Acquire pairs with the Release store below so a reader that sees
        // the flag also sees the two words it describes.
        if PROJ_VALID.load(Ordering::Acquire) {
            Some((
                PROJ_LO.load(Ordering::Relaxed),
                PROJ_HI.load(Ordering::Relaxed),
            ))
        } else {
            None
        }
    }

    pub(super) fn store_projection(lo: u32, hi: u32) {
        PROJ_LO.store(lo, Ordering::Relaxed);
        PROJ_HI.store(hi, Ordering::Relaxed);
        PROJ_VALID.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn clear_projection() {
        PROJ_VALID.store(false, Ordering::Release);
    }

    /// Read all six words.  Relaxed and word-by-word, so a concurrent
    /// `capset` can in principle be observed half-applied; that matches
    /// the pre-existing behaviour and the fact that a process changing
    /// its own privileges from two threads at once is already a race in
    /// the caller.
    pub(super) fn load() -> CapWords {
        CapWords {
            eff_lo: CAP_EFF_LO.load(Ordering::Relaxed),
            eff_hi: CAP_EFF_HI.load(Ordering::Relaxed),
            prm_lo: CAP_PRM_LO.load(Ordering::Relaxed),
            prm_hi: CAP_PRM_HI.load(Ordering::Relaxed),
            inh_lo: CAP_INH_LO.load(Ordering::Relaxed),
            inh_hi: CAP_INH_HI.load(Ordering::Relaxed),
        }
    }

    pub(super) fn store(c: CapWords) {
        CAP_EFF_LO.store(c.eff_lo, Ordering::Relaxed);
        CAP_EFF_HI.store(c.eff_hi, Ordering::Relaxed);
        CAP_PRM_LO.store(c.prm_lo, Ordering::Relaxed);
        CAP_PRM_HI.store(c.prm_hi, Ordering::Relaxed);
        CAP_INH_LO.store(c.inh_lo, Ordering::Relaxed);
        CAP_INH_HI.store(c.inh_hi, Ordering::Relaxed);
    }
}

// -- host build: per-thread, so parallel tests cannot collide --------------

#[cfg(not(target_os = "none"))]
mod store {
    use super::{CAPS_DEFAULT, CapWords};

    std::thread_local! {
        static CAPS: core::cell::Cell<CapWords> =
            const { core::cell::Cell::new(CAPS_DEFAULT) };

        /// The kernel's projected set (§312), or `None` if never seeded.
        ///
        /// Per-thread for the same reason `CAPS` is: the projection tests set
        /// it and then assert on `capget`, and a process-global slot would
        /// make each of those a race against every other test in the suite.
        /// On the target this is a process-global pair of atomics, which is
        /// what the model actually calls for — capabilities belong to the
        /// process, not the thread.
        static PROJECTION: core::cell::Cell<Option<(u32, u32)>> =
            const { core::cell::Cell::new(None) };
    }

    pub(super) fn load_projection() -> Option<(u32, u32)> {
        PROJECTION.try_with(core::cell::Cell::get).unwrap_or(None)
    }

    pub(super) fn store_projection(lo: u32, hi: u32) {
        let _ = PROJECTION.try_with(|cell| cell.set(Some((lo, hi))));
    }

    #[cfg(test)]
    pub(super) fn clear_projection() {
        let _ = PROJECTION.try_with(|cell| cell.set(None));
    }

    /// Falls back to the cold-boot default if the thread's TLS is already
    /// destroyed (only reachable from a TLS destructor, which nothing in
    /// this crate registers).
    pub(super) fn load() -> CapWords {
        CAPS.try_with(core::cell::Cell::get).unwrap_or(CAPS_DEFAULT)
    }

    pub(super) fn store(c: CapWords) {
        // A failed `try_with` means the thread is shutting down and the
        // value is about to be discarded anyway, so dropping the write is
        // the correct response rather than a lost update.
        let _ = CAPS.try_with(|cell| cell.set(c));
    }
}

/// Read all three capability sets.
pub(crate) fn current_caps() -> CapWords {
    store::load()
}

/// Overwrite all three capability sets.
pub(crate) fn set_current_caps(c: CapWords) {
    store::store(c);
}

/// Read the currently-held effective capability set as (low, high).
#[must_use]
pub fn current_caps_effective() -> (u32, u32) {
    let c = store::load();
    (c.eff_lo, c.eff_hi)
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

/// The effective set as `capget()` reports it: the kernel's view, narrowed by
/// anything this process has since dropped.
///
/// Returns the raw stored words unchanged until [`kernel_view::refresh`] has
/// succeeded at least once — on a host (test) build that is never, so the
/// existing behaviour is bit-for-bit preserved there.
///
/// # Why an intersection rather than a replacement
///
/// The two words answer different questions and both are binding.  The
/// projection says *what the kernel would let this process do*; the stored
/// words say *what this process has asked to keep* (`capset` can only drop).
/// A capability is genuinely held only when both agree, so the reported set is
/// their AND.  Replacing the stored words outright would silently undo a
/// voluntary privilege drop on the next refresh — turning `capset` into a
/// suggestion — and reporting only the stored words is the fiction §312 exists
/// to remove.
fn reported_caps_effective() -> (u32, u32) {
    let (lo, hi) = current_caps_effective();
    match store::load_projection() {
        Some((plo, phi)) => (lo & plo, hi & phi),
        None => (lo, hi),
    }
}

// ---------------------------------------------------------------------------
// The kernel's view: projecting real capabilities onto Linux's words (§312)
// ---------------------------------------------------------------------------

/// Derives Linux's capability words from the capabilities the kernel actually
/// granted this process.
///
/// **In short:** libc used to invent this answer. It kept Linux's three
/// capability words in its own memory, initialised to *every bit set*, and
/// never asked the kernel anything — so `capget()` cheerfully reported full
/// privilege to a process spawned with no capabilities at all. That was safe
/// only by accident (the kernel re-checks every privileged operation itself,
/// so the lie could never *grant* anything), but it is the silent kind of
/// wrong: a port that calls `capget()` to decide what to attempt, or to drop
/// privileges it believes it has, gets a confidently false answer with no
/// error anywhere. This module replaces the invention with a derivation.
///
/// # The rule
///
/// Each Linux `CAP_*` bit is the value of a specific predicate over the
/// `(ResourceType, Rights)` pairs the process holds, and **the default is
/// deny**: a `CAP_*` with no rule is reported as *not held*, never as held.
/// Under-reporting is recoverable — the caller tries the operation and the
/// kernel decides — whereas over-reporting is the bug being fixed.
///
/// Decided in `design-decisions.md` §312 (operator; `open-questions.md` Q44).
/// The enumerating syscall this reads is lane A's `SYS_CAP_QUERY`; its ABI is
/// documented in `requests/a-b-cap-query-enumeration-landed.md`.
///
/// # Staging — the gates are still advisory
///
/// [`refresh`] feeds [`capget`] only. The 63 libc gate sites still consult the
/// stored words through [`has_capability`], which on the target still start
/// out permissive. That is deliberate: making the gates truthful in the same
/// change would break every fixture spawned with `capabilities: &[]`
/// (`services/ctest-jobctl`, `self_test_cctty`, `self_test_cpgroup` — the
/// first says so in its own doc comment), which is boot-test-visible. §312
/// step 3 flips them once the fixtures carry real capabilities; the flip is
/// pointing `has_capability` at [`reported_caps_effective`] instead of
/// [`current_caps_effective`].
pub mod kernel_view {
    use super::{
        CAP_KILL, CAP_NET_RAW, CAP_SETGID, CAP_SETUID, CAP_SYS_ADMIN, CAP_SYS_NICE, CAP_SYS_PTRACE,
        CAP_SYS_RAWIO, CAP_LAST_CAP, store,
    };

    /// Kernel `ResourceType` discriminants.
    ///
    /// Mirrors `kernel/src/cap/mod.rs`'s `#[repr(u16)] enum ResourceType`.
    /// Only the variants this module projects are listed; adding a predicate
    /// means adding its type here too.
    pub mod res {
        /// A process, for kill / wait / inspect operations.
        pub const PROCESS: u16 = 6;
        /// A thread, for suspend / resume / priority change.
        pub const THREAD: u16 = 7;
        /// I/O port access, for userspace drivers.
        pub const PORT_IO: u16 = 8;
        /// Filesystem access.
        pub const FILE: u16 = 10;
        /// I/O scheduler privilege (the Realtime priority class).
        pub const IO_SCHEDULER: u16 = 13;
        /// A process namespace.
        pub const NAMESPACE: u16 = 15;
        /// Raw network access (`AF_PACKET`, raw sockets).
        pub const NET_RAW: u16 = 24;
    }

    /// Kernel `Rights` bits.
    ///
    /// Mirrors `kernel/src/cap/rights.rs`. `Rights` is a **`u64`** — thirteen
    /// bits are defined there today, which is exactly why nothing here may
    /// narrow it to `u32`: the width is the ABI, not the current occupancy.
    ///
    /// **This mirror is partial by design, and the gap is not a TODO.** Only
    /// the bits some rule in [`project`] actually tests appear here — nine of
    /// the thirteen. A bit that is projected onto no Linux capability has
    /// nothing to say to this file, and copying it over anyway would invite
    /// the reader to assume the absent ones are unimplemented rather than
    /// simply irrelevant. Add a bit here when, and only when, a predicate
    /// starts asking about it.
    pub mod rights {
        /// Read data from the resource.
        pub const READ: u64 = 1 << 0;
        /// Write data to the resource.
        pub const WRITE: u64 = 1 << 1;
        /// Create child objects within the resource.
        pub const CREATE: u64 = 1 << 3;
        /// Modify metadata (permissions, attributes, …).
        pub const METADATA: u64 = 1 << 5;
        /// Transfer (delegate) the capability to another task.
        pub const TRANSFER: u64 = 1 << 6;
        /// Signal the resource.
        pub const SIGNAL: u64 = 1 << 9;
        /// Permission to use the Realtime I/O priority class.
        pub const IO_REALTIME: u64 = 1 << 16;
        /// Unilateral introspection authority over a process.
        pub const DEBUG: u64 = 1 << 17;
        /// Authority to change a process's own uid/gid credentials.
        ///
        /// Its own bit rather than [`METADATA`] on purpose: `METADATA` is the
        /// generic "modify an attribute" bit, so the next Process grant that
        /// reaches for it would silently confer root-capability, with the
        /// grant site in `kernel/` and this projection in `posix/` and no
        /// single diff showing both halves. See design-decisions.md §207.
        pub const SET_CREDENTIALS: u64 = 1 << 18;
    }

    /// One capability, as `SYS_CAP_QUERY` writes it.
    ///
    /// Layout is fixed by `kernel/src/cap/mod.rs::CapEntryInfo`: 24 bytes,
    /// 8-aligned. `_reserved` is not padding-by-another-name — it is written
    /// as zero and exists so the struct has *no* implicit padding, because
    /// implicit padding is uninitialised bytes crossing a trust boundary.
    ///
    /// The handle value is deliberately absent: an enumeration answers what
    /// authority exists, not which slot holds it, and a list is where a stale
    /// handle survives longest.
    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct CapEntryInfo {
        /// `ResourceType` discriminant — see [`res`].
        pub resource_type: u16,
        /// Reserved; always zero.
        pub reserved: [u16; 3],
        /// `Rights` bits — see [`rights`].
        pub rights: u64,
        /// Kernel-internal identifier of the resource. Names the object, not
        /// the caller's access to it, and is not usable as a handle.
        pub resource_id: u64,
    }

    // A mismatch here is an ABI break that would otherwise show up as
    // garbage rights bits, so fail the build instead.
    const _: () = {
        assert!(core::mem::size_of::<CapEntryInfo>() == 24);
        assert!(core::mem::align_of::<CapEntryInfo>() == 8);
    };

    /// Entries enumerated without touching the allocator.
    ///
    /// A real process holds single digits of capabilities; the kernel's table
    /// caps out at 4096, which at 24 bytes each is 96 KiB and far too much for
    /// the startup stack. So the common case is inline and the rare one falls
    /// back to `malloc`.
    const INLINE_ENTRIES: usize = 64;

    /// Bound on the probe/enumerate retry loop.
    ///
    /// The count can grow between the probe and the fetch — a thread granting
    /// itself a capability in between is enough — and the kernel answers that
    /// with `ERANGE` rather than a truncated list. Retrying re-probes, so it
    /// converges; the bound is only there so a pathological grant storm cannot
    /// spin the startup path forever.
    const MAX_ATTEMPTS: u32 = 4;

    /// Ask the kernel how many capabilities the caller holds, writing nothing.
    fn probe() -> Option<usize> {
        let n = crate::syscall::syscall2(crate::syscall::SYS_CAP_QUERY, 0, 0);
        usize::try_from(n).ok()
    }

    /// Fill `buf`; `Ok(n)` wrote `n` entries, `Err(())` means try again bigger.
    fn enumerate_into(buf: &mut [CapEntryInfo]) -> Result<usize, ()> {
        if buf.is_empty() {
            return Err(());
        }
        let n = crate::syscall::syscall2(
            crate::syscall::SYS_CAP_QUERY,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        );
        usize::try_from(n).ok().filter(|&n| n <= buf.len()).ok_or(())
    }

    /// Does the process hold any capability of type `ty`?
    fn holds(entries: &[CapEntryInfo], ty: u16) -> bool {
        entries.iter().any(|e| e.resource_type == ty)
    }

    /// Does the process hold a capability of type `ty` carrying **any** of
    /// `any_of`?
    ///
    /// Any-of rather than all-of because the predicates in §312 read that way
    /// ("any `PortIo` handle with `READ` **or** `WRITE`"), and because a
    /// single-bit mask makes the two identical anyway.
    fn holds_with(entries: &[CapEntryInfo], ty: u16, any_of: u64) -> bool {
        entries
            .iter()
            .any(|e| e.resource_type == ty && (e.rights & any_of) != 0)
    }

    /// A 64-bit capability set being assembled, as Linux's two `u32` words.
    #[derive(Clone, Copy, Default)]
    struct Mask {
        lo: u32,
        hi: u32,
    }

    impl Mask {
        fn set(&mut self, cap: u32) {
            debug_assert!(cap <= CAP_LAST_CAP);
            if cap < 32 {
                self.lo |= 1u32 << cap;
            } else if cap <= CAP_LAST_CAP {
                self.hi |= 1u32 << (cap.wrapping_sub(32));
            }
        }
    }

    /// Project a set of held capabilities onto Linux's capability words.
    ///
    /// Pure and total: the whole point of splitting it out from [`refresh`] is
    /// that the mapping can be tested exhaustively on the host, where no
    /// kernel exists to enumerate anything.
    ///
    /// Everything not named below reports **false**. That is the decision, not
    /// an omission — see the module docs.
    #[must_use]
    pub fn project(entries: &[CapEntryInfo]) -> (u32, u32) {
        let mut m = Mask::default();

        // --- the derived bits (design-decisions.md §312) ------------------

        // Port I/O is the whole of what CAP_SYS_RAWIO gates for us
        // (`sys_io.rs::ioperm`/`iopl`), and PortIo handles name it exactly.
        if holds_with(entries, res::PORT_IO, rights::READ | rights::WRITE) {
            m.set(CAP_SYS_RAWIO);
        }
        // SIGNAL on a Process is precisely "may signal that process"; kill(2)
        // to a process we hold no handle for is what CAP_KILL overrides.
        if holds_with(entries, res::PROCESS, rights::SIGNAL) {
            m.set(CAP_KILL);
        }
        // DEBUG is unilateral introspection — the target does not consent —
        // which is the authority ptrace(2) actually is.
        if holds_with(entries, res::PROCESS, rights::DEBUG) {
            m.set(CAP_SYS_PTRACE);
        }
        // Raising priority is the only direction CAP_SYS_NICE gates, and
        // IO_REALTIME is the kernel's name for permission to do so.
        if holds_with(entries, res::THREAD, rights::IO_REALTIME) {
            m.set(CAP_SYS_NICE);
        }
        // A NetRaw handle *is* raw-socket authority; there is no narrower
        // right to ask for, so type alone is the predicate.
        if holds(entries, res::NET_RAW) {
            m.set(CAP_NET_RAW);
        }
        // Changing your own uid/gid is what CAP_SETUID/CAP_SETGID gate, and
        // SET_CREDENTIALS is the kernel's name for permission to do so.
        //
        // Both caps from one predicate is deliberate: the credential model is
        // flat — one real uid, one real gid, and `SYS_PROCESS_SET_CREDENTIALS`
        // writes both in a single call — so there is no state in which a
        // process may set one and not the other. Splitting the predicate would
        // claim a distinction the kernel cannot enforce; if they ever do
        // diverge, split the *right* instead.
        //
        // Unlike SIGNAL below, this bit is never granted automatically: no
        // spawn or fork path confers it, so holding it is always a deliberate
        // act and the predicate needs no `resource_id` qualification.
        if holds_with(entries, res::PROCESS, rights::SET_CREDENTIALS) {
            m.set(CAP_SETUID);
            m.set(CAP_SETGID);
        }

        if project_sys_admin(entries) {
            m.set(CAP_SYS_ADMIN);
        }

        (m.lo, m.hi)
    }

    /// `CAP_SYS_ADMIN` — the hand-maintained union, member by member.
    ///
    /// Not derived, on purpose. `CAP_SYS_ADMIN` is Linux's junk drawer: it has
    /// no natural preimage in a per-object capability model, and it gates 21
    /// of libc's call sites that have nothing in common with each other. Any
    /// single derived rule would be either permanently false (breaking all 21)
    /// or broad enough to re-grant everything (which is the bug §312 fixes).
    /// So it is an explicit list, and every member below names the call sites
    /// it exists for.
    ///
    /// # Sites deliberately left uncovered
    ///
    /// - `sethostname` / `setdomainname` (`unistd.rs`) — system identity is
    ///   global state with no object behind it. Inventing a handle for it
    ///   would be the ambient-authority-in-a-capability-costume that §312
    ///   rejected as option B.
    /// - `seccomp` (`linux_seccomp.rs`), `landlock_add_rule` /
    ///   `landlock_restrict_self` (`linux_landlock.rs`) — these *restrict the
    ///   caller*. Linux gates them on `CAP_SYS_ADMIN` only as a stand-in for
    ///   `no_new_privs`, i.e. to stop a setuid binary confusing itself. That
    ///   is not an authority at all, so there is nothing to project; the
    ///   `no_new_privs` path needs no capability and is the one to use.
    fn project_sys_admin(entries: &[CapEntryInfo]) -> bool {
        // Namespace family: clone(CLONE_NEW*), clone3, unshare (process.rs),
        // setns (process.rs).  CREATE makes a namespace, WRITE/TRANSFER joins
        // or hands one over.
        holds_with(
            entries,
            res::NAMESPACE,
            rights::CREATE | rights::WRITE | rights::TRANSFER,
        )
        // Mount tree and on-disk administration: mount, umount, umount2
        // (process.rs), swapon, swapoff (unistd.rs), quotactl (sys_quota.rs).
        // All of them reshape the filesystem rather than read or write within
        // it, which is what METADATA on a File capability names.
        || holds_with(entries, res::FILE, rights::METADATA)
        // ioprio_set(IOPRIO_CLASS_RT) (process.rs) — the one site with an
        // exact preimage: IO_REALTIME on the I/O scheduler is literally the
        // permission being requested.
        || holds_with(entries, res::IO_SCHEDULER, rights::IO_REALTIME)
        // Cross-process observation: bpf (linux_bpf.rs), perf_event_open
        // (linux_perf_event.rs), fanotify_init (linux_fanotify.rs).  Each
        // watches processes other than the caller without their consent,
        // which is DEBUG on a Process handle.
        || holds_with(entries, res::PROCESS, rights::DEBUG)
        // madvise(MADV_HWPOISON | MADV_SOFT_OFFLINE) (mman.rs) — retiring a
        // physical page is hardware authority, not memory-management
        // authority, so it rides on raw port access rather than on any
        // memory-shaped capability.
        || holds_with(entries, res::PORT_IO, rights::WRITE)
    }

    /// Ask the kernel what this process holds and record the projection.
    ///
    /// Returns `true` if the projection was updated. `false` means the query
    /// was unavailable (a host build, where the syscall stub returns
    /// `ENOSYS`) or did not converge; in that case the previous state is left
    /// alone and `capget()` keeps reporting the stored words.
    ///
    /// Failing that way — neither granting nor denying — is right *while the
    /// gates are advisory*: an unanswered query means "we do not know", and
    /// the kernel still re-checks every real operation. §312 step 3, which
    /// makes the gates binding, has to revisit it and fail closed, because at
    /// that point "we do not know" and "you may" stop being the same thing.
    pub fn refresh() -> bool {
        let mut inline = [CapEntryInfo {
            resource_type: 0,
            reserved: [0; 3],
            rights: 0,
            resource_id: 0,
        }; INLINE_ENTRIES];

        for _ in 0..MAX_ATTEMPTS {
            let Some(count) = probe() else {
                return false; // syscall unavailable — nothing to project.
            };
            if count == 0 {
                // A real empty set, not an error: the process holds nothing,
                // so every derived bit is false.  Recording it is the point —
                // this is exactly the fixture case §312 was written about.
                store::store_projection(0, 0);
                return true;
            }

            // `None` is exactly the `count > INLINE_ENTRIES` case, so the
            // bound is expressed once, by the slice, rather than restated as
            // a comparison that could drift from the array's length.
            if let Some(slot) = inline.get_mut(..count) {
                if let Ok(n) = enumerate_into(slot) {
                    let (lo, hi) = project(slot.get(..n).unwrap_or(&[]));
                    store::store_projection(lo, hi);
                    return true;
                }
            } else if refresh_heap(count) {
                return true;
            }
            // ERANGE: the set grew under us.  Re-probe and try again.
        }
        false
    }

    /// The `count > INLINE_ENTRIES` path, kept separate so the common case
    /// never mentions the allocator.
    fn refresh_heap(count: usize) -> bool {
        let Some(bytes) = count.checked_mul(core::mem::size_of::<CapEntryInfo>()) else {
            return false;
        };
        let p = crate::malloc::malloc(bytes).cast::<CapEntryInfo>();
        if p.is_null() {
            return false;
        }
        // SAFETY: `malloc` returned `bytes` = `count * size_of::<CapEntryInfo>()`
        // usable bytes.  `CapEntryInfo` is 8-aligned and malloc's blocks are at
        // least that; the slice is written by the kernel before it is read, and
        // the type has no padding and no invalid bit patterns, so every byte
        // pattern the kernel can write is a valid value.
        let buf = unsafe { core::slice::from_raw_parts_mut(p, count) };
        let ok = match enumerate_into(buf) {
            Ok(n) => {
                let (lo, hi) = project(buf.get(..n).unwrap_or(&[]));
                store::store_projection(lo, hi);
                true
            }
            Err(()) => false,
        };
        // SAFETY: `p` came from `malloc` above, is non-null, and `buf` (the
        // only alias) is dead by here.
        unsafe { crate::malloc::free(p.cast::<u8>()) };
        ok
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

    let CapWords {
        eff_lo: _,
        eff_hi: _,
        prm_lo,
        prm_hi,
        inh_lo,
        inh_hi,
    } = current_caps();
    // The effective set is the one callers act on, so it is the one that has
    // to be true: it comes from the kernel's own answer narrowed by whatever
    // this process has dropped (§312).  Permitted and inheritable stay as
    // stored — they are libc-side bookkeeping that `capset` owns, and the
    // kernel has no corresponding notion to project them from.
    let (eff_lo, eff_hi) = reported_caps_effective();
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
    set_current_caps(CapWords {
        eff_lo: lo.effective,
        eff_hi: hi.effective,
        prm_lo: lo.permitted,
        prm_hi: hi.permitted,
        inh_lo: lo.inheritable,
        inh_hi: hi.inheritable,
    });
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod projection_tests {
    use super::kernel_view::{CapEntryInfo, project, res, rights};
    use super::*;

    /// A capability of `ty` carrying `r`.
    fn cap(ty: u16, r: u64) -> CapEntryInfo {
        CapEntryInfo {
            resource_type: ty,
            reserved: [0; 3],
            rights: r,
            resource_id: 1,
        }
    }

    /// Is `cap` set in a projected `(lo, hi)` pair?
    fn is_set((lo, hi): (u32, u32), c: u32) -> bool {
        if c < 32 {
            lo & (1u32 << c) != 0
        } else {
            hi & (1u32 << (c - 32)) != 0
        }
    }

    #[test]
    fn test_cap_entry_info_matches_kernel_abi() {
        // 24 bytes / 8-aligned is the wire format `SYS_CAP_QUERY` writes
        // (kernel/src/cap/mod.rs::CapEntryInfo, which self-tests the same
        // numbers at boot).  A mismatch would not fail loudly — it would
        // shift `rights` and `resource_id` and silently produce plausible
        // garbage, which is why this is asserted on both sides.
        assert_eq!(core::mem::size_of::<CapEntryInfo>(), 24);
        assert_eq!(core::mem::align_of::<CapEntryInfo>(), 8);
        // `rights` is a u64 because kernel `Rights` is.  Twelve bits are
        // defined today; the width is the ABI, not the occupancy.
        assert_eq!(core::mem::size_of_val(&cap(res::PROCESS, 0).rights), 8);
    }

    #[test]
    fn test_empty_capability_set_projects_to_nothing() {
        // The case §312 exists for: a process spawned `capabilities: &[]`
        // used to be told it held every capability Linux defines.
        let (lo, hi) = project(&[]);
        assert_eq!(lo, 0);
        assert_eq!(hi, 0);
    }

    #[test]
    fn test_default_is_deny_for_unmapped_caps() {
        // Holding a rich set of real capabilities must not light up a
        // `CAP_*` that has no rule.  Deny-by-default is the decision, and
        // this is the test that fails if someone "helpfully" widens a
        // predicate to cover a gate site rather than adding a rule for it.
        let held = [
            cap(res::PROCESS, rights::SIGNAL | rights::DEBUG),
            cap(res::PORT_IO, rights::READ | rights::WRITE),
            cap(res::NET_RAW, 0),
            cap(res::FILE, rights::METADATA),
            cap(res::NAMESPACE, rights::CREATE),
            cap(res::THREAD, rights::IO_REALTIME),
            cap(res::IO_SCHEDULER, rights::IO_REALTIME),
        ];
        let w = project(&held);
        for unmapped in [
            CAP_CHOWN,
            CAP_DAC_OVERRIDE,
            CAP_SETUID,
            CAP_SETGID,
            CAP_SETPCAP,
            CAP_NET_BIND_SERVICE,
            CAP_NET_ADMIN,
            CAP_IPC_LOCK,
            CAP_SYS_MODULE,
            CAP_SYS_CHROOT,
            CAP_SYS_BOOT,
            CAP_SYS_RESOURCE,
            CAP_SYS_TIME,
            CAP_MKNOD,
            CAP_SYSLOG,
            CAP_BPF,
            CAP_PERFMON,
            CAP_CHECKPOINT_RESTORE,
        ] {
            assert!(
                !is_set(w, unmapped),
                "cap {unmapped} has no rule but projected as held"
            );
        }
    }

    #[test]
    fn test_derived_bits_from_312_table() {
        assert!(is_set(
            project(&[cap(res::PORT_IO, rights::READ)]),
            CAP_SYS_RAWIO
        ));
        assert!(is_set(
            project(&[cap(res::PORT_IO, rights::WRITE)]),
            CAP_SYS_RAWIO
        ));
        assert!(is_set(
            project(&[cap(res::PROCESS, rights::SIGNAL)]),
            CAP_KILL
        ));
        assert!(is_set(
            project(&[cap(res::PROCESS, rights::DEBUG)]),
            CAP_SYS_PTRACE
        ));
        assert!(is_set(
            project(&[cap(res::THREAD, rights::IO_REALTIME)]),
            CAP_SYS_NICE
        ));
        // NetRaw is the one predicate keyed on type alone: the handle *is*
        // the authority, there is no narrower right to ask for.
        assert!(is_set(project(&[cap(res::NET_RAW, 0)]), CAP_NET_RAW));
        // Both credential caps come from the one right, because the kernel
        // writes uid and gid in a single call and cannot enforce a split.
        let w = project(&[cap(res::PROCESS, rights::SET_CREDENTIALS)]);
        assert!(is_set(w, CAP_SETUID));
        assert!(is_set(w, CAP_SETGID));
    }

    #[test]
    fn test_set_credentials_is_not_reachable_from_any_other_right() {
        // This is the one projected right that gates a *libc-side* check with
        // no kernel fallback: `SYS_PROCESS_SET_CREDENTIALS` performs no
        // capability test of its own (handlers.rs — "the cap/identity
        // permission check is performed by the userspace posix wrappers"), so
        // a predicate that fired too easily would not merely mis-report, it
        // would hand out uid 0.  Every other Process right must leave it
        // clear, and METADATA especially: choosing a dedicated bit over
        // METADATA is the whole of design-decisions.md §207, and this is the
        // test that would fail if someone later "simplified" it back.
        for r in [
            rights::READ,
            rights::WRITE,
            rights::CREATE,
            rights::METADATA,
            rights::TRANSFER,
            rights::SIGNAL,
            rights::DEBUG,
            rights::IO_REALTIME,
        ] {
            let w = project(&[cap(res::PROCESS, r)]);
            assert!(
                !is_set(w, CAP_SETUID) && !is_set(w, CAP_SETGID),
                "Process right {r:#x} projected a credential capability"
            );
        }
        // Nor from the same right on a different object type.
        for ty in [res::THREAD, res::FILE, res::NAMESPACE, res::NET_RAW] {
            let w = project(&[cap(ty, rights::SET_CREDENTIALS)]);
            assert!(
                !is_set(w, CAP_SETUID) && !is_set(w, CAP_SETGID),
                "SET_CREDENTIALS on resource type {ty} projected a credential capability"
            );
        }
    }

    #[test]
    fn test_set_credentials_does_not_join_the_sys_admin_union() {
        // CAP_SYS_ADMIN is a hand-maintained list; a new right must not slip
        // into it by resembling a member.  (Process, DEBUG) is a member, and
        // SET_CREDENTIALS sits on the same resource type.
        assert!(!is_set(
            project(&[cap(res::PROCESS, rights::SET_CREDENTIALS)]),
            CAP_SYS_ADMIN
        ));
    }

    #[test]
    fn test_rights_are_required_not_just_the_type() {
        // Holding a Process handle you may only wait on is not permission to
        // signal it, and holding a port you may not touch is not raw I/O.
        // The predicates are (type, rights) pairs precisely so that a
        // read-only handle cannot be mistaken for authority over the object.
        let inert = [
            cap(res::PROCESS, rights::READ),
            cap(res::PORT_IO, 0),
            cap(res::THREAD, rights::READ | rights::WRITE),
        ];
        let w = project(&inert);
        assert!(!is_set(w, CAP_KILL));
        assert!(!is_set(w, CAP_SYS_PTRACE));
        assert!(!is_set(w, CAP_SYS_RAWIO));
        assert!(!is_set(w, CAP_SYS_NICE));
    }

    #[test]
    fn test_predicates_do_not_leak_across_resource_types() {
        // SIGNAL on an eventfd is not CAP_KILL; IO_REALTIME on a thread is
        // not CAP_SYS_ADMIN's io-scheduler member.  Getting this wrong is
        // the easy bug — the rights bits are shared across every type, so a
        // predicate that forgets to check the type reads as true far too
        // often.
        const EVENTFD: u16 = 4;
        let w = project(&[cap(EVENTFD, rights::SIGNAL | rights::DEBUG)]);
        assert!(!is_set(w, CAP_KILL));
        assert!(!is_set(w, CAP_SYS_PTRACE));
        assert!(!is_set(w, CAP_SYS_ADMIN));

        // Thread + IO_REALTIME is CAP_SYS_NICE, and must not also satisfy
        // the IoScheduler member of the CAP_SYS_ADMIN union.
        let w = project(&[cap(res::THREAD, rights::IO_REALTIME)]);
        assert!(is_set(w, CAP_SYS_NICE));
        assert!(!is_set(w, CAP_SYS_ADMIN));
    }

    #[test]
    fn test_sys_admin_union_members() {
        // Each member on its own must suffice — the union is an OR, and a
        // member that never fires is a gate site with no way to pass.
        for member in [
            cap(res::NAMESPACE, rights::CREATE),
            cap(res::NAMESPACE, rights::WRITE),
            cap(res::NAMESPACE, rights::TRANSFER),
            cap(res::FILE, rights::METADATA),
            cap(res::IO_SCHEDULER, rights::IO_REALTIME),
            cap(res::PROCESS, rights::DEBUG),
            cap(res::PORT_IO, rights::WRITE),
        ] {
            assert!(
                is_set(project(&[member]), CAP_SYS_ADMIN),
                "union member {}/{:#x} did not grant CAP_SYS_ADMIN",
                member.resource_type,
                member.rights
            );
        }
    }

    #[test]
    fn test_sys_admin_is_not_granted_by_ordinary_file_access() {
        // The File member is METADATA — reshaping the filesystem — and must
        // not be satisfied by a process that merely holds read/write access
        // to files, which is nearly every process there is.  If this fires,
        // CAP_SYS_ADMIN is universal again and §312 has been undone.
        let w = project(&[cap(
            res::FILE,
            rights::READ | rights::WRITE | rights::CREATE,
        )]);
        assert!(!is_set(w, CAP_SYS_ADMIN));
    }

    #[test]
    fn test_projection_narrows_capget_and_capset_still_drops() {
        // The reported effective set is the AND of what the kernel grants
        // and what the process has kept.  Both halves are load-bearing:
        // without the projection `capget` reports libc's fiction, and
        // without the stored words a refresh would silently undo a
        // voluntary privilege drop.
        let saved = current_caps();
        store::clear_projection();

        // No projection yet: the stored words are reported verbatim, which
        // is the pre-§312 behaviour every existing test relies on.
        assert_eq!(reported_caps_effective(), current_caps_effective());

        // Kernel grants exactly CAP_KILL.
        let (plo, phi) = project(&[cap(res::PROCESS, rights::SIGNAL)]);
        store::store_projection(plo, phi);
        let (lo, hi) = reported_caps_effective();
        assert!(is_set((lo, hi), CAP_KILL));
        assert!(!is_set((lo, hi), CAP_SYS_RAWIO), "not granted by the kernel");

        // Now the process drops CAP_KILL itself.  Still not held.
        let mut c = current_caps();
        c.eff_lo &= !(1u32 << CAP_KILL);
        set_current_caps(c);
        assert!(!is_set(reported_caps_effective(), CAP_KILL));

        store::clear_projection();
        set_current_caps(saved);
    }

    #[test]
    fn test_refresh_on_host_is_a_no_op() {
        // The host build has no kernel: `syscall2` returns the ENOSYS
        // sentinel, so `refresh` must report failure and change nothing.
        // This is what keeps every pre-existing capability test in this
        // crate meaningful — they assert against the permissive default and
        // would be silently vacuous if the host started projecting.
        let saved = current_caps();
        store::clear_projection();
        assert!(!super::kernel_view::refresh());
        assert!(store::load_projection().is_none());
        assert_eq!(reported_caps_effective(), current_caps_effective());
        set_current_caps(saved);
    }
}

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
        set_current_caps(CAPS_DEFAULT);
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
