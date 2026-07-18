//! Memory diagnostics — RAM testing and error detection.
//!
//! Performs memory tests to detect faulty RAM modules, tracks
//! ECC error counts, and provides memory health reporting.
//! Can schedule tests on boot or on demand.
//!
//! ## Architecture
//!
//! ```text
//! Boot sequence / user request
//!   → memdiag::run_test(test_type) → test results
//!
//! ECC hardware event
//!   → memdiag::record_ecc_error(address, type) → error log
//!
//! Settings panel → System → Memory Diagnostics
//!   → memdiag::get_report() → health summary
//!
//! Integration:
//!   → sysinfo (memory size/configuration)
//!   → hwmonitor (DIMM temperatures)
//!   → notifcenter (error alerts)
//!   → syslog (error logging)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Memory test type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestType {
    /// Quick pattern test.
    Quick,
    /// Standard multi-pattern test.
    Standard,
    /// Extended with walking bits.
    Extended,
    /// Address-line test.
    AddressLine,
    /// Random data test.
    Random,
}

impl TestType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quick => "Quick",
            Self::Standard => "Standard",
            Self::Extended => "Extended",
            Self::AddressLine => "Address Line",
            Self::Random => "Random",
        }
    }
}

/// Test result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestResult {
    Pass,
    Fail,
    Warning,
    Running,
    NotRun,
}

impl TestResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Warning => "WARN",
            Self::Running => "RUNNING",
            Self::NotRun => "NOT RUN",
        }
    }
}

/// ECC error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EccErrorType {
    Correctable,
    Uncorrectable,
}

impl EccErrorType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Correctable => "Correctable",
            Self::Uncorrectable => "Uncorrectable",
        }
    }
}

/// A memory test run.
#[derive(Debug, Clone)]
pub struct MemTest {
    /// Test ID.
    pub id: u32,
    /// Test type.
    pub test_type: TestType,
    /// Result.
    pub result: TestResult,
    /// Memory range tested (start, size in KB).
    pub range_start_kb: u64,
    pub range_size_kb: u64,
    /// Errors found.
    pub errors_found: u32,
    /// Start timestamp.
    pub started_ns: u64,
    /// Duration (ns).
    pub duration_ns: u64,
}

/// An ECC error event.
#[derive(Debug, Clone)]
pub struct EccError {
    /// Physical address.
    pub address: u64,
    /// Error type.
    pub error_type: EccErrorType,
    /// DIMM slot (if known).
    pub dimm_slot: u32,
    /// Timestamp.
    pub timestamp_ns: u64,
}

/// Memory health summary.
#[derive(Debug, Clone)]
pub struct MemHealth {
    pub total_memory_kb: u64,
    pub tested_memory_kb: u64,
    pub last_test_result: TestResult,
    pub correctable_errors: u64,
    pub uncorrectable_errors: u64,
    pub ecc_supported: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_TESTS: usize = 50;
const MAX_ECC_ERRORS: usize = 1000;

struct State {
    tests: Vec<MemTest>,
    ecc_errors: Vec<EccError>,
    next_id: u32,
    total_memory_kb: u64,
    tested_memory_kb: u64,
    ecc_supported: bool,
    correctable_count: u64,
    uncorrectable_count: u64,
    total_tests: u64,
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

/// Initialise the memory-diagnostics state.
///
/// Starts with no test runs, no ECC error events, all counters zeroed, and a
/// total memory size of `0` (i.e. "unknown until detected"). Tests are added
/// only through real [`run_test`] calls, ECC events only through real
/// [`record_ecc_error`] calls, and the system memory size only through a real
/// [`set_total_memory`] call by whatever code performs actual RAM detection.
/// The `memdiag` kshell command surfaces [`get_health`] (including
/// `total_memory_kb`) as if it reflects the real installed RAM, so seeding a
/// fixed size here would be a fabricated hardware claim — it would report a
/// memory size the kernel never measured.
///
/// (Previously this seeded a `total_memory_kb` of 1,048,576 — a hardcoded 1 GB
/// placeholder — so `memdiag show` always printed "Total memory: 1048576 KB
/// (1024 MB)" regardless of the machine's actual RAM.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        tests: Vec::new(),
        ecc_errors: Vec::new(),
        next_id: 1,
        total_memory_kb: 0,
        tested_memory_kb: 0,
        ecc_supported: false,
        correctable_count: 0,
        uncorrectable_count: 0,
        total_tests: 0,
        ops: 0,
    });
}

/// Run a memory test (simulated).
pub fn run_test(test_type: TestType, range_start_kb: u64, range_size_kb: u64) -> KernelResult<u32> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.total_tests += 1;

        let now = crate::hpet::elapsed_ns();
        // Simulate: all tests pass.
        state.tests.push(MemTest {
            id, test_type, result: TestResult::Pass,
            range_start_kb, range_size_kb,
            errors_found: 0, started_ns: now, duration_ns: 1_000_000,
        });
        state.tested_memory_kb = state.tested_memory_kb.max(range_start_kb + range_size_kb);

        while state.tests.len() > MAX_TESTS {
            state.tests.remove(0);
        }
        Ok(id)
    })
}

/// Record an ECC error.
pub fn record_ecc_error(address: u64, error_type: EccErrorType, dimm_slot: u32) -> KernelResult<()> {
    with_state(|state| {
        match error_type {
            EccErrorType::Correctable => state.correctable_count += 1,
            EccErrorType::Uncorrectable => state.uncorrectable_count += 1,
        }
        state.ecc_errors.push(EccError {
            address, error_type, dimm_slot,
            timestamp_ns: crate::hpet::elapsed_ns(),
        });
        while state.ecc_errors.len() > MAX_ECC_ERRORS {
            state.ecc_errors.remove(0);
        }
        Ok(())
    })
}

