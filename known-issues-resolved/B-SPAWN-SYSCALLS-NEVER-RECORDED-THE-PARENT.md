### B-SPAWN-SYSCALLS-NEVER-RECORDED-THE-PARENT. Every syscall-spawned child was unreapable, uncapability'd, and escaped its parent's namespace — 2026-08-14 — FIXED 2026-08-14

**Where:** `kernel/src/syscall/handlers.rs` — `sys_process_spawn` (~3279) and
`sys_process_spawn_ex` (~3397). Fallout in `kernel/src/proc/spawn.rs`
(`SpawnOptions::parent`, steps 5b/5c) and `kernel/src/proc/pcb.rs`
(`try_reap`, 4219-4247).

**Symptom that led here.** The `ticker` service crash-looped nine times in a
routine boot log, each time reported by init as `exited with code -400` — while
`[ticker] Ready.` kept appearing in the same log. A process cannot both be
dead and printing. `-400` is `KernelError::PermissionDenied`, and the only
permission check on the wait path is `pcb::try_reap`'s
`proc.parent != parent_pid`.

**Root cause.** `SpawnOptions` has a `parent(pid)` builder, and **neither spawn
syscall ever called it**. Both built `SpawnOptions::new(name)` and left
`options.parent = 0`. So every process ever created through
`SYS_PROCESS_SPAWN` / `SYS_PROCESS_SPAWN_EX` recorded the kernel (PID 0) as its
parent. Three consequences, all silent:

1. **No process could reap its own children.** `try_reap` compares the caller
   against the recorded parent and returns `PermissionDenied`. Every spawned
   child therefore leaked as a zombie for the life of the system, and every
   supervisor mis-read the error as an exit status (see the lane-b request
   `requests/a-b-init-conflates-syscall-error-with-exit-code.md` — init prints
   `ret` from `process_try_wait` as an exit code without checking its sign,
   which is how `-400` became "exited with code -400" and triggered a restart).
2. **The parent was never granted a `Process` capability over the child.**
   `spawn_process` step 5b (spawn.rs:974) is gated on `options.parent != 0`, so
   the READ|WRITE|DELETE|WAIT|SIGNAL|DUPLICATE grant never happened. Callers
   had no handle with which to signal or kill what they had spawned.
3. **Sandbox escape.** Step 5c (spawn.rs:1001) inherits the parent's filesystem
   namespace, and is gated on the same condition. A process confined to a
   non-root namespace could spawn a child that landed in the *root* namespace.
   This is the serious one: confinement was defeated by the single act of
   spawning.

**Why nobody noticed.** `fork` sets the parent on its own path, so every
fork→exec→wait test passed. Nothing in the test suite spawned via the syscall
*and then reaped*. And `SpawnOptions::parent` carried
`#[allow(dead_code)] // Public builder API — callers use SpawnOptions::new() + chaining.`
— a suppression that was factually wrong (there were no chaining callers) and
that turned "this builder method has zero callers" from a compiler warning into
a comment asserting the opposite. That is the real lesson: an `#[allow]` whose
justification is a claim about the rest of the codebase silently rots when the
claim stops being true.

**Fix.** Both syscalls now pass `.parent(caller_pid().unwrap_or(0))`.
`unwrap_or(0)` is correct rather than a fallback: no caller PID means the spawn
came from the kernel, and PID 0 has implicit authority and needs no grant. The
`#[allow(dead_code)]` on `SpawnOptions::parent` is removed — if it ever goes
unused again that should be a warning, not a comment.

**Regression test.** `test_spawn_records_parent` in `kernel/src/proc/spawn.rs`
(registered in `spawn::self_test`) spawns with an explicit parent, asserts
`pcb::parent(child) == Some(parent)`, then asserts `try_reap(parent, child)` is
**not** `PermissionDenied`. It deliberately accepts "still running" as a pass:
the bug produced a specific error code, and testing for its absence is what
distinguishes the fix from emulator timing.
