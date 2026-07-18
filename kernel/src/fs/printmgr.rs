//! Print manager — printer discovery, job queue, and settings.
//!
//! Provides CUPS-like functionality: printer registration, default
//! printer selection, job submission and tracking, page size and
//! quality settings.
//!
//! ## Architecture
//!
//! ```text
//! Application → print dialog
//!   → printmgr::submit_job() → PrintJob
//!   → printmgr::list_printers() → available printers
//!
//! Settings panel → Printers
//!   → printmgr::add_printer() / set_default()
//!
//! Integration:
//!   → bluetooth (BT printers)
//!   → netsettings (network printer discovery)
//! ```
//!
//! ## Printer Types
//!
//! - Local USB printers
//! - Network printers (IPP/LPD/SMB)
//! - Virtual printers (PDF, file output)
//! - Bluetooth printers

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_PRINTERS: usize = 32;
const MAX_JOBS: usize = 256;
const MAX_HISTORY: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Printer connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterType {
    Local,
    Network,
    Virtual,
    Bluetooth,
}

impl PrinterType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "Local",
            Self::Network => "Network",
            Self::Virtual => "Virtual",
            Self::Bluetooth => "Bluetooth",
        }
    }
}

/// Printer status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrinterStatus {
    Ready,
    Printing,
    Paused,
    Offline,
    Error,
}

impl PrinterStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Printing => "Printing",
            Self::Paused => "Paused",
            Self::Offline => "Offline",
            Self::Error => "Error",
        }
    }
}

/// Print job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Printing,
    Completed,
    Failed,
    Cancelled,
    Held,
}

impl JobStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Printing => "Printing",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
            Self::Held => "Held",
        }
    }
}

/// Paper size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperSize {
    A4,
    A3,
    A5,
    Letter,
    Legal,
    Tabloid,
    Photo4x6,
    Photo5x7,
    Custom,
}

impl PaperSize {
    pub fn label(self) -> &'static str {
        match self {
            Self::A4 => "A4 (210x297mm)",
            Self::A3 => "A3 (297x420mm)",
            Self::A5 => "A5 (148x210mm)",
            Self::Letter => "Letter (8.5x11in)",
            Self::Legal => "Legal (8.5x14in)",
            Self::Tabloid => "Tabloid (11x17in)",
            Self::Photo4x6 => "4x6 Photo",
            Self::Photo5x7 => "5x7 Photo",
            Self::Custom => "Custom",
        }
    }
}

/// Print quality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintQuality {
    Draft,
    Normal,
    High,
    Photo,
}

impl PrintQuality {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Photo => "Photo",
        }
    }
}

/// Colour mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Color,
    Grayscale,
    BlackWhite,
}

impl ColorMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Color => "Color",
            Self::Grayscale => "Grayscale",
            Self::BlackWhite => "Black & White",
        }
    }
}

/// Duplex (two-sided) printing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplex {
    None,
    LongEdge,
    ShortEdge,
}

impl Duplex {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Single-sided",
            Self::LongEdge => "Two-sided (long edge)",
            Self::ShortEdge => "Two-sided (short edge)",
        }
    }
}

/// A registered printer.
#[derive(Debug, Clone)]
pub struct Printer {
    pub id: u64,
    pub name: String,
    pub model: String,
    pub printer_type: PrinterType,
    pub status: PrinterStatus,
    pub is_default: bool,
    pub uri: String,
    pub location: String,
    pub supports_color: bool,
    pub supports_duplex: bool,
    pub default_paper: PaperSize,
    pub default_quality: PrintQuality,
    pub default_color: ColorMode,
    pub default_duplex: Duplex,
    pub copies: u32,
    pub total_pages: u64,
    pub total_jobs: u64,
    pub error_message: String,
}

/// A print job.
#[derive(Debug, Clone)]
pub struct PrintJob {
    pub id: u64,
    pub printer_id: u64,
    pub document_name: String,
    pub status: JobStatus,
    pub pages: u32,
    pub copies: u32,
    pub paper_size: PaperSize,
    pub quality: PrintQuality,
    pub color_mode: ColorMode,
    pub duplex: Duplex,
    pub submitted_ns: u64,
    pub completed_ns: u64,
    pub uid: u32,
    pub pages_printed: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct PrintState {
    printers: Vec<Printer>,
    jobs: Vec<PrintJob>,
    history: Vec<PrintJob>,
    next_printer_id: u64,
    next_job_id: u64,
    ops: u64,
}

static STATE: Mutex<Option<PrintState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut PrintState) -> KernelResult<R>,
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

