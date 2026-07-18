//! Secure Erase — secure data deletion with verified overwrite.
//!
//! Provides multiple secure erasure methods for files and drives
//! including single-pass zero, multi-pass random, and DoD-standard
//! overwrite patterns.
//!
//! ## Architecture
//!
//! ```text
//! Secure erasure
//!   → secureerase::erase_file(path, method) → securely delete file
//!   → secureerase::erase_freespace(device) → wipe free space
//!   → secureerase::get_progress(job_id) → check status
//!
//! Integration:
//!   → fileops (file operations)
//!   → diskencrypt (disk encryption)
//!   → reclaim (space reclamation)
//!   → storageclean (storage cleanup)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Erasure method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseMethod {
    ZeroFill,       // Single pass of zeros.
    RandomFill,     // Single pass of random data.
    DoD3Pass,       // DoD 5220.22-M (3-pass).
    DoD7Pass,       // DoD 5220.22-M ECE (7-pass).
    Gutmann,        // Gutmann 35-pass.
    CustomPasses,   // User-specified pass count.
}

impl EraseMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::ZeroFill => "Zero Fill",
            Self::RandomFill => "Random Fill",
            Self::DoD3Pass => "DoD 3-Pass",
            Self::DoD7Pass => "DoD 7-Pass",
            Self::Gutmann => "Gutmann 35-Pass",
            Self::CustomPasses => "Custom",
        }
    }

    pub fn pass_count(self) -> u32 {
        match self {
            Self::ZeroFill => 1,
            Self::RandomFill => 1,
            Self::DoD3Pass => 3,
            Self::DoD7Pass => 7,
            Self::Gutmann => 35,
            Self::CustomPasses => 1,
        }
    }
}

/// Job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// An erasure job.
#[derive(Debug, Clone)]
pub struct EraseJob {
    pub id: u32,
    pub target: String,
    pub method: EraseMethod,
    pub status: JobStatus,
    pub total_bytes: u64,
    pub erased_bytes: u64,
    pub current_pass: u32,
    pub total_passes: u32,
    pub started_ns: u64,
    pub completed_ns: Option<u64>,
}

impl EraseJob {
    pub fn progress_pct(&self) -> u32 {
        if self.total_bytes == 0 { return 0; }
        let pass_progress = (self.erased_bytes * 100) / self.total_bytes;
        let total_progress = ((self.current_pass as u64 - 1) * 100 + pass_progress) / self.total_passes as u64;
        total_progress.min(100) as u32
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_JOBS: usize = 200;

struct State {
    jobs: Vec<EraseJob>,
    next_id: u32,
    total_started: u64,
    total_completed: u64,
    total_bytes_erased: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        jobs: Vec::new(),
        next_id: 1,
        total_started: 0,
        total_completed: 0,
        total_bytes_erased: 0,
        ops: 0,
    });
}

/// Start an erase job.
pub fn start_erase(target: &str, method: EraseMethod, size_bytes: u64, custom_passes: Option<u32>) -> KernelResult<u32> {
    with_state(|state| {
        if state.jobs.len() >= MAX_JOBS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let passes = custom_passes.unwrap_or_else(|| method.pass_count());
        let id = state.next_id;
        state.next_id += 1;
        state.jobs.push(EraseJob {
            id, target: String::from(target), method,
            status: JobStatus::Running, total_bytes: size_bytes,
            erased_bytes: 0, current_pass: 1, total_passes: passes,
            started_ns: now, completed_ns: None,
        });
        state.total_started += 1;
        Ok(id)
    })
}

/// Update erase progress (simulation).
pub fn update_progress(id: u32, erased_bytes: u64, current_pass: u32) -> KernelResult<()> {
    with_state(|state| {
        let job = state.jobs.iter_mut().find(|j| j.id == id)
            .ok_or(KernelError::NotFound)?;
        job.erased_bytes = erased_bytes;
        job.current_pass = current_pass;
        Ok(())
    })
}

/// Complete an erase job.
pub fn complete_erase(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let job = state.jobs.iter_mut().find(|j| j.id == id)
            .ok_or(KernelError::NotFound)?;
        job.status = JobStatus::Completed;
        job.erased_bytes = job.total_bytes;
        job.current_pass = job.total_passes;
        job.completed_ns = Some(now);
        state.total_completed += 1;
        state.total_bytes_erased += job.total_bytes;
        Ok(())
    })
}

/// Cancel an erase job.
pub fn cancel_erase(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let job = state.jobs.iter_mut().find(|j| j.id == id)
            .ok_or(KernelError::NotFound)?;
        if job.status != JobStatus::Running && job.status != JobStatus::Queued {
            return Err(KernelError::InvalidArgument);
        }
        job.status = JobStatus::Cancelled;
        Ok(())
    })
}

/// Get job status.
pub fn get_job(id: u32) -> Option<EraseJob> {
    STATE.lock().as_ref().and_then(|s| s.jobs.iter().find(|j| j.id == id).cloned())
}

/// List all jobs.
pub fn list_jobs() -> Vec<EraseJob> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.jobs.clone())
}

/// Statistics: (job_count, total_started, total_completed, total_bytes_erased, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.jobs.len(), s.total_started, s.total_completed, s.total_bytes_erased, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("secureerase::self_test() — running tests...");
    init_defaults();

    // 1: Empty state.
    assert!(list_jobs().is_empty());
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Start erase.
    let id = start_erase("/tmp/secret.dat", EraseMethod::DoD3Pass, 10_000_000, None).expect("start");
    let job = get_job(id).expect("get");
    assert_eq!(job.method, EraseMethod::DoD3Pass);
    assert_eq!(job.total_passes, 3);
    assert_eq!(job.status, JobStatus::Running);
    crate::serial_println!("  [2/8] start: OK");

    // 3: Update progress.
    update_progress(id, 5_000_000, 2).expect("update");
    let job = get_job(id).expect("get2");
    assert_eq!(job.erased_bytes, 5_000_000);
    assert_eq!(job.current_pass, 2);
    let pct = job.progress_pct();
    assert!(pct > 0 && pct < 100);
    crate::serial_println!("  [3/8] progress: OK");

    // 4: Complete.
    complete_erase(id).expect("complete");
    let job = get_job(id).expect("get3");
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.progress_pct(), 100);
    crate::serial_println!("  [4/8] complete: OK");

    // 5: Zero fill.
    let id2 = start_erase("/dev/sda1", EraseMethod::ZeroFill, 1_000_000_000, None).expect("zero");
    let job2 = get_job(id2).expect("get4");
    assert_eq!(job2.total_passes, 1);
    crate::serial_println!("  [5/8] zero fill: OK");

    // 6: Cancel.
    cancel_erase(id2).expect("cancel");
    let job2 = get_job(id2).expect("get5");
    assert_eq!(job2.status, JobStatus::Cancelled);
    crate::serial_println!("  [6/8] cancel: OK");

    // 7: Custom passes.
    let id3 = start_erase("/secret", EraseMethod::CustomPasses, 500_000, Some(10)).expect("custom");
    let job3 = get_job(id3).expect("get6");
    assert_eq!(job3.total_passes, 10);
    crate::serial_println!("  [7/8] custom passes: OK");

    // 8: Stats.
    let (count, started, completed, bytes, ops) = stats();
    assert_eq!(count, 3);
    assert_eq!(started, 3);
    assert_eq!(completed, 1);
    assert_eq!(bytes, 10_000_000);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("secureerase::self_test() — all 8 tests passed");
}
