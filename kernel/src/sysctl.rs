//! Kernel parameter registry (sysctl-like interface).
//!
//! Provides a flat set of named, runtime-tunable kernel parameters.
//! Parameters are identified by a fixed numeric ID and store `u64`
//! values.  Userspace reads/writes parameters via `SYS_SYSCTL_GET`
//! and `SYS_SYSCTL_SET` syscalls.
//!
//! ## Design
//!
//! The design spec calls for runtime-tunable parameters for memory
//! management, scheduling, and other subsystems, with workload profiles
//! as named presets.  This module provides the underlying storage and
//! lookup mechanism.
//!
//! Parameters are registered at boot time by each subsystem.  The
//! registry is a fixed-size array protected by a spinlock — no heap
//! allocation on the read/write path.
//!
//! ## Parameter Naming
//!
//! Each parameter has:
//! - A unique numeric ID (for syscall access — fast O(1) lookup).
//! - A human-readable name (for logging and user-facing display).
//! - A description (for help text).
//! - A valid range `[min, max]` enforced on writes.
//!
//! ## Thread Safety
//!
//! The registry is protected by a spinlock.  Individual parameter
//! reads/writes are atomic at the u64 level, but the lock ensures
//! consistency when a workload profile sets multiple parameters.

use crate::sched::WorkloadProfile;
use crate::serial_println;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of registered parameters.
///
/// 64 is generous for initial development.  When this fills up,
/// increase it — it's a flat array, so cost is 64 * sizeof(Param).
const MAX_PARAMS: usize = 64;

// ---------------------------------------------------------------------------
// Parameter IDs — memory subsystem
// ---------------------------------------------------------------------------

/// Maximum user-space stack growth in 16 KiB frames.
///
/// Default: 256 frames = 4 MiB.  Controls how far down the stack
/// can grow via page faults before the guard page triggers.
pub const PARAM_MM_MAX_STACK_FRAMES: u16 = 0;

/// Whether new anonymous mmap regions default to lazy allocation.
///
/// 0 = committed (default, per design spec).
/// 1 = lazy (demand-paged).
///
/// This is a system-wide default; individual mappings can still
/// override with the `MAP_LAZY` flag.
pub const PARAM_MM_LAZY_DEFAULT: u16 = 1;

/// OOM kill policy.
///
/// 0 = kill the largest process (by RSS).
/// 1 = kill the most recently spawned process.
/// 2 = return error to the allocating process.
///
/// Default: 0 (kill largest).
pub const PARAM_MM_OOM_POLICY: u16 = 2;

/// Page zeroing strategy.
///
/// 0 = zero on allocation (current behavior, secure).
/// 1 = zero on free (slightly faster allocation, pages pre-zeroed).
///
/// Default: 0.
pub const PARAM_MM_ZERO_ON_ALLOC: u16 = 3;

/// Swappiness — how aggressively to evict pages to swap.
///
/// 0 = never swap (only under extreme memory pressure).
/// 100 = swap eagerly.
///
/// From the design spec: "Swappiness (how aggressively to swap vs.
/// drop page cache) — Linux default 60 is too aggressive for desktop,
/// 10-20 is better for desktop with enough RAM."
///
/// Default: 15 (conservative desktop default).
pub const PARAM_MM_SWAPPINESS: u16 = 4;

/// Minimum free pages before the kernel starts swapping.
///
/// When the number of free physical frames drops below this threshold,
/// the page reclaimer starts evicting pages to swap.
///
/// Default: 32 (512 KiB of free memory at 16 KiB pages).
pub const PARAM_MM_MIN_FREE_PAGES: u16 = 5;

/// Maximum pages to swap out per batch before yielding the CPU.
///
/// During page reclamation, the kernel swaps out pages in batches.
/// After each batch of this many pages, the reclaimer yields the CPU
/// so other tasks can run and the system stays responsive.
///
/// Lower values = more responsive system during heavy swapping, but
/// slower reclaim throughput.  Higher values = faster reclaim but
/// potential for UI stutters during swap storms.
///
/// Default: 4 (swap 4 pages, yield, repeat).
pub const PARAM_MM_SWAP_BATCH_SIZE: u16 = 6;

// ---------------------------------------------------------------------------
// Parameter IDs — scheduler subsystem
// ---------------------------------------------------------------------------

/// Interactive task detection threshold (in timer ticks).
///
/// Tasks with average burst length below this threshold are marked
/// as interactive and receive a priority boost.  At 100 Hz,
/// 1 tick = 10 ms.
///
/// Default: 5 (50 ms).
pub const PARAM_SCHED_INTERACTIVE_THRESHOLD: u16 = 10;

/// Interactive priority boost (priority levels).
///
/// How many priority levels to boost interactive tasks by.
///
/// Default: 2.
pub const PARAM_SCHED_INTERACTIVE_BOOST: u16 = 11;

