## 1013. `su` drops uid and gid now, and documents the half it cannot drop

**Date:** 2026-09-10
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `su alice` asked for alice's password, checked it, and then ran
alice's shell as whoever typed the command. It set `HOME`, `USER` and `SHELL`
to alice and changed nothing the operating system checks. It now really becomes
alice. One piece is still missing -- clearing the *extra* groups the previous
user was in -- because the call that does that does not exist yet, and the
decision recorded here is to ship the two-thirds that works rather than wait
for the third.

**How it had been blocked.** `userspace/doas` carries this note: "the real
privilege change will use the kernel's capability system once the POSIX exec
layer supports `setuid`/`setgid` syscalls". That was true when written.
`posix::setuid` and `posix::setgid` now apply real credentials through
`set_real_credentials`, and `getuid()` reflects them.

**Why nobody noticed the premise expire.** The functions did not appear; they
*changed behaviour*. `posix::setuid`'s own doc records it: "pre-Phase-192 we
returned `0` for *every* uid value, ignoring caps ... exactly the kind of
silent 'permission boundary skipped' bug containers care about." So for a while
`setuid` existed, was callable, and did nothing while reporting success. **A
stub that reports success is what keeps a deferral looking current.** There was
no compile error to trip over and no failing call to investigate; the
capability arrived underneath a comment that still said it had not. This is the
`todo.txt` S305 standing rule -- re-check the premise of anything deferred for
a missing capability -- and it is the third time this week it has paid.

**The decision: drop what can be dropped, or drop nothing?**

*Drop nothing until `setgroups` exists.* The status quo, and it has a real
argument: a partial privilege drop is the classic vulnerability shape, and code
that looks like it drops privilege but does so incompletely is more dangerous
than code that visibly does not, because the next reader stops looking.

*Drop uid and gid, and write down what is missing.* Chosen. Three reasons.
First, the missing piece leaks **nothing today**: `posix::getgroups` reports
zero supplementary groups, so there are none to retain -- the gap is real but
currently empty. Second, the alternative is not "safe", it is "wrong in the
other direction": every file the shell created was owned by the caller, and
every permission check it passed was the caller's. Third, waiting has no end
condition anyone is watching, which is precisely how the original deferral
lasted this long.

*Why not `CommandExt::groups`?* It is not a matter of taste.
`posix::setgroups` returns `ENOSYS` on purpose, and `std` calls it in the child
between fork and exec -- so asking for it would abort the child before exec.
The choice is not "with or without groups", it is "uid and gid, or nothing at
all".

**What makes the partial drop safe to leave.** Not a comment -- an entry.
`known-issues.md` gains
TD-B-USER-SWITCHING-PROGRAMS-CANNOT-RESET-SUPPLEMENTARY-GROUPS, which says what
the fix is, what order the calls must go in (groups, then gid, then uid --
each step drops the privilege the previous one needed), and names the filed
request the kernel side is waiting on. The thing that made the *last* deferral
invisible was that its trigger was a sentence rather than a tracked item.

**A second defect the same look found, fixed here.** The uid came from
`record.uid().unwrap_or(u32::MAX)`. That was harmless while the number only
chose a `PATH` string -- and it stops being harmless the instant it is handed
to `setuid`, where a record with no `uid:` would start a shell owned by uid
4294967295, an identity belonging to nobody. It is now `Option`, and a record
that cannot name its owner gets no session. **A sentinel is safe exactly as
long as nothing acts on it**, and adding the first thing that acts on it is not
an occasion the sentinel announces.

**Not done here:** `doas`, `sudo`, `sshd` and `login` have the same hole.
Logged as TD-B-FOUR-MORE-PROGRAMS-RUN-A-SHELL-AS-THE-WRONG-USER. `su` first
because it is the smallest complete case and proves the mechanism; the rest are
a conversion rather than a design, and `login` needs its exec built first.

**Correction, same day: `sshd` did not have the hole.** It has been calling
`cmd.gid(user.gid)` then `cmd.uid(user.uid)` since it was written, and its doc
comments give the reasoning this entry gives, including the gid-before-uid
order. The claim above came from a survey `grep` whose `grep -v` filter --
written to drop *reads* of a record's fields (`user.uid()`) so they would not
drown the signal -- also dropped `cmd.uid(user.uid)`, which is the evidence.
The program printed "NO uid/gid drop found" and that was recorded as a fact
about the program. It is the same defect as §1011's classifier and §1014's
dead check: **a command that answered a narrower question than the one being
asked, whose answer was reported at the width of the question.** Three
programs, not four. Details in `known-issues.md` under that entry.
