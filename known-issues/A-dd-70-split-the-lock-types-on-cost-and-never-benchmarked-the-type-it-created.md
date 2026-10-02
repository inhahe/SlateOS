### [A] dd-70 split the lock types on cost, and never benchmarked the type it created -- 2026-09-17

**Status:** OPEN

`bench_lock_primitives` has four arms. `RAW` is a bare `spin::Mutex`;
`TRACKED` and `TRACKED_B` are `crate::sync::Mutex`. There is **no**
`PreemptSpinMutex` arm -- so the two types measured are the two dd-70 was
choosing *between*, and never the one the decision produced, despite it now
holding 489 instances on the hottest paths in the kernel.

This is the same shape as the leaf claim, on the other half of the same
decision. dd-70's *correctness* premise was "nothing nests inside it" (now
checked, see dd-949). Its *performance* premise was "where the per-acquire
tracking cost of `Mutex` would matter" -- and that had no measurement behind
it either.

A `lock_preempt_spin` arm is added. Fully qualified deliberately: `bench.rs`
aliases `Mutex` *to* `PreemptSpinMutex` at the top of the file, so an
unqualified name there reads as the opposite of what it is, which is how one
would measure the wrong type and believe it.

Two caveats on the existing numbers, so they are not over-read. dd-70 quotes
"~5ns/acquire" for `Mutex`'s overhead; the measured figure on boot
`b1ebbda65` was `+549ns = lockdep 254ns + preempt 36ns + rdtsc 56ns +
unexplained 203ns`, two orders larger. These are QEMU TCG measurements, so
the absolute nanoseconds are not hardware nanoseconds -- the 21x *ratio*
between raw and tracked is the part that transfers, not the ns.

**Measured 2026-09-17, and dd-70's premise holds.** The arm did print --
the teardown-before-bench worry was wrong, four consecutive boots reached
it:

| boot | raw | preempt-spin | tracked |
|---|---|---|---|
| `134333Z` | 25ns | 166ns | 408ns |
| `144018Z` | 26ns | 159ns | 392ns |
| `170456Z` | 26ns | 162ns | 394ns |
| `202155Z` | 26ns | 159ns | 400ns |
| `182832Z` | 28ns | 268ns | 719ns |

`182832Z` is uniformly higher across all three arms, so it is a slow boot
rather than a slow lock; the ratio is what transfers under TCG, not the ns.
**`PreemptSpinMutex` is ~2.5x cheaper than `crate::sync::Mutex`** (159 vs
394) and ~6x dearer than a bare `spin::Mutex` (26). So dd-70 split the types
on a real cost difference, which nobody had measured until the arm existed.

**And the leaf check's own cost is below the noise floor.** `134333Z`
predates the leaf check (`leaf-claim check:` absent) and already has the
arm, so a genuine before/after exists: 166ns uninstrumented against 159,
162, 159 instrumented. The instrumented runs are *faster*, which means the
three added atomics are not resolvable at 2000 iterations on TCG -- variance
between runs exceeds any effect. Not "cheap because it looks cheap": a
measured non-result.

That also narrows A-Q16's cost objection. The 159 -> 394 gap is lockdep plus
contention stats plus two `rdtsc` reads, not a general penalty for touching
the acquire path -- so "converting these locks costs per-acquire tracking"
is about 235ns of specific work, and a conversion's real cost depends on how
often the lock in question is taken.
