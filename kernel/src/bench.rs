//! Kernel microbenchmark infrastructure.
//!
//! Provides cycle-accurate timing via `rdtsc` and a simple benchmark
//! runner that measures min/mean/max cycles for kernel operations.
//!
//! ## Usage
//!
//! ```ignore
//! bench::run("page_alloc", 1000, || {
//!     let f = frame::alloc_frame().unwrap();
//!     unsafe { frame::free_frame(f).unwrap(); }
//! });
//! ```
//!
//! Results are printed to serial in a format that can be compared
//! against the baselines in `bench/baselines.toml`.
//!
//! ## TSC frequency
//!
//! The TSC (Time Stamp Counter) is calibrated against the PIT at boot.
//! This gives us a cycles-to-nanoseconds conversion factor.  All
//! results report both cycles and nanoseconds.
//!
//! ## Caveats
//!
//! - Under QEMU, TSC behavior depends on the acceleration backend
//!   (KVM/WHPX).  Cycle counts are approximate but consistent enough
//!   for relative comparisons.
//! - Interrupts are not disabled during benchmarks (we measure
//!   realistic conditions).  For tight micro-benchmarks, take the
//!   minimum as the most representative value.
//!
//! ## Lint policy
//!
//! Benchmarks run at boot under controlled conditions, not on attacker
//! input.  Panicking here means the benchmark itself is broken, which
//! is fine to surface loudly.  Defensive `?`/`.get()`/`checked_*`
//! boilerplate would obscure the measurement code without adding any
//! defence-in-depth value.  Allow the panicking-style lints at module
//! scope.

#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// TSC reading
// ---------------------------------------------------------------------------

/// Read the Time Stamp Counter (TSC).
///
/// Returns the number of CPU cycles since power-on (approximately).
/// On modern x86_64, TSC is invariant (doesn't change with frequency
/// scaling), making it a reliable monotonic clock source.
#[inline]
pub fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: rdtsc is always available on x86_64 and has no side effects.
    // We use plain rdtsc (not rdtscp) for maximum compatibility — QEMU's
    // emulated CPU may not support rdtscp.  For precise benchmarks,
    // rdtsc_serialized() adds a cpuid fence before the read.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// A serializing fence before TSC read (ensures prior instructions
/// complete before reading the counter).
#[inline]
pub fn serialize() {
    // SAFETY: cpuid is a serializing instruction, always available on x86_64.
    // LLVM reserves rbx, so we save/restore it via xchg with a temp register
    // (the standard Rust inline-asm pattern for cpuid).
    unsafe {
        core::arch::asm!(
            "xchg rbx, {tmp}",
            "cpuid",
            "xchg rbx, {tmp}",
            tmp = out(reg) _,
            inout("eax") 0u32 => _,
            out("ecx") _,
            out("edx") _,
            options(nomem, preserves_flags),
        );
    }
}

/// Read TSC with serialization (for precise micro-benchmarks).
///
/// Uses cpuid (serializing) before rdtscp to ensure all prior
/// instructions are retired before the timestamp is taken.
#[inline]
pub fn rdtsc_serialized() -> u64 {
    serialize();
    rdtsc()
}

// ---------------------------------------------------------------------------
// TSC frequency calibration
// ---------------------------------------------------------------------------

/// TSC frequency in Hz, calibrated once at boot.
///
/// A **lock-free `AtomicU64`**, not a `Mutex`: this is written exactly once by
/// [`calibrate_tsc`] and read forever after, and — critically — it is read on
/// the hot [`crate::timekeeping::clock_monotonic`] path, which is itself called
/// from **interrupt and NMI context** (the timer tick's scheduler heartbeat and
/// the hard-lockup watchdog's `classify_nmi`). A spinlock here self-deadlocks
/// on a uniprocessor: if a timer IRQ or the watchdog NMI fires while non-IRQ
/// code holds the lock, the re-entrant `clock_monotonic` → `tsc_freq` spins on
/// the held lock forever with interrupts disabled — a silent BSP-dead hang with
/// no further ticks (root cause of the boot-battery wedge; see known-issues.md).
/// An atomic load has no such hazard and is also faster on the hot path.
static TSC_FREQ: AtomicU64 = AtomicU64::new(0);

/// Calibrate the TSC frequency using the PIT (Programmable Interval Timer).
///
/// Programs PIT channel 2 for a ~10 ms countdown, measures TSC ticks
/// during that interval, and derives the TSC frequency.
///
/// Must be called after the PIT is accessible (very early in boot).
pub fn calibrate_tsc() {
    // PIT oscillator: 1,193,182 Hz.
    const PIT_FREQ: u32 = 1_193_182;
    // Count for ~10 ms.
    const PIT_COUNT: u16 = (PIT_FREQ / 100) as u16;

    // --- Program PIT channel 2 for one-shot countdown ---
    // Channel 2 is connected to the speaker gate, not IRQs, so we
    // can use it without interfering with the timer interrupt.

    // SAFETY: Direct port I/O to PIT registers.  These are always
    // accessible in ring 0 on x86_64.
    unsafe {
        use crate::port::{inb, outb};

        // Gate on: set bit 0 of port 0x61 (speaker control), clear bit 1
        // (speaker output).
        let gate = inb(0x61);
        outb(0x61, (gate & 0xFC) | 0x01);

        // Command: channel 2, lo/hi byte, mode 0 (one-shot), binary.
        outb(0x43, 0xB0);

        // Write count (lo then hi).
        outb(0x42, (PIT_COUNT & 0xFF) as u8);
        outb(0x42, (PIT_COUNT >> 8) as u8);

        // Read the start TSC.
        let tsc_start = rdtsc_serialized();

        // Wait for PIT channel 2 to count down.
        // Bit 5 of port 0x61 goes high when the count reaches zero.
        loop {
            let status = inb(0x61);
            if status & 0x20 != 0 {
                break;
            }
        }

        // Read the end TSC.
        let tsc_end = rdtsc_serialized();

        // Calculate TSC ticks per 10 ms, then derive frequency.
        let tsc_ticks = tsc_end.saturating_sub(tsc_start);
        // PIT_COUNT ticks at PIT_FREQ Hz = PIT_COUNT / PIT_FREQ seconds.
        // TSC frequency = tsc_ticks / (PIT_COUNT / PIT_FREQ)
        //               = tsc_ticks * PIT_FREQ / PIT_COUNT
        let freq = tsc_ticks
            .saturating_mul(PIT_FREQ as u64)
            .checked_div(PIT_COUNT as u64)
            .unwrap_or(0);

        TSC_FREQ.store(freq, Ordering::Relaxed);

        serial_println!(
            "[bench] TSC calibrated: {} ticks in ~10ms → {:.1} MHz ({} Hz)",
            tsc_ticks,
            freq as f64 / 1_000_000.0,
            freq
        );

        // Restore speaker gate.
        outb(0x61, gate);
    }
}

/// Get the calibrated TSC frequency in Hz.
///
/// Returns 0 if `calibrate_tsc()` has not been called.
#[must_use]
pub fn tsc_freq() -> u64 {
    // Lock-free load: safe to call from IRQ/NMI context (see `TSC_FREQ`).
    TSC_FREQ.load(Ordering::Relaxed)
}

