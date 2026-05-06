//! Task (kernel thread) representation.
//!
//! A *task* is the schedulable unit — a kernel thread with its own
//! stack and saved register context.  User processes will contain one
//! or more tasks (threads), but at this stage we only have kernel-mode
//! tasks used during boot and for kernel services.
//!
//! ## Task Lifecycle
//!
//! ```text
//! Created ──► Ready ──► Running ──► Ready   (yield / preempt)
//!                          │
//!                          ├──► Blocked     (wait for event)
//!                          │       │
//!                          │       └──► Ready (event fired)
//!                          │
//!                          ├──► Suspended   (paused by user/system)
//!                          │       │
//!                          │       └──► Ready (resumed)
//!                          │
//!                          └──► Dead        (exited / killed)
//! ```

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE};
use crate::mm::page_table;
use crate::serial_println;
use super::fpu::FpuState;
use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Task ID
// ---------------------------------------------------------------------------

/// Unique identifier for a task.
///
/// IDs are monotonically increasing and never reused.  Zero is reserved
/// for the idle task.
pub type TaskId = u64;

/// Counter for generating unique task IDs.
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh, unique task ID.
fn alloc_task_id() -> TaskId {
    // Relaxed is fine: we only need uniqueness, not ordering relative
    // to other memory operations.
    NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Task state
// ---------------------------------------------------------------------------

/// The scheduling state of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// In the run queue, eligible to be scheduled.
    Ready,
    /// Currently executing on a CPU.
    Running,
    /// Waiting for an event (I/O, lock, timer, etc.).  Not in the run
    /// queue — will be re-enqueued when the event fires.
    Blocked,
    /// Paused by the user or system.  Memory stays intact but the task
    /// is never scheduled until explicitly resumed.
    Suspended,
    /// Terminated.  Awaiting resource cleanup.
    Dead,
}

impl core::fmt::Display for TaskState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ready => f.write_str("ready"),
            Self::Running => f.write_str("running"),
            Self::Blocked => f.write_str("blocked"),
            Self::Suspended => f.write_str("suspended"),
            Self::Dead => f.write_str("dead"),
        }
    }
}

// ---------------------------------------------------------------------------
// CPU context (saved registers for context switch)
// ---------------------------------------------------------------------------

/// Saved CPU register state for context switching.
///
/// On `x86_64`, the System V AMD64 ABI defines `rbx`, `rbp`, `r12`–`r15`
/// as callee-saved.  The context switch function saves only these plus
/// `rsp`.  Caller-saved registers (`rax`, `rcx`, `rdx`, `rsi`, `rdi`,
/// `r8`–`r11`) are already saved by the compiler-generated code that
/// calls into the scheduler.
///
/// **Layout must match the assembly in `context.rs` exactly.**
///
/// For a newly created task:
/// - `rbx` holds the entry function pointer.
/// - `r12` holds the argument to the entry function.
/// - `rsp` points to a prepared stack with the trampoline address.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rsp: u64,
    /// Saved RFLAGS register.
    ///
    /// Ensures each task resumes with its correct interrupt state.
    /// Without this, a task preempted after `process_pending()` (which
    /// calls CLI) would context-switch to another task that then runs
    /// with interrupts disabled — permanently losing timer ticks and
    /// device IRQ delivery on that CPU.
    pub rflags: u64,
}

