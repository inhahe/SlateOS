### B-LOCKDEP-CLASS-LOOKUP-IS-A-LINEAR-SCAN-ON-EVERY-LOCK — 2026-08-14 (`kernel/src/lockdep.rs`)

**Measured, and it is the largest single overhead found this session.** The lock
microbenchmark added to grade the namespace fix answered a question nobody had
asked it:

```
lock acquire+release: raw 30ns, tracked 632ns, no-lockdep 232ns, no-stats 656ns
lock overhead: total +602ns = lockdep 400ns + preempt 29ns + rdtsc 57ns + unexplained 116ns
```

`raw` is `spin::Mutex`; `tracked` is `crate::sync::Mutex`, the type every global
in the kernel uses. **The tracked mutex costs 21x the raw one, and two thirds of
the difference is lockdep.** Confirmed across both post-fix boots: 400/602 ns
(66%) and 281/430 ns (65%).

The cause is not that validation is expensive. It is that the *lookup* is
`O(classes)`:

```rust
fn find_or_register_class(lock_addr: usize, name: &[u8]) -> Option<u16> {
    let count = CLASS_COUNT.load(Ordering::Relaxed) as usize;
    for i in 0..count.min(MAX_CLASSES) {          // <-- up to 128 iterations
        if unsafe { CLASSES[i].id } == lock_addr { return Some(i as u16); }
    }
    ...
```

and `find_class` — called from `lock_release` — is the same scan again. So every
lock operation in the kernel walks the class table **twice**, and `MAX_CLASSES`
is 128. This is exactly the "linear scan on a hot path" CLAUDE.md's performance
section forbids, hiding inside the *debugging* infrastructure rather than the
code being debugged, which is why no amount of reading the subsystem under
investigation would ever have found it.

Two further consequences worth stating because they distort the whole benchmark
suite:

* **The cost is positional.** A lock class registered early is found in a few
  iterations; one registered late pays the full scan. So the same lockdep call
  is cheap or expensive depending on *boot order*, and a benchmark's own lock —
  registered last, at benchmark time — pays the worst case. The 400 ns figure is
  therefore an upper bound on the average, not the average.
* **Every benchmark in this suite that takes a lock is partly measuring this.**
  `syscall_dispatch` (653–699 ns), `futex_wake_empty` (953 ns) and the VFS
  numbers all include it.

**The fix** (implemented in the same change as this entry): an open-addressed
hash index from lock address to class slot, Fibonacci-hashed and linearly
probed, 512 buckets for 128 classes so the load factor stays at 25%. This is
what Linux does (`classhash_table`, `kernel/locking/lockdep.c`). Entries are
append-only, so a probe run is contiguous and stopping at the first empty bucket
is correct.

**This fix is what makes the tempting question go away.** The obvious reaction to
"lockdep costs 400 ns per lock" is to gate it to debug builds, as Linux does with
`CONFIG_PROVE_LOCKING` — trading deadlock detection in production for lock speed.
That would have been a real architectural fork worth escalating. It is moot: the
validator was never inherently expensive, its index was. Keep both.

**The optimisation is guarded by a test that can actually fail.** A hash that
silently *misses* a registered class is the dangerous failure: `find_or_register_class`
would then register a second class for the same lock, that lock's dependency
edges would split across two graph nodes, no cycle would ever be found through
it, and lockdep would go quiet — looking exactly as healthy as a kernel with no
deadlocks. So the linear scan is not deleted, it is demoted to an oracle:
`test_class_hash_index()` asserts the hash and the scan agree on every registered
class, agree on absence, that double registration yields one class, and — using
a colliding address it *searches for* rather than hopes for — that the probe
sequence survives a bucket collision.

#### PROSPECTIVE PREDICTION (recorded before the fix is booted)

1. `lock_tracked` drops from ~632 ns to **250–330 ns**, i.e. close to the
   measured `no-lockdep` figure (232 ns) plus a hash lookup and probe (~2 memory
   references, call it 20–80 ns under TCG). If it lands *below* 232 ns something
   is wrong — the index cannot be cheaper than not running at all.
