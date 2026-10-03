### BUG-DASH-CMDSUB-INTERMITTENT-HANG. The Path-Z real-dash command-substitution self-test (`echo [$(/bin/emit)] > file`) intermittently hangs the boot thread — no BOOT_OK — but passes on a re-run — 2026-07-23 — OPEN (flaky)

**Symptom.** `self_test_linux_real_glibc_shell_cmdsub` (kernel/src/proc/spawn.rs ~21042, "REAL dash shell command substitution `echo [$(/bin/emit)] > file`") occasionally wedges: dash (the parent process, e.g. pid 272) forks a child (pid 273) which `execve`s `/bin/emit` and becomes a zombie ("Process 273 has no threads left — now zombie", "Task 239 exiting"), and then there is **no further serial output** — the boot never reaches BOOT_OK and the run-timeout watchdog eventually fails the boot at ~540s. On an immediate re-run (identical kernel + rootfs, `--no-build`) the same test **passes** ("read back 18 bytes == expected, exit 0: OK") and the boot reaches BOOT_OK. Observed 2026-07-23: one boot hung here, the very next boot passed it.

**Context / not-a-regression.** This is a **Linux-ABI/glibc** (Path-Z) test, entirely separate from the native-fastpy exec path. It surfaced while validating the fastpy `pipeline` increment, but the fastpy `pipeline`/`capture` self-tests ran and PASSED in the same (hung) boot — the hang is downstream and unrelated. An earlier **non-fatal** liveness watchdog also fired mid-boot in the hung run ("[liveness] SYSTEM HANG: no task-level forward progress for 15+ seconds (useful_work=6, all CPUs idle-ticking)") and the boot then recovered and continued, which points at an intermittent scheduling/wakeup-timing issue rather than a deterministic deadlock.

**Suspected area.** dash's synchronous command-substitution wait uses blocking `waitpid(flags=0)` on the `$( )` child while draining the substitution pipe. Related history: **B-SIG1** (dash `wait`-builtin livelock, resolved 2026-06-16) fixed SIGCHLD-on-exit + a real `rt_sigsuspend` park loop. This looks like a residual **lost-wakeup race** on the `wait4()`/pipe-EOF path: if the child's zombie transition (waiter wakeup) races the parent parking in `wait4()` (or the pipe read-side EOF wakeup races the write-end close), the parent can park after the event and miss it, with nothing to re-arm the wakeup — hence a hang that a slightly different scheduling interleaving (the re-run) avoids.

**Proper fix (to investigate).** Audit the `wait4()` blocking-wait arm and the pipe read/EOF wakeup for a check-then-park TOCTOU: the waiter must re-check the zombie-child / pipe-EOF condition *after* enqueuing on the wait queue (or hold the relevant lock across enqueue+condition-check) so a wakeup delivered between the check and the park is not lost. Reproduce by looping the boot test; if it reproduces, add per-park instrumentation (log the parent's parked-on wait object + the child's wakeup post) to confirm the lost-wakeup ordering, then close the window. Until fixed, a single boot-test failure at this exact test should be re-run before treating it as a real regression.

**Audit done 2026-07-27 — both stated suspects are CLEAN; do not re-audit them.**
The check-then-park TOCTOU hypothesis above does *not* hold for either path:

1. **`sys_wait4` specific-pid arm** (`kernel/src/syscall/linux.rs` ~45544)
   already uses register-then-recheck: `try_reap` → `pcb::set_wait_task` →
   **`try_reap` again** (and re-runs `jc_report_for_child`) before
   `block_current()`, and it registers as a signal-waiter with the same
   recheck idiom. A zombie transition landing anywhere in that window is
   observed by the second `try_reap`.
2. **Pipe read / writer-close EOF** (`kernel/src/ipc/pipe.rs` `read()` ~532,
   close ~1007). The reader tests `write_closed` and installs
   `reader_waiter = Some(task)` under **one** `PIPES.lock()` critical
   section; the closer sets `write_closed = true` and takes
   `reader_waiter` under that **same** lock. The two orderings are
   therefore exhaustive and both safe — either the closer sees the
   installed waiter and wakes it, or the reader sees `write_closed` and
   returns EOF without parking.
