## B-MESG-AND-WRITE-UNTIL-PORTED — `mesg`, `write` and `talk` were deleted for fabricating; `mesg` and `write` are POSIX utilities to port (lane B, 2026-10-01) — **Status: DEFERRED (needs a design: `deferred-questions.md` DQ5)**

**In short:** `userspace/mesg` answered as `mesg`, `write` and `talk`, and the
three agreed with one another through files no other program reads, rather
than through the terminals they are about. `mesg n` wrote
`/var/run/mesg/<user>` instead of clearing the terminal's group-write bit;
`write` "sent" by appending to `/var/run/messages/<user>`, never opening the
recipient's terminal, and exited 0; `talk` printed `[Connecting to
bob...]` and `[Connection from alice]` -- naming the *sender* as the one
connecting -- and echoed the sender's own typing back. It also read
`/var/run/utmp` as text lines, which it is not. Deleted 2026-10-01 under
§1006, in the §1045 triage.

**The proper fix:** ports of util-linux 2.39.3's `term-utils/mesg.c` (the
terminal's `S_IWGRP` bit through `fchmod`, as POSIX specifies `mesg`) and
`term-utils/write.c` (the recipient found through `utmp` with
`libcall::utmp`, its terminal checked writable by group, the message
written to it with control characters made visible), each with a WSL
differential harness. `talk` needs a `talkd` on the other end and is not
coming back with them.

**Why it is deferred, found the same day:** those ports cannot work as
written on SlateOS. A terminal there is reached only through a handle held
by its owner, never by opening `/dev/pts/N` (the kernel's `fchmod` refuses a
terminal descriptor outright, and `SYS_PTY_SLAVE_ID` documents that the name
"grants nothing"). `write` opening someone else's terminal is exactly the
ambient authority the design removes, so the tools need a service that holds
the terminals and delivers with consent first -- a design question, recorded
as `deferred-questions.md` DQ5 with its trigger.

**Where:** new crates `userspace/mesg` and `userspace/write`; the deleted
crate is in history at `userspace/mesg`.