/// Anti-starvation threshold (ticks).
///
/// A Ready task waiting this many ticks without being scheduled gets
/// a temporary priority boost to prevent indefinite starvation.
/// At 100 Hz, 200 ticks = 2 seconds.  Set to 0 to disable anti-starvation.
///
/// Default: 200.
pub const PARAM_SCHED_STARVATION_THRESHOLD: u16 = 12;

/// Scheduler backend algorithm.
///
/// 0 = PriorityRoundRobin (default, O(1) bitmap pick_next)
/// 1 = EEVDF (virtual-runtime fairness + virtual deadlines)
/// 2 = Deadline (EDF, admission control, budget throttling)
///
/// Requires reboot to take effect.  The current value is the *desired*
/// backend; the actual running backend may differ if changed since boot.
///
/// Default: 0 (PriorityRoundRobin).
pub const PARAM_SCHED_BACKEND: u16 = 13;

// --- Filesystem / Buffer Cache (20-29) ---

/// Buffer cache read-ahead maximum window (sectors).
///
/// Maximum number of sectors to prefetch during sequential access.
/// Higher values improve large-file throughput but consume more cache.
///
/// Default: 128 (64 KiB).
pub const PARAM_FS_READAHEAD_MAX: u16 = 20;

/// Buffer cache read-ahead initial window (sectors).
///
/// Starting prefetch window size, doubles on sustained sequential reads.
///
/// Default: 4 (2 KiB).
pub const PARAM_FS_READAHEAD_INITIAL: u16 = 21;

/// Buffer cache dirty entry expiry (seconds).
///
/// How long a dirty entry can remain in cache before background
/// writeback flushes it to disk.  Lower values reduce data loss risk
/// on crash; higher values reduce I/O.
///
/// Default: 5.
pub const PARAM_FS_DIRTY_EXPIRE_SECS: u16 = 22;

/// Enable/disable allocation tracing (mm.alloc_trace).
///
/// 0 = disabled (default, zero-cost — single atomic load per alloc).
/// 1 = enabled (records alloc/free events in 256-entry ring buffer).
///
/// This is the "profiling mode for high-frequency events" — enables
/// detailed tracking of frame allocation/free events at the buddy
/// allocator level.  Use `alloctrace` kshell command to inspect.
pub const PARAM_MM_ALLOC_TRACE: u16 = 7;

/// Whether new anonymous mmap regions from **Linux-ABI** programs default
/// to lazy allocation (`mm.linux_lazy_default`).
///
/// 0 = committed (eager-populate — strict, no overcommit).
/// 1 = lazy/overcommit (default — Linux programs assume sparse mappings
///     backed on first touch).
///
/// This is the Linux-ABI counterpart of [`PARAM_MM_LAZY_DEFAULT`] (which
/// governs the *native* ABI).  The two ABIs get independent system-wide
/// knobs because their idioms differ: native code is committed-by-default
/// per the design spec, whereas Linux code expects overcommit.  This knob
/// is surfaced to userspace under the canonical Linux name
/// `/proc/sys/vm/overcommit_memory`.
///
/// As with the native knob, individual mappings can override with the
/// `MAP_LAZY` flag, and a process can override the default for itself via
/// [`crate::proc::pcb::MmapCommitPolicy`].
pub const PARAM_MM_LINUX_LAZY_DEFAULT: u16 = 8;

// ---------------------------------------------------------------------------
// Parameter IDs — cgroup subsystem (30-39)
// ---------------------------------------------------------------------------

/// Default CPU period for new cgroups (in timer ticks).
///
/// At 100 Hz, 100 ticks = 1 second.  This controls how frequently
/// CPU quota accounting resets.  Shorter periods give finer-grained
/// enforcement but more overhead.
///
/// Default: 100 (1 second at 100 Hz).
pub const PARAM_CGROUP_CPU_PERIOD: u16 = 30;

/// Maximum number of I/O operations per period for the I/O controller.
///
/// Controls the default `io_ops_max` for newly-created cgroups when
/// the I/O controller is configured.  0 = unlimited.
///
/// Default: 0 (unlimited — must be explicitly set per-group).
pub const PARAM_CGROUP_IO_OPS_MAX: u16 = 31;

/// Maximum I/O bytes per period for the I/O controller.
///
/// Controls the default `io_bytes_max` for newly-created cgroups.
/// Measured in 16 KiB frames (one frame = one I/O unit).
/// 0 = unlimited.
///
/// Default: 0 (unlimited — must be explicitly set per-group).
pub const PARAM_CGROUP_IO_BYTES_MAX: u16 = 32;

// ---------------------------------------------------------------------------
// Parameter definition
// ---------------------------------------------------------------------------

/// A single tunable kernel parameter.
struct Param {
    /// Unique parameter ID (index into the syscall interface).
    id: u16,
    /// Human-readable name (e.g., "mm.max_stack_frames").
    name: &'static str,
    /// Current value.
    value: u64,
    /// Default value (for reset).
    default: u64,
    /// Minimum allowed value (inclusive).
    min: u64,
    /// Maximum allowed value (inclusive).
    max: u64,
    /// Whether this slot is in use.
    active: bool,
}

