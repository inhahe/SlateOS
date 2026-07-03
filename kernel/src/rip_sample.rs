//! RIP sampler — statistical profiler via timer interrupt sampling.
//!
//! Records the instruction pointer (RIP) at each timer interrupt to build
//! a histogram of where CPU time is spent.  This is the kernel equivalent
//! of `perf record` — a lightweight statistical profiler.
//!
//! ## Design
//!
//! - On each timer tick (100 Hz), if sampling is enabled, the current RIP
//!   from the interrupt frame is recorded in a ring buffer.
//! - Samples are bucketed by address range (kernel text, heap, stack, user)
//!   for high-level classification.
//! - The raw ring buffer retains the last 256 samples for detailed analysis.
//! - Address-to-symbol resolution uses the ksyms module when available.
//!
//! ## Overhead
//!
//! When enabled: one memory write per timer tick (~10ns).  Negligible.
//! When disabled: single atomic load check (zero overhead).
//!
//! ## Usage
//!
//! ```text
//! kshell> ripsample on      — start sampling
//! kshell> ... do work ...
//! kshell> ripsample off     — stop sampling
//! kshell> ripsample         — show where CPU spent time
//! kshell> ripsample top     — show hottest addresses
//! kshell> ripsample reset   — clear all samples
//! ```
//!
//! ## References
//!
//! - Linux perf (perf_event_open with PERF_SAMPLE_IP) — hardware PMU sampling
//! - OProfile — statistical kernel profiler
//! - Intel VTune — sampling-based hot spot analysis
//! - DTrace profile provider — timed interrupt sampling

// Diagnostic/profiling subsystem — all public API for tooling and kshell
// commands; many helpers may not have call sites in production paths yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Ring buffer size for raw RIP samples.
const RING_SIZE: usize = 256;
const RING_MASK: usize = RING_SIZE - 1;

/// Number of address buckets for classification.
const NUM_BUCKETS: usize = 8;

// ---------------------------------------------------------------------------
// Address classification
// ---------------------------------------------------------------------------

/// Classification of a sampled RIP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AddrClass {
    /// Kernel text (code section).
    KernelText = 0,
    /// Kernel heap region.
    KernelHeap = 1,
    /// Kernel stack.
    KernelStack = 2,
    /// HHDM (physical memory access via direct map).
    Hhdm = 3,
    /// Userspace code.
    UserCode = 4,
    /// Idle loop.
    Idle = 5,
    /// Interrupt handler / ISR.
    Isr = 6,
    /// Unknown / other.
    Other = 7,
}

impl AddrClass {
    pub fn name(self) -> &'static str {
        match self {
            Self::KernelText => "kernel_text",
            Self::KernelHeap => "kernel_heap",
            Self::KernelStack => "kernel_stack",
            Self::Hhdm => "hhdm",
            Self::UserCode => "user",
            Self::Idle => "idle",
            Self::Isr => "isr",
            Self::Other => "other",
        }
    }

    /// Classify a RIP address.
    pub fn classify(rip: u64) -> Self {
        // Kernel text: typically 0xffffffff80000000..0xffffffff80xxxxxx
        if rip >= 0xffff_ffff_8000_0000 && rip < 0xffff_ffff_a000_0000 {
            return Self::KernelText;
        }
        // HHDM: 0xffff800000000000..0xffffc00000000000 (256 TiB HHDM window)
        if rip >= 0xffff_8000_0000_0000 && rip < 0xffff_c000_0000_0000 {
            return Self::Hhdm;
        }
        // Kernel heap/stacks are in higher half but below kernel text
        if rip >= 0xffff_c000_0000_0000 && rip < 0xffff_ffff_8000_0000 {
            return Self::KernelHeap;
        }
        // User space: below 0x0000800000000000
        if rip < 0x0000_8000_0000_0000 {
            return Self::UserCode;
        }
        Self::Other
    }
}

// ---------------------------------------------------------------------------
// Sample data
// ---------------------------------------------------------------------------

/// A single RIP sample.
#[derive(Debug, Clone, Copy)]
pub struct RipSample {
    /// Instruction pointer value.
    pub rip: u64,
    /// CPU that was sampled.
    pub cpu: u8,
    /// Classification.
    pub class: u8,
}

