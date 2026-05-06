//! Scheduler fairness measurement — quantifies CPU time distribution equity.
//!
//! Computes Jain's Fairness Index (JFI) across all active tasks to measure
//! how evenly CPU time is being distributed.  A JFI of 1.0 means all tasks
//! got exactly equal CPU time; lower values indicate some tasks are getting
//! disproportionately more or less time than others.
//!
//! ## Jain's Fairness Index
//!
//! For n tasks with CPU time allocations x_1, x_2, ..., x_n:
//!
//! ```text
//!          (∑ x_i)²
//! JFI = ───────────────
//!        n × ∑ (x_i²)
//! ```
//!
//! Properties:
//! - Range: [1/n, 1]
//! - 1.0 = perfectly fair (all tasks got equal time)
//! - 1/n = maximally unfair (one task got all the time)
//! - Sensitive to the *distribution* of allocations, not the absolute values
//!
//! ## Design
//!
//! Periodically samples per-task CPU tick counts from the scheduler,
//! computes JFI over the delta since last measurement.  This gives the
//! fairness over a window (not cumulative since boot, which would be
//! dominated by long-running tasks).
//!
//! ## Usage
//!
//! ```text
//! kshell> fairness        — show current JFI and per-task breakdown
//! ```
//!
//! ## References
//!
//! - Jain, R. et al. "A Quantitative Measure of Fairness and Discrimination
//!   for Resource Allocation in Shared Computer Systems" (DEC TR-301, 1984)
//! - Linux CFS: targets JFI ~0.99+ under normal workloads
//! - `perf sched latency`: similar per-task scheduling fairness report

use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum tasks tracked for fairness computation.
const MAX_TASKS: usize = 64;

// ---------------------------------------------------------------------------
// Fairness computation
// ---------------------------------------------------------------------------

/// Fairness measurement result.
#[derive(Debug, Clone)]
pub struct FairnessResult {
    /// Jain's Fairness Index (0.0 to 1.0, stored as fixed-point × 1000).
    /// e.g., 985 means JFI = 0.985.
    pub jfi_x1000: u64,
    /// Number of tasks included in the measurement.
    pub task_count: usize,
    /// Per-task CPU ticks in the measurement window.
    pub per_task_ticks: [u64; MAX_TASKS],
    /// Per-task names (first 8 bytes).
    pub per_task_names: [[u8; 8]; MAX_TASKS],
    /// Maximum ticks any single task consumed.
    pub max_ticks: u64,
    /// Minimum ticks any task consumed (of those that ran).
    pub min_ticks: u64,
}

/// Previous snapshot of per-task tick counts (for delta computation).
static PREV_TICKS: [AtomicU64; MAX_TASKS] = {
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; MAX_TASKS]
};

/// Number of measurements taken.
static MEASUREMENT_COUNT: AtomicU64 = AtomicU64::new(0);

/// Compute the current fairness index.
///
/// Queries the scheduler for all active tasks, computes the CPU tick
/// delta since the last measurement, and returns the Jain's Fairness
/// Index over that window.
pub fn measure() -> FairnessResult {
    // Get per-task tick counts from the scheduler.
    let mut task_ticks = [0u64; MAX_TASKS];
    let mut task_names = [[0u8; 8]; MAX_TASKS];
    let mut count = 0usize;

    // Query the scheduler for task stats.
    let task_list = crate::sched::all_task_ticks();
    for (i, (ticks, name_bytes, name_len)) in task_list.iter().enumerate() {
        if i >= MAX_TASKS {
            break;
        }
        if *ticks == 0 {
            continue; // Skip tasks that haven't run.
        }

        // Compute delta since last measurement.
        let prev = PREV_TICKS[i].swap(*ticks, Ordering::Relaxed);
        let delta = ticks.saturating_sub(prev);

        if delta > 0 {
            task_ticks[count] = delta;
            // Copy first 8 bytes of name.
            let copy_len = (*name_len).min(8);
            task_names[count][..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            count += 1;
        }
    }

    MEASUREMENT_COUNT.fetch_add(1, Ordering::Relaxed);

    if count == 0 {
        return FairnessResult {
            jfi_x1000: 1000, // No tasks = trivially fair.
            task_count: 0,
            per_task_ticks: task_ticks,
            per_task_names: task_names,
            max_ticks: 0,
            min_ticks: 0,
        };
    }

    // Compute Jain's Fairness Index.
    // JFI = (sum(x_i))^2 / (n * sum(x_i^2))
    let n = count as u64;
    let sum: u64 = task_ticks[..count].iter().sum();
    let sum_sq: u64 = task_ticks[..count].iter().map(|&x| x.saturating_mul(x)).sum();

    let jfi_x1000 = if sum_sq == 0 || n == 0 {
        1000
    } else {
        // (sum^2 * 1000) / (n * sum_sq)
        let numerator = sum.saturating_mul(sum).saturating_mul(1000);
        let denominator = n.saturating_mul(sum_sq);
        if denominator == 0 { 1000 } else { numerator / denominator }
    };

    let max_ticks = task_ticks[..count].iter().copied().max().unwrap_or(0);
    let min_ticks = task_ticks[..count].iter().copied()
        .filter(|&x| x > 0)
        .min()
        .unwrap_or(0);

    FairnessResult {
        jfi_x1000,
        task_count: count,
        per_task_ticks: task_ticks,
        per_task_names: task_names,
        max_ticks,
        min_ticks,
    }
}

/// Get the number of fairness measurements taken.
pub fn measurement_count() -> u64 {
    MEASUREMENT_COUNT.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the fairness measurement module.
pub fn self_test() {
    serial_println!("[sched_fairness] Running self-test...");

    // Test 1: Measure returns valid data.
    let r = measure();
    // JFI should be between 0 and 1000 (inclusive).
    assert!(r.jfi_x1000 <= 1000,
        "JFI should be <= 1000 (got {})", r.jfi_x1000);
    serial_println!("[sched_fairness]   Measure: OK (JFI={}.{:03}, tasks={})",
        r.jfi_x1000 / 1000, r.jfi_x1000 % 1000, r.task_count);

    // Test 2: JFI formula verification with known values.
    // Two tasks: [10, 10] → JFI = (20)^2 / (2 * (100+100)) = 400/400 = 1.0
    let sum = 20u64;
    let sum_sq = 200u64;
    let n = 2u64;
    let jfi = sum.saturating_mul(sum).saturating_mul(1000)
        / n.saturating_mul(sum_sq);
    assert_eq!(jfi, 1000, "equal distribution should give JFI=1.0");
    serial_println!("[sched_fairness]   Equal distribution: JFI=1.000 (correct)");

    // Two tasks: [1, 99] → JFI = (100)^2 / (2 * (1+9801)) = 10000/19604 ≈ 0.510
    let sum = 100u64;
    let sum_sq = 9802u64;
    let n = 2u64;
    let jfi = sum.saturating_mul(sum).saturating_mul(1000)
        / n.saturating_mul(sum_sq);
    assert!(jfi > 500 && jfi < 520,
        "1:99 distribution should give JFI ~0.510 (got {}.{:03})", jfi/1000, jfi%1000);
    serial_println!("[sched_fairness]   Skewed distribution: JFI={}.{:03} (correct)",
        jfi / 1000, jfi % 1000);

    // Test 3: Measurement count increases.
    let c1 = measurement_count();
    let _ = measure();
    let c2 = measurement_count();
    assert_eq!(c2, c1 + 1);
    serial_println!("[sched_fairness]   Counter increment: OK");

    serial_println!("[sched_fairness] Self-test PASSED");
}
