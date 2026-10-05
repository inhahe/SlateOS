### TD-POSIX-CAP-GATES-OMIT-LINUX-S-NON-CAPABILITY-ALTERNATIVE. A dozen libc capability gates test only the capability, where Linux's rule is "capability **or** something else" — so making them truthful would deny what the kernel permits — ✅ **FIXED (all 14 actionable sites); no longer blocks §312 step 3** — 2026-08-16

> **Status 2026-08-16, later the same day.** Design decision **§314** was taken
> to resolve *how* a conservative projection may be consumed, and applied:
>
> - **Class A — done.** The seven sites below no longer pre-empt the kernel.
>   `kill`/`killpg`/`sigqueue` and `chown`/`fchown`/`lchown` and
>   `sched_setaffinity` carry no libc capability test; `SYS_SIGNAL_SEND` and
>   `sys_fs_set_owner` (which already `require_cap_type(File, WRITE)`) decide.
> - **Class B, the ptrace family — done, differently.** `ptrace`,
>   `process_vm_readv`, `process_vm_writev` and `kcmp` are all stubs, so under
>   §314 rule 3 they now report `ENOSYS` unconditionally rather than inventing
>   an `EPERM` from a capability that could not have made them work.
> - **Class B, the three RLIMIT rows — done, as the correction below prescribed
>   and *not* under §314.** They were misclassified: their alternative was
>   evaluable all along, so they got Linux's whole predicate rather than a
>   removed gate. `RLIMIT_NICE`/`RLIMIT_RTPRIO` now seed to `{0, 0}` like
>   Linux's `INIT_RLIMITS` and our own `kernel/src/proc/pcb.rs`; `nice` and
>   `setpriority` route through a new `can_nice()` that mirrors Linux's
>   `is_nice_reduction(p, nice) || capable(CAP_SYS_NICE)`, evaluated against
>   the **clamped** target nice; `sched_setscheduler`'s RT arm consults
>   `RLIMIT_RTPRIO` first and falls back to the capability, with
>   `SCHED_DEADLINE` left capability-only. Because the corrected default is 0,
>   behaviour for every existing caller is byte-identical — what changed is
>   that raising the limit now means something, and `getrlimit` no longer
>   contradicts the gate.
>
> **This entry no longer blocks §312 step 3.** Step 3 remains blocked on its own
> prerequisites (fixture capability grants + QEMU + fail-closed `refresh()`),
> which are recorded under TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S, not here.


**In short.** Several of our libc functions ask "do you hold capability X?" and
refuse if not. Real Linux asks a two-sided question — "do you hold X, *or* is
this your own process / your own file / within your own limit?" — and permits
either way. Right now nobody notices, because on the target every process
believes it holds every capability, so the gates always pass. The moment §312
step 3 makes them answer honestly, they will start refusing ordinary
operations that the kernel itself is perfectly happy to allow. **This is
therefore a prerequisite for step 3, not a follow-up to it.**

