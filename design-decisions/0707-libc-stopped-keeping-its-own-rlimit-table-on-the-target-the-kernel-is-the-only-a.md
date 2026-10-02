## 707. libc stopped keeping its own rlimit table: on the target the kernel is the only authority, and on the host libc's table *is* the authority and implements the kernel's rules — not Linux's

**Date:** 2026-08-29
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** A "resource limit" is a per-process cap — how many files it may
open, how big its stack may grow. Until now libc kept its own table of these
numbers *and* the kernel kept one, and the two had quietly grown apart on three
of their sixteen rows: a program asking libc "how many files may I open?" got
256, while the thing actually enforcing the cap said something else. The kernel
gained real syscalls for this (`SYS_RLIMIT_GET`/`SYS_RLIMIT_SET`, 557/558), so
libc's copy is now **deleted on the target** rather than merely unused, and
every call goes to the kernel. On the host — where there is no kernel to ask —
libc's table stays, but the policy it enforces was rewritten to be the
*kernel's* policy rather than Linux's, so a host test describes the system that
actually ships.

**The decision, in three parts.**

1. **One authority per build.** On `target_os = "none"`, `getrlimit`,
   `setrlimit` and `prlimit` are thin wrappers over 557/558 and libc states no
   policy rule whatsoever. The `static mut RLIMITS` shadow table, and the whole
   `limit_store` module, are `#[cfg(not(target_os = "none"))]` — they do not
   exist in the shipped binary. On the host, `limit_store` *is* the authority
   and restates the rules, because there is nothing else to ask.
2. **The host arm implements the kernel's rule, not Linux's.** The kernel
   refuses *every* raise of a hard limit, for every resource and every caller,
   with no capability that lifts the refusal. Linux instead lets
   `CAP_SYS_RESOURCE` raise a hard limit. The host arm previously implemented
   Linux's rule; it now implements the kernel's. Four Phase-179 tests that
   asserted "`CAP_SYS_RESOURCE` permits a raise" were **inverted and renamed**
   accordingly (`..._restoring_the_cap_still_does_not_permit_a_raise`,
   `..._sentinel_raise_refused_even_with_cap`).
3. **libc keeps its own null-pointer check, ahead of the syscall.** The kernel
   answers `InvalidArgument` for a null `arg2`; POSIX owes `EFAULT`. Checking
   in libc also preserves the two calls' *asymmetric* error orders —
   `getrlimit` reports `EINVAL` before `EFAULT`, `setrlimit` reports `EFAULT`
   before `EINVAL` — which mirrors Linux's own asymmetric kernel code and which
   a single check inside the kernel could not reproduce.

**Why.**

- *Two tables is not redundancy, it is disagreement waiting to be found.* The
  three drifted rows were not caught by any test, because every test asked the
  same table that answered wrongly. Deleting the copy is the only fix that
  cannot silently re-break: a table that still compiled would be a table
  someone could start reading again.
- *A shadow that agrees today is still wrong.* The alternative — keep libc's
  table and add a test asserting it matches the kernel's — cannot work across a
  build boundary. libc's host tests do not link the kernel.
- *Reproducing Linux is a specific layer's job, and this is not that layer.*
  `posix/src/linux_rlimit.rs` and the Linux syscall shims exist to look like
  Linux. `resource.rs` is our native surface and should look like *us*. Where
  the two differ, the shim keeps Linux's behaviour (e.g. `prlimit`'s
  `ESRCH`/`EPERM` distinction) and `resource.rs` does not.
- *An anti-oracle beats a faithful errno.* `prlimit` on a foreign pid answers
  `EPERM` even when that pid is dead, matching the kernel. Distinguishing
  `ESRCH` from `EPERM` would make `prlimit` a process-existence oracle anyone
  could call in a loop. `ESRCH` is kept only for the cases that leak nothing:
  a caller naming *itself* whose record is gone, and the negative-pid domain
  error.

**What it cost.** Two `#[cfg(test)]` back doors —
`resource::seed_rlimit_for_test` and `limit_store::seed` — which write a limit
row directly, bypassing `setrlimit`. They exist because `RLIMIT_NICE` and
`RLIMIT_RTPRIO` both default to `{0, 0}` and the kernel refuses every hard
raise, so **no supported call sequence** installs a non-zero ceiling for
either. Eight tests (four in `resource.rs`, four in `sched.rs`) needed one, and
all eight were failing on their *setup* line, not their assertion — routing
test setup through the policy under test makes the setup a test. This is filed
against lane A as
`requests/b-a-a-hard-limit-that-starts-at-zero-can-never-rise.md`; if the
policy changes, delete both doors.

**A deletion that came with it.** `linux_rlimit.rs` declared
`RLIMIT_STACK_DEFAULT`, `RLIMIT_CORE_DEFAULT`, `RLIMIT_NOFILE_DEFAULT` (1024)
and `RLIMIT_NOFILE_HARD_DEFAULT` (4096) — a *third* copy of the defaults, and
one holding the kernel's superseded numbers. Nothing outside that file's own
unit test read them. They are gone; the module now declares only resource ids
and `prlimit64` flags, which are ABI rather than policy. Its
`test_default_limits` went with them: it asserted `RLIMIT_STACK_DEFAULT == 8 *
1024 * 1024` (restating a literal twenty lines up — duplication, not
verification) and `NOFILE_DEFAULT < NOFILE_HARD_DEFAULT`, which held perfectly
while both numbers were wrong.

**Alternatives considered.**

- **Keep the target's table as a write-through cache of the kernel's.** Faster
  `getrlimit` — no syscall on a read. Rejected: `prlimit` can change another
  process's limits, so any cache is invalidatable from outside and we would be
  reintroducing exactly the drift this entry removes, for a saving on a call
  nothing makes in a hot loop.
- **Have the host arm keep Linux's `CAP_SYS_RESOURCE` rule.** Tempting because
  it keeps four tests passing unchanged and is what ported programs expect.
  Rejected: it makes the host suite describe a system we do not ship. The four
  tests passing was the *symptom*; a test suite that green-lights behaviour the
  target does not have is worse than one that fails honestly.
- **Push the null check into the kernel and drop libc's.** Rejected: `EFAULT`
  vs `InvalidArgument` is a POSIX obligation on the libc surface, and the
  per-call error *ordering* is not expressible from inside a single syscall
  that both calls share.
- **Route `prlimit` through `getrlimit`/`setrlimit`.** That is what it used to
  do, and it was a bug: those two hard-code pid 0, so `prlimit`'s pid argument
  was silently discarded. It now calls `store_get`/`store_set` directly.

**Where it lives.** `posix/src/resource.rs` — `rlimit_errno`, `store_get`,
`store_set`, `pid_is_self`, the gated `limit_store` module, `seed_rlimit_for_test`;
`posix/src/sched.rs` — `set_rtprio_limit`; `posix/src/linux_rlimit.rs` — the
deleted defaults and the note standing in for them. ABI reference:
`requests/a-b-native-rlimit-syscalls-landed.md`.

**How to reverse.** Reinstate `limit_store` on the target (drop the two `cfg`
gates) and point `store_get`/`store_set`'s target arm at it instead of
`syscall3`. Doing so re-opens the drift question, so if it is ever reversed,
the thing that must come with it is a mechanism — not a comment — keeping the
two tables in step.
