## 1503. Who may change whose nice: whoever may signal it, and a raise only within its `RLIMIT_NICE` or with the right to raise priority

**Date:** 2026-10-01 · **Decided by:** Claude (autonomous) · **Lane:** A

**In short:** "nice" is how politely a program shares the processor, from
-20 (greediest) to 19. Here it is real scheduling: nice -20 is the top
priority, above every system service. Until now:
- any program could set its own nice to -20 by calling the kernel directly,
  because the check was in the C library;
- a Linux program could change any other program's nice;
- asking about a group of programs, or a user's programs, changed the
  caller's own.

Now the kernel decides, in one place (`proc::priority`), for every way of
asking:
- A program may change the nice of the programs it may stop with a signal:
  itself, its children, and any it holds the right to end.
- Making one *greedier* than it is takes room in that program's
  `RLIMIT_NICE` (a per-process ceiling, 0 by default, so no room), or the
  right to raise priority. That right is a Thread capability with
  IO_REALTIME, the kernel's form of Linux's `CAP_SYS_NICE`.

`requests/e-ad-renicing-another-process-renices-the-caller.md` asked for
this, and said plainly that the rule is a design choice.

**The rule** (`priority::may_set_nice`, used by native 532, 1088 and 1089
and by Linux `setpriority`, `getpriority` and `sched_setattr`):

1. **Authority over the target.** The target is the caller, or the caller's
   child; or the caller is a kernel task; or it holds a Process capability
   with DELETE rights for the target. Else `PermissionDenied` (`EPERM`).
2. **A raise** (below the target's *current* nice) needs the target's
   `RLIMIT_NICE` soft limit to be at least `20 - nice`, as Linux's
   `can_nice`, or the caller to hold `(Thread, IO_REALTIME)`. Else
   `ResourceExhausted` (`EACCES`).
3. **Reading** needs nothing, as on Linux.

**Alternatives for (1):**

| | What changes | For | Against |
|---|---|---|---|
| **A. Linux's rule: same user** | any process may renice another of its own uid | what ported programs expect; `renice` of one's own editor works | uid is not authority in this system: no other operation grants it on uid alone, and it would be the first ambient authority |
| **B. Whoever may signal it (chosen)** | a parent may renice its children; a task manager holding the right to end a process may renice it | one rule for "act on another process", already checked by `SYS_SIGNAL_SEND`; lowering a priority is strictly less than ending | `renice` of an unrelated process of one's own user fails where Linux allows it -- as `kill` already does here |
| **C. A new Process right, SCHEDULE** | priority authority granted separately from ending | finest grain | a new right every granter must learn, for a power below one they already grant |

**Alternatives for (2):** checking a raise against 0 rather than the
target's current nice, as the Linux shim did, let a program at nice 10 climb
back to 0 without room. Checking the *caller's* `RLIMIT_NICE` rather than
the target's is not Linux's rule: `can_nice` reads the target's.

**Error codes.** The two refusals need different native codes so that libc
can give Linux's `EPERM` for one and `EACCES` for the other.
`ResourceExhausted` ("resource limit reached") is the closest existing code
for the second, since what refuses a raise is the `RLIMIT_NICE` ceiling.

**Consequences:**
- `cap::rights_without_root` (§1502) now takes `IO_REALTIME` from `Thread`
  capabilities too. A process that drops root loses the right to raise.
- `sched_setattr` checks authority for any change to another process and
  this rule for its nice, answering `EPERM` for either, as Linux's
  `req_priv`.
- The policy and RT-priority stores of `sched_setscheduler`/`sched_setparam`
  are bookkeeping that the scheduler never reads, and are not gated yet.

**Revisit** if the operator wants uid to grant authority (A), or if
priority should be grantable apart from ending (C). Under B, either is a
change to `priority::may_act_on` alone.
