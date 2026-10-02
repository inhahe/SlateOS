## B-BOOT-TEST-HANGS-INTERMITTENTLY-WITH-A-QEMU-GLIB-HANDLE-ERROR, AND LOOKS EXACTLY LIKE A KERNEL REGRESSION (lane B, 2026-08-16)

**Status: OPEN — host-level, cause not identified. Recorded so the next lane
that hits it does not spend the time I did blaming a code change.**

### What happened

A boot test of `lane-b` at `462cd7d09` (a merge of `origin/main`) timed out at
1800s, having hung at Path Z Part 20:

```
[spawn] Running REAL dash shell background job `/bin/emit > file & wait` (ring 3, Path Z) test...
```

The immediately preceding boot of `6039c7d03` had PASSED in 405s. The obvious
reading — "the merge broke it" — was wrong. A re-run of the *same commit*,
changing nothing, passed: the rung logged `OK` at serial line 20812, 27 lines
after the point the previous run stopped at.

### Why it was convincing as a regression, and how each theory died

The merge pulled in lane A's virtqueue rework (`3f0c22500`), which moved the
descriptor free list out of device-visible memory into an Ada-backed pool.
`... & wait` with a redirected stdout is disk I/O, so a descriptor leak was a
plausible fit — and lane B's image carries bash *and* pkgconf, so it does far
more I/O than lane A's, which would explain why lane A's own boot test passed.

It was still wrong. Two hypotheses, both refuted by direct evidence:

| theory | refuted by |
|---|---|
| virtio descriptor leak / mis-completion | `grep -c "\[virtio\]"` on the hung serial log → **0**. The `ada::is_allocated` rejection path at `kernel/src/virtio/queue.rs:595` logs whenever it fires; it never fired. `[ada] FFI boundary self-test … : OK` |
| exit-hook table exhaustion | the log's one `[sched] WARNING: exit hook table full (8 slots)` is at line **284**, immediately followed by the self-test's own cleanup unregisters. It is step 6 of `test_exit_hooks` (`kernel/src/sched/mod.rs:7942-7976`) deliberately overfilling the table to prove rejection works, and a *passing* log contains it too. Only two real registrants exist in the kernel — `pacct.rs:131` and `sched/supervisor.rs:219` — both one-time at boot |

**The lesson worth keeping: an expected-and-benign log line that contains the
word WARNING is a magnet for a wrong diagnosis.** `exit hook table full` reads
as a resource-exhaustion bug and is in fact a self-test asserting a limit. It
cost a full investigative pass. A self-test that provokes a warning it expects
should say so on the same line — e.g. `(expected)`.

### The actual signal

QEMU emitted, on the hung run only:

```
GLib: WaitForMultipleObjectsEx failed: The handle is invalid
```

and then **overran its own 900s `-serial` timeout**, running until the outer
`run-timeout.py` budget of 1800s killed the tree. That is a host-level fault:
QEMU's GLib main loop lost a handle it was waiting on. A kernel that hangs does
not stop QEMU's own timeout from firing, so the overrun points away from the
guest.

### How to tell the two apart, next time

Cheap and decisive, in this order:

1. **Did QEMU blow past its own `TIMEOUT`?** If the wrapper killed it rather
   than QEMU exiting on schedule, suspect the host. A guest hang still lets
   QEMU's timeout fire on time.
2. **Is `GLib: WaitForMultipleObjectsEx failed` in the run log?** Only present
   on the flaky run here.
3. **Re-run the identical commit before filing anything against another lane.**
   One data point is not enough to accuse another lane's change, and the
   re-run is ~7 minutes.

### What is not known

Why the handle goes invalid. Candidates not investigated: host memory
pressure from a concurrent build, an antivirus scan touching the disk image
mid-run, or an interaction with the boot-lock wait loop that had just released.
The frequency is unknown — one occurrence in this session's runs. If it recurs,
capturing QEMU's stderr separately from the serial log would narrow it.

### Interaction with the stale-lock issue

The hung run was killed by the Job Object, so it never released the boot lock,
and the re-run then sat for 5 minutes waiting on a dead owner before I cleared
it by hand — see
`B-TIMED-OUT-BOOT-TEST-STRANDS-THE-CROSS-WORKTREE-LOCK-FOR-20-MINUTES` and
`requests/b-a-boot-lock-survives-its-dead-owner.md`. The two compound: a flaky
hang costs the 1800s timeout *plus* up to 20 minutes of the next run's time.

**The stale-blocker advisory flags this entry, and it is a false positive.**
`boot-test.sh`'s advisory gate reports entries citing a `requests/` file that
reports itself finished, and this one cites
`b-a-boot-lock-survives-its-dead-owner.md`, which landed 2026-08-16. But the
citation above is a *compounding cost*, not a dependency: the lock bug is fixed
and this entry's own defect -- an intermittent host-level QEMU/glib hang, cause
still unidentified -- is untouched by it. Checked 2026-09-14 by lane A; written
down so the next reader does not re-derive it. The advisory's rule is worth
keeping as it is: it cannot tell a blocker from a mention without reading the
prose, and flagging a few resolved-looking entries is cheaper than missing a
real one.