/// Convert TSC cycles to nanoseconds using the calibrated frequency.
///
/// Returns 0 if TSC frequency is not calibrated.
#[must_use]
pub fn cycles_to_ns(cycles: u64) -> u64 {
    let freq = tsc_freq();
    if freq == 0 {
        return 0;
    }
    // ns = cycles * 1_000_000_000 / freq
    // To avoid overflow: (cycles / freq) * 1e9 + (cycles % freq) * 1e9 / freq
    let whole = cycles.checked_div(freq).unwrap_or(0);
    let remainder = cycles.checked_rem(freq).unwrap_or(0);
    whole
        .saturating_mul(1_000_000_000)
        .saturating_add(
            remainder
                .saturating_mul(1_000_000_000)
                .checked_div(freq)
                .unwrap_or(0),
        )
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

/// Result of a benchmark run.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields are available for external benchmark analysis.
pub struct BenchResult {
    /// Benchmark name.
    pub name: String,
    /// Number of iterations.
    pub iterations: u32,
    /// Minimum cycles per iteration.
    pub min_cycles: u64,
    /// Mean cycles per iteration.
    pub mean_cycles: u64,
    /// Maximum cycles per iteration.
    pub max_cycles: u64,
    /// Minimum nanoseconds per iteration.
    pub min_ns: u64,
    /// Mean nanoseconds per iteration.
    pub mean_ns: u64,
}

/// Time a single execution of `f`, in TSC cycles.
///
/// The leading read is serialised so earlier work cannot drift into the
/// measured window; the trailing one is not, because the following
/// instructions are the caller's arithmetic and cannot be reordered ahead of
/// the `rdtsc` in a way that matters here.
/// Any value the closure returns is kept alive with `black_box` *after* the
/// clock stops, so the optimiser cannot delete the work being measured
/// without that keep-alive landing inside the measured window.
#[inline]
fn timed<R, F: FnOnce() -> R>(f: F) -> u64 {
    let start = rdtsc_serialized();
    let produced = f();
    let elapsed = rdtsc().saturating_sub(start);
    core::hint::black_box(produced);
    elapsed
}

/// Compare two alternatives by **interleaving** their iterations, returning
/// `(min_a, min_b)` in cycles.
///
/// Use this instead of two `run()` calls whenever the question is "what does
/// X cost?" rather than "how fast is X?".  Two consecutive measurement
/// windows on a live system are not the same system: a service waking during
/// the second window inflates it wholesale, and `min` does not rescue you —
/// it is robust to *spikes*, not to a window that is uniformly busier than
/// its neighbour.  This is not hypothetical; the `frame_owner` A/B below
/// reported a 10826-cycle cost that way, and the same measurement
/// interleaved reported a fraction of it.
///
/// Interleaving fixes that because the two arms run microseconds apart, so
/// any load drifting on a scheduling timescale lifts both equally and cancels
/// in the difference.
///
/// Each closure returns *its own* elapsed cycles rather than being timed from
/// the outside, so an arm can do untimed setup (flipping a feature flag,
/// say) before starting its own clock.  Use [`timed`] for the measured part.
fn ab_interleaved<A, B>(rounds: u32, mut a: A, mut b: B) -> (u64, u64)
where
    A: FnMut() -> u64,
    B: FnMut() -> u64,
{
    // Warm both arms: first-touch costs (TCG translation of the block, cold
    // branch predictors) belong to neither measurement.
    for _ in 0..core::cmp::max(rounds / 20, 5) {
        let _ = a();
        let _ = b();
    }

    let mut min_a = u64::MAX;
    let mut min_b = u64::MAX;
    for _ in 0..rounds {
        min_a = min_a.min(a());
        min_b = min_b.min(b());
    }
    (min_a, min_b)
}

/// Run a micro-benchmark, reporting min/mean/max cycles.
///
/// Executes `f` a total of `warmup + iterations` times.  The first
/// `warmup` runs are discarded (cache warming).  Results are printed
/// to serial.
///
/// Returns the `BenchResult` for programmatic comparison.
pub fn run<F: FnMut()>(name: &str, iterations: u32, mut f: F) -> BenchResult {
    // Warmup: 10% of iterations, minimum 5.
    let warmup = core::cmp::max(iterations / 10, 5);

    for _ in 0..warmup {
        f();
    }

    let mut min = u64::MAX;
    let mut max = 0u64;
    let mut total = 0u64;

    for _ in 0..iterations {
        let start = rdtsc_serialized();
        f();
        let end = rdtsc();
        let elapsed = end.saturating_sub(start);

        if elapsed < min {
            min = elapsed;
        }
        if elapsed > max {
            max = elapsed;
        }
        total = total.saturating_add(elapsed);
    }

    let mean = total.checked_div(iterations as u64).unwrap_or(0);
    let min_ns = cycles_to_ns(min);
    let mean_ns = cycles_to_ns(mean);

    serial_println!(
        "[bench] {}: min={} cycles ({}ns), mean={} cycles ({}ns), max={} cycles  [{} iters]",
        name, min, min_ns, mean, mean_ns, max, iterations
    );

    BenchResult {
        name: String::from(name),
        iterations,
        min_cycles: min,
        mean_cycles: mean,
        max_cycles: max,
        min_ns,
        mean_ns,
    }
}

// ---------------------------------------------------------------------------
// PMC-aware benchmark variant
// ---------------------------------------------------------------------------

/// Run a micro-benchmark with optional PMC measurement.
///
/// If PMC hardware is available, measures LLC misses alongside cycle
/// counts.  This provides insight into whether a function is cache-bound
/// or compute-bound.
///
/// Falls back to plain `run()` if PMU is unavailable.
#[allow(dead_code)]
pub fn run_with_cache_info<F: FnMut()>(name: &str, iterations: u32, mut f: F) -> BenchResult {
    use crate::pmc;

    let has_pmc = pmc::is_available();

    // Configure LLC miss counter if available.
    if has_pmc {
        pmc::configure(0, pmc::Event::LlcMisses);
        pmc::configure(1, pmc::Event::InstructionsRetired);
    }

    // Warmup: 10% of iterations, minimum 5.
    let warmup = core::cmp::max(iterations / 10, 5);
    for _ in 0..warmup {
        f();
    }

    let mut min = u64::MAX;
    let mut max = 0u64;
    let mut total = 0u64;

    // Start PMC counters for the measurement phase.
    if has_pmc {
        pmc::reset(0);
        pmc::reset(1);
        pmc::start(0);
        pmc::start(1);
    }

    for _ in 0..iterations {
        let start = rdtsc_serialized();
        f();
        let end = rdtsc();
        let elapsed = end.saturating_sub(start);
        if elapsed < min { min = elapsed; }
        if elapsed > max { max = elapsed; }
        total = total.saturating_add(elapsed);
    }

    if has_pmc {
        pmc::stop(0);
        pmc::stop(1);
    }

    let mean = total.checked_div(iterations as u64).unwrap_or(0);
    let min_ns = cycles_to_ns(min);
    let mean_ns = cycles_to_ns(mean);

    serial_println!(
        "[bench] {}: min={} cycles ({}ns), mean={} cycles ({}ns), max={} cycles  [{} iters]",
        name, min, min_ns, mean, mean_ns, max, iterations
    );

    // Report PMC data if available.
    if has_pmc {
        let llc_misses = pmc::read(0);
        let insns = pmc::read(1);
        let misses_per_iter = llc_misses.checked_div(iterations as u64).unwrap_or(0);
        let insns_per_iter = insns.checked_div(iterations as u64).unwrap_or(0);
        serial_println!(
            "[bench]   └─ PMC: {} LLC misses/iter, {} insns/iter, {:.2} IPC",
            misses_per_iter, insns_per_iter,
            if mean > 0 { insns_per_iter as f64 / mean as f64 } else { 0.0 }
        );
    }

    BenchResult {
        name: String::from(name),
        iterations,
        min_cycles: min,
        mean_cycles: mean,
        max_cycles: max,
        min_ns,
        mean_ns,
    }
}

// ---------------------------------------------------------------------------
// Scorecard — automated baseline comparison
// ---------------------------------------------------------------------------

/// A single scorecard entry comparing a benchmark against its target.
struct ScoreEntry {
    name: &'static str,
    measured_ns: u64,
    target_ns: u64,
    passed: bool,
    /// Mean nanoseconds per iteration, carried alongside the reported minimum.
    ///
    /// Not a second performance number — a *dispersion* number. The scorecard
    /// reports `min` because it is the least contaminated estimate of the
    /// code's cost, but that makes every entry look equally trustworthy when
    /// they are not: a benchmark whose mean sits at 1.05x its min took a clean
    /// measurement on nearly every iteration, whereas one at 6x (measured:
    /// `dashboard_api_status`, 160.4ms mean against a 24.4ms min) was
    /// interrupted on most of them, so its min is whichever iteration happened
    /// to dodge the interference. Those two entries cannot share a regression
    /// threshold, and today they do.
    ///
    /// `scripts/bench-history.py` needs a per-benchmark noise scale to size its
    /// band, and the alternative source — the spread of the same benchmark
    /// across past runs — requires several recorded runs before it says
    /// anything, of which there are currently three. `mean/min` is available
    /// from a single boot. See `known-issues.md →
    /// TD-BENCH-COMPARATOR-NEEDS-PER-BENCHMARK-VARIANCE`.
    mean_ns: u64,
    /// Iterations the mean was taken over; a mean over 50 samples and one over
    /// 2000 do not carry the same weight.
    iterations: u32,
}

/// Public view of a scorecard entry for the dashboard API.
#[derive(Clone)]
pub struct ScoreInfo {
    /// Benchmark name.
    pub name: &'static str,
    /// Measured minimum nanoseconds.
    pub measured_ns: u64,
    /// Target nanoseconds from baselines.
    pub target_ns: u64,
    /// Whether the benchmark met its target.
    pub passed: bool,
}

/// Return a snapshot of the current scorecard for external use.
///
/// Returns an empty Vec if benchmarks haven't run yet.
pub fn scorecard_snapshot() -> Vec<ScoreInfo> {
    SCORECARD
        .lock()
        .iter()
        .map(|e| ScoreInfo {
            name: e.name,
            measured_ns: e.measured_ns,
            target_ns: e.target_ns,
            passed: e.passed,
        })
        .collect()
}

/// Global scorecard for collecting benchmark pass/fail results.
///
/// Individual benchmark functions call `score()` to record their result.
/// The scorecard is printed at the end of `run_all()` for quick
/// regression detection.
static SCORECARD: Mutex<alloc::vec::Vec<ScoreEntry>> = Mutex::new(alloc::vec::Vec::new());

/// Record a benchmark result on the global scorecard.
///
/// Call from within benchmark functions after comparing against the target.
/// The scorecard summary is printed at the end of `run_all()`.
/// Express a cycle count as memory accesses, to one decimal place.
///
/// Returns `(whole, tenths)` for printing as `{}.{}`. Plain integer division
/// is not good enough for these diagnostics: a 282-cycle call floor against a
/// 284-cycle access floor is 0.99 accesses and truncates to a flat `0`, which
/// reads as "this costs nothing" in exactly the line whose job is to say where
/// the cost lives. Rounding down to a tenth still understates, but it can no
/// longer round a whole access away to zero.
fn accesses(cycles: u64, floor: u64) -> (u64, u64) {
    if floor == 0 {
        return (0, 0);
    }
    let tenths = cycles.saturating_mul(10) / floor;
    (tenths / 10, tenths % 10)
}

fn score(name: &'static str, result: &BenchResult, target_ns: u64) {
    let passed = result.min_ns <= target_ns;
    SCORECARD.lock().push(ScoreEntry {
        name,
        measured_ns: result.min_ns,
        target_ns,
        passed,
        mean_ns: result.mean_ns,
        iterations: result.iterations,
    });
    // Sampled here, after the lock is released, rather than from a list of
    // hand-placed call sites in `run_all`: hooking the one function every
    // benchmark already calls spreads the samples across the suite
    // automatically and keeps doing so as benchmarks are added or reordered,
    // which a hand-maintained list would not.
    maybe_canary_sample();
}

/// Print the scorecard summary showing which benchmarks met targets.
///
/// Emits two things. First a machine-readable `SCORE` line for **every**
/// entry, passing or not:
///
/// ```text
/// [bench] SCORE <name> <measured_ns> <target_ns> <PASS|OVER> <mean_ns> <iters>
/// ```
///
/// The trailing `<mean_ns> <iters>` are an append-only extension: the parser
/// treats them as optional so logs recorded before they existed still read
/// back. They are not a second performance figure — see [`ScoreEntry::mean_ns`]
/// for why the comparator needs a per-benchmark dispersion number and why the
/// spread across past runs could not supply one.
///
/// `scripts/bench-history.py` parses those, appends them to
/// `bench/history.jsonl`, and diffs the run against the previous boot **on the
/// same host**. That diff, not the target comparison, is the signal that
/// actually means something here: under TCG every guest memory access costs a
/// softmmu lookup of a few hundred host cycles where hardware takes an L1 hit
/// at 1-4, so absolute hardware targets are unreachable by construction and
/// most of this suite reports "ABOVE TARGET" on perfectly correct code. A
/// run-over-run comparison cancels that emulation constant; an absolute target
/// cannot. Passing entries must therefore be printed too — a benchmark that
/// quietly doubles while still beating its target is exactly the regression
/// the failure-only list has always been blind to.
///
/// Second, the human-readable over-target list, kept because it is a useful
/// map of where the emulator is slowest, but explicitly labelled as reference
/// rather than as a verdict.
#[allow(clippy::arithmetic_side_effects)]
fn print_scorecard() {
    let entries = SCORECARD.lock();
    let total = entries.len();
    let passed = entries.iter().filter(|e| e.passed).count();
    let failed = total.saturating_sub(passed);

    // Machine-readable first, so a truncated log still yields a usable record.
    for entry in &*entries {
        serial_println!(
            "[bench] SCORE {} {} {} {} {} {}",
            entry.name,
            entry.measured_ns,
            entry.target_ns,
            if entry.passed { "PASS" } else { "OVER" },
            entry.mean_ns,
            entry.iterations
        );
    }

    serial_println!(
        "[bench] === Scorecard: {}/{} within hardware target ===",
        passed, total
    );

    if failed > 0 {
        serial_println!(
            "[bench] OVER HARDWARE TARGET (reference, not a regression verdict — \
             TCG measurements are 10-400x hardware; compare bench/history.jsonl):"
        );
        for entry in &*entries {
            if !entry.passed {
                let pct = if entry.target_ns > 0 {
                    entry.measured_ns.saturating_mul(100) / entry.target_ns
                } else {
                    0
                };
                serial_println!(
                    "[bench]   {} : {}ns (target {}ns, {}%)",
                    entry.name, entry.measured_ns, entry.target_ns, pct
                );
            }
        }
    }

    if failed == 0 && total > 0 {
        serial_println!("[bench] All benchmarks within target.");
    }
}

// ---------------------------------------------------------------------------
// Standard kernel benchmarks
// ---------------------------------------------------------------------------

/// Interleaved A/B rounds per reference measurement.
const CANARY_ROUNDS: u32 = 500;

/// Stores per timed window. The per-window delta is ~N x one access, so at
/// N=64 a ~200-cycle access shows up as ~13k cycles — an order of magnitude
/// clear of the few-hundred-cycle harness wander.
const CANARY_STORES_PER_WINDOW: u64 = 64;

/// Percent by which the end-of-suite reference may differ from the
/// start-of-suite one before the run is called contaminated.
///
/// Deliberately loose. The honest position is that the run-to-run spread of
/// this measurement has never been quantified, so any tight bound would be a
/// number invented rather than observed — and this project has already been
/// bitten by a benchmark check whose threshold was picked from reasoning
/// instead of data. The raw pair is printed unconditionally, so the threshold
/// can be tightened from real records later without changing what is
/// recorded. Until then it only has to catch the gross case this exists for:
/// `crypto_ed25519_verify` moved 5.1x when the host got busy.
const CANARY_TOLERANCE_PCT: u64 = 25;

/// Take a mid-suite canary sample every Nth scored benchmark.
///
/// 8 gives roughly 8 samples across the current 63-benchmark suite — enough
/// resolution to catch a burst confined to a few benchmarks, without paying
/// the reference measurement's cost 63 times.
const CANARY_SAMPLE_EVERY: u32 = 8;

/// Running extremes of the reference measurement across the suite.
///
/// Extremes rather than a list because the verdict only needs the spread, and
/// two atomics need no allocation and no lock on a path that runs between
/// benchmarks.
static CANARY_MIN: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(u64::MAX);
static CANARY_MAX: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
static CANARY_SAMPLES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
/// Counts `score` calls so every Nth one triggers a sample.
static CANARY_SCORED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Fold one reference measurement into the running extremes.
fn canary_record(measured: u64) {
    CANARY_MIN.fetch_min(measured, Ordering::Relaxed);
    CANARY_MAX.fetch_max(measured, Ordering::Relaxed);
    CANARY_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

/// Sample the reference cost every [`CANARY_SAMPLE_EVERY`] scored benchmarks.
///
/// # Why mid-suite sampling exists
///
/// The first version of this canary measured only the suite's two endpoints,
/// and its first real run showed why that is not enough: it reported the host
/// stable to within 3% while four benchmarks in that same run
/// (`shm_rw_64bytes`, `tcp_checksum_v4`, `net_ipv4_parse`,
/// `net_ethernet_parse`) sat 40-160% above their established values. Endpoint
/// sampling detects a *sustained* load change; the contamination this guards
/// against is a *transient burst* that lands on whichever benchmark is
/// executing at that moment and leaves the rest untouched. An endpoint-only
/// check therefore could not fire on the very case it was built for — which
/// is the failure mode this project keeps rediscovering.
fn maybe_canary_sample() {
    let n = CANARY_SCORED.fetch_add(1, Ordering::Relaxed);
    if n.wrapping_rem(CANARY_SAMPLE_EVERY) == 0 {
        let (measured, _, _) = measure_access_cost();
        canary_record(measured);
    }
}

/// One amplified A/B measurement of a guest memory access, in cycles.
///
/// Returns `(measured, nop, store)` — the per-access cost, and the two raw
/// arm totals behind it.
///
/// Factored into its own function because this same measurement is taken
/// **twice** per suite: once before the benchmarks, to calibrate the budgets,
/// and once after, as the contamination canary. The comparison is only
/// meaningful if both ends measure precisely the same thing, so there is
/// deliberately one implementation and no parameters to let the two drift
/// apart.
fn measure_access_cost() -> (u64, u64, u64) {
    static CALIBRATION_BYTE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
    let n = CANARY_STORES_PER_WINDOW;
    let (nop, store) = ab_interleaved(
        CANARY_ROUNDS,
        || {
            timed(|| {
                let mut i = 0u64;
                while i < n {
                    core::hint::black_box(i);
                    i = i.wrapping_add(1);
                }
            })
        },
        || {
            timed(|| {
                let mut i = 0u64;
                while i < n {
                    CALIBRATION_BYTE.store(core::hint::black_box(1u8), Ordering::Relaxed);
                    i = i.wrapping_add(1);
                }
            })
        },
    );
    (store.saturating_sub(nop) / n, nop, store)
}

/// Re-measure the reference access cost and report whether the host stayed
/// quiet for the whole suite.
///
/// # Why this exists
///
/// The suite runs under QEMU TCG, which is pure emulation and entirely
/// CPU-bound, so any other load on the host scales the measurements. The
/// existing median-ratio drift correction in `scripts/bench-history.py`
/// removes a *uniform* whole-suite factor, but contention from a handful of
/// short commands is not uniform: it lands on whichever benchmark happens to
/// be executing and leaves the rest untouched, which is indistinguishable
/// from a real regression — one or two benchmarks clear of an unchanged
/// median.
///
/// "Remember to keep the machine idle" is not a fix; it already failed once,
/// the same day it was written down. This makes contamination a property the
/// data itself can verify, which is the same principle as the stall
/// detectors: a check that cannot fire is indistinguishable from a check that
/// passes.
///
/// Emits `[bench] CANARY <start> <end> <pct>` where `pct` is `end` as a
/// percentage of `start`, so 100 means the host was equally loaded at both
/// ends. The pair is recorded unconditionally rather than only on failure —
/// a verdict alone would leave no way to ever calibrate the threshold.
fn report_canary(start: u64) {
    let (end, _, _) = measure_access_cost();
    // The endpoints are samples too, so fold them in before reading extremes.
    canary_record(start);
    canary_record(end);

    // Guard the division: a zero start would mean the calibration itself
    // failed, and the run's budgets are already untrustworthy in that case.
    let pct = if start > 0 {
        end.saturating_mul(100) / start
    } else {
        0
    };

    let lo = CANARY_MIN.load(Ordering::Relaxed);
    let hi = CANARY_MAX.load(Ordering::Relaxed);
    let samples = CANARY_SAMPLES.load(Ordering::Relaxed);
    // Spread as a percentage of the quietest sample: the minimum is the best
    // estimate of the uncontended cost, so this reads as "how much slower did
    // the machine get at its worst moment".
    let spread = if lo > 0 && lo != u64::MAX {
        hi.saturating_sub(lo).saturating_mul(100) / lo
    } else {
        0
    };

    // `<start> <end> <pct>` keeps its original meaning; the trailing four are
    // an append-only extension, so the one record written before mid-suite
    // sampling existed still reads back correctly.
    serial_println!(
        "[bench] CANARY {} {} {} {} {} {} {}",
        start, end, pct, lo, hi, spread, samples
    );

    let contaminated = start == 0 || spread > CANARY_TOLERANCE_PCT;
    if contaminated {
        serial_println!(
            "[bench] CONTAMINATED: the reference access cost spread {}% across {} \
             samples during the suite ({}-{} cycles, endpoints {} -> {} = {}%, \
             tolerance {}%). Host load changed mid-run, so a single-benchmark \
             outlier in this run is unproven — do not read it as a regression.",
            spread, samples, lo, hi, start, end, pct, CANARY_TOLERANCE_PCT
        );
    } else {
        serial_println!(
            "[bench] Canary OK: reference access cost stable across {} samples \
             ({}-{} cycles, spread {}%).",
            samples, lo, hi, spread
        );
    }
}

/// Run all standard kernel micro-benchmarks.
///
/// Call after all subsystems are initialized.  Results are printed to
/// serial for comparison against `bench/baselines.toml`.
pub fn run_all() {
    serial_println!("[bench] === Kernel micro-benchmarks ===");
    // Clear scorecard from any previous run.
    SCORECARD.lock().clear();

    // Note: iteration counts are kept modest because these run during
    // boot under QEMU emulation.  For real hardware benchmarks, increase
    // counts 10-50x.

    // --- Calibration: what one guest memory access costs in this harness ---
    //
    // Every threshold below this point is expressed as a multiple of this
    // number rather than as an absolute cycle count, and that is the single
    // most important thing about the micro-benchmarks in this file. The
    // reason is a false alarm that cost four boots to run down.
    //
    // `page_alloc_free_owner_ab` reported that frame-ownership tagging cost
    // 7660-11288 cycles per alloc+free, reproducibly, across five boots. The
    // code it was accusing is a relaxed load, a bounds check, a byte store and
    // a counter bump on each of `set` and `clear` — tens of cycles on
    // hardware. Successive experiments killed every hypothesis that would have
    // made the code guilty: interleaving the arms (ambient load), measuring a
    // relaxed atomic RMW directly (TCG's `cpu_loop_exit_atomic` fallback), and
    // finally splitting `set` into its early-return path and its working path.
    //
    // The split is what explained it, and the answer was not in `frame_owner`
    // at all. `set`'s *work* measured 2416 cycles; a single byte store to an
    // ordinary `.bss` static — this calibration — measured 218. `set` performs
    // roughly half a dozen guest memory accesses (the `ENABLED` load, the
    // length and pointer loads inside `slot`, the tag store, the per-CPU
    // counter's load and store), and 6 x 218 is 1300, the right order. Under
    // TCG *every* guest memory access carries a softmmu lookup costing a few
    // hundred host cycles; on real hardware the same accesses are L1 hits at
    // ~1-4 cycles each. So ownership tagging costs ~10 memory accesses per
    // alloc+free, which is ~30 cycles of real machine and ~2500 cycles of
    // emulator. Nothing regressed. The benchmark was measuring the emulator.
    //
    // An absolute cycle budget therefore cannot work here: it conflates the
    // code under test with an emulation constant that varies with the host,
    // the QEMU build and the accelerator, and it would keep failing forever on
    // code that is correct. Measuring the constant and quoting budgets in
    // units of it makes the check scale-invariant — and it still catches what
    // these budgets exist to catch, because the failures worth catching (an
    // uncached MMIO round-trip, a lock, a per-frame loop that should be one
    // `write_bytes`) cost 10-100x a plain access on *both* hardware and TCG.
    //
    // Measured first so everything below can use it. A store rather than a
    // load, to match the accesses that dominate the paths being judged, and to
    // its own static so nothing else shares the cache line.
    //
    // AMPLIFIED, and that detail is load-bearing. The first version of this
    // block timed *one* store against one empty closure, and it did not
    // survive its first verification boot: it reported `measured=74` with
    // `nop=1278`, while the very next block in the same run measured `nop=448`.
    // The harness's own minimum wanders by several hundred cycles between
    // adjacent measurements, so a single ~200-cycle access has no signal above
    // that noise, and the clamp below silently became the answer — which then
    // under-scaled every budget derived from it and produced a spurious SLOW.
    // Timing N stores in a loop and dividing by N puts the signal N times above
    // the noise floor; the wander stays absolute, so it divides away too.
    //
    // The loop's own overhead (counter increment, compare, branch) is inside
    // the measurement and is NOT subtracted. That is deliberate: it can only
    // make the floor larger, hence every budget below it looser, hence this
    // calibration incapable of manufacturing a false alarm. Erring toward
    // false negatives is the right direction for a check whose entire purpose
    // is to stop crying wolf at correct code.
    //
    // The mechanics live in `measure_access_cost` because this exact
    // measurement is taken a second time at the end of the suite, as the
    // contamination canary (see `report_canary`).
    let (access_floor, canary_start) = {
        let (measured, nop, store) = measure_access_cost();
        // Still clamped: if the two arms somehow land equal the budgets below
        // must not all collapse to 0 and turn every check into a guaranteed
        // failure. With the amplification above this should never bind, and if
        // the printed `measured` is ever at or below it, treat the run's
        // budget-based verdicts as unreliable rather than as findings.
        let floor = core::cmp::max(measured, 100);
        serial_println!(
            "[bench]   memory_access_floor: {} cycles/guest byte-store (measured={} \
             over {} stores/window: nop={} store={}, {} interleaved rounds) — budgets \
             below are multiples of this",
            floor, measured, CANARY_STORES_PER_WINDOW, nop, store, CANARY_ROUNDS
        );
        (floor, measured)
    };

    // --- CPU index lookup (the per-CPU-data primitive under every hot path) ---
    //
    // This is not an interesting operation in itself; it is benchmarked
    // because it is a *multiplier*. `smp::fast_cpu_index` is called twice per
    // frame alloc/free (per-CPU magazine + ownership tag) and twice per heap
    // alloc/free, so its cost is paid several times over inside every
    // benchmark below it, and a regression here shows up as a diffuse slowdown
    // across the whole allocator rather than as an obvious local fault.
    //
    // It regressed exactly that way once: `frame_owner` tagging added a second
    // call per alloc, and on CPU models that advertise neither RDPID nor
    // rdtscp (`qemu64`, this harness) `fast_cpu_index` fell through to an
    // uncached APIC MMIO read. The tier-0 uniprocessor fast path fixed it.
    // Keeping a direct measurement here means the next such regression is
    // visible in one line instead of being inferred from allocator noise.
    // It is measured *against an empty closure*, not against an absolute
    // threshold. Under TCG the harness itself (two serialized TSC reads) costs
    // several hundred cycles, which is far more than the operation, so an
    // absolute number mostly reports emulation overhead: a tier-0 lookup and a
    // tier-3 APIC MMIO read would differ by well under the noise of a fixed
    // nanosecond target. Subtracting a nop measured by the same harness in the
    // same conditions leaves the cost of the lookup itself.
    //
    // The nop and the lookup are interleaved rather than run as two separate
    // 2000-iteration windows. They were two windows to begin with, and across
    // two boots of the *same binary* that reported 0 and then 274 cycles for
    // an operation that is one relaxed load — the spread was ambient load in
    // whichever window drew the busier neighbour, not the code.
    {
        const ROUNDS: u32 = 2000;
        let (nop_cycles, idx_cycles) = ab_interleaved(
            ROUNDS,
            || timed(|| core::hint::black_box(0u64)),
            || timed(|| core::hint::black_box(crate::smp::fast_cpu_index())),
        );

        let cost = idx_cycles.saturating_sub(nop_cycles);
        // An APIC MMIO round-trip under emulation is a *device* access — a TCG
        // exit and an MMIO dispatch, an order of magnitude beyond an ordinary
        // memory access. Tier 0 and tier 1 are a load and a register read, so
        // a healthy lookup is one access or less.
        //
        // The budget was an absolute 200 cycles and reported SLOW on every
        // healthy boot after the tier-0 fix landed (274, 282), because 200 is
        // *below this harness's floor for a single memory access* — the nop
        // baseline alone wanders by more than that between adjacent
        // measurements in one run. It was accusing correct code of the very
        // regression it had just been added to prove was fixed. Four accesses
        // sits clear of the noise while still being far under an MMIO
        // round-trip.
        let mmio_suspicion = access_floor.saturating_mul(4);
        if cost <= mmio_suspicion {
            serial_println!(
                "[bench]   fast_cpu_index: PASS ({} cycles over an empty closure, \
                 limit {} = 4 accesses; nop={} idx={}, {} interleaved rounds)",
                cost, mmio_suspicion, nop_cycles, idx_cycles, ROUNDS
            );
        } else {
            serial_println!(
                "[bench]   fast_cpu_index: SLOW ({} cycles over an empty closure, \
                 limit {} = 4 accesses; nop={} idx={}) — suspect a fallback to the \
                 APIC MMIO path",
                cost, mmio_suspicion, nop_cycles, idx_cycles
            );
        }
    }

    // --- Page allocation (alloc + free cycle) ---
    {
        use crate::mm::frame;
        let result = run("page_alloc_free", 500, || {
            let f = frame::alloc_frame().expect("bench: alloc");
            // SAFETY: frame was just allocated, exclusively ours.
            unsafe { frame::free_frame(f).expect("bench: free"); }
        });

        let target_ns = 1000u64; // From baselines.toml
        score("page_alloc_free", &result, target_ns);
        if result.min_ns <= target_ns {
            serial_println!(
                "[bench]   page_alloc_free: PASS (min {}ns <= target {}ns)",
                result.min_ns, target_ns
            );
        } else {
            serial_println!(
                "[bench]   page_alloc_free: ABOVE TARGET (min {}ns > target {}ns)",
                result.min_ns, target_ns
            );
        }
    }

    // --- Page allocation, A/B against frame-ownership tracking ---
    //
    // `page_alloc_free` above is an *absolute* number, and absolute numbers
    // from this harness are close to unreadable: it runs as a deferred
    // low-priority task on a live system, so the mean and max are dominated by
    // preemption (maxima of ~700M cycles line up with service restarts), and
    // under TCG even `min` carries a large emulation constant that has nothing
    // to do with the code being measured. Comparing such a number against a
    // baseline recorded on a different-sized kernel, months apart, cannot tell
    // you whether a specific change cost anything.
    //
    // This pair can — but only if the two arms are *interleaved*. The first
    // version of this benchmark ran 500 iterations with tagging off, then 500
    // with it on, and reported that tagging cost 10826 cycles per alloc+free.
    // That number was false, and the evidence was in the same output: the off
    // window had max=129078 (nothing perturbed it) while the on window had
    // max=635531436 and a 30x higher mean (a service woke up during it). Two
    // consecutive windows on a live system are not the same system, and `min`
    // does not save you — it is robust to *spikes*, not to a window that is
    // uniformly busier than its neighbour.
    //
    // So the arms alternate per iteration instead (see `ab_interleaved`).
    // Only the `ENABLED` flag differs between them, and it is flipped outside
    // the timed region.
    //
    // Ordering note: tracking is restored to its entry value afterwards, so
    // the experiment cannot leave ownership accounting silently disabled for
    // the rest of the boot.
    {
        use crate::mm::{frame, frame_owner};

        const ROUNDS: u32 = 400;
        let was_enabled = frame_owner::is_enabled();

        // One alloc+free cycle. Both arms call this same function, so they
        // cannot differ by inlining or by TCG translating two distinct blocks.
        let alloc_free = || {
            let f = frame::alloc_frame().expect("bench: alloc");
            // SAFETY: frame was just allocated, exclusively ours.
            unsafe { frame::free_frame(f).expect("bench: free"); }
        };

        let (min_off, min_on) = ab_interleaved(
            ROUNDS,
            || {
                frame_owner::disable();
                timed(alloc_free)
            },
            || {
                frame_owner::enable();
                timed(alloc_free)
            },
        );

        // Restore whatever the system was doing before the experiment.
        if !was_enabled {
            frame_owner::disable();
        }

        // Count what tagging actually adds, in *memory accesses*, because
        // that is the unit this harness can measure (see the calibration
        // block at the top of `run_all`). Per alloc+free:
        //
        //   2 x `is_enabled`            = 2   (one in tag_alloc, one in untag)
        //   `current_owner`             = 2   (fast_cpu_index + the slot load)
        //   `set`  : ENABLED, len, ptr, tag store, counter load+store = 6
        //   `clear`: the same                                          = 6
        //                                                              ----
        //                                                              ~16
        //
        // That is the *architectural* count, and the budget is not 16-ish,
        // because the observed healthy value is consistently 2.5-3x it: 51.7
        // accesses (11288 cycles / 218 floor) on one boot and ~57 on the next.
        // The multiplier is explained, not fudged. `scripts/boot-test.sh` runs
        // a plain `cargo build`, and the workspace's `[profile.dev]` sets only
        // `panic = "abort"` — so opt-level is 0 and the kernel under benchmark
        // is unoptimised. At opt-level 0 nothing is inlined, so each of the
        // half-dozen calls on this path (`is_enabled`, `slot`, `set`, `clear`,
        // `fast_cpu_index`, `bump`) executes a real prologue and epilogue, and
        // every spilled local and saved register on those is another guest
        // memory access that the architectural count above does not include.
        // A ~3x inflation over the source-level access count is exactly what an
        // unoptimised build predicts, and exactly what two independent boots
        // measured.
        //
        // The budget is therefore 150: ~3x the ~50 that healthy code actually
        // costs here. That is deliberately generous, and the generosity is
        // affordable because of what this check is for. It is not a stopwatch —
        // TCG cannot support one (see the calibration block) — it is a
        // structural tripwire, and every failure mode it exists to catch is an
        // order-of-magnitude event, not a percentage: an APIC MMIO round-trip
        // or a contended lock costs 10-100x a plain access, and a per-frame
        // loop where a `write_bytes` belongs scales with `count` and leaves 150
        // behind on the first multi-frame allocation. A tighter budget would
        // not catch anything more; it would only resume failing on correct
        // code, which is precisely how this check spent its first five boots.
        //
        // That previous budget was an absolute 500 cycles and reported SLOW on
        // five consecutive healthy boots (7660-11288). That was not tagging
        // being slow; it was ~16 guest memory accesses at TCG's few-hundred-
        // cycles-each, against a budget sized for real hardware. The whole
        // investigation is written up in known-issues.md under
        // TD-BENCH-OWNER-AB-BUDGET-WAS-AN-ABSOLUTE-CYCLE-COUNT.
        const OWNER_TAG_BUDGET_ACCESSES: u64 = 150; // From baselines.toml
        let budget = access_floor.saturating_mul(OWNER_TAG_BUDGET_ACCESSES);
        let delta = min_on.saturating_sub(min_off);
        let (acc_whole, acc_tenth) = accesses(delta, access_floor);
        if delta <= budget {
            serial_println!(
                "[bench]   page_alloc_free_owner_ab: PASS (tagging costs {} cycles/\
                 alloc+free = {}.{} accesses, limit {} accesses; off={} on={}, {} \
                 interleaved rounds)",
                delta, acc_whole, acc_tenth, OWNER_TAG_BUDGET_ACCESSES,
                min_off, min_on, ROUNDS
            );
        } else {
            serial_println!(
                "[bench]   page_alloc_free_owner_ab: SLOW (tagging costs {} cycles/\
                 alloc+free = {}.{} accesses, limit {} accesses; off={} on={}, {} \
                 interleaved rounds) — suspect an MMIO, a lock, or a per-frame \
                 loop that should be one write_bytes",
                delta, acc_whole, acc_tenth, OWNER_TAG_BUDGET_ACCESSES,
                min_off, min_on, ROUNDS
            );
        }
    }

    // --- Breakdown of `frame_owner::set`, in memory accesses ---
    //
    // Kept as a permanent diagnostic rather than deleted with the
    // investigation that produced it, because it is what makes the A/B above
    // *actionable*. If `page_alloc_free_owner_ab` ever reports SLOW again,
    // this line says immediately whether the added cost is inside `set`'s
    // working path or somewhere else on the allocator's ownership path, which
    // is the fork the original investigation burned four boots failing to
    // resolve by argument.
    //
    // Two numbers, both against the shared `access_floor` calibration:
    //
    //   call_floor  `set` with tracking **off** — the early-return path. Pays
    //               the closure, the call and one relaxed load. This is the
    //               harness's floor for "call into `mm::frame_owner`".
    //   work        `set` with tracking **on**, minus `call_floor`. All shared
    //               overhead cancels, leaving the bounds check, the tag store
    //               and the counter bump: the code the A/B accuses.
    //
    // Measured 278 and 2416 against an `access_floor` of 218 — i.e. `work` is
    // ~11 accesses for a function that performs about half a dozen, which is
    // the right order and is why the verdict was "nothing is wrong with the
    // code, the budget was in the wrong unit". A future `work` of 50+ accesses
    // would mean something genuinely appeared inside `set`.
    //
    // Controlling the `ENABLED` flag is the part an earlier version of this
    // got wrong: without it, "the cost of `set`" silently lumps together the
    // cost of *calling* `set` with the cost of the work it does, and those two
    // point at opposite conclusions.
    //
    // A frame is held for the duration so the index being written is one this
    // benchmark genuinely owns — scribbling a tag onto a frame in use by
    // something else would corrupt the very diagnostic being measured. The
    // `ENABLED` flag is restored to its entry value at the end, so the
    // experiment cannot leave ownership accounting silently off.
    {
        use crate::mm::{frame, frame_owner};

        match frame::alloc_frame() {
            Ok(held) => {
                #[allow(clippy::arithmetic_side_effects)]
                let idx = (held.addr() / frame::FRAME_SIZE as u64) as usize;
                const ROUNDS: u32 = 2000;
                let was_enabled = frame_owner::is_enabled();

                // (1) Early-return path: the harness floor for this call.
                let (nop_off, set_off) = ab_interleaved(
                    ROUNDS,
                    || timed(|| core::hint::black_box(0u64)),
                    || {
                        frame_owner::disable();
                        timed(|| frame_owner::set(idx, frame_owner::Owner::Unknown))
                    },
                );

                // (2) Full path.
                let (nop_on, set_on) = ab_interleaved(
                    ROUNDS,
                    || timed(|| core::hint::black_box(0u64)),
                    || {
                        frame_owner::enable();
                        timed(|| frame_owner::set(idx, frame_owner::Owner::Unknown))
                    },
                );

                if !was_enabled {
                    frame_owner::disable();
                }

                let call_floor = set_off.saturating_sub(nop_off);
                let real_work = set_on.saturating_sub(set_off);
                let (cf_whole, cf_tenth) = accesses(call_floor, access_floor);
                let (rw_whole, rw_tenth) = accesses(real_work, access_floor);
                serial_println!(
                    "[bench]   frame_owner_set_split: call_floor={} cycles ({}.{} \
                     accesses) work={} cycles ({}.{} accesses) (nop_off={} set_off={} \
                     nop_on={} set_on={}, {} interleaved rounds)",
                    call_floor, cf_whole, cf_tenth,
                    real_work, rw_whole, rw_tenth,
                    nop_off, set_off, nop_on, set_on, ROUNDS
                );

                // Restore the tag the allocator gave it, then hand it back.
                frame_owner::set(idx, frame_owner::Owner::Unknown);
                // SAFETY: allocated by this block and not handed out since.
                unsafe {
                    let _ = frame::free_frame(held);
                }
            }
            Err(e) => {
                serial_println!(
                    "[bench]   frame_owner_set_split: SKIPPED (no frame \
                     available: {:?})",
                    e
                );
            }
        }
    }

    // --- Page allocation with zeroing (alloc_zeroed + free cycle) ---
    // This is the standard allocation pattern for page faults, stack
    // growth, and process creation.  Measures alloc + 16 KiB zero + free.
    //
    // The first benchmark runs without the zero pool (cold path).
    // The second benchmark pre-fills the pool to measure the hot path.
    {
        use crate::mm::frame;
        run("page_alloc_zeroed_free", 500, || {
            let f = frame::alloc_frame_zeroed().expect("bench: alloc_zeroed");
            // SAFETY: frame was just allocated, exclusively ours.
            unsafe { frame::free_frame(f).expect("bench: free"); }
        });
    }

    // --- Page allocation from pre-zeroed pool (hot path) ---
    //
    // OPT: When the zero pool is warm (idle CPU has pre-zeroed frames),
    // alloc_frame_zeroed skips the 16 KiB memset entirely.  This
    // benchmark pre-fills the pool to show the best-case latency that
    // page faults see during normal runtime (not boot).
    {
        use crate::mm::frame;

        // Pre-fill the pool to capacity so every benchmark iteration
        // hits the fast path.  refill_zero_pool() fills at most 16
        // frames per call (batch size), so we loop until it returns 0
        // (pool full or no more free frames).  Pool capacity is 256;
        // the benchmark uses ~220 (20 warmup + 200 measured).
        let mut filled = 0usize;
        loop {
            let n = frame::refill_zero_pool();
            if n == 0 { break; }
            filled = filled.saturating_add(n);
        }
        if filled > 0 {
            let result = run("page_alloc_zeroed_pool", 200, || {
                let f = frame::alloc_frame_zeroed().expect("bench: alloc_zeroed");
                // SAFETY: frame was just allocated, exclusively ours.
                unsafe { frame::free_frame(f).expect("bench: free"); }
            });

            let (hits, misses) = frame::zero_pool_stats();
            serial_println!(
                "[bench]   zero_pool: {} hits, {} misses (pool filled: {})",
                hits, misses, frame::zero_pool_count()
            );
            // The pool-warm path should be faster than the cold path
            // (no 16 KiB memset inline).
            let _ = result;
        } else {
            serial_println!("[bench]   page_alloc_zeroed_pool: SKIP (zero pool not enabled)");
        }
    }

    // --- Raw heap alloc + dealloc (64 bytes, no Vec overhead) ---
    //
    // Measures the pure slab allocator round-trip: alloc + dealloc
    // without Vec bookkeeping, Layout construction, or zero-fill.
    // This is the true allocator performance number.
    //
    // Note: measures alloc+free combined.  Per-operation cost is
    // approximately half the reported number (alloc ≈ free cost).
    // The baselines.toml target (200ns) is for a single allocation.
    // Target for alloc+free cycle: 400ns.
    {
        let layout = core::alloc::Layout::from_size_align(64, 8)
            .expect("valid layout");
        let result = run("heap_raw_alloc_free_64", 2000, || {
            // SAFETY: layout is valid, allocator is initialized.
            let ptr = unsafe { alloc::alloc::alloc(layout) };
            debug_assert!(!ptr.is_null(), "bench: alloc returned null");
            core::hint::black_box(ptr);
            // SAFETY: ptr was just allocated with this layout, and
            // is non-null (asserted above, guaranteed by slab cache).
            unsafe { alloc::alloc::dealloc(ptr, layout); }
        });

        // Target is 200ns per single alloc.  This benchmark measures
        // alloc+free, so target is 2× = 400ns for the cycle.
        let target_cycle_ns = 400u64;
        score("heap_alloc_free_64", &result, target_cycle_ns);
        if result.min_ns <= target_cycle_ns {
            serial_println!(
                "[bench]   heap_alloc_free_64: PASS (min {}ns <= alloc+free target {}ns)",
                result.min_ns, target_cycle_ns
            );
        } else {
            serial_println!(
                "[bench]   heap_alloc_free_64: ABOVE TARGET (min {}ns, alloc+free target {}ns, per-op ~{}ns)",
                result.min_ns, target_cycle_ns, result.min_ns / 2
            );
        }
    }

    // --- Raw heap alloc + dealloc (512 bytes) ---
    {
        let layout = core::alloc::Layout::from_size_align(512, 8)
            .expect("valid layout");
        run("heap_raw_alloc_free_512", 2000, || {
            // SAFETY: layout is valid, allocator is initialized.
            let ptr = unsafe { alloc::alloc::alloc(layout) };
            debug_assert!(!ptr.is_null(), "bench: alloc returned null");
            core::hint::black_box(ptr);
            // SAFETY: ptr was just allocated with this layout and is non-null.
            unsafe { alloc::alloc::dealloc(ptr, layout); }
        });
    }

    // --- Raw heap alloc + dealloc (4096 bytes) ---
    {
        let layout = core::alloc::Layout::from_size_align(4096, 8)
            .expect("valid layout");
        run("heap_raw_alloc_free_4096", 500, || {
            // SAFETY: layout is valid, allocator is initialized.
            let ptr = unsafe { alloc::alloc::alloc(layout) };
            debug_assert!(!ptr.is_null(), "bench: alloc returned null");
            core::hint::black_box(ptr);
            // SAFETY: ptr was just allocated with this layout and is non-null.
            unsafe { alloc::alloc::dealloc(ptr, layout); }
        });
    }

    // --- Page compression (zero page) ---
    {
        use alloc::vec;
        use crate::mm::compress;
        let data = vec![0u8; 16384];
        run("compress_zero_page", 200, || {
            let result = compress::compress(&data);
            core::hint::black_box(&result);
        });
    }

    // --- Page compression (repeating pattern) ---
    {
        use alloc::vec;
        use crate::mm::compress;
        let mut data = vec![0u8; 16384];
        for (i, b) in data.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            { *b = (i & 0xFF) as u8; }
        }
        run("compress_repeating", 200, || {
            let result = compress::compress(&data);
            core::hint::black_box(&result);
        });
    }

    // --- TSC read overhead ---
    {
        run("rdtsc_overhead", 5000, || {
            let _ = core::hint::black_box(rdtsc());
        });
    }

    // --- HPET read overhead ---
    //
    // Measures the cost of reading the HPET main counter register
    // via MMIO.  This is the overhead for every hpet::elapsed_ns()
    // call, which SYS_CLOCK_MONOTONIC should use.
    if crate::hpet::is_available() {
        run("hpet_read", 5000, || {
            let _ = core::hint::black_box(crate::hpet::read_counter());
        });
    }

    // --- Context switch (yield to another task and back) ---
    //
    // Measures the round-trip time: current task → other task → back.
    // We spawn a "ping" task that immediately yields on each wakeup,
    // so the measured time is two context switches (there + back).
    //
    // Target from baselines.toml: < 5 µs per switch (Linux: 1-3 µs).
    // Divide the result by 2 to get per-switch cost.
    bench_context_switch();

    // --- Scheduler pick_next (O(1) bitmap scan) ---
    bench_pick_next();

    // --- Scheduler pick_next, ISOLATED + depth-scaling (verifies O(1)) ---
    //
    // The integrated bench above folds pick_next into a full yield (two
    // context switches).  This one drives the run queue directly at
    // depths 1..1024 to prove the pick cost stays flat as the number of
    // runnable tasks grows — the property CLAUDE.md requires ("must be
    // O(1)... never O(n) over all tasks").
    bench_pick_next_scaling();

    // --- Syscall dispatch (kernel-side only) ---
    //
    // Measures the dispatch function for SYS_TASK_ID (trivial syscall
    // that just reads the current task ID).  This excludes the
    // user↔kernel ring transition but measures the handler lookup,
    // dispatch, and result packing.
    //
    // Target from baselines.toml: < 200 ns (Linux getpid: ~100 ns
    // including ring transition — our dispatch-only should be faster).
    bench_syscall_dispatch();

    // --- IPC channel send+recv round-trip ---
    //
    // Measures sending a small message through a channel and receiving
    // it.  This is the primary IPC mechanism and the hot path for all
    // inter-process communication.
    //
    // Target from baselines.toml: < 2 µs round-trip (Fuchsia: ~1.5 µs,
    // L4: ~0.5-1 µs).
    bench_ipc_channel();

    // --- Large (64 KiB) channel round-trip ---
    //
    // Baseline-only: establishes the cost of copying a maximum-size
    // (MAX_MESSAGE_SIZE) payload through the channel today, so a future
    // zero-copy page-flipping optimization can be measured against it.
    bench_ipc_channel_large();

    // --- Sync (rendezvous) channel round-trip ---
    //
    // Measures the L4/seL4-style synchronous IPC path: send_blocking
    // parks a message, receiver takes it directly from the rendezvous
    // slot.  Requires a context switch each direction, so this
    // measures IPC + context switch combined.
    bench_ipc_channel_sync();

    // --- Pipe write+read round-trip ---
    //
    // Measures the kernel-side pipe hot path: write N bytes on the
    // write end, read them back from the read end.
    bench_ipc_pipe();

    // --- Service registry connect+accept ---
    //
    // Measures the service discovery path: connect() creates a channel
    // pair, queues one end, and returns the other.  accept() dequeues.
    bench_service_connect();

    // --- Eventfd signal+read round-trip ---
    //
    // Measures lightweight wake-up notification cost: write (signal)
    // then try_read (consume).
    bench_ipc_eventfd();

    // --- Semaphore signal+wait round-trip ---
    //
    // Measures counting semaphore overhead: signal() increments the
    // counter, try_wait() decrements it.  Both are uncontended so
    // this captures the lock acquisition + counter update cost.
    bench_ipc_semaphore();

    // --- Futex wake (uncontended) ---
    //
    // Measures the cost of futex_wake when nobody is waiting.  This
    // is the fast path for userspace mutexes: unlock does an atomic
    // store + futex_wake(1), which scans the empty wait list and
    // returns immediately.
    bench_ipc_futex();

    // --- Shared memory create+close cycle ---
    //
    // Measures the overhead of creating and destroying a shared memory
    // region (single 16 KiB frame).  This captures handle allocation,
    // frame allocation, and cleanup.
    bench_ipc_shm();

    // --- Completion port try_wait (no events) ---
    //
    // Measures the cost of polling an empty completion port.  This is
    // the fast path for event-driven servers: check for events, get
    // none, go back to work.
    bench_ipc_completion_port();

    // --- io_ring NOP submission throughput ---
    //
    // Measures the per-SQE overhead for the io_ring submission path.
    // This is the critical fast path for high-throughput async I/O.
    //
    // Target from baselines.toml: < 200 ns per SQE (Linux io_uring:
    // 100-200 ns per SQE submission).
    bench_io_ring_nop();

    // --- Page fault (demand-page anonymous fault) ---
    //
    // Measures the page fault handler's resolution path for a demand-
    // paged anonymous page.  Includes frame allocation, zeroing, page
    // table update, and TLB flush.
    //
    // Target from baselines.toml: < 10 µs (Linux: ~2-5 µs).
    bench_page_fault();

    // NOTE: bench_isr_latency() moved to end of sequence because it
    // crashes under QEMU (page fault at near-null struct offset → double
    // fault).  All benchmarks after the crash never run, so ISR goes last
    // to avoid blocking the rest of the scorecard.  See todo.txt
    // "Cross-Zone Bug Reports" for details.

    // --- VFS benchmarks (fs zone) ---
    bench_vfs_stat();
    // Runs immediately after vfs_stat so it sees the same dcache occupancy
    // that vfs_stat was measured against.
    // Before the VFS breakdown: it interprets its stages in terms of lock
    // cost, so the lock cost had better be measured first.
    bench_lock_primitives();
    bench_vfs_stat_breakdown();
    bench_vfs_read_write();
    bench_vfs_readdir();

    // --- Network benchmarks (net zone) ---
    bench_net_ipv4_parse();
    bench_net_ethernet_parse();
    bench_net_arp_lookup();
    bench_net_checksum();
    bench_net_tcp_checksum_v4();
    bench_net_tcp_checksum_v6();
    bench_net_ipv6_parse();
    bench_net_firewall_check();
    bench_net_dns_build_query();
    bench_net_tcp_conn_lookup();

    // --- Veth and per-namespace network benchmarks ---
    // These require veth::init() and netns::init() to have completed,
    // which they have by the time run_all() executes during boot.
    bench_net_veth_send();
    bench_net_veth_recv();
    bench_net_veth_roundtrip();
    bench_net_ns_arp_lookup();

    // --- Cryptographic primitives ---
    bench_crypto_sha256_64();
    bench_crypto_sha256_1k();
    bench_crypto_sha512_64();
    bench_crypto_hmac_sha256();
    bench_crypto_chacha20_1k();
    bench_crypto_poly1305_1k();
    bench_crypto_chacha20_poly1305_1k();
    bench_crypto_x25519();
    bench_crypto_ed25519_sign();
    bench_crypto_ed25519_verify();

    // --- VFS deep-path and throughput benchmarks ---
    bench_vfs_stat_deep();
    bench_vfs_stat_3comp();
    bench_vfs_throughput_16k();

    // --- HTTP server benchmarks ---
    bench_http_parse_request();
    bench_http_mime_type();
    bench_http_percent_decode();
    bench_http_etag();
    bench_http_build_response();
    bench_http_build_response_gzip();
    bench_http_gzip_1k();
    bench_http_gzip_8k();

    // --- Dashboard API benchmarks ---
    bench_dashboard_api_status();
    bench_dashboard_api_health();
    bench_dashboard_api_metrics();

    // --- ISR latency (timer interrupt hard-IRQ phase) ---
    //
    // Measures the time interrupts are disabled during the timer ISR:
    // entry → tick counter increment → scheduler timer_tick → EOI.
    // This is the hard-IRQ phase that blocks device interrupts.
    //
    // WARNING: This benchmark crashes under QEMU (page fault → double fault).
    // It runs LAST so all other benchmarks get measured even if ISR crashes.
    // See todo.txt "Cross-Zone Bug Reports" for details (kernel-core zone bug).
    //
    // Target from baselines.toml: < 10 µs (37000 cycles).
    bench_isr_latency();

    // --- Contamination canary ---
    //
    // Re-measure the same reference the budgets were calibrated against. Runs
    // *before* the scorecard so the verdict is already on the log next to the
    // SCORE lines it qualifies.
    report_canary(canary_start);

    // --- Print scorecard summary ---
    print_scorecard();

    serial_println!("[bench] === Benchmarks complete ===");
}

/// Benchmark context switch round-trip.
///
/// The boot thread (idle task, priority 0) always wins `pick_next` on
/// yield, so we can't measure context switches from it.  Instead, we
/// spawn two tasks at equal priority: a "driver" that measures
/// yield_now latency, and a "helper" that yields in a tight loop.
/// Round-robin scheduling alternates them, giving us the true
/// context-switch round-trip cost.
///
/// The driver task records measurements into a shared static; the boot
/// thread waits for it to finish, then reports results.
fn bench_context_switch() {
    use crate::sched;
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const BENCH_ITERS: u32 = 200;
    const BENCH_PRIO: u8 = 16;

    static BENCH_EXIT: AtomicBool = AtomicBool::new(false);
    static RESULT_MIN: AtomicU64 = AtomicU64::new(u64::MAX);
    static RESULT_MEAN: AtomicU64 = AtomicU64::new(0);
    static RESULT_MAX: AtomicU64 = AtomicU64::new(0);
    static DRIVER_DONE: AtomicBool = AtomicBool::new(false);

    extern "C" fn bench_yield_loop(_arg: u64) {
        while !BENCH_EXIT.load(Ordering::Relaxed) {
            sched::yield_now();
        }
    }

    extern "C" fn bench_driver(_arg: u64) {
        // Warmup.
        for _ in 0..20 {
            sched::yield_now();
        }

        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut total = 0u64;

        for _ in 0..BENCH_ITERS {
            let start = crate::bench::rdtsc_serialized();
            sched::yield_now(); // → helper → back
            let end = crate::bench::rdtsc();
            let elapsed = end.saturating_sub(start);
            if elapsed < min { min = elapsed; }
            if elapsed > max { max = elapsed; }
            total = total.saturating_add(elapsed);
        }

        let mean = total.checked_div(u64::from(BENCH_ITERS)).unwrap_or(0);
        RESULT_MIN.store(min, Ordering::Release);
        RESULT_MEAN.store(mean, Ordering::Release);
        RESULT_MAX.store(max, Ordering::Release);

        // Signal the helper to exit.
        BENCH_EXIT.store(true, Ordering::Release);
        sched::yield_now(); // Let helper see exit flag.

        DRIVER_DONE.store(true, Ordering::Release);
    }

    // Reset state.
    BENCH_EXIT.store(false, Ordering::Release);
    DRIVER_DONE.store(false, Ordering::Release);
    RESULT_MIN.store(u64::MAX, Ordering::Relaxed);

    // Spawn helper and driver at equal priority for round-robin.
    let helper_id = match sched::spawn(b"bench-hlp", BENCH_PRIO, bench_yield_loop, 0, 0) {
        Ok(id) => id,
        Err(e) => {
            serial_println!("[bench] context_switch: SKIP (spawn failed: {:?})", e);
            return;
        }
    };
    let driver_id = match sched::spawn(b"bench-drv", BENCH_PRIO, bench_driver, 0, 0) {
        Ok(id) => id,
        Err(_) => {
            sched::kill_task(helper_id);
            serial_println!("[bench] context_switch: SKIP (driver spawn failed)");
            return;
        }
    };

    // Wait for the driver to complete.  The boot thread (priority 0)
    // yields, letting the benchmark tasks run.  Timer preemption also
    // gives them CPU time.
    for _ in 0..5000u32 {
        if DRIVER_DONE.load(Ordering::Acquire) {
            break;
        }
        sched::yield_now();
    }

    if !DRIVER_DONE.load(Ordering::Acquire) {
        serial_println!("[bench] context_switch: TIMEOUT (driver didn't finish)");
        sched::kill_task(helper_id);
        sched::kill_task(driver_id);
        sched::reap_dead_tasks();
        return;
    }

    let min = RESULT_MIN.load(Ordering::Acquire);
    let mean = RESULT_MEAN.load(Ordering::Acquire);
    let max = RESULT_MAX.load(Ordering::Acquire);
    let min_ns = cycles_to_ns(min);
    let mean_ns = cycles_to_ns(mean);

    // Each yield is a round-trip (2 context switches).
    let per_switch_ns = min_ns / 2;

    serial_println!(
        "[bench] context_switch_rt: min={} cycles ({}ns), mean={} cycles ({}ns), max={} cycles  [{} iters]",
        min, min_ns, mean, mean_ns, max, BENCH_ITERS
    );
    serial_println!(
        "[bench]   per-switch estimate: {}ns (target: <5000ns)",
        per_switch_ns
    );

    let target_ns = 5000u64;
    // Build a pseudo-BenchResult for the scorecard using per-switch estimate.
    let ctx_result = BenchResult {
        name: String::from("context_switch"),
        min_cycles: min / 2,
        mean_cycles: mean / 2,
        max_cycles: max / 2,
        min_ns: per_switch_ns,
        mean_ns: cycles_to_ns(mean / 2),
        iterations: BENCH_ITERS,
    };
    score("context_switch", &ctx_result, target_ns);
    if per_switch_ns <= target_ns {
        serial_println!(
            "[bench]   context_switch: PASS ({}ns <= {}ns)",
            per_switch_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   context_switch: ABOVE TARGET ({}ns > {}ns)",
            per_switch_ns, target_ns
        );
    }

    // Clean up.
    sched::kill_task(helper_id);
    sched::kill_task(driver_id);
    sched::reap_dead_tasks();
}

/// Benchmark the scheduler's `pick_next` operation.
///
/// Measures how long it takes the scheduler to scan the bitmap and
/// find the highest-priority ready task.  This should be O(1) via
/// `trailing_zeros()` instruction on the priority bitmap.
fn bench_pick_next() {
    use crate::sched;

    // Spawn several tasks at different priorities to populate the
    // run queues, then measure yield_now (which includes pick_next).
    let mut task_ids = [0u64; 4];
    for (i, id) in task_ids.iter_mut().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let prio = 8 + (i as u8) * 4; // priorities 8, 12, 16, 20
        match sched::spawn(b"bench-pn", prio, bench_nop_task, 0, 0) {
            Ok(tid) => *id = tid,
            Err(_) => {
                serial_println!("[bench] pick_next: SKIP (spawn failed)");
                return;
            }
        }
    }

    // Measure yield with multiple tasks in the run queue.
    let result = run("sched_pick_next_4tasks", 500, || {
        sched::yield_now();
    });

    // The pick_next portion of yield_now is a small fraction of the
    // total context switch cost.  We report it for tracking.
    serial_println!(
        "[bench]   pick_next overhead included in context switch"
    );
    // Target: same order as context switch round-trip (yield = 2 switches).
    score("pick_next", &result, 10000);

    // Clean up.
    for id in task_ids {
        if id != 0 {
            sched::kill_task(id);
        }
    }
    sched::reap_dead_tasks();
}

