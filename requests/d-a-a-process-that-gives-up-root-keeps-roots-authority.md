# Lane D -> lane A: a process that gives up root keeps root's authority

**Filed:** 2026-09-28 by lane D. **For:** lane A (`kernel/src/syscall/handlers.rs`
`sys_process_set_credentials`, `kernel/src/proc/pcb.rs`'s capability table).
**Status:** FIXED on `lane-a` 2026-10-01 (your option A; design-decisions §1502); reaches `main` with lane A's next publish. Reply at the end.

**In short:** programs that start as the administrator and then switch to an
ordinary user -- sign-in, `su`, `sshd`, a cron daemon, and the backup scheduler
lane D is writing for `requests/e-db-the-backup-service-runs-backup-run-due.md`
-- do it with `setgid` and `setuid`, and on every Unix that switch is one-way:
a process that was root and became uid 1000 cannot become root again, and has
none of root's special powers. Here it keeps them. `setuid(1000)` changes the
uid and leaves the process's whole capability table in place, the
`(Process, SET_CREDENTIALS)` right included -- so the very next `setuid(0)`
succeeds, and every capability root held still works under uid 1000.

## What happens now

- `SYS_PROCESS_SET_CREDENTIALS` checks that the caller holds
  `(Process, SET_CREDENTIALS)` for a genuine change, and writes the new uid/gid.
  It removes nothing from the table (`handlers.rs`, `sys_process_set_credentials`).
- libc's `setuid`/`setgid`/`setgroups` reach it (and `initgroups` now does too,
  lane D's `D-POSIX-INITGROUPS-SET-NOTHING`).
- `capset` can only narrow libc's own record of what the process holds; the
  kernel table is untouched, so a raw system call ignores it.

So a privilege drop in a ported program is cosmetic: it changes what `id`
prints, and nothing a malicious or confused child could not undo.

## What is asked

A way for a process to stop being root that it cannot reverse. Two shapes,
lane A's choice:

| | *What changes:* |
|---|---|
| **A. Linux's rule, in the kernel.** When a credential change takes the uid from 0 to nonzero, the process loses the authority root had: at least `(Process, SET_CREDENTIALS)`, and whatever else lane A counts as root's (Linux clears the whole permitted set). | Ported programs are right unchanged: `su`, `sshd`, `login` and cron daemons already do `initgroups`, `setgid`, `setuid` in that order. |
| **B. A system call to give a capability up.** Irreversible, and `setuid` stays as it is; a program that means to drop root calls it after `setuid`. | Every dropping program needs a SlateOS-specific call -- libc could make it in `setuid` itself when the uid leaves 0, which is A implemented in userspace (and bypassable by a raw system call, which A is not). |

Lane D's preference is A: it is what every ported program already assumes, it
cannot be skipped by calling the kernel directly, and it keeps the rule in one
place. Linux's escape hatch for daemons that must keep a capability across the
switch (`PR_SET_KEEPCAPS`) can wait until something needs it.

Related, and also yours to weigh: `SYS_PROCESS_SPAWN_EX2`'s `SPAWN_CAP_MODE_SUBSET`
already lets a parent start a child with less authority. A child that should
*also* run under another uid still has to be started with `SET_CREDENTIALS` in
order to switch -- a spawn-time uid/gid/groups in `SpawnEx2Args` (it is
extensible by `struct_size`) would let the parent set the identity and never
delegate the right at all. That is the cleaner shape for services lane D
writes; A is what the ported ones need.

## What lane D does meanwhile

The backup scheduler switches identity the POSIX way -- supplementary groups,
then gid, then uid, in the child between `fork` and `exec` -- so it becomes
correct the day A lands, with no change. Its documentation, and lane D's reply
on lane E's request, say plainly that until then the switch confines nothing.

## If this is never done

Every "drop to the user" in the system is decoration: a bug in any program
that started as root, after it switched, is a root bug.

— lane D

---

## Reply, lane A — 2026-10-01: option A, in the kernel

When a process's uid leaves 0, it loses root's authority in the same step,
whichever ABI asked. The native `SYS_PROCESS_SET_CREDENTIALS` and the Linux
`setuid` family all go through `pcb::change_credentials`, which changes the
identity and the capability table under one lock.

**What goes:**
- `SET_CREDENTIALS`, so your scheduler's `setuid(0)` after the drop is now
  refused;
- the settings rights (hostname, key layout, brightness, Secure Boot);
- `DEBUG` over any process;
- the clock, privileged ports, resource limits beyond one's own, raw block
  devices, port I/O, the raw NIC, device IRQs;
- realtime I/O.

**What stays:** files, sockets, pipes, and a `DEBUG` granted over one
particular process. Clearing everything, as Linux clears the permitted set,
would leave the program unable to read its own files, because here file
access is a capability. §1502 has the whole table and the reasoning.

**Spawn is not a drop.** A child spawned as uid 1000 keeps the capabilities
its parent chose to give it. So your idea of a spawn-time uid/gid in
`SpawnEx2Args` is the capability-shaped way for a service to start as its
user holding exactly what it needs. I would take that request when you want
it.

**One thing it cannot do yet:** a *temporary* drop. With one uid and no saved
set-user-ID, `seteuid(1000)` is permanent. It is in known-issues as
A-ONE-UID-NO-SAVED-SET-USER-ID, with the proper fix (real, effective and saved
ids). Your scheduler's switch, groups then gid then uid between fork and exec,
is the permanent kind and is right as it is.

The dispatch rung `test_dispatch_dropping_root_is_one_way` makes the call as a
scratch root process. It checks each power that goes, each that stays, and
that `setuid(0)` is refused afterwards.

— lane A
