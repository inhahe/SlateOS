### B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY — 2026-08-14 — OPEN (`kernel/src/fs/vfs.rs`, `kernel/src/ipc/namespace.rs`)

`vfs_stat_root` — `Vfs::stat("/")`, the single cheapest path operation the VFS
can perform — costs **6151 ns** on the release-profile run (`min` of 500
iterations, and *not* flagged by the dispersion check in that run, so the number
is clean). The CLAUDE.md target for a cached lookup is 200–500 ns per component.
For a zero-component path that is roughly **12–30x over**.

**The hypothesis I started with was wrong, and measurement is what killed it.**
`VfsDcache::lookup` (`kernel/src/fs/vfs.rs:1189`) is an O(n) linear scan over
`VFS_DCACHE_SIZE = 1024` slots, and CLAUDE.md explicitly forbids linear scans in
VFS path lookup. It was the obvious culprit and I was one step from rewriting it
as a hash table. Instrumenting first (`bench_vfs_stat_breakdown`, this commit)
showed:

```
vfs_stat_breakdown: dcache 25 valid entries (of 1024), +550 hits +0 misses
```

**25 live entries, filled from index 0, 100% hit rate.** A hit-scan terminates
in ~25 iterations, not 1024 — the cost of a linear scan is a function of
*occupancy*, not capacity. The scan cannot account for microseconds. The
1024-slot scan remains a latent defect (it degrades as occupancy grows, and it
is the *miss* path that walks all 1024) and is tracked as such below — but it is
**not** this bug's cause. Had I "fixed" it I would have burned a refactor and
moved the number by nothing.

**Where the time actually goes.** Splitting `Vfs::stat` at its own seam —
`resolve_follow(path)` then `stat_resolved(&path)`:

```
vfs_stat_breakdown_full:      6191 ns
vfs_stat_breakdown_resolved:  2442 ns
  => resolve_follow ~3749 ns (61%) + stat_resolved 2442 ns (39%)
```

So path *resolution* is the larger half, and both halves are individually over
target.

**Prime suspect for the 3749 ns, not yet confirmed.** `resolve_follow`
(`vfs.rs:1553`) calls `namespace::resolve_path` (`ipc/namespace.rs:721`), which
via `resolve_path_for` (`:735`) takes **`PROCESS_NS.lock()`**, then
**`PROCESS_ROOT.lock()`**, then conditionally **`PROCESS_MOUNTS.lock()`** — three
global spinlocks — and performs `path.to_path_buf()`, a heap allocation, *even
in the trivial `ROOT_NAMESPACE` pass-through case where the answer is the input
unchanged*. That is a fixed per-resolution cost paid by every single VFS
operation in the system. `validate_path`, `normalize_path` (another alloc), the
`VFS_DCACHE.lock()`, and `entry.resolved.clone()` (another alloc) are the other
candidates in that 3749 ns.

**Explicitly not yet attributed.** The above is a reading of the code, not a
measurement, and the last time I reasoned this way about a hot path
(`B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`, prediction 2) I got the *sign*
wrong. The next step is to split `resolve_follow` the same way this commit split
`stat` — `namespace::resolve_path` vs `validate_path`+`normalize_path` vs the
dcache lock+clone — and let the numbers pick the target. Do not optimise any of
the four candidates before that split exists.

**Related, same shape, worse:** `vfs_stat_deep_2comp` = 33573 ns, ~16786 ns per
component against a 200–500 ns/component target. If the fixed per-resolution
prologue is the cause of `vfs_stat_root`, it does not explain this one — 2
components cost 5.4x one component, so there is a *per-component* cost here too.
Both need the same treatment.

#### PROSPECTIVE PREDICTION (written and committed before the stage-split run)

Same protocol as `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL-1.7x`: the prediction is
committed before the measurement exists, so it can be graded rather than
rationalised. Last time this protocol caught me getting a *sign* wrong; the
point is to let it do that again.

**Primitive costs from the same release run** (`bench/history.jsonl`, commit
`040049442`) — these are the anchors, not guesses:

| primitive | measured | what it bounds |
|---|---|---|
| `heap_alloc_free_64` | 184 ns | one alloc+free pair ⇒ a single alloc ≲ 180 ns |
| `sched_pick_next` | 40 ns | takes the run-queue lock ⇒ an uncontended spinlock is *cheap*, ≲ 20 ns |
| `context_switch` | 1275 ns | nothing here should approach this |

**What each stage actually does** (from the code, and this is the weak part —
inspection is exactly what was wrong about the dcache):

* `ns_translate` = `current_task_id()` + `owner_process()` (a `THREAD_OWNERS.lock()` + `BTreeMap::get`) + `PROCESS_NS.lock()` + get + `path.to_path_buf()` (**1 alloc**, of a 1-byte path) + `PROCESS_ROOT.lock()` + get → `None`. So **3 spinlocks + 3 map lookups + 1 alloc**.
* `validate_normalize` = a byte scan of `"/"` + `normalize_path` (**1 alloc**).
* `dcache_hit` = `VFS_DCACHE.lock()` + ~25 path compares + `entry.resolved.clone()` (**1 alloc**).

**Predictions, falsifiable:**

1. `ns_translate` < 400 ns.
2. `validate_normalize` < 400 ns.
3. `dcache_hit` < 500 ns.
4. **Therefore the three stages sum to well under the 3749 ns that subtraction
   attributed to `resolve_follow` — I predict the sum is < 1500 ns.** Three
   allocations at ≤180 ns and six-ish uncontended spinlocks at ≤20 ns simply
   do not reach 3.7 µs.