impl Context {
    /// An empty context (all zeros).  Used for the idle task whose
    /// context is captured on its first yield.
    ///
    /// `rflags` starts at 0 — the first context switch out of the idle
    /// task captures the actual RFLAGS.  When the idle task is resumed,
    /// it gets the RFLAGS it had when it last called `switch_context`.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            rbx: 0,
            rbp: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rsp: 0,
            rflags: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Task struct
// ---------------------------------------------------------------------------

/// Number of priority levels.  0 = highest (real-time), 31 = lowest
/// (idle / background).
pub const NUM_PRIORITIES: usize = 32;

/// Priority level for the idle task (lowest possible).
// Truncation: NUM_PRIORITIES is 32, so 31 fits in u8.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
pub const IDLE_PRIORITY: u8 = (NUM_PRIORITIES - 1) as u8;

/// Default priority for normal tasks.
pub const DEFAULT_PRIORITY: u8 = 16;

/// Stack size for kernel tasks: 4 frames = 64 KiB.
///
/// Linux uses 4 pages (16 KiB) with 4 KiB pages.  We use 4 frames
/// (64 KiB) with 16 KiB pages — proportionally equivalent.  Debug
/// builds require more stack than release due to unoptimized frames
/// (no inlining, no dead-store elimination).  32 KiB was insufficient
/// for deep call chains (scheduler → wait queue → lock → yield).
#[allow(clippy::arithmetic_side_effects)]
pub const TASK_STACK_SIZE: usize = 4 * FRAME_SIZE;

/// Number of frames per task stack.
#[allow(clippy::arithmetic_side_effects)]
const TASK_STACK_FRAMES: usize = TASK_STACK_SIZE / FRAME_SIZE;

/// Magic value written to the bottom of every kernel stack.
///
/// Checked on every context switch.  If the value is corrupted, the
/// task has overflowed its stack and we panic immediately instead of
/// silently corrupting adjacent memory.
///
/// The canary is randomized at boot via the CSPRNG.  An attacker cannot
/// predict the value from the source code alone — they would need to
/// read kernel memory (which SMEP/SMAP/KASLR prevent).
///
/// Based on Linux's `CONFIG_SCHED_STACK_END_CHECK` mechanism, enhanced
/// with per-boot randomization (like `__stack_chk_guard` in GCC/Clang).
///
/// Initialized by [`init_canary()`]; falls back to `0xDEAD_BEEF_CAFE_BABE`
/// if called before RNG is ready.
static STACK_CANARY_VALUE: AtomicU64 = AtomicU64::new(0xDEAD_BEEF_CAFE_BABE);

/// Get the current stack canary value.
///
/// After [`init_canary()`], this returns a random 64-bit value unique to
/// this boot.  Before init, returns the fallback constant.
#[inline]
pub fn stack_canary() -> u64 {
    STACK_CANARY_VALUE.load(Ordering::Relaxed)
}

/// Randomize the stack canary using the kernel CSPRNG.
///
/// Called once during early boot, after `rng::init()`.  After this,
/// `stack_canary()` returns an unpredictable per-boot value.
pub fn init_canary() {
    let random = crate::rng::next_u64();
    // Ensure the canary is never zero (trivial to guess) or the sentinel
    // (which would confuse watermark tracking).
    let canary = if random == 0 || random == STACK_SENTINEL {
        random ^ 0x5A5A_5A5A_5A5A_5A5A
    } else {
        random
    };
    STACK_CANARY_VALUE.store(canary, Ordering::Release);
    crate::serial_println!("[sched] Stack canary randomized (per-boot CSPRNG)");
}


/// Sentinel pattern used to paint stack memory for watermark tracking.
///
/// On task creation, the stack is filled with this repeating 8-byte
/// pattern (after the canary).  By scanning from the bottom of the
/// stack upward, we can determine how deep the stack has grown —
/// the first location where the pattern is absent marks the high
/// water mark.
///
/// Based on Linux's `CONFIG_DEBUG_STACK_USAGE` (STACK_END_MAGIC).
pub const STACK_SENTINEL: u64 = 0xABBA_CDDC_1234_5678;

/// Maximum number of ticks a CPU burst can be to still count as
/// "interactive."  At 100 Hz, 5 ticks = 50 ms.  Tasks that use
/// less than this before blocking are considered I/O-bound / interactive.
pub const INTERACTIVE_THRESHOLD_TICKS: u64 = 5;

/// Maximum depth of the transitive PI chain walk.
///
/// When task A blocks on B's lock and B blocks on C's lock (and so on),
/// the chain-walking code boosts each owner up to this depth.  A limit
/// prevents infinite loops (from cycles in pathological/buggy lock
/// orderings) and bounds the time spent in the boost path.
///
/// Linux uses a limit of 10 (`MAX_LOCK_DEPTH`).  We match that —
/// real-world lock nesting beyond 10 levels is either a bug or a design
/// that should be rethought.
pub const PI_CHAIN_DEPTH_LIMIT: usize = 10;

/// Number of priority levels to boost interactive tasks.
///
/// A task at priority 16 that is detected as interactive will
/// effectively schedule at priority 14 (16 - 2).  The boost is
/// clamped so it never goes above priority 0 (highest).
pub const INTERACTIVE_BOOST: u8 = 2;

/// Default CPU affinity mask: all 64 CPUs allowed.
///
/// For systems with fewer CPUs, extra bits are harmless — the
/// scheduler only considers online CPUs.
pub const CPU_AFFINITY_ALL: u64 = u64::MAX;

/// A kernel task (thread).
pub struct Task {
    /// Unique identifier (never reused).
    pub id: TaskId,
    /// Human-readable name for debug output.
    pub name: [u8; 32],
    /// Length of the name (bytes used in `name`).
    pub name_len: usize,
    /// Current scheduling state.
    pub state: TaskState,
    /// Base priority level (0 = highest, 31 = lowest).
    ///
    /// This is the user-assigned priority.  The effective scheduling
    /// priority may be higher (lower number) due to interactive boost.
    pub priority: u8,
    /// Saved CPU register state.
    pub context: Context,
    /// Physical address of the stack's backing frame(s).
    /// Zero for the idle task (uses the bootloader stack).
    pub stack_phys: u64,
    /// Virtual address of the bottom of the stack (lowest address).
    /// The stack grows downward from `stack_bottom + TASK_STACK_SIZE`.
    pub stack_bottom: u64,
    /// PML4 physical address for this task's address space.
    ///
    /// 0 = kernel address space (no CR3 switch needed, uses the
    /// boot-time PML4).  Non-zero = the task belongs to a process
    /// with its own page table hierarchy — the scheduler will load
    /// this PML4 via `write_cr3` on context switch.
    pub pml4_phys: u64,

