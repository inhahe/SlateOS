//! `<linux/perf_event.h>` — performance monitoring events.
//!
//! Provides types and constants for the `perf_event_open()` syscall
//! and the hardware/software performance counters.
//!
//! # Status
//!
//! `perf_event_open()` now performs full input validation matching
//! Linux's contract — bad attr pointer, bad size, unknown type/config
//! pairs, invalid pid/cpu combinations, unknown flag bits, and stale
//! group_fds all surface clean POSIX errnos (EFAULT/EINVAL/E2BIG/EBADF)
//! instead of "ENOSYS for everything." Once every input is valid, the
//! call still returns -1/ENOSYS because there is no kernel-side PMU
//! driver yet — but real callers (`perf record`, `perf stat`, eBPF
//! tracers, Java JIT counter profiles, the Linux `bcc` and `bpftrace`
//! toolchains) detect this exact shape and either fall back to
//! software-only timing or disable PMC-based profiling entirely,
//! matching their behavior on a Linux kernel with
//! `perf_event_paranoid = 3` or a kernel built without
//! `CONFIG_PERF_EVENTS=y`.

use crate::errno;
use crate::linux_perf_attr_types::PERF_ATTR_FLAG_EXCLUDE_KERNEL;

// ---------------------------------------------------------------------------
// perf_type_id — what is being measured
// ---------------------------------------------------------------------------

/// Hardware event.
pub const PERF_TYPE_HARDWARE: u32 = 0;
/// Software event.
pub const PERF_TYPE_SOFTWARE: u32 = 1;
/// Tracepoint event.
pub const PERF_TYPE_TRACEPOINT: u32 = 2;
/// Raw hardware cache event.
pub const PERF_TYPE_HW_CACHE: u32 = 3;
/// Raw event (CPU-specific).
pub const PERF_TYPE_RAW: u32 = 4;
/// Breakpoint event.
pub const PERF_TYPE_BREAKPOINT: u32 = 5;
/// Highest defined static perf type. Linux also accepts dynamic
/// PMU types ≥ `PERF_TYPE_MAX` if they were registered, but we
/// don't have a dynamic-PMU registry.
const PERF_TYPE_MAX: u32 = 6;

// ---------------------------------------------------------------------------
// perf_hw_id — hardware events
// ---------------------------------------------------------------------------

/// Total CPU cycles.
pub const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
/// Retired instructions.
pub const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
/// Cache accesses.
pub const PERF_COUNT_HW_CACHE_REFERENCES: u64 = 2;
/// Cache misses.
pub const PERF_COUNT_HW_CACHE_MISSES: u64 = 3;
/// Retired branch instructions.
pub const PERF_COUNT_HW_BRANCH_INSTRUCTIONS: u64 = 4;
/// Mispredicted branch instructions.
pub const PERF_COUNT_HW_BRANCH_MISSES: u64 = 5;
/// Bus cycles.
pub const PERF_COUNT_HW_BUS_CYCLES: u64 = 6;
/// Stalled frontend cycles.
pub const PERF_COUNT_HW_STALLED_CYCLES_FRONTEND: u64 = 7;
/// Stalled backend cycles.
pub const PERF_COUNT_HW_STALLED_CYCLES_BACKEND: u64 = 8;
/// Total reference cycles (not affected by frequency scaling).
pub const PERF_COUNT_HW_REF_CPU_CYCLES: u64 = 9;
/// Highest defined hardware event ID (exclusive upper bound).
const PERF_COUNT_HW_MAX: u64 = 10;

// ---------------------------------------------------------------------------
// perf_sw_ids — software events
// ---------------------------------------------------------------------------

/// CPU clock.
pub const PERF_COUNT_SW_CPU_CLOCK: u64 = 0;
/// Task clock.
pub const PERF_COUNT_SW_TASK_CLOCK: u64 = 1;
/// Page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS: u64 = 2;
/// Context switches.
pub const PERF_COUNT_SW_CONTEXT_SWITCHES: u64 = 3;
/// CPU migrations.
pub const PERF_COUNT_SW_CPU_MIGRATIONS: u64 = 4;
/// Minor page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS_MIN: u64 = 5;
/// Major page faults.
pub const PERF_COUNT_SW_PAGE_FAULTS_MAJ: u64 = 6;
/// Alignment faults.
pub const PERF_COUNT_SW_ALIGNMENT_FAULTS: u64 = 7;
/// Emulation faults.
pub const PERF_COUNT_SW_EMULATION_FAULTS: u64 = 8;
/// BPF output (Linux 4.4+).
pub const PERF_COUNT_SW_BPF_OUTPUT: u64 = 10;
/// cgroup switches (Linux 5.13+).
pub const PERF_COUNT_SW_CGROUP_SWITCHES: u64 = 11;
/// Highest defined software event ID (exclusive upper bound).
const PERF_COUNT_SW_MAX: u64 = 12;

// ---------------------------------------------------------------------------
// perf_event_open flags
// ---------------------------------------------------------------------------

/// Don't put new event in a group (matches Linux's PERF_FLAG_FD_NO_GROUP).
pub const PERF_FLAG_FD_NO_GROUP: u64 = 1 << 0;
/// Use group_fd's mmap'd ringbuffer for output.
pub const PERF_FLAG_FD_OUTPUT: u64 = 1 << 1;
/// `pid` argument is actually a cgroup file descriptor.
pub const PERF_FLAG_PID_CGROUP: u64 = 1 << 2;
/// Set close-on-exec on returned fd.
pub const PERF_FLAG_FD_CLOEXEC: u64 = 1 << 3;

/// OR of every flag bit `perf_event_open` accepts.
const PERF_FLAG_VALID: u64 =
    PERF_FLAG_FD_NO_GROUP | PERF_FLAG_FD_OUTPUT | PERF_FLAG_PID_CGROUP | PERF_FLAG_FD_CLOEXEC;

// ---------------------------------------------------------------------------
// PERF_EVENT_IOC_* ioctl commands
// ---------------------------------------------------------------------------

