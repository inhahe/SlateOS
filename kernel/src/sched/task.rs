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
}

impl Context {
    /// An empty context (all zeros).  Used for the idle task whose
    /// context is captured on its first yield.
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

/// Stack size for kernel tasks: 2 frames = 32 KiB.
///
/// Typical kernel stacks are 8–16 KiB on Linux.  We use 32 KiB
/// (2 × 16 KiB frames) for extra headroom — stack overflow in the
/// kernel is fatal and hard to debug.
#[allow(clippy::arithmetic_side_effects)]
pub const TASK_STACK_SIZE: usize = 2 * FRAME_SIZE;

/// Number of frames per task stack.
#[allow(clippy::arithmetic_side_effects)]
const TASK_STACK_FRAMES: usize = TASK_STACK_SIZE / FRAME_SIZE;

/// Maximum number of ticks a CPU burst can be to still count as
/// "interactive."  At 100 Hz, 5 ticks = 50 ms.  Tasks that use
/// less than this before blocking are considered I/O-bound / interactive.
pub const INTERACTIVE_THRESHOLD_TICKS: u64 = 5;

/// Number of priority levels to boost interactive tasks.
///
/// A task at priority 16 that is detected as interactive will
/// effectively schedule at priority 14 (16 - 2).  The boost is
/// clamped so it never goes above priority 0 (highest).
pub const INTERACTIVE_BOOST: u8 = 2;

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
}

impl Task {
    /// Get the effective scheduling priority, accounting for interactive boost.
    ///
    /// If the task is interactive, the effective priority is
    /// `priority - INTERACTIVE_BOOST` (clamped to 0).
    #[must_use]
    pub fn effective_priority(&self) -> u8 {
        if self.interactive {
            self.priority.saturating_sub(INTERACTIVE_BOOST)
        } else {
            self.priority
        }
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

    /// Increment the burst tick counter.  Called on each timer tick
    /// while the task is Running.
    pub fn tick_burst(&mut self) {
        self.burst_ticks = self.burst_ticks.saturating_add(1);
    }

    /// Create the idle task (task 0).
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

        // Allocate stack frames.
        //
        // For a 2-frame (32 KiB) stack, we allocate order 1 (2 frames).
        // The buddy allocator returns a physically contiguous block.
        let order = if TASK_STACK_FRAMES <= 1 { 0 } else {
            TASK_STACK_FRAMES.next_power_of_two().trailing_zeros() as usize
        };
        let stack_frame = frame::alloc_order(order)?;
        let stack_phys = stack_frame.addr();

        // Convert to virtual address via HHDM.
        let hhdm = page_table::hhdm().ok_or(KernelError::NotSupported)?;
        let stack_bottom = stack_phys + hhdm;
        let stack_top = stack_bottom + TASK_STACK_SIZE as u64;

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

        let order = if TASK_STACK_FRAMES <= 1 { 0 } else {
            TASK_STACK_FRAMES.next_power_of_two().trailing_zeros() as usize
        };

        if let Some(frame) = frame::PhysFrame::from_addr(self.stack_phys) {
            // SAFETY: Caller guarantees no CPU is using this stack.
            unsafe { frame::free_order(frame, order)?; }
        }

        self.stack_phys = 0;
        self.stack_bottom = 0;
        Ok(())
    }

    /// Get the task name as a string slice (for debug output).
    pub fn name_str(&self) -> &str {
        let bytes = self.name.get(..self.name_len).unwrap_or(&[]);
        core::str::from_utf8(bytes).unwrap_or("<invalid>")
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