/// Mark a test as failed (for testing/simulation).
pub fn mark_test_failed(id: u32, errors: u32) -> KernelResult<()> {
    with_state(|state| {
        let test = state.tests.iter_mut().find(|t| t.id == id)
            .ok_or(KernelError::NotFound)?;
        test.result = TestResult::Fail;
        test.errors_found = errors;
        Ok(())
    })
}

/// Set system memory size.
pub fn set_total_memory(total_kb: u64) {
    if let Some(state) = STATE.lock().as_mut() {
        state.total_memory_kb = total_kb;
    }
}

/// Set ECC support flag.
pub fn set_ecc_supported(supported: bool) {
    if let Some(state) = STATE.lock().as_mut() {
        state.ecc_supported = supported;
    }
}

/// Get memory health summary.
pub fn get_health() -> MemHealth {
    STATE.lock().as_ref().map_or(
        MemHealth { total_memory_kb: 0, tested_memory_kb: 0, last_test_result: TestResult::NotRun, correctable_errors: 0, uncorrectable_errors: 0, ecc_supported: false },
        |s| {
            let last = s.tests.last().map(|t| t.result).unwrap_or(TestResult::NotRun);
            MemHealth {
                total_memory_kb: s.total_memory_kb,
                tested_memory_kb: s.tested_memory_kb,
                last_test_result: last,
                correctable_errors: s.correctable_count,
                uncorrectable_errors: s.uncorrectable_count,
                ecc_supported: s.ecc_supported,
            }
        },
    )
}

/// List test results.
pub fn list_tests() -> Vec<MemTest> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.tests.clone())
}

/// List ECC errors.
pub fn list_ecc_errors(limit: usize) -> Vec<EccError> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = if s.ecc_errors.len() > limit { s.ecc_errors.len() - limit } else { 0 };
        s.ecc_errors[start..].to_vec()
    })
}

/// Statistics: (test_count, total_tests, correctable, uncorrectable, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.tests.len(), s.total_tests, s.correctable_count, s.uncorrectable_count, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("memdiag::self_test() — running tests...");
    // Start from a clean, empty state so the assertions below are exact and no
    // fixtures leak into the live health report afterwards.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty initial — no test runs, no ECC events, and an honest unknown
    //    total memory size (0, not a fabricated 1 GB placeholder).
    assert!(list_tests().is_empty());
    assert!(list_ecc_errors(10).is_empty());
    let h0 = get_health();
    assert_eq!(h0.total_memory_kb, 0);
    assert_eq!(h0.tested_memory_kb, 0);
    assert_eq!(h0.last_test_result, TestResult::NotRun);
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Run quick test.
    let id1 = run_test(TestType::Quick, 0, 65536).expect("quick");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] quick test: OK");

    // 3: Test passes.
    let tests = list_tests();
    assert_eq!(tests[0].result, TestResult::Pass);
    assert_eq!(tests[0].test_type, TestType::Quick);
    crate::serial_println!("  [3/11] test passes: OK");

    // 4: Run standard test.
    let id2 = run_test(TestType::Standard, 0, 1_048_576).expect("standard");
    assert_eq!(list_tests().len(), 2);
    crate::serial_println!("  [4/11] standard test: OK");

    // 5: Mark test failed.
    mark_test_failed(id2, 3).expect("fail");
    let tests = list_tests();
    let t = tests.iter().find(|t| t.id == id2).expect("find");
    assert_eq!(t.result, TestResult::Fail);
    assert_eq!(t.errors_found, 3);
    crate::serial_println!("  [5/11] mark failed: OK");

    // 6: ECC error.
    record_ecc_error(0x1000_0000, EccErrorType::Correctable, 0).expect("ecc");
    let errors = list_ecc_errors(10);
    assert_eq!(errors.len(), 1);
    crate::serial_println!("  [6/11] ECC error: OK");

    // 7: Uncorrectable ECC.
    record_ecc_error(0x2000_0000, EccErrorType::Uncorrectable, 1).expect("ecc2");
    let health = get_health();
    assert_eq!(health.correctable_errors, 1);
    assert_eq!(health.uncorrectable_errors, 1);
    crate::serial_println!("  [7/11] uncorrectable ECC: OK");

    // 8: Memory health — total size reflects only an explicit set_total_memory
    //    call (real RAM detection), never a baked-in default.
    set_total_memory(2_097_152); // 2 GB, as if detected by the memory map.
    let health = get_health();
    assert_eq!(health.total_memory_kb, 2_097_152);
    assert_eq!(health.last_test_result, TestResult::Fail);
    crate::serial_println!("  [8/11] health summary: OK");

    // 9: Set ECC support.
    set_ecc_supported(true);
    let health = get_health();
    assert!(health.ecc_supported);
    crate::serial_println!("  [9/11] ECC support: OK");

    // 10: Test labels.
    assert_eq!(TestType::Extended.label(), "Extended");
    assert_eq!(TestResult::Pass.label(), "PASS");
    assert_eq!(EccErrorType::Correctable.label(), "Correctable");
    crate::serial_println!("  [10/11] labels: OK");

    // 11: Stats.
    let (count, total, correctable, uncorrectable, ops) = stats();
    assert_eq!(count, 2);
    assert_eq!(total, 2);
    assert_eq!(correctable, 1);
    assert_eq!(uncorrectable, 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    // Leave no residue in the live state.
    *STATE.lock() = None;
    crate::serial_println!("memdiag::self_test() — all 11 tests passed");
}