/// Trivial benchmark helper task: runs one iteration then exits.
extern "C" fn bench_nop_task(_arg: u64) {
    crate::sched::yield_now();
    // Exit after one yield.
}

/// Benchmark scheduler `pick_next_task` in **isolation**, across
/// increasing run-queue depths, to empirically verify its O(1) claim.
///
/// `bench_pick_next` above measures pick_next *inside* a full `yield_now`
/// (two context switches, register save/restore, address-space reload),
/// so it can neither isolate the pick cost nor reveal how it scales with
/// the number of runnable tasks.  Here we drive a *local*
/// `PriorityRoundRobin` directly: fill it with N synthetic tasks, then
/// measure one steady-state round-robin rotation per iteration —
/// `pick_next` (bitmap `trailing_zeros` + `pop_front`) followed by
/// `enqueue` (`push_back` + bit-set), exactly what a running task's
/// preemption does.  The queue depth is held constant across the whole
/// measured loop (each pick is immediately re-enqueued), so if pick_next
/// were secretly O(N) the per-op latency would climb with N.
///
/// All N tasks share one priority level: that is the worst case for a
/// per-priority FIFO (a single queue holds everything), so a hidden
/// linear scan in queue depth would surface here rather than being
/// masked by the 32-way bitmap fan-out.
fn bench_pick_next_scaling() {
    use crate::sched::priority_rr::PriorityRoundRobin;

    // Mid priority; the specific level is irrelevant to the O(1) claim.
    const PRIO: u8 = 16;
    const DEPTHS: [u32; 5] = [1, 8, 64, 256, 1024];

    let mut shallow_ns = 0u64;
    let mut deepest = None;

    for (i, &depth) in DEPTHS.iter().enumerate() {
        let mut rq = PriorityRoundRobin::new();
        for id in 1..=u64::from(depth) {
            rq.enqueue(id, PRIO);
        }

        // Steady-state rotation keeps `depth` tasks queued throughout.
        let result = run("sched_pick_next_isolated", 2000, || {
            if let Some(id) = rq.pick_next() {
                rq.enqueue(id, PRIO);
                core::hint::black_box(id);
            }
        });
        serial_println!(
            "[bench]   pick_next depth={:>4}: min={}ns mean={}ns",
            depth, result.min_ns, result.mean_ns
        );

        if i == 0 {
            shallow_ns = result.min_ns;
        }
        deepest = Some(result);
    }

    let Some(deepest) = deepest else { return };

    // O(1) verdict: the 1024-deep pick must not be materially slower than
    // the 1-deep pick.  A truly linear scan would be ~1000x here; we flag
    // anything past 4x (generous headroom for cache effects and the
    // coarse rdtsc/rounding noise that dominates at single-digit ns).
    let ratio_x100 = deepest
        .min_ns
        .saturating_mul(100)
        .checked_div(shallow_ns.max(1))
        .unwrap_or(0);
    if deepest.min_ns <= shallow_ns.saturating_mul(4).max(shallow_ns.saturating_add(30)) {
        serial_println!(
            "[bench]   pick_next O(1) CONFIRMED: depth 1->1024 is {}.{:02}x (flat)",
            ratio_x100 / 100, ratio_x100 % 100
        );
    } else {
        serial_println!(
            "[bench]   pick_next WARNING: depth 1->1024 scaled {}.{:02}x — not O(1)!",
            ratio_x100 / 100, ratio_x100 % 100
        );
    }

    // Score the deepest-depth isolated rotation.  On real hardware the
    // pick+enqueue is single-digit ns, but under QEMU/TCG the `run()`
    // harness pays one CPUID-serialized `rdtsc` per iteration (~900-950ns
    // — the same floor `hpet_read` sees), which entirely dominates the
    // measurement.  So the absolute number is a TCG floor artifact and
    // the real regression signal is the O(1) *ratio* above; the target
    // here is just set above that floor so a genuine O(n) blow-up (a
    // linear scan of 1024 queued tasks would add microseconds) still
    // trips it, without false-alarming on the constant overhead.
    score("sched_pick_next", &deepest, 1500);
}