/// Initialize the print manager with a default PDF printer.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let mut state = PrintState {
        printers: Vec::new(),
        jobs: Vec::new(),
        history: Vec::new(),
        next_printer_id: 1,
        next_job_id: 1,
        ops: 0,
    };

    // Built-in PDF virtual printer.
    state.printers.push(Printer {
        id: state.next_printer_id,
        name: String::from("Print to PDF"),
        model: String::from("Virtual PDF Printer"),
        printer_type: PrinterType::Virtual,
        status: PrinterStatus::Ready,
        is_default: true,
        uri: String::from("pdf:///output"),
        location: String::from("Virtual"),
        supports_color: true,
        supports_duplex: false,
        default_paper: PaperSize::A4,
        default_quality: PrintQuality::Normal,
        default_color: ColorMode::Color,
        default_duplex: Duplex::None,
        copies: 1,
        total_pages: 0,
        total_jobs: 0,
        error_message: String::new(),
    });
    state.next_printer_id += 1;

    // Built-in File printer.
    state.printers.push(Printer {
        id: state.next_printer_id,
        name: String::from("Print to File"),
        model: String::from("Virtual File Printer"),
        printer_type: PrinterType::Virtual,
        status: PrinterStatus::Ready,
        is_default: false,
        uri: String::from("file:///output"),
        location: String::from("Virtual"),
        supports_color: true,
        supports_duplex: false,
        default_paper: PaperSize::A4,
        default_quality: PrintQuality::Normal,
        default_color: ColorMode::Color,
        default_duplex: Duplex::None,
        copies: 1,
        total_pages: 0,
        total_jobs: 0,
        error_message: String::new(),
    });
    state.next_printer_id += 1;

    *guard = Some(state);
}

// ---------------------------------------------------------------------------
// Printer management
// ---------------------------------------------------------------------------

/// Add a printer. Returns printer ID.
pub fn add_printer(
    name: &str,
    model: &str,
    printer_type: PrinterType,
    uri: &str,
) -> KernelResult<u64> {
    if name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.printers.len() >= MAX_PRINTERS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_printer_id;
        state.next_printer_id += 1;
        state.printers.push(Printer {
            id,
            name: String::from(name),
            model: String::from(model),
            printer_type,
            status: PrinterStatus::Ready,
            is_default: false,
            uri: String::from(uri),
            location: String::new(),
            supports_color: true,
            supports_duplex: false,
            default_paper: PaperSize::A4,
            default_quality: PrintQuality::Normal,
            default_color: ColorMode::Color,
            default_duplex: Duplex::None,
            copies: 1,
            total_pages: 0,
            total_jobs: 0,
            error_message: String::new(),
        });
        Ok(id)
    })
}

/// Remove a printer.
pub fn remove_printer(id: u64) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.printers.iter().position(|p| p.id == id) {
            state.printers.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Get a printer by ID.
pub fn get_printer(id: u64) -> KernelResult<Printer> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.printers.iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all printers.
pub fn list_printers() -> Vec<Printer> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.printers.clone())
}

/// Set the default printer.
pub fn set_default(id: u64) -> KernelResult<()> {
    with_state(|state| {
        if !state.printers.iter().any(|p| p.id == id) {
            return Err(KernelError::NotFound);
        }
        for p in &mut state.printers {
            p.is_default = p.id == id;
        }
        Ok(())
    })
}

/// Get the default printer.
pub fn default_printer() -> Option<Printer> {
    let guard = STATE.lock();
    guard.as_ref()
        .and_then(|s| s.printers.iter().find(|p| p.is_default).cloned())
}

/// Set printer status.
pub fn set_printer_status(id: u64, status: PrinterStatus) -> KernelResult<()> {
    with_state(|state| {
        let printer = state.printers.iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        printer.status = status;
        Ok(())
    })
}