/// Enable a perf event.
pub const PERF_EVENT_IOC_ENABLE: u64 = 0x2400;
/// Disable a perf event.
pub const PERF_EVENT_IOC_DISABLE: u64 = 0x2401;
/// Refresh overflow count.
pub const PERF_EVENT_IOC_REFRESH: u64 = 0x2402;
/// Reset counter values.
pub const PERF_EVENT_IOC_RESET: u64 = 0x2403;
/// Set output.
pub const PERF_EVENT_IOC_SET_OUTPUT: u64 = 0x2405;
/// Set BPF program.
pub const PERF_EVENT_IOC_SET_BPF: u64 = 0x2408;

// ---------------------------------------------------------------------------
// PerfEventAttr — describes what to measure (simplified)
// ---------------------------------------------------------------------------

/// Simplified perf event attributes.
///
/// The full Linux `perf_event_attr` is ~120 bytes with many bitfields.
/// This provides the essential fields for common use cases.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PerfEventAttr {
    /// Event type (PERF_TYPE_*).
    pub type_: u32,
    /// Size of this struct.
    pub size: u32,
    /// Type-specific config (PERF_COUNT_HW_* or PERF_COUNT_SW_*).
    pub config: u64,
    /// Sample period or frequency.
    pub sample_period_or_freq: u64,
    /// Sample type bitmask.
    pub sample_type: u64,
    /// Read format bitmask.
    pub read_format: u64,
    /// Bitfield flags (disabled, inherit, pinned, exclusive, etc.).
    pub flags: u64,
    /// Wakeup events/watermark.
    pub wakeup_events_or_watermark: u32,
    /// Breakpoint type.
    pub bp_type: u32,
    /// Breakpoint address or config1.
    pub bp_addr_or_config1: u64,
    /// Breakpoint len or config2.
    pub bp_len_or_config2: u64,
    /// Branch sample type.
    pub branch_sample_type: u64,
    /// Sample registers (user).
    pub sample_regs_user: u64,
    /// Sample stack (user).
    pub sample_stack_user: u32,
    /// Clock ID.
    pub clockid: i32,
    /// Sample registers (intr).
    pub sample_regs_intr: u64,
    /// Aux watermark.
    pub aux_watermark: u32,
    /// Sample max stack.
    pub sample_max_stack: u16,
    /// Reserved.
    pub _reserved_2: u16,
}

impl PerfEventAttr {
    /// Create a zeroed `PerfEventAttr` with `size` set correctly.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: `PerfEventAttr` is a plain-old-data POD layout; the
        // all-zero bit pattern is a valid instance.
        let mut attr: Self = unsafe { core::mem::zeroed() };
        attr.size = core::mem::size_of::<Self>() as u32;
        attr
    }
}

