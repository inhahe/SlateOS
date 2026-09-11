# Performance targets and benchmarking protocol


Extracted from `CLAUDE.md` on 2026-09-06. It lived there, resident in every
lane's context on every turn, and is needed only when you are actually doing
the thing it describes -- which announces itself, so a pointer suffices.
`CLAUDE.md` points here.



The design spec calls out that AI tends to write "correct-but-naive" code. This OS must be competitive with Linux, Windows, and macOS in performance-critical paths. The mitigation: **write benchmarks first, then optimize against concrete targets.**

### Performance-Critical Subsystems

These subsystems are on the hot path for virtually every workload. Naive implementations here are not acceptable.

| Subsystem | Why it's critical | Benchmark target (reference) |
|-----------|-------------------|------------------------------|
| **Syscall dispatch** | Every userspace→kernel transition | Linux: ~100ns for getpid. Target: within 2x of Linux. |
| **IPC channel send/recv** | Primary inter-process communication | Fuchsia channel round-trip: ~1-2us. L4 IPC: ~0.5-1us. Target: < 2us round-trip. |
| **Context switch** | Every preemption, every blocking call | Linux: ~1-3us. Target: < 5us. |
| **Page fault handling** | Demand paging, stack growth, CoW | Linux: ~2-5us for anonymous page fault. Target: < 10us. |
| **Physical page alloc/free** | Every mmap, every process start | Linux buddy allocator: ~100-500ns. Target: < 1us. |
| **Heap allocation (kernel)** | Constant kernel bookkeeping | jemalloc small alloc: ~20-50ns. Target: < 200ns for common sizes. |
| **Scheduler pick_next_task** | Every timer tick, every blocking op | Must be O(1) or O(log n). Never O(n) over all tasks. |
| **Futex wait/wake (uncontended)** | Userspace mutex fast path | Linux: uncontended = no syscall (atomic CAS in userspace). Contended wake: ~1-3us. Match this design. |
| **io_uring submission** | High-throughput async I/O | Linux io_uring: ~100-200ns per SQE submission. Target: same order. |
| **IOCP-like completion wait** | Main event loop for all servers/GUI apps | Windows IOCP / Linux epoll: sub-microsecond for ready events. |
| **Interrupt dispatch** | Every keystroke, every packet, every timer | Total ISR latency < 10us. Deferred work via softirq/tasklet equivalent. |
| **VFS path lookup** | Every file open, every path resolution | Linux: cached lookup ~200-500ns per component. Use dcache equivalent. |
| **Filesystem read/write** | All I/O | Compare to ext4 on Linux for sequential and random I/O throughput. Target: within 20% of Linux ext4. |
| **Compositor frame** | Every display refresh | Must composite a full desktop in < 2ms at 4K to not miss 144Hz vsync. |

### Benchmarking Protocol

1. **Write the benchmark before or alongside the implementation**, not after. Use `criterion` for microbenchmarks. Put benchmarks in `bench/<subsystem>/`.
2. **Record baseline numbers** from Linux/Fuchsia/Windows (from published benchmarks, academic papers, or your own measurements on the dev machine). Store these in `bench/baselines.toml`:
   ```toml
   [syscall_getpid]
   linux_ns = 100
   target_ns = 200
   source = "measured on dev machine, Linux 6.x, same hardware"

   [ipc_channel_roundtrip]
   fuchsia_us = 1.5
   l4_us = 0.8
   target_us = 2.0
   source = "Fuchsia perf docs, L4 published benchmarks"
   ```
3. **Run benchmarks after every change to a critical subsystem.** Compare to baselines. If a change regresses a benchmark by more than 10%, investigate before merging.
4. **Optimize iteratively.** Write the correct version first, benchmark it, then optimize. Use profiling (`perf` equivalent, or manual cycle counting via `rdtsc`) to find the actual bottleneck. Don't guess.
5. **Document optimizations.** When you apply a non-obvious optimization, add a comment explaining what it does, why it helps, and what the benchmark improvement was:
   ```rust
   // OPT: Using per-CPU free lists avoids atomic operations on the global
   // allocator in the common case. Benchmark: page_alloc dropped from 800ns
   // to 150ns (5.3x improvement). See bench/mm/page_alloc.rs.
   ```

### Performance Patterns to Follow