    // --- Interactive task detection fields ---

    /// Number of timer ticks the task has run in the current burst.
    ///
    /// Reset to 0 each time the task transitions from Blocked → Ready
    /// (i.e., wakes from I/O).  Incremented on each timer tick while
    /// the task is Running.
    pub burst_ticks: u64,

    /// Exponentially weighted moving average of CPU burst lengths.
    ///
    /// Updated each time the task blocks (transitions Running → Blocked).
    /// Low values (< `INTERACTIVE_THRESHOLD_TICKS`) indicate an
    /// interactive / I/O-bound task.
    ///
    /// Uses fixed-point arithmetic: stored as `avg * 8` to avoid
    /// floating point.  Divide by 8 to get the actual average.
    ///
    /// EWMA formula: `avg = (7 * avg + burst_ticks) / 8`
    /// (equivalent to α = 1/8, weighting recent history ~87.5%).
    pub avg_burst_x8: u64,

    /// Whether the task currently has an interactive priority boost.
    ///
    /// When true, the task is enqueued at `effective_priority()` which
    /// is `priority - INTERACTIVE_BOOST` (clamped to 0).
    pub interactive: bool,

    /// Priority inherited from higher-priority tasks blocked on a
    /// PI (Priority Inheritance) mutex held by this task.
    ///
    /// When set, [`effective_priority`](Self::effective_priority)
    /// returns the minimum (highest priority, i.e., lowest number) of
    /// the base effective priority and this inherited value.
    ///
    /// Managed by the futex PI subsystem: set when a high-priority
    /// task blocks on our lock, cleared when we release the lock.
    pub inherited_priority: Option<u8>,

    /// The PI futex address this task is currently blocked on, if any.
    ///
    /// Set by `futex_lock_pi()` just before the task blocks on a
    /// contended PI mutex.  Cleared when the task acquires the lock
    /// or is interrupted.
    ///
    /// Used for **transitive priority inheritance**: when task A blocks
    /// on a lock held by B, and B is itself blocked on a lock held by
    /// C, the chain A→B→C is walked by following each task's
    /// `blocked_on_pi_addr` to find the next owner.  This ensures C
    /// gets boosted to A's priority, preventing unbounded priority
    /// inversion chains.
    ///
    /// The chain walk is depth-limited by [`PI_CHAIN_DEPTH_LIMIT`] to
    /// prevent cycles or excessive traversal.
    pub blocked_on_pi_addr: Option<u64>,

    /// The CPU this task last ran on.
    ///
    /// Used for cache-warm scheduling: when enqueuing, prefer the CPU
    /// the task last ran on (its caches are warm there).  For new
    /// tasks, defaults to the spawning CPU.
    pub last_cpu: usize,

    /// CPU affinity mask.
    ///
    /// Bit N set means the task is allowed to run on CPU N.  The
    /// default (`CPU_AFFINITY_ALL`) allows all CPUs.  When enqueuing
    /// or stealing, the scheduler only places this task on CPUs that
    /// are set in the mask.
    ///
    /// If `last_cpu` is in the mask, it is preferred (cache locality).
    /// Otherwise the lightest-loaded allowed CPU is chosen.
    pub cpu_affinity: u64,

    // --- CPU time accounting ---

    /// Total CPU time consumed by this task, in timer ticks.
    ///
    /// Incremented on every timer tick while the task is Running.
    /// At 100 Hz, each tick = 10 ms.  Used by `ps`/`top`-style
    /// commands and resource accounting.
    pub total_ticks: u64,

    /// Total CPU cycles consumed by this task (TSC-based).
    ///
    /// Updated at each context switch: the delta between the time this
    /// task was switched-in and switched-out is added.  Provides
    /// nanosecond-precision CPU time accounting (unlike `total_ticks`
    /// which has 10ms granularity).
    ///
    /// Convert to nanoseconds via `bench::cycles_to_ns(total_cycles)`.
    pub total_cycles: u64,

