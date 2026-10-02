### B-PTHREAD-CHILD-JUMPS-TO-GARBAGE. One `pthread_create`d thread intermittently starts at a bogus RIP and is killed; the process keeps running and reports a wrong answer — FIXED (defect 2 fixed 2026-08-13 `315a7e0ca`; defect 1 fixed 2026-08-13 `975114f54`, corroborated by a 20/20 clean soak) 2026-08-13

**Symptom.** A deliberate 40-boot soak (`scripts/wedge-soak.sh`, run
`soak-20260813-093459`) was launched to hunt an unrelated wedge. It did not
find the wedge; it found this instead, on iteration 10 of 10 completed
(iterations 1–9 clean, so the measured rate is **1 failure in 10 boots**).
The Path-Z real-glibc pthread self-test produced:

```
captured: SLATE_GLIBC_PTHREAD_OK counter=30000 joinsum=9
expected: SLATE_GLIBC_PTHREAD_OK counter=40000 joinsum=10
```

**What those numbers mean.** The test binary (built by
`scripts/create-ext4-rootfs.sh`, the `pthread.c` heredoc) creates 4 threads;
worker `i` does 10 000 mutex-guarded `counter += 1` and returns `i + 1`, so a
correct run is always `counter=40000 joinsum=10`. `40000 - 30000 = 10000` and
`10 - 9 = 1` identify the casualty exactly: **the thread with `id == 0`** —
the first one created — contributed neither its increments nor its return
value. Nothing else diverged.

**The kill.** From the serial log
(`build/hang-catches/soak-20260813-093459-iter10.serial.txt`, lines ~19256–19280;
`build/` is gitignored, so the excerpt is reproduced here):

```
[sched] Spawned task 266 (priority 16, cpu 0)      <- worker id 0
[mmap] Lazy mapped 0x6000a1a000..0x600121e000 (513 frames, demand-paged)
[sched] Spawned task 267 (priority 16, cpu 0)
...
[sched] Task 267 exiting
[sched] Task 268 exiting
[sched] Task 269 exiting
[exception] User page fault (task 266) at 0x600005eff0, addr=0x600005eff0 (not-present, read) — trying SEH
[exception] Killing task 266 — Page Fault (#PF) at 0x600005eff0 (ring 3)
  CS=0x23 RFLAGS=0x10216 RSP=0x6000a15788 SS=0x1b
[exception] Recording crash: pid=296 exception=8 rip=0x600005eff0 aux=0x600005eff0
```

**`rip == aux == CR2`** — the faulting address *is* the instruction pointer.
Task 266 did not deref a bad pointer; it **jumped to** one. And 0x600005eff0
is not in either loaded image (the binary is at bias 0x57ffc1e4c000, the
loader/libc at 0x72c7a9914000) — it is an address in the low mmap arena,
*below* every region this process lazily mapped (the lowest logged is
0x6000212000). Its `RSP=0x6000a15788` is correctly inside its own thread
stack (0x6000216000..0x6000a1a000), so the stack pointer survived; only the
control transfer went wrong.

**Reading of the mechanism (unconfirmed).** glibc's `start_thread` reads
`pd->start_routine` out of the thread descriptor via `%fs`. A garbage value
there — because `CLONE_SETTLS` installed the wrong `%fs` base for this child,
or because the child was made runnable before the parent's descriptor stores
were visible to it — produces exactly this: a jump to an arena-looking
address with an otherwise intact stack. That the victim is always(?) the
*first* child, while children 2–4 ran and exited normally, points at a
first-time-through / setup-ordering window rather than steady-state
contention. Confirming this needs the child's `%fs` base and the descriptor
contents logged at `clone` time — see below.

**Two separate defects are visible here, and the second one is arguably worse:**

1. The thread jumped to garbage (above). **Still OPEN** — see "Next step"
   below.
