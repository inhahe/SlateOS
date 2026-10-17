# D → A: `posix_spawn`'s `RESETIDS` and its scheduling need two more spawn fields

**Filed:** 2026-10-16 by lane D. **For:** lane A (`kernel/src/proc/spawn.rs`,
`SpawnEx2Args`). **Status:** OPEN.

## In short

`posix_spawn` lets a program start its child with its effective user and
group ids reset to its real ones (`POSIX_SPAWN_RESETIDS`) -- how a
set-user-id program starts a helper without its privilege -- or with a
scheduling policy and priority (`POSIX_SPAWN_SETSCHEDULER`,
`SETSCHEDPARAM`). Your spawn applies four of the attributes before the
child's first instruction since 2026-10-16 (`SpawnEx2Args`'s last six
fields, `d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`);
these three have no field. Today the library **refuses** `RESETIDS` with
`ENOTSUP` while the caller's effective ids differ from its real ones -- a
child that kept them would keep the privilege it was spawned to drop -- and
**ignores** the two scheduling ones (`known-issues/TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES.md`).

## Asked

1. **Reset the child's effective ids to the parent's real ones**, user and
   group, before its first instruction: what glibc's child does with
   `seteuid(getuid())` and `setegid(getgid())` between `clone` and `exec`.
   A flag in `SpawnEx2Args` would do; the saved and filesystem ids then
   follow as an exec leaves them (the saved id from the effective one).
   It is never a privilege gain -- every process may set its effective id
   to its real one -- so it needs no right.
2. **The child's first thread's scheduling policy and priority**, as your
   `SYS_THREAD_SCHEDULER` (1146) sets them on a running thread, applied
   before it runs -- with that call's refusals, which `posix_spawn` then
   answers (Linux's `EPERM` for a real-time policy without the right).

The library side for each is a few lines in `posix/src/spawn.rs`
(`spawn_attrs`, `refused_request`), and `ctest-spawn-attrs` would gain the
checks.