**Why it is invisible today.** `has_capability()` reads libc's own stored
words, which on the target start out with every bit set
(TD-POSIX-CAPS-ARE-NOT-THE-KERNEL'S). A gate written as `if !has_capability(X)
{ EPERM }` is thus a no-op, and a missing "or same-owner" branch has no
observable consequence. Step 3 flips all 63 gates from no-ops to real tests on
the same day — so every one of these divergences becomes user-visible
simultaneously, and the ones already documented in-place become bugs at exactly
the same moment as the ones that are not.

**Class A — the alternative exists in our model and is simply not tested.**
These are the actionable ones: the information needed to write the missing
branch is available to libc today.

| Site | Function | Linux's actual predicate | What we test |
|---|---|---|---|
| `posix/src/signal.rs:1014` | `kill`, process-group form | same real/effective uid as target **or** `CAP_KILL` | `CAP_KILL` only |
| `posix/src/signal.rs:1037` | `kill`, `KillTarget::Other` | same uid **or** `CAP_KILL` | `CAP_KILL` only |
| `posix/src/signal.rs:1715` | `sigqueue` | same uid **or** `CAP_KILL` | `CAP_KILL` only |
| `posix/src/file.rs:2920` | `chown` | owner may chgrp to a group they belong to; only chown-to-another-user needs `CAP_CHOWN` | `CAP_CHOWN` only |
| `posix/src/file.rs:2960` | `fchown` | ditto | `CAP_CHOWN` only |
| `posix/src/file.rs:3000` | `lchown` | ditto | `CAP_CHOWN` only |
| `posix/src/sched.rs:419` | `sched_setaffinity` | `check_same_owner(p)` **or** `CAP_SYS_NICE` | `pid > 0` **and** `CAP_SYS_NICE` |

`sched.rs:419` is the sharpest of these because it *looks* like it handles the
alternative and does not: it uses `pid > 0` as a proxy for "not same owner",
but `pid > 0` includes `pid == getpid()`. Under a truthful gate a process could
not set its **own** affinity by explicit pid — only by passing `0`. The comment
above it names `check_same_owner` correctly, so the intent was right and the
proxy is the bug.

The `kill` row is the one with the widest blast radius, and it is already
written down as load-bearing elsewhere:
`services/ctest-jobctl/main.c`'s "Authority." paragraph explains that the
fixture's parent→child `kill(child, SIGCONT)` "needs no capability grant from
the kernel spawn: the kernel authorises a signal when the caller **is the
target's parent**, and our libc's own `CAP_KILL` gate reads the process
capability words, which start out as 'every capability held'." Under a truthful
gate the kernel would still permit that send and libc would refuse it — so
parent→child signalling breaks for every process that was not handed `CAP_KILL`.

**Class B — the alternative is a concept our model does not have yet.** These
are already documented in place, which is the right call; they are listed so
step 3 does not rediscover them as surprises. Each needs a decision (implement
the missing concept, or accept the divergence deliberately) rather than a code
fix.

| Site | Function | Missing alternative | Status |
|---|---|---|---|
| `posix/src/sched.rs:155` | `sched_setscheduler` → RT/DEADLINE | `RLIMIT_RTPRIO` | ✅ fixed — **misclassified**; got the whole predicate, see correction below |
| `posix/src/resource.rs:592` | `nice`, `inc < 0` | `RLIMIT_NICE`; its own comment says the test "collapses to a pure cap probe" | ✅ fixed — ditto |
| `posix/src/resource.rs:648` | `setpriority`, raising priority | `RLIMIT_NICE`, same | ✅ fixed — ditto |
| `posix/src/unistd.rs:3039` | `ptrace` attach | same-thread-group bypass — we do not track thread groups | ✅ fixed (§314 rule 3: stub → `ENOSYS`) |
| `posix/src/process.rs:3197` | `process_vm_readv` | `ptrace_may_access`: same-uid **and** dumpable | ✅ fixed (ditto) |
| `posix/src/process.rs:3239` | `process_vm_writev` | ditto | ✅ fixed (ditto) |
| `posix/src/process.rs:3348` | `kcmp` | ditto, twice (once per target pid) | ✅ fixed (ditto) |

**Correction to the three RLIMIT rows — they were never Class B, and they are
a live bug, not a divergence.** Putting them here rested on believing the
comments above them, which say the capability test is the whole rule "under the
default `RLIMIT_NICE = 0`" / "`RLIMIT_RTPRIO = 0`" and that we have no rlimit
model. Both halves are false:

1. **We do have the model.** `posix/src/resource.rs` keeps a real per-process
   `RlimitTable`; `getrlimit`/`setrlimit`/`prlimit` read and write it, and
   `setrlimit` already gates hard-limit raises on `CAP_SYS_RESOURCE`. So the
   alternative arm *is* locally evaluable — exactly as `mman.rs`'s
   `check_mlock_caps` already evaluates `RLIMIT_MEMLOCK`.
2. **The default is not 0, it is infinity.** `RLIMITS_INIT`
   (`posix/src/resource.rs:146`) seeds every slot to `RLIM_INFINITY` and then
   overrides only `STACK`, `NOFILE` and `CORE` — so `RLIMIT_NICE` and
   `RLIMIT_RTPRIO` come out `{INFINITY, INFINITY}`. Linux's own defaults are
   `{0, 0}` for both (`include/asm-generic/resource.h`). A ported program that
   reads `getrlimit(RLIMIT_NICE)` to decide whether to bother asking for
   priority is told it has unlimited headroom, and then denied by the gate.
   The test `default_limits_others_are_infinity`
   (`posix/src/resource.rs:920`) currently *asserts* the wrong values.

So the two facts contradict each other: the gates behave as if the limit were
0 while `getrlimit` reports infinity. That is observable **today**, with the
capability words still permissive, by any caller that consults `getrlimit` —
it does not wait for step 3.

*Proper fix (both halves; either alone leaves a lie in place):*

- Seed `RLIMIT_NICE` and `RLIMIT_RTPRIO` to `{0, 0}` in `RLIMITS_INIT`, matching
  Linux, and update `default_limits_others_are_infinity` accordingly.
- Give the three gates Linux's whole predicate, in the `check_mlock_caps`
  shape: `can_nice` is `20 - target_nice <= rlim_cur(RLIMIT_NICE) ||
  capable(CAP_SYS_NICE)`; the RT gate is `sched_priority <=
  rlim_cur(RLIMIT_RTPRIO) && rlim_cur != 0 || capable(CAP_SYS_NICE)`, with
  `SCHED_DEADLINE` capability-only (Linux permits no rlimit alternative for it).
  With the corrected defaults this preserves today's behaviour exactly, while
  making a raised limit actually mean something.

*Both halves landed 2026-08-16.* `RLIMITS_INIT` now overrides `RLIMIT_NICE` and
`RLIMIT_RTPRIO` to `{0, 0}` (with the Linux and `pcb.rs` citations in place);
`default_limits_others_are_infinity` no longer lists them and a new
`default_priority_limits_are_zero_like_linux_and_our_kernel` pins them.
`resource.rs` gained `can_nice()`, and `nice()` was reordered so the `[-20, 19]`
clamp happens **before** the gate — Linux evaluates `can_nice` against the
clamped value, so `nice(-100)` from a caller at nice 0 must be judged as a
request for nice −20, not for nice −100. `sched.rs` gained
`current_rtprio_limit()`; note it defaults to `0` on a failed `getrlimit`,
deliberately the opposite of `mman.rs`'s `current_memlock_limit()`, which
defaults to `u64::MAX`. Neither is "the safe default" in the abstract — each is
the *no-change* default for its own site: a zero `RLIMIT_MEMLOCK` is a hard
`EPERM`, so zero would break unprivileged `mlock`, whereas a zero
`RLIMIT_RTPRIO` merely falls back to `CAP_SYS_NICE`, which is where that
decision sat before today. Tests: 5 new in
`resource.rs` (raise permitted by rlimit without the capability; one step past
the ceiling still `EPERM`/`EACCES`; a zero limit leaves the capability as the
only route) and 5 new in `sched.rs` (the same three shapes for RT, plus
`SCHED_DEADLINE` ignoring a generous `RLIMIT_RTPRIO`, plus `EINVAL` still
beating the whole predicate).

**Class C — verified correct, for the record**, so a future survey does not
re-walk them: `posix/src/mman.rs:331` (`check_mlock_caps` — `CAP_IPC_LOCK`
**or** within `RLIMIT_MEMLOCK`, the pattern the Class A sites should copy),
`posix/src/unistd.rs:605`/`611` (`target == cur || has_capability(...)`),
`posix/src/sys_fsuid.rs:140`/`166` (`matches_cred || ...`),
`posix/src/stat.rs:376`/`430` (`CAP_MKNOD` fires only for `S_IFCHR`/`S_IFBLK`,
which is exactly Linux's `vfs_mknod` placement), `posix/src/socket.rs:1312`
(raw sockets) and `:1699` (`CAP_NET_BIND_SERVICE` for low ports) — both
capability-only in Linux too, `posix/src/time.rs:391`/`668` and
`posix/src/sys_timex.rs:398` (`CAP_SYS_TIME`), `posix/src/epoll.rs:1372`
(`CAP_WAKE_ALARM`), `posix/src/file.rs:5161` (`CAP_DAC_READ_SEARCH` for
`open_by_handle_at`), `posix/src/sys_io.rs:109`/`191` (`CAP_SYS_RAWIO`),
`posix/src/linux_module.rs` (`CAP_SYS_MODULE`).

**Proper fix.** For Class A, give each gate the shape
`if !permitted_by_ownership(...) && !has_capability(X) { EPERM }`, with the
ownership predicate written once per family rather than inlined per call site —
`kill`/`sigqueue` share one, the three `chown` variants share one. The uid
comparison has the credentials it needs already (`posix/src/unistd.rs` keeps
the real/effective ids); the *target's* uid is the part we do not have for
cross-process cases, and for those the honest predicate is "the kernel will
decide" — i.e. do not pre-empt with `EPERM` at all, make the call and report
what comes back. That is also the shape that survives step 3 unchanged, since
it stops libc from second-guessing an authority it does not hold.

For Class B, each row is a separate decision; none should be silently widened
into a Class A-style fix, because inventing an ownership test we cannot
actually evaluate would be worse than the current honest over-restriction.
That reasoning held for the ptrace family (resolved by §314 rule 3, since a
stub's honest answer is `ENOSYS`) and did **not** hold for the three RLIMIT
rows, whose alternative turned out to be evaluable — see the correction above.

**Do not fix this by weakening the gates.** Deleting the capability test, or
making `has_capability` return `true` while advisory, both "work" and both
destroy the property §312 was built for. There are also ~9 existing tests
(`process.rs:8079`, `:11455`, `:11849`, `:12295`, `:12679`,
`unistd.rs:5287`, `:9153`, `mman.rs:3162`, `sys_quota.rs:998`) that explicitly
drop a bit and assert `!has_capability(...)`, so an unconditionally-true gate
fails the suite immediately — which is the correct outcome and worth knowing
before trying it.

**Found** 2026-08-16 by lane B while scoping §312 step 3, immediately after
step 2 landed. **Fixed** 2026-08-16, same day, in two commits (§314 for the
eleven sites it governs; the whole-predicate treatment for the three RLIMIT
sites). Fixing it surfaced a separate divergence, logged next.
