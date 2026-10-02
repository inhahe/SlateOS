### [A] A-SOCKET-LOCKS-WERE-SPINLOCKS-HELD-ACROSS-DAEMON-ROUND-TRIPS: every socket call that reached the network daemon blocked holding a spinlock -- 2026-09-27
**Status:** FIXED on lane-a 2026-09-27, awaiting a boot. Found tracing the
scheduler's one-shot `voluntary context switch (task 0, cpu 0) while holding
1 tracked spinlock(s)` warning. It appears in all eight retained boot logs
since 2026-09-19 that reached the socket server self-test, each time straight
after the `persistent netstack listen/accept` line.

**In short:** each network socket has a lock, so two threads cannot use it at
once. It was the kind of lock that stops the processor from switching to
other work while it is held. But a socket call holds it while waiting for the
network daemon's reply, and waiting means switching to other work. So the
other work ran on that processor unable to be interrupted, and if the waiting
thread came back on a different processor, the first one stayed that way for
the rest of the boot.

**Where.** `kernel/src/net/socket.rs`:
- each socket's state, `Arc<Mutex<SocketInner>>`;
- a listener's shared session, `Arc<Mutex<NetstackConn>>`.

Both were `crate::sync::Mutex`, a spinlock that raises its CPU's preempt
count. Every daemon operation is taken under them (`connect`, `listen`,
`accept`, `send`, `recv`, `poll_ready`, `shutdown`, `local`, and the UDP
calls), and every one reaches `NetstackConn::submit_round`, which waits in
`channel::recv_timeout`. The module's own "Lock discipline" note said so. It
kept the socket *table* lock out of the round-trip, and put the per-socket
lock in it.

**Why it went unfixed.** The warning was one-shot and gave no site, and
nothing counted it, so a boot passed with it. Being one-shot, it could also
not say whether anything else did the same.

**The fix.**
- Both locks are `sched::kmutex::KMutex`: a sleeping mutex, which leaves the
  preempt count alone, and whose contenders sleep instead of spinning.
  `SOCKET_TABLE` stays a spinlock, held only for a lookup or an insert.
- `KMutex` had never been tested under contention. Its self-test now
  proves it, with a second task:
  - taking it leaves the preempt count unchanged;
  - a contender parks `Blocked`, registered as a waiter, without the mutex;
  - unlocking wakes the contender and hands it the mutex.
- The scheduler's check counts every occurrence, instead of warning once.
  Each distinct site is described once, with:
  - the held lock's acquisition site, from lockdep (or the `PreemptSpinMutex`
    leaf record);
  - the held-lock stack;
  - a symbolized backtrace.
- Before BOOT_OK, `sched::report_switches_under_lock` gives the total
  against the yields and blocks counted, and **fails the boot** if it is not
  zero. A control runs immediately before the verdict: the predicate on
  synthetic counts, and the counting path on a real raised count. A zero is
  therefore a count the check was shown able to make.

**What the next boot says.** If the socket locks were the only offender, the
verdict line reads none. Any other site now names itself, and fails the run
until fixed.