3. **The generic net underneath both**: `sched::wake()`
   (`kernel/src/sched/mod.rs` ~1841) does *not* drop a wake aimed at a task
   that is still `Running`/`Ready` — it sets `task.pending_wake`, and
   `block_current()` (~1805) consumes that flag and returns **without
   blocking**. So even a wake delivered strictly between "release the lock"
   and "park" cannot be lost.

**Where to look next instead.** Since no wake can be lost on these paths, the
hang is more likely (a) a wake aimed at the **wrong task** — note
`reader_waiter`/`writer_waiter` are *single slots*, not queues, so a second
blocked reader/writer on the same pipe would be silently forgotten rather than
woken; (b) a **stale `pending_wake`** consumed by an unrelated later park,
making some *other* park return early and leaving the real waiter parked; or
(c) not a lost wake at all but the dash-side `wait4` ↔ pipe-drain **ordering**
(parent blocked in `wait4` for a child that is itself blocked writing into a
full substitution pipe the parent has not drained — a genuine deadlock rather
than a lost wakeup, which would also explain the liveness watchdog firing with
all CPUs idle).

**Lead status (updated 2026-07-27):**

* **(a) single waiter slots — CLOSED, and it was a real bug.** All four
  blocking IPC objects held one `Option<TaskId>` per end; see
  `BUG-PIPE-SINGLE-WAITER-SLOT` below (fixed, regression-tested). It is
  *not* expected to be this hang's cure — the cmdsub test is strictly
  single-reader/single-writer — but the lead is resolved.
* **(b) stale `pending_wake` — CLOSED, and it was also a real bug.** See
  `BUG-TRYWAKE-FALSE-CONFLATES-CONTENTION` below: `try_wake` returned
  `false` both for "lock contended, retry me" *and* for "task wasn't parked,
  I set `pending_wake`". Since every caller spells
  `if !try_wake(t) { defer_wake(t); }`, the second case manufactured a
  **duplicate** wake in the deferred queue that later fired against an
  unrelated park. Fixed and regression-tested. Whether it was *this* hang is
  unproven (the hang is intermittent and has not been reproduced since), but
  it was a genuine spurious-wake generator on exactly the paths dash uses.
* **(c) `wait4` ↔ pipe-drain ordering — IMPLAUSIBLE for this test.** The
  substitution output is **18 bytes**; the write side cannot block on a full
  pipe, so the parent-blocked-in-`wait4`-while-child-blocked-in-`write`
  deadlock cannot form here. Retained only as a hypothesis for a
  *large*-output command substitution, which this test is not.

**Where to look next.** With (a) and (b) fixed — and the residual
single-shot parks they endangered closed in
`BUG-SINGLE-SHOT-PARK-FABRICATES-EVENT` (an audit of all 61
`block_current()` sites; every other one was already a re-check loop) —
re-run the boot test in a loop to see whether the hang still reproduces at
all before investing in new instrumentation. If it does, the remaining shape
is a spurious `pending_wake` that survives its own park (a park loop whose
condition is already satisfied returns without ever calling
`block_current()`, leaving the token set for a later, unrelated park).
Eliminating that class properly needs the token to be scoped to a specific
wait rather than being a bare per-task bool.

**Loop result 2026-07-27 — NOT REPRODUCED in 4 consecutive boots.** After
(a), (b) and `BUG-SINGLE-SHOT-PARK-FABRICATES-EVENT` landed, the boot test
was run four times back to back (one verification boot at 315 s, then a
three-iteration loop: BOOT_OK at 299 s / 331 s / 339 s). Every run passed
`self_test_linux_real_glibc_shell_cmdsub`; the liveness watchdog did not
fire in any of them. That is **not** proof the hang is gone — the original
observation was roughly one failure in two boots on 2026-07-23, so four
clean boots only bounds the current rate below ~20 %, and both fixed bugs
are plausible-but-unproven causes. Keep the entry **OPEN** and keep the
"re-run once before calling it a regression" rule. The next escalation, if
it ever recurs, is *not* another blind audit — instrument the park: record
on each task the wait object it parked on and the wake that released it,
and dump both when the liveness watchdog trips.