- **Per-CPU data structures** for anything accessed on every syscall/interrupt (allocator free lists, scheduler run queues, IPC queues). Avoid cross-CPU atomic contention.
- **Lock-free fast paths** for high-frequency operations. Futexes should be pure atomic CAS in userspace for the uncontended case. Only enter the kernel on contention.
- **Cache-line alignment** for per-CPU structures and heavily-contended locks. False sharing destroys performance.
- **Avoid heap allocation on hot paths.** Pre-allocate, use slab allocators, or use stack allocation. No `Vec::push` in interrupt handlers.
- **Batching** for I/O operations (io_uring model: submit many, complete many, one syscall).
- **Lazy computation.** Don't compute what you might not need (relatime for atime updates, lazy TLB shootdown, deferred page zeroing).

### Performance Anti-Patterns to Avoid

- **Premature abstraction that prevents optimization.** A trait with dynamic dispatch (`dyn Trait`) on a hot path costs an indirect call (~5-10ns + branch mispredict). Use monomorphization or manual dispatch for hot paths.
- **Holding locks across I/O or long operations.** Design for fine-grained locking. If a critical section could block, redesign.
- **Copying data when you can transfer ownership or use zero-copy.** IPC channels should move pages between address spaces, not copy message contents.
- **Linear scans** in the scheduler, allocator, or event dispatch. These must be O(1) or O(log n).

---

- **Before implementing a major subsystem, study how proven OSes do it** --
  see `reference-implementations.md` for which sources to read per subsystem
  and how to cite them. This is not optional; it is the primary mitigation
  against writing correct-but-naive code.

---

## The 10% rule needs a noise floor, and under emulation it does not have one

Rule 3 above says to investigate a regression over 10%. That is the right rule and
it **cannot be applied to a QEMU TCG measurement**, because the run-to-run spread of
an unchanged benchmark is several times larger than 10%.

Measured 2026-09-11 over `bench/history.jsonl` — 144 recorded runs, restricted to
rows whose `run_verdict` is `clean`, grouped by `accel`, across all 86 benchmarks
with at least 4 samples in a group. The figure is each benchmark's worst sample
divided by its own median:

| `accel` | clean runs | median benchmark | 90th pct | 99th pct |
|---|---|---|---|---|
| `Hyper-V/WHPX` | 8 | **1.03×** | 1.23× | 1.53× |
| `QEMU TCG` | 9 | 1.29× | 2.13× | 2.78× |
| (unrecorded, TCG-era) | 36 | 1.58× | 2.64× | 3.33× |

### What to do with that

* **Under WHPX, the 10% rule is sound as written.** The median benchmark moves 3%
  between clean runs, so a 10% move is signal.
* **Under TCG, treat anything below ~2× as unmeasured, not as unchanged.** A 40%
  "regression" on a single TCG run is the ordinary behaviour of half the suite. Do
  not investigate it, and — more important — do not take a 40% *improvement* as
  evidence that an optimisation worked.
* **A single run is never a measurement under TCG.** If you need a TCG number,
  compare medians of several clean runs, and say how many.
* **Check `run_verdict` before reading any number.** The figures above are for
  `clean` rows only; on `contaminated` rows the spread is worse, and 55 of the 144
  recorded runs are contaminated. A contaminated row is not a weak measurement, it
  is an absent one.
* **Never compare across `accel`.** The same unchanged code reads ~1716 ns under
  TCG and ~249 ns under WHPX for `tcp_checksum_v4` — a 7× difference that is purely
  the accelerator. Two rows with different `accel` are two different experiments.

### Why this is written down rather than left to judgment

The static cycle budgets in `kernel/src/bench.rs` compare against hardware figures
and report `SCORE … OVER` for 9–21 benchmarks on **every** run. Nothing treats that
as a failure, and `print_scorecard`'s own comment calls the over-target list
"reference rather than a verdict" — correctly, as it turns out, but without saying
that the reason is the noise floor above. Anyone who reads the budgets as a gate,
as I did on 2026-09-10, concludes that a gate is being skipped rather than that the
measurement cannot support one. See `known-issues.md` →
`TD-A-THE-BENCHMARK-BUDGETS-NEVER-FIRE-IN-A-BOOT-TEST`, including both of its
corrections.

To re-derive these numbers after more runs accumulate: group `bench/history.jsonl`
by `accel` over `run_verdict == "clean"`, and take the distribution of
`max(samples) / median(samples)` per benchmark name. Nine clean TCG rows is a thin
basis for the 99th percentile; the 50th is solid.