    /// Total number of times this task has been scheduled (context
    /// switched into).
    ///
    /// Useful for detecting tasks that are being scheduled too
    /// frequently (excessive context switch overhead).
    pub schedule_count: u64,

    // --- Wait time tracking (starvation detection) ---

    /// Tick count when the task last entered the Ready state.
    ///
    /// Set when transitioning from any state to Ready (blocked→ready,
    /// or newly spawned and enqueued).  Used to measure how long the
    /// task waited in the run queue before being dispatched.
    ///
    /// A value of 0 means "not waiting" (task is Running/Blocked/Dead).
    pub ready_since_tick: u64,

    /// Cumulative time spent waiting in the Ready state (in ticks).
    ///
    /// Updated each time the task is dispatched (scheduled in): the
    /// delta `current_tick - ready_since_tick` is added.  This allows
    /// detecting starvation — tasks with high `total_wait_ticks` relative
    /// to `total_ticks` are spending more time waiting than running.
    pub total_wait_ticks: u64,

    /// Maximum single wait duration seen (in ticks).
    ///
    /// Tracks the longest continuous wait in the run queue.  Useful
    /// for latency profiling — a high max_wait_ticks indicates the
    /// task experienced scheduling starvation at some point.
    pub max_wait_ticks: u64,

    // --- CPU bandwidth limiting ---

    /// CPU bandwidth quota as a percentage (0 = unlimited, 1–100).
    ///
    /// Limits how many ticks per bandwidth period (100 ticks = 1 second)
    /// this task may consume.  A value of 50 means 50% of one CPU core.
    ///
    /// Enforcement: when `cpu_period_used >= cpu_quota_pct`, the task is
    /// throttled (removed from the run queue) until the next period reset.
    /// The BSP drives period resets every [`BANDWIDTH_PERIOD_TICKS`] ticks.
    ///
    /// Set via [`set_cpu_quota`](super::set_cpu_quota).
    pub cpu_quota_pct: u8,

    /// Ticks consumed by this task in the current bandwidth period.
    ///
    /// Incremented on each timer tick.  Reset to 0 at each period
    /// boundary (every 100 ticks / 1 second).  When this reaches
    /// `cpu_quota_pct`, the task is throttled.
    pub cpu_period_used: u64,

    /// Whether this task is currently throttled due to exceeding its
    /// CPU bandwidth quota.
    ///
    /// When true, the task is in [`TaskState::Ready`] but NOT in any
    /// run queue.  It will be re-enqueued by the period-reset logic
    /// in [`unthrottle_expired`](super::unthrottle_expired).
    pub throttled: bool,

    /// If this task's stack was allocated via the kstack allocator
    /// (with hardware guard pages), this holds the slot index for
    /// deallocation.  `None` means the stack uses legacy HHDM-based
    /// allocation (idle tasks, AP idle tasks) or is externally provided.
    pub kstack_slot: Option<usize>,

    /// Saved FPU/SSE state (x87 + XMM0-XMM15).
    ///
    /// Saved by `fxsave64` on switch-out, restored by `fxrstor64` on
    /// switch-in.  Initialized to a clean default state (all registers
    /// zeroed, FCW=0x037F, MXCSR=0x1F80) for new tasks.
    ///
    /// 512 bytes, 16-byte aligned.  Placed last in the struct to avoid
    /// padding between smaller fields.
    pub fpu_state: FpuState,
}

impl Task {
    /// Get the effective scheduling priority, accounting for both
    /// interactive boost and priority inheritance.
    ///
    /// Returns the minimum (highest priority) of:
    /// - Base priority with interactive boost (if applicable)
    /// - Inherited priority from PI futex (if any)
    ///
    /// Lower number = higher priority.
    #[must_use]
    pub fn effective_priority(&self) -> u8 {
        let base = if self.interactive {
            self.priority.saturating_sub(INTERACTIVE_BOOST)
        } else {
            self.priority
        };
        match self.inherited_priority {
            Some(inh) => base.min(inh),
            None => base,
        }
    }

    /// Check whether this task is allowed to run on `cpu`.
    #[must_use]
    pub fn can_run_on(&self, cpu: usize) -> bool {
        if cpu >= 64 { return false; }
        (self.cpu_affinity >> cpu) & 1 != 0
    }

    /// Update the burst EWMA when the task is about to block.
    ///
    /// Called when the task transitions from Running → Blocked.
    /// Records the current burst length into the exponentially weighted
    /// moving average and determines if the task is interactive.
    pub fn record_block(&mut self) {
        // EWMA update: avg = (7 * avg + burst * 8) / 8
        // Stored as avg_x8, so: avg_x8 = (7 * avg_x8 / 8) + burst
        // Simplified: avg_x8 = avg_x8 - avg_x8/8 + burst
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.avg_burst_x8 = self.avg_burst_x8
                .saturating_sub(self.avg_burst_x8 / 8)
                .saturating_add(self.burst_ticks);
        }

