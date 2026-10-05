### A-FASTPY-FORKEXEC-PARENT-MISSES-THE-4000-YIELD-POLL-BUDGET. `self_test_fastpy_forkexec` failed once in 15 boots: the guest printed its success line, but the parent task had still not exited when the harness gave up polling — 2026-08-26 — **Status: OPEN (one occurrence, cause not established)**

**In short:** the kernel runs a start-up self-test that launches a small Python
program, has it start a second program, and waits for the first one to finish.
On one boot the program did its whole job and said so, but the kernel gave up
waiting a moment too soon and declared the test failed. The boot was otherwise
completely clean. It has happened once and has not been reproduced, so it is
recorded rather than diagnosed — and this entry exists mainly so the *next*
occurrence is recognised as the second one rather than investigated from
scratch.

**Where.** `kernel/src/proc/spawn.rs`, `self_test_fastpy_forkexec` — the poll
loop at ~9261 and the verdict at ~9277:

```rust
let mut became_zombie = false;
for _ in 0..4000 {
    if pcb::state(result.pid) == Some(pcb::ProcessState::Zombie) { became_zombie = true; break; }
    crate::sched::yield_now();
}
let state = pcb::state(result.pid);
…
thread::on_thread_exit(result.task_id);   // ← this force-reaps the parent
pcb::destroy(result.pid);
if !became_zombie || state != Some(pcb::ProcessState::Zombie) { … FAIL … }
```

**The observation.** Boot `497f31a17` (debug, QEMU TCG, 430 s to `BOOT_OK`),
`build/serial-test.txt` lines 3300–3382:

```
[spawn] Created process 192 ("fastpy-forkexec")
[sched] Spawned task 155 …                    ← the parent
[sched] Spawned task 156 … in process 193     ← the fork child
[exec] Process 193 exec complete …            ← the child execs `cat`
fastpy fork+exec+wait OK                      ← the CHILD: `cat` echoing the staged file
[thread] Process 193 has no threads left — now zombie
[sched] Task 156 exiting
[thread] Process 192 has no threads left — now zombie
[spawn]   FAIL: fastpy-forkexec (ring 3) — expected Zombie, got Some(Running)
```

**Read the last two lines in the right order or the diagnosis inverts.** It
looks as though process 192 became a zombie and the harness then failed it
anyway, i.e. a stale read. It is not that. `[thread] Process N has no threads
left — now zombie` is printed by `thread::on_thread_exit`
(`kernel/src/proc/thread.rs:767`), and the *harness itself* calls
`on_thread_exit(result.task_id)` at spawn.rs:9273 — after capturing `state` at
9270 and before printing the verdict at 9278. So that zombie line is the
harness force-reaping the parent, not the parent exiting on its own. The read
was not stale; task 155 really had not exited.

**What is not the cause.** The commit under test changed only
`parse_blkread_args` in `kshell.rs` and added self-test rung 86. The spawn
self-tests run at serial line ~3300; `kshell::self_test 86` runs at line 40962.
The changed code executes roughly 37,000 serial lines *after* the failure, so it
cannot have contributed. The 14 boots immediately preceding this one were all
`PASS`, and the boot in question was clean everywhere else — 0 `!! ` lines,
`kshell::self_test PASSED`, `BOOT_OK detected`.

**Two candidate causes, not distinguished.**

1. *Budget too tight.* 4000 `yield_now()` calls is a fixed count, not a
   deadline, so what it is worth in real time depends on how much else is
   runnable. This boot took 430 s to `BOOT_OK` against a recent median of ~400 s
   and was competing with a concurrent `cargo`/gate run on the host. A
   fastpy parent has an interpreter to tear down after its last `print`, and
   that teardown is not free.
2. *Same family as `B-FORKEXEC-BOOT-HANG`.* That entry documents the exit path
   of a just-reaped process wedging, with a suspected raw-spin
   holder-preemption deadlock (Q24). The signature there is a **silent** stop
   with no further output; here output continued and the boot completed, so it
   is not the same event — but "the last thread of a fork/exec parent is slow
   or stuck leaving" is the same neighbourhood, and this may be a survivable
   instance of it.

**Correction, made the same day, before this entry had been acted on — and it
changes which cause is likely.** The first version of this entry read
`fastpy fork+exec+wait OK` as the *parent's* success line and built a
hypothesis on it: that the line appears before the child's zombie line, which
would mean `waitpid` returned before the child was reaped. That is wrong twice
over. The runner program (`services/fastpy-forkexec/build.py`, the embedded
source at lines 78–96) **prints nothing at all** — it ends
`sys.exit(os.WEXITSTATUS(status))`. And `FE_CONTENT` in spawn.rs:9208 is
literally `b"fastpy fork+exec+wait OK\n"`: the harness stages that text in
`/tmp/forkexec-input.txt`, and the line on serial is `cat` — *the child* —
echoing the file it was handed. There is no ordering anomaly and no early
`waitpid`.

**The corrected reading is worse for cause 1, not better.** With the line
attributed to the child, the log says the child had printed its output, exited,
zombified *and* had its task torn down — and the parent still had not left
`waitpid` when the budget expired. The parent had produced no output of its own
and had no interpreter teardown left to blame, because it never got past the
wait. So "the fastpy parent needed a few more yields to finish exiting" is not
supported by anything in the log; what the log shows is a parent still blocked
in `wait4` after the event it was waiting for had fully completed.

That makes **a lost wakeup on the guest's `wait4` the leading hypothesis**, and
it is worth being explicit that this is *not* excluded by the static audit under
`B-FORKEXEC-BOOT-HANG`. That audit ruled out a lost wakeup on the ground that
"the kernel harness … does not block — it *polls*", which is true and remains
true. It says nothing about the **guest parent**, which genuinely does block in
`waitpid`. The two are different waiters and only one of them was cleared.

**Deliberately not "fixed" by raising the budget.** Changing `4000` to a bigger
number would make the symptom rarer without establishing which of the two
causes is real, and if it is cause 2 the loop would be papering over a kernel
bug with a longer wait. Note also that the harness reports a *poll timeout*
with the words "faulted on the fork / execv / waitpid path", which is a
misdiagnosis of exactly the kind §600 is about: nothing faulted.

**Next step on recurrence.** Print the parent's `pcb::state` *and its scheduler
state* at the moment the budget expires — the distinction that matters is
`Ready` (merely starved: cause 1) versus `Blocked` (parked in `wait4` with the
child already reaped: a lost wakeup). The harness currently records only
`pcb::state`, which reports `Running` for both and so cannot tell them apart;
that is the single cheapest instrumentation change and should be made whether
or not this recurs. Then re-run with `--hard-lockup-watchdog` to capture the
guest RIP.

If it is cause 1, the fix is a *deadline* (a tick-based timeout) rather than a
yield count, plus a verdict line that says "still Running after N ms" instead of
blaming the fork/exec path. If it is the lost wakeup, the place to look is the
wait/reap side: whether the child's zombification wakes a parent already parked
in `wait4`, and whether the `pending_wake` flag in `sched/mod.rs` (which closes
the register→park window for the core protocol) is actually on the path
`wait4` uses.

**Note on the evidence.** `build/serial-test.txt` is overwritten by the next
boot, so the log quoted above no longer exists on disk. The excerpt in this
entry is the whole of the surviving record — which is the argument for quoting
generously here rather than referring to a file.
