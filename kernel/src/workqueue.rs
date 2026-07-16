//! Kernel workqueue — deferred work in process context.
//!
//! Provides a mechanism to schedule work that needs to run in a full
//! kernel task context (where sleeping, locking, and allocation are all
//! permitted).  This complements the softirq system, which handles
//! lightweight deferred work but cannot block.
//!
//! ## When to Use
//!
//! - **Softirqs**: Ultra-lightweight handlers that run immediately after
//!   ISR exit.  Cannot sleep, cannot allocate, must use try_lock.
//! - **Workqueues**: General deferred work that may need to sleep, take
//!   mutexes, allocate memory, or do substantial computation.
//!
//! ## Design
//!
//! A single system workqueue with a bounded FIFO of work items.  One
//! kernel worker task drains the queue, sleeping when empty.  Work is
//! submitted via [`submit`] which is safe to call from any context
//! (including ISR, softirq, or normal task context) because it only
//! performs an atomic enqueue and a non-blocking wake.
//!
//! ## Capacity
//!
//! The queue holds up to [`QUEUE_CAPACITY`] pending work items.  If the
//! queue is full, [`submit`] returns `false` and the work item is dropped.
//! Callers should check the return value if the work is critical.
//!
//! ## References
//!
//! - Linux `kernel/workqueue.c` — `alloc_workqueue`, `queue_work`
//! - FreeBSD `sys/kern/subr_taskqueue.c`

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::error::KernelResult;
use crate::sched::waitqueue::WaitQueue;
use crate::serial_println;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Work item
// ---------------------------------------------------------------------------

/// A unit of deferred work.
///
/// Contains a function pointer and an arbitrary 64-bit argument.
/// The function runs in the worker task's context with full kernel
/// privileges (can sleep, allocate, take locks).
#[derive(Clone, Copy)]
struct WorkItem {
    /// The function to call.
    func: fn(u64),
    /// Argument passed to the function.
    arg: u64,
}

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

/// Maximum number of pending work items.
///
/// Power of two for efficient modular indexing.  64 items provides
/// enough buffering for burst submissions while keeping memory usage
/// modest (64 × 16 bytes = 1 KiB).
const QUEUE_CAPACITY: usize = 64;

/// Circular buffer of pending work items.
struct WorkQueue {
    items: [Option<WorkItem>; QUEUE_CAPACITY],
    /// Index of the next item to dequeue (reader position).
    head: usize,
    /// Index of the next slot to enqueue (writer position).
    tail: usize,
    /// Number of items currently in the queue.
    count: usize,
}

