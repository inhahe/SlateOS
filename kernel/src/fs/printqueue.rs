//! Print queue — per-printer job queue management.
//!
//! Manages individual print job queues for each registered printer,
//! supporting job submission, cancellation, completion tracking, and
//! per-printer pause/resume control.
//!
//! ## Architecture
//!
//! ```text
//! Application
//!   → printqueue::submit_job(printer_id, doc, ...)
//!   → job queued in printer's queue
//!
//! Print driver
//!   → printqueue::complete_job(printer_id, job_id)
//!   → stats updated, job marked completed
//!
//! Settings / print dialog
//!   → printqueue::add_printer(name) / remove_printer(id)
//!   → printqueue::pause_printer(id) / resume_printer(id)
//!   → printqueue::get_jobs(id) / list_printers() / stats()
//!   → printqueue::clear_completed(id)
//! ```
//!
//! ## Design
//!
//! Each printer owns an independent `PrinterQueue` with its own job
//! list.  Printers can be paused (jobs still accepted but not
//! dispatched) and resumed.  Completed, failed, and cancelled jobs
//! remain in the queue until explicitly cleared via `clear_completed`.

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of printers.
const MAX_PRINTERS: usize = 20;

/// Maximum jobs per printer queue.
const MAX_JOBS_PER_PRINTER: usize = 100;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Status of a print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    /// Job is waiting in the queue.
    Queued,
    /// Job is currently being printed.
    Printing,
    /// Job was paused by the user or system.
    Paused,
    /// Job finished successfully.
    Completed,
    /// Job failed (driver error, paper jam, etc.).
    Failed,
    /// Job was cancelled by the user.
    Cancelled,
}

impl JobStatus {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Printing => "Printing",
            Self::Paused => "Paused",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// Paper/page size for a print job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSize {
    /// ISO A4 (210 x 297 mm).
    A4,
    /// US Letter (8.5 x 11 in).
    Letter,
    /// US Legal (8.5 x 14 in).
    Legal,
    /// ISO A3 (297 x 420 mm).
    A3,
    /// US Tabloid (11 x 17 in).
    Tabloid,
    /// Custom / unspecified size.
    Custom,
}

impl PageSize {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
            Self::A3 => "A3",
            Self::Tabloid => "Tabloid",
            Self::Custom => "Custom",
        }
    }
}

/// A single print job.
#[derive(Debug, Clone)]
pub struct PrintJob {
    /// Unique job identifier.
    pub id: u32,
    /// Printer this job belongs to.
    pub printer_id: u32,
    /// Document name / title.
    pub document_name: String,
    /// User who submitted the job.
    pub user: String,
    /// Current job status.
    pub status: JobStatus,
    /// Number of pages in the document.
    pub pages: u32,
    /// Number of copies requested.
    pub copies: u32,
    /// Page size.
    pub page_size: PageSize,
    /// Whether to print in colour.
    pub color: bool,
    /// Whether to print double-sided.
    pub duplex: bool,
    /// Timestamp when the job was submitted (nanoseconds since boot).
    pub submitted_ns: u64,
    /// Timestamp when the job completed (0 if not yet complete).
    pub completed_ns: u64,
    /// Size of the source document in bytes.
    pub size_bytes: u64,
}