// ---------------------------------------------------------------------------
// Syscall dispatch benchmark
// ---------------------------------------------------------------------------

/// Benchmark kernel-side syscall dispatch for a trivial syscall.
///
/// Measures the cost of looking up and executing SYS_TASK_ID (which just
/// returns the current task ID — minimal work).  This is the kernel-side
/// dispatch overhead, excluding the user↔kernel ring transition.
fn bench_syscall_dispatch() {
    use crate::syscall::dispatch::{dispatch, SyscallArgs};
    use crate::syscall::number::SYS_TASK_ID;

    let args = SyscallArgs {
        arg0: SYS_TASK_ID,
        arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };

    let result = run("syscall_dispatch_task_id", 2000, || {
        let r = dispatch(SYS_TASK_ID, &args);
        core::hint::black_box(r);
    });

    // Target: < 200 ns.  Linux getpid is ~100 ns INCLUDING ring
    // transition — dispatch-only should be well under that.
    let target_ns = 200u64;
    score("syscall_dispatch", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   syscall_dispatch: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   syscall_dispatch: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }

    bench_syscall_dispatch_breakdown(&result);
}

/// Decompose `dispatch()` into the work it does *besides* running the handler.
///
/// `syscall_dispatch` has sat at ~3.5x its 200 ns target across every boot on
/// record, and the lockdep O(1) fix — which cut `crate::sync::Mutex`
/// acquire+release from 632 ns to 274 ns and `vfs_stat_root` by 44% — moved it
/// by exactly 0 ns.  That is a real finding, not a null result: it says the
/// cost is somewhere `crate::sync::Mutex` is not.
///
/// Rather than infer which of dispatch's five prologue/epilogue stages that is,
/// each is measured **directly, in isolation**, and the residual is printed as
/// an explicit `unexplained` term.  This project has twice reasoned about a hot
/// path from the source alone and been wrong (the `pick_next` benchmark measured
/// a full yield; the `sched_pick_next` anchor came from a benchmark that took no
/// lock at all), so the source is used here only to decide *what* to measure,
/// never to decide *how much* something costs.
fn bench_syscall_dispatch_breakdown(dispatch_result: &BenchResult) {
    use crate::syscall::dispatch::SyscallArgs;
    use crate::syscall::handlers::sys_task_id;
    use crate::syscall::number::SYS_TASK_ID;

    let args = SyscallArgs {
        arg0: SYS_TASK_ID,
        arg1: 0, arg2: 0, arg3: 0, arg4: 0, arg5: 0,
    };
    let tid = crate::sched::current_task_id();

    // Stage 1: the handler itself — the only work a caller actually asked for.
    // Everything else this function measures is overhead by definition.
    let handler = run("sd_handler", 2000, || {
        core::hint::black_box(sys_task_id(&args));
    });

    // Stage 2: `sched::current_task_id()`.  Dispatch calls it once directly, and
    // each `ktrace::record` calls it again — so it is charged three times per
    // syscall, and is measured alone to price that.
    let task_id = run("sd_current_task_id", 2000, || {
        core::hint::black_box(crate::sched::current_task_id());
    });

    // Stage 3: the syscall filter (seccomp equivalent).  Its doc comment claims
    // "O(1)" and "~5 ns"; two bullets later the same comment says "linear scan
    // miss".  Both cannot be true.  Measure it.
    let scfilter = run("sd_scfilter_check", 2000, || {
        core::hint::black_box(crate::scfilter::check(tid, SYS_TASK_ID));
    });

    // Stage 4: the two `ktrace::record` calls (enter + exit).  Measured as the
    // pair, because that is how dispatch pays for them.
    let ktrace = run("sd_ktrace_pair", 2000, || {
        crate::ktrace::record(
            crate::ktrace::Category::Syscall,
            crate::ktrace::event::SYSCALL_ENTER,
            SYS_TASK_ID,
            0,
        );
        crate::ktrace::record(
            crate::ktrace::Category::Syscall,
            crate::ktrace::event::SYSCALL_EXIT,
            SYS_TASK_ID,
            0,
        );
    });

    // Stage 5: the syscall-latency histogram, enter+exit as a pair.
    let sclatency = run("sd_sclatency_pair", 2000, || {
        let s = crate::sclatency::enter();
        crate::sclatency::exit(s, SYS_TASK_ID);
    });

    let accounted = handler.min_ns
        .saturating_add(task_id.min_ns)
        .saturating_add(scfilter.min_ns)
        .saturating_add(ktrace.min_ns)
        .saturating_add(sclatency.min_ns);
    let total = dispatch_result.min_ns;

    serial_println!(
        "[bench]   syscall_dispatch breakdown: total {}ns = handler {}ns + task_id {}ns \
         + scfilter {}ns + ktrace_pair {}ns + sclatency_pair {}ns + unexplained {}ns",
        total,
        handler.min_ns,
        task_id.min_ns,
        scfilter.min_ns,
        ktrace.min_ns,
        sclatency.min_ns,
        total.saturating_sub(accounted),
    );
    // How many filters exist decides *which* path `sd_scfilter_check` above
    // just measured, so print it rather than leaving the reading to inference.
    // With 0 installed, `check` returns after a single atomic load and the
    // number above is the fast path; with any installed it is the locked
    // hash-lookup path, and the two are not comparable.
    //
    // This line used to end "with 0 installed every call still walks all N
    // slots", which is how the O(n) scan was found: it made the benchmark
    // state the thing that was wrong with it.  That is no longer true — the
    // scan is gone — so the sentence goes with it, because a stale
    // explanation next to a live number is worse than no explanation.
    let installed = crate::scfilter::active_count();
    serial_println!(
        "[bench]   syscall_dispatch breakdown: scfilter has {} filter(s) installed of {} \
         slots — measured path: {}",
        installed,
        crate::scfilter::MAX_FILTERS,
        if installed == 0 { "lock-free fast path (1 atomic load)" } else { "locked O(1) hash lookup" },
    );

    // ---- Coherence gate --------------------------------------------------
    //
    // The stage sum above is only usable if the parts were measured under the
    // same conditions as the whole.  Under TCG they need not be: the VFS
    // breakdown once printed a part *larger* than its whole, and two
    // byte-identical benchmarks in one boot disagreed 1.67x.  So re-measure the
    // whole at the end of the block and report the drift; and flag it when the
    // parts do not fit inside the whole, because "unexplained" computed with
    // `saturating_sub` renders that case as a comfortable 0.
    let total_again = run("sd_dispatch_again", 2000, || {
        core::hint::black_box(crate::syscall::dispatch::dispatch(SYS_TASK_ID, &args));
    });
    let (lo, hi) = if total <= total_again.min_ns {
        (total, total_again.min_ns)
    } else {
        (total_again.min_ns, total)
    };
    let drift_pct = if lo == 0 { 0 } else { (hi.saturating_sub(lo)).saturating_mul(100) / lo };
    const DRIFT_LIMIT_PCT: u64 = 25;
    serial_println!(
        "[bench]   syscall_dispatch breakdown: drift check — dispatch twice: {}ns then {}ns ({}%)",
        total, total_again.min_ns, drift_pct
    );
    if drift_pct > DRIFT_LIMIT_PCT {
        serial_println!(
            "[bench]   WARNING: syscall_dispatch breakdown is NOT internally coherent \
             ({}% drift > {}% limit) — the stage split above is measurement drift and \
             must not be used to attribute cost",
            drift_pct, DRIFT_LIMIT_PCT
        );
    }
    if accounted > total {
        serial_println!(
            "[bench]   WARNING: syscall_dispatch stages sum to {}ns but the whole measured \
             {}ns — the parts do not fit in the whole, so the split is noise, not attribution",
            accounted, total
        );
    }
}

// ---------------------------------------------------------------------------
// IPC channel benchmark
// ---------------------------------------------------------------------------