5. **If (4) holds, the subtraction is what was wrong.** The specific mechanism I
   expect: `Vfs::stat` feeds `stat_resolved` the *resolved* path, while the
   isolated `vfs_stat_breakdown_resolved` benchmark feeds it the literal `"/"`.
   If `resolve_path("/")` returns something longer than `"/"`, then the
   `stat_resolved` inside `stat` is doing strictly more work than the isolated
   measurement of it, and subtraction charges that surplus to `resolve_follow`.
   **In that case the real culprit is `stat_resolved` — `resolve_mount`'s
   `VFS.lock()` + linear mount scan + `to_path_buf()` + `Arc::clone`, then
   `fs.lock().stat()` — and I will have misattributed the cost twice in a row
   on this one benchmark.**

This run therefore carries a direct measurement of `resolve_follow`
(`Vfs::resolve_path` is a public alias for it) *alongside* the subtraction, plus
a print of what `resolve_path("/")` actually returns. Prediction 5 is decided by
those two lines and needs no further argument.

**Standing caution, restated:** predictions 1–3 lean on the same
fine-grained cost reasoning that got the tcp_checksum sign wrong. Treat a hit as
weak confirmation and a miss as strong disconfirmation.

#### RESULT — 2026-08-14, release profile, commit `f9807f73a` (`build/stage-split.log`)

```
vfs_stat_breakdown: full 6423ns = resolve_follow ~3843ns + stat_resolved 2580ns
vfs_stat_breakdown: resolve_follow measured directly 3504ns (vs 3843ns by subtraction)
vfs_stat_breakdown: resolve_follow 3504ns = ns_translate 1948ns + validate_normalize 318ns + dcache_hit ~1238ns
vfs_stat_breakdown: resolve_path("/") -> "/" (1 bytes)
vfs_stat_breakdown: dcache 25 valid entries (of 1024), +1100 hits +0 misses over the run
```

| # | prediction | actual | verdict |
|---|---|---|---|
| 1 | `ns_translate` < 400 ns | **1948 ns** | **MISS, 4.9x** |
| 2 | `validate_normalize` < 400 ns | 318 ns | hit |
| 3 | `dcache_hit` < 500 ns | **~1238 ns** | **MISS, 2.5x** |
| 4 | three stages sum < 1500 ns | **3504 ns** | **MISS, 2.3x** |
| 5 | "the subtraction is what was wrong" | subtraction was **right** | **disconfirmed** |

**Prediction 5 was wrong in the way that matters most: it was an escape
hatch.** It said that if the stages came out cheap, the *subtraction* must be
the error and the real culprit would be `stat_resolved`. Both halves are
refuted outright by the two lines this run was built to print:
`resolve_path("/")` returns `"/"` unchanged (1 byte), so the different-inputs
hazard that would have made subtraction unsound never existed on this path; and
the direct measurement (3504 ns) agrees with the subtraction (3843 ns) to within
9.7%. `resolve_follow` really is ~55% of the whole stat, exactly where
subtraction put it. I did **not** misattribute the cost twice — I misattributed
it once, to the dcache, and then predicted I had misattributed it again in the
opposite direction. The second guess was as wrong as the first.

**Why 1 and 3 missed: a bad anchor, and it was bad by misreading the code.**
The prediction leaned on "`sched_pick_next` = 40 ns, and it takes the run-queue
lock, therefore an uncontended spinlock is ≲ 20 ns." That premise is simply
false about the benchmark. `bench_sched_pick_next` builds a **local**
`PriorityRoundRobin::new()` on the stack and calls `rq.pick_next()` directly —
it never touches `SCHED.lock()`. **It takes no lock at all.** So the one number
in the anchor table that was supposed to bound lock cost was measuring a
lock-free path, and the 20 ns figure was manufactured from nothing. This is the
same failure as the dcache: a claim about what the code does, asserted from
reading rather than from measuring, load-bearing for the conclusion.

**The cost model the measurement actually supports.** Solving the three stages
against their contents (3 locks + 3 map lookups + 1 alloc = 1948; 1 lock + ~25
path compares + 1 alloc = 1238; a byte scan + 1 alloc = 318) gives a consistent
fit at roughly:

| primitive | implied cost under QEMU-TCG |
|---|---|
| uncontended **global spinlock** acquire+release | **~500 ns** |
| heap alloc (small) | ~180 ns (matches `heap_alloc_free_64`) |
| one dcache path compare | ~21 ns |

A lock is ~3x an allocation here, and the whole path is **lock-dominated**: 4
global spinlocks across `resolve_follow` alone, ~2000 ns of its 3504. Every
optimisation instinct I had was aimed at allocations and at scan length, and
both are minor terms.

**But that model is derived, not measured, and deriving is what just failed
twice.** So the next run adds `bench_spinlock_uncontended` to measure the
primitive directly. The suite has anchors for allocation, context switch and
syscall dispatch but none for the single most common operation in the kernel,
which is precisely why a fabricated 20 ns figure went unchallenged.

**Consequences (tracked as `B-NAMESPACE-RESOLVE-TAKES-3-GLOBAL-LOCKS-TO-RETURN-ITS-INPUT` below).**
`ns_translate` is 1948 ns — 56% of `resolve_follow`, 30% of the entire stat —
and for a process in the root namespace with no chroot and no volume mounts
(i.e. every process on a normal desktop) it does all of that work to **return
its input unchanged**.
