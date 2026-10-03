## A-FD-TABLE-CAPACITY-IS-256-AND-THAT-IS-NOW-THE-ADVERTISED-LIMIT (lane A)
**Status:** OPEN — 2026-08-21

**In short.** A program can have at most 256 files open at once. Until today
the kernel *told* programs they could have 4096, which was a lie that produced
"too many open files" errors at 256 with a limit that read 4096. The lie is
fixed — `getrlimit(RLIMIT_NOFILE)` now honestly reports 256. What is left is
the underlying number: 256 is low for a real build (`make -j`, a compiler with
many headers open, a shell with several pipelines) and raising it is a change
in two lanes at once, not one.

**Where it lives.**

| | |
|---|---|
| kernel fd table | `kernel/src/proc/linux_fd.rs:57` — `MAX_FDS = 256`, `[Option<FdEntry>; MAX_FDS]` per PCB (8 KiB) |
| kernel advertised limit | `kernel/src/proc/pcb.rs` `DEFAULT_RLIMITS[RLIMIT_NOFILE]`, now derived from `MAX_FDS_U64` |
| kernel enforcement | `pcb::linux_fd_install` (soft limit) and `FdTable::install_lowest_from` (array bound) |
| libc fd table | `posix/src/fdtable.rs:73` — its own `MAX_FDS = 256`, **plus** a `MAX_FDS × FD_PATH_MAX` = 1 MiB static path buffer sized from it (`fdtable.rs:569`) |

**Why it is not just a constant to bump.** Three numbers have to move together
and two of them are in lane B's tree. Raising the kernel alone puts the two fd
tables out of sync in the *opposite* direction from the bug just fixed — libc
would refuse fd 300 that the kernel happily installed. Raising both takes libc's
static path buffer from 1 MiB to 4 MiB at 1024 slots, which is a real cost in a
`static mut` that every process carries. The honest options are (a) bump both
and pay the 4 MiB, (b) make libc's path buffer sparse/heap-backed first and then
bump both, or (c) leave it and treat 256 as the platform's answer. This is a
`requests/a-b-*.md` and a decision, not an edit.

**What was fixed today, so this is not confused with it.** `DEFAULT_RLIMITS`
said `(1024, 4096)` against the 256-slot table. It now reads
`(linux_fd::MAX_FDS_U64, linux_fd::MAX_FDS_U64)`, derived rather than restated,
with `const _: () = assert!` guards that fail the build if anyone writes a
literal there again — which is how it drifted in the first place. `set_rlimit`
additionally caps `RLIMIT_NOFILE`'s hard limit at `MAX_FDS` *unconditionally*,
so the ceiling survives the day `CAP_SYS_RESOURCE` makes hard-limit raises legal
for every other resource. Covered by `pcb::self_test::test_rlimits`.

**How you would notice.** A ported program that opens many files fails with
`EMFILE` at 256 — but now with a `getrlimit` that agrees, so the program can
size its pool correctly or report the real limit instead of being surprised.
That is the whole difference between this entry and the bug it replaces.

**Found** 2026-08-21 by lane A while implementing `SYS_RLIMIT_GET`/`SYS_RLIMIT_SET`
(`requests/b-a-native-rlimit-syscalls.md`), whose filer had spotted the
contradiction and asked which of the two numbers should move.