        // Interactive if average burst < threshold (compare x8 values).
        #[allow(clippy::arithmetic_side_effects)]
        let threshold_x8 = INTERACTIVE_THRESHOLD_TICKS * 8;
        self.interactive = self.avg_burst_x8 < threshold_x8;

        // Reset burst counter for the next wake cycle.
        self.burst_ticks = 0;
    }

    /// Increment the burst tick counter and total CPU time.
    /// Called on each timer tick while the task is Running.
    pub fn tick_burst(&mut self) {
        self.burst_ticks = self.burst_ticks.saturating_add(1);
        self.total_ticks = self.total_ticks.saturating_add(1);
    }

    /// Mark this task as Ready and record the timestamp for wait
    /// time tracking.
    ///
    /// Call this instead of directly setting `state = TaskState::Ready`
    /// to ensure the `ready_since_tick` field is properly updated for
    /// starvation detection.
    #[inline]
    pub fn mark_ready(&mut self, current_tick: u64) {
        self.state = TaskState::Ready;
        self.ready_since_tick = current_tick;
    }

    /// Record that this task has been dispatched (scheduled in).
    ///
    /// Computes the wait time (time spent in Ready state) and updates
    /// the `total_wait_ticks` and `max_wait_ticks` counters.
    ///
    /// Call this when picking the task for execution (in the context
    /// switch path).
    #[inline]
    pub fn record_dispatch(&mut self, current_tick: u64) {
        if self.ready_since_tick > 0 {
            let waited = current_tick.saturating_sub(self.ready_since_tick);
            self.total_wait_ticks = self.total_wait_ticks.saturating_add(waited);
            if waited > self.max_wait_ticks {
                self.max_wait_ticks = waited;
            }
            // Feed the system-wide latency histogram.
            super::record_dispatch_latency(waited);
        }
        self.ready_since_tick = 0; // No longer waiting.
        self.schedule_count = self.schedule_count.saturating_add(1);
    }

    /// Measure stack usage by scanning the sentinel pattern.
    ///
    /// Returns the number of bytes of stack actually used (high water
    /// mark) since task creation.  Returns `None` if the task doesn't
    /// have an allocated stack (idle tasks, AP idle tasks).
    ///
    /// The measurement works by scanning from the bottom of the stack
    /// (lowest address, just above the canary) upward, counting how
    /// many sentinel words are still intact.  The first non-sentinel
    /// word marks where the stack has grown to.
    ///
    /// This is a read-only operation and is safe to call while the task
    /// is running (on another CPU), though the result may be slightly
    /// stale.
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn stack_usage_bytes(&self) -> Option<usize> {
        if self.stack_bottom == 0 {
            return None; // Idle task — no allocated stack.
        }

        // Scan from just above the canary (offset 8) upward.
        // Each intact sentinel word means 8 bytes of unused stack.
        let end_addr = self.stack_bottom.wrapping_add(TASK_STACK_SIZE as u64);
        let total_scannable = TASK_STACK_SIZE.saturating_sub(8);

        let mut unused_words: usize = 0;

        // SAFETY: The stack memory range [stack_bottom..stack_bottom+TASK_STACK_SIZE]
        // was allocated as contiguous physical frames mapped via HHDM.
        // We're reading (not writing) aligned u64 values within that range.
        unsafe {
            let start = self.stack_bottom as *const u64;
            let mut ptr = start.add(1); // Skip canary (first 8 bytes).
            let end = end_addr as *const u64;
            while (ptr as u64) < (end as u64) {
                if ptr::read_volatile(ptr) != STACK_SENTINEL {
                    break;
                }
                unused_words = unused_words.saturating_add(1);
                ptr = ptr.add(1);
            }
        }

        let unused_bytes = unused_words.saturating_mul(8);
        Some(total_scannable.saturating_sub(unused_bytes))
    }

    /// Stack usage as a percentage (0-100).
    ///
    /// Returns `None` for tasks without allocated stacks.
    #[must_use]
    #[allow(dead_code)]
    pub fn stack_usage_pct(&self) -> Option<u8> {
        let used = self.stack_usage_bytes()?;
        let total = TASK_STACK_SIZE.saturating_sub(8); // Exclude canary.
        if total == 0 { return Some(0); }
        #[allow(clippy::arithmetic_side_effects)]
        let pct = (used * 100) / total;
        Some(pct.min(100) as u8)
    }

