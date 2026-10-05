### B-KASAN-INSTRUMENTED-BOOT-WEDGES-MID-PRINT-ON-A-PAGE-FAULT. The instrumented kernel spins forever with a half-printed `#PF` line, and the report path has no way to say anything more — 2026-08-12 — WATCHLIST (did not reproduce 2026-08-14; harness now armed to capture the RIP)

**Reproduce.** `./scripts/kasan-build.sh --boot`. Observed once so far, at the
same place; not yet known whether it is deterministic.

**Symptom.** Serial output stops permanently in the *middle* of a line:

```
[spawn]   fastpy-on-SlateOS `ftype` (ring 3: file-type via os.path.isfile/isdir …): OK
EXCEPTION: Page Fault (#PF) at
```

— truncated exactly where `{:#x}` would have formatted `frame.rip`. Evidence
preserved at
`build/hang-catches/kasan-wedge-20260812-pf-midprint.serial.txt`
(347929 bytes, 5560 lines).

It is a spin, not slowness: three samples over ~90 s showed the byte count
frozen at 347929 while QEMU's CPU time climbed 591.9 → 594.9 → 597.7 s. A
reference *uninstrumented* boot has no exception at this point at all, so the
fault is either instrumentation-induced or a latent bug that the ~20× slowdown
reschedules into existence.

**Suspected mechanism.** `mm::kasan_rt::report` and the `#PF` handler both print
through `serial_println!`, which takes a spin mutex, and **neither has a
re-entrancy guard**. Any fault or KASAN report raised *while that lock is held*
deadlocks on the spot, with the partial line already emitted — which is exactly
the observed shape. The likeliest sequence is: `#PF` handler starts printing →
formatting `frame.rip` touches something that faults again (or trips an
instrumented access that calls `report`) → the nested printer spins on the lock
its own caller holds.

**Why it is bad beyond the hang.** This failure mode destroys its own evidence.
The one thing the operator needs — the faulting RIP — is the token that never
got printed, and the wedge then prevents any later mechanism from reporting it.

**Two separable fixes:**

- **(a) Give the serial printer a re-entrancy escape.** ✅ **DONE 2026-08-12**
  (`58102abca`). If the lock is already held by this CPU, fall back to the
  unlocked/polled emergency writer (`emergency_println!` already exists for the
  hard-lockup path) instead of spinning. Turns an evidence-free wedge into an
  interleaved-but-complete report. This is the one worth doing regardless of the
  underlying fault, since it is a diagnosis multiplier for every future wedge,
  not just this one. `serial::_print` now keeps a per-CPU `IN_PRINT` flag,
  claimed *before* the lock is taken (so the window in which this CPU is merely
  *waiting* for the lock is also covered), and a nested call from the same CPU
  writes through `SerialPort::emergency()`. `serial::reentrancy_self_test()`
  guards it at every boot by raising `#BP` from inside a `Display::fmt` — the
  faithful reproduction of the failure, since the fault is taken *during the
  formatting of an argument*, not merely during the write.
- **(b) Find the actual fault.** Was blocked on capture: `scripts/boot-test.sh`
  attaches the HMP monitor and `capture_guest_state()` **only** when
  `HARD_LOCKUP_WATCHDOG=1`, and the original run was launched without it, so no
  live RIP could be read from the wedged guest.

  **The re-run was not merely forgotten — it was unrequestable.**
  `kasan-build.sh --boot` `exec`'d `boot-test.sh --no-build` with a fixed,
  empty argument list, so the instrumented profile — the one most likely to
  wedge, and therefore the one that most needs the diagnostic options — was the
  only profile that could not ask for them. Fixed 2026-08-14 (`2db09232a`):
  everything after `--` is forwarded verbatim, so the capture run is now

  ```bash
  ./scripts/kasan-build.sh --boot -- --hard-lockup-watchdog --stall-secs=240
  ```

  `--stall-secs` matters as much as the watchdog here. A wedge is defined by
  serial output *stopping*, and an instrumented boot is legitimately ~20x
  slower, so waiting for the outer timeout to distinguish "wedged" from "slow"
  wastes the whole budget; the stall detector calls it after 240 s of silence
  and captures the frozen RIP on that path (`boot-test.sh:739-753`) rather than
  only on timeout.

  Note that fix (a) landed *after* the only observed occurrence, so the wedge
  may no longer reproduce in its original evidence-destroying form: if the
  nested print now escapes to the emergency port, the run should emit the
  complete `#PF` line — the faulting RIP included — and then either recover or
  fail in some new, legible way. Either outcome is progress; a silent
  half-printed line is the one result that is now unexpected.

