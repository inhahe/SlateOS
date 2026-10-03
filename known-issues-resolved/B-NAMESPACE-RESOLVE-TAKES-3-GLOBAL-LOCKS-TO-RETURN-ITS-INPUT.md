### B-NAMESPACE-RESOLVE-TAKES-3-GLOBAL-LOCKS-TO-RETURN-ITS-INPUT — 2026-08-14 (`kernel/src/ipc/namespace.rs`)

**Measured, not inferred:** `ns_translate` = **1948 ns**, which is 56% of
`resolve_follow` and **30% of an entire `stat("/")`**. See the RESULT section of
`B-VFS-STAT-ROOT-IS-12x-OVER-TARGET-AND-THE-DCACHE-IS-NOT-WHY` above.

`namespace::resolve_path` is called before **every** path operation in the VFS —
read, write, stat, open, mkdir, unlink, all of it. For a process in the root
namespace with no chroot and no volume mounts — which is every process on a
normal desktop, and every process in this kernel today — the entire function
body is:

1. `current_task_id()` — cheap, an atomic load.
2. `owner_process(task_id)` → **`THREAD_OWNERS.lock()`** + map get.
3. **`PROCESS_NS.lock()`** + map get → `ROOT_NAMESPACE`.
4. `path.to_path_buf()` — a heap allocation.
5. **`PROCESS_ROOT.lock()`** + map get → `None`.
6. Return the path, byte-for-byte identical to the input.

**Three global spinlock acquisitions and one heap allocation, to return the
argument unchanged.** At the measured ~500 ns per uncontended global spinlock
under TCG, the locks alone are ~1500 of the 1948 ns.

This is not a micro-optimisation target, it is a missing fast path. The
structure charges every path operation in the system for a feature (containers)
that is not in use, and the charge is paid in the most expensive primitive
available.

**The fix** — a global "namespace features are in use" flag, checked with one
relaxed atomic load before any lock is taken:

* An `AtomicBool` (`NS_FEATURES_ACTIVE`) set with `Release` ordering at the
  three sites that can make namespace state non-trivial: inserting into
  `PROCESS_NS`, into `PROCESS_ROOT`, and into `PROCESS_MOUNTS`.
* `resolve_path_for` loads it with `Acquire`; if clear, it returns immediately.
* **The flag is never cleared.** Clearing it on the last teardown would
  introduce a race with a resolve already in flight, and the cost of staying on
  the slow path after containers have been used once is exactly the cost we have
  today. Monotonic is the sound choice and it is deliberate, not an oversight.

This is the standard rarely-used-feature pattern (Linux's static keys). It does
not change behaviour for any process: with the flag clear, no process has a
namespace, a root, or a volume, so every branch the slow path could take is the
identity branch — which is what makes the fast path a refactor rather than a
semantic change.

**The allocation in step 4 survives this fix** and is the correct next target:
`resolve_path` returns `PathBuf`, so the pass-through allocates a copy that
`resolve_prologue` immediately re-allocates in `normalize_path`. Returning
`Cow<'_, Path>` would remove one of the two. Deferred until the lock fix is
measured, because at ~180 ns it is a third of a single lock and chasing it first
would have been another instance of optimising the minor term.

#### PROSPECTIVE PREDICTION (recorded before the fix is built)

Same protocol, and this time with a directly measured anchor rather than a
fabricated one — the next run also adds `bench_spinlock_uncontended`.

1. `bench_spinlock_uncontended` comes out in **300–700 ns**. This is the load-
   bearing one: the whole cost model above stands or falls on it. If it lands
   below ~150 ns, the lock attribution is wrong and something else in
   `ns_translate` is the real cost.
2. `ns_translate` drops from 1948 ns to **< 150 ns** (one atomic load, one
   allocation removed only if the `Cow` change lands too — so expect ~180 ns if
   the allocation stays; I predict the allocation is skipped entirely on the
   fast path, hence < 150).
3. `resolve_follow` drops from 3504 ns to **1700–2000 ns**, now dominated by
   `dcache_hit`.
4. Full `vfs_stat_root` drops from ~6151 ns to **~4400–4700 ns**, a ~28%
   improvement on a benchmark I twice tried to fix by looking at the wrong
   subsystem.

**If (1) holds but (2) does not**, the fast path is not being taken — most
likely because some process really did set one of the three maps during boot,
which would itself be worth knowing and is why the benchmark prints the flag.

#### RESULT — 2026-08-14, two post-fix release boots ✅ FIXED

The first post-fix boot reported `namespace fast path DISABLED
(NS_FEATURES_ACTIVE=true)` — the pre-registered fallback clause above, firing
verbatim. The cause was not "some process set one of the maps during boot" but
something better: **the namespace self-tests themselves**.
`test_process_attach_detach`, `test_process_root` and `test_volume_mounts` call
`attach`/`set_root`/`add_volume`, which arm the monotonic flag, and nothing
disarmed it. So the self-tests were permanently degrading the VFS of the kernel
they had just finished validating — every path operation for the rest of the
boot paid three global spinlocks to exercise a feature that no longer had a
user. Fixed by asserting `reset_ns_features_if_trivial()` at the end of
`self_test()`, which doubles as a leak check: it can only succeed if every
namespace test cleaned up its process state.

Two boots after that fix (the first aborted on an unrelated flake — see
`B-FASTPY-SLEEP-SELF-TEST-IS-FLAKY` — so both are reported):

| # | prediction | pre-fix | run A | run B | grade |
|---|---|---|---|---|---|
| 1 | uncontended tracked lock **300–700 ns** | 628 | 448 | 632 | **HIT** (all three in band) |
| 2 | `ns_translate` **< 150 ns** | 1670 | 347 | 264 | **MISS** (1.8x over) |
| 3 | `resolve_follow` **1700–2000 ns** | 3138 | 2488 | 1627 | **UNPROVEN** (band narrower than the noise) |
| 4 | `vfs_stat_root` **4400–4700 ns** | 5930 | 2971 | 4394 | **HIT** (run B lands 0.14% under the band) |

**(1) HIT, and it was the load-bearing one.** The previous prediction on this
benchmark failed because its lock cost came from a *fabricated* anchor; this one
was measured first, and everything built on it held.

**(2) MISS, and the miss was avoidable by reading a type signature.** The
prediction said "I predict the allocation is skipped entirely on the fast path,
hence < 150". It cannot be: `resolve_path` returns `PathBuf`, so *every* return
allocates, fast path or not. The residual ~264 ns is one atomic load plus that
allocation. This is not a measurement surprise — it is a claim contradicted by
the function's own declaration, which was there to be read. It also promotes the
deferred `Cow<'_, Path>` change from "the correct next target" to "the only
remaining term".

**(3) UNPROVEN, and that is the more useful result.** 1627 and 2488 straddle the
band. The two runs differ by 1.53x while the band spans 1.18x — the prediction
was finer-grained than the instrument meant to grade it. Predicting to a
precision the measurement cannot resolve yields a verdict that is noise wearing
a grade's clothes, which is worse than no verdict. See
`TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK` below, where the same two runs
disagree by 1.67x on two byte-identical benchmarks.

**(4) HIT.** Predicted "~28% improvement"; measured −26% (5930 → 4394). This is
the benchmark twice attacked in the wrong subsystem (first the dcache, then the
subtraction). The third attempt — measure the anchor, then follow the
measurement — worked on the first try.
