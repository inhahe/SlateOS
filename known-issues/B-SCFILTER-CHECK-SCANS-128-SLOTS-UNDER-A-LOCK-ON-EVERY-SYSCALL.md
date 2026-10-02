## B-SCFILTER-CHECK-SCANS-128-SLOTS-UNDER-A-LOCK-ON-EVERY-SYSCALL

**Status:** 🔬 PREDICTION REGISTERED (measurement not yet taken)
**Where:** `kernel/src/scfilter.rs:253` (`check`), called from
`kernel/src/syscall/dispatch.rs:700` on every syscall.

### Why this was looked at

`syscall_dispatch` has been OVER its 200 ns target on every boot on record —
699 ns, 3.5x — and the lockdep O(1) fix moved it by **exactly 0 ns**, while it
cut `crate::sync::Mutex` acquire+release 632 → 274 ns and `vfs_stat_root` by
44%. That null result was pre-registered as meaningful: *"if syscall dispatch
does not move, then the 'every benchmark is partly measuring lockdep' claim is
overstated and must be narrowed."* It was narrowed. The follow-up question it
left open — *then where does 699 ns go?* — is this entry.

### What the code says (used to choose what to measure, not how much it costs)

`dispatch()` runs five things around the handler: `sclatency::enter`,
`ktrace::record`, `sched::current_task_id`, `scfilter::check`, a second
`ktrace::record`, and `sclatency::exit`. Of these, `scfilter::check` is the
one that reads wrong:

```rust
let mut guard = TABLE.lock();                    // PreemptSpinMutex
let Some(table) = guard.as_mut() else { return true };
for entry in &mut table.filters {                // MAX_FILTERS == 128
    if entry.active && entry.pid == task_id { ... }
}
true                                             // no filter -> full 128-slot walk
```

With **zero filters installed** — the state the kernel boots in and stays in —
the loop never matches, so it walks all 128 slots of a ~19 KiB table, under a
lock, on every syscall. This is the same shape as the lockdep bug fixed
immediately before it: **a linear scan hiding in infrastructure that everything
else pays for.**

Two aggravating details:

1. **The doc comment contradicts itself.** It claims *"If no filter is
   installed for this task, returns `true` (O(1))"* and, five lines later,
   *"No filter installed: ~5 ns (atomic load + **linear scan miss**)"*. Both
   cannot be true. An O(1) claim and a linear-scan claim in the same comment
   means nobody has measured it — the `~5 ns` is an estimate presented in the
   typography of a measurement.
2. **It explains the lockdep null result.** `TABLE` is a
   `PreemptSpinMutex`, which by construction carries *no* lockdep and *no*
   contention tracking. So the lockdep fix could not have moved
   `syscall_dispatch` no matter how large scfilter's cost is — which is
   consistent with the observed 0 ns, and is why "no change" was not evidence
   that dispatch is cheap.

### Prospective predictions — registered BEFORE the measurement exists

A decomposition benchmark (`bench_syscall_dispatch_breakdown`) now measures each
stage **directly and in isolation**, never by subtraction, and prints an explicit
`unexplained` residual plus a drift gate. It has not been run yet. Graded on the
next `--bench` boot:

1. **`sd_scfilter_check` is the largest non-handler stage, and is > 150 ns.**
   *If it is under 150 ns, or is not the largest, the "linear scan under a lock
   on every syscall" story is wrong and the 699 ns lives somewhere else — and I
   must go look at the indirect call through the handler table before touching
   scfilter.*
2. **`sd_handler` < 60 ns** — i.e. the work the caller actually asked for is
   under 10% of dispatch, and dispatch is overwhelmingly overhead.
3. **The four overhead stages together account for ≥ 60% of the 699 ns**
   (`unexplained` ≤ 40%). *If `unexplained` dominates, the decomposition missed
   the real cost, and the finding is that isolated per-stage measurement does
   not reconstruct this path — which matters more than this bug.*
4. **Consequence clause.** If (1) holds, giving `check` a genuine O(1) zero-filter
   fast path (one relaxed atomic load, no lock, no scan) must drop
   `syscall_dispatch` by **at least 100 ns**. *If it does not, then
   `sd_scfilter_check` measured in isolation is not what dispatch actually pays,
   and the whole direct-component-measurement method used here and in the VFS
   breakdown is unreliable in this kernel. That would be a much more important
   result than the bug, and would be recorded as such rather than explained away.*

### Deliberately NOT predicted

A per-stage nanosecond figure for `ktrace_pair` or `sclatency_pair`. Both are
dominated by `rdtsc`, whose measured pair cost (56 ns) already carries more
run-to-run spread than any band worth stating. Predicting finer than the
instrument resolves produces noise wearing a grade's clothes.

### RESULT — measured 2026-08-14, predictions 1–3 graded

```
[bench] sd_handler:          min=  92 cycles (19ns)
[bench] sd_current_task_id:  min=  84 cycles (17ns)
[bench] sd_scfilter_check:   min=1438 cycles (299ns)
[bench] sd_ktrace_pair:      min= 410 cycles (85ns)
[bench] sd_sclatency_pair:   min= 370 cycles (77ns)
[bench]   syscall_dispatch breakdown: total 525ns = handler 19ns + task_id 17ns
          + scfilter 299ns + ktrace_pair 85ns + sclatency_pair 77ns + unexplained 28ns
[bench]   syscall_dispatch breakdown: scfilter has 0 filter(s) installed of 128 slots
[bench]   syscall_dispatch breakdown: drift check — dispatch twice: 525ns then 511ns (2%)
```

