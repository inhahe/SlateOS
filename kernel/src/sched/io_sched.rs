//! BFQ-style I/O scheduler.
//!
//! Sits between the filesystem/VFS layer and the block device drivers,
//! reordering and scheduling I/O requests to optimize throughput and
//! fairness across processes.
//!
//! ## Design
//!
//! Based on Linux's BFQ (Budget Fair Queueing) I/O scheduler:
//!
//! - **Per-process queues**: each process (identified by PID) gets its
//!   own sorted queue of I/O requests.
//! - **Budget-based fairness**: each process receives a budget per service
//!   round.  When a process exhausts its budget, the scheduler moves to
//!   the next process.
//! - **Small-I/O pass-through**: requests ≤ 16 sectors (8 KiB) cost only
//!   1 budget unit regardless of size, so processes doing metadata reads,
//!   directory walks, or small random I/O get up to 128 operations per
//!   round.  Bulk sequential I/O costs its full sector count.  This
//!   prevents heavy I/O (e.g., large file copies) from making the system
//!   unresponsive.
//! - **Priority classes**: realtime (audio/video), best-effort (normal),
//!   idle (background).  Higher priority classes are served first.
//! - **Elevator ordering**: within a process's queue, requests are sorted
//!   by sector number (C-SCAN) to minimize disk seek times.
//! - **Request merging**: adjacent or overlapping sector ranges are merged
//!   into a single larger request where possible.
//!
//! ## Priority Classes
//!
//! | Class       | Semantics                                   | Budget    |
//! |-------------|---------------------------------------------|-----------|
//! | Realtime    | Guaranteed bandwidth, low latency            | 2× normal |
//! | BestEffort  | Fair share with 8 priority levels (0=high)   | 1× normal |
//! | Idle        | Background only, served when no other I/O    | 0.5× normal |
//!
//! ## Usage
//!
//! ```ignore
//! // Submit a request
//! io_sched::submit(IoRequest { ... });
//!
//! // The block device layer calls dispatch() to get the next request
//! if let Some(req) = io_sched::dispatch(device_id) { ... }
//! ```
//!
//! ## References
//!
//! - Linux `block/bfq-iosched.c` — BFQ I/O scheduler
//! - Valente & Checconi, "High Throughput Disk Scheduling with Fair Bandwidth
//!   Distribution" (2010) — BFQ theory

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::serial_println;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Priority classes
// ---------------------------------------------------------------------------

/// I/O priority class.
///
/// Determines the scheduling class and relative importance of I/O requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IoPriorityClass {
    /// Realtime: guaranteed bandwidth, low latency.
    /// Use for audio/video playback, time-critical I/O.
    /// Requires a capability to use (prevents abuse).
    Realtime = 0,

    /// Best-effort: fair share of I/O bandwidth.
    /// Default for all normal user and kernel I/O.
    BestEffort = 1,

    /// Idle: only served when no other I/O is pending.
    /// Use for background indexing, backup, dedup, fsck.
    Idle = 2,
}

/// Full I/O priority: class + level within class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoPriority {
    /// Priority class (Realtime > BestEffort > Idle).
    pub class: IoPriorityClass,
    /// Level within the class (0 = highest, 7 = lowest).
    /// Only meaningful for BestEffort; ignored for Realtime/Idle.
    pub level: u8,
}

impl IoPriority {
    /// Default best-effort priority (level 4, middle of range).
    pub const DEFAULT: Self = Self {
        class: IoPriorityClass::BestEffort,
        level: 4,
    };

    /// Create a new I/O priority.
    ///
    /// `level` is clamped to 0..=7.
    #[must_use]
    pub const fn new(class: IoPriorityClass, level: u8) -> Self {
        let level = if level > 7 { 7 } else { level };
        Self { class, level }
    }
}

// ---------------------------------------------------------------------------
// I/O request
// ---------------------------------------------------------------------------

/// Direction of the I/O operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Used when submitting I/O requests to the scheduler.
pub enum IoDirection {
    /// Read from device to buffer.
    Read,
    /// Write from buffer to device.
    Write,
}

/// Unique identifier for an I/O request.
pub type IoRequestId = u64;

