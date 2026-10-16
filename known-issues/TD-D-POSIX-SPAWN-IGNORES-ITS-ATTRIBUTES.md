### [D] TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES — 2026-09-24 — OPEN

**Status:** OPEN — three of seven attributes left. `POSIX_SPAWN_SETPGROUP`,
`SETSIGMASK`, `SETSIGDEF` and `SETSID` are applied since 2026-10-16, through
lane A's `SpawnEx2Args` fields (`requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`).
`RESETIDS`, `SETSCHEDPARAM` and `SETSCHEDULER` are still recorded and not
applied.

**In short:** a program can ask `posix_spawn` to start its child with its
effective user and group reset to its real ones, or with a scheduling policy
and priority. Those requests are accepted and then ignored: the child starts
with the parent's ids and scheduling. Until 2026-10-16 the same was true of
the process group, the session and the signal mask, and of the signals to
reset from ignored -- which had become a regression once ignored signals
reached the kernel: `system()` ran its shell with `SIGINT` and `SIGQUIT`
ignored, so `^C` could not stop what it ran (Opus's review of lane D's round
202).

**Where:** `posix/src/spawn.rs` — `spawn_attrs` turns the four applied
attributes into `SpawnEx2Args`'s six fields; nothing reads `RESETIDS`,
`schedpolicy` or `schedpriority`.

**Who reaches it:** a set-user-id program that spawns a helper with
`RESETIDS` to drop its privilege (the child keeps it -- the safe direction
would be to refuse); one that spawns a real-time worker with
`SETSCHEDULER`, as `rtkit`-style tools do (the child runs at the parent's
policy).

Today the library reports a process's effective ids as its real ones
(`posix/src/unistd.rs`, `geteuid`), not having taken up lane A's separate
ids (`SYS_PROCESS_GET_IDS`, 1160), so as far as it can see `RESETIDS` has
nothing to reset. **Taking up 1160 must settle `RESETIDS` in the same
change** -- apply it, or refuse a spawn that asks for it while the ids
differ -- or a set-user-id program's helper will silently keep the
privilege it was meant to drop.

**Proper fix:** `RESETIDS` is "effective ids := real ids" before the first
instruction, which needs either a `SpawnEx2Args` field or the child's own
`SYS_PROCESS_SET_IDS` (1159) call before `main` -- the first is
race-free, the second runs code with the old ids first. `SETSCHEDULER` and
`SETSCHEDPARAM` need lane A's per-thread policy (`SYS_THREAD_SCHEDULER`,
1146) applied to the child's first thread at spawn, which `SpawnEx2Args`
has no field for: a request to lane A for both, as for the four done.