impl RipSample {
    pub const fn empty() -> Self {
        Self { rip: 0, cpu: 0, class: 0 }
    }

    pub fn is_valid(&self) -> bool {
        self.rip != 0
    }

    pub fn addr_class(&self) -> AddrClass {
        AddrClass::classify(self.rip)
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

struct SampleRing(core::cell::UnsafeCell<[RipSample; RING_SIZE]>);
unsafe impl Sync for SampleRing {}

static RING: SampleRing = SampleRing(core::cell::UnsafeCell::new(
    [RipSample::empty(); RING_SIZE]
));

/// Write position.
static WRITE_POS: AtomicU32 = AtomicU32::new(0);

/// Whether sampling is enabled.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Per-class counters (bucket histogram).
static BUCKET_COUNTS: [AtomicU64; NUM_BUCKETS] = [
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
    AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
];

/// Total samples taken.
static TOTAL_SAMPLES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Always-on per-CPU last interrupted RIP (hang diagnostics)
// ---------------------------------------------------------------------------
//
// Independent of the opt-in profiler above: every timer tick unconditionally
// records the RIP the interrupt preempted into a per-CPU slot.  Cost is a
// single relaxed store per tick (~1 ns), so it stays enabled at all times.
//
// The liveness watchdog's SYSTEM-HANG dump reads these to answer the one
// question the task-table dump cannot: *where is each CPU actually executing*
// while the system appears wedged (e.g. spinning in a context-switch path,
// parked in the idle HLT loop, or stuck in a task that never yields).

/// Maximum CPUs tracked — mirrors [`crate::smp::MAX_CPUS`].
const MAX_CPUS: usize = 16;

/// Per-CPU last interrupted RIP, updated every timer tick regardless of the
/// profiler's enabled state.
static LAST_RIP: [AtomicU64; MAX_CPUS] = {
    const INIT: AtomicU64 = AtomicU64::new(0);
    [INIT; MAX_CPUS]
};

/// Record the RIP a timer interrupt preempted on `cpu`.  Always on.
///
/// # Performance
/// One relaxed atomic store.  Called unconditionally from the timer ISR.
#[inline]
pub fn record_last_rip(rip: u64, cpu: usize) {
    if let Some(slot) = LAST_RIP.get(cpu) {
        slot.store(rip, Ordering::Relaxed);
    }
}

/// Read the last interrupted RIP recorded on `cpu` (0 if never sampled).
#[must_use]
pub fn last_rip(cpu: usize) -> u64 {
    LAST_RIP.get(cpu).map_or(0, |slot| slot.load(Ordering::Relaxed))
}

// ---------------------------------------------------------------------------
// Public API — recording (called from timer ISR)
// ---------------------------------------------------------------------------

/// Record a RIP sample.  Called from the timer interrupt handler.
///
/// # Performance
/// This is on the hot path (100 Hz per CPU).  Keep it minimal:
/// one atomic load, one ring write, one atomic increment.
#[inline]
pub fn record(rip: u64, cpu: u8) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }

    let class = AddrClass::classify(rip);

    let sample = RipSample {
        rip,
        cpu,
        class: class as u8,
    };

    // Write to ring.
    let pos = WRITE_POS.fetch_add(1, Ordering::Relaxed);
    let slot = (pos as usize) & RING_MASK;
    // SAFETY: slot is masked to RING_MASK (< RING_SIZE), so ptr.add(slot) is
    // within the RING array.  Single-writer (atomic counter) ensures no aliasing.
    unsafe {
        let ptr = RING.0.get() as *mut RipSample;
        ptr.add(slot).write(sample);
    }