/// A per-printer job queue.
#[derive(Debug, Clone)]
pub struct PrinterQueue {
    /// Printer identifier.
    pub printer_id: u32,
    /// Display name of the printer.
    pub printer_name: String,
    /// Jobs in this queue (ordered by submission time).
    pub jobs: Vec<PrintJob>,
    /// Whether the queue is paused (jobs accepted but not dispatched).
    pub paused: bool,
    /// Total number of successfully printed pages on this printer.
    pub total_printed: u64,
    /// Total number of failed jobs on this printer.
    pub total_failed: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Internal state for the print queue subsystem.
struct State {
    /// Registered printer queues.
    printers: Vec<PrinterQueue>,
    /// Next printer ID to assign.
    next_printer_id: u32,
    /// Next job ID to assign (global across all printers).
    next_job_id: u32,
    /// Total jobs submitted since init.
    total_jobs: u64,
    /// Total pages printed (completed jobs only).
    total_pages: u64,
    /// Operation counter.
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

/// Lock the state and run `f`.  Increments the operation counter on
/// success.  Returns `NotSupported` if the subsystem is not initialised.
fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialise the print queue subsystem with one default printer.
///
/// Creates a "Virtual PDF Printer" so that the system always has at
/// least one printer available.  Safe to call more than once (subsequent
/// calls are no-ops).
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let mut state = State {
        printers: Vec::new(),
        next_printer_id: 1,
        next_job_id: 1,
        total_jobs: 0,
        total_pages: 0,
        ops: 0,
    };

    state.printers.push(PrinterQueue {
        printer_id: state.next_printer_id,
        printer_name: String::from("Virtual PDF Printer"),
        jobs: Vec::new(),
        paused: false,
        total_printed: 0,
        total_failed: 0,
    });
    state.next_printer_id += 1;

    *guard = Some(state);
}

// ---------------------------------------------------------------------------
// Printer management
// ---------------------------------------------------------------------------

/// Register a new printer.  Returns the assigned printer ID.
pub fn add_printer(name: &str) -> KernelResult<u32> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.printers.len() >= MAX_PRINTERS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_printer_id;
        state.next_printer_id += 1;
        state.printers.push(PrinterQueue {
            printer_id: id,
            printer_name: String::from(name),
            jobs: Vec::new(),
            paused: false,
            total_printed: 0,
            total_failed: 0,
        });
        Ok(id)
    })
}

/// Remove a printer and all its queued jobs.
pub fn remove_printer(printer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.printers.iter().position(|p| p.printer_id == printer_id) {
            state.printers.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

// ---------------------------------------------------------------------------
// Job management
// ---------------------------------------------------------------------------

/// Submit a print job to a specific printer.  Returns the job ID.
#[allow(clippy::too_many_arguments)]
pub fn submit_job(
    printer_id: u32,
    doc_name: &str,
    user: &str,
    pages: u32,
    copies: u32,
    page_size: PageSize,
    color: bool,
    duplex: bool,
    size_bytes: u64,
) -> KernelResult<u32> {
    if doc_name.is_empty() || pages == 0 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let queue = state.printers.iter_mut()
            .find(|p| p.printer_id == printer_id)
            .ok_or(KernelError::NotFound)?;

        if queue.jobs.len() >= MAX_JOBS_PER_PRINTER {
            return Err(KernelError::ResourceExhausted);
        }

        let job_id = state.next_job_id;
        state.next_job_id += 1;
        state.total_jobs += 1;

        queue.jobs.push(PrintJob {
            id: job_id,
            printer_id,
            document_name: String::from(doc_name),
            user: String::from(user),
            status: JobStatus::Queued,
            pages,
            copies: if copies == 0 { 1 } else { copies },
            page_size,
            color,
            duplex,
            submitted_ns: crate::hpet::elapsed_ns(),
            completed_ns: 0,
            size_bytes,
        });
        Ok(job_id)
    })
}

