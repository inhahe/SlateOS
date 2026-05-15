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

use alloc::string::String;
use crate::serial_println;
use spin::Mutex;

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

/// TSC frequency in Hz, calibrated at boot.
static TSC_FREQ: Mutex<u64> = Mutex::new(0);

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

        *TSC_FREQ.lock() = freq;

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
    *TSC_FREQ.lock()
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
// Standard kernel benchmarks
// ---------------------------------------------------------------------------

/// Run all standard kernel micro-benchmarks.
///
/// Call after all subsystems are initialized.  Results are printed to
/// serial for comparison against `bench/baselines.toml`.
pub fn run_all() {
    serial_println!("[bench] === Kernel micro-benchmarks ===");

    // Note: iteration counts are kept modest because these run during
    // boot under QEMU emulation.  For real hardware benchmarks, increase
    // counts 10-50x.

    // --- Page allocation (alloc + free cycle) ---
    {
        use crate::mm::frame;
        let result = run("page_alloc_free", 500, || {
            let f = frame::alloc_frame().expect("bench: alloc");
            // SAFETY: frame was just allocated, exclusively ours.
            unsafe { frame::free_frame(f).expect("bench: free"); }
        });

        let target_cycles = 3700u64; // From baselines.toml
        if result.min_cycles <= target_cycles {
            serial_println!(
                "[bench]   page_alloc_free: PASS (min {} <= target {})",
                result.min_cycles, target_cycles
            );
        } else {
            serial_println!(
                "[bench]   page_alloc_free: ABOVE TARGET (min {} > target {})",
                result.min_cycles, target_cycles
            );
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
            let ptr = unsafe { alloc::alloc::alloc(layout) };
            debug_assert!(!ptr.is_null(), "bench: alloc returned null");
            core::hint::black_box(ptr);
            unsafe { alloc::alloc::dealloc(ptr, layout); }
        });
    }

    // --- Raw heap alloc + dealloc (4096 bytes) ---
    {
        let layout = core::alloc::Layout::from_size_align(4096, 8)
            .expect("valid layout");
        run("heap_raw_alloc_free_4096", 500, || {
            let ptr = unsafe { alloc::alloc::alloc(layout) };
            debug_assert!(!ptr.is_null(), "bench: alloc returned null");
            core::hint::black_box(ptr);
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

    // --- ISR latency (timer interrupt hard-IRQ phase) ---
    //
    // Measures the time interrupts are disabled during the timer ISR:
    // entry → tick counter increment → scheduler timer_tick → EOI.
    // This is the hard-IRQ phase that blocks device interrupts.
    //
    // Target from baselines.toml: < 10 µs (37000 cycles).
    bench_isr_latency();

    // --- VFS benchmarks (fs zone) ---
    bench_vfs_stat();
    bench_vfs_read_write();
    bench_vfs_readdir();

    // --- Network benchmarks (net zone) ---
    bench_net_ipv4_parse();
    bench_net_ethernet_parse();
    bench_net_arp_lookup();
    bench_net_checksum();

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
    let _result = run("sched_pick_next_4tasks", 500, || {
        sched::yield_now();
    });

    // The pick_next portion of yield_now is a small fraction of the
    // total context switch cost.  We report it for tracking.
    serial_println!(
        "[bench]   pick_next overhead included in context switch"
    );

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
    let target_ns = 200u64;
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
        "[bench] isr_latency: measuring ~100 timer ticks (start_tick={})...",
        start_tick,
    );

    // Start measurement — next timer ISR begins sampling.
    apic::start_isr_measurement();

    // Busy-wait for ~100 timer ticks (~1 second at 100 Hz).
    //
    // We can't use yield_now() here because the boot task (priority 0)
    // gets re-selected immediately on each yield — all 2000 yields
    // complete before a single timer tick fires.  Instead, spin-wait
    // on the tick counter.  The timer ISR fires normally (interrupts
    // are enabled) and records ISR latency measurements on each tick.
    //
    // Use a TSC-based timeout (~5 seconds) as a safety net in case
    // tick_count stops advancing (e.g., timer misconfiguration, CPU
    // migration to a non-BSP core).
    let target_ticks = 100u64;
    let tsc_start = rdtsc();
    let tsc_timeout = tsc_freq().saturating_mul(5); // 5 seconds worth of cycles
    loop {
        let elapsed_ticks = apic::tick_count().saturating_sub(start_tick);
        if elapsed_ticks >= target_ticks {
            break;
        }
        let elapsed_tsc = rdtsc().saturating_sub(tsc_start);
        if elapsed_tsc > tsc_timeout {
            serial_println!(
                "[bench] isr_latency: TSC timeout after ~5s (ticks advanced: {}, expected: {})",
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
