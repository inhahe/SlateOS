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

**Measured 2026-09-11, pairwise.** For each consecutive pair of runs and each
benchmark, `max(a,b) / min(a,b)` — the factor by which an unchanged benchmark moved
between two runs. Pairwise rather than `max/median` because **`max/median` is not a
spread statistic: it grows with the number of samples**, so it cannot compare two
accelerators with different run counts. (The first version of this table did exactly
that, at 8 WHPX samples against 36 TCG ones, which flattered WHPX mechanically. See
the note below.)

| `accel` | basis | median benchmark | 90th pct | worst / p99 |
|---|---|---|---|---|
| `Hyper-V/WHPX` | 1 pair, 99 benchmarks | **1.022×** | 1.060× | 1.33× (worst) |
| `QEMU TCG` | 31 pairs, 2749 observations | **1.099×** | 1.750× | 2.62× (p99) |

WHPX is genuinely the tighter surface — 2.2% typical movement against 9.9% — and that
conclusion now rests on unperturbed runs rather than on layout-sweep arms. **But the
WHPX row is a single pair**: one draw of the statistic per benchmark. Whether 1.022×
is stable needs a third run, and until it exists treat the WHPX column as indicative.

### What to do with that

* **How the WHPX row got its number, because the first version of it was wrong in a
  way worth not repeating.** Until 2026-09-11 every `Hyper-V/WHPX` row in
  `bench/history.jsonl` — all fourteen — was an arm of one *layout sweep*, carrying
  `experiment: "layout sweep: textpad=N (identical source, deliberately perturbed…)"`,
  five of them on a single commit. The first version of this table read those as
  ordinary runs and reported 1.03×, which was the within-sitting spread of one build
  under deliberate perturbation: a smaller quantity than run-to-run spread, in the
  direction that flattered the conclusion. The figure in the table now comes from two
  runs made specifically for it, via
  `QEMU_EXTRA="-accel whpx" ./scripts/boot-test.sh --bench`. The lesson is the cheap
  one: **read the `experiment` field before reading `accel`** — they sit in the same
  JSON object.
* **Under TCG, a 10% rule is inside the median benchmark's own movement** (9.9%), and
  **33% of observations exceed 25%** (p75 = 1.386×). Treat anything below ~1.75× as unmeasured
  rather than unchanged — that is the 90th percentile, so one benchmark in ten beats
  it with no code change at all. This cuts both ways and the second way is the one
  that bites: do not take a 40% *improvement* on one TCG run as evidence that an
  optimisation worked.
* **Under WHPX a 10% rule is just outside the 90th percentile** (6.0%), so it is
  plausibly usable at the cost of roughly one false alarm in ten benchmarks; ~15%
  would be quiet. Device- and timer-bound benchmarks are the exception and are not
  measurable there at all — see `known-issues.md` →
  `TD-A-THE-BENCHMARK-BUDGETS-NEVER-FIRE-IN-A-BOOT-TEST`, where three of them collapse
  onto one VM-exit cost.
* **A single run is never a measurement under TCG.** If you need a TCG number,
  compare medians of several clean runs, and say how many.
* **Check `run_verdict` before reading any number.** The figures above are for
  `clean` rows only; on `contaminated` rows the spread is worse, and 55 of the 144
  recorded runs are contaminated. A contaminated row is not a weak measurement, it
  is an absent one.
* **Never compare across `accel`**, and the tree already measured why better than a
  single benchmark can show. `scripts/bench-history.py::comparable_records` records,
  from one byte-identical binary (`kernel_sha 7a17cf6be2a1`, 2026-08-19): the median
  benchmark is **~3.5× faster** under Hyper-V/WHPX than under TCG, the best ~10×,
  **and the device-bound ones ~30× _slower_** — an HPET read costs a VM exit under
  hardware virtualisation and is emulated inline under TCG. So WHPX is not uniformly
  faster, and the difference is not a scale factor that could be divided out. In that
  function's words, a window mixing the two *"does not have a wider spread; it has two
  populations."*
  `comparable_records` already filters on `accel` for exactly this reason, so the
  machinery respects the rule; it is the prose here that needed to catch up.

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
