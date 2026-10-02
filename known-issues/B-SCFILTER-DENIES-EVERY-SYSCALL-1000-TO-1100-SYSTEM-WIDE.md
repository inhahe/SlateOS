## B-SCFILTER-DENIES-EVERY-SYSCALL-1000-TO-1100-SYSTEM-WIDE

**Severity: HIGH — live, shipped, silent.** Found 2026-08-14 as a side effect of
the `scfilter::check` hot-path rewrite above.

`scfilter::MAX_SYSCALL_NR` was a hand-written `1000`, under a doc comment
reading *"Matches `syscall::number::MAX_SYSCALL_NR`."* It did not.
`syscall::number::MAX_SYSCALL_NR` is **1100**. Nothing checked, because a
comment asserting two numbers are equal is not a check — it is a wish.

The gap is not a slow path or a missed filter. `check` **denies** any
`nr >= MAX_SYSCALL_NR` (correctly: a bitmap that cannot represent a syscall
must not claim to allow it). So from the moment `scfilter::init()` ran at
`main.rs:4969`, every syscall in `1000..1100` returned `PermissionDenied` to
**every process on the system, filtered or not**. That range is:

- `SYS_DRM_OPEN` (1000) through `SYS_DRM_ATOMIC_COMMIT` (1060) — the *entire*
  DRM/graphics syscall interface, i.e. everything the compositor needs;
- `SYS_PROCESS_SET_EXEC_FDS` (1061), `SYS_SIGNAL_STOP_SELF` (1062),
  `SYS_PROCESS_WAIT_STATUS` (1063).

### Why every existing test missed it

Two independent blind spots, and it needed both:

1. **The dispatch self-test runs in a configuration the system never runs in.**
   `syscall::self_test()` is at `main.rs:759`; `scfilter::init()` is at 4969.
   All ~90 dispatch cases — including
   `test_dispatch_signal_stop_self_rejects_non_stop_signals`, which dispatches
   syscall **1062** — execute with the filter subsystem off, where `check`
   returned `true` from its first line. A dispatch↔filter interaction bug was
   invisible to the suite by construction.
2. **The scfilter self-test encoded the bug as its expected value.** Test 12
   read `assert!(!check(200, 1000)); // >= MAX_SYSCALL_NR — always denied`.
   That assertion *passed only because the constant was wrong*. Written as a
   literal instead of against the constant, it pinned the boundary to the
   wrong place and defended it.

The hot-path rewrite exposed it by accident: hoisting the range check ahead of
the new no-filters fast path made the branch reachable before `init()`, turning
a silent post-boot denial into an immediate boot failure —
`FAIL: signal_stop_self(0) returned -400, expected InvalidArgument`. `-400` is
`PermissionDenied`, which `dispatch` returns on exactly one path: `!check(..)`.

### Fix

- `scfilter::MAX_SYSCALL_NR` is now **derived** —
  `= crate::syscall::number::MAX_SYSCALL_NR` — not copied. Derivation makes the
  two impossible to drift apart; a `const assert!(a == b)` would only *detect*
  drift after someone reintroduced it. `BITMAP_WORDS` follows automatically
  (18 words / 144 bytes at 1100).
- Test 12 is phrased against the constant, so it tests the boundary wherever
  the boundary actually is, and gains a **12b** asserting the last
  *representable* number is still allowed — the off-by-one in the other
  direction, which would deny the top syscall to every process.
- New `syscall::dispatch::verify_dispatch_under_filtering()`, called from
  `main.rs` immediately after `scfilter::init()`, dispatches
  `SYS_SIGNAL_STOP_SELF` (the highest-numbered syscall whose rejection path is
  safe to call from the boot thread) and fails the boot if the answer is
  `PermissionDenied` rather than `InvalidArgument`. This closes blind spot (1)
  generally: there is now at least one dispatch assertion that runs *in the
  live configuration*.

### Prospective predictions (registered before the boot that tests them)

5. The boot now passes `signal_stop_self` and reaches `BOOT_OK`. If it does
   not, the `-400` had a second cause beyond the constant mismatch and the
   diagnosis above is incomplete.