    // Update bucket counter.
    if let Some(counter) = BUCKET_COUNTS.get(class as usize) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    TOTAL_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API — control
// ---------------------------------------------------------------------------

/// Enable RIP sampling.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable RIP sampling.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether sampling is enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Reset all sampling data.
pub fn reset() {
    WRITE_POS.store(0, Ordering::Release);
    TOTAL_SAMPLES.store(0, Ordering::Relaxed);
    for counter in &BUCKET_COUNTS {
        counter.store(0, Ordering::Relaxed);
    }
    for i in 0..RING_SIZE {
        // SAFETY: i < RING_SIZE, so ptr.add(i) is within the RING array.
        // reset() is called single-threaded (sampling is stopped before reset).
        unsafe {
            let ptr = RING.0.get() as *mut RipSample;
            ptr.add(i).write(RipSample::empty());
        }
    }
}

// ---------------------------------------------------------------------------
// Public API — querying
// ---------------------------------------------------------------------------

/// Sampling statistics.
#[derive(Debug, Clone, Copy)]
pub struct SampleStats {
    pub enabled: bool,
    pub total_samples: u64,
    pub bucket_counts: [u64; NUM_BUCKETS],
}

/// Get sampling statistics with per-class breakdown.
#[must_use]
pub fn stats() -> SampleStats {
    let mut bucket_counts = [0u64; NUM_BUCKETS];
    for (i, counter) in BUCKET_COUNTS.iter().enumerate() {
        bucket_counts[i] = counter.load(Ordering::Relaxed);
    }
    SampleStats {
        enabled: ENABLED.load(Ordering::Relaxed),
        total_samples: TOTAL_SAMPLES.load(Ordering::Relaxed),
        bucket_counts,
    }
}

/// Get recent samples (newest first).
pub fn recent(buf: &mut [RipSample]) -> usize {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let available = write_pos.min(RING_SIZE);
    let to_copy = buf.len().min(available);

    for i in 0..to_copy {
        let idx = (write_pos.wrapping_sub(1).wrapping_sub(i)) & RING_MASK;
        // SAFETY: idx is masked to RING_MASK (< RING_SIZE), so ptr.add(idx) is
        // within the RING array.  We read a snapshot; torn reads produce a
        // potentially stale but structurally valid RipSample.
        unsafe {
            let ptr = RING.0.get() as *const RipSample;
            buf[i] = ptr.add(idx).read();
        }
    }
    to_copy
}

/// Find the hottest RIP address (most frequently sampled).
///
/// Scans the ring buffer and returns the address that appears most often,
/// along with its count.
#[must_use]
pub fn hottest_rip() -> Option<(u64, u32)> {
    let write_pos = WRITE_POS.load(Ordering::Acquire) as usize;
    let count = write_pos.min(RING_SIZE);
    if count == 0 {
        return None;
    }

    // Simple O(n²) scan for most common RIP in the ring.
    // Acceptable because RING_SIZE is small (256).
    let mut best_rip: u64 = 0;
    let mut best_count: u32 = 0;

    // SAFETY (group — covers all unsafe blocks below): idx/jdx are masked to
    // RING_MASK (< RING_SIZE), so every ptr.add() stays within the RING array.
    // We hold no mutable reference; reads may race with concurrent record()
    // writes but produce structurally valid (possibly stale) RipSamples.
    for i in 0..count {
        let idx = i & RING_MASK;
        let sample = unsafe {
            let ptr = RING.0.get() as *const RipSample;
            ptr.add(idx).read()
        };
        if sample.rip == 0 || sample.rip == best_rip {
            continue;
        }

        // Count occurrences of this RIP.
        let mut freq: u32 = 0;
        for j in 0..count {
            let jdx = j & RING_MASK;
            let other = unsafe {
                let ptr = RING.0.get() as *const RipSample;
                ptr.add(jdx).read()
            };
            if other.rip == sample.rip {
                freq += 1;
            }
        }

        if freq > best_count {
            best_count = freq;
            best_rip = sample.rip;
        }
    }

    if best_count > 0 {
        Some((best_rip, best_count))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for RIP sampling.
pub fn self_test() {
    serial_println!("[rip_sample] Running self-test...");

    // Test 1: Reset state.
    reset();
    let s = stats();
    assert_eq!(s.total_samples, 0);
    assert!(!s.enabled);
    serial_println!("[rip_sample]   Reset: OK");

    // Test 2: Recording while disabled does nothing.
    record(0xffffffff80001000, 0);
    assert_eq!(stats().total_samples, 0);
    serial_println!("[rip_sample]   Disabled no-op: OK");

    // Test 3: Enable and record samples.
    // We reset and re-enable inside without_interrupts on this CPU, but
    // APs may still fire timer interrupts and record extra samples.
    // Use delta-based assertions to tolerate AP-contributed samples.
    crate::cpu::without_interrupts(|| {
        reset();
        enable();
        assert!(is_enabled());

        // Snapshot counters after enable (APs may have already added samples).
        let baseline = stats();

        // Simulate kernel text samples.
        record(0xffffffff80001000, 0);
        record(0xffffffff80001000, 0);
        record(0xffffffff80002000, 1);
        // Simulate user space sample.
        record(0x00000000_00401000, 0);
        // Simulate HHDM sample.
        record(0xffff800010000000, 0);

        let s = stats();
        let delta = s.total_samples.saturating_sub(baseline.total_samples);
        // APs may contribute extra samples between baseline and final check,
        // so we assert >= rather than ==.  Our 5 must be present.
        assert!(delta >= 5, "expected at least 5 manually-recorded samples, got {}", delta);
        // Kernel text: at least 3 new (APs in kernel text can add more).
        let kt_delta = s.bucket_counts[AddrClass::KernelText as usize]
            .saturating_sub(baseline.bucket_counts[AddrClass::KernelText as usize]);
        assert!(kt_delta >= 3, "expected at least 3 kernel_text samples, got {}", kt_delta);
        // User code: exactly 1 new (APs don't run in userspace during boot).
        let uc_delta = s.bucket_counts[AddrClass::UserCode as usize]
            .saturating_sub(baseline.bucket_counts[AddrClass::UserCode as usize]);
        assert_eq!(uc_delta, 1);
        // HHDM: at least 1 new (APs with HHDM addresses can add more).
        let hh_delta = s.bucket_counts[AddrClass::Hhdm as usize]
            .saturating_sub(baseline.bucket_counts[AddrClass::Hhdm as usize]);
        assert!(hh_delta >= 1, "expected at least 1 hhdm sample, got {}", hh_delta);

        // Disable while still in no-interrupt context so no spurious
        // samples can arrive before we inspect the ring buffer.
        disable();
    });
    serial_println!("[rip_sample]   Record/classify: OK (5 samples, 3 kernel, 1 user, 1 hhdm)");

    // Test 4: Recent samples (newest first).
    // On SMP, APs may have contributed extra samples between enable() and
    // disable(), so we check >= 5 and verify our samples are present.
    let mut buf = [RipSample::empty(); 16];
    let n = recent(&mut buf);
    assert!(n >= 5, "expected at least 5 samples in ring, got {}", n);
    // Verify our test addresses appear somewhere in the recent samples.
    let has_hhdm = buf[..n].iter().any(|s| s.rip == 0xffff800010000000);
    let has_user = buf[..n].iter().any(|s| s.rip == 0x00000000_00401000);
    let has_kernel = buf[..n].iter().any(|s| s.rip == 0xffffffff80001000);
    assert!(has_hhdm, "HHDM sample should be in recent");
    assert!(has_user, "User sample should be in recent");
    assert!(has_kernel, "Kernel sample should be in recent");
    serial_println!("[rip_sample]   Recent: OK ({} entries, test samples present)", n);

    // Test 5: Hottest RIP.
    // Our test recorded 0xffffffff80001000 twice, which should be the
    // hottest unless APs hammered a single address more.  Just verify
    // hottest_rip() returns something reasonable.
    let hot = hottest_rip();
    assert!(hot.is_some(), "hottest_rip should return Some after recording");
    let (rip, count) = hot.unwrap();
    assert!(count >= 1, "hottest should have at least 1 sample");
    serial_println!("[rip_sample]   Hottest: OK ({:#x} seen {} times)", rip, count);

    // Test 6: Address classification.
    assert_eq!(AddrClass::classify(0xffffffff80100000), AddrClass::KernelText);
    assert_eq!(AddrClass::classify(0x00007fff12345678), AddrClass::UserCode);
    assert_eq!(AddrClass::classify(0xffff800012345678), AddrClass::Hhdm);
    serial_println!("[rip_sample]   Classification: OK");

    // Cleanup.
    disable();
    reset();

    serial_println!("[rip_sample] Self-test PASSED");
}