impl Param {
    /// Empty parameter slot.
    const fn empty() -> Self {
        Self {
            id: 0,
            name: "",
            value: 0,
            default: 0,
            min: 0,
            max: 0,
            active: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

/// The parameter registry.
///
/// Lock ordering: this lock should be acquired BEFORE subsystem locks
/// when a parameter change triggers a subsystem reconfiguration.
static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

struct Registry {
    params: [Param; MAX_PARAMS],
    count: usize,
}

impl Registry {
    const fn new() -> Self {
        Self {
            params: [const { Param::empty() }; MAX_PARAMS],
            count: 0,
        }
    }

    /// Register a new parameter.  Returns false if the registry is full
    /// or the ID is already registered.
    fn register(
        &mut self,
        id: u16,
        name: &'static str,
        default: u64,
        min: u64,
        max: u64,
    ) -> bool {
        // Check for duplicate ID.
        for p in self.params.iter().take(self.count) {
            if p.active && p.id == id {
                return false;
            }
        }

        if self.count >= MAX_PARAMS {
            return false;
        }

        self.params[self.count] = Param {
            id,
            name,
            value: default,
            default,
            min,
            max,
            active: true,
        };
        self.count = self.count.saturating_add(1);
        true
    }

    /// Get a parameter's current value by ID.
    fn get(&self, id: u16) -> Option<u64> {
        for p in self.params.iter().take(self.count) {
            if p.active && p.id == id {
                return Some(p.value);
            }
        }
        None
    }

    /// Set a parameter's value by ID.  Returns the old value on success,
    /// None if the ID is unknown or the value is out of range.
    fn set(&mut self, id: u16, value: u64) -> Option<u64> {
        for p in self.params.iter_mut().take(self.count) {
            if p.active && p.id == id {
                if value < p.min || value > p.max {
                    return None;
                }
                let old = p.value;
                p.value = value;
                return Some(old);
            }
        }
        None
    }

    /// Find a parameter by ID, returning its metadata for listing.
    fn find(&self, id: u16) -> Option<ParamInfo> {
        for p in self.params.iter().take(self.count) {
            if p.active && p.id == id {
                return Some(ParamInfo {
                    id: p.id,
                    name: p.name,
                    value: p.value,
                    default: p.default,
                    min: p.min,
                    max: p.max,
                });
            }
        }
        None
    }
}

/// Read-only snapshot of a parameter's metadata.
#[allow(dead_code)] // Returned by sysctl::info() for parameter introspection.
pub struct ParamInfo {
    pub id: u16,
    pub name: &'static str,
    pub value: u64,
    pub default: u64,
    pub min: u64,
    pub max: u64,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the sysctl registry with default parameters.
///
/// Called during kernel boot, after the heap is available.
pub fn init() {
    let mut reg = REGISTRY.lock();

    // Memory parameters.
    reg.register(
        PARAM_MM_MAX_STACK_FRAMES,
        "mm.max_stack_frames",
        256,    // 4 MiB default
        16,     // 256 KiB minimum
        4096,   // 64 MiB maximum
    );

    reg.register(
        PARAM_MM_LAZY_DEFAULT,
        "mm.lazy_default",
        0,  // 0 = committed (default per design spec)
        0,
        1,
    );

    reg.register(
        PARAM_MM_OOM_POLICY,
        "mm.oom_policy",
        0,  // Kill largest
        0,
        2,
    );

    reg.register(
        PARAM_MM_ZERO_ON_ALLOC,
        "mm.zero_on_alloc",
        0,  // Zero on allocation (secure default)
        0,
        1,
    );

    reg.register(
        PARAM_MM_SWAPPINESS,
        "mm.swappiness",
        15, // Conservative desktop default (0=never, 100=eager)
        0,
        100,
    );

    reg.register(
        PARAM_MM_MIN_FREE_PAGES,
        "mm.min_free_pages",
        32, // 512 KiB of free memory at 16 KiB/page
        4,  // Minimum: 64 KiB
        1024, // Maximum: 16 MiB
    );

    reg.register(
        PARAM_MM_SWAP_BATCH_SIZE,
        "mm.swap_batch_size",
        4,  // Swap 4 pages per batch, then yield CPU
        1,  // Minimum: yield after every page (most responsive)
        64, // Maximum: swap 64 pages before yielding
    );

    reg.register(
        PARAM_MM_ALLOC_TRACE,
        "mm.alloc_trace",
        0,  // Disabled (zero-cost single atomic load per alloc/free)
        0,
        1,  // 1 = enabled
    );

    reg.register(
        PARAM_MM_LINUX_LAZY_DEFAULT,
        "mm.linux_lazy_default",
        1,  // 1 = lazy/overcommit (Linux programs assume overcommit)
        0,
        1,
    );

    // Scheduler parameters (informational — actual values are in the
    // task module, but exposing them here allows the sysctl interface
    // to read them).
    reg.register(
        PARAM_SCHED_INTERACTIVE_THRESHOLD,
        "sched.interactive_threshold",
        5,  // 5 ticks = 50 ms
        1,
        100,
    );

    reg.register(
        PARAM_SCHED_INTERACTIVE_BOOST,
        "sched.interactive_boost",
        2,  // 2 priority levels
        0,
        8,
    );

    reg.register(
        PARAM_SCHED_STARVATION_THRESHOLD,
        "sched.starvation_threshold",
        200,    // 200 ticks = 2 seconds at 100 Hz
        0,      // 0 = disable anti-starvation
        1000,   // 10 seconds max
    );

    reg.register(
        PARAM_SCHED_BACKEND,
        "sched.backend",
        0,  // PriorityRoundRobin (default)
        0,  // min: PriorityRR
        2,  // max: Deadline
    );

    // Filesystem / buffer cache parameters.
    reg.register(
        PARAM_FS_READAHEAD_MAX,
        "fs.readahead_max",
        128,    // 64 KiB default
        1,
        1024,   // 512 KiB maximum
    );

    reg.register(
        PARAM_FS_READAHEAD_INITIAL,
        "fs.readahead_initial",
        4,      // 2 KiB default
        1,
        64,
    );

    reg.register(
        PARAM_FS_DIRTY_EXPIRE_SECS,
        "fs.dirty_expire_secs",
        5,      // 5 seconds
        1,      // 1 second minimum
        60,     // 60 seconds maximum
    );

    // Cgroup parameters.
    reg.register(
        PARAM_CGROUP_CPU_PERIOD,
        "cgroup.cpu_period",
        100,    // 1 second at 100 Hz timer
        10,     // 100 ms minimum (10 ticks)
        1000,   // 10 seconds maximum
    );

    reg.register(
        PARAM_CGROUP_IO_OPS_MAX,
        "cgroup.io_ops_max_default",
        0,      // Unlimited — explicitly set per-group
        0,
        1_000_000,
    );

    reg.register(
        PARAM_CGROUP_IO_BYTES_MAX,
        "cgroup.io_bytes_max_default",
        0,      // Unlimited — explicitly set per-group (in frames)
        0,
        u64::MAX,
    );

    let count = reg.count;
    drop(reg);

    serial_println!("[sysctl] Initialized {} parameters", count);
}

/// Get a parameter's current value.
///
/// Returns `None` if the parameter ID is unknown.
#[must_use]
pub fn get(id: u16) -> Option<u64> {
    REGISTRY.lock().get(id)
}

/// Non-blocking parameter read for **interrupt / exception context**.
///
/// `REGISTRY` is a raw `spin::Mutex` that does not mask interrupts, so any
/// blocking `REGISTRY.lock()` reached from an ISR (timer tick, page-fault
/// handler, …) will hard-deadlock if it interrupts a task that is *already*
/// holding the lock in normal context: the ISR spins on the lock forever
/// while the interrupted holder can never resume to release it (single CPU).
/// This was observed as a real boot wedge — the timer IRQ's
/// `sched::check_starvation` blocked on `REGISTRY` behind a task in
/// `sysctl::set()` that held it across a slow `serial_println!`
/// (`known-issues.md` B-SYSCTL-IRQ-DEADLOCK).
///
/// Interrupt-context callers must use this instead of [`get`]: it uses
/// `try_lock` and returns `None` on contention (or unknown id), letting the
/// caller fall back to a compile-time default. Best-effort by design — a
/// momentarily-contended tunable read simply uses its default for that one
/// tick, which is always safe for the tunables read from ISRs.
#[must_use]
pub fn try_get(id: u16) -> Option<u64> {
    REGISTRY.try_lock().and_then(|reg| reg.get(id))
}

/// Set a parameter's value.
///
/// Returns the old value on success, `None` if the ID is unknown or
/// the value is out of range.
///
/// After updating the registry, this function dispatches to the
/// relevant subsystem setter so the change takes effect immediately
/// (e.g., updating atomic tunables in the buffer cache).
pub fn set(id: u16, value: u64) -> Option<u64> {
    let result = REGISTRY.lock().set(id, value);
    if let Some(old) = result {
        // Snapshot the metadata (owned `ParamInfo`, `name: &'static str`) and
        // release the lock BEFORE logging.  Never hold `REGISTRY` across a
        // `serial_println!`: `REGISTRY` is also read from interrupt context
        // (via `try_get`), and holding it across the slow serial write widens
        // the deadlock window that B-SYSCTL-IRQ-DEADLOCK exploited — the
        // `let … = REGISTRY.lock().find(id);` temporary guard drops at the
        // statement's `;`, so the log below runs lock-free.
        let info = REGISTRY.lock().find(id);
        if let Some(info) = info {
            serial_println!(
                "[sysctl] {} = {} (was {})",
                info.name, value, old
            );
        }
        // Propagate the new value to the owning subsystem.
        notify_subsystem(id, value);
    }
    result
}

/// Dispatch a parameter change to the subsystem that owns it.
///
/// This is the bridge between the sysctl registry (which stores u64
/// values) and subsystem-specific atomic tunables.  Each parameter
/// ID that has a runtime-tunable backing variable gets a match arm
/// here.
fn notify_subsystem(id: u16, value: u64) {
    match id {
        PARAM_FS_READAHEAD_MAX => {
            // Registry value is in sectors; cache setter takes u32.
            crate::fs::cache::set_readahead_max(value as u32);
        }
        PARAM_FS_READAHEAD_INITIAL => {
            crate::fs::cache::set_readahead_initial(value as u32);
        }
        PARAM_FS_DIRTY_EXPIRE_SECS => {
            // Registry stores seconds; cache tunable is nanoseconds.
            // Saturating multiply avoids overflow on large values
            // (clamped to 60s max by the registry, so 60 * 1e9 fits u64).
            let ns = value.saturating_mul(1_000_000_000);
            crate::fs::cache::set_dirty_expire_ns(ns);
        }
        PARAM_MM_ZERO_ON_ALLOC => {
            // value 0 = zero-on-alloc (default), 1 = zero-on-free.
            crate::mm::frame::set_zero_on_free_mode(value == 1);
        }
        PARAM_MM_ALLOC_TRACE => {
            // Toggle frame allocation profiling.
            if value != 0 {
                crate::mm::alloc_trace::enable();
            } else {
                crate::mm::alloc_trace::disable();
            }
        }
        PARAM_SCHED_BACKEND => {
            // Update the desired scheduler backend.  Takes effect on
            // next reboot (next PerCpuScheduler::init() call).
            #[allow(clippy::cast_possible_truncation)]
            let id = value as u8;
            crate::sched::backend::set_desired_backend(id);
        }
        PARAM_CGROUP_CPU_PERIOD => {
            // Update the default CPU period for new cgroups.
            crate::cgroup::set_default_cpu_period(value);
        }
        // Cgroup io_ops_max and io_bytes_max are informational defaults;
        // actual limits are set per-group via the cgroup API.  No
        // subsystem notification needed.
        PARAM_CGROUP_IO_OPS_MAX | PARAM_CGROUP_IO_BYTES_MAX => {}
        // Other subsystems will add arms here as they register
        // runtime-tunable parameters.
        _ => {}
    }
}

/// Get metadata for a parameter by ID.
#[must_use]
#[allow(dead_code)] // Public API for sysctl info syscall.
pub fn info(id: u16) -> Option<ParamInfo> {
    REGISTRY.lock().find(id)
}

/// Get the number of registered parameters.
#[must_use]
#[allow(dead_code)] // Public API for sysctl enumeration.
pub fn count() -> usize {
    REGISTRY.lock().count
}

/// List all registered parameters with their current values.
///
/// Returns a snapshot of every active parameter.  Used by sysfs to
/// expose kernel tunables via the virtual filesystem.
pub fn list_all() -> alloc::vec::Vec<ParamInfo> {
    let reg = REGISTRY.lock();
    let mut result = alloc::vec::Vec::with_capacity(reg.count);
    for p in reg.params.iter().take(reg.count) {
        if p.active {
            result.push(ParamInfo {
                id: p.id,
                name: p.name,
                value: p.value,
                default: p.default,
                min: p.min,
                max: p.max,
            });
        }
    }
    result
}

/// Look up a parameter by its dotted name (e.g., "mm.swappiness").
///
/// Returns the parameter info if found.  Used by sysfs for
/// name-based lookups.
pub fn find_by_name(name: &str) -> Option<ParamInfo> {
    let reg = REGISTRY.lock();
    for p in reg.params.iter().take(reg.count) {
        if p.active && p.name == name {
            return Some(ParamInfo {
                id: p.id,
                name: p.name,
                value: p.value,
                default: p.default,
                min: p.min,
                max: p.max,
            });
        }
    }
    None
}

/// Set a parameter by its dotted name (e.g., "mm.swappiness").
///
/// Returns the old value on success, `None` if not found or out of range.
pub fn set_by_name(name: &str, value: u64) -> Option<u64> {
    let id = {
        let reg = REGISTRY.lock();
        let mut found_id = None;
        for p in reg.params.iter().take(reg.count) {
            if p.active && p.name == name {
                found_id = Some(p.id);
                break;
            }
        }
        found_id
    };
    id.and_then(|id| set(id, value))
}

// ---------------------------------------------------------------------------
// Memory workload profiles
// ---------------------------------------------------------------------------

/// Memory-subsystem parameter presets for each workload profile.
///
/// These mirror the scheduler workload profiles defined in
/// [`WorkloadProfile`], applying tuned mm.* sysctl values for the
/// selected workload.  The idea (from the design spec) is that a
/// single "apply profile" action configures both scheduler and memory
/// subsystems for the workload.
///
/// ## Presets
///
/// | Parameter           | Desktop | Server | Development | Gaming |
/// |---------------------|---------|--------|-------------|--------|
/// | mm.max_stack_frames | 256     | 512    | 512         | 512    |
/// | mm.lazy_default     | 0       | 1      | 0           | 0      |
/// | mm.linux_lazy_default | 1     | 1      | 1           | 1      |
/// | mm.oom_policy       | 0       | 2      | 0           | 0      |
/// | mm.zero_on_alloc    | 0       | 1      | 0           | 1      |
/// | mm.swap_batch_size  | 4       | 16     | 4           | 2      |
///
/// **Desktop**: Committed allocation, moderate stack, kill-largest OOM.
/// Predictable and secure — good for mixed interactive workloads.
///
/// **Server**: Lazy allocation reduces memory pressure for many-process
/// server deployments (fork-heavy, CoW-friendly).  Return-error OOM
/// lets servers handle memory exhaustion gracefully.  Zero-on-free
/// amortises zeroing cost for high-throughput allocation patterns.
///
/// **Development**: Committed allocation for predictable behavior during
/// debugging.  Larger stack (compiler recursion, test harnesses).
/// Kill-largest OOM avoids complex error handling in dev tools.
///
/// **Gaming**: Committed allocation avoids page-fault latency spikes
/// during gameplay.  Large stack for deep game-engine call stacks.
/// Zero-on-free slightly reduces allocation latency on the hot path.
struct MemoryProfilePreset {
    max_stack_frames: u64,
    lazy_default: u64,
    linux_lazy_default: u64,
    oom_policy: u64,
    zero_on_alloc: u64,
    swappiness: u64,
    swap_batch_size: u64,
}

impl MemoryProfilePreset {
    /// Get the preset for a workload profile.
    const fn for_profile(profile: WorkloadProfile) -> Self {
        match profile {
            WorkloadProfile::Desktop => Self {
                max_stack_frames: 256, // 4 MiB — moderate, enough for typical apps
                lazy_default: 0,       // committed (per design spec default)
                linux_lazy_default: 1, // Linux apps (browsers, etc.) expect overcommit
                oom_policy: 0,         // kill largest — protect desktop responsiveness
                zero_on_alloc: 0,      // secure default
                swappiness: 15,        // conservative — only swap under real pressure
                swap_batch_size: 4,    // yield often — keep UI responsive during swap
            },
            WorkloadProfile::Server => Self {
                max_stack_frames: 512, // 8 MiB — servers may have deep stacks (Java, etc.)
                lazy_default: 1,       // lazy — many-process servers benefit from CoW/overcommit
                linux_lazy_default: 1, // Linux server workloads expect overcommit
                oom_policy: 2,         // return error — servers should handle OOM gracefully
                zero_on_alloc: 1,      // zero on free — amortise for high-throughput alloc
                swappiness: 30,        // moderate — servers benefit from more aggressive reclaim
                swap_batch_size: 16,   // larger batches ��� throughput over responsiveness
            },
            WorkloadProfile::Development => Self {
                max_stack_frames: 512, // 8 MiB — compilers/debuggers use deep stacks
                lazy_default: 0,       // committed — predictable for debugging
                linux_lazy_default: 1, // Linux toolchains (gcc, etc.) expect overcommit
                oom_policy: 0,         // kill largest — just kill the runaway build
                zero_on_alloc: 0,      // secure default, clean state for debugging
                swappiness: 10,        // low — keep build artifacts in memory
                swap_batch_size: 4,    // moderate — balance responsiveness and IDE perf
            },
            WorkloadProfile::Gaming => Self {
                max_stack_frames: 512, // 8 MiB — game engines use deep stacks
                lazy_default: 0,       // committed — avoid page fault latency during gameplay
                linux_lazy_default: 1, // Linux games (Proton-style) expect overcommit
                oom_policy: 0,         // kill largest — protect the game process
                zero_on_alloc: 1,      // zero on free — reduce alloc latency spikes
                swappiness: 5,         // very low — minimize swap latency during gameplay
                swap_batch_size: 2,    // smallest batches — minimize swap-induced stutters
            },
        }
    }
}

/// Apply a memory workload profile, setting all mm.* sysctl parameters.
///
/// Returns `true` if the profile was applied successfully (all four
/// parameters set).  Returns `false` if the profile ID is invalid
/// or any parameter write failed (e.g., value out of range — should
/// not happen with well-defined presets).
///
/// The sysctl lock is acquired once per parameter.  Callers who need
/// both scheduler and memory profiles applied atomically should call
/// this from within a higher-level coordination function.
pub fn apply_memory_profile(profile_id: u8) -> bool {
    let Some(profile) = WorkloadProfile::from_u8(profile_id) else {
        return false;
    };

    let preset = MemoryProfilePreset::for_profile(profile);

    // Apply all four mm.* parameters.  Each `set()` call acquires and
    // releases the registry lock, which is fine — the parameters are
    // independent and the lock is very fast (no contention during
    // profile application).
    // Starvation threshold per profile:
    // - Desktop (0): 200 ticks (2s) — responsive, protect UI
    // - Server (1): 500 ticks (5s) — less aggressive, throughput matters
    // - Development (2): 300 ticks (3s) — balanced for many tasks
    // - Gaming (3): 100 ticks (1s) — very aggressive, protect frame rate
    let starv_threshold: u64 = match profile_id {
        0 => 200,   // Desktop
        1 => 500,   // Server
        2 => 300,   // Development
        3 => 100,   // Gaming
        _ => 200,   // Fallback
    };

    let ok = set(PARAM_MM_MAX_STACK_FRAMES, preset.max_stack_frames).is_some()
        && set(PARAM_MM_LAZY_DEFAULT, preset.lazy_default).is_some()
        && set(PARAM_MM_LINUX_LAZY_DEFAULT, preset.linux_lazy_default).is_some()
        && set(PARAM_MM_OOM_POLICY, preset.oom_policy).is_some()
        && set(PARAM_MM_ZERO_ON_ALLOC, preset.zero_on_alloc).is_some()
        && set(PARAM_MM_SWAPPINESS, preset.swappiness).is_some()
        && set(PARAM_MM_SWAP_BATCH_SIZE, preset.swap_batch_size).is_some()
        && set(PARAM_SCHED_STARVATION_THRESHOLD, starv_threshold).is_some();

    if ok {
        serial_println!(
            "[sysctl] Applied memory profile: {} (stack={}, lazy={}, linux_lazy={}, oom={}, zero={}, swap={}, batch={})",
            profile.name(),
            preset.max_stack_frames,
            preset.lazy_default,
            preset.linux_lazy_default,
            preset.oom_policy,
            preset.zero_on_alloc,
            preset.swappiness,
            preset.swap_batch_size
        );
    }

    ok
}

/// Detect the current memory workload profile, if the mm.* sysctl
/// parameters match a known preset.
///
/// Returns `None` if the parameters have been manually tuned and
/// don't match any profile exactly.
#[must_use]
pub fn current_memory_profile() -> Option<WorkloadProfile> {
    let reg = REGISTRY.lock();

    // Read current values under a single lock acquisition for consistency.
    let stack = reg.get(PARAM_MM_MAX_STACK_FRAMES)?;
    let lazy = reg.get(PARAM_MM_LAZY_DEFAULT)?;
    let linux_lazy = reg.get(PARAM_MM_LINUX_LAZY_DEFAULT)?;
    let oom = reg.get(PARAM_MM_OOM_POLICY)?;
    let zero = reg.get(PARAM_MM_ZERO_ON_ALLOC)?;
    let swap = reg.get(PARAM_MM_SWAPPINESS)?;
    let batch = reg.get(PARAM_MM_SWAP_BATCH_SIZE)?;
    drop(reg);

    // Check each profile's preset against current values.
    for id in 0..=3u8 {
        if let Some(profile) = WorkloadProfile::from_u8(id) {
            let preset = MemoryProfilePreset::for_profile(profile);
            if stack == preset.max_stack_frames
                && lazy == preset.lazy_default
                && linux_lazy == preset.linux_lazy_default
                && oom == preset.oom_policy
                && zero == preset.zero_on_alloc
                && swap == preset.swappiness
                && batch == preset.swap_batch_size
            {
                return Some(profile);
            }
        }
    }
    None
}

/// Apply a unified system workload profile — both scheduler and memory.
///
/// This is the "one call to rule them all" function that sets both
/// scheduler time slices (via `sched::apply_workload_profile`) and
/// memory parameters (via `apply_memory_profile`).
///
/// Returns `true` if both subsystems were configured successfully.
pub fn apply_system_profile(profile_id: u8) -> bool {
    let sched_ok = crate::sched::apply_workload_profile(profile_id);
    let mm_ok = apply_memory_profile(profile_id);

    if sched_ok && mm_ok {
        if let Some(profile) = WorkloadProfile::from_u8(profile_id) {
            serial_println!(
                "[sysctl] Applied system profile: {} (scheduler + memory)",
                profile.name()
            );
        }
    }

    sched_ok && mm_ok
}

/// Run self-test.
pub fn self_test() {
    serial_println!("[sysctl] Running self-test...");

    // Read default values.
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(256));
    assert_eq!(get(PARAM_MM_LAZY_DEFAULT), Some(0));
    assert_eq!(get(PARAM_MM_LINUX_LAZY_DEFAULT), Some(1));
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(0));

    // Set within range.
    assert_eq!(set(PARAM_MM_MAX_STACK_FRAMES, 512), Some(256));
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));