/// Set printer location string.
pub fn set_location(id: u64, location: &str) -> KernelResult<()> {
    with_state(|state| {
        let printer = state.printers.iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        printer.location = String::from(location);
        Ok(())
    })
}

/// Configure printer capabilities.
pub fn set_capabilities(id: u64, color: bool, duplex: bool) -> KernelResult<()> {
    with_state(|state| {
        let printer = state.printers.iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        printer.supports_color = color;
        printer.supports_duplex = duplex;
        Ok(())
    })
}

/// Set default print settings for a printer.
pub fn set_defaults(
    id: u64,
    paper: PaperSize,
    quality: PrintQuality,
    color: ColorMode,
    duplex: Duplex,
) -> KernelResult<()> {
    with_state(|state| {
        let printer = state.printers.iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        printer.default_paper = paper;
        printer.default_quality = quality;
        printer.default_color = color;
        printer.default_duplex = duplex;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Job management
// ---------------------------------------------------------------------------

/// Submit a print job. Returns job ID.
pub fn submit_job(
    printer_id: u64,
    document_name: &str,
    pages: u32,
    copies: u32,
    uid: u32,
) -> KernelResult<u64> {
    if document_name.is_empty() || pages == 0 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let printer = state.printers.iter()
            .find(|p| p.id == printer_id)
            .ok_or(KernelError::NotFound)?;
        if printer.status == PrinterStatus::Offline || printer.status == PrinterStatus::Error {
            return Err(KernelError::NotSupported);
        }

        if state.jobs.len() >= MAX_JOBS {
            return Err(KernelError::ResourceExhausted);
        }

        let job_id = state.next_job_id;
        state.next_job_id += 1;

        state.jobs.push(PrintJob {
            id: job_id,
            printer_id,
            document_name: String::from(document_name),
            status: JobStatus::Queued,
            pages,
            copies: if copies == 0 { 1 } else { copies },
            paper_size: printer.default_paper,
            quality: printer.default_quality,
            color_mode: printer.default_color,
            duplex: printer.default_duplex,
            submitted_ns: crate::hpet::elapsed_ns(),
            completed_ns: 0,
            uid,
            pages_printed: 0,
        });
        Ok(job_id)
    })
}

/// Cancel a print job.
pub fn cancel_job(job_id: u64) -> KernelResult<()> {
    with_state(|state| {
        let job = state.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(KernelError::NotFound)?;
        if job.status == JobStatus::Completed {
            return Err(KernelError::InvalidArgument);
        }
        job.status = JobStatus::Cancelled;
        Ok(())
    })
}

/// Complete a print job (called by print driver after finishing).
pub fn complete_job(job_id: u64, success: bool) -> KernelResult<()> {
    with_state(|state| {
        let job = state.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(KernelError::NotFound)?;

        job.completed_ns = crate::hpet::elapsed_ns();
        if success {
            job.status = JobStatus::Completed;
            job.pages_printed = job.pages * job.copies;
            // Update printer stats.
            if let Some(printer) = state.printers.iter_mut()
                .find(|p| p.id == job.printer_id)
            {
                printer.total_jobs += 1;
                printer.total_pages += job.pages_printed as u64;
            }
        } else {
            job.status = JobStatus::Failed;
        }

        // Move to history.
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(job.clone());

        Ok(())
    })
}

/// Get a job by ID.
pub fn get_job(job_id: u64) -> KernelResult<PrintJob> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.jobs.iter()
        .find(|j| j.id == job_id)
        .or_else(|| state.history.iter().find(|j| j.id == job_id))
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all pending/active jobs.
pub fn list_jobs() -> Vec<PrintJob> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.jobs.iter()
            .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Printing | JobStatus::Held))
            .cloned()
            .collect()
    })
}

/// List jobs for a specific printer.
pub fn jobs_for_printer(printer_id: u64) -> Vec<PrintJob> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| {
        s.jobs.iter()
            .filter(|j| j.printer_id == printer_id)
            .cloned()
            .collect()
    })
}

/// Get print history.
pub fn job_history() -> Vec<PrintJob> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.history.clone())
}

