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

    serial_println!("[sysctl] Self-test PASSED");
}
