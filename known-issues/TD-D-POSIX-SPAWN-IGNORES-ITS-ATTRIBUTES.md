### [D] TD-D-POSIX-SPAWN-IGNORES-ITS-ATTRIBUTES — 2026-09-24 — OPEN

**Status:** OPEN — lane D's code; the proper fix needs spawn-time fields in
lane A's `SpawnEx2Args`, requested 2026-09-25 in
`requests/d-a-ignored-signals-and-spawn-attributes-need-a-kernel-record.md`.

**In short:** a program can ask `posix_spawn` to start its child in a new
process group, with certain signals blocked or reset, or in a new session. All
of those requests are accepted and then ignored: the child starts in the
parent's group, with the parent's signal mask. Job-control shells and anything
using Rust's `Command::process_group` get a child that `^C` and `fg`/`bg` will
treat as part of the parent.

**Where:** `posix/src/spawn.rs` — `posix_spawn` and `posix_spawnp` take
`attrp` and hand nothing of it to `spawn_impl`. `posix_spawnattr_set*` store
their values faithfully (and have tests), so the object is right and the
consumer is missing: `POSIX_SPAWN_SETPGROUP`, `SETSIGMASK`, `SETSIGDEF`,
`SETSID`, `RESETIDS` and `SETSCHEDULER` are all no-ops. The module doc's claim
that `SETPGROUP` "is meaningfully supported" is not true of the code.

**Who reaches it:** Rust `std` on this target takes the `posix_spawn` path for
any `Command` without a `pre_exec` closure; `process_group(pgid)` there
becomes `POSIX_SPAWN_SETPGROUP`. (Oils uses `pre_exec`, so it takes the
`fork` + `execvp` path and calls `setpgid` itself — not affected.)

**Proper fix:** the child must be in its group and have its mask *before* its
first instruction, which the parent cannot arrange after the spawn returns
without a race. That means `SpawnEx2Args` fields — `pgid`, `sigmask`,
`sigdefault`, a `setsid` flag — applied by the kernel as `SpawnOptions` already
applies `cwd` and `uid_gid`; `struct_size` makes them additive. The interim of
calling `setpgid(child, pgid)` from the parent after the syscall is the race
shells tolerate for `fork`, but it is not what `posix_spawn` promises, and it
does not help the signal attributes at all. File with the cwd request if lane A
takes that one, since both widen the same struct.