    /// Create the idle task (task 0) for the BSP.
    ///
    /// The idle task uses the bootloader-provided stack; no allocation
    /// is needed.  Its context starts empty and is populated when it
    /// first yields to another task.
    #[must_use]
    pub fn new_idle() -> Self {
        let mut name = [0u8; 32];
        let tag = b"idle";
        name[..tag.len()].copy_from_slice(tag);

        Self {
            id: 0,
            name,
            name_len: tag.len(),
            state: TaskState::Running,
            priority: IDLE_PRIORITY,
            context: Context::empty(),
            stack_phys: 0,
            stack_bottom: 0,
            pml4_phys: 0, // Kernel address space.
            burst_ticks: 0,
            avg_burst_x8: 0,
            interactive: false,
            inherited_priority: None,
            blocked_on_pi_addr: None,
            last_cpu: 0,
            cpu_affinity: CPU_AFFINITY_ALL,
            total_ticks: 0,
            total_cycles: 0,
            schedule_count: 0,
            ready_since_tick: 0,
            total_wait_ticks: 0,
            max_wait_ticks: 0,
            cpu_quota_pct: 0,
            cpu_period_used: 0,
            throttled: false,
            kstack_slot: None,
            fpu_state: FpuState::new_default(),
        }
    }

    /// Create an idle task for an Application Processor.
    ///
    /// Like the BSP's idle task (task 0), the AP idle task uses an
    /// externally-allocated stack (the AP trampoline stack) and has no
    /// canary.  Its context starts empty and is populated when the AP
    /// first yields to a real task.
    ///
    /// Each AP gets its own idle task so there's always a fallback task
    /// for every CPU.  Without this, an AP whose only task blocks has
    /// no valid task to switch to — it would need an ad-hoc idle loop
    /// inside schedule_inner which is error-prone on SMP.
    #[must_use]
    pub fn new_ap_idle(cpu_index: usize) -> Self {
        let id = alloc_task_id();
        let mut name = [0u8; 32];
        // Format: "idle/N" where N is the CPU index.
        let tag = b"idle/";
        let tag_len = tag.len();
        name[..tag_len].copy_from_slice(tag);
        // Write CPU index digit(s).  Support up to 3-digit CPU indices.
        let idx_str = if cpu_index < 10 {
            name[tag_len] = b'0' + cpu_index as u8;
            tag_len + 1
        } else if cpu_index < 100 {
            #[allow(clippy::arithmetic_side_effects)]
            {
                name[tag_len] = b'0' + (cpu_index / 10) as u8;
                name[tag_len + 1] = b'0' + (cpu_index % 10) as u8;
            }
            tag_len + 2
        } else {
            // Fallback: just "idle/X" for huge indices.
            name[tag_len] = b'X';
            tag_len + 1
        };

        Self {
            id,
            name,
            name_len: idx_str,
            state: TaskState::Running,
            priority: IDLE_PRIORITY,
            context: Context::empty(),
            stack_phys: 0,
            stack_bottom: 0,   // Externally allocated (AP trampoline stack).
            pml4_phys: 0,      // Kernel address space.
            burst_ticks: 0,
            avg_burst_x8: 0,
            interactive: false,
            inherited_priority: None,
            blocked_on_pi_addr: None,
            last_cpu: cpu_index,
            cpu_affinity: CPU_AFFINITY_ALL,
            total_ticks: 0,
            total_cycles: 0,
            schedule_count: 0,
            ready_since_tick: 0,
            total_wait_ticks: 0,
            max_wait_ticks: 0,
            cpu_quota_pct: 0,
            cpu_period_used: 0,
            throttled: false,
            kstack_slot: None,
            fpu_state: FpuState::new_default(),
        }
    }