    // Set out of range — should fail.
    assert_eq!(set(PARAM_MM_MAX_STACK_FRAMES, 0), None);
    assert_eq!(set(PARAM_MM_MAX_STACK_FRAMES, 10000), None);
    // Value unchanged.
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));

    // Unknown parameter.
    assert_eq!(get(999), None);
    assert_eq!(set(999, 42), None);

    // Restore default.
    let _ = set(PARAM_MM_MAX_STACK_FRAMES, 256);

    // -----------------------------------------------------------------------
    // Memory workload profiles
    // -----------------------------------------------------------------------

    // Default values match the Desktop profile.
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Desktop));

    // Apply Server profile.
    assert!(apply_memory_profile(1)); // Server
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));
    assert_eq!(get(PARAM_MM_LAZY_DEFAULT), Some(1));
    assert_eq!(get(PARAM_MM_LINUX_LAZY_DEFAULT), Some(1));
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(2));
    assert_eq!(get(PARAM_MM_ZERO_ON_ALLOC), Some(1));
    assert_eq!(get(PARAM_MM_SWAPPINESS), Some(30));
    assert_eq!(get(PARAM_MM_SWAP_BATCH_SIZE), Some(16));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Server));

    // Apply Development profile.
    assert!(apply_memory_profile(2)); // Development
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));
    assert_eq!(get(PARAM_MM_LAZY_DEFAULT), Some(0));
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(0));
    assert_eq!(get(PARAM_MM_ZERO_ON_ALLOC), Some(0));
    assert_eq!(get(PARAM_MM_SWAPPINESS), Some(10));
    assert_eq!(get(PARAM_MM_SWAP_BATCH_SIZE), Some(4));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Development));

    // Apply Gaming profile.
    assert!(apply_memory_profile(3)); // Gaming
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));
    assert_eq!(get(PARAM_MM_LAZY_DEFAULT), Some(0));
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(0));
    assert_eq!(get(PARAM_MM_ZERO_ON_ALLOC), Some(1));
    assert_eq!(get(PARAM_MM_SWAP_BATCH_SIZE), Some(2));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Gaming));

    // Invalid profile ID.
    assert!(!apply_memory_profile(4));
    assert!(!apply_memory_profile(255));

    // Manual tuning breaks profile detection.
    let _ = set(PARAM_MM_OOM_POLICY, 1); // change one param
    assert_eq!(current_memory_profile(), None); // no profile matches

    // Restore Desktop defaults.
    assert!(apply_memory_profile(0));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Desktop));

    // Tuning the Linux lazy default also breaks detection (all profiles
    // ship linux_lazy_default=1, so flipping it to 0 matches no preset).
    let _ = set(PARAM_MM_LINUX_LAZY_DEFAULT, 0);
    assert_eq!(current_memory_profile(), None);
    assert!(apply_memory_profile(0)); // restore
    assert_eq!(get(PARAM_MM_LINUX_LAZY_DEFAULT), Some(1));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Desktop));

    // -----------------------------------------------------------------------
    // Filesystem / buffer cache parameter callbacks
    // -----------------------------------------------------------------------
    // Verify that sysctl set() propagates values to the cache atomics.

    // Read-ahead max: default 128 sectors.
    assert_eq!(get(PARAM_FS_READAHEAD_MAX), Some(128));
    assert_eq!(crate::fs::cache::get_readahead_max(), 128);

    // Change to 256 — should propagate to the cache atomic.
    assert_eq!(set(PARAM_FS_READAHEAD_MAX, 256), Some(128));
    assert_eq!(get(PARAM_FS_READAHEAD_MAX), Some(256));
    assert_eq!(crate::fs::cache::get_readahead_max(), 256);

    // Out of range — rejected by registry, cache unchanged.
    assert_eq!(set(PARAM_FS_READAHEAD_MAX, 0), None);
    assert_eq!(crate::fs::cache::get_readahead_max(), 256);
    assert_eq!(set(PARAM_FS_READAHEAD_MAX, 2000), None);
    assert_eq!(crate::fs::cache::get_readahead_max(), 256);

    // Restore default.
    let _ = set(PARAM_FS_READAHEAD_MAX, 128);
    assert_eq!(crate::fs::cache::get_readahead_max(), 128);

    // Read-ahead initial: default 4 sectors.
    assert_eq!(get(PARAM_FS_READAHEAD_INITIAL), Some(4));
    assert_eq!(crate::fs::cache::get_readahead_initial(), 4);

    let _ = set(PARAM_FS_READAHEAD_INITIAL, 8);
    assert_eq!(crate::fs::cache::get_readahead_initial(), 8);
    let _ = set(PARAM_FS_READAHEAD_INITIAL, 4); // restore

    // Dirty expire: default 5 seconds → 5_000_000_000 ns in cache.
    assert_eq!(get(PARAM_FS_DIRTY_EXPIRE_SECS), Some(5));
    assert_eq!(crate::fs::cache::get_dirty_expire_ns(), 5_000_000_000);

    // Change to 10 seconds → 10_000_000_000 ns.
    let _ = set(PARAM_FS_DIRTY_EXPIRE_SECS, 10);
    assert_eq!(get(PARAM_FS_DIRTY_EXPIRE_SECS), Some(10));
    assert_eq!(crate::fs::cache::get_dirty_expire_ns(), 10_000_000_000);

    // Restore default.
    let _ = set(PARAM_FS_DIRTY_EXPIRE_SECS, 5);
    assert_eq!(crate::fs::cache::get_dirty_expire_ns(), 5_000_000_000);

    // Name-based lookup for fs parameters.
    let info = find_by_name("fs.readahead_max");
    assert!(info.is_some());
    assert_eq!(info.unwrap().value, 128);

    let info = find_by_name("fs.dirty_expire_secs");
    assert!(info.is_some());
    assert_eq!(info.unwrap().value, 5);

    serial_println!("[sysctl] Self-test PASSED");
}