/// Cancel a print job.  The job must not already be completed.
pub fn cancel_job(printer_id: u32, job_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let queue = state.printers.iter_mut()
            .find(|p| p.printer_id == printer_id)
            .ok_or(KernelError::NotFound)?;

        let job = queue.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(KernelError::NotFound)?;

        if job.status == JobStatus::Completed {
            return Err(KernelError::InvalidArgument);
        }
        job.status = JobStatus::Cancelled;
        job.completed_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Mark a print job as completed.  Updates the printer's
/// `total_printed` page count and the global `total_pages`.
pub fn complete_job(printer_id: u32, job_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let queue = state.printers.iter_mut()
            .find(|p| p.printer_id == printer_id)
            .ok_or(KernelError::NotFound)?;

        let job = queue.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(KernelError::NotFound)?;

        if job.status == JobStatus::Completed || job.status == JobStatus::Cancelled {
            return Err(KernelError::InvalidArgument);
        }

        let pages_done = (job.pages as u64).saturating_mul(job.copies as u64);

        job.status = JobStatus::Completed;
        job.completed_ns = crate::hpet::elapsed_ns();

        queue.total_printed = queue.total_printed.saturating_add(pages_done);
        state.total_pages = state.total_pages.saturating_add(pages_done);

        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Printer control
// ---------------------------------------------------------------------------

/// Pause a printer queue.  New jobs are still accepted but will not be
/// dispatched until the queue is resumed.
pub fn pause_printer(printer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let queue = state.printers.iter_mut()
            .find(|p| p.printer_id == printer_id)
            .ok_or(KernelError::NotFound)?;
        queue.paused = true;
        Ok(())
    })
}

/// Resume a paused printer queue.
pub fn resume_printer(printer_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let queue = state.printers.iter_mut()
            .find(|p| p.printer_id == printer_id)
            .ok_or(KernelError::NotFound)?;
        queue.paused = false;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Get all jobs for a printer (cloned snapshot).
pub fn get_jobs(printer_id: u32) -> KernelResult<Vec<PrintJob>> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    let queue = state.printers.iter()
        .find(|p| p.printer_id == printer_id)
        .ok_or(KernelError::NotFound)?;
    Ok(queue.jobs.clone())
}

/// List all printers.  Returns `(id, name, paused, job_count)` tuples.
pub fn list_printers() -> Vec<(u32, String, bool, usize)> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(state) => state.printers.iter()
            .map(|p| (p.printer_id, p.printer_name.clone(), p.paused, p.jobs.len()))
            .collect(),
        None => Vec::new(),
    }
}

