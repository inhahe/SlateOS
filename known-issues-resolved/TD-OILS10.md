### TD-OILS10. `osh` `time` keyword / `times` builtin: user/sys CPU times are always reported as 0.00 — 2026-08-03 — ✅ **RESOLVED**: Windows 2026-08-03, Linux/SlateOS 2026-08-25

**Resolved on Linux too (2026-08-25),** which is the half that mattered, since
SlateOS's target is Linux-flavoured (`toolchain/x86_64-slateos.json` sets
`os = "linux"`, family `unix`) and the shell's reference comparisons now run
against a glibc bash. The gap was invisible while the only host was Windows and
surfaced the moment the unit suite was first run under Linux:
`time_charges_cpu_to_the_command_that_used_it` reported `0.00` for a loop that
burned **20.9 s of real time** — measured with `TIMEFORMAT=%3R/%3U/%3S`, osh
`20.941/0.000/0.000` against bash's `0.551/0.601/0.000` on the same loop.

The premise that had to go was "osh does not link libc's `rusage` bindings".
osh already declares libc functions where it needs them — `parent_pid` has an
`unsafe extern "C" { fn getppid() -> i32; }` a few hundred lines above — so
`getrusage` is declared the same way, with `struct rusage` spelled as the two
`timeval`s plus the fourteen `long` counters that follow them (named as a block
and never read, but present, because the kernel writes the whole structure).

**The two platforms now use the source each one actually keeps**, which is a
real structural difference and not a portability shim:

* **Windows** charges a *process*, and keeps the charge only while a handle to
  it is open — so the shell must read each child as it reaps it and add up the
  total itself. That is what `CHILD_USER_NS`/`CHILD_SYS_NS` and `account_child`
  are, and they are now `#[cfg(windows)]`.
* **Linux** keeps the running total in the kernel and hands it over on demand:
  `getrusage(RUSAGE_CHILDREN)` *is* the total over children already waited for,
  which is exactly the set `reap_child` has reaped. It is also the only route
  available, because `std` reaps with `waitpid` and discards the `rusage` a
  `wait4` would have returned — so there is nothing per-child to accumulate.

`RUSAGE_SELF` is the right class for the shell's own line because it sums every
thread in the process, and osh's subshells and pipeline stages *are* threads; a
per-thread figure would miss most of what a `time` is asked about.

**On SlateOS this is `posix`'s own `getrusage`** (`posix/src/resource.rs`),
which fills the two times for `RUSAGE_SELF` from the kernel's aggregate CPU
counters and leaves `RUSAGE_CHILDREN` zeroed until per-child accounting exists
there. The shell asks the same question on both, so SlateOS's answer improves
as the kernel's does with nothing in the shell to change.

The historical Windows-only account follows.

**Resolved:** the premise — "no per-child CPU accounting exists" — was only true
of `std`. Windows exposes it: `GetProcessTimes` answers for the current process
*and* for any child process whose handle is still open, which a
`std::process::Child` holds until it is dropped. So `interp.rs` gained

* `self_cpu_secs()` — the shell process's own `(user, system)`, which covers
  every osh thread (a subshell, a builtin pipeline stage) since they are all
  one process;
* `child_cpu_times()` + `account_child()` + the `CHILD_USER_NS`/`CHILD_SYS_NS`
  process-global totals, fed by `reap_child()` — now the single place a child
  is waited for, so no child is counted twice or missed;
* `Shell::cpu_used()`, the sum, which `exec_pipeline` samples on either side of
  a timed pipeline exactly as bash's `time_command` differences `times()`.

`times` prints the two cumulative pairs (shell, then children) it always
claimed to, and `$TIMEFORMAT`'s `%U`, `%S` and `%P` report real numbers.
Measured against the reference bash on the same busy loop: bash
`user 0m0.593s sys 0m0.030s`, osh `user 0m0.515s sys 0m0.062s`, with `times`
attributing them to the children line in both.

**Two documented approximations.** The child totals are per *process*, not per
`Shell`, because osh's subshells are threads — so a `( times )` sees the
totals its parent accumulated, where a forked bash's subshell starts from zero.
And on non-Windows hosts `self_cpu_secs`/`child_cpu_times` still return zero:
`getrusage` needs libc bindings osh does not have, and the SlateOS answer is
the process-accounting syscall described under "Proper fix" below. Both are
marked in the source. *(The second of those was closed on 2026-08-25 — see the
head of this entry. The first still stands.)*

**Where:** `userspace/oils/src/interp.rs` (`Shell::format_time_report`, called
from `exec_pipeline` when a pipeline is prefixed with `time`; and
`Shell::builtin_times`, the `times` builtin).

**Also affects `times`:** the POSIX `times` builtin prints two `user sys` lines
(shell, then children) in bash's `%dm%d.%03ds` form, but both are reported as
`0m0.000s` for the same reason — no per-process CPU accounting exists yet. The
line structure/format matches bash so parsers still work.

**What:** the `time` reserved word (and `time -p`) reports **real** (wall-clock)
time accurately via `std::time::Instant`, but the **user** and **system** CPU
time fields are hard-coded to `0m0.000s` / `0.00`. `std::process::Command` on
the host does not surface per-child `rusage`-style CPU accounting, and the
in-process builtin/function stages have no separate CPU meter. bash reports the
combined user/sys CPU consumed by the timed pipeline's children.

**Proper fix:** on SlateOS, gather child CPU times from the process-exit
accounting the kernel already tracks (wait/waitid returning cumulative
user/sys ticks) and sum them across the pipeline's stages; for in-process
stages, sample a per-thread CPU clock around the stage. Deferred: real time is
the field scripts most commonly want, and it is exact; user/sys reported as
zero is clearly documented and does not affect the pipeline's stdout/status.
