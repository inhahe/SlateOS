## 2. argv/envp persistence for /proc — Approach A (persistent PCB snapshots)

**Date:** 2026-06-10

**Decided by:** Claude (autonomous) — an implementation choice among three
mechanisms, resolved while building `/proc/<pid>/cmdline` and `environ`.

**Context:**
`/proc/<pid>/cmdline` and `/proc/<pid>/environ` must report a process's argv
and environment for the whole lifetime of the process. But the PCB's
`initial_argv`/`initial_envp` are **one-shot**: they are drained by
`SYS_PROCESS_GET_ARGS` at child startup and then cleared
(`take_initial_args`). After startup there was no surviving copy to serve to
procfs.

**Decision — Approach A: keep a separate, persistent snapshot in the PCB.**
- Added `pub proc_argv: Vec<Vec<u8>>` and `pub proc_envp: Vec<Vec<u8>>` to
  `Process`.
- Populated in `set_initial_args` by cloning before the one-shot move into
  `initial_argv`/`initial_envp`.
- **Never drained** (distinct from the one-shot fields).
- **Inherited across `fork`** (cloned from parent), matching POSIX semantics
  where a child initially shares the parent's argv/env view.
- Read by procfs via `get_proc_argv(pid)` / `get_proc_envp(pid)`.

**Rationale:**
- Stores the data as **bytes** (`Vec<Vec<u8>>`), honoring the project rule
  that argv/env/paths are bytes, not UTF-8 strings.
- Cheap and simple: a clone at spawn (one-time) buys lifetime availability.
- Keeps the one-shot startup contract untouched, so no risk to the existing
  `SYS_PROCESS_GET_ARGS` fast path.

**Alternatives considered:**
- **Approach B — a "consumed" flag instead of clearing**: keep
  `initial_argv`/`initial_envp` populated but mark them consumed. Rejected:
  conflates two concerns (startup handoff vs. introspection) in one field and
  makes the drain semantics subtler; a future change to the startup path could
  silently break procfs.
- **Approach C — read argv/env back from the process's user stack on demand**:
  Linux-authentic (it reads `mm->arg_start..arg_end`). Rejected for now: needs
  safe cross-address-space reads, must tolerate a process that has overwritten
  its own argv (`setproctitle`), and is materially more code. Approach A's
  snapshot is "argv as captured at spawn," which is the common, predictable
  case. **If we later want `setproctitle` to be reflected, switch to C.**

**Where it lives:**
- `kernel/src/proc/pcb.rs`: fields (~line 308), spawn ctor (~line 949), fork
  destructure/clone/literal (~lines 1193/1238/1300), `set_initial_args`
  population (~line 3392), getters `get_proc_argv`/`get_proc_envp` (~line 3409).
- `kernel/src/fs/procfs.rs`: `gen_pid_cmdline`, `gen_pid_environ`.

**How to reverse:**
- To drop persistence: remove the two fields + getters and revert
  `gen_pid_cmdline` to the name-only form; delete `gen_pid_environ` and its
  `PID_FILES`/dispatch entries.
- To move to Approach C: replace the getters' bodies with user-stack reads and
  remove the snapshot fields once the stack reader is proven.
