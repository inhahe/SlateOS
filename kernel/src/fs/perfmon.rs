//! Performance monitor — a Task-Manager-shaped view over the kernel's own
//! metrics history.
//!
//! Presents CPU and memory usage over time for a "Performance tab" style
//! display: a rolling window of samples, a latest reading, and threshold
//! alerts.
//!
//! ## Design Reference
//!
//! design.txt line 863: "a system-wide resource monitor view
//!   (CPU/RAM/disk/network graphs over time, like Windows Task
//!   Manager's Performance tab or htop)"
//!
//! ## Architecture — this module stores no samples
//!
//! ```text
//! Timer softirq ──► kstat::sample()  ──► kstat's 60-entry ring
//!                                             │
//! Task Manager / kshell / procfs ──► perfmon::cpu_history()
//!                                    perfmon::mem_history()
//!                                             │
//!                                    projects kstat::recent()
//! ```
//!
//! It used to own four `Vec` histories fed by `record_cpu`/`record_mem`/
//! `record_disk`/`record_net`. **Nothing ever called any of the four**, outside
//! this module's own self-test, so every history was permanently empty and
//! `/proc/perfmon` reported `CPU samples: 0` for the life of every boot — on a
//! kernel that had been sampling CPU and memory once a second since boot into
//! [`crate::kstat`]. The recorders were not an unfinished feature waiting for a
//! caller; a second sampler was never wanted, because the constraint that
//! shapes the real one (softirq context: no allocation, no blocking lock) makes
//! having two of them strictly worse than having one.
//!
//! So this is now a **projection**: every history is computed at read time from
//! `kstat`'s ring, and the only thing the module stores is the alert policy —
//! the thresholds, which are a user's choice and not a measurement. That is the
//! same shape `fs::pagecache` and `fs::netdev` settled on, and the reasoning is
//! recorded in `design-decisions.md` §641.
//!
//! ## What is not here, and why
//!
//! **Disk and network history are gone**, not stubbed. `kstat` records neither,
//! and it cannot be made to: a disk or interface counter read needs
//! `diskstat`'s or `net::interface`'s spin lock, and the sampler runs in the
//! timer softirq where a blocking acquire of a lock the interrupted code may
//! hold can never complete. The cumulative counters are published, live and
//! correct, by `/proc/diskstat` and `/proc/netdev`; what does not exist is a
//! *time series* of them, and a struct promising one it cannot fill is worse
//! than its absence.
//!
//! Likewise absent from [`CpuSample`]: a user/system split (the scheduler
//! publishes only `(total, idle)` ticks per CPU), a core frequency, a
//! temperature, and process/thread counts. Each was a field of the old struct
//! that no source in the kernel could fill, so each was only ever whatever the
//! one unreachable test caller passed in.

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::KernelResult;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// CPU usage at one point in the history.
///
/// Every field is projected from a [`crate::kstat::Sample`]; there is no
/// storage behind it.
#[derive(Debug, Clone)]
pub struct CpuSample {
    /// Timestamp (ns since boot), derived from the sample's tick count.
    pub timestamp_ns: u64,
    /// Overall CPU usage (0-100) — the mean of [`per_core`](Self::per_core).
    pub usage_pct: u32,
    /// Per-core usage (0-100 each) over the interval since the previous
    /// sample, not since boot; see [`crate::kstat::Sample::cpu_util`] for why
    /// that distinction is the difference between a graph and a flat line.
    pub per_core: Vec<u32>,
    /// Tasks the scheduler could have run at that instant.
    pub runnable_tasks: u32,
    /// Tasks that existed at that instant (spawned minus exited).
    pub live_tasks: u32,
}

/// Memory usage at one point in the history.
#[derive(Debug, Clone)]
pub struct MemSample {
    /// Timestamp (ns since boot).
    pub timestamp_ns: u64,
    /// Used physical memory (bytes).
    pub used_bytes: u64,
    /// Free physical memory (bytes).
    pub available_bytes: u64,
    /// Total physical memory (bytes).
    pub total_bytes: u64,
    /// Kernel heap bytes in use.
    pub heap_bytes: u64,
    /// Memory pressure score (0-100).
    pub pressure_score: u32,
}

