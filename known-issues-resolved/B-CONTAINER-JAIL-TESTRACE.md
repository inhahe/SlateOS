### B-CONTAINER-JAIL-TESTRACE. `container` self-tests 18/19 (rootfs jail + volume mounts) flaked non-deterministically: spawned a real init process, then inspected its per-PID namespace state, which the process cleared by exiting mid-test — FIXED 2026-06-30

**Where:** `kernel/src/container.rs`, self-tests "Rootfs jail (chroot) for
init process" (Test 18) and "Volume (bind) mounts for init process"
(Test 19). Both originally did `let pid = run(ct, HELLO_ELF, &opts)` to
spawn a *real, schedulable* init process, then called
`namespace::resolve_path_for(pid, …)` several times to assert the chroot/
volume wiring. The race: `HELLO_ELF` prints one line and **exits
immediately**; on another CPU it could run and exit *between* two of the
test's resolves. Thread teardown on exit calls `namespace::detach(pid)`,
which drops `PROCESS_ROOT[pid]`/`PROCESS_MOUNTS[pid]`, so a later
`resolve_path_for(pid, …)` returned the **unjailed input verbatim** and
the `assert_eq!` panicked → hard-halted the boot. Observed as Test 18's
`..`-escape assert failing on a heavy boot while an identical-binary
re-run passed (load-dependent flake). Production code is correct: a live
process resolves its *own* paths inside its own syscall handler, so the
jail always exists for the duration; only a third-party test reading
another process's namespace after it may have exited hits this.

**Fix:** Tests 18/19 no longer spawn a schedulable process. They register
a *synthetic, never-scheduled* PID through `add_process(ct, FAKE_PID)` —
the exact same container-layer wiring path `run()` uses
(`add_process_task` → `set_root`/`add_volume`) — and then run the
resolution asserts deterministically (the PID has no thread, so it cannot
exit and clear its state). The concerns that genuinely need a live
process are still covered without the race: the end-to-end
`run()`→cgroup-billing path by the "Run init process + cgroup billing"
test (Test 17), and the resolution *semantics* (`..` clamp, longest-
prefix volume match) by `namespace::test_process_root` /
`test_volume_mounts` (which already use synthetic PIDs 88888/88889). The
`state != Created` config-rejection guard is now exercised via `stop()`
rather than a live process, so it too is deterministic. Verified: clean
build + green boot self-test ("Self-test PASSED (19 tests)").

**Update (2026-06-30) — latent flake OBSERVED as a boot hang, now FIXED:**
The Test 17 liveness risk noted above stopped being theoretical. On a
heavy boot run the serial log froze mid-test right after the `run()` log
line (`[container] run id=8 'test-run-ct': init pid=219 …`) and never
reached `BOOT_OK` (480s timeout → boot gate FAILED). An identical-binary
re-run passed (`BOOT_OK after 187s`), confirming a load-dependent race,
not a logic bug — a timer ISR preempted the boot self-test thread into
the freshly-spawned init task, which executed `hello`; the exiting
thread's teardown then raced the test's explicit teardown, deadlocking
(a hang, not an assert panic — no `[PANIC]` was printed). This was worse
than the predicted assertion flake because a hang fails the *entire* boot
gate. **Fix:** Test 17's spawn→teardown window is now bracketed in
`cpu::without_interrupts(...)`, so the init task is still *registered*
(cgroup billing is verified end-to-end exactly as before) but can never
be *scheduled* before `destroy()` removes it — deterministic, with no
loss of real-`run()` coverage. Verified: clean build + green boot
self-test. Production code is unaffected (a live process only ever
resolves its *own* state inside its own syscall handler).
