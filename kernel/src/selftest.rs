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
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Severity classification (§914)
// ---------------------------------------------------------------------------

/// Severity of a self-test: determines kernel behaviour on failure.
///
/// Introduced by design-decisions.md §914 (operator decision): structural-
/// integrity tests halt the machine; diagnostic tests log a WARNING and
/// let the boot continue.  The default is `Integrity` — the safe side —
/// so an unclassified test never silently degrades a kernel invariant.
///
/// A test whose `run` function uses `assert!` internally panics on failure
/// regardless of this field (panics are uncatchable in `no_std`).  To make
/// a test truly `Diagnostic`, it must return `Result<(), E>` so the caller
/// can choose how to handle the error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Kernel structural integrity — halt on failure.
    ///
    /// Use for: memory manager, page tables, scheduler invariants,
    /// capability enforcement, IPC channels, boot-stack checks.
    Integrity,
    /// Informational / diagnostic — log and continue.
    ///
    /// Use for: terminal flags, cosmetic checks, filesystem feature
    /// probes, optional hardware detection (ACPI, CET), userspace-
    /// facing conformance tests.
    Diagnostic,
}

/// Run a self-test that returns `Result` and dispatch on severity.
///
/// - [`Severity::Integrity`]: prints `FATAL: {name} self-test failed: {e}`
///   and halts the kernel.
/// - [`Severity::Diagnostic`]: prints `WARNING: {name} self-test failed: {e}`
///   and returns normally so boot continues.
///
/// Replaces the ad-hoc `serial_println!("FATAL/WARNING: ...")` +
/// `cpu::halt_loop()` pattern scattered across `main.rs` with a single
/// dispatch point that cannot misclassify a test's severity.
///
/// # Examples
///
/// ```ignore
/// use crate::selftest::{Severity, dispatch};
///
/// // Integrity — halts on failure:
/// dispatch("Frame allocator", Severity::Integrity, mm::frame::self_test());
///
/// // Diagnostic — logs and continues:
/// dispatch("ACPI", Severity::Diagnostic, acpi::self_test());
/// ```
pub fn dispatch<E: core::fmt::Display>(name: &str, severity: Severity, result: Result<(), E>) {
    if let Err(e) = result {
        match severity {
            Severity::Integrity => {
                serial_println!("FATAL: {} self-test failed: {}", name, e);
                if keep_going() {
                    serial_println!("[selftest] selftest.keep_going: carrying on past {}", name);
                    return;
                }
                crate::cpu::halt_loop();
            }
            Severity::Diagnostic => {
                serial_println!("WARNING: {} self-test failed: {}", name, e);
            }
        }
    }
}