impl Default for PerfEventAttr {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Linux PERF_ATTR_SIZE_VER constants (smallest historically-shipped sizes)
// ---------------------------------------------------------------------------

/// Linux's smallest perf_event_attr size that we accept — anything
/// smaller means the caller is targeting a kernel older than 2.6.32,
/// which we don't support. Matches Linux's `PERF_ATTR_SIZE_VER0 = 64`.
const PERF_ATTR_SIZE_MIN: u32 = 64;
/// Sanity cap on attr size. Linux uses `PAGE_SIZE`. We use 4096 (the
/// smallest reasonable page).
const PERF_ATTR_SIZE_MAX: u32 = 4096;

/// Sentinel passed as `pid` to mean "any process" (whole-cpu mode).
const PERF_PID_ANY: i32 = -1;
/// Sentinel passed as `pid` to mean "the calling process".
/// Documented for future use; not referenced in current validation logic
/// because pid==0 is a normal "specific pid" case as far as our stub is
/// concerned (we don't yet attach to a real task, so we treat it like
/// any other non-negative pid).
#[allow(dead_code)]
const PERF_PID_SELF: i32 = 0;
/// Sentinel passed as `cpu` to mean "any cpu" (per-process mode).
const PERF_CPU_ANY: i32 = -1;
/// Sanity cap on cpu argument. Linux uses `num_possible_cpus()`; we
/// use a generous 1024 to mirror the upstream `CPU_SETSIZE`.
const PERF_CPU_MAX: i32 = 1024;
/// Sentinel passed as `group_fd` to mean "no group" (event is its own group leader).
const PERF_GROUP_FD_NONE: i32 = -1;

/// Bit position of `exclude_kernel` in `PerfEventAttr.flags`.
///
/// The Linux uapi `perf_event_attr` is a packed bitfield whose first
/// six 1-bit slots are: `disabled`, `inherit`, `pinned`, `exclusive`,
/// `exclude_user`, `exclude_kernel`.  Bit 5 is therefore
/// `exclude_kernel`.

// ---------------------------------------------------------------------------
// Validators
// ---------------------------------------------------------------------------

/// Validate a `PerfEventAttr` shape.
fn validate_attr(attr: &PerfEventAttr) -> Result<(), i32> {
    // Size must be in the documented version range. Linux returns
    // E2BIG when size exceeds PAGE_SIZE (so callers know the kernel
    // is older than the struct they shipped with) and EINVAL when
    // size is below the smallest ever-shipped version.
    if attr.size < PERF_ATTR_SIZE_MIN {
        return Err(errno::EINVAL);
    }
    if attr.size > PERF_ATTR_SIZE_MAX {
        return Err(errno::E2BIG);
    }

    // Type must be a known top-level perf class.
    if attr.type_ >= PERF_TYPE_MAX {
        return Err(errno::EINVAL);
    }

    // Type-specific config validation.
    match attr.type_ {
        PERF_TYPE_HARDWARE => {
            if attr.config >= PERF_COUNT_HW_MAX {
                return Err(errno::EINVAL);
            }
        }
        PERF_TYPE_SOFTWARE => {
            // Software events have a sparse layout: 0..=8 are
            // sequential, then 10 (BPF_OUTPUT) and 11 (CGROUP_SWITCHES).
            // 9 was never assigned. Anything beyond CGROUP_SWITCHES is
            // unknown.
            if attr.config >= PERF_COUNT_SW_MAX || attr.config == 9 {
                return Err(errno::EINVAL);
            }
        }
        PERF_TYPE_HW_CACHE => {
            // Encoded as cache_id | (op_id << 8) | (result_id << 16).
            // Linux accepts any 24-bit value but the kernel later
            // returns ENOENT for unsupported combinations. We do the
            // shape check only — bits above the 24-bit window are
            // invalid.
            if (attr.config >> 24) != 0 {
                return Err(errno::EINVAL);
            }
        }
        PERF_TYPE_TRACEPOINT | PERF_TYPE_RAW | PERF_TYPE_BREAKPOINT => {
            // Linux doesn't pre-validate config for these — the
            // tracepoint id / raw event code / breakpoint config is
            // handed off to the PMU. We follow the same approach.
        }
        _ => unreachable!(), // already vetted by type_ range check above
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// perf_event_open syscall
// ---------------------------------------------------------------------------

/// Open a performance monitoring event.
///
/// Returns `-1` on failure with errno set.
///
/// # Linux semantics
///
/// `SYSCALL_DEFINE5(perf_event_open, ...)` in `kernel/events/core.c`
/// has this prologue:
///
/// 1. `if (flags & ~PERF_FLAG_ALL) return -EINVAL;` — runs first, even
///    before `perf_copy_attr()` touches the userspace pointer.
/// 2. `perf_copy_attr(attr_uptr, &attr)` — runs `get_user(size,
///    &uattr->size)` which yields `-EFAULT` if `attr_uptr` is NULL,
///    then validates `size` (`-EINVAL` if too small, `-E2BIG` if
///    larger than `PAGE_SIZE`), then copies the body.
/// 3. type/config validation, pid/cpu range checks, group_fd lookup.
/// 4. `if (!attr.exclude_kernel) perf_allow_kernel(&attr)` — the
///    `CAP_PERFMON` / `perf_event_paranoid` gate.  Returns `-EACCES`
///    when `sysctl_perf_event_paranoid > 1 && !perfmon_capable()`.
///    Note Linux uses **EACCES**, not EPERM, for this rejection —
///    it's a "policy denies you" not a "you can't perform this
///    operation" signal, and the perf tooling distinguishes them.
///
/// Phase 181 wires the privilege step into our implementation.  Before
/// Phase 181 we skipped it ("no privilege model"); now we have one
/// (`sys_capability::has_capability`) and we model
/// `perf_event_paranoid > 1` (the strictest setting) so the gate
/// fires for every kernel-space measurement without CAP_PERFMON.
///
/// Phase 129 reorders our checks to match steps 1→3→4 exactly. The
/// previous arrangement (`attr` NULL check first, then `validate_attr`,
/// then flags) was wrong: a caller passing both `attr = NULL` and
/// `flags = 0xdeadbeef` saw `EFAULT` here but `EINVAL` on real Linux.
/// Probing tools (libpfm, libperf, perf-tool's `perf list`) inspect
/// errno to decide whether to retry with a smaller flag set or to
/// give up entirely, so the wrong errno would mislead them.
///
/// # Errors
///
/// * `EINVAL` — unknown flag bit (checked first, matching Linux);
///   then bad `attr.size` (too small), bad `attr.type_`, bad config
///   for the type, bad pid/cpu combination, bad pid value (< -1),
///   bad cpu value (out of range), or `PID_CGROUP` flag with pid < 0.
/// * `EFAULT` — `attr` is NULL (checked after flags).
/// * `E2BIG` — `attr.size` larger than what the kernel knows (the
///   caller is from the future).
/// * `EBADF` — `group_fd` is non-negative but isn't a known perf fd
///   (since we don't allocate perf fds, this catches every positive
///   `group_fd`).
/// * `EACCES` — caller wants kernel-space events (i.e.
///   `attr.exclude_kernel == 0`) but lacks both `CAP_PERFMON` and
///   `CAP_SYS_ADMIN`.  Matches Linux's `perf_allow_kernel` under
///   `perf_event_paranoid > 1`.
/// * `ENOSYS` — everything valid and privilege held, but the kernel
///   has no PMU driver yet. Real callers treat this identically to a
///   Linux kernel built without `CONFIG_PERF_EVENTS=y`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn perf_event_open(
    attr: *mut PerfEventAttr,
    pid: i32,
    cpu: i32,
    group_fd: i32,
    flags: u64,
) -> i32 {
    // Linux's perf_event_open checks the flag mask *first*, before
    // calling perf_copy_attr (which is where NULL-attr EFAULT would
    // be observed). Mirror that ordering exactly: a caller passing
    // both bad flags and a NULL attr sees EINVAL, just like Linux.
    if (flags & !PERF_FLAG_VALID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if attr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // SAFETY: caller promises `attr` points to a readable
    // `PerfEventAttr`. We use `read_unaligned` so a misaligned attr
    // doesn't UB; the read is local and we don't dereference the
    // pointer again.
    let attr_val = unsafe { core::ptr::read_unaligned(attr) };

    if let Err(e) = validate_attr(&attr_val) {
        errno::set_errno(e);
        return -1;
    }

    // Phase 181: privilege check.  Linux's `perf_event_open` calls
    // `perf_allow_kernel(&attr)` after `perf_copy_attr` and the type/
    // config validators *and before* the pid/cpu/group_fd lookups —
    // see `kernel/events/core.c::SYSCALL_DEFINE5(perf_event_open)`.
    // It returns `-EACCES` (NOT `-EPERM`) when:
    //
    //     sysctl_perf_event_paranoid > 1 && !perfmon_capable()
    //
    // (`perfmon_capable() == capable(CAP_PERFMON) ||
    // capable(CAP_SYS_ADMIN)`).
    //
    // The check only fires for events that measure kernel-space —
    // `attr.exclude_kernel == 0`.  A caller asking exclusively for
    // user-mode counters never trips the gate, which matches Linux's
    // behaviour on a `perf_event_paranoid = 2` system (Debian/Ubuntu
    // default): unprivileged users can profile their own user-mode
    // code, but reading kernel PMCs requires CAP_PERFMON.
    //
    // We do not model `perf_event_paranoid` (would need a sysctl
    // backend); we assume the strictest setting (`> 1`) so the cap
    // gate fires whenever a kernel-space event is requested without
    // CAP_PERFMON/CAP_SYS_ADMIN.  Real Linux callers (`perf record`
    // without `-e user`, eBPF tracers, JFR, libpfm probes) handle
    // EACCES by either re-running with `--exclude-kernel`, retrying
    // with reduced privilege, or surfacing a clear "missing
    // CAP_PERFMON" diagnostic — all of which are now reachable.
    //
    // Placement note: this is *before* the pid/cpu/group_fd checks
    // so a no-cap caller passing bad pid/cpu sees EACCES first —
    // matching Linux's source-order behaviour.
    if (attr_val.flags & PERF_ATTR_FLAG_EXCLUDE_KERNEL) == 0
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_PERFMON)
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_SYS_ADMIN)
    {
        errno::set_errno(errno::EACCES);
        return -1;
    }

    // pid validation.
    // -1 = any process, 0 = self, > 0 = specific process, < -1 = bad.
    if pid < PERF_PID_ANY {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // PID_CGROUP flag re-interprets `pid` as a cgroup fd, which must
    // be non-negative.
    if (flags & PERF_FLAG_PID_CGROUP) != 0 && pid < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // cpu validation: -1 = any cpu, 0..PERF_CPU_MAX = specific cpu.
    if !(PERF_CPU_ANY..PERF_CPU_MAX).contains(&cpu) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // pid == -1 AND cpu == -1 is meaningless (no anchor) — Linux
    // EINVAL. Cgroup mode is excepted because the cgroup itself
    // anchors the event.
    if pid == PERF_PID_ANY && cpu == PERF_CPU_ANY && (flags & PERF_FLAG_PID_CGROUP) == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // group_fd: -1 = leader-of-own-group, >= 0 must be a real perf
    // fd. We have no perf fd table, so any non-negative group_fd is
    // EBADF. FLAG_FD_OUTPUT and FLAG_FD_NO_GROUP both require
    // group_fd >= 0 to be meaningful — if a caller sets those with
    // group_fd == -1, Linux EINVAL.
    if group_fd < PERF_GROUP_FD_NONE {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    if group_fd == PERF_GROUP_FD_NONE
        && (flags & (PERF_FLAG_FD_OUTPUT | PERF_FLAG_FD_NO_GROUP)) != 0
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if group_fd >= 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    // All inputs valid and privilege held — but we have no kernel-side
    // PMU driver to hand back a counter fd. Real callers (`perf
    // record`, `perf stat`, JFR-on-Linux, the BPF observability stack)
    // detect this and fall back to software-only timing, just like on
    // a kernel built without CONFIG_PERF_EVENTS.
    let _ = pid;
    let _ = cpu;
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Constant invariants
    // -----------------------------------------------------------------

    #[test]
    fn test_perf_types_distinct() {
        let types = [
            PERF_TYPE_HARDWARE,
            PERF_TYPE_SOFTWARE,
            PERF_TYPE_TRACEPOINT,
            PERF_TYPE_HW_CACHE,
            PERF_TYPE_RAW,
            PERF_TYPE_BREAKPOINT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
        assert_eq!(PERF_TYPE_MAX, 6);
    }

    #[test]
    fn test_hw_events_sequential() {
        assert_eq!(PERF_COUNT_HW_CPU_CYCLES, 0);
        assert_eq!(PERF_COUNT_HW_INSTRUCTIONS, 1);
        assert_eq!(PERF_COUNT_HW_REF_CPU_CYCLES, 9);
        assert_eq!(PERF_COUNT_HW_MAX, 10);
    }

    #[test]
    fn test_sw_events_sequential() {
        assert_eq!(PERF_COUNT_SW_CPU_CLOCK, 0);
        assert_eq!(PERF_COUNT_SW_EMULATION_FAULTS, 8);
        // 9 is the historical hole.
        assert_eq!(PERF_COUNT_SW_BPF_OUTPUT, 10);
        assert_eq!(PERF_COUNT_SW_CGROUP_SWITCHES, 11);
        assert_eq!(PERF_COUNT_SW_MAX, 12);
    }

    #[test]
    fn test_perf_event_attr_new() {
        let attr = PerfEventAttr::new();
        assert_eq!(attr.type_, 0);
        assert_eq!(attr.size as usize, core::mem::size_of::<PerfEventAttr>());
        assert_eq!(attr.config, 0);
    }

    #[test]
    fn test_perf_event_attr_default() {
        let attr: PerfEventAttr = Default::default();
        assert_eq!(attr.type_, 0);
        assert_eq!(attr.size as usize, core::mem::size_of::<PerfEventAttr>());
    }

    #[test]
    fn test_flags_are_bits() {
        let combined = PERF_FLAG_FD_NO_GROUP
            | PERF_FLAG_FD_OUTPUT
            | PERF_FLAG_PID_CGROUP
            | PERF_FLAG_FD_CLOEXEC;
        assert_eq!(combined, 0x0F);
    }

    #[test]
    fn test_ioc_commands_distinct() {
        let cmds = [
            PERF_EVENT_IOC_ENABLE,
            PERF_EVENT_IOC_DISABLE,
            PERF_EVENT_IOC_REFRESH,
            PERF_EVENT_IOC_RESET,
            PERF_EVENT_IOC_SET_OUTPUT,
            PERF_EVENT_IOC_SET_BPF,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    // -----------------------------------------------------------------
    // Helpers used by tests
    // -----------------------------------------------------------------

    fn make_valid_hw_attr() -> PerfEventAttr {
        let mut attr = PerfEventAttr::new();
        attr.type_ = PERF_TYPE_HARDWARE;
        attr.config = PERF_COUNT_HW_CPU_CYCLES;
        attr
    }

    fn make_valid_sw_attr() -> PerfEventAttr {
        let mut attr = PerfEventAttr::new();
        attr.type_ = PERF_TYPE_SOFTWARE;
        attr.config = PERF_COUNT_SW_CPU_CLOCK;
        attr
    }

    // -----------------------------------------------------------------
    // attr pointer / size validation
    // -----------------------------------------------------------------

    #[test]
    fn test_null_attr_efault() {
        errno::set_errno(0);
        let ret = perf_event_open(core::ptr::null_mut(), 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_attr_size_below_min_einval() {
        let mut attr = make_valid_hw_attr();
        attr.size = PERF_ATTR_SIZE_MIN - 1;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_attr_size_at_min_ok() {
        let mut attr = make_valid_hw_attr();
        attr.size = PERF_ATTR_SIZE_MIN;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_attr_size_above_max_e2big() {
        let mut attr = make_valid_hw_attr();
        attr.size = PERF_ATTR_SIZE_MAX + 1;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    // -----------------------------------------------------------------
    // type / config validation
    // -----------------------------------------------------------------

    #[test]
    fn test_unknown_type_einval() {
        let mut attr = make_valid_hw_attr();
        attr.type_ = PERF_TYPE_MAX;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_hw_config_out_of_range_einval() {
        let mut attr = make_valid_hw_attr();
        attr.config = PERF_COUNT_HW_MAX;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_hw_config_max_minus_one_ok() {
        let mut attr = make_valid_hw_attr();
        attr.config = PERF_COUNT_HW_MAX - 1;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_sw_config_hole_9_einval() {
        let mut attr = make_valid_sw_attr();
        attr.config = 9; // historical hole
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sw_config_out_of_range_einval() {
        let mut attr = make_valid_sw_attr();
        attr.config = PERF_COUNT_SW_MAX;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_sw_bpf_output_ok() {
        let mut attr = make_valid_sw_attr();
        attr.config = PERF_COUNT_SW_BPF_OUTPUT;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_hw_cache_high_bits_einval() {
        let mut attr = make_valid_hw_attr();
        attr.type_ = PERF_TYPE_HW_CACHE;
        attr.config = 1u64 << 24; // bit 24 set — out of the 24-bit window
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_hw_cache_low_24_bits_ok() {
        let mut attr = make_valid_hw_attr();
        attr.type_ = PERF_TYPE_HW_CACHE;
        attr.config = 0x0011_2233;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_breakpoint_arbitrary_config_ok() {
        // PERF_TYPE_BREAKPOINT skips config validation.
        let mut attr = make_valid_hw_attr();
        attr.type_ = PERF_TYPE_BREAKPOINT;
        attr.config = 0xDEAD_BEEF_CAFE_BABE;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // flags validation
    // -----------------------------------------------------------------

    #[test]
    fn test_unknown_flag_bit_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 1 << 16);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_cloexec_flag_ok() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, PERF_FLAG_FD_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // pid validation
    // -----------------------------------------------------------------

    #[test]
    fn test_pid_minus_2_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, -2, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_pid_self_ok() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_pid_specific_ok() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 12345, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_pid_cgroup_with_pid_negative_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, -1, 0, -1, PERF_FLAG_PID_CGROUP);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_pid_cgroup_with_pid_positive_ok() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        // pid here is reinterpreted as a cgroup fd.
        let ret = perf_event_open(&mut attr, 5, 0, -1, PERF_FLAG_PID_CGROUP);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // cpu validation
    // -----------------------------------------------------------------

    #[test]
    fn test_cpu_minus_2_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, -2, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_cpu_too_large_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, PERF_CPU_MAX, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_cpu_max_minus_one_ok() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, PERF_CPU_MAX - 1, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_pid_any_cpu_any_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, -1, -1, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_pid_any_cpu_any_with_cgroup_ok() {
        // CGROUP-mode events anchor on the cgroup, not pid/cpu.
        // Linux accepts pid==-1, cpu==-1 here.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 5, -1, -1, PERF_FLAG_PID_CGROUP);
        // pid=5 (cgroup fd), cpu=-1 (any), still valid for cgroup mode.
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // group_fd validation
    // -----------------------------------------------------------------

    #[test]
    fn test_group_fd_minus_2_ebadf() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -2, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_group_fd_positive_ebadf() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, 7, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_fd_output_without_group_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, PERF_FLAG_FD_OUTPUT);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_fd_no_group_without_group_einval() {
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, PERF_FLAG_FD_NO_GROUP);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // Workflow
    // -----------------------------------------------------------------

    #[test]
    fn test_typical_perf_stat_workflow() {
        // `perf stat -e cycles` calls perf_event_open with:
        //   type = HARDWARE, config = CPU_CYCLES, pid = -1 (any), cpu = 0,
        //   group_fd = -1, flags = FD_CLOEXEC.
        let mut attr = PerfEventAttr::new();
        attr.type_ = PERF_TYPE_HARDWARE;
        attr.config = PERF_COUNT_HW_CPU_CYCLES;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, -1, 0, -1, PERF_FLAG_FD_CLOEXEC);
        assert_eq!(ret, -1);
        // Real callers detect ENOSYS as "kernel has no PMU driver" and
        // fall back to clock_gettime-based timing.
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_misaligned_attr_pointer_handled() {
        // Build a byte buffer big enough to hold a PerfEventAttr and
        // offset by 1 to guarantee misalignment.
        let mut buf: [u8; core::mem::size_of::<PerfEventAttr>() + 8] =
            [0; core::mem::size_of::<PerfEventAttr>() + 8];
        // Stamp a valid hardware attr at offset 1.
        let template = make_valid_hw_attr();
        let template_bytes = unsafe {
            core::slice::from_raw_parts(
                (&template as *const PerfEventAttr).cast::<u8>(),
                core::mem::size_of::<PerfEventAttr>(),
            )
        };
        for (i, &b) in template_bytes.iter().enumerate() {
            buf[i + 1] = b;
        }
        let misaligned = unsafe { buf.as_mut_ptr().add(1) }.cast::<PerfEventAttr>();
        errno::set_errno(0);
        let ret = perf_event_open(misaligned, 0, 0, -1, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Phase 129 — perf_event_open flag-mask precedence parity with Linux
    //
    // Linux's `SYSCALL_DEFINE5(perf_event_open, ...)` in
    // `kernel/events/core.c` runs the flag-mask check before anything
    // else, even before `perf_copy_attr` touches the userspace
    // pointer:
    //
    //     if (flags & ~PERF_FLAG_ALL)
    //         return -EINVAL;
    //     ...
    //     err = perf_copy_attr(attr_uptr, &attr);  // EFAULT here for NULL
    //
    // Phase 129 reorders our impl so the same precedence holds. Tools
    // like libperf, libpfm4 and the standalone `perf` binary feed
    // bad-shaped attrs / flags during feature probing; they branch
    // on errno to decide what to retry, so the wrong code (EFAULT vs
    // EINVAL) sends them down the wrong path.
    // -----------------------------------------------------------------

    #[test]
    fn test_perf_event_open_phase129_einval_wins_over_efault() {
        // Both bad flags AND NULL attr: Linux checks flags first,
        // returns -EINVAL. We now match.
        errno::set_errno(0);
        let ret = perf_event_open(core::ptr::null_mut(), 0, 0, -1, 1 << 16);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_einval_wins_over_size_einval() {
        // Bad flags AND attr.size below PERF_ATTR_SIZE_MIN: flag check
        // wins (the kernel never reaches perf_copy_attr).
        let mut attr = make_valid_hw_attr();
        attr.size = 0;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 1 << 17);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_einval_wins_over_size_e2big() {
        // Bad flags AND attr.size above PERF_ATTR_SIZE_MAX: flag check
        // wins. Without the reorder the caller saw E2BIG.
        let mut attr = make_valid_hw_attr();
        attr.size = PERF_ATTR_SIZE_MAX + 1;
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 1 << 18);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_einval_wins_over_pid_einval() {
        // Bad flags AND pid < -1: flag check wins. The pid-range
        // check should never be reached.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, -2, 0, -1, 1 << 19);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_einval_wins_over_cpu_einval() {
        // Bad flags AND cpu out of range: flag check wins.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, PERF_CPU_MAX, -1, 1 << 20);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_einval_wins_over_ebadf() {
        // Bad flags AND group_fd >= 0: flag check wins. Without the
        // reorder this would have reached the group_fd >= 0 EBADF
        // branch (after attr validation succeeded).
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, 5, 1 << 21);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_high_bit_unknown_flag_einval() {
        // Sign bit (1 << 63) is an unknown flag bit -> EINVAL.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 1u64 << 63);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_all_ones_flags_einval() {
        // u64::MAX -> EINVAL (unknown bits dominate). Confirms we
        // don't accidentally accept all-ones as a wildcard.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_compatible_known_flags_reach_enosys() {
        // PERF_FLAG_FD_CLOEXEC | PERF_FLAG_PID_CGROUP is the subset
        // of valid flags that doesn't require `group_fd >= 0`
        // (FD_OUTPUT and FD_NO_GROUP both demand a group fd, so
        // combining them with group_fd=-1 is its own EINVAL — that
        // case is exercised separately).  With pid=0 (valid cgroup
        // fd shape), cpu=0, group_fd=-1, this should pass every
        // validation step and reach the ENOSYS stub.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(
            &mut attr,
            0,
            0,
            -1,
            PERF_FLAG_FD_CLOEXEC | PERF_FLAG_PID_CGROUP,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_perf_event_open_phase129_null_attr_with_clean_flags_still_efault() {
        // Sanity: after the reorder, the NULL-attr case with clean
        // flags still reports EFAULT (we didn't drop that branch).
        errno::set_errno(0);
        let ret = perf_event_open(core::ptr::null_mut(), 0, 0, -1, PERF_FLAG_FD_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_perf_event_open_phase129_libperf_feature_probe_workflow() {
        // libperf's feature-detection sequence:
        //   1. Try perf_event_open with FD_CLOEXEC. Linux 3.14+
        //      accepts; older returns EINVAL.
        //   2. On EINVAL, retry without FD_CLOEXEC.
        // Our shim: FD_CLOEXEC is in the valid mask, so step 1
        // proceeds to ENOSYS — libperf sees "kernel supports the
        // flag but not the syscall," which is the truthful answer.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, PERF_FLAG_FD_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_perf_event_open_phase129_recovery_after_flag_einval() {
        // After a flag-mask EINVAL, a clean call still produces
        // ENOSYS (errno is rewritten, not sticky).
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let r1 = perf_event_open(&mut attr, 0, 0, -1, 1 << 30);
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let r2 = perf_event_open(&mut attr, 0, 0, -1, 0);
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_perf_event_open_phase129_buggy_caller_passes_negative_one_flags() {
        // C caller doing `perf_event_open(&attr, 0, 0, -1, -1ULL)`
        // (which casts to u64::MAX) hits the unknown-bit check first
        // and gets EINVAL — same as Linux. Confirms negative
        // sign-extended flag arg is handled correctly.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, !0u64);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_perf_event_open_phase129_first_invalid_bit_above_mask_einval() {
        // 1 << 4 is the first bit just past PERF_FLAG_FD_CLOEXEC
        // (=1 << 3) and is unused by Linux as of 6.x -> EINVAL.
        let mut attr = make_valid_hw_attr();
        errno::set_errno(0);
        let ret = perf_event_open(&mut attr, 0, 0, -1, 1 << 4);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // ----------------------------------------------------------------------
    // Phase 181: perf_event_open — CAP_PERFMON (or CAP_SYS_ADMIN) gate
    // on kernel-space measurements.
    //
    // Pre-Phase-181 behaviour: every well-formed perf_event_open call
    // fell through to ENOSYS regardless of capability, because the
    // docstring's privilege step ("we have no privilege model") was
    // unimplemented.  That let unprivileged code probe kernel-PMC
    // access without ever seeing EACCES, which:
    //   - misleads tools like `perf record` and `bpftrace` that
    //     inspect errno to decide whether to retry with
    //     `--exclude-kernel` (they now retry on EACCES, but our stub
    //     gave ENOSYS so they gave up entirely);
    //   - hides the capability requirement from the audit layer
    //     (a sandboxed callee can no longer be distinguished from
    //     "kernel built without CONFIG_PERF_EVENTS=y" until it tries
    //     a forbidden flag).
    //
    // Linux semantics (kernel/events/core.c::perf_event_open):
    //     if (!attr.exclude_kernel) {
    //         err = perf_allow_kernel(&attr);
    //         if (err) return err;        // -EACCES
    //     }
    // where perf_allow_kernel returns -EACCES when
    //     sysctl_perf_event_paranoid > 1 && !perfmon_capable()
    // and perfmon_capable() ::= capable(CAP_PERFMON) ||
    //                            capable(CAP_SYS_ADMIN).
    //
    // Implementation: assume paranoid > 1 (strictest setting); gate
    // every kernel-space event behind CAP_PERFMON OR CAP_SYS_ADMIN.
    // Either capability suffices, matching Linux.  Errno is EACCES,
    // not EPERM.  Placement is after attr/flags validation but
    // before pid/cpu/group_fd validation — matching Linux source
    // order — so a no-cap caller passing bad pid sees EACCES first.
    // ----------------------------------------------------------------------

    mod perf_event_open_cap_phase181 {
        use super::*;

        /// Snapshot/restore-on-drop guard — same pattern as Phase 180.
        struct CapGuard {
            lo: u32,
            hi: u32,
        }
        impl CapGuard {
            fn snapshot() -> Self {
                let (lo, hi) = crate::sys_capability::current_caps_effective();
                Self { lo, hi }
            }
        }
        impl Drop for CapGuard {
            fn drop(&mut self) {
                let mut hdr = crate::sys_capability::CapUserHeader {
                    version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                    pid: 0,
                };
                let data = [
                    crate::sys_capability::CapUserData {
                        effective: self.lo,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                    crate::sys_capability::CapUserData {
                        effective: self.hi,
                        permitted: u32::MAX,
                        inheritable: 0,
                    },
                ];
                let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            }
        }

        fn drop_cap(cap: u32) {
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            let (new_lo, new_hi) = if cap < 32 {
                (lo & !(1u32 << cap), hi)
            } else {
                (lo, hi & !(1u32 << (cap - 32)))
            };
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: new_lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: new_hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
            assert_eq!(rc, 0, "capset must succeed when dropping cap");
            assert!(!crate::sys_capability::has_capability(cap));
        }

        fn drop_cap_perfmon_and_sys_admin() {
            drop_cap(crate::sys_capability::CAP_PERFMON);
            drop_cap(crate::sys_capability::CAP_SYS_ADMIN);
        }

        // -- Per-error-class ----------------------------------------------

        /// Kernel-mode event (exclude_kernel == 0, the default) with
        /// neither CAP_PERFMON nor CAP_SYS_ADMIN held → -1/EACCES.
        #[test]
        fn test_perf_phase181_kernel_event_no_cap_returns_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        /// User-only event (exclude_kernel == 1) passes the cap gate
        /// even without CAP_PERFMON — same as Linux under
        /// `perf_event_paranoid = 2`.  Falls through to ENOSYS
        /// (no PMU backend).
        #[test]
        fn test_perf_phase181_user_only_event_no_cap_succeeds_to_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            attr.flags = PERF_ATTR_FLAG_EXCLUDE_KERNEL;
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Errno is specifically EACCES (Linux's chosen errno for
        /// `perf_allow_kernel`), not EPERM (the generic-cap errno) and
        /// not ENOSYS (the backend signal).
        #[test]
        fn test_perf_phase181_errno_is_eacces_not_eperm() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_ne!(errno::get_errno(), errno::EPERM);
            assert_ne!(errno::get_errno(), errno::ENOSYS);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        /// Holding CAP_SYS_ADMIN alone (without CAP_PERFMON) is
        /// sufficient — matches `perfmon_capable() = CAP_PERFMON ||
        /// CAP_SYS_ADMIN`.  CAP_SYS_ADMIN is the historical
        /// "superuser" cap; CAP_PERFMON (Linux 5.8+) is the modern
        /// fine-grained alternative.
        #[test]
        fn test_perf_phase181_sys_admin_alone_satisfies_gate() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_PERFMON);
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_PERFMON
            ));
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Holding CAP_PERFMON alone is sufficient — confirms the
        /// gate is OR'd, not AND'd, across the two caps.
        #[test]
        fn test_perf_phase181_perfmon_alone_satisfies_gate() {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_SYS_ADMIN);
            assert!(crate::sys_capability::has_capability(
                crate::sys_capability::CAP_PERFMON
            ));
            assert!(!crate::sys_capability::has_capability(
                crate::sys_capability::CAP_SYS_ADMIN
            ));
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Dropping BOTH CAP_PERFMON and CAP_SYS_ADMIN denies — the
        /// only path to ENOSYS is via one of those caps.
        #[test]
        fn test_perf_phase181_drop_both_caps_denies() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        // -- Ordering matrix ----------------------------------------------

        /// Bad flag bits beat EACCES.  A no-cap caller passing
        /// `flags = 1 << 30` sees EINVAL — confirms the flag mask
        /// check runs before perf_allow_kernel (matching Linux's
        /// source-order: SYSCALL_DEFINE5 prologue checks flags first).
        #[test]
        fn test_perf_phase181_einval_flags_beats_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 1 << 30);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// NULL attr beats EACCES.  Linux's perf_copy_attr runs
        /// `get_user(size, &uattr->size)` first, which faults on NULL
        /// — that's before perf_allow_kernel.
        #[test]
        fn test_perf_phase181_efault_null_attr_beats_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            errno::set_errno(0);
            let r = perf_event_open(core::ptr::null_mut(), 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EFAULT);
        }

        /// Bad attr.size (too small) beats EACCES.  validate_attr
        /// runs before perf_allow_kernel.
        #[test]
        fn test_perf_phase181_einval_bad_size_beats_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            attr.size = 1; // < PERF_ATTR_SIZE_MIN
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        /// E2BIG (oversize attr) also beats EACCES — same reason.
        #[test]
        fn test_perf_phase181_e2big_oversize_attr_beats_eacces() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            attr.size = 8192; // > PERF_ATTR_SIZE_MAX (4096)
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::E2BIG);
        }

        /// EACCES beats EINVAL for bad pid.  Linux runs
        /// perf_allow_kernel BEFORE pid validation, so a no-cap caller
        /// passing pid = -42 (a bad pid) sees EACCES, not EINVAL.
        #[test]
        fn test_perf_phase181_eacces_beats_einval_bad_pid() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, -42, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        /// EACCES beats EBADF for stale group_fd.  Same placement
        /// argument — group_fd lookup is after perf_allow_kernel.
        #[test]
        fn test_perf_phase181_eacces_beats_ebadf_stale_group_fd() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, 999, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        // -- Workflow -----------------------------------------------------

        /// `perf record` retry workflow: first call requests kernel
        /// events without CAP_PERFMON (EACCES); caller responds by
        /// setting exclude_kernel and retrying (ENOSYS — backend
        /// missing, which `perf record` then handles by falling
        /// back to software-only).
        #[test]
        fn test_perf_phase181_workflow_perf_record_retry_with_exclude_kernel() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            // 1st: default attr → EACCES.
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            // 2nd: same attr but exclude_kernel set → ENOSYS.
            attr.flags |= PERF_ATTR_FLAG_EXCLUDE_KERNEL;
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        /// Sandbox workflow: a privileged daemon spawns a child that
        /// drops CAP_PERFMON and CAP_SYS_ADMIN before exec'ing
        /// untrusted code.  The child can no longer open kernel
        /// events even though the parent could.
        #[test]
        fn test_perf_phase181_workflow_sandbox_drops_then_denied() {
            let _g = CapGuard::snapshot();
            // Parent with caps: ENOSYS (backend missing).
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
            // After sandbox drop: EACCES.
            drop_cap_perfmon_and_sys_admin();
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        // -- Buggy caller -------------------------------------------------

        /// A no-cap caller passing both bad flags and bad attr-size
        /// (multiple errors) sees EINVAL (flags check) — the
        /// earliest-failing check wins, just like Linux.
        #[test]
        fn test_perf_phase181_buggy_caller_multi_error_flags_wins() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            attr.size = 1;
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, -42, 0, 999, 1 << 30);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }

        // -- Recovery -----------------------------------------------------

        /// After EACCES, restoring CAP_PERFMON lets the same call
        /// reach ENOSYS.  Confirms dynamic cap evaluation per call.
        #[test]
        fn test_perf_phase181_recovery_restore_perfmon_reaches_enosys() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_hw_attr();
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
            // Restore caps via CapGuard drop semantics is end-of-test
            // — for in-test recovery, re-grant CAP_PERFMON manually.
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: u32::MAX,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: (1u32 << 9) - 1,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            assert_eq!(crate::sys_capability::capset(&mut hdr, data.as_ptr()), 0);
            errno::set_errno(0);
            assert_eq!(perf_event_open(&mut attr, 0, 0, -1, 0), -1);
            assert_eq!(errno::get_errno(), errno::ENOSYS);
        }

        // -- No-side-effect ----------------------------------------------

        /// EACCES rejection leaves capability sets untouched —
        /// perf_event_open does not silently drop or grant caps.
        #[test]
        fn test_perf_phase181_eacces_preserves_other_caps() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            // Snapshot the post-drop caps.
            let (lo_before, hi_before) = crate::sys_capability::current_caps_effective();
            let mut attr = make_valid_hw_attr();
            let _ = perf_event_open(&mut attr, 0, 0, -1, 0);
            let (lo_after, hi_after) = crate::sys_capability::current_caps_effective();
            assert_eq!(lo_before, lo_after);
            assert_eq!(hi_before, hi_after);
        }

        // -- Sentinel ----------------------------------------------------

        /// Software events (PERF_TYPE_SOFTWARE) also require the cap
        /// when exclude_kernel == 0 — Linux's perf_allow_kernel does
        /// not distinguish event types.  Even a CPU_CLOCK event is
        /// kernel-measured by default.
        #[test]
        fn test_perf_phase181_software_event_also_gated() {
            let _g = CapGuard::snapshot();
            drop_cap_perfmon_and_sys_admin();
            let mut attr = make_valid_sw_attr();
            errno::set_errno(0);
            let r = perf_event_open(&mut attr, 0, 0, -1, 0);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EACCES);
        }

        // -- Cross-checks ------------------------------------------------

        /// Exclude_kernel is bit 5 of attr.flags (Linux's bitfield
        /// position).  Confirm our local constant matches Linux's
        /// uapi: disabled=0, inherit=1, pinned=2, exclusive=3,
        /// exclude_user=4, exclude_kernel=5.
        #[test]
        fn test_perf_phase181_exclude_kernel_is_bit_five() {
            assert_eq!(PERF_ATTR_FLAG_EXCLUDE_KERNEL, 1u64 << 5);
        }

        /// Exclude_kernel bit does not collide with exclusive
        /// (bit 3, in the canonical Linux layout) — this is the bug
        /// observed in `linux_perf_types.rs`.  Documents the
        /// expected non-overlap.
        #[test]
        fn test_perf_phase181_exclude_kernel_distinct_from_exclusive() {
            const PERF_ATTR_FLAG_EXCLUSIVE: u64 = 1 << 3;
            assert_eq!(PERF_ATTR_FLAG_EXCLUDE_KERNEL & PERF_ATTR_FLAG_EXCLUSIVE, 0);
        }
    }
}
