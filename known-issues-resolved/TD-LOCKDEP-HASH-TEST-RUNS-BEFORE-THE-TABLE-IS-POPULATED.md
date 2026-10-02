### TD-LOCKDEP-HASH-TEST-RUNS-BEFORE-THE-TABLE-IS-POPULATED — 2026-08-14 — ✅ FIXED 2026-08-14 (`kernel/src/lockdep.rs`, `kernel/src/main.rs`)

`test_class_hash_index()` verifies the O(1) class index against a linear-scan
oracle, but it is called from `lockdep::self_test()` during early boot, when the
class table holds **3** entries. By the time the kernel is doing real work it
holds **43**. So the "every registered class is found at the index the scan
reports" assertion — the one that would catch a probe-sequence bug — is
exercised at 7% of the table size it needs to defend.

The synthetic part of the test (register a fresh address, then register a
deliberately colliding one and check both resolve) does not depend on table size
and is doing the real work today. That is why this is tech debt and not a hole:
the collision path *is* covered, just not at realistic occupancy.

**Proper fix:** expose it as `pub fn verify_class_index()` and call it a second
time late in boot — after driver/subsystem init, when the table is full — so the
oracle comparison runs against all 43 classes. It must run on every boot, not
only `--bench` boots, or it inherits the "check that only runs when you're
already looking" problem.

> **Resolution.** Done as described; the call takes a `when` label so the two
> runs are distinguishable in the log and the vacuous early pass cannot be
> misread as the meaningful one:
>
> ```
> [lockdep]   class hash (early): OK (3 classes verified vs scan, bucket collision handled)
> [lockdep]   class hash (populated): OK (31 classes verified vs scan, bucket collision handled)
> ```
>
> **The placement was itself the interesting part, and got it wrong on the first
> attempt.** The late call went in next to the deferred-benchmark spawn, which
> reads as "late in boot" — but that sits *after* `BOOT_OK`, and
> `boot-test.sh` kills QEMU at `BOOT_OK` unless `--bench` is given. So the first
> version printed nothing on a normal boot test: a check that would have run only
> on benchmark boots, i.e. only when someone was already looking, which is the
> precise failure mode it was added to prevent. Moved above the `BOOT_OK` marker,
> with a comment at the site saying why it must stay there. Verified by the
> absence-then-presence of the line across two boots, not by reading the code.

**Residual, not worth a separate entry:** 31 classes at `BOOT_OK` versus 43 by
benchmark time — the last dozen register during post-boot activity. Coverage is
now 72% of the eventual table rather than 7%, and the synthetic collision case
covers the probe path independently of occupancy.
