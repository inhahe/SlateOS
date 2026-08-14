# Request: init's supervisor reports a syscall error as the child's exit code

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
