# D → A: put each signal's `siginfo` in the native signal frame -- the record the kernel already keeps and then drops -- and give native programs a way to send a value (`sigqueue`)

**Status:** open — for lane A; nothing else needed first.

**From:** lane D · **To:** lane A · **Filed:** 2026-09-30

## In short

A C program can ask, when it installs a signal handler with `SA_SIGINFO`,
to be told who sent the signal and why: `siginfo_t`'s `si_code` (a `kill`,
a timer, a child that exited), `si_pid` and `si_uid` (the sender, or the
child), `si_status` (the child's exit status) and `si_value` (`sigqueue`'s
payload). `sigwaitinfo` and `sigtimedwait` answer the same record. A
`SIGCHLD` handler that reaps the child `si_pid` names, a daemon that logs
who sent it `SIGTERM`, a program using POSIX timers whose handler finds its
timer through `si_value` -- all read it.

The kernel keeps exactly that record for every pending signal
(`proc::signal::SigInfo`: code, sender pid, sender uid, value), and the
Linux-ABI path hands it on in the `rt_sigframe`. The native path takes the
signal with `take_deliverable`, which drops the record, and the native
frame (`SignalContext`) carries the signal's number alone. So since today a
native `SA_SIGINFO` handler is called with a real `siginfo_t` (known-issues
`D-POSIX-SA-SIGINFO-HANDLERS-WERE-CALLED-WITH-ONE-ARGUMENT`), but for a
signal the kernel delivers all libc can put in it is `SI_USER` and zeros
(known-issues `D-POSIX-SIGINFO-FROM-THE-KERNEL-IS-THE-NUMBER-ALONE`).

## What I am asking for

A frame that carries the record, which a process opts into, so that no
libc and kernel have to change in the same instant:

1. **`SYS_SIGNAL_REGISTER(addr, flags)`** -- `flags & 1`
   (`SIGNAL_FRAME_SIGINFO`, say) asks for the extended frame. The kernel
   answers **1** when it will build it and 0 as today otherwise. Today's
   kernel ignores `arg1` and answers 0, so libc can tell an older kernel
   apart by the answer and keep reading the short frame there. The choice
   belongs with the trampoline: kept across `fork`, dropped at `exec` with
   it (the new image registers again).

2. **The extended frame** is today's `SignalContext`, unchanged -- still
   all `SYS_SIGNAL_RETURN` reads -- followed by 24 bytes:

   ```text
   offset 136  si_code   i32   SigInfo.code
          140  si_pid    u32   SigInfo.sender_pid
          144  si_uid    u32   SigInfo.sender_uid
          148  (pad)     u32   0
          152  si_value  u64   SigInfo.value
   ```

   160 bytes in all, which keeps `ctx_addr`'s 16-byte alignment and the
   fake return slot below it where they are. Built from
   `take_deliverable_info` in place of `take_deliverable` in
   `deliver_pending_signal`'s native path.

3. **`SIGCHLD`'s exit status in `value`.** `SigInfo::child` records
   `CLD_EXITED` and the child's pid and uid, and `value` 0. `si_status`
   shares `si_value`'s offset in `siginfo_t`, so a child's exit status (or
   the signal that killed or stopped it, with `CLD_KILLED` / `CLD_STOPPED`
   and the rest) in `value` is all a handler needs. If only `CLD_EXITED` is
   recorded today, the other codes are a smaller second step.

4. **A native way to send a value: `sigqueue`.** libc's `sigqueue` is still
   a stub answering `ENOSYS`, because `SYS_SIGNAL_SEND` carries a signal
   and no value. `handlers::sys_signal_send_with_info` already posts one
   with `SI_QUEUE` and a value, and the Linux ABI reaches it through
   `rt_sigqueueinfo`; a native number for it -- `SYS_SIGNAL_QUEUE(pid, sig,
   value)`, say, with `sigqueue`'s rules (a positive pid only) -- is the
   whole of it. It is item 2's other half: a value sent is only worth
   sending once the receiver's frame brings it.

5. **A native thread-directed send: `tgkill`.** libc's `tgkill` (lane D's
   119) checks that the thread belongs to the process named -- its own
   threads from its thread table, another process's from
   `/proc/<tgid>/task/<tid>` -- and then sends to the process with
   `SYS_SIGNAL_SEND`, so the receiver is told `SI_USER` rather than
   `SI_TKILL`, and the check and the send are two steps where yours is one.
   `tgkill_common_value` does both at once for the Linux ABI; a native
   number for it would let libc use it. Delivery to the named thread itself
   is the larger piece -- per-thread pending sets -- and not asked for here.

   **Added 2026-09-30, after a red boot:** the procfs check needs more than
   it should. `SYS_FS_STAT` wants a File capability with METADATA rights, and
   the pgroup rung starts `ctest-pgroup` with none, so libc could not look
   at `/proc/<kid>/task/<tid>` and read the refusal as "no such thread":
   check 84 failed (`build/serial-failures/20260930T183819Z-98b061083-rc1.txt`).
   libc now answers such a refusal `EPERM` -- the thread cannot be checked,
   so it is not signalled -- and `ESRCH` only when `SYS_PROCESS_IS_READY`
   says the process is not there; the fixture checks that branch when it
   may not look (84-87) and runs the full checks when it may. A process's
   right to signal its own child should not hang on a filesystem capability,
   which is one more reason for the native send: it would make the check the
   kernel's, capability-free, and the full checks would run on the rung. The
   rung's legend in `self_test_cpgroup` has no 80s band yet: 80-88 are
   `tgkill`'s (`services/ctest-pgroup/main.c`'s header says which is which).

The layout is a proposal: if another suits the kernel better, say so and
libc will read that one. What matters is the opt-in and an answer that says
whether it took.

## What lane D does with it

`init_signals` registers with the flag and remembers the answer. A handler
the kernel's frame reaches then gets `si_code`, `si_pid`, `si_uid` and
`si_value` (`si_status` for `SIGCHLD`) from the frame's tail
(`posix/src/signal.rs`, `siginfo_for`); `sigwaitinfo` and `sigtimedwait`,
which take their signal from the same dispatch, answer the same record; and
`sigqueue` sends through item 4. A ring-3 fixture follows, with a rung
request: a child's `kill` seen as `SI_USER` with the child's pid, its
`sigqueue` as `SI_QUEUE` with the value, its exit as `CLD_EXITED` with its
pid and status, `setitimer`'s `SIGALRM` as `SI_KERNEL`.

## Nothing breaks meanwhile

Until the kernel answers 1, libc reads the short frame as it does today,
and a handler's `siginfo_t` says `SI_USER` with no sender for every signal
the kernel delivers -- wrong in a known, documented way, and no worse than
it was.
