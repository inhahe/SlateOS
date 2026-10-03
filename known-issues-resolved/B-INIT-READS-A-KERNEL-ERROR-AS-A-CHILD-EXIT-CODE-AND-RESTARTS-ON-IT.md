### [B] B-INIT-READS-A-KERNEL-ERROR-AS-A-CHILD-EXIT-CODE-AND-RESTARTS-ON-IT — ✅ FIXED 2026-08-14
**Status:** FIXED 2026-08-14

**Reported by lane-a** in
`requests/a-b-init-conflates-syscall-error-with-exit-code.md`; fixed in
`services/init/src/main.rs`, which is lane-b's tree.

**Where:** `ServiceRegistry`'s supervisor poll (`services/init/src/main.rs`,
the `process_try_wait` call in the reap loop) and the `process_try_wait`
wrapper itself.

**What was wrong.** `SYS_PROCESS_TRY_WAIT` overloads one `i64` with two
disjoint domains — a child's exit status (`>= 0`) and a kernel error (`< 0`).
init decoded that `i64` *at the use site*, and only special-cased
`ERR_WOULD_BLOCK` (`-4`). Every other negative value fell through to the
"child exited with code N" path, so the supervisor concluded the service had
died and scheduled a restart.

**How it bit.** Before lane-a's kernel fix, neither `sys_process_spawn` nor
`sys_process_spawn_ex` set `SpawnOptions::parent`, so every syscall-spawned
child recorded `parent = 0` and `pcb::try_reap` answered `PermissionDenied`
(`-400`). init read `-400` as an exit code and restarted `ticker` **nine
times** — while `ticker` was alive and printing `[ticker] Ready.` throughout.
The kernel half is fixed, but the misreading is a bug on its own terms and
would re-fire on the next error the wait path can return (`NoSuchProcess`,
`PermissionDenied` after a re-parent, anything added later).

**The fix.** Classify once, at the syscall boundary, so the two domains cannot
be confused downstream:

```rust
enum WaitStatus { Running, Exited(i64), Failed(i64) }
fn process_try_wait(pid: u64) -> WaitStatus { … }
```

- `Running` — as before, and it resets the new consecutive-error counter.
- `Failed(err)` — logs `[svc] <name> (PID n): wait failed (err=N, K in a row)`
  and returns **without touching** `pid`, `crash_count`, `backoff_ns` or
  `restart_after_ns`. No restart can be caused by a wait error any more.
- `Exited(code)` — the pre-existing path, now reachable only for `code >= 0`.

After `MAX_WAIT_ERRORS = 5` *consecutive* failures on one service init prints
`giving up supervision of PID n … it is NOT being restarted`, clears `pid` and
sets `auto_restart = false`. Five rides through a transient (a re-parent
racing a poll) and stops a permanent one from scrolling the console at the
supervisor's tick rate. The child is deliberately left running and
unsupervised: a wait failure is a bug in the kernel or in init, and killing a
healthy service to tidy up our own bookkeeping is the same category of mistake
as restarting it.

**The general shape, worth remembering.** An overloaded return value is a fine
ABI — as long as **exactly one place decodes it**. The bug was not the `i64`;
it was decoding it inline at the use site, where the error case is easy to
forget and impossible to see. Nothing downstream of `process_try_wait` can now
name a raw wait return at all.

**Known residue (not a bug today).** The `Failed` discriminator is `ret < 0`.
If the status domain is ever widened into negative values — a signal-style
encoding, say — that test stops being sound and the kernel-side out-param
variant lane-a offered becomes necessary. Recorded so the assumption is
written down rather than implied.

**Verified.** `services/init` builds clean for `x86_64-unknown-none`; full
QEMU boot test green.