impl MemSample {
    /// Used physical memory as a percentage of total (0-100).
    ///
    /// Returns 0 when total is 0, which happens only for a sample taken before
    /// the frame allocator published a size.
    #[must_use]
    pub fn used_pct(&self) -> u32 {
        // `checked_div` rather than a guard-then-divide: the guard and the
        // division are then one expression that cannot be separated by a later
        // edit, which is the failure mode a bare `/` behind an `if` invites.
        let pct = self
            .used_bytes
            .saturating_mul(100)
            .checked_div(self.total_bytes)
            .unwrap_or(0);
        u32::try_from(pct.min(100)).unwrap_or(100)
    }
}

/// Monitor configuration.
///
/// Two of these four fields are *reported*, not *set*: the sample interval and
/// the history depth belong to [`crate::kstat`], which owns the buffer. They
/// appear here so a caller has one place to ask what window it is looking at.
///
/// They used to be settable, and the setters were the clearest symptom of the
/// module's problem: `set_interval(500)` stored 500, `get_config()` read 500
/// back, and nothing anywhere sampled any faster — because this module had no
/// sampler. A knob that moves and changes nothing is worse than a fixed value,
/// since it answers the question "did that work?" with yes.
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Interval between samples, in milliseconds. Read-only: `kstat`'s.
    pub sample_interval_ms: u32,
    /// History depth, in samples. Read-only: `kstat`'s ring size.
    pub max_samples: usize,
    /// Alert threshold: CPU usage percentage.
    pub cpu_alert_pct: u32,
    /// Alert threshold: memory usage percentage.
    pub mem_alert_pct: u32,
}

