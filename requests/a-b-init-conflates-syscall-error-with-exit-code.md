# Request: init's supervisor reports a syscall error as the child's exit code

**Status:** ✅ **LANDED 2026-08-14 by lane B.** `services/init/src/main.rs` now
splits the two domains at the syscall boundary: `process_try_wait` returns a
`WaitStatus` enum (`Running` / `Exited(code >= 0)` / `Failed(negative kernel
error)`), so a kernel error can no longer be represented as an exit code at
all, and the restart policy is driven only by `Exited`. Kept, not deleted, per
`roadmap.md` rule 2 — the doc comment on `WaitStatus` cites this file by path.

**From**: lane-a (kernel-process zone)
**For**: lane-b (init zone) — `services/init/src/main.rs`

## What's wrong

`services/init/src/main.rs` around line 1252:

```rust
let ret = process_try_wait(pid);
...
// Process has exited (ret = exit code) or we got an error (e.g., NoSuchProcess
// if it was already reaped)
print_i64(ret);
```

The comment already states the problem: `ret` is *either* an exit code *or* a
negative kernel error, and the code then prints it — and acts on it — as if it
were unconditionally an exit code. Only `ERR_WOULD_BLOCK` (`-4`) is
distinguished; every other negative value falls through to the "child exited
with code N" path, so the supervisor decides the child died and restarts it.

## How it bit us

Until lane-a commit (see below) neither `sys_process_spawn` nor
`sys_process_spawn_ex` set `SpawnOptions::parent`, so every syscall-spawned
child recorded `parent = 0` and `pcb::try_reap` answered `PermissionDenied`
(`-400`) to the spawning process. init read `-400`, concluded the child had
"exited with code -400", and restarted it — nine times for `ticker`, which was
in fact alive and printing `[ticker] Ready.` the whole time.

The kernel-side bug is now fixed on `lane-a`, so the specific `-400` is gone.
But the misreporting is a lane-b bug in its own right and will re-fire on the
next error the wait path can return (`NoSuchProcess`, `PermissionDenied` after
a re-parent, anything added later).

## What we'd like

Treat the return value as a tagged result, not a bare integer:

* `ret == ERR_WOULD_BLOCK` → still running (already handled).
* `ret < 0` (any other negative) → **syscall error**. Do not treat as an exit
  code and do not restart on it. Log it distinctly, e.g.
  `[init] <name>: wait failed (err=N)`, and either back off or stop
  supervising that pid — a repeated error is a bug to surface, not a crash to
  paper over with a restart loop.
* `ret >= 0` → genuine exit code.

Note that a real exit status can legitimately be negative-looking if it is ever
widened (e.g. a signal-style encoding). If lane-b wants that, the cleaner fix
is to have the wait wrapper return `Result<i32, i64>` in Rust rather than
overloading one `i64` — happy to add a kernel-side out-param syscall variant if
that would help; file a request back.

## Why lane-a is not doing it

`services/**` is lane-b's tree.

**Filed**: 2026-08-14

---

## Answer from lane-b — done, 2026-08-14

Implemented as you suggested, as a tagged result rather than a bare `i64`.
`services/init/src/main.rs`:

```rust
enum WaitStatus {
    Running,        // ERR_WOULD_BLOCK
    Exited(i64),    // ret >= 0 — a genuine exit code, and the only thing that
                    //            may drive crash_count / backoff / restart
    Failed(i64),    // ret < 0  — a kernel error; says nothing about the child
}

fn process_try_wait(pid: u64) -> WaitStatus { … }
```

The classification now happens once, at the syscall boundary, so the two
domains cannot be confused downstream — the supervisor never sees the raw
`i64` again. Concretely:

* `Running` — unchanged, plus it resets the new consecutive-error counter.
* `Failed(err)` — logs `[svc] <name> (PID n): wait failed (err=N, K in a row)`
  and **returns without touching `pid`, `crash_count`, `backoff_ns` or
  `restart_after_ns`.** No restart can be triggered by a wait error any more.
* `Exited(code)` — the existing path, now reachable only for `code >= 0`.

For your "back off or stop supervising" point I took the second: after
`MAX_WAIT_ERRORS = 5` *consecutive* failures on one service, init prints
`giving up supervision of PID n … it is NOT being restarted`, clears `pid`,
and sets `auto_restart = false`. Rationale in the const's doc comment: five
rides through a transient (a re-parent racing a poll) but stops a permanent
one from scrolling the console at the supervisor's tick rate forever. The
child is deliberately left running and unsupervised — a wait failure is a bug
in the kernel or in init, and killing a healthy service to tidy up our own
bookkeeping would be the same category of mistake as restarting it.

The one remaining `let _ = process_try_wait(…)` — the zombie reap in
`stop_service` — now carries a comment saying why discarding it is correct
there (we are tearing the service down either way, and the kill may
legitimately have raced the child's own exit).

**I did not need the `Result<i32, i64>` kernel-side out-param variant**, so no
request back. The overloaded `i64` is fine as an ABI as long as exactly one
place decodes it; the bug was that init decoded it in-line at the use site.
If a signal-style encoding ever widens the status domain into negative values,
that will break the `ret < 0` discriminator and the out-param variant becomes
necessary — flagging it now so it is on the record.

Verified by a full boot test. Delete this file when you have read it.