    /// Create a new kernel task with an allocated stack.
    ///
    /// The task starts in [`TaskState::Ready`] and will run `entry(arg)`
    /// when first scheduled.  When `entry` returns, the task is
    /// automatically marked [`TaskState::Dead`].
    ///
    /// # Errors
    ///
    /// - [`KernelError::OutOfMemory`] if stack allocation fails.
    /// - [`KernelError::InvalidArgument`] if `task_name` is empty.
    #[allow(clippy::arithmetic_side_effects)]
    pub fn new_kernel(
        task_name: &[u8],
        priority: u8,
        entry: extern "C" fn(u64),
        arg: u64,
        pml4_phys: u64,
    ) -> KernelResult<Self> {
        if task_name.is_empty() {
            return Err(KernelError::InvalidArgument);
        }

        let id = alloc_task_id();

        // Copy name (truncate if too long).
        let mut name = [0u8; 32];
        let copy_len = task_name.len().min(name.len());
        name[..copy_len].copy_from_slice(&task_name[..copy_len]);

        // Allocate stack with hardware guard page (preferred) or fall
        // back to HHDM-based allocation if the kstack subsystem isn't
        // initialized yet (early boot tasks before mm::kstack::init()).
        let (stack_phys, stack_bottom, stack_top, kstack_slot) =
            if let Ok(info) = crate::mm::kstack::alloc() {
                // Guard-page stack: the kstack allocator maps physical frames
                // into a dedicated virtual region with an unmapped guard page
                // below.  Any stack overflow triggers an immediate page fault.
                (info.stack_phys, info.stack_bottom, info.stack_top, Some(info.slot))
            } else {
                // Fallback: allocate via buddy allocator + HHDM (no guard page).
                // Only used during early boot before kstack::init().
                let order = if TASK_STACK_FRAMES <= 1 { 0 } else {
                    TASK_STACK_FRAMES.next_power_of_two().trailing_zeros() as usize
                };
                let stack_frame = frame::alloc_order(order)?;
                let sp = stack_frame.addr();
                let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
                let sb = sp + hhdm;
                let st = sb + TASK_STACK_SIZE as u64;
                (sp, sb, st, None)
            };

        // Plant the stack canary at the very bottom of the stack.
        // This is the first 8 bytes — furthest from the stack top,
        // so it will be the last thing overwritten on overflow.
        // Retained as defense-in-depth alongside the guard page.
        // SAFETY: stack_bottom is a valid, freshly-allocated address
        // (either HHDM or kstack-mapped).
        unsafe {
            ptr::write_volatile(stack_bottom as *mut u64, stack_canary());
        }

        // Paint the stack with a sentinel pattern for watermark tracking.
        // Skip the first 8 bytes (canary) and fill the rest.  This lets
        // us later scan from the bottom up to find how deep the stack grew.
        // SAFETY: The entire [stack_bottom..stack_top] range is freshly
        // allocated, contiguous memory.  The pointer arithmetic stays within
        // the 32 KiB allocation (which is well below usize::MAX on 64-bit).
        #[allow(clippy::arithmetic_side_effects)]
        unsafe {
            let start = stack_bottom as *mut u64;
            let end_addr = stack_bottom.wrapping_add(TASK_STACK_SIZE as u64);
            let mut ptr = start.add(1); // Skip canary (first 8 bytes).
            let end = end_addr as *mut u64;
            while (ptr as u64) < (end as u64) {
                ptr.write(STACK_SENTINEL);
                ptr = ptr.add(1);
            }
        }

        // Set up the initial stack and context so that when
        // switch_context switches to this task, it "returns" into
        // the task_entry_trampoline which calls entry(arg).
        let context = Self::prepare_context(stack_top, entry, arg);

        Ok(Self {
            id,
            name,
            name_len: copy_len,
            state: TaskState::Ready,
            priority,
            context,
            stack_phys,
            stack_bottom,
            pml4_phys,
            burst_ticks: 0,
            avg_burst_x8: 0,
            interactive: false,
            inherited_priority: None,
            blocked_on_pi_addr: None,
            last_cpu: 0,
            cpu_affinity: CPU_AFFINITY_ALL,
            total_ticks: 0,
            total_cycles: 0,
            schedule_count: 0,
            ready_since_tick: 0,
            total_wait_ticks: 0,
            max_wait_ticks: 0,
            cpu_quota_pct: 0,
            cpu_period_used: 0,
            throttled: false,
            kstack_slot,
            fpu_state: FpuState::new_default(),
        })
    }

    /// Prepare the initial context and stack for a new task.
    ///
    /// Sets up a fake stack frame so that `switch_context`'s `ret`
    /// instruction jumps to `task_entry_trampoline`, which reads the
    /// entry function from `rbx` and the argument from `r12`.
    ///
    /// Stack layout (growing downward):
    /// ```text
    /// stack_top (high address)
    ///   [alignment padding if needed]
    ///   [trampoline address]  ← context.rsp points here
    /// ```
    #[allow(clippy::arithmetic_side_effects)]
    fn prepare_context(
        stack_top: u64,
        entry: extern "C" fn(u64),
        arg: u64,
    ) -> Context {
        // Resolve the trampoline address.
        //
        // SAFETY: We take the address of the extern symbol declared
        // adjacent to the global_asm! in context.rs.
        let trampoline_addr: u64 = {
            unsafe extern "C" { fn task_entry_trampoline(); }
            task_entry_trampoline as *const () as u64
        };

        // Align stack_top down to 16 bytes.
        let sp = stack_top & !0xF;

        // Push the trampoline address.  When switch_context does `ret`,
        // it pops this and jumps to the trampoline.
        //
        // After `ret` pops the 8-byte address, RSP will be at sp
        // (which is 16-byte aligned), matching the ABI requirement
        // that RSP is 16-byte aligned at a `call` boundary.
        let sp = sp - 8;

        // SAFETY: sp is within the freshly-allocated stack (we're 8
        // bytes below stack_top, which is well within the 32 KiB stack).
        // The stack memory is accessible via HHDM.
        unsafe {
            ptr::write(sp as *mut u64, trampoline_addr);
        }

        Context {
            rbx: entry as *const () as u64,
            rbp: 0,
            r12: arg,
            r13: 0,
            r14: 0,
            r15: 0,
            rsp: sp,
            // RFLAGS with IF=1 (interrupts enabled) and reserved bit 1
            // set.  When switch_context restores this via popfq, the new
            // task starts with interrupts enabled — the expected state
            // for all user and kernel tasks.
            rflags: 0x202,
        }
    }

