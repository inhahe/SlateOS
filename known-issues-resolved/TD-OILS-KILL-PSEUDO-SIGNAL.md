### TD-OILS-KILL-PSEUDO-SIGNAL. `kill -s DEBUG %1` errors in osh where bash silently succeeds — 2026-07-27 — ✅ CLOSED WONTFIX 2026-08-06 (host artifact; osh already matches bash)

**Where:** `userspace/oils/src/interp.rs` — the pseudo-signal rejection at the
end of `builtin_kill`'s option run.

**What.** `sleep 1 & kill -s DEBUG %1` gives rc 0 and no output in bash; osh
prints `kill: DEBUG: invalid signal specification` per target and gives rc 1.

**Why.** bash's `decode_signal` accepts `DEBUG` (it is a real entry in the shared
`signal_names` table, numbered above `NSIG`), and bash only learns the number is
unsendable when `kill(2)` refuses it — but for a *job spec* bash calls
`killpg`/`kill` on the job's pgrp and, on this host, the call succeeds because
the number is not validated against `NSIG` before the syscall. osh has no real
signals, so it must decide for itself, and it decides the spec is invalid.

**Impact.** Negligible: sending a pseudo signal is meaningless in either shell.
Deliberately left out of the corpus.

**Closed WONTFIX 2026-08-06** — the confirmation the entry asked for, read out of
bash 5.2's own source (`/d/refs/bash-5.2`), says osh is already right and the
reference shell is the odd one out. bash has **no policy here at all**: it
decodes the name and then lets the kernel decide.

`trap.h:40-46` puts the pseudo signals immediately above the real table —

```c
#define DEBUG_TRAP	NSIG
#define ERROR_TRAP	NSIG+1
#define RETURN_TRAP	NSIG+2
#define BASH_NSIG	NSIG+3
```

— and `decode_signal` (`trap.c:259`) scans `signal_names` all the way to
`BASH_NSIG`, so `DEBUG` decodes to `NSIG` rather than to `NO_SIG`. `kill.def`
therefore never takes its `sig == NO_SIG` branch (line 173) and goes straight to
the syscall, in *both* the pid path (line 197) and the job-spec path (line 239):

```c
if (kill_pid (pid, sig, 1) < 0)
  {
    if (errno == EINVAL)
      sh_invalidsig (sigspec);
    else
      kill_error (pid, errno);
    CONTINUE_OR_FAIL;
  }
```

`sh_invalidsig` is `builtins/common.c:252`, `"%s: invalid signal specification"`
— **the exact message osh prints, naming the same spec, failing the same way,
once per target.** On Linux, `kill(2)` rejects a number above `SIGRTMAX` with
`EINVAL`, so bash there behaves as osh does. The silent rc-0 success measured on
this host is Cygwin's `kill()` accepting an out-of-range number, not bash policy;
matching it would have made osh diverge from bash everywhere else.