2. **The process did not die, and reported a plausible-looking wrong answer.**
   `pthread_join` on the killed thread returned success with `ret == NULL`,
   which is how `joinsum` became 9 instead of 10. On Linux a `SIGSEGV` in any
   thread terminates the whole process; here only the thread was killed, the
   remaining threads finished, and `main` printed a result that looks like a
   normal run. A test that asserted only "the binary exited 13" would have
   passed. Whatever the fix for (1), the kill path must not let a
   fault-killed thread be joined as if it had returned normally — a
   `pthread_join` on a thread the kernel killed should be distinguishable, and
   a ring-3 fault with no SEH handler should take down the process, not one
   thread of it.
   **FIXED 2026-08-13.** Three changes, all of which had to land together:

   - `kernel/src/idt.rs::kill_userspace_task_with_info` now calls
     `proc::thread::kill_process_threads(pid)` instead of
     `on_thread_exit(task_id)`: an unhandled ring-3 fault — one that both
     the Linux-ABI signal path (`try_deliver_linux_fault_signal`) and the
     native SEH trampoline (`try_dispatch_user_exception`) declined —
     takes down the **whole process**, which is the default disposition
     under both Windows SEH and Linux `SIGSEGV`. `kill_process_threads`
     subsumes the old `on_thread_exit` for the faulting task itself
     (`sched::kill_task` refuses the *current* task, which is
     `task_exit`'s job, but the thread→process mapping is still dropped).
   - `kernel/src/proc/thread.rs` replaces `THREAD_EXIT_VALUES:
     BTreeMap<TaskId, i64>` with `THREAD_OUTCOMES: BTreeMap<TaskId,
     ThreadOutcome>`, where `ThreadOutcome` is `Exited(i64) | Killed`.
     `join()` used to report `Ok(0)` for *any* thread that ended without
     recording a value — which is exactly what a killed thread looks like
     — so a dead worker's contribution silently vanished into a zero.
     Every involuntary death path now calls `record_killed()` **before**
     `on_thread_exit` (which is what releases a parked joiner, so a marker
     written afterwards can arrive too late), and `join()` reports
     `KernelError::Cancelled`. Reaching the "no outcome at all" case now
     means the caller joined a *detached* thread, which is `EINVAL`, not a
     silent zero. New `proc::thread` self-test 8
     (`test_killed_thread_does_not_join_normally`) locks this in.
   - `test_blocking_join`'s second phase had to change with the semantics,
     and the way it failed is worth recording: it models a thread that dies
     *without* passing through `thread_exit_with_value` (a crash), and it
     asserted the old `Ok(0)`. The first boot after the fix duly printed
     `FAIL: join() returned -9223372036854775808` — `i64::MIN` being the
     joiner's `unwrap_or` sentinel for "join returned `Err`". The phase now
     stamps `record_killed()` on the target, which is what the real crash
     path does, and expects `Cancelled`. The joiner also had to stop
     folding the error into the value (`join(t).unwrap_or(i64::MIN)`) and
     publish the value and the error discriminant in separate atomics —
     the same value/error ambiguity that forced the `SYS_THREAD_JOIN` ABI
     change, reproduced in miniature in the test harness.
   - `SYS_THREAD_JOIN` (512) changed shape: the exit value now travels
     through an `arg1` out-pointer and the syscall returns `0`/`-errno`.
     The old value-in-rax ABI **could not represent the fix**: a thread may
     exit with a legitimately negative value — `pthread_exit(PTHREAD_CANCELED)`
     is `(void *)-1`, and `Cancelled` is `-5` — so an exit value and an
     error code were indistinguishable. `posix`'s `pthread_join` passes a
     stack slot, and maps `Cancelled` to a *successful* join returning
     `PTHREAD_CANCELED`, which is precisely the slot POSIX reserves for
     "this thread did not finish normally".

   Also fixed in passing: kshell's `kill` command called `sched::kill_task`
   directly, which only marks the scheduler task Dead — leaving the
   thread→process mapping registered, IRQ registrations dangling and any
   joiner parked forever. It now goes through the new
   `proc::thread::kill_thread()`, which records the kill, kills the task
   and runs the universal death hook.

   Note the failing fixture above reaches `pthread_join` through *glibc's*
   futex-based join over the Linux ABI, not through `SYS_THREAD_JOIN`, so
   for that test it is the `idt.rs` half that makes the difference: the
   process now dies instead of printing `joinsum=9`.

**Distinct from the neighbouring pthread entries.** B-PTHREAD-TEARDOWN-PF
(below) is a *kernel*-mode `#PF` at a near-null address during *teardown*;
this is a *ring-3* fault at *startup*, and the kernel itself stays healthy.
B-PTHREAD-YIELDBUDGET (resolved) was a silent hang, not a fault.

**Reproduce.** `bash scripts/wedge-soak.sh` (or plain repeated
`scripts/boot-test.sh`) — expect roughly one failure per ten boots. The
soak script already treats a self-test regression as a catch and preserves
the serial log, which is how this was captured.

**Next step when picked up (defect 1).** ~~Add a `clone`-time trace to
`kernel/src/proc/thread_clone.rs` printing, for each child: the requested TLS
base, the `%fs` base actually installed, and the first 8 bytes at
`tls_base + offsetof(struct pthread, start_routine)`; then soak until it trips.
The failure is frequent enough (1/10) that a single 20-boot soak should catch
it with the trace attached.~~ **Superseded — the defect was found by static
audit instead, see below.**

**Defect 1 FIXED 2026-08-13** (`975114f54`, *seed thread `%fs`/`%gs` base
before admission*). The planned trace was never needed: auditing the spawn
path for the register-after-admit pattern found the mechanism directly.

`clone_thread` called `thread::spawn_with_tls`, which **admitted the child to
the run queue before writing its `%fs`/`%gs` base**. On our uniprocessor
(TCG) build a timer preemption inside that window lets the child start with an
unseeded `%fs`. glibc's clone entry stub loads the thread function from
TLS — a `%fs`-relative fetch of `struct pthread`'s `start_routine` — so with a
stale/zero `%fs` base it reads a garbage word and jumps to it. That is exactly
the reported signature: worker `id == 0` (the first child created, i.e. the one
most likely to be preempted before seeding) starting at a bogus RIP with
`rip == aux == CR2`, the fault address *being* the instruction pointer.

Fixed structurally rather than by reordering two statements: `thread` now
exposes a two-phase API — `spawn_suspended_with_tls()` (create + register
everything, including the TLS bases) followed by an explicit `admit()` — so a
child cannot become runnable before its per-thread state exists.
`spawn_with_tls` is retained as a thin wrapper that calls both.

**Confirmation and its honest limits.** The 20-boot soak this entry asked for
has since run (`build/hang-catches/soak-ctidfix.log`, 2026-08-13 23:02 →
2026-08-14 01:52): **20/20 boots passed**, every one reporting
`REAL glibc pthread (… 40000 mutex/futex ops, pthread_join, captured 48 bytes
== expected): OK` — i.e. the exact `counter=40000 joinsum=10` assertion whose
failure defined this bug — and zero kernel faults.

That is consistent with a fix but is **not** statistically conclusive on its
own: at the measured 1-in-10 rate, 20 clean boots would happen by chance
`0.9^20 ≈ 12%` of the time. The confidence comes primarily from the mechanism
being understood and closed by construction, with the soak as corroboration.
If a `counter=30000 joinsum=9` (or the now-loud faulting variant) ever
reappears, reopen this entry rather than assuming a new bug.

**Bug class.** Third of three register-after-admit defects found in this
subsystem, alongside B-PTHREAD-JOIN-LOST-CTID (the ctid registration) and
B-THREAD-JOIN-EXIT-RACE (the join-waiter registration). Worth grepping for
whenever new per-thread state is keyed on a task id.

**How the defect-2 fix changes what a soak looks like.** Before, the 1-in-10
boot that hit this produced a *quiet wrong answer* (`counter=30000
joinsum=9`) that only the self-test's exact-value assertion caught. Now the
process dies on the fault, so the same race surfaces as a loud failure. That
is the intended consequence, not a regression — and it is exactly the signal
the trace above needs.
