## TD-B-POLL-WITH-A-ZERO-TIMEOUT-IS-NOT-A-YIELD (lane B, 2026-09-10) — swept, one instance, fixed

**In short:** `poll(fds, n, 0)` returns immediately on this kernel **without
parking or requesting a reschedule**. That is correct per POSIX — poll with
timeout 0 must not block — so it is not a kernel defect. It does mean a spin
loop built on it holds its quantum until the timer preempts, and **is not a
wait**. Swept `services/` and `userspace/`: two files use the idiom, one was a
spin, and it is fixed.

**How it was found.** Lane A checked the precondition `services/ctest-pty`
rests on, rather than taking my word that the fixture yielded. It did not. My
earlier `O_NONBLOCK`+poll change fixed the *hang*; the exit-44 race went away
because each iteration now enters the kernel and TCG is slow enough that the
parent always wins. **I was holding a timing property and believed it was an
invariant**, and nothing in the source says otherwise, because "poll with a
zero timeout, then continue" reads as a wait in every language it appears in.

**The sweep.**

| file | verdict |
|---|---|
| `services/ctest-pty/main.c` | three spins, all now `sched_yield()` per iteration |
| `userspace/sshd/src/lib.rs` | **fine** — its `fd_readable` is a readiness *check* inside a loop that blocks; the loop's own comment reads "no polling, no wake-ups" |

**The yield primitive that does work**, verified in the kernel rather than
assumed: `posix::sched_yield` issues `SYS_SLEEP(0)`, and
`kernel/src/syscall/handlers.rs:8439` reads `if duration_ns == 0 { // Zero
sleep -> just yield. sched::yield_now(); }`.

**Why this is worth an entry given one instance.** The idiom is attractive and
wrong in a way that testing does not reveal: a spin that never yields still
*works*, because the timer preempts it eventually. It fails only under a
faster poll path, KVM instead of TCG, or a smaller iteration budget — none of
which looks like a change to the code that contains the bug.
