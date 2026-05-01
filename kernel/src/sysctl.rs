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

/// Set a parameter's value.
///
/// Returns the old value on success, `None` if the ID is unknown or
/// the value is out of range.
pub fn set(id: u16, value: u64) -> Option<u64> {
    let result = REGISTRY.lock().set(id, value);
    if let Some(old) = result {
        if let Some(info) = REGISTRY.lock().find(id) {
            serial_println!(
                "[sysctl] {} = {} (was {})",
                info.name, value, old
            );
        }
    }
    result
}

/// Get metadata for a parameter by ID.
#[must_use]
pub fn info(id: u16) -> Option<ParamInfo> {
    REGISTRY.lock().find(id)
}

/// Get the number of registered parameters.
#[must_use]
pub fn count() -> usize {
    REGISTRY.lock().count
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
/// | mm.oom_policy       | 0       | 2      | 0           | 0      |
/// | mm.zero_on_alloc    | 0       | 1      | 0           | 1      |
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
    oom_policy: u64,
    zero_on_alloc: u64,
    swappiness: u64,
}

impl MemoryProfilePreset {
    /// Get the preset for a workload profile.
    const fn for_profile(profile: WorkloadProfile) -> Self {
        match profile {
            WorkloadProfile::Desktop => Self {
                max_stack_frames: 256, // 4 MiB — moderate, enough for typical apps
                lazy_default: 0,       // committed (per design spec default)
                oom_policy: 0,         // kill largest — protect desktop responsiveness
                zero_on_alloc: 0,      // secure default
                swappiness: 15,        // conservative — only swap under real pressure
            },
            WorkloadProfile::Server => Self {
                max_stack_frames: 512, // 8 MiB — servers may have deep stacks (Java, etc.)
                lazy_default: 1,       // lazy — many-process servers benefit from CoW/overcommit
                oom_policy: 2,         // return error — servers should handle OOM gracefully
                zero_on_alloc: 1,      // zero on free — amortise for high-throughput alloc
                swappiness: 30,        // moderate — servers benefit from more aggressive reclaim
            },
            WorkloadProfile::Development => Self {
                max_stack_frames: 512, // 8 MiB — compilers/debuggers use deep stacks
                lazy_default: 0,       // committed — predictable for debugging
                oom_policy: 0,         // kill largest — just kill the runaway build
                zero_on_alloc: 0,      // secure default, clean state for debugging
                swappiness: 10,        // low — keep build artifacts in memory
            },
            WorkloadProfile::Gaming => Self {
                max_stack_frames: 512, // 8 MiB — game engines use deep stacks
                lazy_default: 0,       // committed — avoid page fault latency during gameplay
                oom_policy: 0,         // kill largest — protect the game process
                zero_on_alloc: 1,      // zero on free — reduce alloc latency spikes
                swappiness: 5,         // very low — minimize swap latency during gameplay
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
    let ok = set(PARAM_MM_MAX_STACK_FRAMES, preset.max_stack_frames).is_some()
        && set(PARAM_MM_LAZY_DEFAULT, preset.lazy_default).is_some()
        && set(PARAM_MM_OOM_POLICY, preset.oom_policy).is_some()
        && set(PARAM_MM_ZERO_ON_ALLOC, preset.zero_on_alloc).is_some()
        && set(PARAM_MM_SWAPPINESS, preset.swappiness).is_some();

    if ok {
        serial_println!(
            "[sysctl] Applied memory profile: {} (stack={}, lazy={}, oom={}, zero={}, swap={})",
            profile.name(),
            preset.max_stack_frames,
            preset.lazy_default,
            preset.oom_policy,
            preset.zero_on_alloc,
            preset.swappiness
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
    let oom = reg.get(PARAM_MM_OOM_POLICY)?;
    let zero = reg.get(PARAM_MM_ZERO_ON_ALLOC)?;
    let swap = reg.get(PARAM_MM_SWAPPINESS)?;
    drop(reg);

    // Check each profile's preset against current values.
    for id in 0..=3u8 {
        if let Some(profile) = WorkloadProfile::from_u8(id) {
            let preset = MemoryProfilePreset::for_profile(profile);
            if stack == preset.max_stack_frames
                && lazy == preset.lazy_default
                && oom == preset.oom_policy
                && zero == preset.zero_on_alloc
                && swap == preset.swappiness
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
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(2));
    assert_eq!(get(PARAM_MM_ZERO_ON_ALLOC), Some(1));
    assert_eq!(get(PARAM_MM_SWAPPINESS), Some(30));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Server));

    // Apply Development profile.
    assert!(apply_memory_profile(2)); // Development
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));
    assert_eq!(get(PARAM_MM_LAZY_DEFAULT), Some(0));
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(0));
    assert_eq!(get(PARAM_MM_ZERO_ON_ALLOC), Some(0));
    assert_eq!(get(PARAM_MM_SWAPPINESS), Some(10));
    assert_eq!(current_memory_profile(), Some(WorkloadProfile::Development));

    // Apply Gaming profile.
    assert!(apply_memory_profile(3)); // Gaming
    assert_eq!(get(PARAM_MM_MAX_STACK_FRAMES), Some(512));
    assert_eq!(get(PARAM_MM_LAZY_DEFAULT), Some(0));
    assert_eq!(get(PARAM_MM_OOM_POLICY), Some(0));
    assert_eq!(get(PARAM_MM_ZERO_ON_ALLOC), Some(1));
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

    serial_println!("[sysctl] Self-test PASSED");
}