/// Benchmark IPC channel send + recv round-trip.
///
/// Creates a channel pair, sends a small message on one end, and receives
/// it on the other.  Measures the kernel-side IPC hot path.
fn bench_ipc_channel() {
    use crate::ipc::channel::{self, Message};

    let (tx, rx) = channel::create();

    // Warm up: send/recv once so caches are primed.
    {
        let msg = Message::from_bytes(b"warmup")
            .expect("bench: create warmup msg");
        channel::send(tx, msg).expect("bench: warmup send");
        let _ = channel::try_recv(rx).expect("bench: warmup recv");
    }

    let result = run("ipc_channel_roundtrip", 1000, || {
        let msg = Message::from_bytes(b"bench")
            .expect("bench: create msg");
        channel::send(tx, msg).expect("bench: send");
        let received = channel::try_recv(rx).expect("bench: recv");
        core::hint::black_box(received);
    });

    channel::close(tx);
    channel::close(rx);

    // Target: < 2 µs round-trip (Fuchsia: ~1.5 µs, L4: ~0.5-1 µs).
    let target_ns = 2000u64;
    score("ipc_channel", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   ipc_channel_roundtrip: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   ipc_channel_roundtrip: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Large-message IPC channel benchmark
// ---------------------------------------------------------------------------

/// Benchmark a *large* (64 KiB) channel send + recv round-trip.
///
/// The small-message [`bench_ipc_channel`] is dominated by fixed per-call
/// overhead (queue ops, lock, wakeup) and barely touches the payload.  This
/// variant uses a `MAX_MESSAGE_SIZE`-byte message so the cost is dominated by
/// the per-byte data handling instead: `Message::from_bytes` copies the slice
/// into a heap `Vec`, and (today) the syscall boundary copies it once more in
/// each direction.  It establishes the baseline that a future zero-copy
/// page-flipping large-message path (channel.rs module docs, roadmap §2.3 IPC)
/// would improve — you can't tell whether that optimization helps without a
/// number for the copy-based path it replaces.
///
/// Note: this measures the *kernel-internal* path only (no userspace copy),
/// where the payload `Vec` is moved through the queue rather than re-copied,
/// so the dominant term here is the single `from_bytes` allocation+copy of the
/// 64 KiB buffer plus the queue/wakeup overhead — i.e. the per-message cost a
/// real sender pays to marshal a large message.
fn bench_ipc_channel_large() {
    use crate::ipc::channel::{self, Message};

    // MAX_MESSAGE_SIZE is private to the channel module; mirror its 64 KiB
    // value here (a compile-time check in channel.rs guards the constant).
    const LARGE: usize = 64 * 1024;

    let (tx, rx) = channel::create();
    let payload = alloc::vec![0xABu8; LARGE];

    // Warm up so the allocator free-list and caches are primed.
    if let Ok(msg) = Message::from_bytes(&payload) {
        if channel::send(tx, msg).is_ok() {
            let _ = channel::try_recv(rx);
        }
    }

    let result = run("ipc_channel_roundtrip_64k", 500, || {
        if let Ok(msg) = Message::from_bytes(&payload) {
            if channel::send(tx, msg).is_ok() {
                if let Ok(received) = channel::try_recv(rx) {
                    core::hint::black_box(received);
                }
            }
        }
    });

    channel::close(tx);
    channel::close(rx);

    // No hard latency target: this is a baseline for the data-handling cost,
    // not a pass/fail gate (the small-message round-trip carries the < 2 µs
    // hot-path target).  Deliberately NOT added to the scorecard — a target of
    // 0 would always register as a failure and skew the pass/fail summary.
    // Report min/mean for regression tracking instead; a future zero-copy path
    // should drive this well below the copy-bound number.
    serial_println!(
        "[bench]   ipc_channel_roundtrip_64k: baseline min {}ns mean {}ns (64 KiB payload)",
        result.min_ns, result.mean_ns
    );
}

// ---------------------------------------------------------------------------
// Sync (rendezvous) channel benchmark
// ---------------------------------------------------------------------------

/// Benchmark synchronous (rendezvous) channel round-trip.
///
/// Creates a sync channel pair, spawns a receiver task that loops
/// calling `recv()`, and the driver task loops calling
/// `send_blocking()`.  Each send parks the message and blocks until
/// the receiver takes it, so this measures IPC + 2 context switches
/// per iteration (sender→receiver→sender).
///
/// This is the L4/seL4-style zero-copy IPC path (minus the actual
/// zero-copy optimization, which is not yet implemented).
fn bench_ipc_channel_sync() {
    use crate::ipc::channel::{self, Message};
    use crate::sched;
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const ITERS: u32 = 500;
    const RECV_PRIO: u8 = 8;
    const DRIVER_PRIO: u8 = 8;

    static SYNC_MIN: AtomicU64 = AtomicU64::new(u64::MAX);
    static SYNC_MEAN: AtomicU64 = AtomicU64::new(0);
    static SYNC_MAX: AtomicU64 = AtomicU64::new(0);
    static SYNC_DONE: AtomicBool = AtomicBool::new(false);
    static SYNC_EXIT: AtomicBool = AtomicBool::new(false);

    // The receiver handle is passed via a static.  We use a raw u64
    // because ChannelHandle isn't Sync (interior mutability isn't needed,
    // but it doesn't implement Sync).
    static RX_RAW: AtomicU64 = AtomicU64::new(0);
    static TX_RAW: AtomicU64 = AtomicU64::new(0);

    extern "C" fn sync_receiver(_arg: u64) {
        let rx = channel::ChannelHandle::from_raw(RX_RAW.load(Ordering::Acquire));
        loop {
            if SYNC_EXIT.load(Ordering::Relaxed) {
                break;
            }
            match channel::recv(rx) {
                Ok(_msg) => { /* consumed */ }
                Err(_) => break, // channel closed
            }
        }
    }

    extern "C" fn sync_driver(_arg: u64) {
        let tx = channel::ChannelHandle::from_raw(TX_RAW.load(Ordering::Acquire));

        // Warmup.
        for _ in 0..20u32 {
            let msg = match Message::from_bytes(b"warm") {
                Ok(m) => m,
                Err(_) => break,
            };
            if channel::send_blocking(tx, msg).is_err() {
                break;
            }
        }

        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut total = 0u64;

        for _ in 0..ITERS {
            let msg = match Message::from_bytes(b"sync") {
                Ok(m) => m,
                Err(_) => break,
            };
            let start = crate::bench::rdtsc_serialized();
            if channel::send_blocking(tx, msg).is_err() {
                break;
            }
            let end = crate::bench::rdtsc();
            let elapsed = end.saturating_sub(start);
            if elapsed < min { min = elapsed; }
            if elapsed > max { max = elapsed; }
            total = total.saturating_add(elapsed);
        }

        let mean = total.checked_div(u64::from(ITERS)).unwrap_or(0);
        SYNC_MIN.store(min, Ordering::Release);
        SYNC_MEAN.store(mean, Ordering::Release);
        SYNC_MAX.store(max, Ordering::Release);

        // Signal receiver to exit and close our end.
        SYNC_EXIT.store(true, Ordering::Release);
        channel::close(tx);

        SYNC_DONE.store(true, Ordering::Release);
    }

    // Reset statics.
    SYNC_DONE.store(false, Ordering::Release);
    SYNC_EXIT.store(false, Ordering::Release);
    SYNC_MIN.store(u64::MAX, Ordering::Relaxed);

    // Create sync channel.
    let (tx, rx) = channel::create_sync();
    TX_RAW.store(tx.raw(), Ordering::Release);
    RX_RAW.store(rx.raw(), Ordering::Release);

    // Spawn receiver first so it's blocked in recv() by the time
    // the driver starts sending.
    let recv_id = match sched::spawn(b"bch-srx", RECV_PRIO, sync_receiver, 0, 0) {
        Ok(id) => id,
        Err(e) => {
            serial_println!("[bench] ipc_channel_sync: SKIP (recv spawn: {:?})", e);
            channel::close(tx);
            channel::close(rx);
            return;
        }
    };

    // Let the receiver task run and block on recv().
    sched::yield_now();

    let driver_id = match sched::spawn(b"bch-stx", DRIVER_PRIO, sync_driver, 0, 0) {
        Ok(id) => id,
        Err(_) => {
            serial_println!("[bench] ipc_channel_sync: SKIP (driver spawn failed)");
            sched::kill_task(recv_id);
            channel::close(tx);
            channel::close(rx);
            sched::reap_dead_tasks();
            return;
        }
    };

    // Wait for driver to finish.
    for _ in 0..10_000u32 {
        if SYNC_DONE.load(Ordering::Acquire) {
            break;
        }
        sched::yield_now();
    }

    if !SYNC_DONE.load(Ordering::Acquire) {
        serial_println!("[bench] ipc_channel_sync: TIMEOUT");
        sched::kill_task(recv_id);
        sched::kill_task(driver_id);
        sched::reap_dead_tasks();
        channel::close(rx);
        return;
    }

    let min = SYNC_MIN.load(Ordering::Acquire);
    let mean = SYNC_MEAN.load(Ordering::Acquire);
    let max = SYNC_MAX.load(Ordering::Acquire);
    let min_ns = cycles_to_ns(min);
    let mean_ns = cycles_to_ns(mean);

    serial_println!(
        "[bench] ipc_channel_sync_rt: min={} cycles ({}ns), mean={} cycles ({}ns), max={} cycles  [{} iters]",
        min, min_ns, mean, mean_ns, max, ITERS
    );

    // Target: < 5 µs.  Sync IPC includes context switches (sender→receiver
    // and back), so it's slower than async channel send+recv.  L4/seL4
    // achieve ~0.5-1 µs for the pure IPC portion; our target includes
    // full context switch overhead under QEMU emulation.
    let target_ns = 5000u64;
    let sync_result = BenchResult {
        name: String::from("ipc_channel_sync"),
        iterations: ITERS,
        min_cycles: min,
        mean_cycles: mean,
        max_cycles: max,
        min_ns,
        mean_ns,
    };
    score("ipc_channel_sync", &sync_result, target_ns);
    if min_ns <= target_ns {
        serial_println!(
            "[bench]   ipc_channel_sync: PASS (min {}ns <= target {}ns)",
            min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   ipc_channel_sync: ABOVE TARGET (min {}ns > target {}ns)",
            min_ns, target_ns
        );
    }

    // Clean up.
    sched::kill_task(recv_id);
    sched::kill_task(driver_id);
    sched::reap_dead_tasks();
    channel::close(rx);
}

// ---------------------------------------------------------------------------
// Pipe round-trip benchmark
// ---------------------------------------------------------------------------

/// Benchmark pipe write+read round-trip.
///
/// Creates a pipe, writes 64 bytes, reads them back.  Measures the
/// kernel-side hot path for byte-stream IPC.
fn bench_ipc_pipe() {
    use crate::ipc::pipe;

    let (rd, wr) = pipe::create();

    // Warm up.
    {
        let data = [0xABu8; 64];
        pipe::write(wr, &data).expect("bench: pipe warmup write");
        let mut buf = [0u8; 64];
        let _ = pipe::read(rd, &mut buf).expect("bench: pipe warmup read");
    }

    let result = run("ipc_pipe_roundtrip_64", 1000, || {
        let data = [0x42u8; 64];
        pipe::write(wr, &data).expect("bench: pipe write");
        let mut buf = [0u8; 64];
        let n = pipe::try_read(rd, &mut buf).expect("bench: pipe read");
        core::hint::black_box(n);
    });

    pipe::close(rd);
    pipe::close(wr);

    // Target: comparable to channel roundtrip (~1-2 µs).
    let target_ns = 3000u64;
    score("ipc_pipe", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   ipc_pipe_roundtrip: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   ipc_pipe_roundtrip: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Service registry connect+accept benchmark
// ---------------------------------------------------------------------------

/// Benchmark service connect + accept cycle.
///
/// Registers a service, then repeatedly connects and accepts.  Measures
/// the overhead of creating a channel pair and brokering the connection.
fn bench_service_connect() {
    use crate::ipc::service;
    use crate::ipc::channel;

    let listener = service::register(b"bench.svc")
        .expect("bench: service register");

    // Warm up.
    {
        let client = service::connect(b"bench.svc").expect("bench: warmup connect");
        let server = service::try_accept(listener).expect("bench: warmup accept")
            .expect("bench: warmup pending");
        channel::close(client);
        channel::close(server);
    }

    let result = run("service_connect_accept", 500, || {
        let client = service::connect(b"bench.svc").expect("bench: connect");
        let server = service::try_accept(listener).expect("bench: accept")
            .expect("bench: pending");
        channel::close(client);
        channel::close(server);
    });

    service::unregister(listener).expect("bench: unregister");

    // Target: connect+accept should be < 5 µs (channel create + queue push + dequeue).
    let target_ns = 5000u64;
    score("service_connect", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   service_connect_accept: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   service_connect_accept: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Eventfd signal+read benchmark
// ---------------------------------------------------------------------------

/// Benchmark eventfd signal+read round-trip.
///
/// Creates an eventfd, writes (signals) it, then try_reads (consumes).
/// Measures the lightweight wake-up notification path.
fn bench_ipc_eventfd() {
    use crate::ipc::eventfd;

    let efd = eventfd::create(0);

    // Warm up.
    {
        eventfd::write(efd, 1).expect("bench: efd warmup write");
        let _ = eventfd::try_read(efd).expect("bench: efd warmup read");
    }

    let result = run("eventfd_signal_read", 2000, || {
        eventfd::write(efd, 1).expect("bench: efd write");
        let val = eventfd::try_read(efd).expect("bench: efd read");
        core::hint::black_box(val);
    });

    eventfd::close(efd);

    // Target: < 1 µs (lighter than channels).
    let target_ns = 1000u64;
    score("ipc_eventfd", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   eventfd_signal_read: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   eventfd_signal_read: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Semaphore benchmark
// ---------------------------------------------------------------------------

/// Benchmark semaphore signal + try_wait round-trip (uncontended).
///
/// Creates a semaphore with count 0 and max 1000, then repeatedly
/// signals (increment) and try_waits (decrement).  Both operations
/// are uncontended — no other task is involved — so this measures
/// pure lock acquisition + atomic counter manipulation.
fn bench_ipc_semaphore() {
    use crate::ipc::semaphore;

    let sem = semaphore::create(0, 1000);

    // Warm up.
    for _ in 0..10 {
        semaphore::signal(sem, 1).expect("bench: sem warmup signal");
        semaphore::try_wait(sem).expect("bench: sem warmup wait");
    }

    let result = run("semaphore_signal_wait", 2000, || {
        semaphore::signal(sem, 1).expect("bench: sem signal");
        semaphore::try_wait(sem).expect("bench: sem wait");
    });

    semaphore::close(sem);

    // Target: < 1 µs (similar to eventfd — both are counter-based).
    let target_ns = 1000u64;
    score("ipc_semaphore", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   semaphore_signal_wait: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   semaphore_signal_wait: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Futex benchmark
// ---------------------------------------------------------------------------

/// Benchmark futex wake on empty wait list (uncontended fast path).
///
/// The critical performance requirement for futex-based userspace mutexes
/// is that unlock (atomic store + futex_wake) is fast when nobody is
/// waiting.  This measures just the kernel side: futex_wake scans the
/// hash bucket, finds no waiters, returns 0.
///
/// Also benchmarks futex_wait with a value mismatch (the other fast
/// path: CAS fails, return immediately without blocking).
fn bench_ipc_futex() {
    use crate::ipc::futex;

    // Use a stack-allocated u32 as the futex address.
    // The address must be 4-byte aligned (guaranteed for stack u32).
    let futex_var: u32 = 42;
    let futex_addr = &futex_var as *const u32 as u64;

    // Warm up.
    for _ in 0..10 {
        let _ = futex::futex_wake(futex_addr, 1);
    }

    // Benchmark: wake with no waiters.
    let result = run("futex_wake_empty", 2000, || {
        let woken = futex::futex_wake(futex_addr, 1);
        core::hint::black_box(woken);
    });

    // Target: < 500 ns.  This is a hash lookup + empty list check.
    // Linux uncontended futex_wake: ~200-500ns.
    let target_ns = 500u64;
    score("futex_wake_empty", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   futex_wake_empty: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   futex_wake_empty: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }

    // Benchmark: wait with value mismatch (immediate return, no block).
    // We pass expected=0 but the actual value is 42 → returns false
    // immediately.
    let result2 = run("futex_wait_mismatch", 2000, || {
        // Value is 42 but expected=0 → immediate return (Ok(false)).
        let r = futex::futex_wait(futex_addr, 0);
        let _ = core::hint::black_box(r);
    });

    // Target: < 500 ns.  Compare + return, no blocking.
    if result2.min_ns <= target_ns {
        serial_println!(
            "[bench]   futex_wait_mismatch: PASS (min {}ns <= target {}ns)",
            result2.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   futex_wait_mismatch: ABOVE TARGET (min {}ns > target {}ns)",
            result2.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Shared memory benchmark
// ---------------------------------------------------------------------------

/// Benchmark shared memory create + close cycle.
///
/// Measures the overhead of creating and destroying a shared memory
/// region.  The create path allocates a handle, allocates physical
/// frames, and maps them into the kernel address space.  Close
/// unmaps and frees everything.
///
/// This is the setup cost for any shared-memory IPC interaction.
fn bench_ipc_shm() {
    use crate::ipc::shm;

    // Warm up.
    for _ in 0..5 {
        let h = shm::create(16384).expect("bench: shm warmup create");
        shm::close(h);
    }

    let result = run("shm_create_close_16k", 500, || {
        let h = shm::create(16384).expect("bench: shm create");
        core::hint::black_box(h);
        shm::close(h);
    });

    // Target: < 5 µs.  Includes frame allocation, handle management,
    // and kernel mapping/unmapping.
    let target_ns = 5000u64;
    score("shm_create_close", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   shm_create_close: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   shm_create_close: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }

    // Also benchmark a read/write cycle through shared memory.
    // Create once, write 64 bytes, read them back.
    {
        let h = shm::create(16384).expect("bench: shm bench create");
        let ptr = shm::kernel_addr(h).expect("bench: shm addr");

        let result_rw = run("shm_rw_64bytes", 2000, || {
            // SAFETY: ptr is valid kernel memory from shm::create,
            // exclusively ours, 16 KiB region is large enough for 64 bytes.
            unsafe {
                core::ptr::write_bytes(ptr, 0xAB, 64);
                let val = core::ptr::read_volatile(ptr);
                core::hint::black_box(val);
            }
        });

        shm::close(h);

        // Target: < 200 ns.  This is just a memset + memory read.
        let rw_target_ns = 200u64;
        score("shm_rw_64bytes", &result_rw, rw_target_ns);
        if result_rw.min_ns <= rw_target_ns {
            serial_println!(
                "[bench]   shm_rw_64bytes: PASS (min {}ns <= target {}ns)",
                result_rw.min_ns, rw_target_ns
            );
        } else {
            serial_println!(
                "[bench]   shm_rw_64bytes: ABOVE TARGET (min {}ns > target {}ns)",
                result_rw.min_ns, rw_target_ns
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Completion port benchmark
// ---------------------------------------------------------------------------

/// Benchmark completion port try_wait on empty port (no events).
///
/// Measures the fast-path polling cost when no events are ready.
/// Event-driven servers call this in their main loop to check for
/// new completions.  The try_wait path acquires a lock, checks the
/// event queue, and returns an empty Vec.
fn bench_ipc_completion_port() {
    use crate::ipc::completion;

    let cp = completion::create();

    // Warm up.
    for _ in 0..10 {
        let _ = completion::try_wait(cp);
    }

    let result = run("cp_try_wait_empty", 2000, || {
        let events = completion::try_wait(cp);
        let _ = core::hint::black_box(events);
    });

    // Target: < 500 ns.  Lock acquire, check empty queue, return.
    let target_ns = 500u64;
    score("cp_try_wait_empty", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   cp_try_wait_empty: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   cp_try_wait_empty: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }

    // Also benchmark notify + try_wait (post an event and consume it).
    {
        use crate::ipc::eventfd;
        use crate::ipc::completion::WaitSource;

        let efd = eventfd::create(0);
        completion::register(cp, WaitSource::EventFd(efd.raw()), 0x1234)
            .expect("bench: cp register");

        // Each iteration: signal the eventfd (which notifies the CP),
        // then try_wait to consume the event, then consume the eventfd.
        let result_rt = run("cp_notify_wait_rt", 1000, || {
            eventfd::write(efd, 1).expect("bench: cp efd write");
            let events = completion::try_wait(cp).expect("bench: cp wait");
            core::hint::black_box(&events);
            // Drain the eventfd so the next iteration starts clean.
            let _ = eventfd::try_read(efd);
        });

        completion::unregister(cp, WaitSource::EventFd(efd.raw()))
            .expect("bench: cp unregister");
        eventfd::close(efd);

        // Target: < 2 µs.  Eventfd write + CP notification + try_wait.
        let rt_target_ns = 2000u64;
        score("cp_notify_wait_rt", &result_rt, rt_target_ns);
        if result_rt.min_ns <= rt_target_ns {
            serial_println!(
                "[bench]   cp_notify_wait_rt: PASS (min {}ns <= target {}ns)",
                result_rt.min_ns, rt_target_ns
            );
        } else {
            serial_println!(
                "[bench]   cp_notify_wait_rt: ABOVE TARGET (min {}ns > target {}ns)",
                result_rt.min_ns, rt_target_ns
            );
        }
    }

    completion::close(cp);
}

// ---------------------------------------------------------------------------
// io_ring benchmark
// ---------------------------------------------------------------------------

/// Benchmark io_ring NOP submission throughput.
///
/// Measures the per-SQE overhead of the io_ring submission path by
/// submitting NOP operations in batches.  This captures:
/// - Ring buffer pointer arithmetic (atomic loads/stores)
/// - SQE read + opcode dispatch
/// - CQE write
/// - Completion port notification check (no CP registered)
///
/// NOP is used because it isolates the ring overhead from any actual
/// I/O work.  Real opcodes add their own cost on top.
fn bench_io_ring_nop() {
    use crate::ipc::io_ring::{self, SqEntry, IoRingHeader, IO_OP_NOP};

    // Create a ring with 64 entries.
    let (ring_handle, base_virt, _frames) = match io_ring::setup(64, 64) {
        Ok(r) => r,
        Err(e) => {
            serial_println!("[bench]   io_ring_nop_submit: SKIP ({:?})", e);
            return;
        }
    };

    // SAFETY: base_virt was returned by io_ring::setup, pointing to a
    // valid IoRingHeader at the start of the mapped shared memory region.
    let header = unsafe { &mut *(base_virt as *mut IoRingHeader) };
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + core::mem::size_of::<IoRingHeader>() as u64) as *mut SqEntry;

    // Pre-fill the SQ with 32 NOP entries (batch size per iteration).
    let batch_size: u32 = 32;
    for i in 0..batch_size {
        let sqe = SqEntry {
            opcode: IO_OP_NOP,
            flags: 0,
            _pad0: [0; 2],
            _pad1: 0,
            user_data: i as u64,
            handle: 0,
            addr: 0,
            len: 0,
            _pad2: 0,
            arg1: 0,
            arg2: 0,
        };
        // SAFETY: sq_base points to a valid SQ array with 64 entries.
        unsafe { *sq_base.add(i as usize) = sqe; }
    }

    // Warm up.
    for _ in 0..5 {
        header.sq_head.store(0, core::sync::atomic::Ordering::Release);
        header.sq_tail.store(batch_size, core::sync::atomic::Ordering::Release);
        header.cq_head.store(0, core::sync::atomic::Ordering::Release);
        header.cq_tail.store(0, core::sync::atomic::Ordering::Release);
        let _ = io_ring::enter(ring_handle, 0);
    }

    // Benchmark: submit 32 NOP SQEs per iteration.
    // We measure the cost of the enter() call and divide by batch_size
    // to get per-SQE cost.
    let iterations: u32 = 500;
    let mut min_cycles = u64::MAX;
    let mut total_cycles = 0u64;

    for _ in 0..iterations {
        // Reset ring pointers for a fresh batch.
        header.sq_head.store(0, core::sync::atomic::Ordering::Release);
        header.sq_tail.store(batch_size, core::sync::atomic::Ordering::Release);
        header.cq_head.store(0, core::sync::atomic::Ordering::Release);
        header.cq_tail.store(0, core::sync::atomic::Ordering::Release);

        let start = rdtsc();
        let _ = io_ring::enter(ring_handle, 0);
        let end = rdtsc();

        let elapsed = end.wrapping_sub(start);
        min_cycles = min_cycles.min(elapsed);
        total_cycles = total_cycles.saturating_add(elapsed);
    }

    let _ = io_ring::destroy(ring_handle);

    // Convert to per-SQE metrics.
    #[allow(clippy::arithmetic_side_effects)]
    let min_per_sqe = min_cycles / batch_size as u64;
    #[allow(clippy::arithmetic_side_effects)]
    let mean_per_sqe = total_cycles / (iterations as u64 * batch_size as u64);

    let min_ns = cycles_to_ns(min_per_sqe);
    let mean_ns = cycles_to_ns(mean_per_sqe);

    serial_println!(
        "[bench]   io_ring_nop_submit: min={}cy ({}ns) mean={}cy ({}ns) [per SQE, batch={}]",
        min_per_sqe, min_ns, mean_per_sqe, mean_ns, batch_size
    );

    // Target: < 200ns per SQE (Linux io_uring: 100-200ns).
    let result = BenchResult {
        name: String::from("io_ring_nop_submit"),
        iterations,
        min_cycles: min_per_sqe,
        mean_cycles: mean_per_sqe,
        max_cycles: min_per_sqe, // no max tracked per-SQE
        min_ns,
        mean_ns,
    };
    let target_ns = 200u64;
    score("io_ring_nop", &result, target_ns);
    if min_ns <= target_ns {
        serial_println!(
            "[bench]   io_ring_nop_submit: PASS (min {}ns <= target {}ns)",
            min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   io_ring_nop_submit: ABOVE TARGET (min {}ns > target {}ns)",
            min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// Page fault benchmark
// ---------------------------------------------------------------------------

/// Benchmark anonymous page fault resolution.
///
/// Registers a demand-page VMA, writes to each page (triggering a fault),
/// measures the fault handler's resolution time.  Each iteration:
///   1. Maps a page table entry as "lazy" (no physical frame yet)
///   2. Calls the fault handler to resolve it (alloc frame, zero, map, flush)
///   3. Unmaps the page (cleanup for next iteration)
///
/// This measures the full fault path excluding the CPU exception overhead
/// (which we can't trigger from kernel mode).
fn bench_page_fault() {
    use crate::mm::{frame, page_table::{self, PageFlags, VirtAddr}};

    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());

    // Pick a kernel-space virtual address range that's not in use.
    // Use a high address in the kernel reserved range.
    // Must be 16 KiB aligned for map_frame.
    let bench_virt_base: u64 = 0xFFFF_CB00_0000_0000;
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;

    // Measure only the demand-fault path: alloc_zeroed + map + local TLB flush.
    //
    // The previous benchmark also timed unmap + IPI-broadcast flush + free,
    // which inflated results by ~50-100%.  A real demand fault only does
    // alloc+map+local_flush; cleanup happens later (munmap, process exit).
    //
    // Use unique virtual addresses per iteration so each map goes to a fresh
    // page.  Clean up all mappings in bulk after the timed loop.

    let iterations: u32 = 200;
    let warmup = core::cmp::max(iterations / 10, 5);
    let total_runs = warmup.saturating_add(iterations);

    // Run warmup + measurement with unique addresses.
    let mut min = u64::MAX;
    let mut max = 0u64;
    let mut total_cycles = 0u64;

    for i in 0..total_runs {
        #[allow(clippy::arithmetic_side_effects)]
        let vaddr = bench_virt_base + (i as u64) * (frame::FRAME_SIZE as u64);
        let virt = VirtAddr::new(vaddr);

        // --- Timed section: matches real demand_page() path ---
        let start = rdtsc_serialized();

        let f = frame::alloc_frame_zeroed().expect("bench: alloc_zeroed");
        // SAFETY: vaddr is in unused kernel space, pml4 is valid,
        // f is freshly allocated.
        unsafe {
            page_table::map_frame(pml4, virt, f, flags).expect("bench: map");
        }
        // Local-only flush — matches real demand fault path (no IPI
        // broadcast needed for never-before-mapped pages).
        // SAFETY: invlpg is always safe in ring 0.
        unsafe { page_table::flush_frame_local(virt); }

        let end = rdtsc();
        // --- End timed section ---

        // Only record measurement iterations (skip warmup).
        if i >= warmup {
            let elapsed = end.saturating_sub(start);
            if elapsed < min { min = elapsed; }
            if elapsed > max { max = elapsed; }
            total_cycles = total_cycles.saturating_add(elapsed);
        }
    }

    let mean = total_cycles.checked_div(iterations as u64).unwrap_or(0);
    let min_ns = cycles_to_ns(min);
    let mean_ns = cycles_to_ns(mean);

    serial_println!(
        "[bench] page_fault_anonymous: min={} cycles ({}ns), mean={} cycles ({}ns), max={} cycles  [{} iters]",
        min, min_ns, mean, mean_ns, max, iterations
    );

    // Bulk cleanup: unmap and free all frames.
    for i in 0..total_runs {
        #[allow(clippy::arithmetic_side_effects)]
        let vaddr = bench_virt_base + (i as u64) * (frame::FRAME_SIZE as u64);
        let virt = VirtAddr::new(vaddr);
        // SAFETY: we mapped these pages above.
        let returned = unsafe {
            page_table::unmap_frame(pml4, virt).expect("bench: unmap cleanup")
        };
        // SAFETY: sole owner, all mappings removed.
        unsafe { frame::free_frame(returned).expect("bench: free cleanup"); }
    }
    // Single TLB shootdown for the entire range after all unmaps.
    crate::tlb::flush_range(bench_virt_base, total_runs.saturating_mul(4));

    let result = BenchResult {
        name: String::from("page_fault_anonymous"),
        iterations,
        min_cycles: min,
        mean_cycles: mean,
        max_cycles: max,
        min_ns,
        mean_ns,
    };

    // Target: < 10 µs (Linux anonymous page fault: ~2-5 µs).
    let target_ns = 10_000u64;
    score("page_fault", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   page_fault_anonymous: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   page_fault_anonymous: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

// ---------------------------------------------------------------------------
// ISR latency benchmark
// ---------------------------------------------------------------------------

/// Benchmark the timer ISR hard-IRQ phase latency.
///
/// Enables per-tick TSC sampling inside `handle_timer_irq`, lets the
/// timer fire for ~100 ticks (~1 second at 100 Hz), then reads the
/// accumulated min/mean/max cycles.
///
/// The hard-IRQ phase is the interval from ISR entry to EOI — the time
/// during which other device interrupts are blocked on this CPU.  Our
/// target (from `baselines.toml`) is < 10 µs (37 000 cycles).
///
/// Unlike other benchmarks that call a function in a loop, this one
/// measures work driven by hardware interrupts, so we yield to let
/// timer ticks accumulate.
fn bench_isr_latency() {
    use crate::apic;

    let start_tick = apic::tick_count();
    serial_println!(
        "[bench] isr_latency: measuring ~10 timer ticks (start_tick={})...",
        start_tick,
    );

    // Start measurement — next timer ISR begins sampling.
    apic::start_isr_measurement();

    // Busy-wait for ~10 timer ticks (~100ms at 100 Hz).
    //
    // We can't use yield_now() here because the boot task (priority 0)
    // gets re-selected immediately on each yield — all 2000 yields
    // complete before a single timer tick fires.  Instead, spin-wait
    // on the tick counter.  The timer ISR fires normally (interrupts
    // are enabled) and records ISR latency measurements on each tick.
    //
    // Under QEMU/TCG, timer delivery is very slow — 100 ticks could
    // take minutes of wall-clock time.  We keep the sample count low
    // (10 ticks, ~100ms on real hardware) with a tight 2-second TSC
    // timeout.  Even a few samples give a reliable minimum measurement.
    let target_ticks = 10u64;
    let tsc_start = rdtsc();
    let tsc_timeout = tsc_freq().saturating_mul(2); // 2 seconds worth of cycles
    loop {
        let elapsed_ticks = apic::tick_count().saturating_sub(start_tick);
        if elapsed_ticks >= target_ticks {
            break;
        }
        let elapsed_tsc = rdtsc().saturating_sub(tsc_start);
        if elapsed_tsc > tsc_timeout {
            serial_println!(
                "[bench] isr_latency: TSC timeout after ~2s (ticks advanced: {}, expected: {})",
                elapsed_ticks, target_ticks
            );
            break;
        }
        core::hint::spin_loop();
    }

    // Stop measurement.
    apic::stop_isr_measurement();

    let actual_ticks = apic::tick_count().saturating_sub(start_tick);

    match apic::isr_measurement_results() {
        Some(m) => {
            let min_ns = cycles_to_ns(m.min_cycles);
            let mean_ns = cycles_to_ns(m.mean_cycles);
            let max_ns = cycles_to_ns(m.max_cycles);

            serial_println!(
                "[bench] isr_hard_irq: min={} cycles ({}ns), mean={} cycles ({}ns), max={} cycles ({}ns)  [{} samples in {} ticks]",
                m.min_cycles, min_ns,
                m.mean_cycles, mean_ns,
                m.max_cycles, max_ns,
                m.count, actual_ticks
            );

            // Target from baselines.toml: < 37000 cycles (< 10 µs).
            let target_cycles = 37_000u64;
            let isr_result = BenchResult {
                name: String::from("isr_latency"),
                iterations: m.count as u32,
                min_cycles: m.min_cycles,
                mean_cycles: m.mean_cycles,
                max_cycles: m.max_cycles,
                min_ns,
                mean_ns,
            };
            score("isr_latency", &isr_result, 10000);
            if m.min_cycles <= target_cycles {
                serial_println!(
                    "[bench]   isr_latency: PASS (min {} cycles <= target {} cycles)",
                    m.min_cycles, target_cycles
                );
            } else {
                serial_println!(
                    "[bench]   isr_latency: ABOVE TARGET (min {} cycles > target {} cycles)",
                    m.min_cycles, target_cycles
                );
            }
        }
        None => {
            serial_println!(
                "[bench] isr_latency: NO SAMPLES (timer ticks elapsed: {})",
                actual_ticks
            );
        }
    }
}

// ---------------------------------------------------------------------------
// VFS benchmarks (fs zone)
// ---------------------------------------------------------------------------

/// Benchmark VFS stat() — single path component lookup.
///
/// Measures the time to stat the root directory ("/"), which hits the
/// VFS path-resolution hot path.  This is the simplest VFS operation
/// and represents the cached-lookup fast path.
///
/// Target from baselines.toml: < 700 ns per component (Linux: ~350 ns).
fn bench_vfs_stat() {
    use crate::fs::vfs::Vfs;

    // Verify VFS is available (it's initialized after self-tests).
    if Vfs::stat("/").is_err() {
        serial_println!("[bench] vfs_stat: SKIP (VFS not initialized)");
        return;
    }

    let result = run("vfs_stat_root", 500, || {
        let _ = core::hint::black_box(Vfs::stat("/"));
    });

    let target_ns = 700u64;
    score("vfs_stat_root", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   vfs_stat_root: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   vfs_stat_root: ABOVE TARGET (min {}ns > target {}ns)",
            result.min_ns, target_ns
        );
    }
}

/// Diagnostic breakdown of `vfs_stat_root`, which misses its 700 ns target
/// by ~8.5x (5920 ns measured on the first release-profile run).
///
/// This exists to locate the cost rather than guess at it.  `Vfs::stat` is
/// exactly two phases and both are reachable from public API, so the split
/// needs no new plumbing:
///
/// * `Vfs::stat(p)`          = `resolve_follow(p)` + `stat_resolved(p)`
/// * `Vfs::stat_resolved(p)` = the second phase alone
///
/// so the difference is `resolve_follow` — namespace translation, path
/// normalisation, and the `VFS_DCACHE` lookup.
///
/// **The dcache was the first suspect and it was wrong.**  `VfsDcache::lookup`
/// is a linear scan over `VFS_DCACHE_SIZE` = 1024 slots, which CLAUDE.md's
/// performance rules forbid outright ("Linear scans … must be O(1) or
/// O(log n)"), so it looked like the answer.  The occupancy line below said
/// otherwise: 25 live entries, 100% hit rate, so a hit-scan terminates in ~25
/// iterations.  A linear scan's cost is a function of occupancy, not capacity.
/// The scan is still a latent defect — the *miss* path walks all 1024 slots,
/// and occupancy grows — but it is not what makes `stat("/")` slow.  The
/// counters stay in the output precisely so that conclusion keeps being
/// checked as occupancy changes.
///
/// `resolve_follow` is therefore split one level further, into the three
/// stages it actually performs, so the next fix is aimed by measurement
/// instead of by inspection.
/// Benchmark the cost of acquiring and releasing an **uncontended** lock.
///
/// This anchor was missing, and its absence is why a wrong number went
/// unchallenged for a whole investigation.  A prediction about `stat("/")`
/// argued that "`sched_pick_next` = 40 ns and it takes the run-queue lock,
/// therefore an uncontended spinlock is ≲ 20 ns" — but `bench_sched_pick_next`
/// builds a *local* `PriorityRoundRobin` on the stack and never touches
/// `SCHED.lock()`.  The one figure bounding the most common operation in the
/// kernel was measured on a lock-free path.  The prediction missed by 4.9x.
/// See `B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY`.
///
/// The variants, because the interesting quantity is a difference, not an
/// absolute:
///
/// * **raw** — `spin::Mutex`, the bare atomic `try_lock` + store. The floor.
/// * **tracked** — `crate::sync::Mutex` as the kernel actually uses it, with
///   `TRACKING_ENABLED` in its default (on) state. Every acquisition calls
///   `lockdep::lock_acquire`, `preempt_disable`, **one `rdtsc`**, and
///   `stats.record_uncontended()`; every release calls a **second `rdtsc`**,
///   `record_hold`, `lockdep::lock_release` and `preempt_enable`.
/// * **no-lockdep** — the same type with `lockdep::set_enabled(false)`, so both
///   `lock_acquire` and `lock_release` return on their first load.
/// * **untracked** — the same type with `TRACKING_ENABLED` off.
///
/// Plus the two suspected components measured **directly**, in isolation,
/// rather than inferred from the differences: a bare `preempt_disable()` /
/// `preempt_enable()` pair and a bare `rdtsc()` pair. Direct measurement is
/// the point: the first version of this benchmark reported a lumped
/// "lockdep+preempt +629ns" and I then *reasoned* about which half dominated —
/// the same move that has been wrong every single time this session. The
/// components and the differences are printed together so they can disagree;
/// if `lockdep + preempt + 2×rdtsc` does not roughly account for
/// `tracked - raw`, the model is missing something and says so out loud.
///
/// A caveat the first version got wrong: **untracked is not "tracked minus the
/// statistics".** With tracking off, `Mutex::lock` skips the `try_lock` fast
/// path entirely and calls `lock_contended()`, which is `#[cold]`,
/// `#[inline(never)]`, and computes `tsc_freq()` plus its own `rdtsc()` before
/// the first acquisition attempt. So `set_tracking_enabled(false)` can be
/// *slower* than leaving it on — which is exactly what the first run measured
/// (654 ns vs 628 ns) and what the old "+0ns for stats" line misreported as
/// statistics being free.
///
/// Whatever the split, it is paid by **every lock acquisition in the kernel**,
/// so if it is large this is not a VFS finding at all — it is a whole-kernel
/// one, and every benchmark in this suite that takes a lock is partly
/// measuring instrumentation.
fn bench_lock_primitives() {
    // NB: fully qualified. `Mutex` is aliased to `PreemptSpinMutex` at the top
    // of this file, which is a *different* lock type with different overhead --
    // the one under investigation is `crate::sync::Mutex`, the type
    // `PROCESS_NS` and friends use.
    static RAW: spin::Mutex<u64> = spin::Mutex::new(0);
    static TRACKED: crate::sync::Mutex<u64> = crate::sync::Mutex::new(0);

    let raw = run("lock_raw_spin", 2000, || {
        let mut g = RAW.lock();
        *g = core::hint::black_box(*g).wrapping_add(1);
    });

    let tracked = run("lock_tracked", 2000, || {
        let mut g = TRACKED.lock();
        *g = core::hint::black_box(*g).wrapping_add(1);
    });

    // Toggle lockdep off for the third variant, then restore it. Safe to
    // toggle here only because no tracked lock is held at this point -- see
    // `lockdep::set_enabled`. Restoring matters twice over: leaving it off
    // would both change the cost of every later lock and silently retire the
    // deadlock validator for the rest of the boot.
    let lockdep_was_on = crate::lockdep::is_enabled();
    crate::lockdep::set_enabled(false);
    let no_lockdep = run("lock_no_lockdep", 2000, || {
        let mut g = TRACKED.lock();
        *g = core::hint::black_box(*g).wrapping_add(1);
    });
    crate::lockdep::set_enabled(lockdep_was_on);

    // Toggle tracking off for the fourth variant, then restore it. Same
    // reasoning: leaving it off would silently change the cost of every lock in
    // every benchmark that runs after this one.
    crate::sync::set_tracking_enabled(false);
    let untracked = run("lock_tracked_no_stats", 2000, || {
        let mut g = TRACKED.lock();
        *g = core::hint::black_box(*g).wrapping_add(1);
    });
    crate::sync::set_tracking_enabled(true);

    // The two suspected components, measured on their own rather than
    // differenced out of the variants above.
    let preempt = run("preempt_pair", 2000, || {
        crate::sched::preempt_disable();
        crate::sched::preempt_enable();
    });
    let tsc_pair = run("rdtsc_pair", 2000, || {
        let a = rdtsc();
        let b = rdtsc();
        let _ = core::hint::black_box(b.wrapping_sub(a));
    });

    serial_println!(
        "[bench]   lock acquire+release: raw {}ns, tracked {}ns, no-lockdep {}ns, no-stats {}ns",
        raw.min_ns, tracked.min_ns, no_lockdep.min_ns, untracked.min_ns
    );
    serial_println!(
        "[bench]   lock components (measured): preempt pair {}ns, rdtsc pair {}ns",
        preempt.min_ns, tsc_pair.min_ns
    );
    // Lockdep's per-acquire cost used to be O(registered classes) — a linear
    // scan run twice per lock operation — so this number was the multiplier on
    // it. Printed even now that the lookup is a hash, because it is what would
    // reveal the regression if the index were ever bypassed.
    serial_println!(
        "[bench]   lock context: {} lockdep classes registered",
        crate::lockdep::class_count()
    );

    // Differences, and then the check that the differences and the direct
    // measurements tell the same story. `lockdep_delta` is the only component
    // obtained by subtraction, so it is the one to distrust; the residual line
    // is what would expose it.
    let lockdep_delta = tracked.min_ns.saturating_sub(no_lockdep.min_ns);
    let total_delta = tracked.min_ns.saturating_sub(raw.min_ns);
    let accounted = lockdep_delta
        .saturating_add(preempt.min_ns)
        .saturating_add(tsc_pair.min_ns);
    serial_println!(
        "[bench]   lock overhead: total +{}ns = lockdep {}ns + preempt {}ns + rdtsc {}ns \
         + unexplained {}ns",
        total_delta,
        lockdep_delta,
        preempt.min_ns,
        tsc_pair.min_ns,
        total_delta.saturating_sub(accounted),
    );
    if accounted > total_delta {
        serial_println!(
            "[bench]   lock overhead: WARNING components ({}ns) exceed the measured \
             total ({}ns) -- the cost model is wrong, not merely imprecise",
            accounted, total_delta
        );
    }

    // Scored against the tracked variant, because that is what the kernel
    // actually pays. 500ns is the value the vfs_stat stage split *implied*
    // (3 locks + 3 map lookups + 1 alloc = 1948ns); this benchmark exists to
    // replace that inference with a measurement, so the target is deliberately
    // set at the inferred value: if the inference was right this sits exactly
    // on the line, and any movement is then real.
    score("lock_uncontended", &tracked, 500);
}

fn bench_vfs_stat_breakdown() {
    use crate::fs::vfs::Vfs;

    if Vfs::stat("/").is_err() {
        serial_println!("[bench] vfs_stat_breakdown: SKIP (VFS not initialized)");
        return;
    }

    let (hits_before, misses_before, valid_entries) = Vfs::dcache_stats();

    // Phase A+B together.
    let full = run("vfs_stat_breakdown_full", 500, || {
        let _ = core::hint::black_box(Vfs::stat("/"));
    });
    // Phase B alone. "/" is already normalised and mount-relative, so this is
    // the same work `stat` does after `resolve_follow` returns.
    let resolved_only = run("vfs_stat_breakdown_resolved", 500, || {
        let _ = core::hint::black_box(Vfs::stat_resolved("/"));
    });

    // Phase A measured *directly* rather than by subtraction.  `resolve_path`
    // is a public alias for `resolve_follow`, so the two numbers are the same
    // quantity obtained two ways; if they disagree, the subtraction is what is
    // wrong, not the code under it.  That check matters here because `stat`
    // feeds `stat_resolved` the *resolved* path, whereas the isolated
    // `stat_resolved` benchmark is fed "/" — if resolution rewrites the path,
    // subtraction silently charges the difference to the wrong phase.
    let resolve_direct = run("vfs_stat_breakdown_resolve", 500, || {
        let _ = core::hint::black_box(Vfs::resolve_path("/"));
    });

    // Phase A1: per-process namespace translation alone.  For the root
    // namespace this is semantically a no-op — it returns the input path
    // unchanged — but it still takes `PROCESS_NS.lock()` and `PROCESS_ROOT
    // .lock()` and allocates a `PathBuf` to say so.  Measuring it separately
    // is the point: a no-op that costs anything is pure overhead on every
    // path operation the OS performs.
    let root_path = crate::fs::path::Path::new("/");
    let ns_only = run("vfs_stat_breakdown_ns", 500, || {
        let _ = core::hint::black_box(crate::ipc::namespace::resolve_path(root_path));
    });
    // Phase A1+A2: the whole prologue — namespace translation, then
    // `validate_path` + `normalize_path` (a second allocation).
    let prologue = run("vfs_stat_breakdown_prologue", 500, || {
        let _ = core::hint::black_box(Vfs::resolve_prologue(root_path));
    });

    let (hits_after, misses_after, valid_after) = Vfs::dcache_stats();

    let resolve_ns = full.min_ns.saturating_sub(resolved_only.min_ns);
    serial_println!(
        "[bench]   vfs_stat_breakdown: full {}ns = resolve_follow ~{}ns + stat_resolved {}ns",
        full.min_ns, resolve_ns, resolved_only.min_ns
    );
    // Within `resolve_follow`, the residual after the prologue is the dcache
    // lock + linear scan + `PathBuf` clone of the hit.  Subtracting measured
    // stages rather than attributing by inspection: the last time this hot
    // path was reasoned about from the code alone, the conclusion was wrong.
    serial_println!(
        "[bench]   vfs_stat_breakdown: resolve_follow measured directly {}ns (vs {}ns by subtraction)",
        resolve_direct.min_ns, resolve_ns
    );
    serial_println!(
        "[bench]   vfs_stat_breakdown: resolve_follow {}ns = ns_translate {}ns + validate_normalize {}ns + dcache_hit ~{}ns",
        resolve_direct.min_ns,
        ns_only.min_ns,
        prologue.min_ns.saturating_sub(ns_only.min_ns),
        resolve_direct.min_ns.saturating_sub(prologue.min_ns)
    );
    // What resolution actually returns decides whether the subtraction above
    // compares like with like: `stat` stats *this*, the isolated benchmark
    // stats "/".
    match Vfs::resolve_path("/") {
        Ok(p) => serial_println!(
            "[bench]   vfs_stat_breakdown: resolve_path(\"/\") -> {:?} ({} bytes)",
            core::str::from_utf8(p.as_path().as_bytes()).unwrap_or("<non-utf8>"),
            p.as_path().as_bytes().len()
        ),
        Err(e) => serial_println!("[bench]   vfs_stat_breakdown: resolve_path(\"/\") -> Err({:?})", e),
    }
    serial_println!(
        "[bench]   vfs_stat_breakdown: dcache {} valid entries (of {}), +{} hits +{} misses over the run",
        valid_after,
        crate::fs::vfs::VFS_DCACHE_SIZE,
        hits_after.saturating_sub(hits_before),
        misses_after.saturating_sub(misses_before)
    );
    // "The fast path did not help" and "the fast path was never taken" are
    // different findings, and telling them apart after the fact is exactly what
    // went wrong twice on this benchmark. Print which one it is.
    serial_println!(
        "[bench]   vfs_stat_breakdown: namespace fast path {} (NS_FEATURES_ACTIVE={})",
        if crate::ipc::namespace::ns_features_active() { "DISABLED" } else { "available" },
        crate::ipc::namespace::ns_features_active(),
    );
    // ---- Coherence gate -------------------------------------------------
    //
    // Everything above is stage attribution by subtraction, and subtraction is
    // only meaningful if the stages were measured under comparable conditions.
    // They are not guaranteed to be: `min` over 500 iterations still reflects
    // whatever the *host* was doing during those 500 iterations, and under TCG
    // that varies enormously (this block has recorded per-iteration maxima of
    // 1.1e8 cycles -- 21 ms -- inside a benchmark whose min is 2.5 us).
    //
    // So: re-measure the very first quantity, unchanged, at the *end* of the
    // block. `full` and `full_again` are the same code over the same input; any
    // difference between them is pure measurement drift across the width of
    // this block, and it bounds how much of every stage difference above is
    // real. Without this, a run where the harness drifted 1.7x mid-block looks
    // exactly like a run where a stage got 1.7x slower -- and the last run was
    // that run: `vfs_stat_root` and `vfs_stat_breakdown_full` are byte-identical
    // benchmarks and reported 2971 ns and 4976 ns in the same boot, while the
    // stage components summed to 133% of the whole they were subtracted from.
    // Both facts were printed and neither was flagged.
    let full_again = run("vfs_stat_breakdown_full2", 500, || {
        let _ = core::hint::black_box(Vfs::stat("/"));
    });
    let (lo, hi) = if full.min_ns <= full_again.min_ns {
        (full.min_ns, full_again.min_ns)
    } else {
        (full_again.min_ns, full.min_ns)
    };
    // Percent, not a ratio: integer division of a ratio would floor 1.9x to 1.
    let drift_pct = if lo == 0 { 0 } else { (hi.saturating_sub(lo)).saturating_mul(100) / lo };
    const DRIFT_LIMIT_PCT: u64 = 25;
    serial_println!(
        "[bench]   vfs_stat_breakdown: drift check — same benchmark twice: {}ns then {}ns ({}%)",
        full.min_ns, full_again.min_ns, drift_pct
    );
    if drift_pct > DRIFT_LIMIT_PCT {
        serial_println!(
            "[bench]   vfs_stat_breakdown: WARNING run is NOT internally coherent ({}% > {}%) \
             — the stage split above is measurement drift and must not be used to attribute \
             cost; only cross-boot-replicated totals are usable from this run",
            drift_pct, DRIFT_LIMIT_PCT
        );
    }
    // The second, independent coherence check: the parts must add up to the
    // whole. `resolve_follow` is measured both directly and by subtracting
    // `stat_resolved` from `full`; those are the same quantity by construction,
    // so a large disagreement means at least one of the three measurements is
    // not measuring what its name says.
    let sum_pct = if full.min_ns == 0 {
        0
    } else {
        resolve_direct.min_ns.saturating_add(resolved_only.min_ns).saturating_mul(100)
            / full.min_ns
    };
    serial_println!(
        "[bench]   vfs_stat_breakdown: parts/whole check — resolve {}ns + resolved {}ns = {}% of full {}ns",
        resolve_direct.min_ns, resolved_only.min_ns, sum_pct, full.min_ns
    );
    if !(75..=125).contains(&sum_pct) {
        serial_println!(
            "[bench]   vfs_stat_breakdown: WARNING parts sum to {}% of the whole — the stage \
             attribution above is not arithmetic, it is noise",
            sum_pct
        );
    }

    let _ = (valid_entries, misses_before);
}

/// Benchmark VFS read + write cycle.
///
/// Writes a small file, reads it back, then deletes it.  Measures the
/// combined cost of write_file + read_file for a 256-byte payload.
/// This exercises the full VFS → driver → buffer path.
fn bench_vfs_read_write() {
    use crate::fs::vfs::Vfs;

    // Test data: 256 bytes of pattern data.
    let data: [u8; 256] = {
        let mut buf = [0u8; 256];
        for (i, b) in buf.iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            { *b = (i & 0xFF) as u8; }
        }
        buf
    };

    let path = "/bench_rw_test.tmp";

    // Verify VFS write works.
    if Vfs::write_file(path, &data).is_err() {
        serial_println!("[bench] vfs_read_write: SKIP (VFS write not available)");
        return;
    }

    // Benchmark write.
    let write_result = run("vfs_write_256", 200, || {
        // write_file creates/overwrites the file.
        let _ = core::hint::black_box(Vfs::write_file(path, &data));
    });

    // Benchmark read.
    let read_result = run("vfs_read_256", 200, || {
        let _ = core::hint::black_box(Vfs::read_file(path));
    });

    // Clean up.
    let _ = Vfs::remove(path); // Best-effort cleanup.

    // Metadata cycle (create+stat+delete) target: <10us per design spec.
    // A full write(256B)+read(256B) is heavier — target 200us under QEMU.
    score("vfs_write_256", &write_result, 200_000);
    score("vfs_read_256", &read_result, 200_000);
    serial_println!(
        "[bench]   vfs_write_256: min {}ns, vfs_read_256: min {}ns",
        write_result.min_ns, read_result.min_ns
    );
}

/// Benchmark VFS readdir on root directory.
///
/// Measures the cost of listing all entries in the root directory.
/// This exercises the VFS directory iteration path.
fn bench_vfs_readdir() {
    use crate::fs::vfs::Vfs;

    if Vfs::readdir("/").is_err() {
        serial_println!("[bench] vfs_readdir: SKIP (VFS not initialized)");
        return;
    }

    let result = run("vfs_readdir_root", 200, || {
        let _ = core::hint::black_box(Vfs::readdir("/"));
    });

    serial_println!(
        "[bench]   vfs_readdir_root: min {}ns ({}ns mean)",
        result.min_ns, result.mean_ns
    );
    score("vfs_readdir", &result, 50000);
}

// ---------------------------------------------------------------------------
// Network benchmarks (net zone)
// ---------------------------------------------------------------------------

/// Benchmark IPv4 packet parsing.
///
/// Parses a minimal 20-byte IPv4 header from a pre-built packet.
/// This is the entry point for all received network traffic.
fn bench_net_ipv4_parse() {
    use crate::net::ipv4;

    // Build a minimal valid IPv4 packet (20-byte header + 4-byte payload).
    let packet: [u8; 24] = [
        0x45, 0x00, 0x00, 0x18, // version/IHL=5, length=24
        0x00, 0x01, 0x00, 0x00, // ID=1, flags=0, frag=0
        0x40, 0x11, 0x00, 0x00, // TTL=64, proto=UDP, checksum=0
        0x0A, 0x00, 0x00, 0x01, // src=10.0.0.1
        0x0A, 0x00, 0x00, 0x02, // dst=10.0.0.2
        0xDE, 0xAD, 0xBE, 0xEF, // payload
    ];

    let result = run("net_ipv4_parse", 2000, || {
        let _ = core::hint::black_box(ipv4::Ipv4Packet::parse(&packet));
    });

    serial_println!(
        "[bench]   net_ipv4_parse: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 300ns (runs on every incoming IP packet).
    score("net_ipv4_parse", &result, 300);
}

/// Benchmark Ethernet frame parsing.
///
/// Parses a minimal Ethernet frame header (14 bytes).
fn bench_net_ethernet_parse() {
    use crate::net::ethernet;

    // Build a minimal Ethernet frame: 14-byte header + 4 bytes payload.
    let frame: [u8; 18] = [
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // dst MAC (broadcast)
        0x02, 0x00, 0x00, 0x00, 0x00, 0x01, // src MAC
        0x08, 0x00,                           // EtherType: IPv4
        0x45, 0x00, 0x00, 0x14,              // payload (IPv4 header start)
    ];

    let result = run("net_ethernet_parse", 2000, || {
        let _ = core::hint::black_box(ethernet::EthernetFrame::parse(&frame));
    });

    serial_println!(
        "[bench]   net_ethernet_parse: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_ethernet_parse", &result, 200);
}

/// Benchmark ARP table lookup.
///
/// Looks up a known-missing IP in the ARP cache.  This measures the
/// hash lookup + miss path, which is the common case for the first
/// packet to a new destination.
fn bench_net_arp_lookup() {
    use crate::net::arp;

    // Use an IP that's unlikely to be in the cache.
    let ip = crate::net::interface::Ipv4Addr([198, 51, 100, 1]);

    let result = run("net_arp_lookup_miss", 2000, || {
        let _ = core::hint::black_box(arp::lookup(ip));
    });

    serial_println!(
        "[bench]   net_arp_lookup_miss: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_arp_lookup", &result, 1000);
}

/// Benchmark IP checksum computation.
///
/// Computes the one's-complement checksum over a 20-byte IPv4 header.
/// This operation runs on every sent and received packet.
fn bench_net_checksum() {
    // 20-byte IPv4 header (with checksum field zeroed for computation).
    let header: [u8; 20] = [
        0x45, 0x00, 0x00, 0x28,
        0x00, 0x01, 0x00, 0x00,
        0x40, 0x06, 0x00, 0x00, // checksum = 0
        0x0A, 0x00, 0x00, 0x01,
        0x0A, 0x00, 0x00, 0x02,
    ];

    let result = run("net_ip_checksum_20b", 5000, || {
        let _ = core::hint::black_box(internet_checksum(&header));
    });

    serial_println!(
        "[bench]   net_ip_checksum_20b: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_checksum", &result, 500);
}

/// Internet checksum (RFC 1071) — one's complement sum of 16-bit words.
///
/// Duplicated here to avoid depending on a specific module's internal
/// checksum function.  The benchmark measures pure computation, not
/// module call overhead.
fn internet_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = ((data[i] as u32) << 8) | (data[i + 1] as u32);
        sum = sum.wrapping_add(word);
        i += 2;
    }
    // Handle odd byte.
    if i < data.len() {
        sum = sum.wrapping_add((data[i] as u32) << 8);
    }
    // Fold 32-bit sum to 16 bits.
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Benchmark TCP checksum computation (IPv4 pseudo-header).
///
/// Computes the TCP checksum over a typical MSS-sized segment (1460 bytes)
/// with the IPv4 12-byte pseudo-header.  This runs on every TCP segment
/// sent or received — it is the single most frequent checksum operation.
fn bench_net_tcp_checksum_v4() {
    // Build a 1460-byte TCP segment (20-byte header + 1440 payload).
    let mut segment = [0xABu8; 1460];
    // Minimal TCP header fields at start.
    segment[0] = 0x1F; segment[1] = 0x90; // src port 8080
    segment[2] = 0x00; segment[3] = 0x50; // dst port 80
    // seq, ack, flags, window...
    segment[12] = 0x50; // data offset 5 (20 bytes)
    segment[13] = 0x18; // PSH|ACK
    // Checksum field zeroed for computation.
    segment[16] = 0; segment[17] = 0;

    let src = crate::net::interface::Ipv4Addr([10, 0, 0, 1]);
    let dst = crate::net::interface::Ipv4Addr([10, 0, 0, 2]);

    let result = run("net_tcp_checksum_v4_1460b", 2000, || {
        let _ = core::hint::black_box(tcp_checksum_bench(&segment, src, dst));
    });

    serial_println!(
        "[bench]   net_tcp_checksum_v4_1460b: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 2000ns for 1460 bytes.
    score("tcp_checksum_v4", &result, 2000);
}

/// TCP checksum (duplicated to avoid depending on tcp module internals).
fn tcp_checksum_bench(segment: &[u8], src: crate::net::interface::Ipv4Addr, dst: crate::net::interface::Ipv4Addr) -> u16 {
    let len = segment.len();
    let mut sum: u32 = 0;
    // IPv4 pseudo-header (12 bytes).
    sum = sum.wrapping_add(((src.0[0] as u32) << 8) | src.0[1] as u32);
    sum = sum.wrapping_add(((src.0[2] as u32) << 8) | src.0[3] as u32);
    sum = sum.wrapping_add(((dst.0[0] as u32) << 8) | dst.0[1] as u32);
    sum = sum.wrapping_add(((dst.0[2] as u32) << 8) | dst.0[3] as u32);
    sum = sum.wrapping_add(6); // protocol TCP
    sum = sum.wrapping_add(len as u32);
    // TCP segment.
    let mut i = 0;
    while i + 1 < len {
        sum = sum.wrapping_add(((segment[i] as u32) << 8) | segment[i + 1] as u32);
        i += 2;
    }
    if i < len {
        sum = sum.wrapping_add((segment[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Benchmark TCP checksum computation (IPv6 pseudo-header).
///
/// Same 1460-byte segment but with the 40-byte IPv6 pseudo-header
/// (src addr 16 + dst addr 16 + length 4 + next_header 4).
/// Compares directly against the IPv4 variant to show the overhead
/// of the larger pseudo-header.
fn bench_net_tcp_checksum_v6() {
    let mut segment = [0xABu8; 1460];
    segment[0] = 0x1F; segment[1] = 0x90;
    segment[2] = 0x00; segment[3] = 0x50;
    segment[12] = 0x50;
    segment[13] = 0x18;
    segment[16] = 0; segment[17] = 0;

    // fe80::1 and fe80::2
    let mut src = [0u8; 16];
    src[0] = 0xfe; src[1] = 0x80; src[15] = 0x01;
    let mut dst = [0u8; 16];
    dst[0] = 0xfe; dst[1] = 0x80; dst[15] = 0x02;

    let result = run("net_tcp_checksum_v6_1460b", 2000, || {
        let _ = core::hint::black_box(tcp_checksum_v6_bench(&segment, &src, &dst));
    });

    serial_println!(
        "[bench]   net_tcp_checksum_v6_1460b: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("tcp_checksum_v6", &result, 2200);
}

/// TCP checksum with IPv6 pseudo-header (bench-local copy).
fn tcp_checksum_v6_bench(segment: &[u8], src: &[u8; 16], dst: &[u8; 16]) -> u16 {
    let len = segment.len();
    let mut sum: u32 = 0;
    // IPv6 pseudo-header: src(16) + dst(16) + length(4) + zero+NH(4).
    let mut i = 0;
    while i < 16 {
        sum = sum.wrapping_add(((src[i] as u32) << 8) | src[i + 1] as u32);
        sum = sum.wrapping_add(((dst[i] as u32) << 8) | dst[i + 1] as u32);
        i += 2;
    }
    // Upper-layer packet length (u32, network order).
    sum = sum.wrapping_add((len >> 16) as u32);
    sum = sum.wrapping_add((len & 0xFFFF) as u32);
    // Zero + next header (TCP = 6).
    sum = sum.wrapping_add(6);
    // TCP segment body.
    i = 0;
    while i + 1 < len {
        sum = sum.wrapping_add(((segment[i] as u32) << 8) | segment[i + 1] as u32);
        i += 2;
    }
    if i < len {
        sum = sum.wrapping_add((segment[i] as u32) << 8);
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Benchmark IPv6 packet parsing.
///
/// Parses a 40-byte IPv6 fixed header.  Increasingly important as
/// dual-stack networking means every received IPv6 packet hits this path.
fn bench_net_ipv6_parse() {
    use crate::net::ipv6;

    // Build a minimal IPv6 packet: 40-byte header + 8-byte UDP payload.
    let mut packet = [0u8; 48];
    // Version (6) + traffic class + flow label.
    packet[0] = 0x60; // version=6, TC=0, flow[0]=0
    // Payload length = 8.
    packet[4] = 0x00; packet[5] = 0x08;
    // Next header = UDP (17).
    packet[6] = 0x11;
    // Hop limit = 64.
    packet[7] = 0x40;
    // Source: fe80::1
    packet[8] = 0xfe; packet[9] = 0x80; packet[23] = 0x01;
    // Destination: fe80::2
    packet[24] = 0xfe; packet[25] = 0x80; packet[39] = 0x02;
    // 8 bytes of dummy UDP payload.
    packet[40] = 0x1F; packet[41] = 0x90; // src port
    packet[42] = 0x00; packet[43] = 0x35; // dst port 53
    packet[44] = 0x00; packet[45] = 0x08; // length
    packet[46] = 0x00; packet[47] = 0x00; // checksum

    let result = run("net_ipv6_parse", 2000, || {
        let _ = core::hint::black_box(ipv6::Ipv6Packet::parse(&packet));
    });

    serial_println!(
        "[bench]   net_ipv6_parse: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_ipv6_parse", &result, 500);
}

/// Benchmark firewall inbound packet check.
///
/// Checks a packet against the firewall rule table.  This runs on
/// every received IPv4 packet when the firewall is enabled.  Measures
/// the rule-matching loop (linear scan over rules + conntrack lookup).
///
/// `check_inbound(protocol, src_ip, payload)` where payload contains
/// port numbers in the TCP/UDP header position.
fn bench_net_firewall_check() {
    use crate::net::firewall;

    let src = crate::net::interface::Ipv4Addr([198, 51, 100, 1]);

    // Build a minimal TCP payload (20-byte header) with src/dst ports.
    let mut payload = [0u8; 20];
    payload[0] = 0x30; payload[1] = 0x39; // src port 12345
    payload[2] = 0x00; payload[3] = 0x50; // dst port 80
    payload[12] = 0x50; // data offset 5

    let result = run("net_firewall_inbound_check", 2000, || {
        let _ = core::hint::black_box(
            firewall::check_inbound(6, src, &payload)
        );
    });

    serial_println!(
        "[bench]   net_firewall_inbound_check: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 1000ns (runs on every inbound packet).
    //
    // This literal said 2000 while the file said 1000, and the comment cited
    // the file for the number it disagreed with -- the exact failure that
    // `report_baselines()` in scripts/bench-history.py was added to catch, and
    // the last of the 11 it found. The file wins: its 1000ns is corroborated
    // by its own `target_cycles = 3700` (1000ns at 3.7GHz), whereas the 2000
    // here had no support other than a citation that was not true. Measured
    // 53ns, so this is comfortable either way -- which is precisely why it
    // could drift undetected for so long.
    score("firewall_check", &result, 1000);
}

/// Benchmark DNS query packet building (label encoding).
///
/// Constructs a DNS query packet locally, mimicking the internal
/// `build_query_typed()` path.  This measures the label encoding
/// (hostname → DNS wire format) plus the Vec allocation, which runs
/// once per DNS resolution.
fn bench_net_dns_build_query() {
    let result = run("net_dns_build_a_query", 1000, || {
        let _ = core::hint::black_box(build_dns_query_bench("www.example.com", 1));
    });

    // DNS query build includes a heap allocation (Vec::with_capacity) which
    // is expensive under QEMU (~35us).  Target set to 40us to track regressions
    // without false-failing on the allocation overhead.
    score("dns_build_query", &result, 40000);
    serial_println!(
        "[bench]   net_dns_build_a_query: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
}

/// Build a DNS query (bench-local copy of the internal label encoder).
fn build_dns_query_bench(name: &str, qtype: u16) -> alloc::vec::Vec<u8> {
    let mut buf = alloc::vec::Vec::with_capacity(64);
    // Header: ID=0x1234, flags=0x0100 (recursion desired), qdcount=1.
    buf.extend_from_slice(&[0x12, 0x34, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    // Encode labels.
    for label in name.split('.') {
        let bytes = label.as_bytes();
        let len = bytes.len().min(63);
        buf.push(len as u8);
        buf.extend_from_slice(&bytes[..len]);
    }
    buf.push(0x00); // Root label.
    // QTYPE + QCLASS IN.
    buf.extend_from_slice(&qtype.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes());
    buf
}

/// Benchmark TCP connection table scan.
///
/// Calls `all_connections()` which locks the CONNECTIONS table and
/// scans all 32 entries collecting active connection info.  This is
/// the same lock+scan path that `process_tcp_common()` uses to find
/// matching connections for incoming segments.
fn bench_net_tcp_conn_lookup() {
    use crate::net::tcp;

    let result = run("net_tcp_conn_table_scan", 2000, || {
        let _ = core::hint::black_box(tcp::all_connections());
    });

    serial_println!(
        "[bench]   net_tcp_conn_table_scan: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_tcp_conn_lookup", &result, 3000);
}

/// Benchmark veth pair send (TX → peer RX enqueue).
///
/// Creates a veth pair, brings both ends up, then measures the cost of
/// sending a minimal Ethernet frame from end A to end B.  This is the
/// hot path for container-to-host networking: lock TABLE → validate
/// state → record TX stats → enqueue on peer RX.
///
/// Between iterations we drain the peer's RX queue every 64 frames to
/// avoid hitting the VETH_QUEUE_DEPTH limit.
fn bench_net_veth_send() {
    use crate::net::veth;

    if !veth::is_initialized() {
        serial_println!("[bench]   net_veth_send: SKIPPED (veth not initialized)");
        return;
    }

    // Create a pair and bring both ends up.
    let pair_id = match veth::create_pair() {
        Ok(id) => id,
        Err(_) => {
            serial_println!("[bench]   net_veth_send: SKIPPED (could not create pair)");
            return;
        }
    };
    let _ = veth::set_up(pair_id, veth::VethEndId::A, true);
    let _ = veth::set_up(pair_id, veth::VethEndId::B, true);

    // Minimal valid Ethernet frame (14-byte header + 46-byte payload = 60 bytes).
    let frame_template: alloc::vec::Vec<u8> = {
        let mut f = alloc::vec![0u8; 60];
        // Dst MAC (broadcast).
        f[0] = 0xFF; f[1] = 0xFF; f[2] = 0xFF;
        f[3] = 0xFF; f[4] = 0xFF; f[5] = 0xFF;
        // Src MAC (arbitrary locally-administered).
        f[6] = 0x02; f[7] = 0x00; f[8] = 0x00;
        f[9] = 0x00; f[10] = 0x00; f[11] = 0x01;
        // EtherType: IPv4 (0x0800).
        f[12] = 0x08; f[13] = 0x00;
        f
    };

    let mut drain_counter: u32 = 0;
    let result = run("net_veth_send", 2000, || {
        let frame = frame_template.clone();
        let _ = core::hint::black_box(
            veth::send(pair_id, veth::VethEndId::A, frame)
        );
        drain_counter = drain_counter.wrapping_add(1);
        if drain_counter & 63 == 0 {
            // Drain to keep the queue from filling up.
            while veth::recv(pair_id, veth::VethEndId::B).is_some() {}
        }
    });

    // Drain remaining frames.
    while veth::recv(pair_id, veth::VethEndId::B).is_some() {}

    // Cleanup.
    let _ = veth::set_up(pair_id, veth::VethEndId::A, false);
    let _ = veth::set_up(pair_id, veth::VethEndId::B, false);
    let _ = veth::destroy_pair(pair_id);

    serial_println!(
        "[bench]   net_veth_send: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_veth_send", &result, 2000);
}

/// Benchmark veth pair recv (dequeue from RX queue).
///
/// Pre-fills one endpoint's RX queue with frames, then measures the
/// cost of dequeuing them one at a time.  This is the other half of
/// the veth data path: lock TABLE → find pair/end → pop_front.
fn bench_net_veth_recv() {
    use crate::net::veth;

    if !veth::is_initialized() {
        serial_println!("[bench]   net_veth_recv: SKIPPED (veth not initialized)");
        return;
    }

    let pair_id = match veth::create_pair() {
        Ok(id) => id,
        Err(_) => {
            serial_println!("[bench]   net_veth_recv: SKIPPED (could not create pair)");
            return;
        }
    };
    let _ = veth::set_up(pair_id, veth::VethEndId::A, true);
    let _ = veth::set_up(pair_id, veth::VethEndId::B, true);

    // Minimal Ethernet frame.
    let frame_template: alloc::vec::Vec<u8> = {
        let mut f = alloc::vec![0u8; 60];
        f[0] = 0xFF; f[1] = 0xFF; f[2] = 0xFF;
        f[3] = 0xFF; f[4] = 0xFF; f[5] = 0xFF;
        f[6] = 0x02; f[12] = 0x08;
        f
    };

    // We need to keep the queue topped up.  Strategy: pre-fill before
    // each batch of measurements, then measure dequeue cost.
    // The `run()` harness does warmup+measured iterations.  We pre-fill
    // the queue before calling run and refill periodically.
    let mut refill_counter: u32 = 0;
    let result = run("net_veth_recv", 2000, || {
        // Re-fill if queue is empty (checked every iteration to ensure
        // we always have something to dequeue).
        refill_counter = refill_counter.wrapping_add(1);
        if refill_counter & 63 == 0 || refill_counter <= 1 {
            // Push up to 128 frames.
            for _ in 0..128 {
                let frame = frame_template.clone();
                if veth::send(pair_id, veth::VethEndId::A, frame).is_err() {
                    break;
                }
            }
        }
        let _ = core::hint::black_box(
            veth::recv(pair_id, veth::VethEndId::B)
        );
    });

    // Cleanup.
    while veth::recv(pair_id, veth::VethEndId::B).is_some() {}
    let _ = veth::set_up(pair_id, veth::VethEndId::A, false);
    let _ = veth::set_up(pair_id, veth::VethEndId::B, false);
    let _ = veth::destroy_pair(pair_id);

    serial_println!(
        "[bench]   net_veth_recv: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_veth_recv", &result, 1500);
}

/// Benchmark veth send+recv round-trip (TX on A → RX on B).
///
/// Measures the complete data path for a single frame traversing a
/// veth pair: send on A enqueues on B, recv on B dequeues.  This is
/// the full cost of a single packet crossing from one namespace to
/// another.
fn bench_net_veth_roundtrip() {
    use crate::net::veth;

    if !veth::is_initialized() {
        serial_println!("[bench]   net_veth_roundtrip: SKIPPED (veth not initialized)");
        return;
    }

    let pair_id = match veth::create_pair() {
        Ok(id) => id,
        Err(_) => {
            serial_println!("[bench]   net_veth_roundtrip: SKIPPED (could not create pair)");
            return;
        }
    };
    let _ = veth::set_up(pair_id, veth::VethEndId::A, true);
    let _ = veth::set_up(pair_id, veth::VethEndId::B, true);

    let frame_template: alloc::vec::Vec<u8> = {
        let mut f = alloc::vec![0u8; 60];
        f[0] = 0xFF; f[1] = 0xFF; f[2] = 0xFF;
        f[3] = 0xFF; f[4] = 0xFF; f[5] = 0xFF;
        f[6] = 0x02; f[12] = 0x08;
        f
    };

    let result = run("net_veth_roundtrip", 2000, || {
        let frame = frame_template.clone();
        let _ = veth::send(pair_id, veth::VethEndId::A, frame);
        let _ = core::hint::black_box(
            veth::recv(pair_id, veth::VethEndId::B)
        );
    });

    // Cleanup.
    let _ = veth::set_up(pair_id, veth::VethEndId::A, false);
    let _ = veth::set_up(pair_id, veth::VethEndId::B, false);
    let _ = veth::destroy_pair(pair_id);

    serial_println!(
        "[bench]   net_veth_roundtrip: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_veth_roundtrip", &result, 3500);
}

/// Benchmark per-namespace ARP cache lookup.
///
/// Measures the cost of looking up an IP in a non-root namespace's ARP
/// cache.  This is the critical path for container packet forwarding:
/// the namespace needs to resolve a destination MAC before it can send
/// a frame on its veth endpoint.
fn bench_net_ns_arp_lookup() {
    use crate::net::arp;

    if !crate::netns::is_initialized() {
        serial_println!("[bench]   net_ns_arp_lookup: SKIPPED (netns not initialized)");
        return;
    }

    // Create a temporary namespace.
    let ns_id = match crate::netns::create() {
        Ok(id) => id,
        Err(_) => {
            serial_println!("[bench]   net_ns_arp_lookup: SKIPPED (could not create ns)");
            return;
        }
    };

    // Initialize per-namespace ARP cache and seed it. A failure here means
    // the ns just created above is unusable — skip the bench rather than
    // continuing with a half-initialized state.
    if let Err(e) = arp::ns_init(ns_id) {
        serial_println!("[bench]   net_ns_arp_lookup: SKIPPED (arp::ns_init failed: {:?})", e);
        return;
    }
    let target_ip = crate::net::interface::Ipv4Addr([10, 0, 0, 1]);
    let target_mac = crate::virtio::net::MacAddress([0x02, 0x00, 0x00, 0x00, 0xBE, 0x01]);
    arp::ns_insert(ns_id, target_ip, target_mac);

    let result = run("net_ns_arp_lookup", 2000, || {
        let _ = core::hint::black_box(arp::ns_lookup(ns_id, target_ip));
    });

    // Cleanup.
    arp::ns_destroy(ns_id);
    let _ = crate::netns::delete(ns_id);

    serial_println!(
        "[bench]   net_ns_arp_lookup: min {}ns ({}cycles)",
        result.min_ns, result.min_cycles
    );
    score("net_ns_arp_lookup", &result, 1000);
}

// ---------------------------------------------------------------------------
// Cryptographic benchmarks
// ---------------------------------------------------------------------------

/// SHA-256 on a 64-byte input (common: TLS record MAC, file hashing).
fn bench_crypto_sha256_64() {
    use crate::crypto;

    let data = [0xABu8; 64];
    let result = run("crypto_sha256_64B", 2000, || {
        let _ = core::hint::black_box(crypto::sha256(core::hint::black_box(&data)));
    });

    // OpenSSL SHA-256 64B: ~200ns.  QEMU target: 5000ns (25x overhead).
    score("crypto_sha256_64B", &result, 5000);
    serial_println!(
        "[bench]   crypto_sha256_64B: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
}

/// SHA-256 on a 1 KiB input (file content hashing, integrity checks).
fn bench_crypto_sha256_1k() {
    use crate::crypto;

    let data = [0xCDu8; 1024];
    let result = run("crypto_sha256_1KiB", 1000, || {
        let _ = core::hint::black_box(crypto::sha256(core::hint::black_box(&data)));
    });

    // OpenSSL SHA-256 1KiB: ~1500ns.  QEMU target: 50000ns.
    score("crypto_sha256_1KiB", &result, 50000);
    serial_println!(
        "[bench]   crypto_sha256_1KiB: min {}ns ({}cy)  [{} MiB/s]",
        result.min_ns, result.min_cycles,
        if result.min_ns > 0 { 1_000_000_000u64 / result.min_ns * 1024 / (1024 * 1024) } else { 0 }
    );
}

/// SHA-512 on a 64-byte input (Ed25519 key derivation, per-signature).
fn bench_crypto_sha512_64() {
    use crate::crypto;

    let data = [0xEFu8; 64];
    let result = run("crypto_sha512_64B", 2000, || {
        let _ = core::hint::black_box(crypto::sha512(core::hint::black_box(&data)));
    });

    serial_println!(
        "[bench]   crypto_sha512_64B: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    score("crypto_sha512_64B", &result, 6000);
}

/// HMAC-SHA256 with 32-byte key and 64-byte message (TLS Finished, HKDF).
fn bench_crypto_hmac_sha256() {
    use crate::crypto;

    let key = [0x01u8; 32];
    let msg = [0x02u8; 64];
    let result = run("crypto_hmac_sha256", 2000, || {
        let _ = core::hint::black_box(crypto::hmac_sha256(
            core::hint::black_box(&key),
            core::hint::black_box(&msg),
        ));
    });

    serial_println!(
        "[bench]   crypto_hmac_sha256: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    score("crypto_hmac_sha256", &result, 15000);
}

/// ChaCha20 encryption of 1 KiB (TLS/SSH bulk data encryption).
fn bench_crypto_chacha20_1k() {
    use crate::crypto;

    let key = [0x03u8; 32];
    let nonce = [0x04u8; 12];
    let mut buf = [0x55u8; 1024];
    let result = run("crypto_chacha20_1KiB", 1000, || {
        crypto::chacha20_xor(
            core::hint::black_box(&key),
            core::hint::black_box(&nonce),
            0,
            core::hint::black_box(&mut buf),
        );
    });

    serial_println!(
        "[bench]   crypto_chacha20_1KiB: min {}ns ({}cy)  [{} MiB/s]",
        result.min_ns, result.min_cycles,
        if result.min_ns > 0 { 1_000_000_000u64 / result.min_ns * 1024 / (1024 * 1024) } else { 0 }
    );
    score("crypto_chacha20_1KiB", &result, 40000);
}

/// Poly1305 MAC of 1 KiB (TLS/SSH authentication tag).
fn bench_crypto_poly1305_1k() {
    use crate::crypto;

    let key = [0x05u8; 32];
    let data = [0xAAu8; 1024];
    let result = run("crypto_poly1305_1KiB", 1000, || {
        let _ = core::hint::black_box(crypto::poly1305(
            core::hint::black_box(&key),
            core::hint::black_box(&data),
        ));
    });

    serial_println!(
        "[bench]   crypto_poly1305_1KiB: min {}ns ({}cy)  [{} MiB/s]",
        result.min_ns, result.min_cycles,
        if result.min_ns > 0 { 1_000_000_000u64 / result.min_ns * 1024 / (1024 * 1024) } else { 0 }
    );
    score("crypto_poly1305_1KiB", &result, 30000);
}

/// ChaCha20-Poly1305 AEAD encrypt of 1 KiB (TLS 1.3 / SSH record layer).
///
/// This is the combined cipher used for every TLS record and SSH packet.
/// It measures the full encrypt+MAC pipeline.
fn bench_crypto_chacha20_poly1305_1k() {
    use crate::crypto;

    let key = [0x06u8; 32];
    let nonce = [0x07u8; 12];
    let aad = [0x08u8; 13]; // Typical TLS record header.
    let mut buf = [0xBBu8; 1024];

    let result = run("crypto_aead_1KiB", 500, || {
        // Reset plaintext each iteration (encrypt is in-place).
        for b in buf.iter_mut() { *b = 0xBB; }
        let _ = core::hint::black_box(crypto::chacha20_poly1305_encrypt(
            core::hint::black_box(&key),
            core::hint::black_box(&nonce),
            core::hint::black_box(&aad),
            core::hint::black_box(&mut buf),
        ));
    });

    // OpenSSL chacha20-poly1305 1KiB: ~2000ns.  QEMU target: 100000ns.
    score("crypto_aead_1KiB", &result, 100_000);
    serial_println!(
        "[bench]   crypto_aead_1KiB: min {}ns ({}cy)  [{} MiB/s]",
        result.min_ns, result.min_cycles,
        if result.min_ns > 0 { 1_000_000_000u64 / result.min_ns * 1024 / (1024 * 1024) } else { 0 }
    );
}

/// X25519 Diffie-Hellman key exchange (one scalar multiplication).
///
/// This runs once per TLS handshake and once per SSH key exchange.
/// Not a hot path, but establishes the baseline for connection setup
/// latency.  Uses basepoint multiplication (public key derivation).
fn bench_crypto_x25519() {
    use crate::crypto;

    // Use a fixed scalar to avoid RNG cost in the measurement.
    let scalar: [u8; 32] = {
        let mut s = [0u8; 32];
        for (i, b) in s.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(0x42);
        }
        s[0] &= 248;
        s[31] &= 127;
        s[31] |= 64;
        s
    };

    let result = run("crypto_x25519", 100, || {
        let _ = core::hint::black_box(crypto::x25519_base(
            core::hint::black_box(&scalar),
        ));
    });

    serial_println!(
        "[bench]   crypto_x25519: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 2_000_000ns (2ms per key exchange).
    score("crypto_x25519", &result, 2_000_000);
}

/// Ed25519 signature generation (per SSH auth, per signed message).
///
/// Includes two SHA-512 hashes plus scalar multiplication — the most
/// expensive per-connection operation for SSH public key authentication.
fn bench_crypto_ed25519_sign() {
    use crate::crypto;

    let seed = [0x09u8; 32];
    let message = [0xCCu8; 128];

    let result = run("crypto_ed25519_sign", 50, || {
        let _ = core::hint::black_box(crypto::ed25519_sign(
            core::hint::black_box(&seed),
            core::hint::black_box(&message),
        ));
    });

    serial_println!(
        "[bench]   crypto_ed25519_sign: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 5_000_000ns (5ms per signature).
    score("crypto_ed25519_sign", &result, 5_000_000);
}

/// Ed25519 signature verification (per SSH host key check, per cert verify).
///
/// The costliest single operation in a TLS or SSH handshake — includes
/// point decompression, two scalar multiplications, and SHA-512.
fn bench_crypto_ed25519_verify() {
    use crate::crypto;

    let seed = [0x0Au8; 32];
    let message = [0xDDu8; 128];

    // Pre-compute a valid signature to verify.
    let pubkey = crypto::ed25519_public_key(&seed);
    let sig = crypto::ed25519_sign(&seed, &message);

    let result = run("crypto_ed25519_verify", 50, || {
        let _ = core::hint::black_box(crypto::ed25519_verify(
            core::hint::black_box(&pubkey),
            core::hint::black_box(&message),
            core::hint::black_box(&sig),
        ));
    });

    serial_println!(
        "[bench]   crypto_ed25519_verify: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 10_000_000ns (10ms per verify).
    score("crypto_ed25519_verify", &result, 10_000_000);
}

// ---------------------------------------------------------------------------
// VFS deep-path and throughput benchmarks (fs zone)
// ---------------------------------------------------------------------------

/// Benchmark VFS stat on a multi-component path.
///
/// Measures the cost of resolving "/proc/meminfo" — a 2-component path
/// that traverses the VFS mount table, descends into the procfs mount,
/// and does a final filename lookup.  This captures the per-component
/// traversal cost better than stat("/").
///
/// The design spec says Linux cached lookup is ~200-500ns per component.
/// With 2 components, expect 2× the single-component cost.
fn bench_vfs_stat_deep() {
    use crate::fs::vfs::Vfs;

    // /proc/meminfo should exist if procfs is mounted (it is by boot time).
    if Vfs::stat("/proc/meminfo").is_err() {
        serial_println!("[bench] vfs_stat_deep: SKIP (/proc/meminfo not available)");
        return;
    }

    let result = run("vfs_stat_deep_2comp", 500, || {
        let _ = core::hint::black_box(Vfs::stat("/proc/meminfo"));
    });

    let target_ns = 1400u64; // 2 components × 700ns target
    score("vfs_stat_deep", &result, target_ns);
    if result.min_ns <= target_ns {
        serial_println!(
            "[bench]   vfs_stat_deep_2comp: PASS (min {}ns <= target {}ns)",
            result.min_ns, target_ns
        );
    } else {
        serial_println!(
            "[bench]   vfs_stat_deep_2comp: ABOVE TARGET (min {}ns > target {}ns, per-component ~{}ns)",
            result.min_ns, target_ns, result.min_ns / 2
        );
    }
}

/// Benchmark VFS stat on a 3-component path.
///
/// Uses "/proc/net/tcp" to measure the cost of 3-level path resolution.
/// If that path doesn't exist, falls back to creating a temporary
/// 3-level directory structure.
fn bench_vfs_stat_3comp() {
    use crate::fs::vfs::Vfs;

    // Try to use an existing deep path first.
    let path = "/proc/sched/stats";
    let alt_path = "/proc/meminfo"; // fallback: 2-component

    let test_path = if Vfs::stat(path).is_ok() {
        path
    } else {
        // Create a temporary 3-level path for the benchmark.
        let dir = "/bench_deep_dir";
        let subdir = "/bench_deep_dir/sub";
        let file = "/bench_deep_dir/sub/testfile";
        if Vfs::mkdir(dir).is_ok() {
            let _ = Vfs::mkdir(subdir);
            let _ = Vfs::write_file(file, b"bench");
            if Vfs::stat(file).is_ok() {
                file
            } else {
                // Clean up and skip.
                let _ = Vfs::remove(file);
                let _ = Vfs::remove(subdir);
                let _ = Vfs::remove(dir);
                if Vfs::stat(alt_path).is_ok() {
                    alt_path
                } else {
                    serial_println!("[bench] vfs_stat_3comp: SKIP (no deep path available)");
                    return;
                }
            }
        } else if Vfs::stat(alt_path).is_ok() {
            alt_path
        } else {
            serial_println!("[bench] vfs_stat_3comp: SKIP (no paths available)");
            return;
        }
    };

    let components = test_path.matches('/').count(); // approximate
    let result = run("vfs_stat_3comp", 500, || {
        let _ = core::hint::black_box(Vfs::stat(test_path));
    });

    // 3 components × 500ns target = 1500ns (design spec: ≤500ns/component).
    let target_3comp = 2100u64; // 3 × 700ns accounting for QEMU overhead
    score("vfs_stat_3comp", &result, target_3comp);
    serial_println!(
        "[bench]   vfs_stat_3comp ({}comp, \"{}\"): min {}ns ({}ns/component)",
        components, test_path, result.min_ns, result.min_ns / components as u64
    );

    // Clean up temporary files if we created them.
    let _ = Vfs::remove("/bench_deep_dir/sub/testfile");
    let _ = Vfs::remove("/bench_deep_dir/sub");
    let _ = Vfs::remove("/bench_deep_dir");
}

/// Benchmark VFS sequential write throughput (4 KiB chunks).
///
/// Writes a 16 KiB file in a single call, then reads it back.
/// Measures the throughput for the common file I/O pattern.
fn bench_vfs_throughput_16k() {
    use crate::fs::vfs::Vfs;

    // 16 KiB of pattern data (one full page).  Heap-allocated rather than a
    // `[u8; 16384]` stack array: this benchmark runs in the deferred bench
    // task, whose 64 KiB kernel stack is marginal (see B-DF1), and a 16 KiB
    // stack frame here needlessly cuts headroom for an ill-timed interrupt.
    let mut data = alloc::vec![0u8; 16384];
    for (i, b) in data.iter_mut().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        { *b = ((i * 7 + 13) & 0xFF) as u8; }
    }

    let path = "/bench_throughput_16k.tmp";

    // Verify VFS write works.
    if Vfs::write_file(path, &data).is_err() {
        serial_println!("[bench] vfs_throughput_16k: SKIP (VFS write not available)");
        return;
    }

    // Benchmark write.
    let write_result = run("vfs_write_16k", 100, || {
        let _ = core::hint::black_box(Vfs::write_file(path, &data));
    });

    // Benchmark read.
    let read_result = run("vfs_read_16k", 100, || {
        let _ = core::hint::black_box(Vfs::read_file(path));
    });

    // Throughput: 16 KiB / time_ns * 1e9 / 1e6 = MiB/s
    let write_mibs = if write_result.min_ns > 0 {
        16384u64.saturating_mul(1_000) / write_result.min_ns
    } else { 0 };
    let read_mibs = if read_result.min_ns > 0 {
        16384u64.saturating_mul(1_000) / read_result.min_ns
    } else { 0 };

    serial_println!(
        "[bench]   vfs_write_16k: min {}ns (~{} MiB/s), vfs_read_16k: min {}ns (~{} MiB/s)",
        write_result.min_ns, write_mibs, read_result.min_ns, read_mibs
    );
    score("vfs_throughput_16k_write", &write_result, 50000);
    score("vfs_throughput_16k_read", &read_result, 50000);

    // Clean up.
    let _ = Vfs::remove(path);
}

// ---------------------------------------------------------------------------
// HTTP server benchmarks (net zone)
// ---------------------------------------------------------------------------

/// Benchmark HTTP request parsing.
///
/// Measures the cost of parsing a typical GET request from raw bytes.
/// This is the entry point for every HTTP/HTTPS request served.
fn bench_http_parse_request() {
    use crate::net::httpd;

    // Typical browser GET request (~200 bytes).
    let raw_request = b"GET /index.html HTTP/1.1\r\n\
        Host: 10.0.2.15\r\n\
        User-Agent: Mozilla/5.0\r\n\
        Accept: text/html\r\n\
        Connection: keep-alive\r\n\
        \r\n";

    let result = run("http_parse_request", 1000, || {
        let _ = core::hint::black_box(httpd::bench_parse_request(raw_request));
    });

    serial_println!(
        "[bench]   http_parse_request: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    // Target from baselines.toml: 15000ns (dominated by string allocations).
    score("http_parse_request", &result, 15000);
}

/// Benchmark HTTP MIME type detection.
///
/// Measures the cost of determining the MIME type from a file extension.
/// This runs once per served file.
fn bench_http_mime_type() {
    use crate::net::httpd;

    let result = run("http_mime_type", 2000, || {
        let _ = core::hint::black_box(httpd::bench_mime_type("/styles/main.css"));
        let _ = core::hint::black_box(httpd::bench_mime_type("/app.js"));
        let _ = core::hint::black_box(httpd::bench_mime_type("/photo.png"));
        let _ = core::hint::black_box(httpd::bench_mime_type("/data.json"));
    });

    serial_println!(
        "[bench]   http_mime_type (4 lookups): min {}ns ({}cy, ~{}ns/lookup)",
        result.min_ns, result.min_cycles, result.min_ns / 4
    );
    // Benchmark does 4 lookups; target 500ns per lookup = 2000ns total.
    score("http_mime_type", &result, 2000);
}

/// Benchmark HTTP percent-decode path.
///
/// Measures the cost of decoding a URL path with percent-encoded
/// characters.  This runs on every request URI.
fn bench_http_percent_decode() {
    use crate::net::httpd;

    // Path with several percent-encoded characters (spaces, etc.).
    let encoded = "/path%20to/my%20file%20%28copy%29.txt";

    let result = run("http_percent_decode", 2000, || {
        let _ = core::hint::black_box(httpd::bench_percent_decode(encoded));
    });

    serial_println!(
        "[bench]   http_percent_decode: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    score("http_percent_decode", &result, 20000);
}

/// Benchmark gzip compression for 1 KiB HTML-like content.
///
/// Measures the time to gzip-compress a typical HTML page body, which
/// is now on the HTTP response hot path when clients send Accept-Encoding.
fn bench_http_gzip_1k() {
    use crate::fs::compress;

    // Build a 1 KiB body that resembles HTML (varied text).
    let mut body = Vec::with_capacity(1024);
    for _ in 0..16 {
        body.extend_from_slice(b"<div class=\"item\"><h3>Title</h3><p>Content goes here.</p></div>\n");
    }
    // Truncate or pad to exactly 1024 bytes.
    body.truncate(1024);
    while body.len() < 1024 {
        body.push(b' ');
    }

    let result = run("http_gzip_1KiB", 500, || {
        let _ = core::hint::black_box(compress::gzip(&body));
    });

    // Report compressed size for reference.
    let compressed = compress::gzip(&body);
    serial_println!(
        "[bench]   http_gzip_1KiB: min {}ns ({}cy), {}B → {}B",
        result.min_ns, result.min_cycles, body.len(), compressed.len()
    );
    // Target: 200us — gzip is expensive but only runs once per response.
    score("http_gzip_1KiB", &result, 200_000);
}

/// Benchmark gzip compression for 8 KiB dashboard HTML.
///
/// The dashboard HTML is ~10 KiB, so this measures a realistic
/// compression workload for the auto-refresh API.
fn bench_http_gzip_8k() {
    use crate::fs::compress;

    // Build an 8 KiB body with JSON-like content.
    let mut body = Vec::with_capacity(8192);
    for i in 0..128u32 {
        let line = alloc::format!(
            r#"{{"id":{},"name":"task_{}","state":"running","cpu":0,"ticks":{}}}"#,
            i, i, i.saturating_mul(100)
        );
        body.extend_from_slice(line.as_bytes());
        body.push(b'\n');
    }
    body.truncate(8192);

    let result = run("http_gzip_8KiB", 200, || {
        let _ = core::hint::black_box(compress::gzip(&body));
    });

    let compressed = compress::gzip(&body);
    serial_println!(
        "[bench]   http_gzip_8KiB: min {}ns ({}cy), {}B → {}B",
        result.min_ns, result.min_cycles, body.len(), compressed.len()
    );
    // Target: 1ms — larger content takes proportionally longer.
    score("http_gzip_8KiB", &result, 1_000_000);
}

/// Benchmark HTTP ETag computation.
///
/// Measures the FNV-1a hash + hex formatting that runs on every response.
/// This is on the critical path for both plain and gzip responses.
fn bench_http_etag() {
    use crate::net::httpd;

    // 4 KiB body — typical small page or JSON API response.
    let body: Vec<u8> = (0u8..=255).cycle().take(4096).collect();

    let result = run("http_etag_4KiB", 2000, || {
        let _ = core::hint::black_box(httpd::bench_etag(&body));
    });

    serial_println!(
        "[bench]   http_etag_4KiB: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    // Target: 5000ns — FNV-1a over 4 KiB + hex format + String alloc.
    score("http_etag_4KiB", &result, 5000);
}

/// Benchmark full HTTP response construction (headers + body, no gzip).
///
/// Measures the complete response building path: ETag hash, header
/// formatting via format!(), Vec concatenation.  This is the code path
/// for every non-compressed response served.
fn bench_http_build_response() {
    use crate::net::httpd;

    // Build a 1 KiB HTML body — typical small page.
    let body: Vec<u8> = b"<html><body><h1>Hello</h1><p>World</p></body></html>\n"
        .iter()
        .cycle()
        .take(1024)
        .copied()
        .collect();

    let result = run("http_build_response_1KiB", 1000, || {
        let _ = core::hint::black_box(httpd::bench_build_response(&body));
    });

    serial_println!(
        "[bench]   http_build_response_1KiB: min {}ns ({}cy)",
        result.min_ns, result.min_cycles
    );
    // Target: 20000ns — dominated by format!() header + ETag hash + Vec::extend.
    score("http_build_response_1KiB", &result, 20000);
}

/// Benchmark full gzip-compressed HTTP response construction.
///
/// Measures the complete compressed response path: gzip compression,
/// ETag hash (on original body), header formatting, Vec concatenation.
/// This is the hot path for text/html and application/json responses
/// when the client sends Accept-Encoding: gzip.
fn bench_http_build_response_gzip() {
    use crate::net::httpd;

    // 1 KiB HTML body (same as build_response benchmark for comparison).
    let body: Vec<u8> = b"<html><body><h1>Hello</h1><p>World</p></body></html>\n"
        .iter()
        .cycle()
        .take(1024)
        .copied()
        .collect();

    let result = run("http_build_response_gzip_1KiB", 500, || {
        let _ = core::hint::black_box(httpd::bench_build_response_gzip(&body));
    });

    // Report response sizes.
    let plain = httpd::bench_build_response(&body);
    let gzip = httpd::bench_build_response_gzip(&body);
    serial_println!(
        "[bench]   http_build_response_gzip_1KiB: min {}ns ({}cy), plain {}B vs gzip {}B",
        result.min_ns, result.min_cycles, plain.len(), gzip.len()
    );
    // Target: 250000ns — gzip dominates (200us) + response building (~20us).
    score("http_build_response_gzip_1KiB", &result, 250_000);
}

// ---------------------------------------------------------------------------
// Dashboard API benchmarks (net zone)
// ---------------------------------------------------------------------------

/// Benchmark /api/status JSON generation.
///
/// Measures the cost of collecting system state (uptime, memory, CPU count,
/// task count, scheduler ticks) and formatting it as JSON.  This endpoint
/// is polled every 3 seconds by the dashboard auto-refresh.
fn bench_dashboard_api_status() {
    use crate::net::dashboard;

    let result = run("dashboard_api_status", 1000, || {
        let _ = core::hint::black_box(dashboard::bench_api_status());
    });

    serial_println!(
        "[bench]   dashboard_api_status: min {}ns ({}cy), ~{}B",
        result.min_ns, result.min_cycles,
        dashboard::bench_api_status().len()
    );
    // Target: 10000ns — a few atomic reads + format!() JSON.
    score("dashboard_api_status", &result, 10000);
}

/// Benchmark /api/health JSON generation.
///
/// Measures the cost of the aggregated health check that queries memory,
/// networking, HTTP server, and DNS subsystems to produce an overall
/// health status (ok/degraded/critical).
fn bench_dashboard_api_health() {
    use crate::net::dashboard;

    let result = run("dashboard_api_health", 1000, || {
        let _ = core::hint::black_box(dashboard::bench_api_health());
    });

    serial_println!(
        "[bench]   dashboard_api_health: min {}ns ({}cy), ~{}B",
        result.min_ns, result.min_cycles,
        dashboard::bench_api_health().len()
    );
    // Target: 15000ns — queries several subsystems + JSON format.
    score("dashboard_api_health", &result, 15000);
}

/// Benchmark /metrics Prometheus text exposition format generation.
///
/// Measures the cost of formatting ~50 Prometheus metrics (including
/// per-CPU labeled metrics) with TYPE and HELP annotations.  This is
/// polled by monitoring stacks (Prometheus, Grafana, etc.).
fn bench_dashboard_api_metrics() {
    use crate::net::dashboard;

    let result = run("dashboard_api_metrics", 500, || {
        let _ = core::hint::black_box(dashboard::bench_api_metrics());
    });

    serial_println!(
        "[bench]   dashboard_api_metrics: min {}ns ({}cy), ~{}B",
        result.min_ns, result.min_cycles,
        dashboard::bench_api_metrics().len()
    );
    // Target: 55000ns — ~50 metrics with per-CPU labels, TCP stats, swap,
    // scheduler stats, block cache, firewall.  Raised from 50000ns after
    // adding 8 block cache metric families.
    score("dashboard_api_metrics", &result, 55000);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify the benchmark infrastructure works.
pub fn self_test() {
    serial_println!("[bench] Running self-test...");

    // TSC should be calibrated.
    let freq = tsc_freq();
    assert!(freq > 0, "TSC frequency should be calibrated");
    serial_println!("[bench]   TSC frequency: {} Hz", freq);

    // TSC should advance.
    let t1 = rdtsc();
    for _ in 0..1000 {
        core::hint::black_box(0);
    }
    let t2 = rdtsc();
    assert!(t2 > t1, "TSC should advance over time");
    serial_println!("[bench]   TSC advancing: OK (delta={})", t2.saturating_sub(t1));

    // Cycle-to-ns conversion should be reasonable.
    let ns = cycles_to_ns(freq);
    // freq cycles = 1 second = 1_000_000_000 ns.
    assert!(
        ns >= 900_000_000 && ns <= 1_100_000_000,
        "1 second of cycles should convert to ~1e9 ns, got {}",
        ns
    );
    serial_println!("[bench]   cycles_to_ns: OK ({}Hz → {}ns)", freq, ns);

    // Run a trivial benchmark.
    let result = run("self_test_nop", 1000, || {
        core::hint::black_box(42);
    });
    assert!(result.min_cycles < 10000, "NOP benchmark should be very fast");
    serial_println!("[bench]   Benchmark runner: OK");

    serial_println!("[bench] Self-test PASSED");
}