/// An I/O request submitted to the scheduler.
///
/// Represents a contiguous range of sectors to read or write.
#[derive(Debug)]
pub struct IoRequest {
    /// Unique request ID (assigned by `submit()`).
    pub id: IoRequestId,
    /// Target block device ID (0 for the first device, etc.).
    pub device_id: u32,
    /// Starting sector (LBA).
    pub sector: u64,
    /// Number of sectors to transfer.
    pub count: u32,
    /// Read or write.
    pub direction: IoDirection,
    /// I/O priority.
    pub priority: IoPriority,
    /// Process ID that submitted this request (0 = kernel).
    pub pid: u32,
}

// ---------------------------------------------------------------------------
// Per-process I/O queue
// ---------------------------------------------------------------------------

/// Default budget per process per service round (in budget units).
///
/// For large requests (> `SMALL_IO_THRESHOLD` sectors), one budget unit
/// equals one sector.  For small requests (≤ threshold), the cost is
/// capped at 1 unit regardless of sector count.  This ensures small
/// random I/O (metadata, directory traversal) gets many operations per
/// round, while bulk sequential I/O yields sooner — keeping the system
/// responsive under heavy I/O load.
const DEFAULT_BUDGET: u32 = 128;

/// Budget multiplier for realtime I/O (2× normal).
const REALTIME_BUDGET_MULT: u32 = 2;

/// Budget divisor for idle I/O (½ normal).
const IDLE_BUDGET_DIV: u32 = 2;

/// Sector count threshold for "small I/O" budget discount.
///
/// Requests of ≤ this many sectors cost only 1 budget unit (instead of
/// their actual sector count).  This prevents bulk sequential I/O from
/// monopolizing the device while small reads/writes are starved.
///
/// 16 sectors × 512 B/sector = 8 KiB.  Covers typical metadata reads,
/// inode lookups, directory entries, and small file reads.
const SMALL_IO_THRESHOLD: u32 = 16;

/// Compute the budget cost for a request of `count` sectors.
///
/// Small requests (≤ `SMALL_IO_THRESHOLD` sectors) cost only 1 unit,
/// giving small-I/O processes up to `DEFAULT_BUDGET` operations per
/// round.  Large requests cost their full sector count.
///
/// OPT: This is the key mechanism for "prevent heavy I/O from making
/// the system unusable."  A process doing 4-sector metadata reads gets
/// 128 operations per round; a process doing 64-sector bulk writes
/// gets only 2 operations.  Both processes get fair budget rotation,
/// but small I/O stays responsive.
#[inline]
const fn budget_cost(count: u32) -> u32 {
    if count <= SMALL_IO_THRESHOLD {
        1
    } else {
        count
    }
}

/// A per-process I/O queue.
///
/// Requests within a process's queue are sorted by sector number
/// (elevator ordering) to minimize seek times.
struct ProcessIoQueue {
    /// Queued requests, sorted by sector number.
    requests: Vec<IoRequest>,
    /// Priority class of this queue (from the highest-priority request).
    class: IoPriorityClass,
    /// Best-effort level (0-7, from highest-priority request).
    level: u8,
    /// Remaining budget for the current service round (sectors).
    budget: u32,
    /// Total budget for a full service round.
    full_budget: u32,
}

impl ProcessIoQueue {
    fn new(priority: IoPriority) -> Self {
        let full_budget = match priority.class {
            IoPriorityClass::Realtime => DEFAULT_BUDGET.saturating_mul(REALTIME_BUDGET_MULT),
            IoPriorityClass::BestEffort => DEFAULT_BUDGET,
            IoPriorityClass::Idle => DEFAULT_BUDGET / IDLE_BUDGET_DIV,
        };
        Self {
            requests: Vec::new(),
            class: priority.class,
            level: priority.level,
            budget: full_budget,
            full_budget,
        }
    }

    /// Insert a request in sector-sorted order (elevator/C-SCAN).
    fn insert_sorted(&mut self, req: IoRequest) {
        let pos = self.requests
            .binary_search_by_key(&req.sector, |r| r.sector)
            .unwrap_or_else(|p| p);

        // Capture priority info before moving req into the Vec.
        let req_class = req.priority.class;
        let req_level = req.priority.level;
        self.requests.insert(pos, req);

        // Update priority to the highest among all requests.
        if (req_class as u8) < (self.class as u8) {
            self.class = req_class;
            self.level = req_level;
        } else if req_class == self.class && req_level < self.level {
            self.level = req_level;
        }
    }

