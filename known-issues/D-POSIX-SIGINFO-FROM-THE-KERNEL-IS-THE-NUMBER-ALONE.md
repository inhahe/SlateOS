## D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE — for a signal the kernel delivers, a native `siginfo_t` says `SI_USER` and names no sender: the kernel's record of the signal does not reach the trampoline (lane D, 2026-09-30) — **Status: OPEN -- both halves are written: lane A's extended frame, `SYS_SIGNAL_QUEUE` and `SYS_SIGNAL_TGKILL` (on `lane-a`) and lane D's reading and sending (2026-10-07). It closes when lane A's kernel is on `main` and `services/ctest-siginfo` passes in a lane D boot.**

**In short:** a program that asks who sent a signal -- in an `SA_SIGINFO`
handler, or from `sigwaitinfo` -- is told "a user process, pid 0" for every
signal that came from outside it: another process's `kill`, a child's exit,
a timer. A `SIGCHLD` handler that reaps the child `si_pid` names, a daemon
that logs who sent it `SIGTERM`, a POSIX-timer handler that finds its timer
through `si_value`, all get nothing to go on. Signals the program raises
itself are described correctly.

**Why:** the kernel keeps the record for every pending signal
(`kernel/src/proc/signal.rs`, `SigInfo`: code, sender pid and uid, value)
and hands it to Linux-ABI programs, but the native path takes the signal
with `take_deliverable`, which drops it, and the native frame
(`SignalContext`) carries the number alone.

**The proper fix:** a native frame that carries the record, opted into at
`SYS_SIGNAL_REGISTER` so libc and kernel need not change together; libc then
reads it into the `siginfo_t` (`siginfo_for`) and the waits' answer.
Requested of lane A, with a proposed layout, in
`requests/d-a-put-each-signal-s-siginfo-in-the-native-frame.md`; also that
`SIGCHLD`'s record carry the exit status, and a native call that posts a
signal with a value, which `sigqueue` -- a stub answering `ENOSYS` -- needs
to send one.

**Where:** `posix/src/signal.rs` (`siginfo_for`, `hand_to_a_waiter`);
`kernel/src/syscall/handlers.rs` (`deliver_pending_signal`).

**Lane A's half** (on `lane-a`): `SYS_SIGNAL_REGISTER(addr, flags)` with
`SIGNAL_FRAME_SIGINFO` (1) builds the extended frame -- the 136-byte context,
then 24 bytes of `si_code`, `si_pid`, `si_uid`, padding and `si_value`, which
for `SIGCHLD` is the exit status or the signal -- and answers the flags it
honours, so an older kernel's 0 says it will not. `SYS_SIGNAL_QUEUE` (1086)
posts a signal with `SI_QUEUE` and a value; `SYS_SIGNAL_TGKILL` (1087) checks
a thread belongs to a process and posts `SI_TKILL`, in one step and with no
File capability.

**Lane D's half, done 2026-10-07** (`posix/src/signal.rs`,
`posix/src/pthread.rs`):

- `init_signals` asks for the extended frame and keeps the answer; the
  dispatch reads the record from the frame only if the kernel said it builds
  one (`Record::from_frame`), so an older kernel's frame is read as before.
- A handler gets the record in its `siginfo_t`, and `sigtimedwait` and
  `sigwaitinfo` get it too: the dispatch claims the waiter (`CLAIMING`),
  writes the record, then the signal, so a waiter that sees the signal sees
  its record.
- `sigqueue` sends through `SYS_SIGNAL_QUEUE`, and `pthread_sigqueue`
  through it to this process. A pid not above zero is `ESRCH`, as Linux
  answers (measured; it was `EINVAL`).
- `tgkill` to any thread but the calling one goes through
  `SYS_SIGNAL_TGKILL`, and `pthread_kill` of a peer through `tgkill`.
- A send to a zombie succeeds, as on Linux (measured): every native send
  answers `ProcessExited` for one, which `kill` had turned into `ESRCH`.

**What is still not there:** a signal this library keeps pending -- one
that came while the kernel's mask lagged the program's, or one a waiter
had to give back -- is sent again as this process's own, and its record
goes with that: `SI_USER` from this process. Both are windows of a few
instructions. Delivery to the named thread itself (per-thread pending sets)
is lane A's larger, later step; until then `tgkill` and `pthread_kill` reach
the process, and whichever of its threads returns to userspace next runs the
handler.

`services/ctest-siginfo` checks it against Linux's answers, and passes on
Linux as an ordinary user.