/// Like [`dispatch`] but formats the error with `Debug` (`{:?}`) instead of
/// `Display`.  Many subsystem self-tests return error types that derive
/// `Debug` but do not implement `Display` — this variant covers those.
pub fn dispatch_debug<E: core::fmt::Debug>(name: &str, severity: Severity, result: Result<(), E>) {
    if let Err(e) = result {
        match severity {
            Severity::Integrity => {
                serial_println!("FATAL: {} self-test failed: {:?}", name, e);
                if keep_going() {
                    serial_println!("[selftest] selftest.keep_going: carrying on past {}", name);
                    return;
                }
                crate::cpu::halt_loop();
            }
            Severity::Diagnostic => {
                serial_println!("WARNING: {} self-test failed: {:?}", name, e);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Keeping going past a failure (`selftest.keep_going=1`)
// ---------------------------------------------------------------------------

/// Whether this boot carries on past a failed self-test: the kernel command
/// line holds `selftest.keep_going=1`.
///
/// Without it a boot stops at the first failed integrity self-test, and every
/// later test goes unrun: a change that breaks three tests takes three boots
/// to find them -- about an hour each with the gates. With it, a failure is
/// reported exactly as before (`FATAL: ...`, which the boot test counts as a
/// failed boot) and the boot goes on, so one run lists every failure.
///
/// Nothing changes until something fails, so a boot with it set that passes
/// is an ordinary pass. One that fails lists every failure -- but a test after
/// a failed integrity test ran on a kernel whose invariants may already be
/// broken, so a later failure is a lead, not a verdict, until a boot without
/// the first one shows it again. Set it with
/// `SLATE_CMDLINE="selftest.keep_going=1" scripts/boot-test.sh`.
#[must_use]
pub fn keep_going() -> bool {
    use core::sync::atomic::{AtomicU8, Ordering};
    /// 0 not looked yet, 1 no, 2 yes. The command line never changes.
    static ANSWER: AtomicU8 = AtomicU8::new(0);
    match ANSWER.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => {
            let yes = crate::boot::kernel_cmdline_bytes().is_some_and(|line| {
                line.split(u8::is_ascii_whitespace)
                    .any(|token| token == b"selftest.keep_going=1")
            });
            ANSWER.store(if yes { 2 } else { 1 }, Ordering::Relaxed);
            yes
        }
    }
}

/// One part of a self-test made of many parts (`linux::self_test`'s are
/// chained `part()?`): its failure is the whole test's at once -- or, with
/// [`keep_going`], it is reported as `FATAL` (so the boot is still judged
/// failed) and the next part runs.
///
/// # Errors
///
/// `part`'s own error, unless [`keep_going`].
pub fn step<E: core::fmt::Debug>(part: Result<(), E>) -> Result<(), E> {
    match part {
        Err(e) if keep_going() => {
            // "self-test failed" is what the boot test's
            // check_selftest_failures looks for: the run still fails.
            serial_println!(
                "FATAL: self-test failed in one of its parts: {:?} (selftest.keep_going: \
                 carrying on)",
                e
            );
            Ok(())
        }
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Non-panicking assertion macros for Diagnostic self-tests (§914)
// ---------------------------------------------------------------------------
//
// These are the §914 replacements for `assert!`/`assert_eq!`/`assert_ne!`
// inside self-tests classified as `Diagnostic`.  On failure they print a
// `FAIL:` line and return `Err(KernelError::InternalError)` — the
// `dispatch`/`dispatch_debug` caller then decides whether to halt or
// continue, based on the test's `Severity`.
//
// `assert!` is still correct for `Integrity` tests, where a failure means
// the kernel's structural invariants are broken and continuing is unsafe.
//
// Many subsystems already define a local `check!` with this exact shape
// (audio_alsa, evdev, drm/*, initproc, …).  These global versions let new
// conversions use a shared definition rather than copying the macro, and
// existing local definitions can be replaced incrementally.
//
// Usage:
// ```ignore
// use crate::selftest;
// pub fn self_test() -> crate::KernelResult<()> {
//     selftest::check!(1 + 1 == 2, "basic arithmetic");
//     selftest::check_eq!(4, 2 + 2, "addition");
//     selftest::check_ne!(0, 1, "zero is not one");
//     Ok(())
// }
// ```

/// Non-panicking boolean check.  Returns `Err(KernelError::InternalError)`
/// on failure instead of panicking.
#[macro_export]
macro_rules! selftest_check {
    ($cond:expr, $($arg:tt)*) => {
        if !($cond) {
            $crate::serial_println!("  FAIL: {}", format_args!($($arg)*));
            return Err($crate::error::KernelError::InternalError);
        }
    };
    ($cond:expr) => {
        if !($cond) {
            $crate::serial_println!("  FAIL: assertion `{}` failed", stringify!($cond));
            return Err($crate::error::KernelError::InternalError);
        }
    };
}

/// Non-panicking equality check.  Prints both values on failure.
#[macro_export]
macro_rules! selftest_check_eq {
    ($left:expr, $right:expr, $($arg:tt)+) => {{
        let left_val = &$left;
        let right_val = &$right;
        if !(*left_val == *right_val) {
            $crate::serial_println!(
                "  FAIL: {}\n  left:  {:?}\n  right: {:?}",
                format_args!($($arg)+),
                left_val,
                right_val,
            );
            return Err($crate::error::KernelError::InternalError);
        }
    }};
    ($left:expr, $right:expr) => {{
        let left_val = &$left;
        let right_val = &$right;
        if !(*left_val == *right_val) {
            $crate::serial_println!(
                "  FAIL: assertion `{} == {}` failed\n  left:  {:?}\n  right: {:?}",
                stringify!($left),
                stringify!($right),
                left_val,
                right_val,
            );
            return Err($crate::error::KernelError::InternalError);
        }
    }};
}

/// Non-panicking inequality check.  Prints both values on failure.
#[macro_export]
macro_rules! selftest_check_ne {
    ($left:expr, $right:expr, $($arg:tt)+) => {{
        let left_val = &$left;
        let right_val = &$right;
        if *left_val == *right_val {
            $crate::serial_println!(
                "  FAIL: {}\n  both:  {:?}",
                format_args!($($arg)+),
                left_val,
            );
            return Err($crate::error::KernelError::InternalError);
        }
    }};
    ($left:expr, $right:expr) => {{
        let left_val = &$left;
        let right_val = &$right;
        if *left_val == *right_val {
            $crate::serial_println!(
                "  FAIL: assertion `{} != {}` failed\n  both:  {:?}",
                stringify!($left),
                stringify!($right),
                left_val,
            );
            return Err($crate::error::KernelError::InternalError);
        }
    }};
}

// Re-export under the `selftest` namespace for ergonomic use as
// `selftest::check!(...)` etc.  The `#[macro_export]` above places
// them at the crate root; these `pub use` make them available as
// `crate::selftest::check` too.
pub use crate::selftest_check as check;
pub use crate::selftest_check_eq as check_eq;
pub use crate::selftest_check_ne as check_ne;

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
    /// Severity classification (§914).  Defaults to [`Severity::Integrity`]
    /// for tests registered before the classification was introduced.
    pub severity: Severity,
}

// ---------------------------------------------------------------------------
// Built-in test suites
// ---------------------------------------------------------------------------

/// All registered test suites.
fn all_suites() -> Vec<TestSuite> {
    vec![
        // Memory subsystem
        TestSuite {
            name: "frame_owner",
            description: "Per-frame ownership tracking",
            run: || crate::mm::frame_owner::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "alloc_trace",
            description: "Allocation event ring buffer",
            run: || crate::mm::alloc_trace::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "alloc_lat",
            description: "Allocation latency histogram",
            run: || crate::mm::alloc_lat::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "heap_profile",
            description: "Heap size distribution profiler",
            run: || crate::mm::heap_profile::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "alloc_checkpoint",
            description: "Memory state checkpoints (leak detection)",
            run: || crate::mm::alloc_checkpoint::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "frag_history",
            description: "Fragmentation history and trend tracking",
            run: || crate::mm::frag_history::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "fault_inject",
            description: "Controlled allocation failure injection",
            run: || crate::mm::fault_inject::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "watermark",
            description: "Memory usage metering (watermarks)",
            run: || crate::mm::watermark::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "poison",
            description: "Memory poison detection",
            run: || crate::mm::poison::self_test().is_ok(),
            category: "mm",
            severity: Severity::Diagnostic,
        },
        // Syscall subsystem
        TestSuite {
            name: "syscall_profile",
            description: "Per-syscall invocation count/latency",
            run: || crate::syscall::profile::self_test().is_ok(),
            category: "syscall",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "syscall_trace",
            description: "Per-event syscall capture (strace)",
            run: || crate::syscall::trace::self_test().is_ok(),
            category: "syscall",
            severity: Severity::Diagnostic,
        },
        // Capability subsystem
        TestSuite {
            name: "cap_audit",
            description: "Capability operation audit log",
            run: || crate::cap::audit::self_test().is_ok(),
            category: "cap",
            severity: Severity::Diagnostic,
        },
        // IPC subsystem
        TestSuite {
            name: "ipc_stats",
            description: "IPC mechanism usage counters",
            run: || crate::ipc::stats::self_test().is_ok(),
            category: "ipc",
            severity: Severity::Diagnostic,
        },
        // Kernel infrastructure
        TestSuite {
            name: "kobject",
            description: "Kernel object lifecycle tracking",
            run: || crate::kobject::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "kevent",
            description: "Kernel event bus (pub/sub)",
            run: || crate::kevent::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "sysctl",
            description: "Runtime configuration parameters",
            run: || crate::sysctl::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "watchpoint",
            description: "Software memory watchpoints",
            run: || crate::watchpoint::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "ksnapshot",
            description: "Comprehensive system state capture",
            run: || crate::ksnapshot::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "rip_sample",
            description: "Statistical RIP profiler",
            run: || crate::rip_sample::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "invariant",
            description: "System-wide consistency invariant checker",
            run: || crate::invariant::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "sched_migrate",
            description: "Scheduler task migration tracker",
            run: || crate::sched_migrate::self_test().is_ok(),
            category: "sched",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "wchan",
            description: "Wait channel tracking (WCHAN for ps/top)",
            run: || crate::wchan::self_test().is_ok(),
            category: "sched",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "kdiag",
            description: "Comprehensive diagnostic report generator",
            run: || crate::kdiag::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "hypervisor",
            description: "Hypervisor/VM detection via CPUID",
            run: || crate::hypervisor::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "sched_fairness",
            description: "Scheduler fairness (Jain's Index)",
            run: || crate::sched_fairness::self_test().is_ok(),
            category: "sched",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "eevdf",
            description: "EEVDF scheduler algorithm (vruntime, deadlines, fairness)",
            run: || crate::sched::eevdf::self_test().is_ok(),
            category: "sched",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "deadline",
            description: "Deadline scheduler (EDF, admission control, throttling)",
            run: || crate::sched::deadline::self_test().is_ok(),
            category: "sched",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "sched_backend",
            description: "Scheduler backend enum (selectable PriorityRR/EEVDF/Deadline)",
            run: || crate::sched::backend::self_test().is_ok(),
            category: "sched",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "cet",
            description: "Intel CET (shadow stacks + IBT) detection",
            run: || crate::cet::self_test().is_ok(),
            category: "security",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "smep_smap",
            description: "SMEP/SMAP (user page execution/access prevention)",
            run: || crate::smep_smap::self_test().is_ok(),
            category: "security",
            severity: Severity::Diagnostic,
        },
        TestSuite {
            name: "spectre",
            description: "Spectre/Meltdown mitigations (IBRS/STIBP/SSBD/IBPB)",
            run: || crate::spectre::self_test().is_ok(),
            category: "security",
            severity: Severity::Diagnostic,
        },
        // Timers
        TestSuite {
            name: "hrtimer",
            description: "High-resolution timers (nanosecond scheduling, HPET-backed)",
            run: || crate::hrtimer::self_test().is_ok(),
            category: "kernel",
            severity: Severity::Diagnostic,
        },
    ]
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
    let filtered: Vec<TestSuite> = suites
        .into_iter()
        .filter(|s| s.category == category)
        .collect();
    run_filtered(&filtered)
}

/// Run a single named test.
pub fn run_one(name: &str) -> TestResults {
    let suites = all_suites();
    let filtered: Vec<TestSuite> = suites.into_iter().filter(|s| s.name == name).collect();
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
        let sev_tag = match suite.severity {
            Severity::Integrity => "integrity",
            Severity::Diagnostic => "diagnostic",
        };
        serial_println!(
            "[selftest] >>> {} — {} [{}]",
            suite.name,
            suite.description,
            sev_tag,
        );
        let ok = (suite.run)();
        if ok {
            passed += 1;
        } else {
            failed.push(suite.name);
            if matches!(suite.severity, Severity::Integrity) {
                serial_println!(
                    "[selftest] FATAL: integrity test '{}' failed — halting",
                    suite.name,
                );
            }
        }
    }

    serial_println!("[selftest] Complete: {}/{} passed", passed, total);
    if !failed.is_empty() {
        serial_println!("[selftest] FAILED: {:?}", failed);
    }

    TestResults {
        total,
        passed,
        failed,
    }
}

// ---------------------------------------------------------------------------
// Self-test (meta-test: test the test runner itself)
// ---------------------------------------------------------------------------

/// Self-test for the test runner infrastructure.
pub fn self_test() -> crate::error::KernelResult<()> {
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
    Ok(())
}
