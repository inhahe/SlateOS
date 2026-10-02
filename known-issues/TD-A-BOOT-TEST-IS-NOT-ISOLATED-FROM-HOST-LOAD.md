### TD-A-BOOT-TEST-IS-NOT-ISOLATED-FROM-HOST-LOAD — a concurrent `cargo` run can fail an otherwise-clean boot — PARTLY FIXED 2026-08-24 (lane A, `026d61d9a`) — see the correction at the end of this entry

**In short:** the boot test runs the kernel inside QEMU, which is a program on
this machine competing for the same CPUs as everything else. If you start a
Rust build while a boot test is running, the emulated machine runs about half
as fast — and the kernel has a watchdog that gives up when the boot takes too
long. So the boot test can report a hang that never happened. It did, on
2026-08-24, and the cause was me: I ran `cargo check` and `cargo clippy` to
fill the wait.

**Observed.** Boot test batch40 (`5fbbd539b`) reached `BOOT_OK` and every
self-test passed, but the harness returned FAIL:

```
BOOT_OK detected after 665s!
LIVENESS WATCHDOG failure detected in serial log:
[liveness] BOOT DEADLINE EXCEEDED: still armed 805s after arming (no BOOT_OK).
=== Boot test FAILED (BOOT_OK reached but the liveness watchdog reported) ===
```

The armed window is logged at disarm, so the two runs can be compared directly:

| Boot | armed window | deadline | host activity during QEMU |
|---|---|---|---|
| batch37 | **420.5 s** | 826 s | idle |
| batch40 | **839.6 s** | 805 s | `cargo check --release` + `cargo clippy --release` |

Same battery, 2.0× the guest time, and the only variable was two release-mode
`rustc` invocations on the host. The kernel diff between the two builds is a
~40-line self-test that runs once, right after heap init.

**Why it fails rather than merely running slow.** `liveness_arm` derives its
deadline as `harness_timeout − 45 s margin − (now at arm)` — deliberately, so
that growing the self-test battery can never desynchronise the watchdog from
the harness's kill (see `sched/mod.rs` and the `BUG-LIVENESS-DEADLINE-FALSE-FIRE`
history). That derivation is sound, but it assumes the *rate* at which guest
time is consumed is roughly a property of the battery. Host contention breaks
the assumption: the battery did not grow, the machine got slower, and the
watchdog cannot tell those apart from inside the guest.

**What to do meanwhile — this is the actionable part.** Do not run `cargo`
(or anything else expensive) while a boot test is running. It is tempting,
because a boot is ~8–25 minutes of apparent idleness, but the cost is a
false FAIL that then costs a full re-run to disprove — strictly worse than
having waited. Read code, edit documentation, or edit sources *without
building* instead.

**Proper fix, unresolved.** Three candidates, none obviously right:

1. **Make the harness serialise against local builds** the way it already
   serialises QEMU across lanes (`scripts/boot-test.sh` takes a boot lock).
   A build lock would have to be taken by every `cargo` invocation to work,
   which means wrapping cargo, which is invasive and easy to forget.
2. **Have the watchdog measure progress rather than time** for the boot
   deadline — e.g. deadline in "self-tests completed" rather than seconds.
   Robust against host load by construction, but it stops being a backstop
   for the hang modes that *do* keep completing self-tests.
3. **Let the harness pass a slack multiplier** when it knows the host is
   loaded. Requires the harness to know, which it does not.

---

### Correction, 2026-08-24 — the diagnosis above is wrong, and one real bug under it is now fixed (`026d61d9a`)

**In short:** I filed this as "the machine got slower and the watchdog cannot
tell that apart from a hang." That is not what happened. The harness was
measuring time with a broken ruler, and the kernel with a good one, and the two
were being compared as though they were the same ruler.

**The bug.** The QEMU wait loop in `scripts/boot-test.sh` counted its own
iterations — `sleep 1`, then `ELAPSED=$((ELAPSED + 1))`. An iteration is not a
second: it is the sleep *plus* a `grep -q` over a serial log that reaches
2.7 MB, plus the stall-tracking `stat`. Measured against the same script's
epoch stamps:

| Boot | host | `ELAPSED` at BOOT_OK | guest armed + arm | real QEMU wall | undercount |
|---|---|---|---|---|---|
| batch40 | loaded | 665 s | ~890 s | 903 s | 26% |
| batch41 | idle | 349 s | ~472 s | 465 s | 25% |
| batch42 | idle | 439 s | 549 s | 563 s | 22% |

**The guest's clock is accurate** — within ~2.5% of real time on every run. The
drift was entirely in the harness, and it is present on an *idle* host, so it
was never really about contention at all. Host load only widened a gap that was
always there.

**Why that produced a false FAIL.** The harness passes `$TIMEOUT` to the guest
as `sched.boot_deadline_ms`, and `liveness_arm` derives
`deadline = timeout − 45 s − now_at_arm` in *real* monotonic nanoseconds. With
`$TIMEOUT` denominated in slow iterations, the guest's deadline landed hundreds
of seconds before the harness's kill rather than the 45 s the design intends.
So on batch40 the watchdog fired at ~860 s real while the harness still
believed it had a quarter of its budget left. The watchdog was not confused by
contention; it was correctly applying a deadline that had been handed to it in
the wrong units.

Two further consequences, both live until this fix: **`$TIMEOUT` did not bound
wall time** (batch40's "900 s timeout" permitted 903 s and would have permitted
~1200 s — the kill under-fires exactly when a run most needs it), and
**`"BOOT_OK detected after Ns"` was systematically low**, which matters because
that is the figure a reader quotes when comparing two boots.

**What the fix does and does not buy.** `ELAPSED` is now computed from an epoch
stamp, so both sides measure the same thing. It does **not** make a loaded
host's boot pass: batch40 genuinely needed 903 s of an 855 s allowance, and no
clock change invents time. What it buys is a truthful verdict — "this boot
exceeded its wall-clock budget", which points at the budget — instead of a
watchdog report that reads as a hang and sends the next reader into
`sched/mod.rs`.

**The three candidate fixes above are therefore mostly answered.** (2) is moot:
the watchdog's time base was never the problem. (3) is moot: the slack it would
have added was an attempt to compensate for the drift now removed. (1) remains
genuinely open, and is now the *only* open part — a `cargo` run beside QEMU
still steals CPU from a TCG emulator that is CPU-bound and single-threaded, and
that still shows up as a longer boot. It just no longer shows up as a *lie*.

**The interim rule still stands** and is still the cheap answer: do not run
`cargo` while a boot test is running.

Candidate 2 is the most principled and the least compatible with the
watchdog's stated purpose ("catches *any* hang mode"). Recorded rather than
decided; the failure is cheap to avoid by hand today.
