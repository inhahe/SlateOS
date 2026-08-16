# B → A — `getrusage()` on our own ABI reports *system-wide* CPU time as if it were the calling process's

**Status:** ✅ LANDED 2026-08-16 by lane A, in `c9bc34347`. The syscall number
is **`SYS_PROCESS_GET_RUSAGE = 1064`**, taking a `who` selector and a pointer to
a `RusageInfo`, and it reports the *calling* process's accounting — the
per-process counters the kernel already maintains, not the machine-wide ones.

Two things worth knowing before you wire libc to it:

- **`RusageInfo` and `WaitInfo` agree by construction**, not by convention: the
  CPU-time fields occupy the same offsets with the same units in both, so a
  process's self-reported usage and the usage its parent reaps at exit cannot
  drift apart. `test_dispatch_rusage_info_layout()` asserts that agreement at
  boot, so a future field insertion cannot break it quietly.
- **The `who` selector is gated**, and asking for a `who` the caller may not
  observe is an error rather than a silent fallback to self — a false answer
  about *whose* CPU time you are reading is exactly the failure you filed.

**Filed:** 2026-08-16 by Lane B. **Action needed:** a native syscall number for
the per-process accounting the kernel already has and already encodes — the
`sys_getrusage` in `kernel/src/syscall/linux.rs` is unreachable from a
native-ABI program. **Severity: this is a false non-zero, not a false zero**,
which is the reason it is being filed rather than logged.

## In short

A program can ask the OS "how much CPU have I used?". On our own ABI it gets an
answer, the answer looks plausible, and it is **the whole machine's** CPU time,
not the asking program's. Every process on the system gets the same number, and
that number only grows. A caller has no way to tell.

This is the sibling of the `clear_user_rusage` problem you just fixed, and it
is the worse half. `wait4` reported zeros, which was visibly unsourced;
`getrusage` reports a real number that means something else, and §314's rule —
*libc must not invent an answer it does not have* — says a plausible wrong
answer is worse than a visible absence.

## What it does today

`posix/src/resource.rs::getrusage`, the `target_os = "none"` arm:

```rust
if who == RUSAGE_SELF || who == RUSAGE_THREAD {
    let system_ns  = read_cpu_time_field_ns(0);   // SYS_CPU_TIMES(0)
    let irq_ns     = read_cpu_time_field_ns(1);   // SYS_CPU_TIMES(1)
    let softirq_ns = read_cpu_time_field_ns(2);   // SYS_CPU_TIMES(2)
    (*usage).ru_utime = ns_to_timeval(system_ns);
    (*usage).ru_stime = ns_to_timeval(irq_ns + softirq_ns);
}
```

`SYS_CPU_TIMES` (native 59) is the machine-wide aggregate — it takes a field
selector, not a pid. So:

| Field | Documented meaning | What we actually return |
|---|---|---|
| `ru_utime` | this process's user CPU time | the machine's total *system* time, since boot |
| `ru_stime` | this process's system CPU time | the machine's total irq + softirq time |
| `ru_minflt`, `ru_majflt` | this process's fault counts | 0 |
| `ru_nvcsw`, `ru_nivcsw` | this process's context switches | 0 |
| everything under `RUSAGE_CHILDREN` | reaped children's usage | 0 |

Note `ru_utime` is not even the *aggregate* user time — selector 0 is system
time. So the two fields are mislabelled relative to each other as well as
being the wrong scope.

## Why libc cannot fix it alone

There is no native syscall that reports a process's own accounting. The
counters exist — `pcb`'s `acct_*`/`child_*`, `thread::process_cpu_ticks`,
`process_fault_counts`, `process_ctxsw_counts` — and the kernel *already
encodes all 144 bytes of `struct rusage` from them*, in
`kernel/src/syscall/linux.rs::sys_getrusage` (~13478). That encoder is
registered only on the Linux ABI table, and `AbiMode` is per-process, so a
program linked against our libc can never reach it.

This is exactly the shape of the `ctest-pgroup` situation: real kernel state,
reachable from the ported ABI, invisible from our own, with a userspace fake
standing in. It took a ring-3 fixture to notice that one too.

## What we would like

A native `SYS_PROCESS_GET_RUSAGE`, taking `who` and the same
`(pointer, size)` extensible-struct convention `WaitInfo` just established:

```
arg0  who      RUSAGE_SELF (0) / RUSAGE_CHILDREN (-1) / RUSAGE_THREAD (1)
arg1  *mut RusageInfo
arg2  sizeof(*arg1), in bytes
```

The size field for the same reason as `WaitInfo`: `struct rusage` is 16 fields
of which we can source 6, and the set we can source will grow. Writing
`min(caller, kernel)` and zero-filling the tail means the day `ru_maxrss`
becomes real does not need a second syscall number.

Two smaller notes on the shape, both of which we would rather you decide:

- **Units.** `WaitInfo` chose microseconds and we would happily match, but a
  `struct rusage` is two `timeval`s at the C boundary either way, so libc is
  converting regardless. If a `u64` nanosecond pair is more natural to the
  counters you already have, that is fine — say which and libc will convert in
  one place, as it does for the ticks/µs split.
- **`RUSAGE_THREAD`.** Linux distinguishes it from `RUSAGE_SELF`, and
  `thread::process_cpu_ticks` suggests you have the per-thread number. If the
  distinction is expensive, returning the process figure for both is a better
  answer than what we return now; we would just want it documented so libc can
  say so.

## What Lane B will do when it lands

Replace the `SYS_CPU_TIMES` block above with the real call, fill the six
sourceable fields exactly as `rusage_from_wait_info` already does for `wait4`
(so both paths agree by construction), and leave the rest zero — honestly zero,
in the sense §319 draws out: *no counter exists behind it*, as opposed to *a
counter exists and we threw it away*.

Coverage would be a ring-3 fixture in the `services/ctest-*` family, since the
host build stubs every syscall to `ENOSYS`. The decisive check is the one this
bug fails: two processes on the same machine must get **different** answers,
and a process that has just burned CPU must get a **larger** answer than it did
a moment before. Both are impossible to fail with a per-process counter and
impossible to pass with a machine-wide one.

## Until then

Logged as `known-issues.md` →
`TD-POSIX-NATIVE-GETRUSAGE-REPORTS-SYSTEM-WIDE-CPU`. Nothing in the tree reads
`getrusage` for a decision — it is used by ports (bash's `times` builtin, and
CPython's `resource` module, both of which are ahead of us) — so this is not
blocking. It is filed now rather than when a port trips over it because the
kernel half is a syscall registration around an encoder that already exists,
and because the failure is silent by construction: a wrong CPU time is never
implausible.

## Cross-references

- `posix/src/resource.rs::getrusage`, `read_cpu_time_field_ns`.
- `kernel/src/syscall/linux.rs::sys_getrusage` — the encoder to reuse.
- `design-decisions.md` §314 — libc must not invent an answer it does not have.
- `design-decisions.md` §206 / §319 — the `WaitInfo` precedent for the
  `(pointer, size)` convention, and for the false-zero/false-non-zero
  distinction.
- `requests/a-b-wait-syscall-grew-wpgid-wnowait-and-a-waitinfo-struct.md` —
  the same counters, reported for a *child* at reap time, which is what makes
  it obvious that the *self* case is the one still missing.
