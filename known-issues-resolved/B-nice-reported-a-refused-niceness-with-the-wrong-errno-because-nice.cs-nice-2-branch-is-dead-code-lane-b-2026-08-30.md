## `nice` reported a refused niceness with the wrong errno, because `nice.c`'s `nice(2)` branch is dead code (lane B, 2026-08-30)

**Status: FIXED.** `scripts/nice-diff.sh` is back to **127 cases, 125 passed,
0 differed, 2 differ on purpose**.

### The symptom

Nine cases in `scripts/nice-diff.sh` — every one that lowers the niceness —
differed by one word:

```text
DIFF nice -n -5 nice
  ours: rc=0 out{0} err{nice: cannot set niceness: Operation not permitted}
  gnu : rc=0 out{0} err{nice: cannot set niceness: Permission denied}
```

`EPERM` where GNU says `EACCES`. Text and exit status were otherwise identical,
and the command still ran, so nothing about the *behaviour* was wrong — only
the diagnostic.

### Why it was latent for six days

Lowering the niceness needs privilege, so the message only appears when the
attempt is refused. The figures recorded when `nice` was ported were taken
where the attempt *succeeded* — both sides then print nothing at all and the
nine cases pass vacuously. They started failing when the harness ran as an
ordinary user under a `RLIMIT_NICE` of 0 (`uid=1000`, `ulimit -e` → `0`),
which refuses every lowering. The harness itself is byte-identical to the
version that scored 0 differed; only the environment moved.

This is the second time a divergence has hidden behind a case that cannot
distinguish the two implementations — the first was the `rm` quoting bug, where
no measured row held both `/` and `'`. The lesson is the same: a green harness
proves the cases it ran, not the cases it could have run.

### The root cause: a `#if` that never fires

`nice.c` reads as though it calls `nice(2)`:

```c
#if HAVE_NICE
# define GET_NICENESS() nice (0)
#else
# define GET_NICENESS() getpriority (PRIO_PROCESS, 0)
#endif
...
#if HAVE_NICE
  ok = (nice (adjustment) != -1 || errno == 0);
#else
  current_niceness = GET_NICENESS ();
  if (current_niceness == -1 && errno != 0)
    error (EXIT_CANCELED, errno, _("cannot get niceness"));
  ok = (setpriority (PRIO_PROCESS, 0, current_niceness + adjustment) == 0);
#endif
```

We ported the first arm. It is **never compiled on glibc**, because
`configure.ac` only asks whether `nice` exists when three-argument
`setpriority` does not:

```text
AC_CACHE_CHECK([for 3-argument setpriority function], [utils_cv_func_setpriority], ...)
if test $utils_cv_func_setpriority = no; then
  AC_CHECK_FUNCS([nice])
fi
```

`setpriority` exists on glibc, so `AC_CHECK_FUNCS([nice])` never runs,
`HAVE_NICE` is never defined, `#if HAVE_NICE` is false, and the shipped binary
takes the `getpriority`/`setpriority` arm.

That matters because glibc's `nice()` is itself `getpriority` + `setpriority`
with **`EACCES` rewritten to `EPERM`** on the way out. Measured under WSL:

```text
nice(-5)                        -> -1, errno 1  Operation not permitted
setpriority(PRIO_PROCESS, 0, n) -> -1, errno 13 Permission denied
```

Two independent lines of evidence agree — the `configure.ac` conditional, and
the observed `Permission denied` that only the `setpriority` spelling can
produce.

### The fix

`imp::set_niceness` in `userspace/coreutils/src/bin/nice.rs` now takes an
**absolute** niceness and calls `setpriority(PRIO_PROCESS, 0, ...)`; the caller
reads the current niceness with `getpriority` first and adds the adjustment,
exactly as the surviving arm does — including upstream's fatal
`cannot get niceness` if that read fails, which we did not have before because
the `nice()` spelling never needed it. The `extern` block no longer declares
`nice`. The module docs gained a section on the dead `#if`, and the defect
table's `nice(adjustment)` row is corrected.

The warning-versus-fatal split is unaffected: upstream's `perm_related_errno`
is `EACCES || EPERM` and Rust maps both to `io::ErrorKind::PermissionDenied`,
so only the rendered string moved.

### The general hazard, for the next port

**Do not port a `#if` arm without establishing which arm the shipped binary
compiled.** Reading `src/*.c` is not enough; the answer is in `configure.ac`,
and here the two arms differ in observable output. Anywhere else a coreutils
source offers a portability fork, the same check is owed.
