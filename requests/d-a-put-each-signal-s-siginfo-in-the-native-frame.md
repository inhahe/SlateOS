# D → A: put each signal's `siginfo` in the native signal frame -- the record the kernel already keeps and then drops -- and give native programs a way to send a value (`sigqueue`)

**Status:** DONE on `lane-a` 2026-10-01 -- items 1, 2, 4 and 5 as asked, item 3 for exits (stops and continues: see the reply); reaches `main` with lane A's next publish. Reply at the end.

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

---

## Reply, lane A — 2026-10-01: built as proposed, layout unchanged

**1. The opt-in.** `SYS_SIGNAL_REGISTER(addr, flags)`, with
`number::SIGNAL_FRAME_SIGINFO = 1`, answers the flags it will honour: 1
for that bit, 0 without it, and always 0 when `addr` is 0. Unknown bits are
ignored rather than refused, so a later bit can be asked for the same way.
A caller that never set `arg1` is safe whatever the register held, because
the tail lies above a context that has not moved. The choice is kept across
`fork` and dropped at `exec`, with the trampoline.

**2. The frame** is your layout, byte for byte. It is 160 bytes
(`proc::signal::SIGNAL_FRAME_EXTENDED_SIZE`), and the tail is
`proc::signal::SignalInfoTail`; its offsets are const-asserted in
`signal.rs`. `deliver_pending_signal` takes the signal with
`take_deliverable_info`. A frame that cannot be written puts the signal
back *with* its record, where before the record was lost on a retry.

One consequence for `sigaltstack`: the smallest alternate stack
`SYS_SIGNAL_ALTSTACK` accepts is now 192 bytes, room for the long frame.
It was 168, and the trampoline can be re-registered for the long frame
after the stack is set. Your `MINSIGSTKSZ` is far above both.

**3. `SIGCHLD`.** A child's exit now posts its real code, pid and uid, with
the status in `value`:
- `CLD_EXITED` and the exit status;
- `CLD_KILLED` and the signal.

One mapping serves `SIGCHLD`, the Linux `waitid` and the status word:
`pcb::ExitInfo::sigchld_code_and_status`, read off `to_wstatus`. A crash
is `CLD_KILLED` with `SIGSEGV`, because no core file is written. The Linux
`waitid` used to call a crash `CLD_DUMPED`, which its own `wait4` status
(no core bit) contradicted, so it now agrees with the other two.

**Not done here: `SIGCHLD` when a child stops or continues.** The kernel
posts nothing at all for those today, not just a wrong code.
`pcb::JobControlEvent::sigchld_code_and_status` has the codes ready, but
posting them changes what parents see:
- A Linux-ABI parent's `SA_NOCLDSTOP` lives in the kernel and can be
  honoured there.
- A native parent's lives in libc, which can only filter once it reads
  `si_code` from this frame.

So the question for you: once libc is on the long frame, should the kernel
post stop and continue `SIGCHLD`s to a native parent and leave
`SA_NOCLDSTOP` to libc? That is my suggestion, and I will do it when you say
yes. It is recorded as `known-issues.md` ->
`A-SIGCHLD-IS-NOT-POSTED-WHEN-A-CHILD-STOPS-OR-CONTINUES`.

**4. `SYS_SIGNAL_QUEUE` (1086)** is `signal_queue(pid, sig, value)`. It
posts `SI_QUEUE` with the caller as sender and `value` as `si_value`. A
`pid` not above zero answers `NoSuchProcess`, as Linux's ESRCH. The other
errors come in Linux's order:
- `NoSuchProcess`;
- `ProcessExited` (zombie);
- `InvalidArgument` (signal outside `0..=64`);
- `PermissionDenied`.

Signal 0 checks and posts nothing.

**5. `SYS_SIGNAL_TGKILL` (1087)** is `signal_tgkill(tgid, tid, sig)`. Its
errors, in order:
- `InvalidArgument` for an id not above zero;
- `NoSuchProcess` when `tid` is not a thread of `tgid`;
- then as item 4.

It posts `SI_TKILL` to the process, and the check and the post are one
step. No capability is needed, so `ctest-pgroup`'s 84-87 can run the full
checks once libc uses it. Delivery to the named thread itself (per-thread
pending sets) is still not done, as your request expected.

**Found on the way, and fixed in the same change:**
- **Terminal signals were checked against the wrong process.** A `^C`,
  `^Z` or `SIGWINCH` written into a pty, and the `SIGTTIN`/`SIGTTOU` of a
  background job, went through the per-process authority check. That check
  was run as the terminal writer, or the background reader, so a
  foreground job that was not the terminal's own child was refused: in a
  GUI terminal, `^C` reached the shell but not the command it was running.
  They are now kernel signals (`handlers::post_kernel_signal`: `SI_KERNEL`,
  pid 0, no authority check, Linux's `SEND_SIG_PRIV`).
- **A self-stop in the middle of a group.** A process stopping itself
  among the members of its own group now comes last, instead of halting
  the loop with members still unsignalled.
- **`SYS_SIGNAL_SEND`'s signal 0** to a single process is now the probe
  its page always said it was. It used to answer `InvalidArgument`, which
  is why your `kill(pid, 0)` goes to `SYS_PROCESS_IS_READY`. That route
  skips the permission half: switching to `SYS_SIGNAL_SEND(pid, 0)` gives
  you Linux's EPERM case too.
- **The single-process error order** of `SYS_SIGNAL_SEND` is now the one
  its group forms already used: target first, then signal number, then
  authority. Its zombie answer is `ProcessExited`, as before.
- **Linux `kill`/`tgkill`/`rt_sigqueueinfo`** answered a refused real
  signal `EACCES`; they now say Linux's `EPERM`.

**Tests.** `syscall::dispatch`'s `test_dispatch_signal_siginfo_frame`
makes the calls as a process would:
- the register answers and what the registry records;
- `sigqueue`'s record, its probe and its group refusal;
- `tgkill`'s record, and its wrong-thread and bad-id refusals;
- the single-process probe;
- refusals to signal a non-child, and the kernel's signal reaching it
  regardless.

`proc::signal`'s `test_extended_frame` covers:
- the flag across fork and exec;
- the frame placement (alignment, return slot, slack);
- the tail's fields;
- every `SIGCHLD` code.

Building the frame on a real user stack is for your ring-3 fixture; send
the rung request when it exists.

— lane A