6. `verify_dispatch_under_filtering()` **passes** on this boot. It is
   deliberately written so that it would have **failed** on the pre-fix tree —
   if it passes on both, it is not testing what its doc comment claims and is
   worthless as a regression guard.

### RESULT — measured 2026-08-14, predictions 4–6 graded

Boot green, `BOOT_OK` reached. Serial evidence:

```
[scfilter]   Out-of-range denied: OK
[scfilter]   Top-of-range allowed: OK                     <- new test 12b
[scfilter] Self-test PASSED (16 tests)
[syscall] Top-of-range dispatch under live filtering (nr 1062 of 1100): OK
```

| # | Prediction | Verdict |
|---|---|---|
| 4 | Fixing the scan drops `syscall_dispatch` by ≥ 100 ns, else isolated component measurement is unreliable here | **HIT** — 525 → 393 ns raw (−132), −220 ns drift-adjusted |
| 5 | Boot passes `signal_stop_self` and reaches `BOOT_OK` | **HIT** |
| 6 | `verify_dispatch_under_filtering()` passes, and would have failed pre-fix | **HIT** — passes; pre-fix it dispatches nr 1062 ≥ scfilter's 1000 and gets `PermissionDenied`, which is exactly the `-400` that failed the earlier boot |

Post-fix breakdown, internally coherent (drift gate: 393 ns then 393 ns, 0%):

```
total 393ns = handler 24 + task_id 23 + scfilter 44 + ktrace_pair 111
            + sclatency_pair 142 + unexplained 49 (12.5%)
```

**Read the share, not the nanoseconds.** The whole suite drifted **+29%**
between these two boots (median ratio 1.290 over 64 common benchmarks), so this
boot's machine is ~29% slower and the raw −132 ns *understates* the fix:

- `scfilter` share of dispatch: **57% → 11%**.
- `syscall_dispatch` raw ×0.749; drift-adjusted **×0.580** — i.e. ~304 ns in the
  previous boot's units, a **−220 ns** corrected drop.
- It is the **3rd-largest drop of 64 benchmarks** in a boot where the median
  went the *other* way, and it is the one benchmark whose code changed.

Prediction 4's consequence clause is therefore satisfied: the component measured
in isolation (299 ns) is close to what dispatch actually paid, so direct
component measurement is validated as a method for this kernel — which is what
licenses trusting the same technique on the two remaining terms below.

### Follow-on: dispatch is now 64% observability infrastructure

With scfilter down to 44 ns, the two biggest terms are both tracing:
`sclatency_pair` 142 ns + `ktrace_pair` 111 ns = **253 ns of a 393 ns dispatch**,
and both default to enabled. Two concrete causes already identified by reading:

- `sclatency::exit` calls `bench::cycles_to_ns` on every syscall **purely to
  pick a histogram bucket**, and that does two 64-bit divisions. Every other
  statistic it keeps (`TOTAL_CYCLES`, `MIN/MAX_CYCLES`, `PER_SYSCALL_CYCLES`) is
  already in cycles. Fix: convert the 12 thresholds to cycles once at
  calibration and bucket in cycles — no division on the hot path.
- `ktrace::record` calls `sched::current_task_id()` itself (23 ns measured),
  twice per dispatch — and `dispatch` has already computed that exact value.
  Fix: a `record_with_task(...)` taking the caller's `task_id`, with `record`
  as the wrapper that looks it up.

Latent, same file: if `tsc_freq()` is 0, `cycles_to_ns` returns 0, so **every**
sample lands in bucket 0 and the histogram reports "100% of syscalls under 1 µs"
when it simply cannot measure. Cycle-bucketing must not inherit that.

### Unrelated movements worth watching (this boot vs previous)

Both far exceed the +29% suite drift, so they are not explained by it:

- `isr_latency` 19065 → 44619 ns (**×2.34**) — already over its 10000 ns target.
- `pick_next` 443 → 781 ns (**×1.76**).

Not investigated yet; logged so a later "it was always like that" is checkable.