| # | Prediction | Actual | Verdict |
|---|---|---|---|
| 1 | `sd_scfilter_check` is the largest non-handler stage and > 150 ns | **299 ns**, 3.5x the next stage | **HIT** |
| 2 | `sd_handler` < 60 ns (real work < 10% of dispatch) | **19 ns**, 3.6% of 525 ns | **HIT** |
| 3 | four overhead stages ≥ 60% of total (`unexplained` ≤ 40%) | stages = **94.7%**, unexplained **5.3%** | **HIT** |
| 4 | consequence clause — fixing (1) drops `syscall_dispatch` ≥ 100 ns | pending the next boot | — |

**The headline: 57% of kernel-side syscall dispatch was a linear scan over an
empty table.** The work the caller actually asked for — `sys_task_id`, which
reads one per-CPU word — is 19 ns of 525. Dispatch was 96% overhead, and more
than half of the overhead was one function whose doc comment said `O(1)`.

**A caveat that the coherence gate is the only reason I can state.** The total
here is 525 ns, not the 699 ns quoted when this investigation started, with no
relevant code change between them: the whole suite drifted −22.3% this run.
Quoting "699 ns" as a property of `syscall_dispatch` would have been wrong.
What survives drift is the *ratio*, and the drift gate (2%) plus the residual
(5.3%) are what license reading the split at all. This is the first time one of
these gates has been load-bearing rather than decorative.

**Prediction 3 is the one worth noting.** It was the hedge — "if `unexplained`
dominates, my decomposition missed the real cost and the method is unreliable
here". At 5.3% it did not just pass, it says the five measured stages *are*
dispatch: there is no meaningful hidden cost in the call, the bounds check, the
table indirect call, or the return. That is a stronger result than the bug, and
it is what makes the fix's target unambiguous.

### Fix

`scfilter::check` now:

1. **Range-checks first** (a compare against a constant), *then* consults a new
   `ACTIVE_FILTERS: AtomicUsize`. Zero — i.e. no process anywhere is sandboxed,
   which is the state the kernel boots in — returns `true` immediately: no lock,
   no table access, one atomic load. The ordering matters and is deliberate:
   putting the range check *after* the fast path would have made
   `check(pid, 1000)` return `true` or `false` depending on whether some
   unrelated process happened to be sandboxed, and `dispatch` range-checks
   before calling this, so nothing would have caught it.
2. **When something is filtered**, uses a Fibonacci-hashed open-addressed
   `pid → slot` index (256 buckets, < 50% load, const-asserted) instead of
   walking 128 slots. So installing one filter no longer taxes every syscall of
   every *other* process — which the "just add a zero-filter fast path" version
   of this fix would have left in place, as a cliff waiting for the first
   sandbox.
3. High bits of the multiply, not low — pids are a dense ascending sequence, so
   low bits would map the first 128 processes onto 128 consecutive buckets and
   turn every miss into a long probe run: the same linear scan, wearing a hash's
   clothes.

**The index is rebuilt wholesale after every mutation** rather than maintained
incrementally. Incremental deletion from a linear-probe table needs tombstones
(which accumulate across install/remove cycles until it degrades back to a scan)
or backward-shift deletion (easy to get subtly wrong). Mutations happen at
process create/exit; lookups happen on every syscall. Paying a 256-byte memset
on the rare path to keep the hot path both O(1) and obviously correct is the
right side of that trade.

**This bug fails OPEN, which is why it gets an oracle.** In lockdep, a hash that
silently missed a class made the validator go quiet — bad, but it fails safe.
Here, a hash that silently misses a pid means *a filtered process runs
unfiltered*: a sandbox escape with no log line. So the deleted linear scan
survives as `FilterTable::lookup_by_scan`, and `verify_index()` asserts on every
boot and inside the self-test that hash and scan agree for every active pid,
that a never-installed pid resolves to `None` through both, and that the cached
`ACTIVE_FILTERS` equals a linear count of active slots. That last one is the
dangerous direction: a counter that reads *too low* switches syscall filtering
off entirely and silently.

**Two tests, because a test on an empty table proves nothing about lookup.**
`self_test` now verifies with filters installed *and* searches 200k candidate
pids for one that genuinely collides with an installed pid's bucket, so the
probe path is exercised rather than hoped for — and prints `UNTESTED` rather
than passing if it finds none. It also asserts the filters it installed were
all drained, so the suite cannot leave the syscall fast path switched off for
the rest of the boot the way the namespace self-tests once left
`NS_FEATURES_ACTIVE` armed.

**Also removed: a kill switch that switched nothing.** `ENABLED: AtomicBool` was
set once by `init` and read only by `check`. Once `check` stopped reading it, it
was write-only — worse than absent, because it reads like a way to disable
syscall filtering and a later `disable()` built on it would have disabled
nothing while appearing to work.