    /// Pop the first request (lowest sector number).
    fn pop_first(&mut self) -> Option<IoRequest> {
        if self.requests.is_empty() {
            return None;
        }
        Some(self.requests.remove(0))
    }

    /// Check if this queue can be merged with an adjacent request.
    ///
    /// Returns the index of a mergeable request if found.
    fn find_merge(&self, sector: u64, count: u32, direction: IoDirection) -> Option<usize> {
        let end = sector.saturating_add(u64::from(count));
        for (i, req) in self.requests.iter().enumerate() {
            if req.direction != direction {
                continue;
            }
            let req_end = req.sector.saturating_add(u64::from(req.count));
            // Check if adjacent: new range immediately follows existing,
            // or existing immediately follows new.
            if req_end == sector || end == req.sector {
                return Some(i);
            }
        }
        None
    }

    fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Reset the budget for a new service round.
    fn reset_budget(&mut self) {
        self.budget = self.full_budget;
    }
}

// ---------------------------------------------------------------------------
// I/O scheduler
// ---------------------------------------------------------------------------

/// The BFQ-style I/O scheduler.
///
/// Manages per-process queues, budget-based fairness, and priority
/// class scheduling.
struct IoSchedulerInner {
    /// Per-process I/O queues, keyed by PID.
    queues: BTreeMap<u32, ProcessIoQueue>,
    /// PID of the currently active process (being serviced).
    active_pid: Option<u32>,
    /// Next request ID to assign.
    next_id: IoRequestId,
    /// Total pending requests across all queues.
    pending_count: usize,
    /// Statistics: total requests submitted.
    total_submitted: u64,
    /// Statistics: total requests dispatched.
    total_dispatched: u64,
    /// Statistics: total requests merged.
    total_merged: u64,
}

