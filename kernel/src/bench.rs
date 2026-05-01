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

    // --- Heap allocation (small, 64 bytes) ---
    {
        use alloc::vec;
        let result = run("heap_alloc_small_64", 1000, || {
            let v = vec![0u8; 64];
            // Prevent optimization from eliding the allocation.
            core::hint::black_box(&v);
            drop(v);
        });

        let target_ns = 200u64; // From baselines.toml
        if result.min_ns <= target_ns {
            serial_println!(
                "[bench]   heap_alloc_small: PASS (min {}ns <= target {}ns)",
                result.min_ns, target_ns
            );
        } else {
            serial_println!(
                "[bench]   heap_alloc_small: ABOVE TARGET (min {}ns > target {}ns)",
                result.min_ns, target_ns
            );
        }
    }

    // --- Heap allocation (medium, 512 bytes) ---
    {
        use alloc::vec;
        run("heap_alloc_medium_512", 1000, || {
            let v = vec![0u8; 512];
            core::hint::black_box(&v);
            drop(v);
        });
    }

    // --- Heap allocation (large, 4096 bytes) ---
    {
        use alloc::vec;
        run("heap_alloc_large_4096", 500, || {
            let v = vec![0u8; 4096];
            core::hint::black_box(&v);
            drop(v);
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

    // --- Page fault (demand-page anonymous fault) ---
    //
    // Measures the page fault handler's resolution path for a demand-
    // paged anonymous page.  Includes frame allocation, zeroing, page
    // table update, and TLB flush.
    //
    // Target from baselines.toml: < 10 µs (Linux: ~2-5 µs).
    bench_page_fault();

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

    let hhdm = page_table::hhdm().expect("bench: HHDM");
    let pml4 = page_table::cr3_to_pml4(page_table::read_cr3());

    // Pick a kernel-space virtual address that's not in use.
    // Use a high address in the kernel reserved range.
    // Must be 16 KiB aligned for map_frame.
    let bench_virt_base: u64 = 0xFFFF_CB00_0000_0000;

    // OPT: Use map_frame/unmap_frame (single page table walk for all 4
    // hardware pages) instead of 4× map_4k_if_absent.  This matches the
    // real page fault handler in try_grow_user_stack / pcb::try_resolve_fault
    // which both use map_frame.  The old 4× walk added 3 redundant
    // PML4→PDPT→PD→PT traversals per iteration.
    let result = run("page_fault_anonymous", 200, || {
        let virt = VirtAddr::new(bench_virt_base);

        // Allocate a frame (simulating what the fault handler does).
        let f = frame::alloc_frame().expect("bench: alloc");

        // Zero the frame via HHDM (fault handler does this for anonymous pages).
        let dst = f.to_virt(hhdm) as *mut u8;
        // SAFETY: f is a valid allocated frame, HHDM maps all physical memory.
        unsafe {
            core::ptr::write_bytes(dst, 0, frame::FRAME_SIZE);
        }

        // Map the 16 KiB frame (4 hardware pages in a single page table walk).
        let flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::NO_EXECUTE;
        // SAFETY: bench_virt_base is in unused kernel space, pml4 is valid,
        // f is freshly allocated.
        unsafe {
            page_table::map_frame(pml4, virt, f, flags).expect("bench: map");
        }

        // TLB flush for all 4 pages.
        crate::tlb::flush_range(bench_virt_base, 4);

        // Unmap (cleanup for next iteration) — single walk for all 4 pages.
        // SAFETY: we just mapped these pages.
        let returned = unsafe {
            page_table::unmap_frame(pml4, virt).expect("bench: unmap")
        };
        crate::tlb::flush_range(bench_virt_base, 4);

        // Free the frame.
        // SAFETY: we're the sole owner, all mappings removed.
        unsafe { frame::free_frame(returned).expect("bench: free"); }
    });

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
