### TD-POSIX-RLIMITS-ARE-A-SHADOW-OF-THE-KERNEL'S. libc keeps its own rlimit table instead of asking the kernel, so the two can disagree silently — ✅ **RESOLVED 2026-08-29** — 2026-08-16

**In short.** Every process has a set of "resource limits" — ceilings on things
like stack size, open files, or how much priority it may ask for. Our kernel
keeps the real ones. Our libc keeps a *second*, private copy and never consults
the kernel's. **They already disagree** — so a program that asks "how many
files may I open?" is told 1024 or 256 depending on which ABI it was compiled
for, on the same machine, in the same process.

(This entry originally said the two "happen to agree" and that the hazard was
latent. Both were assumptions, not measurements, and both were false. See
**Correction 2026-08-21** below.)

**Where it lives.** `posix/src/resource.rs` → `mod limit_store` (line 140; the
table itself is `RLIMITS_INIT` at line 151): on target a `static mut RLIMITS`,
on host a `thread_local!`. Every `getrlimit` / `setrlimit` / `prlimit64` reads
and writes *that*. The kernel's authoritative table is `DEFAULT_RLIMITS`
(`kernel/src/proc/pcb.rs:2382`), copied into `Process::rlimits` at creation
(`pcb.rs:1342`), inherited across `fork`, and read by the kernel's own
`setpriority`, `sched_setscheduler`, `brk` and `write` paths.

**Why libc cannot simply ask.** There is no native SlateOS syscall for
rlimits. The kernel's table is reachable only through the Linux-compat
translation layer's `GETRLIMIT` / `SETRLIMIT` / `PRLIMIT64` numbers — which is
the path a *ported Linux program* takes, not the path our own libc's
`getrlimit` currently takes. Making libc authoritative therefore needs either
(a) a native `SYS_RLIMIT_GET`/`SYS_RLIMIT_SET` pair, or (b) libc routing its
own calls through the compat numbers. Both are lane A's tree, so this needs a
`requests/b-a-*.md` before it can be done.

**Correction 2026-08-21 — it is not latent, and it was never two rows.** This
entry claimed the copies "are still in sync" and had drifted "on two rows."
Neither had been checked; both were wrong. Reading `DEFAULT_RLIMITS`
(`kernel/src/proc/pcb.rs:2382`) against `RLIMITS_INIT`
(`posix/src/resource.rs:151`) row by row gives **three** disagreements out of
sixteen, present today at cold start:

| # | Resource | kernel | libc |
|---|---|---|---|
| 7 | `RLIMIT_NOFILE` | `(1024, 4096)` | `(256, 256)` — `fdtable::MAX_FDS` |
| 11 | `RLIMIT_SIGPENDING` | `(65_536, 65_536)` | `(INFINITY, INFINITY)` |
| 12 | `RLIMIT_MSGQUEUE` | `(819_200, 819_200)` | `(INFINITY, INFINITY)` |

Every other row matches exactly. So the "it becomes live the moment the kernel
adjusts a limit" framing was backwards — it is live now, without the kernel
doing anything, purely because the two tables were typed twice.

Note the shape of the mistake, because it is the reusable part: the original
entry inferred "they agree" from "both were written from the same Linux
defaults." That is a plausible story about the code's history, not an
observation of the code. Two hand-maintained copies of one fact do not stay
equal because of where they came from.

**Two related bugs, in lane A's tree, found while measuring the above.** Both
are written up in the request; neither is Lane B's to fix. (i) The kernel's
`RLIMIT_NOFILE` default of `(1024, 4096)` contradicts the kernel's own Linux fd
table, `kernel/src/proc/linux_fd.rs:57`, which is `MAX_FDS = 256` — so the
kernel promises ported programs a number it cannot honour, and they get
`EMFILE` at 256. (ii) The kernel's `prlimit64` has no `sysctl_nr_open`
equivalent: Linux rejects any `RLIMIT_NOFILE` hard limit above it
unconditionally, but our handler (`kernel/src/syscall/linux.rs:45323`) checks
only `cur > max`. libc *does* enforce that ceiling (`posix/src/resource.rs:407`,
`EPERM` above `MAX_FDS`) — a third way the two ABIs answer one question
differently.

On (ii), note what the first draft of this correction got wrong, since it is
the same failure the entry is about. I wrote that a process could therefore
install `RLIMIT_NOFILE = (u64::MAX, u64::MAX)`. It cannot: `pcb::set_rlimit`
(`pcb.rs:2470`) independently rejects `new_max > old_max` for every resource,
so the reachable ceiling is the seeded hard limit, `4096`. Still 16× the
256-entry table, still `EMFILE` for anyone who believes it — but bounded. I had
inferred "no ceiling check on the path" from reading the `prlimit64` handler
and not read the function it delegates to. Reading one layer and predicting the
next is how this entry's original "they happen to agree" got written.