impl IoSchedulerInner {
    const fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
            active_pid: None,
            next_id: 1,
            pending_count: 0,
            total_submitted: 0,
            total_dispatched: 0,
            total_merged: 0,
        }
    }

    /// Submit a request to the scheduler.
    fn submit(&mut self, mut req: IoRequest) -> IoRequestId {
        req.id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let id = req.id;

        let pid = req.pid;
        let sector = req.sector;
        let count = req.count;
        let direction = req.direction;
        let priority = req.priority;

        // Get or create the per-process queue.
        let queue = self.queues
            .entry(pid)
            .or_insert_with(|| ProcessIoQueue::new(priority));

        // Try to merge with an existing request.
        if let Some(merge_idx) = queue.find_merge(sector, count, direction) {
            // Merge: extend the existing request's sector range.
            if let Some(existing) = queue.requests.get_mut(merge_idx) {
                let existing_end = existing.sector.saturating_add(u64::from(existing.count));
                let new_end = sector.saturating_add(u64::from(count));

                // Expand the range to cover both.
                let merged_start = existing.sector.min(sector);
                let merged_end = existing_end.max(new_end);
                existing.sector = merged_start;
                existing.count = (merged_end - merged_start) as u32;

                self.total_merged = self.total_merged.wrapping_add(1);
                self.total_submitted = self.total_submitted.wrapping_add(1);
                return id;
            }
        }

        // No merge — insert as a new request.
        queue.insert_sorted(req);
        self.pending_count = self.pending_count.saturating_add(1);
        self.total_submitted = self.total_submitted.wrapping_add(1);

        id
    }

    /// Dispatch the next request to the device.
    ///
    /// Selects the highest-priority active process, pops its next
    /// request (lowest sector = elevator order), and decrements the
    /// budget.  When a process exhausts its budget, the scheduler
    /// rotates to the next eligible process at the same (or higher)
    /// priority class, ensuring fair bandwidth distribution.
    ///
    /// Small requests (≤ `SMALL_IO_THRESHOLD` sectors) cost only 1 budget
    /// unit via [`budget_cost`], so processes doing small random I/O get
    /// many more operations per round than those doing bulk sequential I/O.
    fn dispatch(&mut self, device_id: u32) -> Option<IoRequest> {
        // If there's an active process with remaining budget, continue
        // servicing it.
        if let Some(pid) = self.active_pid {
            if let Some(queue) = self.queues.get_mut(&pid) {
                if !queue.is_empty() && queue.budget > 0 {
                    if let Some(req) = queue.pop_first() {
                        if req.device_id == device_id {
                            queue.budget = queue.budget.saturating_sub(budget_cost(req.count));
                            self.pending_count = self.pending_count.saturating_sub(1);
                            self.total_dispatched = self.total_dispatched.wrapping_add(1);
                            return Some(req);
                        }
                        // Wrong device — put it back.
                        queue.insert_sorted(req);
                    }
                }
            }
        }

        // Active process exhausted budget or queue is empty.
        // Select the next process to service.
        //
        // Priority ordering: Realtime > BestEffort(0) > ... > BestEffort(7) > Idle.
        //
        // To ensure fairness among equal-priority processes, we skip the
        // just-exhausted process in the first pass.  If no other candidate
        // exists at the same or higher priority, we fall back to it.
        let exhausted_pid = self.active_pid;
        self.active_pid = None;

        // Two-pass selection: first pass excludes the exhausted PID,
        // second pass allows it (fallback when it's the only candidate).
        for pass in 0..2u8 {
            let mut best_pid: Option<u32> = None;
            let mut best_class = IoPriorityClass::Idle;
            let mut best_level: u8 = 255;

            for (&pid, queue) in &self.queues {
                if queue.is_empty() {
                    continue;
                }
                // First pass: skip the exhausted PID to give others a turn.
                if pass == 0 && Some(pid) == exhausted_pid {
                    continue;
                }
                // Check if any request targets this device.
                let has_device = queue.requests.iter().any(|r| r.device_id == device_id);
                if !has_device {
                    continue;
                }

                let dominated = (queue.class as u8) < (best_class as u8)
                    || (queue.class == best_class && queue.level < best_level);

                if best_pid.is_none() || dominated {
                    best_pid = Some(pid);
                    best_class = queue.class;
                    best_level = queue.level;
                }
            }

            if let Some(pid) = best_pid {
                self.active_pid = Some(pid);

                // Reset the budget for the new active process.
                if let Some(queue) = self.queues.get_mut(&pid) {
                    queue.reset_budget();
                    if let Some(req) = queue.pop_first() {
                        if req.device_id == device_id {
                            queue.budget = queue.budget.saturating_sub(budget_cost(req.count));
                            self.pending_count = self.pending_count.saturating_sub(1);
                            self.total_dispatched = self.total_dispatched.wrapping_add(1);
                            return Some(req);
                        }
                        // Wrong device — shouldn't happen (we filtered above).
                        queue.insert_sorted(req);
                    }
                }
                return None;
            }
            // First pass found nothing — second pass will include exhausted PID.
        }

        None
    }

    /// Number of pending requests.
    fn pending(&self) -> usize {
        self.pending_count
    }

    /// Clean up empty queues for processes that have no pending I/O.
    fn cleanup_empty_queues(&mut self) {
        self.queues.retain(|_, q| !q.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Global instance
// ---------------------------------------------------------------------------

static IO_SCHEDULER: Mutex<IoSchedulerInner> = Mutex::new(IoSchedulerInner::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Submit an I/O request to the scheduler.
///
/// Returns a unique request ID that can be used to track completion.
///
/// The request is queued and will be dispatched to the block device
/// when [`dispatch`] is called (typically by the block device driver
/// or a kernel I/O thread).
///
/// **Capability check**: If the request uses `IoPriorityClass::Realtime`
/// and `pid != 0` (not kernel), the process must hold an `IoScheduler`
/// capability with `Rights::IO_REALTIME`.  If not, the request is
/// silently downgraded to `BestEffort` level 0 (highest best-effort).
/// This prevents unprivileged processes from monopolizing I/O bandwidth
/// via the Realtime class.
pub fn submit(
    device_id: u32,
    sector: u64,
    count: u32,
    direction: IoDirection,
    mut priority: IoPriority,
    pid: u32,
) -> IoRequestId {
    // Capability gate: Realtime I/O requires an explicit capability.
    // Kernel I/O (pid == 0) is always allowed.
    if priority.class == IoPriorityClass::Realtime && pid != 0 {
        if !crate::proc::pcb::has_capability_type(
            u64::from(pid),
            crate::cap::ResourceType::IoScheduler,
            crate::cap::Rights::IO_REALTIME,
        ) {
            // Downgrade to highest-priority best-effort.
            priority = IoPriority::new(IoPriorityClass::BestEffort, 0);
        }
    }

    let req = IoRequest {
        id: 0, // Will be assigned by submit()
        device_id,
        sector,
        count,
        direction,
        priority,
        pid,
    };
    let mut sched = IO_SCHEDULER.lock();
    sched.submit(req)
}

/// Dispatch the next I/O request for a given device.
///
/// Returns the highest-priority, elevator-ordered request for the
/// specified device, or `None` if no requests are pending.
///
/// Called by the block device driver when it's ready to accept a new
/// request (e.g., after completing the previous one).
pub fn dispatch(device_id: u32) -> Option<IoRequest> {
    let mut sched = IO_SCHEDULER.lock();
    let result = sched.dispatch(device_id);
    if sched.pending() == 0 {
        sched.cleanup_empty_queues();
    }
    result
}

/// Get the number of pending I/O requests across all processes.
#[must_use]
#[allow(dead_code)] // Public API for I/O pressure monitoring.
pub fn pending_count() -> usize {
    IO_SCHEDULER.lock().pending()
}

/// Get I/O scheduler statistics.
#[must_use]
pub fn stats() -> IoSchedStats {
    let sched = IO_SCHEDULER.lock();
    IoSchedStats {
        pending: sched.pending_count,
        total_submitted: sched.total_submitted,
        total_dispatched: sched.total_dispatched,
        total_merged: sched.total_merged,
        active_queues: sched.queues.len(),
    }
}

/// I/O scheduler statistics.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API for I/O scheduler dashboard.
pub struct IoSchedStats {
    /// Number of pending requests.
    pub pending: usize,
    /// Total requests submitted (lifetime).
    pub total_submitted: u64,
    /// Total requests dispatched (lifetime).
    pub total_dispatched: u64,
    /// Total requests merged with existing ones (lifetime).
    pub total_merged: u64,
    /// Number of active per-process queues.
    pub active_queues: usize,
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the I/O scheduler.
pub fn self_test() {
    serial_println!("[io_sched] Running self-test...");

    // Test 1: Submit and dispatch a single request.
    test_single_request();

    // Test 2: Priority ordering (realtime before best-effort).
    test_priority_ordering();

    // Test 3: Elevator ordering (sector-sorted within a process).
    test_elevator_ordering();

    // Test 4: Request merging.
    test_request_merging();

    // Test 5: Budget fairness (alternating between processes).
    test_budget_fairness();

    // Test 6: Capability-gated realtime priority.
    test_realtime_cap_gate();

    // Test 7: Small I/O pass-through (budget discount).
    test_small_io_passthrough();

    serial_println!("[io_sched] Self-test PASSED");
}

fn test_single_request() {
    let id = submit(0, 100, 8, IoDirection::Read, IoPriority::DEFAULT, 1);
    assert!(id > 0, "request ID should be nonzero");

    let dispatched = dispatch(0).expect("should dispatch one request");
    assert!(dispatched.sector == 100, "sector mismatch");
    assert!(dispatched.count == 8, "count mismatch");
    assert!(dispatched.pid == 1, "pid mismatch");

    assert!(dispatch(0).is_none(), "no more requests");
    serial_println!("[io_sched]   Single request: OK");
}

fn test_priority_ordering() {
    // Submit idle request first, then realtime.
    let idle_prio = IoPriority::new(IoPriorityClass::Idle, 0);
    let rt_prio = IoPriority::new(IoPriorityClass::Realtime, 0);

    submit(0, 200, 4, IoDirection::Read, idle_prio, 10);
    submit(0, 300, 4, IoDirection::Read, rt_prio, 20);

    // Realtime should dispatch first (different PID = different queue).
    let first = dispatch(0).expect("should dispatch rt");
    assert!(
        first.pid == 20,
        "realtime should dispatch before idle (got pid={})",
        first.pid
    );

    let second = dispatch(0).expect("should dispatch idle");
    assert!(second.pid == 10, "idle should dispatch second");

    assert!(dispatch(0).is_none(), "no more");
    serial_println!("[io_sched]   Priority ordering: OK");
}

fn test_elevator_ordering() {
    // Submit requests out of sector order for the same PID.
    let prio = IoPriority::DEFAULT;
    submit(0, 500, 1, IoDirection::Read, prio, 30);
    submit(0, 100, 1, IoDirection::Read, prio, 30);
    submit(0, 300, 1, IoDirection::Read, prio, 30);

    // Should dispatch in sector order: 100, 300, 500.
    let r1 = dispatch(0).expect("dispatch 1");
    let r2 = dispatch(0).expect("dispatch 2");
    let r3 = dispatch(0).expect("dispatch 3");

    assert!(r1.sector == 100, "first should be sector 100, got {}", r1.sector);
    assert!(r2.sector == 300, "second should be sector 300, got {}", r2.sector);
    assert!(r3.sector == 500, "third should be sector 500, got {}", r3.sector);

    assert!(dispatch(0).is_none(), "no more");
    serial_println!("[io_sched]   Elevator ordering: OK");
}

fn test_request_merging() {
    let prio = IoPriority::DEFAULT;

    // Submit sector 100-107, then 108-115 (adjacent).
    submit(0, 100, 8, IoDirection::Read, prio, 40);
    submit(0, 108, 8, IoDirection::Read, prio, 40);

    // Should merge into one request: 100-115 (16 sectors).
    let merged = dispatch(0).expect("dispatch merged");
    assert!(merged.sector == 100, "merged start should be 100");
    assert!(merged.count == 16, "merged count should be 16, got {}", merged.count);

    assert!(dispatch(0).is_none(), "no more");

    let s = stats();
    assert!(s.total_merged > 0, "merge counter should increment");
    serial_println!("[io_sched]   Request merging: OK");
}

fn test_budget_fairness() {
    let prio = IoPriority::DEFAULT;

    // Submit many non-adjacent requests for two different processes.
    // Sectors are spaced 1000 apart to prevent merge coalescence, so
    // each request remains individual and the budget rotation is
    // actually exercised.
    //
    // Use 32-sector requests (above SMALL_IO_THRESHOLD of 16) so that
    // budget_cost = actual sector count and budget rotation occurs.
    for i in 0..16u64 {
        submit(0, i * 1000, 32, IoDirection::Read, prio, 50);
        submit(0, 500_000 + i * 1000, 32, IoDirection::Read, prio, 51);
    }

    // Dispatch all — track how many consecutive requests each PID gets.
    let mut dispatched = 0u32;
    let mut max_consecutive = 0u32;
    let mut consecutive = 0u32;
    let mut last_pid = 0u32;

    while let Some(req) = dispatch(0) {
        if req.pid == last_pid {
            consecutive = consecutive.saturating_add(1);
        } else {
            if consecutive > max_consecutive {
                max_consecutive = consecutive;
            }
            consecutive = 1;
            last_pid = req.pid;
        }
        dispatched = dispatched.saturating_add(1);
    }
    if consecutive > max_consecutive {
        max_consecutive = consecutive;
    }

    // With budget of 128 and requests of 32 sectors each (above
    // SMALL_IO_THRESHOLD, so budget_cost = 32), each process gets
    // 4 requests per round (128/32 = 4).  With 16 requests per
    // process, we expect ~4 rounds each.
    assert!(dispatched == 32, "expected 32 dispatched, got {}", dispatched);
    assert!(
        max_consecutive <= 6, // 4 + some slack
        "max consecutive {} too high (budget fairness broken)",
        max_consecutive
    );

    serial_println!(
        "[io_sched]   Budget fairness: OK (dispatched={}, max_consec={})",
        dispatched,
        max_consecutive
    );
}

fn test_realtime_cap_gate() {
    // PID 0 (kernel) should always be able to submit Realtime.
    let rt_prio = IoPriority::new(IoPriorityClass::Realtime, 0);
    submit(0, 900, 4, IoDirection::Read, rt_prio, 0);

    let dispatched = dispatch(0).expect("kernel RT should dispatch");
    assert!(
        dispatched.priority.class == IoPriorityClass::Realtime,
        "kernel RT should not be downgraded"
    );

    // PID 999 (non-existent process, no capabilities) should get
    // Realtime downgraded to BestEffort.
    submit(0, 1000, 4, IoDirection::Read, rt_prio, 999);

    let dispatched = dispatch(0).expect("downgraded request should dispatch");
    assert!(
        dispatched.priority.class == IoPriorityClass::BestEffort,
        "uncapped process RT should downgrade to BestEffort (got {:?})",
        dispatched.priority.class
    );
    assert!(
        dispatched.priority.level == 0,
        "downgraded level should be 0 (highest BE)"
    );

    assert!(dispatch(0).is_none(), "no more");
    serial_println!("[io_sched]   Realtime capability gate: OK");
}

fn test_small_io_passthrough() {
    let prio = IoPriority::DEFAULT;

    // Two processes: one doing small random reads (4 sectors each),
    // one doing large sequential writes (64 sectors each).
    //
    // Small requests cost 1 budget unit (below SMALL_IO_THRESHOLD).
    // Large requests cost 64 budget units (above threshold).
    //
    // With budget=128:
    //   - Small-I/O process: 128 ops per round (budget 128 / cost 1)
    //   - Large-I/O process: 2 ops per round (budget 128 / cost 64)
    //
    // We submit 256 small and 4 large requests.  Expected dispatch:
    //   Round 1: Small gets 128 ops (budget 128 / cost 1, exhausted)
    //   Round 2: Large gets 2 ops (budget 128 / cost 64, exhausted)
    //   Round 3: Small gets 128 ops (256 done)
    //   Round 4: Large gets 2 ops (4 done)
    //
    // Both processes exhaust their requests at the same time, ensuring
    // proper interleaving throughout the test (no tail where one
    // process runs unopposed).

    // PID 70: 256 small reads (4 sectors each), spaced to prevent merging.
    for i in 0..256u64 {
        submit(0, 10_000 + i * 100, 4, IoDirection::Read, prio, 70);
    }
    // PID 71: 4 large writes (64 sectors each), spaced to prevent merging.
    for i in 0..4u64 {
        submit(0, 500_000 + i * 1000, 64, IoDirection::Write, prio, 71);
    }

    // Dispatch all and count per-PID consecutive runs.
    let mut small_count = 0u32;
    let mut large_count = 0u32;
    let mut small_max_consec = 0u32;
    let mut large_max_consec = 0u32;
    let mut consec = 0u32;
    let mut last_pid = 0u32;

    while let Some(req) = dispatch(0) {
        if req.pid == 70 {
            small_count += 1;
        } else {
            large_count += 1;
        }

        if req.pid == last_pid {
            consec += 1;
        } else {
            // Record the run that just ended.
            if last_pid == 70 && consec > small_max_consec {
                small_max_consec = consec;
            }
            if last_pid == 71 && consec > large_max_consec {
                large_max_consec = consec;
            }
            consec = 1;
            last_pid = req.pid;
        }
    }
    // Final run.
    if last_pid == 70 && consec > small_max_consec {
        small_max_consec = consec;
    }
    if last_pid == 71 && consec > large_max_consec {
        large_max_consec = consec;
    }

    // All 260 requests should be dispatched.
    let total = small_count + large_count;
    assert!(
        total == 260,
        "expected 260 total, got {} (small={}, large={})",
        total, small_count, large_count
    );

    // Key invariant: the small-I/O process should get a long consecutive
    // run (up to 128 per budget round) because each 4-sector request
    // costs only 1 budget unit.  The large-I/O process should get at
    // most 2 per round (128/64 = 2).
    assert!(
        small_max_consec >= 64,
        "small I/O should get many consecutive ops (got {}), budget discount not working",
        small_max_consec
    );
    assert!(
        large_max_consec <= 3,
        "large I/O should rotate quickly (got {} consecutive)",
        large_max_consec
    );

    serial_println!(
        "[io_sched]   Small I/O pass-through: OK (small_consec={}, large_consec={})",
        small_max_consec,
        large_max_consec
    );
}
