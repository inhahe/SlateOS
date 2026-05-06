//! Kernel self-test runner — runs all subsystem self-tests on demand.
//!
//! Provides a centralized way to run all kernel subsystem self-tests
//! and report a pass/fail summary.  This is the "regression test suite"
//! that verifies nothing is broken after changes.
//!
//! ## Design
//!
//! Each subsystem registers a test function via a static table.  The
//! runner invokes each test, catches panics (where possible), and
//! reports aggregate results.
//!
//! Since this is a `no_std` kernel, we can't catch panics — a test
//! failure (assert!) will halt the kernel.  So the "pass" confirmation
//! is implicit: if we reach the end, everything passed.
//!
//! ## Usage
//!
//! ```text
//! kshell> selftest         — run all registered tests
//! kshell> selftest list    — list available test suites
//! kshell> selftest mm      — run only memory subsystem tests
//! ```
//!
//! ## References
//!
//! - Linux kselftest — kernel self-test infrastructure
//! - Fuchsia unit test framework — in-kernel testing

use crate::serial_println;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Test suite registry
// ---------------------------------------------------------------------------

/// A registered test suite.
#[derive(Clone, Copy)]
pub struct TestSuite {
    /// Short name (e.g., "mm", "ipc", "cap").
    pub name: &'static str,
    /// Human-readable description.
    pub description: &'static str,
    /// Test function.  Returns true on pass, false on expected failure.
    /// Panics are uncatchable — they abort the kernel.
    pub run: fn() -> bool,
    /// Subsystem category for filtering.
    pub category: &'static str,
}

// ---------------------------------------------------------------------------
// Built-in test suites
// ---------------------------------------------------------------------------