/// Remove all completed, failed, and cancelled jobs from a printer's
/// queue.  Returns the number of jobs removed.
pub fn clear_completed(printer_id: u32) -> KernelResult<usize> {
    with_state(|state| {
        let queue = state.printers.iter_mut()
            .find(|p| p.printer_id == printer_id)
            .ok_or(KernelError::NotFound)?;
        let before = queue.jobs.len();
        queue.jobs.retain(|j| !matches!(j.status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled));
        Ok(before - queue.jobs.len())
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns `(printer_count, total_jobs, total_pages, active_jobs, ops)`.
///
/// `active_jobs` is the number of jobs across all printers that are
/// in `Queued` or `Printing` status.
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active: u64 = s.printers.iter()
                .flat_map(|p| p.jobs.iter())
                .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Printing))
                .count() as u64;
            (s.printers.len(), s.total_jobs, s.total_pages, active, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the print queue subsystem.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[printqueue] Running self-tests...");

    // Reset state for a clean test run.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state — one default printer, no jobs.
    {
        let printers = list_printers();
        assert!(printers.len() == 1);
        assert_eq!(printers[0].1, "Virtual PDF Printer");
        assert!(!printers[0].2); // not paused
        assert_eq!(printers[0].3, 0); // no jobs
    }
    serial_println!("[printqueue]  1/11 initial state OK");

    // Test 2: add a printer.
    {
        let id = add_printer("Office Laser").unwrap();
        let printers = list_printers();
        assert_eq!(printers.len(), 2);
        assert!(printers.iter().any(|p| p.0 == id && p.1 == "Office Laser"));
    }
    serial_println!("[printqueue]  2/11 add printer OK");

    // Test 3: reject empty printer name.
    {
        let result = add_printer("");
        assert_eq!(result, Err(KernelError::InvalidArgument));
    }
    serial_println!("[printqueue]  3/11 reject empty name OK");

    // Test 4: submit a job.
    let default_pid = list_printers()[0].0;
    let job_id;
    {
        job_id = submit_job(
            default_pid, "report.pdf", "alice", 10, 2,
            PageSize::A4, true, false, 524_288,
        ).unwrap();
        let jobs = get_jobs(default_pid).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, job_id);
        assert_eq!(jobs[0].document_name, "report.pdf");
        assert_eq!(jobs[0].user, "alice");
        assert_eq!(jobs[0].status, JobStatus::Queued);
        assert_eq!(jobs[0].pages, 10);
        assert_eq!(jobs[0].copies, 2);
        assert!(jobs[0].color);
        assert!(!jobs[0].duplex);
    }
    serial_println!("[printqueue]  4/11 submit job OK");

    // Test 5: complete a job — stats update.
    {
        complete_job(default_pid, job_id).unwrap();
        let jobs = get_jobs(default_pid).unwrap();
        assert_eq!(jobs[0].status, JobStatus::Completed);
        assert!(jobs[0].completed_ns > 0);
        let (_, total_jobs, total_pages, _, _) = stats();
        assert_eq!(total_jobs, 1);
        // 10 pages * 2 copies = 20
        assert_eq!(total_pages, 20);
    }
    serial_println!("[printqueue]  5/11 complete job OK");

    // Test 6: cancel a job.
    {
        let jid = submit_job(
            default_pid, "draft.txt", "bob", 3, 1,
            PageSize::Letter, false, false, 4096,
        ).unwrap();
        cancel_job(default_pid, jid).unwrap();
        let jobs = get_jobs(default_pid).unwrap();
        let cancelled = jobs.iter().find(|j| j.id == jid).unwrap();
        assert_eq!(cancelled.status, JobStatus::Cancelled);
    }
    serial_println!("[printqueue]  6/11 cancel job OK");

    // Test 7: cannot complete an already-completed job.
    {
        let result = complete_job(default_pid, job_id);
        assert_eq!(result, Err(KernelError::InvalidArgument));
    }
    serial_println!("[printqueue]  7/11 double-complete rejected OK");

    // Test 8: pause and resume printer.
    {
        pause_printer(default_pid).unwrap();
        let printers = list_printers();
        let p = printers.iter().find(|p| p.0 == default_pid).unwrap();
        assert!(p.2); // paused

        resume_printer(default_pid).unwrap();
        let printers = list_printers();
        let p = printers.iter().find(|p| p.0 == default_pid).unwrap();
        assert!(!p.2); // resumed
    }
    serial_println!("[printqueue]  8/11 pause/resume OK");

    // Test 9: clear completed jobs.
    {
        let removed = clear_completed(default_pid).unwrap();
        // We had 1 completed + 1 cancelled = 2 cleared.
        assert_eq!(removed, 2);
        let jobs = get_jobs(default_pid).unwrap();
        assert!(jobs.is_empty());
    }
    serial_println!("[printqueue]  9/11 clear completed OK");

    // Test 10: remove a printer.
    {
        let id = add_printer("Temp Printer").unwrap();
        remove_printer(id).unwrap();
        let result = get_jobs(id);
        assert!(result.is_err());
    }
    serial_println!("[printqueue] 10/11 remove printer OK");

    // Test 11: stats reflect current state.
    {
        // Submit a fresh queued job to the default printer.
        let _jid = submit_job(
            default_pid, "final.pdf", "carol", 5, 1,
            PageSize::Legal, false, true, 10_000,
        ).unwrap();
        let (printer_count, total_jobs, total_pages, active, ops) = stats();
        assert_eq!(printer_count, 2); // default + Office Laser
        assert_eq!(total_jobs, 3); // 3 submitted in total
        assert_eq!(total_pages, 20); // only the first job completed
        assert_eq!(active, 1); // the fresh queued job
        assert!(ops > 0);
    }
    serial_println!("[printqueue] 11/11 stats OK");

    serial_println!("[printqueue] All self-tests passed.");
}