**Not the same bug as B-KASAN-INSTRUMENTED-BUILD-PANICS-ON-ITS-OWN-REDZONE-CHECKS.**
That one flooded `[kasan] CRITICAL` reports and panicked; this run reached 5560
lines with **zero** such reports, confirming the `mm::rawmem` fix landed. The
wedge is a distinct, later failure.

**Not caused by `mm::rawmem`.** An ordinary (uninstrumented) boot of the same
tree — which compiles and exercises `rawmem` identically — reached `BOOT_OK` in
273 s.

---

**Capture run 2026-08-14 — DID NOT REPRODUCE. Downgraded OPEN → WATCHLIST.**

The re-run fix (b) asked for was finally *requestable* once `2db09232a` taught
`kasan-build.sh` to forward flags, and it was run as:

```bash
./scripts/kasan-build.sh --boot -- --hard-lockup-watchdog --stall-secs=180
```

**Result: the instrumented kernel booted clean, end to end.** `BOOT_OK` after
**1938 s**, 26094 lines of serial (`build/serial-kasan-pass.txt`), exit 0. This
is, as far as the logs show, the **first complete KASAN-instrumented boot this
project has achieved** — the shadow was live for the entire run, and the `[kasan]`
self-test battery passed all five checks with `violations=7, shadow_frames=3,
poisoned=112B, unpoisoned=60B, map_lock_giveups=0` (all seven violations are the
self-test's own deliberate probes; there was not one unexpected shadow report in
the whole boot). The instrumented kernel binary is preserved at
`build/kernel-kasan-capture.elf` for symbolizing any future recurrence.

The decisive detail is **where** it got past. The original wedge died at ~line
5560, mid-print, inside the `ftype` test. This run printed that same test's
result complete at line **5562** and continued for another twenty thousand
lines.

**Fix (a) did not rescue it — the fault simply did not happen.** This matters,
because "the fix worked" and "the bug is nondeterministic" predict different
logs and only the second one matches. Had a nested fault occurred and been
caught by the `IN_PRINT` fallback, the emergency port would have emitted the
*complete* `EXCEPTION: Page Fault` line; that is the entire purpose of fix (a).
No such line exists anywhere in the 26094. The only `#PF` in the log is line
1258, the intentional ring-3 fault-handling self-test. So the page fault that
truncated the original run never occurred here at all.

That answers the entry's own open question — *"not yet known whether it is
deterministic"* — with **nondeterministic**, and it answers it without yielding
a root cause. A one-in-N fault under a profile that takes ~32 min per attempt is
not something to chase blind.

**Why WATCHLIST rather than FIXED.** Nothing was diagnosed. What changed is that
the failure is now *survivable evidence* instead of a dead end: the harness is
armed (watchdog + `--stall-secs`), fix (a) guarantees a nested fault escapes to
the emergency port with its RIP intact, and the matching instrumented binary is
kept. If it recurs, one run yields the faulting instruction. Until then there is
nothing actionable, and re-running a 32-minute boot hoping to lose a coin flip
is not a use of the boot lock.

**Secondary result — the KASAN profile is now a usable routine tool.** It had
never survived a full boot before, so it could only ever be pointed at a
suspected bug and hoped at. A clean 26094-line baseline means a future KASAN run
can be *diffed* against this one, which is a categorically better instrument
than "did it crash".