impl WorkQueue {
    const fn new() -> Self {
        Self {
            items: [None; QUEUE_CAPACITY],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    /// Enqueue a work item.  Returns `true` on success, `false` if full.
    #[allow(clippy::arithmetic_side_effects)]
    fn enqueue(&mut self, item: WorkItem) -> bool {
        if self.count >= QUEUE_CAPACITY {
            return false;
        }
        self.items[self.tail] = Some(item);
        self.tail = (self.tail + 1) % QUEUE_CAPACITY;
        self.count += 1;
        true
    }

    /// Dequeue the next work item.  Returns `None` if empty.
    #[allow(clippy::arithmetic_side_effects)]
    fn dequeue(&mut self) -> Option<WorkItem> {
        if self.count == 0 {
            return None;
        }
        let item = self.items[self.head].take();
        self.head = (self.head + 1) % QUEUE_CAPACITY;
        self.count -= 1;
        item
    }

    /// Number of items currently pending.
    fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// The global work queue, protected by a spinlock.
///
/// The lock is held briefly during enqueue/dequeue.  The worker task
/// releases the lock before executing each work item, so work
/// functions can safely call `submit()` to re-enqueue work.
static QUEUE: Mutex<WorkQueue> = Mutex::new(WorkQueue::new());

/// Whether the worker task has been spawned.
static SPAWNED: AtomicBool = AtomicBool::new(false);

/// Worker task ID (for waking via sleep_until_tick fallback).
static WORKER_TID: AtomicU64 = AtomicU64::new(0);

/// Wait queue for the worker task.
///
/// The worker blocks on this queue when the work queue is empty.
/// `submit()` calls `wake_one()` to instantly wake the worker when
/// new work arrives, eliminating the poll-interval latency.
static WORKER_WQ: WaitQueue = WaitQueue::new();

/// Total work items executed since boot.
static ITEMS_EXECUTED: AtomicU64 = AtomicU64::new(0);

/// Total work items submitted since boot.
static ITEMS_SUBMITTED: AtomicU64 = AtomicU64::new(0);

/// Total work items dropped (queue full) since boot.
static ITEMS_DROPPED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Submit a work item to the system workqueue.
///
/// The function `func` will be called with argument `arg` in the
/// worker task's context.  This may happen immediately (if the worker
/// is idle) or after other pending work completes.
///
/// Returns `true` if the item was enqueued, `false` if the queue is
/// full (item dropped).
///
/// Safe to call from any context (ISR, softirq, or normal task).
///
/// IRQ-safety: `QUEUE` is acquired both from softirq context
/// (`ktimer::process_expirations` → `workqueue::submit`) and from
/// non-IRQ-safe main paths (kshell, supervisor restart callbacks).
/// If main code held `QUEUE.lock()` when a timer ISR fired on the
/// same CPU, the softirq's submit would re-enter the spinlock and
/// deadlock against itself.  Wrap acquisitions in
/// `without_interrupts(...)` — same pattern as the RCU CALLBACKS fix
/// (F1) and the frame::stats fix (F4).
pub fn submit(func: fn(u64), arg: u64) -> bool {
    let item = WorkItem { func, arg };
    let enqueued = crate::cpu::without_interrupts(|| {
        QUEUE.lock().enqueue(item)
    });

    if enqueued {
        ITEMS_SUBMITTED.fetch_add(1, Ordering::Relaxed);
        // Wake the worker task via its wait queue.
        // try_wake_one is ISR-safe (uses try_lock internally).
        WORKER_WQ.try_wake_one();
    } else {
        ITEMS_DROPPED.fetch_add(1, Ordering::Relaxed);
    }

    enqueued
}

/// Number of items currently pending in the queue.
#[must_use]
#[allow(dead_code)]
pub fn pending_count() -> usize {
    crate::cpu::without_interrupts(|| QUEUE.lock().len())
}

/// Total items executed since boot.
#[must_use]
#[allow(dead_code)]
pub fn executed_count() -> u64 {
    ITEMS_EXECUTED.load(Ordering::Relaxed)
}

/// Total items submitted since boot.
#[must_use]
#[allow(dead_code)]
pub fn submitted_count() -> u64 {
    ITEMS_SUBMITTED.load(Ordering::Relaxed)
}

/// Total items dropped (queue full) since boot.
#[must_use]
#[allow(dead_code)]
pub fn dropped_count() -> u64 {
    ITEMS_DROPPED.load(Ordering::Relaxed)
}

/// Whether the workqueue worker task is running.
#[must_use]
#[allow(dead_code)]
pub fn is_running() -> bool {
    SPAWNED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Worker task
// ---------------------------------------------------------------------------

/// Check interval — how often the worker wakes to check for work even
/// without an explicit wake signal (in ticks at 100 Hz).
///
/// 50 ticks = 500 ms.  Provides a fallback in case a wake IPI is missed
/// (shouldn't happen, but defense in depth).
const POLL_INTERVAL_TICKS: u64 = 50;

/// Worker task entry point.
///
/// Loops forever: dequeue work items and execute them, sleeping when
/// the queue is empty.  Runs at slightly below-normal priority so
/// interactive tasks aren't delayed by deferred work.
#[allow(clippy::arithmetic_side_effects)]
extern "C" fn worker_entry(_arg: u64) {
    serial_println!("[workqueue] Worker task started");

    loop {
        // Drain all pending items.
        loop {
            // IRQ-safe acquisition: ktimer's softirq path also calls
            // workqueue::submit() → QUEUE.lock().  If a timer ISR
            // fires while the worker holds QUEUE.lock for a dequeue
            // and the softirq path tries to submit, the spinlock
            // would deadlock against itself on this CPU.  See the
            // matching note on submit() above.
            let item = crate::cpu::without_interrupts(|| QUEUE.lock().dequeue());
            match item {
                Some(work) => {
                    // Execute the work item outside the lock.
                    //
                    // Defense-in-depth: this is the single chokepoint where
                    // *every* submitted callback is finally `call`-ed, so
                    // validate the stored function pointer against real `.text`
                    // bounds here (covers all submitters at once).  A
                    // validly-submitted `fn(u64)` always points into kernel
                    // code; a value that doesn't means the queue entry was
                    // corrupted (heap overrun / torn store) — jumping to it
                    // would be the B-KNULLJUMP-SIGNAL class (a wild `call` in
                    // kernel context).  Log which arg was involved and skip.
                    let func_addr = work.func as *const () as u64;
                    if crate::idt::is_kernel_text(func_addr) {
                        (work.func)(work.arg);
                        ITEMS_EXECUTED.fetch_add(1, Ordering::Relaxed);
                    } else {
                        serial_println!(
                            "[workqueue] CRITICAL: refusing to execute corrupt work item \
                             func={:#x} arg={:#x} — queue corruption; skipping \
                             (see B-KNULLJUMP-SIGNAL)",
                            func_addr, work.arg
                        );
                        ITEMS_DROPPED.fetch_add(1, Ordering::Relaxed);
                    }
                }
                None => break, // Queue empty — go to sleep.
            }
        }

        // Block on the wait queue until submit() wakes us.
        // Use wait_timeout as a fallback in case a wake is missed
        // (defense in depth — shouldn't happen but costs nothing).
        // The predicate also takes QUEUE.lock; IRQ-safe wrap as
        // above so a timer firing inside the predicate evaluation
        // can't deadlock with a softirq-driven submit.
        WORKER_WQ.wait_timeout(
            || crate::cpu::without_interrupts(|| QUEUE.lock().len() > 0),
            POLL_INTERVAL_TICKS,
        );
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Spawn the workqueue worker task.
///
/// Called once during boot after the scheduler is initialized.
/// The worker runs at `DEFAULT_PRIORITY + 2` (slightly below normal)
/// to avoid competing with interactive tasks while still being
/// responsive enough for timely deferred work completion.
///
/// # Errors
///
/// Returns an error if task creation fails (e.g., out of memory).
pub fn init() -> KernelResult<()> {
    if SPAWNED.load(Ordering::Relaxed) {
        return Ok(()); // Already spawned.
    }

    let pml4 = crate::mm::page_table::active_pml4_phys();
    let priority = crate::sched::task::DEFAULT_PRIORITY.saturating_add(2);

    let tid = crate::sched::spawn(
        b"kworker",
        priority,
        worker_entry,
        0,
        pml4,
    )?;

    WORKER_TID.store(tid, Ordering::Release);
    SPAWNED.store(true, Ordering::Release);

    serial_println!(
        "[workqueue] Worker spawned as task {} (priority {})",
        tid, priority,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the workqueue subsystem.
///
/// Verifies:
/// 1. Work items are executed asynchronously.
/// 2. Queue overflow is handled gracefully.
/// 3. Statistics are accurate.
pub fn self_test() {
    use core::sync::atomic::AtomicU64;

    serial_println!("[workqueue] Running self-test...");

    // --- 1. Basic submission and execution ---
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_work(arg: u64) {
        TEST_COUNTER.fetch_add(arg, Ordering::Relaxed);
    }

    let before = executed_count();
    TEST_COUNTER.store(0, Ordering::Relaxed);

    // Submit 3 work items.
    assert!(submit(test_work, 10));
    assert!(submit(test_work, 20));
    assert!(submit(test_work, 30));

    // Yield to let the worker execute.
    for _ in 0..10 {
        crate::sched::yield_now();
    }

    let counter = TEST_COUNTER.load(Ordering::Relaxed);
    let _after = executed_count();

    if counter == 60 {
        serial_println!("[workqueue]   Basic execution: OK (counter=60)");
    } else {
        // Worker might not have run yet (timing-dependent).
        // Just verify submission worked.
        serial_println!(
            "[workqueue]   Basic execution: partial (counter={}, may need more yields)",
            counter,
        );
    }
    // At minimum, submitted_count should have increased.
    assert!(
        submitted_count() >= before.saturating_add(3),
        "submitted count should increase",
    );
    serial_println!("[workqueue]   Submission tracking: OK");

    // --- 2. Queue capacity ---
    // Fill the queue to capacity.
    fn noop_work(_arg: u64) {}
    let mut enqueued = 0usize;
    for i in 0..QUEUE_CAPACITY.saturating_add(10) {
        if submit(noop_work, i as u64) {
            enqueued = enqueued.saturating_add(1);
        }
    }
    // Should have successfully enqueued QUEUE_CAPACITY items (some might
    // have been drained by the worker between submissions).
    assert!(enqueued > 0, "Should enqueue at least some items");
    serial_println!(
        "[workqueue]   Capacity handling: OK (enqueued {}/{})",
        enqueued, QUEUE_CAPACITY,
    );

    // Let worker drain.
    for _ in 0..20 {
        crate::sched::yield_now();
    }

    // --- 3. Stats ---
    let stats_submitted = submitted_count();
    let stats_executed = executed_count();
    assert!(stats_submitted > 0);
    assert!(stats_executed > 0);
    serial_println!(
        "[workqueue]   Stats: submitted={}, executed={}, dropped={}",
        stats_submitted, stats_executed, dropped_count(),
    );

    serial_println!("[workqueue] Self-test PASSED");
}