2. `lockdep` in the overhead split drops from ~400 ns to **< 100 ns**.
3. The knock-on: `syscall_dispatch` (653–699 ns across four boots, target 200)
   improves by **at least 15%**, because it takes tracked locks. This is the
   riskiest of the three — if syscall dispatch does *not* move, then either it
   takes no tracked lock or the lock is registered early enough to have been
   cheap already, and the "every benchmark is partly measuring lockdep" claim
   above is overstated and must be narrowed.
4. `lockdep classes registered` (newly printed) comes out **> 40**. If it is in
   single digits, the scan was never long and the 400 ns has some *other* cause
   inside `lock_acquire` — most likely `smp::current_cpu_index()` or the
   re-entrancy guard — and this whole diagnosis is wrong.

#### RESULT — 2026-08-14, release boot ✅ FIXED

```
[lockdep]   class hash: OK (3 classes verified vs scan, bucket collision handled)
[bench]   lock acquire+release: raw 25ns, tracked 274ns, no-lockdep 223ns, no-stats 301ns
[bench]   lock context: 43 lockdep classes registered
[bench]   lock overhead: total +249ns = lockdep 51ns + preempt 29ns + rdtsc 56ns + unexplained 113ns
[bench] SCORE lock_uncontended 274 500 PASS
```

| # | prediction | before | after | grade |
|---|---|---|---|---|
| 1 | `lock_tracked` **250–330 ns** | 632 | **274** | **HIT** |
| 2 | lockdep's share **< 100 ns** | 400 | **51** | **HIT** (7.8x) |
| 3 | `syscall_dispatch` improves **≥ 15%** | 653–699 | **699** | **MISS** (0%) |
| 4 | **> 40** classes registered | — | **43** | **HIT** |

**The tracked mutex went from 21x the raw spinlock to 11x, and
`lock_uncontended` moved from OVER to PASS** (274 vs the 500 ns target). Knock-on
in the same boot: `vfs_stat_root` 4394 → **3344 ns**, so with the namespace fast
path the total on that benchmark is **5930 → 3344, −44%**.

**(3) MISS, and the pre-registered consequence is honoured rather than
explained away.** The prediction said: *"if syscall dispatch does not move, then
the 'every benchmark is partly measuring lockdep' claim is overstated and must be
narrowed."* It did not move — 699 ns, identical to the best of the four pre-fix
boots. **Narrowing it: the claim was overstated.** Lockdep taxed benchmarks that
take `crate::sync::Mutex` *specifically*, which is the VFS/namespace path, not
"every benchmark that takes a lock". `syscall_dispatch` evidently takes none, or
takes a different lock type (`PreemptSpinMutex`, which is a distinct type with
distinct overhead — a distinction this session already had to write a comment
about in `bench.rs`). `syscall_dispatch` at 3.5x its 200 ns target is therefore
still unexplained and remains open.

**The coherence gates from `TD-BENCH-STAGE-SPLIT-HAS-NO-COHERENCE-CHECK` shipped
in the same boot and reported a clean run:** drift 3331 → 3353 ns (0%),
parts/whole 96%. That is the *quiet* outcome, so it proves only that the gates do
not fire spuriously — **it does not prove they fire.** They have not yet been
observed rejecting a run, and until they have, they carry exactly the weakness
this file keeps documenting. The two incoherent runs that motivated them are
recorded above, so the next drifting boot is the test.

**Weak spot in the new test, recorded rather than glossed:** `test_class_hash_index()`
runs from `lockdep::self_test()`, which executes early in boot when only **3**
classes are registered — but the pathology it guards against (a probe run
walking into a collision) needs a *populated* table, and by benchmark time there
are 43. The synthetic collision case is what carries the test today; the
verify-every-registered-class part is checking 3 of the eventual 43. It should be
re-run late in boot as well. Tracked as
`TD-LOCKDEP-HASH-TEST-RUNS-BEFORE-THE-TABLE-IS-POPULATED`.
