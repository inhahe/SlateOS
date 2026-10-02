### TD-POSIX-NATIVE-GETRUSAGE-REPORTS-SYSTEM-WIDE-CPU. `getrusage()` on our own ABI returns the machine's total CPU time as if it were the caller's — LOGGED 2026-08-16 by lane B, filed to lane A — ✅ FIXED 2026-08-16 by lane A (`SYS_PROCESS_GET_RUSAGE = 1064`, commit `c9bc34347`)

**In short:** a program can ask the OS "how much CPU have I used?". On our own
ABI it gets an answer, the answer looks entirely plausible, and it is **the
whole machine's** CPU time rather than the asking program's. Every process gets
the same number and that number only ever grows. There is no way for a caller
to tell.

**This is a false non-zero, and that is the point.** It is the sibling of the
`clear_user_rusage` bug lane A just fixed, and the worse half: `wait4` reported
zeros, which are visibly unsourced, whereas this reports a real measurement of
something else. §314's rule — libc must not invent an answer it does not have —
is aimed exactly here.

**Where it lives.** `posix/src/resource.rs::getrusage`, the
`target_os = "none"` arm. It fills `ru_utime` from `SYS_CPU_TIMES(0)` and
`ru_stime` from `SYS_CPU_TIMES(1) + SYS_CPU_TIMES(2)`. `SYS_CPU_TIMES` (native
59) takes a *field selector*, not a pid — it is the machine-wide aggregate. Note
selector 0 is **system** time, so `ru_utime` is not even the aggregate *user*
time; the two fields are mislabelled relative to each other as well as being
the wrong scope. `ru_minflt`/`ru_majflt`/`ru_nvcsw`/`ru_nivcsw` and all of
`RUSAGE_CHILDREN` are zero.

**Why libc cannot fix it alone.** There is no native syscall reporting a
process's own accounting. The counters exist (`pcb`'s `acct_*`/`child_*`,
`thread::process_cpu_ticks`, `process_fault_counts`, `process_ctxsw_counts`) and
the kernel already encodes the full 144-byte `struct rusage` from them in
`kernel/src/syscall/linux.rs::sys_getrusage` (~13478) — but that encoder is
registered only on the **Linux** ABI table, and `AbiMode` is per-process, so a
program on our libc can never reach it. This is the same shape as
`TD-POSIX-PROCESS-GROUPS-ARE-FAKE-FOR-NATIVE-ABI-PROGRAMS`: real kernel state,
reachable from the ported ABI, invisible from our own, with a userspace
approximation standing in.

**Proper fix** — filed as
`requests/b-a-native-getrusage-reports-system-wide-cpu-as-per-process.md`: a
native `SYS_PROCESS_GET_RUSAGE` taking `who` plus the `(pointer, size)`
extensible-struct convention `WaitInfo` established, wrapping the encoder that
already exists. Lane B then fills the six sourceable fields exactly as
`rusage_from_wait_info` already does for `wait4`, so the self and child paths
agree by construction.

**Not blocking.** Nothing in the tree reads `getrusage` for a decision; the
callers are ports that are ahead of us (bash's `times` builtin, CPython's
`resource` module). Filed now anyway because the kernel half is a registration
around existing code, and because the failure is silent by construction — a
wrong CPU time is never implausible.

**Coverage when it lands** must be ring-3 (the host build stubs every syscall
to `ENOSYS`), and the decisive checks are the two this bug cannot pass: two
processes on the same machine must get **different** answers, and a process that
has just burned CPU must get a **larger** answer than a moment before.