/// A resource reading that is over its threshold *right now*.
///
/// Alerts are derived from the newest sample each time they are asked for, and
/// so have no identity and cannot be dismissed. That is deliberate: the old
/// implementation appended an `Alert` row per over-threshold sample and offered
/// `dismiss_alert(id)`, which meant a CPU pinned at 100% produced a growing
/// list of identical rows, and dismissing them all made a still-overloaded
/// machine report itself healthy. A condition that is still true cannot be
/// dismissed; one that has passed disappears without being told to.
#[derive(Debug, Clone)]
pub struct Alert {
    /// Resource name (`"CPU"` or `"Memory"`).
    pub resource: String,
    /// Human-readable description.
    pub message: String,
    /// Current value (percent).
    pub value: u32,
    /// Threshold that was crossed (percent).
    pub threshold: u32,
    /// Timestamp (ns since boot) of the sample that crossed it.
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default CPU alert threshold, percent.
const DEFAULT_CPU_ALERT_PCT: u32 = 90;

/// Default memory alert threshold, percent.
const DEFAULT_MEM_ALERT_PCT: u32 = 90;

/// Bytes per physical frame. `kstat` counts frames; callers want bytes.
const FRAME_BYTES: u64 = 16 * 1024;

// ---------------------------------------------------------------------------
// State — thresholds only
// ---------------------------------------------------------------------------

/// The only thing this module stores: the alert policy.
///
/// Deliberately not a cache of anything. Anything derivable from `kstat` is
/// derived on demand, so there is no window in which this disagrees with the
/// numbers it is meant to describe.
struct State {
    cpu_alert_pct: u32,
    mem_alert_pct: u32,
    /// Threshold changes since boot. Reported by [`stats`] so a caller can see
    /// that the policy has been touched.
    changes: u64,
}

impl State {
    /// The state a fresh boot starts with.
    ///
    /// Extracted from the initialiser of `STATE` so the self-test can be handed
    /// a pristine copy without disturbing the live one; see
    /// `crate::fs::selftest`.
    const fn new() -> Self {
        Self {
            cpu_alert_pct: DEFAULT_CPU_ALERT_PCT,
            mem_alert_pct: DEFAULT_MEM_ALERT_PCT,
            changes: 0,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Get monitor configuration: the live thresholds, plus the window `kstat`
/// actually provides.
#[must_use]
pub fn get_config() -> MonitorConfig {
    let state = STATE.lock();
    MonitorConfig {
        sample_interval_ms: u32::try_from(crate::kstat::sample_interval_ms()).unwrap_or(u32::MAX),
        max_samples: crate::kstat::history_depth(),
        cpu_alert_pct: state.cpu_alert_pct,
        mem_alert_pct: state.mem_alert_pct,
    }
}

/// Set the CPU alert threshold, and return the value stored.
///
/// Clamped to 0-100, so the return value is not always the argument. Returning
/// it rather than clamping silently is what lets a caller report what actually
/// happened without restating the range and drifting from it.
pub fn set_cpu_alert(pct: u32) -> u32 {
    let mut state = STATE.lock();
    let effective = pct.min(100);
    state.cpu_alert_pct = effective;
    state.changes = state.changes.saturating_add(1);
    effective
}

/// Set the memory alert threshold, and return the value stored.
///
/// Clamped to 0-100; see [`set_cpu_alert`].
pub fn set_mem_alert(pct: u32) -> u32 {
    let mut state = STATE.lock();
    let effective = pct.min(100);
    state.mem_alert_pct = effective;
    state.changes = state.changes.saturating_add(1);
    effective
}

/// Restore the default alert thresholds.
pub fn init_defaults() {
    let mut state = STATE.lock();
    state.cpu_alert_pct = DEFAULT_CPU_ALERT_PCT;
    state.mem_alert_pct = DEFAULT_MEM_ALERT_PCT;
    state.changes = state.changes.saturating_add(1);
}

// ---------------------------------------------------------------------------
// Projection
// ---------------------------------------------------------------------------

/// Convert a `kstat` tick count to nanoseconds since boot.
///
/// Exact rather than approximate: the tick rate divides a nanosecond evenly at
/// every rate the APIC is programmed to, so this is a multiplication and not a
/// rounding.
fn tick_to_ns(tick: u64) -> u64 {
    // A tick rate of 0 would be a misconfigured APIC rather than a value to
    // handle, but it is a `const` someone can edit, and a division by it here
    // would take the kernel down at a metrics read.  Falling back to 0 ns/tick
    // makes every timestamp 0 — visibly wrong, which is the right failure for a
    // display path.
    let ns_per_tick = 1_000_000_000u64
        .checked_div(u64::from(crate::apic::TICK_RATE_HZ))
        .unwrap_or(0);
    tick.saturating_mul(ns_per_tick)
}

/// Project one `kstat` sample into a CPU reading.
fn project_cpu(s: &crate::kstat::Sample) -> CpuSample {
    // `cpu_util` is a fixed four-wide array whose unused tail is zero, so the
    // number of cores has to come from somewhere other than its length; the
    // scheduler's CPU count is that somewhere. Taking `len()` instead would
    // report a 2-core machine as having two idle cores it does not have.
    let ncpu = crate::sched::sched_stats().num_cpus.min(s.cpu_util.len());
    let per_core: Vec<u32> = s
        .cpu_util
        .iter()
        .take(ncpu)
        .map(|&u| u32::from(u).min(100))
        .collect();
    // `checked_div` covers the empty-`per_core` case (a machine the scheduler
    // reports as having no CPUs, i.e. a sample taken before SMP brought any
    // up), so the emptiness test and the division stay one expression.
    let sum: u32 = per_core.iter().copied().sum();
    let usage_pct = u32::try_from(per_core.len())
        .ok()
        .and_then(|n| sum.checked_div(n))
        .unwrap_or(0);
    CpuSample {
        timestamp_ns: tick_to_ns(s.tick),
        usage_pct,
        per_core,
        runnable_tasks: u32::from(s.runnable_tasks),
        live_tasks: u32::from(s.live_tasks),
    }
}

/// Project one `kstat` sample into a memory reading.
fn project_mem(s: &crate::kstat::Sample) -> MemSample {
    let total = u64::from(s.total_frames).saturating_mul(FRAME_BYTES);
    let free = u64::from(s.free_frames).saturating_mul(FRAME_BYTES);
    MemSample {
        timestamp_ns: tick_to_ns(s.tick),
        used_bytes: total.saturating_sub(free),
        available_bytes: free,
        total_bytes: total,
        heap_bytes: u64::from(s.heap_bytes_in_use),
        pressure_score: u32::from(s.pressure_score),
    }
}

/// Fetch the whole `kstat` window, newest first.
fn window() -> Vec<crate::kstat::Sample> {
    crate::kstat::recent(crate::kstat::history_depth())
}

// ---------------------------------------------------------------------------
// History retrieval
// ---------------------------------------------------------------------------

/// CPU history, **oldest first**, so a caller can plot it left to right.
///
/// `kstat::recent` hands back newest-first; the reversal happens here rather
/// than at each of the three call sites that would otherwise each have to
/// remember to do it.
#[must_use]
pub fn cpu_history() -> Vec<CpuSample> {
    let mut w = window();
    w.reverse();
    w.iter().map(project_cpu).collect()
}

/// Most recent CPU reading, or `None` before the first sample is taken.
#[must_use]
pub fn cpu_latest() -> Option<CpuSample> {
    crate::kstat::recent(1).first().map(project_cpu)
}

/// Memory history, oldest first.
#[must_use]
pub fn mem_history() -> Vec<MemSample> {
    let mut w = window();
    w.reverse();
    w.iter().map(project_mem).collect()
}

/// Most recent memory reading, or `None` before the first sample is taken.
#[must_use]
pub fn mem_latest() -> Option<MemSample> {
    crate::kstat::recent(1).first().map(project_mem)
}

// ---------------------------------------------------------------------------
// Alerts
// ---------------------------------------------------------------------------

/// Conditions currently over threshold, derived from the newest sample.
///
/// Empty before the first sample, and empty again the moment the load drops —
/// there is no list to prune and nothing to dismiss. See [`Alert`].
#[must_use]
pub fn active_alerts() -> Vec<Alert> {
    let (cpu_threshold, mem_threshold) = {
        let state = STATE.lock();
        (state.cpu_alert_pct, state.mem_alert_pct)
    };

    let Some(s) = crate::kstat::recent(1).first().copied() else {
        return Vec::new();
    };

    let mut out = Vec::new();

    let cpu = project_cpu(&s);
    if cpu.usage_pct >= cpu_threshold {
        out.push(Alert {
            resource: String::from("CPU"),
            message: format!(
                "CPU usage {}% exceeds threshold {}%",
                cpu.usage_pct, cpu_threshold
            ),
            value: cpu.usage_pct,
            threshold: cpu_threshold,
            timestamp_ns: cpu.timestamp_ns,
        });
    }

    let mem = project_mem(&s);
    let used_pct = mem.used_pct();
    // A total of zero means the sample predates the frame allocator publishing
    // a size, which would make `used_pct` 0 — harmless — but a threshold of 0
    // would then fire on it and report a memory alert on an empty machine.
    if mem.total_bytes > 0 && used_pct >= mem_threshold {
        out.push(Alert {
            resource: String::from("Memory"),
            message: format!(
                "Memory usage {}% exceeds threshold {}%",
                used_pct, mem_threshold
            ),
            value: used_pct,
            threshold: mem_threshold,
            timestamp_ns: mem.timestamp_ns,
        });
    }

    out
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Return `(samples_in_window, samples_since_boot, active_alerts, threshold_changes)`.
///
/// `samples_in_window` is what the histories will hand back; `samples_since_boot`
/// keeps rising past the ring's depth and is how a caller tells "sampling has
/// been running for an hour" from "the buffer holds an hour".
#[must_use]
pub fn stats() -> (usize, u64, usize, u64) {
    let changes = STATE.lock().changes;
    let total = crate::kstat::total_samples();
    let in_window = usize::try_from(total)
        .unwrap_or(usize::MAX)
        .min(crate::kstat::history_depth());
    (in_window, total, active_alerts().len(), changes)
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// The suite changes thresholds, so it needs a policy table of its own; the
/// live one is moved aside for the duration and put back afterwards. See
/// `crate::fs::selftest` for why this shape rather than the alternatives.
///
/// There are no counters outside the table to save any more — the sample counts
/// this module reports belong to `kstat` and the suite does not write them.
///
/// Returns `KernelResult` because both callers (`main`'s boot suite and the
/// `perfmon test` shell arm) expect one, not because the body can fail: every
/// check here panics on failure like the rest of `fs`'s suites. The inner
/// function therefore returns `()` and the wrap happens here, so no reader has
/// to look for the `Err` arm that does not exist.
pub fn self_test() -> KernelResult<()> {
    crate::fs::selftest::with_pristine(&STATE, State::new(), self_test_inner);
    Ok(())
}

fn self_test_inner() {
    use crate::serial_println;

    // Test 1: the config reports kstat's window, not a wish.
    //
    // This is the assertion the module most needed and did not have. It used to
    // advertise `max_samples: 300` and a 1000 ms interval as stored config,
    // while nothing sampled at all — so both numbers were arbitrary and no test
    // could have caught it, because there was nothing to compare them against.
    serial_println!("perfmon::self_test 1: config mirrors kstat");
    let cfg = get_config();
    assert_eq!(
        cfg.max_samples,
        crate::kstat::history_depth(),
        "history depth must be kstat's, not a second copy of it"
    );
    assert_eq!(
        u64::from(cfg.sample_interval_ms),
        crate::kstat::sample_interval_ms(),
        "sample interval must be kstat's"
    );
    assert_eq!(cfg.cpu_alert_pct, DEFAULT_CPU_ALERT_PCT);
    assert_eq!(cfg.mem_alert_pct, DEFAULT_MEM_ALERT_PCT);

    // Test 2: thresholds clamp, and report what they stored.
    serial_println!("perfmon::self_test 2: threshold clamping");
    assert_eq!(set_cpu_alert(150), 100, "above-range request clamps");
    assert_eq!(get_config().cpu_alert_pct, 100, "and the clamp is stored");
    assert_eq!(set_mem_alert(150), 100, "above-range request clamps");
    assert_eq!(set_cpu_alert(90), 90, "an in-range request is honoured");
    assert_eq!(set_mem_alert(90), 90, "an in-range request is honoured");

    // Test 3: the histories are as deep as kstat's window and no deeper.
    //
    // Asserting a *bound* rather than a count, because the depth on a live
    // kernel depends on how long it has been up: below the ring size early in
    // boot, exactly the ring size afterwards. Both are correct; more than the
    // ring size never is.
    serial_println!("perfmon::self_test 3: history depth");
    let depth = crate::kstat::history_depth();
    let before = usize::try_from(crate::kstat::total_samples())
        .unwrap_or(usize::MAX)
        .min(depth);
    let cpu_hist = cpu_history();
    let mem_hist = mem_history();
    assert!(
        cpu_hist.len() <= depth,
        "cpu history {} exceeds kstat's ring of {depth}",
        cpu_hist.len()
    );
    // Bracketed rather than equated, and this is not defensiveness: the softirq
    // keeps sampling while this test runs, so `total_samples()` read before and
    // after the projection can genuinely differ, and an `assert_eq` against
    // either one is a boot that panics roughly once per (interval / runtime)
    // boots.  Bracketing asserts exactly as much as is true — the length is the
    // sample count as of *some* instant inside this test — and nothing that
    // isn't.  `before` is captured above the `cpu_history()` call at the top of
    // this test, so the bracket really does contain the projection.
    let after = usize::try_from(crate::kstat::total_samples())
        .unwrap_or(usize::MAX)
        .min(depth);
    assert!(
        cpu_hist.len() >= before && cpu_hist.len() <= after,
        "history holds {} samples, outside the {before}..={after} kstat took across \
         the projection — it is not projecting the whole ring",
        cpu_hist.len()
    );
    // The two histories are separately projected, so they can differ by the one
    // sample that may land between them, but no more: they read one ring.
    assert!(
        cpu_hist.len().abs_diff(mem_hist.len()) <= 1,
        "cpu history ({}) and mem history ({}) project the same ring and cannot \
         differ by more than the one sample that may land between them",
        cpu_hist.len(),
        mem_hist.len()
    );

    // Test 4: history is oldest-first, and `latest` is its last element.
    //
    // The ordering matters to every caller that plots it, and `kstat::recent`
    // returns the opposite order, so getting this backwards is a one-character
    // mistake that draws every graph mirrored.
    serial_println!("perfmon::self_test 4: ordering");
    if cpu_hist.len() >= 2 {
        let first = cpu_hist.first().map_or(0, |c| c.timestamp_ns);
        let last = cpu_hist.last().map_or(0, |c| c.timestamp_ns);
        assert!(
            first <= last,
            "history must run oldest ({first}) to newest ({last})"
        );
    }
    if let (Some(latest), Some(tail)) = (cpu_latest(), cpu_hist.last()) {
        // A sample may land between the two calls, so the newest of the history
        // can only be asserted to be no *newer* than a later reading.
        assert!(
            tail.timestamp_ns <= latest.timestamp_ns,
            "cpu_latest must be at least as recent as the end of the history"
        );
    }

    // Test 5: projection arithmetic holds on whatever the live sample says.
    serial_println!("perfmon::self_test 5: projection");
    if let Some(mem) = mem_latest() {
        assert_eq!(
            mem.used_bytes.saturating_add(mem.available_bytes),
            mem.total_bytes,
            "used + free must account for all of physical memory"
        );
        assert!(mem.used_pct() <= 100, "a percentage cannot exceed 100");
        assert!(
            mem.used_bytes <= mem.total_bytes,
            "used ({}) cannot exceed total ({})",
            mem.used_bytes,
            mem.total_bytes
        );
    }
    if let Some(cpu) = cpu_latest() {
        assert!(cpu.usage_pct <= 100, "a percentage cannot exceed 100");
        assert!(
            cpu.per_core.iter().all(|&c| c <= 100),
            "a per-core percentage cannot exceed 100"
        );
        assert_eq!(
            cpu.per_core.len(),
            crate::sched::sched_stats().num_cpus.min(4),
            "per_core must be as wide as the machine, not as wide as the array"
        );
    }

    // Test 6: alerts fire on the live reading and clear when the bar is raised.
    //
    // Driven by moving the *threshold* rather than the load, because this
    // module can no longer be handed a fabricated sample — which is the point
    // of the rewrite, and does cost the suite its ability to test an exact
    // value. A threshold of 0 fires on any reading, including a genuine 0%.
    serial_println!("perfmon::self_test 6: alerts");
    if crate::kstat::total_samples() > 0 {
        set_cpu_alert(0);
        let firing = active_alerts();
        assert!(
            firing.iter().any(|a| a.resource == "CPU"),
            "a threshold of 0% must fire on any CPU reading"
        );
        set_cpu_alert(100); // Only a fully pinned CPU can reach it.
        let quiet = active_alerts();
        assert!(
            quiet.iter().all(|a| a.resource != "CPU") || quiet.iter().any(|a| a.value >= 100),
            "a threshold of 100% must be quiet unless the CPU really is pinned"
        );
        set_cpu_alert(DEFAULT_CPU_ALERT_PCT);
    }

    // Test 7: stats agree with the histories they describe.
    serial_println!("perfmon::self_test 7: stats");
    let (in_window, since_boot, alerts_n, changes) = stats();
    // Again bracketed against the live sampler rather than equated; see test 3.
    let hist_len = cpu_history().len();
    assert!(
        hist_len >= in_window && hist_len <= in_window.saturating_add(1),
        "stats reports {in_window} samples in the window but the history returns \
         {hist_len} — the two are not counting the same ring"
    );
    assert!(
        since_boot >= in_window as u64,
        "samples since boot ({since_boot}) cannot be fewer than the window holds ({in_window})"
    );
    // Not compared to a second `active_alerts()` call: the reading moves, so a
    // count taken now may honestly differ.  What must hold is that `stats` did
    // not invent a figure — it can only report as many alerts as there are
    // resources to alert on.
    assert!(
        alerts_n <= 2,
        "stats reports {alerts_n} alerts, but only CPU and memory can raise one"
    );
    assert!(
        changes > 0,
        "test 2 changed thresholds, so this must have moved"
    );

    // Test 8: defaults are restorable.
    serial_println!("perfmon::self_test 8: init_defaults");
    set_cpu_alert(11);
    set_mem_alert(22);
    init_defaults();
    let cfg = get_config();
    assert_eq!(cfg.cpu_alert_pct, DEFAULT_CPU_ALERT_PCT);
    assert_eq!(cfg.mem_alert_pct, DEFAULT_MEM_ALERT_PCT);

    serial_println!("perfmon::self_test: all 8 tests passed");
}
