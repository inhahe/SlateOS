## 274. A health check that has never once passed gets a new question, not a looser threshold: `syshealth` now provokes a page fault instead of counting old ones

**Date:** 2026-08-24
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** The kernel has a `syshealth` command that prints seven pass/fail
rows about its own condition. One of them, "page faults", said FAIL on every
single boot — it counted every fault the kernel had ever failed to fix by
itself, and complained if the number wasn't zero. Two problems. The number was
wrong: it included faults the *program* went on to handle successfully and
carry on from, which are the opposite of a crash. And the question was wrong:
the rest were programs that crashed and were correctly killed, which is the
kernel doing its job, not the kernel being ill. The row now runs a real test
instead — it deliberately triggers a page fault and checks the kernel fixes it
— so it can pass, and so a failure means something an operator can act on.

This is the third bug behind the same `syshealth` exit status as §273's two,
found by the same self-test rung. But its shape is different, and the
difference is the point of this entry. §273's bugs were *a verdict nobody
read*: the row printed FAIL and was ignored because the command still exited 0.
This one would have survived that fix entirely. It reported loudly and
correctly, forever, for a reason nobody could do anything about — which trains
a reader to ignore the row just as thoroughly as a missing exit status does,
and does it while looking like a working check.

### The decision: change the subject, not the threshold

The obvious repair, once the miscounting was fixed, was to keep the counter and
relax what counts — ignore user-fatal faults, or ignore faults raised before
the shell started, or subtract the ones the boot self-tests provoke on purpose
(exactly the remedy §273 applied to lockdep's planted violations).

Rejected, because it patches the symptom of a deeper mismatch. Ask what the
check *could* observe that would be actionable, and the answer is: nothing.

| Kind of fatal fault | Is it a kernel health problem? | Can `syshealth` ever see it? |
|---|---|---|
| Kernel-mode, unresolvable | Yes — always a kernel bug | **No.** That path ends in `halt_loop()`; no shell runs afterwards. |
| User-mode, no handler | No — the kernel correctly killed a bad program | Yes, and it always will, so the row is permanently red |

The one population worth a verdict is structurally invisible, and the one
that's visible is not a verdict. No threshold fixes that. Any tuning would have
produced a row that *usually* passes, which is worse than one that never does:
it would look trustworthy while still being unable to report the thing it
names.

`syshealth`'s own doc comment already had the rule: "Unlike `diag` (which reads
passive counters), this command actively tests kernel subsystem integrity."
Checks 1, 2 and 6 obey it — they allocate a frame and free it, allocate 256
bytes and read them back. Check 7 was reading a counter that `diag` already
displayed. Making it active is not a new idea; it is the file's existing rule
applied to the one row that had drifted from it.

*What changes:* the row reads `[PASS] Demand paging: fault resolved, 16 KiB R/W
OK` on a healthy kernel instead of `[FAIL] Page faults: 5 fatal fault(s)`, and
a FAIL now names a stage (`fault returned but left the address unmapped`) that
points at code rather than at history.

### The counter is still fixed, and split

Independently of where it's read, `record_fatal()` was being called several
recovery attempts too early — see `known-issues.md` for the two faults it
misclassified, both of which were self-tests *succeeding*. The tally moved to
the kill path in `dispatch_or_kill_userspace_raw`, and a second counter,
`FAULTS_DELIVERED`, records faults a userspace handler took.

The alternative was to fold handled faults into `user_resolved`. Rejected: the
memory access never succeeded, so calling it a resolution is a second, quieter
lie in the same field. Three outcomes need three buckets — the kernel fixed it,
the program handled it, nobody did.

*What changes:* `pgfault` gains a "Handled by userspace" line; `diag`'s
PgFaults row reports `handled=` and `killed-task=` and no longer labels the
latter `FATAL FAULTS` or folds it into its issue count.

### Alternatives rejected

- **Delete check 7 and ship six rows.** Honest, and better than the status quo,
  but it gives up a genuinely testable property. Demand paging is on the hot
  path of every process start and every `mmap`; that it still works is exactly
  what an *active* health check should establish.
- **Flag the boot self-tests' deliberate faults, as §273 did for lockdep.**
  Would have made the row green, and would have been the wrong lesson. Lockdep's
  planted violations really are indistinguishable from real ones and really do
  need a flag; here the deliberate faults fall out on the correct side of a
  *correct* classification with no special case at all. Reaching for the same
  mechanism twice would have hidden that the second bug had a different cause.
- **Keep `AddressBusy` as a silent skip returning `Ok(())`,** as the boot-only
  original did. That is this sweep's own bug class — a test reporting success
  for something it never ran — sitting inside the test written to catch it. The
  probe address is reserved and used by nothing else, so it being mapped means a
  leak or a squatter, both real.

**Consequence to watch:** check 7 now *does something* rather than reading a
counter, so it can perturb the system it measures — it takes a real fault, and
allocates and frees a 16 KiB frame per run. That is deliberate (it is what
makes the check meaningful) and it is the same exposure checks 1, 2 and 6
already carry. The risk it adds is a leak on a failure path making every
subsequent run fail for the wrong reason; that specific bug existed in the
boot-only version and is fixed, and `AddressBusy` is the tripwire if it recurs.