A related asymmetry found the same way: `pcb::set_rlimit`'s blanket
no-raise rule has *no* privileged escape (its doc: `CAP_SYS_RESOURCE` "we have
no equivalent"), and that is accurate — the kernel defines no such constant and
`pcb.rs:994` records that capabilities aren't enforced. libc's raise-gate keys
on `sys_capability::CAP_SYS_RESOURCE` (`posix/src/sys_capability.rs:157`),
which the kernel never sees, so it is self-asserted rather than authoritative.
When the syscalls land, libc drops its local check and inherits the kernel's
rule.

**Proper fix.** ✅ `requests/b-a-native-rlimit-syscalls.md` **filed 2026-08-21**
— asks for a native `SYS_RLIMIT_GET`/`SYS_RLIMIT_SET` pair keyed on the
kernel's `RLIMIT_*_INDEX` numbering, modelled on `SYS_TTY_GET_TERMIOS` /
`SYS_TTY_SET_TERMIOS` (which lane A built to fix this exact shape of bug for
`termios` — its own doc comment says *"libc previously answered this from a
hardcoded constant of its own"*). When it lands, **delete `limit_store`
outright** and call through every time. Do *not* build a cache: rlimit reads
are not a hot path, and a cache is how the same class of bug gets reintroduced
at smaller scale. The host-build `thread_local!` half stays as the test double.

Until it lands, do **not** add more libc logic that treats the local table as
authoritative beyond what `can_nice()` / `current_rtprio_limit()` /
`check_mlock_caps()` already do. Copying the kernel's three values across as an
interim fix is explicitly **not** wanted — it restores the appearance of
agreement, which is what stopped anyone checking for five days.

**Resolution 2026-08-29.** Lane A landed `SYS_RLIMIT_GET` / `SYS_RLIMIT_SET`
at 557/558 (`requests/a-b-native-rlimit-syscalls-landed.md`), and
`posix/src/resource.rs` now calls through on every `getrlimit`, `setrlimit` and
`prlimit`. `limit_store` was **deleted on the target**, not merely left unread
— the module and `RlimitTable` are `#[cfg(not(target_os = "none"))]`, so the
`static mut RLIMITS` does not exist in the shipped binary and no future edit
can start consulting it. The host arm keeps the `thread_local!` as the test
double the entry above asked for, but its *policy* was rewritten to be the
kernel's: the blanket no-raise rule with no privileged escape, replacing libc's
self-asserted `CAP_SYS_RESOURCE` gate. Four Phase-179 tests that asserted the
capability permits a hard raise were inverted and renamed to match, with a
companion test (`..._sentinel_lowering_still_works`) added so the inversion
could not be satisfied by a `setrlimit` that simply always fails. No cache was
built, per the instruction above. 20562 host tests pass; bare-metal `cargo
check` and lib clippy are clean. Rationale, alternatives and the reversal
procedure: `design-decisions.md` §707.

Three loose ends the rewiring produced, all tracked elsewhere rather than here:

- A **third** copy of the defaults turned up in `posix/src/linux_rlimit.rs`
  (`RLIMIT_NOFILE_DEFAULT` = 1024, `RLIMIT_NOFILE_HARD_DEFAULT` = 4096 — the
  kernel's *superseded* pair, disagreeing with both the kernel and
  `resource.rs`). Nothing outside that file's own unit test read them. Deleted,
  along with the test, which asserted a literal restated twenty lines above it
  and an inequality that held while both numbers were wrong.
- `prlimit` was routing through `getrlimit`/`setrlimit`, which hard-code pid 0
  — so its `pid` argument was silently discarded. It now calls the store
  helpers directly. This was a live bug, not tech debt, and it is fixed.
- `RLIMIT_NICE` and `RLIMIT_RTPRIO` default to `{0, 0}` and the kernel refuses
  every hard raise, so **no supported call sequence** can lift either ceiling —
  making the rlimit-instead-of-capability path unreachable on the target and
  forcing two `#[cfg(test)]` seeding doors into libc. Filed as
  `requests/b-a-a-hard-limit-that-starts-at-zero-can-never-rise.md`; delete
  both doors when it is answered.

**Found** 2026-08-16 by lane B while fixing
TD-POSIX-CAP-GATES-OMIT-LINUX-S-NON-CAPABILITY-ALTERNATIVE, on discovering
that `RLIMITS_INIT` and `kernel/src/proc/pcb.rs` are two independent
hand-written copies of Linux's `INIT_RLIMITS`. Re-measured 2026-08-21 while
filing the request.