    /// Free the task's stack memory.
    ///
    /// # Safety
    ///
    /// The task must be [`Dead`](TaskState::Dead) and no CPU may be
    /// using this task's stack.
    #[allow(clippy::arithmetic_side_effects)]
    pub unsafe fn free_stack(&mut self) -> KernelResult<()> {
        if self.stack_phys == 0 {
            // Idle task — no stack to free.
            return Ok(());
        }

        if let Some(slot) = self.kstack_slot {
            // Guard-page stack: use the kstack allocator to unmap, free
            // physical frames, remove guard VMA, and release the slot.
            let info = crate::mm::kstack::KstackInfo {
                stack_bottom: self.stack_bottom,
                stack_top: self.stack_bottom + TASK_STACK_SIZE as u64,
                stack_phys: self.stack_phys,
                slot,
            };
            // SAFETY: Caller guarantees no CPU is using this stack.
            unsafe { crate::mm::kstack::free(info)?; }
        } else {
            // Legacy HHDM-based stack: free via buddy allocator.
            let order = if TASK_STACK_FRAMES <= 1 { 0 } else {
                TASK_STACK_FRAMES.next_power_of_two().trailing_zeros() as usize
            };

            if let Some(frame) = frame::PhysFrame::from_addr(self.stack_phys) {
                // SAFETY: Caller guarantees no CPU is using this stack.
                unsafe { frame::free_order(frame, order)?; }
            }
        }

        self.stack_phys = 0;
        self.stack_bottom = 0;
        self.kstack_slot = None;
        Ok(())
    }

    /// Get the task name as a string slice (for debug output).
    pub fn name_str(&self) -> &str {
        let bytes = self.name.get(..self.name_len).unwrap_or(&[]);
        core::str::from_utf8(bytes).unwrap_or("<invalid>")
    }

    /// Verify the stack canary is intact.
    ///
    /// Called on every context switch (for the task that just ran).
    /// If the canary is corrupted, the task has overflowed its kernel
    /// stack.  We panic immediately because the alternative — silent
    /// memory corruption — is far worse.
    ///
    /// The idle task (stack_bottom == 0) uses the bootloader stack
    /// and has no canary — skip the check.
    #[inline]
    pub fn check_stack_canary(&self) {
        if self.stack_bottom == 0 {
            return; // Idle task, no canary.
        }
        // SAFETY: stack_bottom is a valid HHDM address for this task's
        // allocated stack.  The canary was written during new_kernel().
        let canary = unsafe {
            ptr::read_volatile(self.stack_bottom as *const u64)
        };
        if canary != stack_canary() {
            // Stack overflow detected.  Print as much info as possible
            // before halting — the stack is corrupted so we might crash
            // trying to print, but it's better than silent corruption.
            serial_println!(
                "FATAL: Stack canary corrupted for task {} ({})!",
                self.id, self.name_str()
            );
            serial_println!(
                "  Expected: {:#018x}, Found: {:#018x}",
                stack_canary(), canary
            );
            serial_println!(
                "  stack_bottom={:#x}, stack_top={:#x}",
                self.stack_bottom,
                self.stack_bottom.wrapping_add(TASK_STACK_SIZE as u64)
            );
            serial_println!(
                "FATAL: Kernel stack overflow is unrecoverable. Halting."
            );
            crate::cpu::halt_loop();
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        // Warn if a task with an allocated stack is dropped without
        // freeing it first.  This is a resource leak.
        if self.stack_phys != 0 {
            serial_println!(
                "[sched] WARNING: task {} ({}) dropped with stack still allocated",
                self.id,
                self.name_str()
            );
        }
    }
}