/// Clear completed jobs from the queue.
pub fn clear_completed() -> KernelResult<usize> {
    with_state(|state| {
        let before = state.jobs.len();
        state.jobs.retain(|j| !matches!(j.status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled));
        Ok(before - state.jobs.len())
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (printer_count, pending_jobs, total_pages, history_count, ops).
pub fn stats() -> (usize, usize, u64, usize, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let pending = s.jobs.iter()
                .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Printing))
                .count();
            let total_pages: u64 = s.printers.iter().map(|p| p.total_pages).sum();
            (s.printers.len(), pending, total_pages, s.history.len(), s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the print manager.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[printmgr] Running self-tests...");

    // Reset state.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state with virtual printers.
    {
        let printers = list_printers();
        assert!(printers.len() >= 2);
        assert!(printers.iter().any(|p| p.name == "Print to PDF"));
        let def = default_printer().unwrap();
        assert_eq!(def.name, "Print to PDF");
    }
    serial_println!("[printmgr]  1/11 initial state OK");

    // Test 2: add printer.
    {
        let id = add_printer("HP LaserJet", "HP LaserJet Pro", PrinterType::Network, "ipp://192.168.1.100").unwrap();
        let printer = get_printer(id).unwrap();
        assert_eq!(printer.name, "HP LaserJet");
        assert_eq!(printer.printer_type, PrinterType::Network);
    }
    serial_println!("[printmgr]  2/11 add printer OK");

    // Test 3: set default.
    {
        let printers = list_printers();
        let hp_id = printers.iter().find(|p| p.name == "HP LaserJet").unwrap().id;
        set_default(hp_id).unwrap();
        let def = default_printer().unwrap();
        assert_eq!(def.name, "HP LaserJet");
    }
    serial_println!("[printmgr]  3/11 set default OK");

    // Test 4: submit job.
    {
        let def = default_printer().unwrap();
        let job_id = submit_job(def.id, "test.pdf", 5, 1, 1000).unwrap();
        let job = get_job(job_id).unwrap();
        assert_eq!(job.document_name, "test.pdf");
        assert_eq!(job.pages, 5);
        assert_eq!(job.status, JobStatus::Queued);
    }
    serial_println!("[printmgr]  4/11 submit job OK");

    // Test 5: complete job.
    {
        let jobs = list_jobs();
        let job_id = jobs.first().unwrap().id;
        complete_job(job_id, true).unwrap();
        let job = get_job(job_id).unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.pages_printed, 5);
    }
    serial_println!("[printmgr]  5/11 complete job OK");

    // Test 6: cancel job.
    {
        let def = default_printer().unwrap();
        let job_id = submit_job(def.id, "cancel.pdf", 10, 2, 1000).unwrap();
        cancel_job(job_id).unwrap();
        let job = get_job(job_id).unwrap();
        assert_eq!(job.status, JobStatus::Cancelled);
    }
    serial_println!("[printmgr]  6/11 cancel job OK");

    // Test 7: printer stats.
    {
        let def = default_printer().unwrap();
        let printer = get_printer(def.id).unwrap();
        assert_eq!(printer.total_jobs, 1);
        assert_eq!(printer.total_pages, 5);
    }
    serial_println!("[printmgr]  7/11 printer stats OK");

    // Test 8: clear completed.
    {
        let removed = clear_completed().unwrap();
        assert!(removed > 0);
    }
    serial_println!("[printmgr]  8/11 clear completed OK");

    // Test 9: remove printer.
    {
        let id = add_printer("Temp", "Temp", PrinterType::Local, "usb://temp").unwrap();
        remove_printer(id).unwrap();
        assert!(get_printer(id).is_err());
    }
    serial_println!("[printmgr]  9/11 remove printer OK");

    // Test 10: printer capabilities.
    {
        let def = default_printer().unwrap();
        set_capabilities(def.id, false, true).unwrap();
        let printer = get_printer(def.id).unwrap();
        assert!(!printer.supports_color);
        assert!(printer.supports_duplex);
    }
    serial_println!("[printmgr] 10/11 capabilities OK");

    // Test 11: history.
    {
        let history = job_history();
        assert!(!history.is_empty());
        let (_, _, _, hist_count, _) = stats();
        assert!(hist_count > 0);
    }
    serial_println!("[printmgr] 11/11 history OK");

    serial_println!("[printmgr] All self-tests passed.");
}