/// All registered test suites.
fn all_suites() -> Vec<TestSuite> {
    let mut suites = Vec::new();

    // Memory subsystem
    suites.push(TestSuite {
        name: "frame_owner",
        description: "Per-frame ownership tracking",
        run: || { crate::mm::frame_owner::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "alloc_trace",
        description: "Allocation event ring buffer",
        run: || { crate::mm::alloc_trace::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "alloc_lat",
        description: "Allocation latency histogram",
        run: || { crate::mm::alloc_lat::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "heap_profile",
        description: "Heap size distribution profiler",
        run: || { crate::mm::heap_profile::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "alloc_checkpoint",
        description: "Memory state checkpoints (leak detection)",
        run: || { crate::mm::alloc_checkpoint::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "frag_history",
        description: "Fragmentation history and trend tracking",
        run: || { crate::mm::frag_history::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "fault_inject",
        description: "Controlled allocation failure injection",
        run: || { crate::mm::fault_inject::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "watermark",
        description: "Memory usage metering (watermarks)",
        run: || { crate::mm::watermark::self_test(); true },
        category: "mm",
    });
    suites.push(TestSuite {
        name: "poison",
        description: "Memory poison detection",
        run: || { crate::mm::poison::self_test(); true },
        category: "mm",
    });

    // Syscall subsystem
    suites.push(TestSuite {
        name: "syscall_profile",
        description: "Per-syscall invocation count/latency",
        run: || { crate::syscall::profile::self_test(); true },
        category: "syscall",
    });
    suites.push(TestSuite {
        name: "syscall_trace",
        description: "Per-event syscall capture (strace)",
        run: || { crate::syscall::trace::self_test(); true },
        category: "syscall",
    });

    // Capability subsystem
    suites.push(TestSuite {
        name: "cap_audit",
        description: "Capability operation audit log",
        run: || { crate::cap::audit::self_test(); true },
        category: "cap",
    });

    // IPC subsystem
    suites.push(TestSuite {
        name: "ipc_stats",
        description: "IPC mechanism usage counters",
        run: || { crate::ipc::stats::self_test(); true },
        category: "ipc",
    });

    // Kernel infrastructure
    suites.push(TestSuite {
        name: "kobject",
        description: "Kernel object lifecycle tracking",
        run: || { crate::kobject::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "kevent",
        description: "Kernel event bus (pub/sub)",
        run: || { crate::kevent::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "sysctl",
        description: "Runtime configuration parameters",
        run: || { crate::sysctl::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "watchpoint",
        description: "Software memory watchpoints",
        run: || { crate::watchpoint::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "ksnapshot",
        description: "Comprehensive system state capture",
        run: || { crate::ksnapshot::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "rip_sample",
        description: "Statistical RIP profiler",
        run: || { crate::rip_sample::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "invariant",
        description: "System-wide consistency invariant checker",
        run: || { crate::invariant::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "sched_migrate",
        description: "Scheduler task migration tracker",
        run: || { crate::sched_migrate::self_test(); true },
        category: "sched",
    });
    suites.push(TestSuite {
        name: "wchan",
        description: "Wait channel tracking (WCHAN for ps/top)",
        run: || { crate::wchan::self_test(); true },
        category: "sched",
    });
    suites.push(TestSuite {
        name: "kdiag",
        description: "Comprehensive diagnostic report generator",
        run: || { crate::kdiag::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "hypervisor",
        description: "Hypervisor/VM detection via CPUID",
        run: || { crate::hypervisor::self_test(); true },
        category: "kernel",
    });
    suites.push(TestSuite {
        name: "sched_fairness",
        description: "Scheduler fairness (Jain's Index)",
        run: || { crate::sched_fairness::self_test(); true },
        category: "sched",
    });

    suites
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of running the test suite.
#[derive(Debug, Clone)]
pub struct TestResults {
    /// Number of tests run.
    pub total: usize,
    /// Number that passed.
    pub passed: usize,
    /// Names of failed tests (empty if all passed — panics are fatal).
    pub failed: Vec<&'static str>,
}

/// Run all registered tests.
pub fn run_all() -> TestResults {
    let suites = all_suites();
    run_filtered(&suites)
}

/// Run tests matching a category filter.
pub fn run_category(category: &str) -> TestResults {
    let suites = all_suites();
    let filtered: Vec<TestSuite> = suites.into_iter()
        .filter(|s| s.category == category)
        .collect();
    run_filtered(&filtered)
}

/// Run a single named test.
pub fn run_one(name: &str) -> TestResults {
    let suites = all_suites();
    let filtered: Vec<TestSuite> = suites.into_iter()
        .filter(|s| s.name == name)
        .collect();
    run_filtered(&filtered)
}

/// List all available test suites.
pub fn list() -> Vec<TestSuite> {
    all_suites()
}

/// Get available categories.
pub fn categories() -> Vec<&'static str> {
    let suites = all_suites();
    let mut cats: Vec<&'static str> = suites.iter().map(|s| s.category).collect();
    cats.sort_unstable();
    cats.dedup();
    cats
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

fn run_filtered(suites: &[TestSuite]) -> TestResults {
    let total = suites.len();
    let mut passed: usize = 0;
    let mut failed: Vec<&'static str> = Vec::new();

    serial_println!("[selftest] Running {} test(s)...", total);

    for suite in suites {
        serial_println!("[selftest] >>> {} — {}", suite.name, suite.description);
        let ok = (suite.run)();
        if ok {
            passed += 1;
        } else {
            failed.push(suite.name);
        }
    }

    serial_println!("[selftest] Complete: {}/{} passed", passed, total);
    if !failed.is_empty() {
        serial_println!("[selftest] FAILED: {:?}", failed);
    }

    TestResults { total, passed, failed }
}

// ---------------------------------------------------------------------------
// Self-test (meta-test: test the test runner itself)
// ---------------------------------------------------------------------------

/// Self-test for the test runner infrastructure.
pub fn self_test() {
    serial_println!("[selftest] Running self-test...");

    // Test 1: List returns suites.
    let suites = list();
    assert!(!suites.is_empty(), "should have registered tests");
    serial_println!("[selftest]   List: OK ({} suites)", suites.len());

    // Test 2: Categories are non-empty.
    let cats = categories();
    assert!(!cats.is_empty());
    serial_println!("[selftest]   Categories: OK ({:?})", cats);

    // Test 3: Can find specific test.
    let found = suites.iter().any(|s| s.name == "kobject");
    assert!(found, "should find kobject test");
    serial_println!("[selftest]   Lookup: OK");

    serial_println!("[selftest] Self-test PASSED");
}
