## A-A-GATE-BUILT-TO-CATCH-UNOBSERVED-VERDICTS-ISSUED-ONE (lane A, 2026-09-06) — **FIXED** `8f09124fd`

**In short:** a check that guards the boot test against a specific kind of
mistake made exactly that mistake, and accused the thing it was guarding. The
boot test copies itself to a snapshot before running, so that editing the
script mid-run cannot corrupt a two-hour job. `check-boot-test-reexec.sh`
proves that copy works. On 2026-09-06 it reported **"the snapshot did not
isolate the run"** — i.e. the protection was broken. It was not. The harness
had raced, and reported the race as a property of the code.

### The signature worth recognising

**It passed standalone, twice, on the same commit that failed inside the boot
test.** That combination is diagnostic: when a check disagrees with itself
depending on who invoked it, the thing under test is the harness, not the
subject. Both times today that pattern appeared, the harness was at fault — the
other being `BOOT_TEST_REEXEC` leaking from the parent boot test into the
child and disabling the very branch under test (fixed earlier, `20df7965d`).

### The mechanism

The checker launches an editor that overwrites the script mid-run, then checks
whether the running process saw the edit. The editor fired on a **fixed one
second sleep** — a bet that the guarded script would reach its snapshot within
a second.

A boot test's gate phase is hundreds of short-lived processes hammering the
disk, and under that load `mktemp` + `cat` + `chmod` + `exec` can take longer.
The edit then lands *before* the snapshot is taken, the snapshot faithfully
captures an already-edited file, and the run prints `CLOBBERED`. The mechanism
worked perfectly; the harness had measured the wrong instant.

### The fix, and two things that cost an attempt each

The payload now announces itself and the editor waits for that announcement, so
the snapshot is provably taken before any edit, at any load. Two subtleties,
both invisible in prospect and obvious in retrospect:

1. **The replacement script must share a byte-identical *prefix* with the
   payload.** The whole mechanism is bash re-reading at a saved file offset, so
   adding a signal line to the payload without adding it to the replacement
   shifted every following byte and the offset stopped landing on the
   `CLOBBERED` line. That failed loudly (the control went `INCONCLUSIVE`),
   which is the only reason it was cheap.
2. **The signal must be cleared *before* the editor is armed, not after.**
   Cleared afterwards, the guarded run's editor saw the *control* run's
   leftover signal, fired instantly, and edited before the snapshot — exactly
   reproducing the false accusation, from inside the fix for it.

### The generalisation

**A harness that measures a race by waiting a fixed interval is making a claim
about the machine, not about the code — and it will make that claim most
confidently on the machine state it was written under.** Every such delay is an
unstated assumption about scheduling, and it is wrong precisely when the
machine is busiest, which is when the gate matters most. Where a synchronisation
point is available, a sleep is not a simpler alternative to it; it is a quieter
one.

Related, same day, same shape: `A-A-A-BOOT-TEST-ONLY-GATE-DOES-NOT-EXIST-FOR-A-LANE-THAT-NEVER-BOOTS`.

---
