# B → A — a hard limit that starts at zero can never rise, so `RLIMIT_NICE` and `RLIMIT_RTPRIO` are decorative

**Filed:** 2026-08-29 by Lane B. **Action needed:** a policy decision plus a
small change in `SYS_RLIMIT_SET` (557/558's setter). Nothing is *broken* — no
program misbehaves — but two of the sixteen resources cannot do the one job
they exist to do, and libc now has two `#[cfg(test)]` back doors that exist
solely to work around it.

## In short

A "hard limit" is the ceiling a process may not raise its own soft limit past.
Your `SYS_RLIMIT_SET` refuses **every** raise of a hard limit, for every
resource and every caller, with no capability that lifts the refusal. That is a
defensible rule on its own. But two resources — `RLIMIT_NICE` (how far a
process may lower its own nice value, i.e. make itself more important) and
`RLIMIT_RTPRIO` (the highest real-time priority it may ask for) — **start at
`{0, 0}`**. A ceiling of zero that can never be raised is not a ceiling; it is
a permanent "no."

The practical effect: the only way for a process to get a nice boost or a
real-time policy on this OS is to hold `CAP_SYS_NICE`. The rlimit route — hand
one process a bit of headroom *without* handing it a capability that lets it
renice anything on the system — is unreachable. That route is the entire
reason `RLIMIT_NICE` and `RLIMIT_RTPRIO` exist in POSIX/Linux.

## How I found it

Rewiring `posix/src/resource.rs` onto 557/558 (per
`requests/a-b-native-rlimit-syscalls-landed.md`) deleted libc's shadow limit
table, so libc's tests started exercising the kernel's real policy instead of
their own copy of it. Eight tests failed at once — four in `resource.rs`, four
in `sched.rs` — and every one of them failed on its *setup* line rather than
its assertion: each was calling `setrlimit` to install a non-zero
`RLIMIT_NICE`/`RLIMIT_RTPRIO` ceiling before testing the consumer of that
ceiling. No supported call sequence installs one.

I fixed the tests by seeding the row directly, out of band, through two
`#[cfg(test)]` doors (`resource::seed_rlimit_for_test` and, behind it,
`limit_store::seed`). Both carry a comment pointing at this file. If the policy
below changes, delete them — a test-only door around the policy under test is
not something to keep once it is unnecessary.

## The rule, precisely

Today, in the setter:

| Caller | Wants | Result |
|---|---|---|
| anyone | lower own soft limit | ✅ allowed |
| anyone | lower own hard limit | ✅ allowed (one-way door, correct) |
| anyone | raise soft limit up to existing hard limit | ✅ allowed |
| anyone | raise hard limit **at all** | ❌ `PERMISSION_DENIED`, unconditionally |

Linux's rule differs on the last row: a process holding `CAP_SYS_RESOURCE`
(the "may exceed resource limits" capability) *may* raise a hard limit. That is
what makes a zero default survivable there — something privileged sets the
ceiling for you at process-creation or via `prlimit` from outside, and you live
under it.

## What would fix it — three options, in my order of preference

**1. Let `CAP_SYS_RESOURCE` raise a hard limit.** *What changes:* a process
holding that capability can widen its own ceilings; everyone else still cannot.
This is Linux's rule, it is the one every ported program already expects, and
it makes `prlimit(other_pid, …)` from a privileged manager work — which is the
normal way a supervisor hands a worker headroom. Cost: `CAP_SYS_RESOURCE`
becomes a genuinely powerful capability, which it already is elsewhere.

**2. Give `RLIMIT_NICE` and `RLIMIT_RTPRIO` non-zero hard defaults** (Linux
ships `RLIMIT_NICE` hard = 0 too, but many distributions raise it via
`limits.conf`; a hard default of e.g. `{0, 20}` / `{0, 20}` would work).
*What changes:* a fresh process still has soft 0 — no boost by default — but
can raise its soft limit up to the hard default without any capability, and
lower it back down again. Cheaper than option 1 and needs no capability work.
It does not help the other fourteen resources, and it is a number we would be
choosing rather than inheriting.

**3. Leave it, and say so.** *What changes:* nothing observable; we document
that this OS deliberately has no rlimit-based priority path and
`CAP_SYS_NICE` is the only route. Honest, and defensible for a capability-first
system that never wanted the Unix "privileged uid may exceed limits" model in
the first place. But then the two resources should arguably report as
unsupported rather than silently accepting `setrlimit` calls that can only
ever be no-ops or lowerings of zero.

I recommend **1**, with **2** as a fallback if capability plumbing in the
setter is more than you want to open right now. **3** is a real answer and I
would not argue with it, but it should be a *decision*, recorded in
`design-decisions.md`, rather than the current state of affairs — which is
option 3 by accident.

## If it is never answered

Nothing degrades. The status quo is safe: processes without `CAP_SYS_NICE`
simply cannot raise their priority, which fails closed. The cost is the two
test-only seeding doors in libc staying where they are, and `RLIMIT_NICE` /
`RLIMIT_RTPRIO` remaining rows in a table that no call sequence can move.

## Where it bites, by file

- `kernel` — `SYS_RLIMIT_SET` (syscall 558), the hard-limit-raise refusal and
  the `{0, 0}` defaults for resources 13 (`RLIMIT_NICE`) and 14
  (`RLIMIT_RTPRIO`).
- `posix/src/resource.rs` — `limit_store::seed` and `seed_rlimit_for_test`,
  both `#[cfg(test)]`, both citing this file. Also
  `test_setrlimit_phase179_restoring_the_cap_still_does_not_permit_a_raise`
  and `..._sentinel_raise_refused_even_with_cap`, which were *inverted* to
  match the kernel: they used to assert `CAP_SYS_RESOURCE` permits a raise.
- `posix/src/sched.rs` — `set_rtprio_limit`, shared by four
  `sched_setscheduler` tests that were written specifically to prove we had
  moved past capability-only RT policy. They now pass, but only because the
  seeder puts them in a state the running system cannot reach.
- `design-decisions.md` §707 records the libc-side half of this (one authority
  per build; the host arm implements the kernel's rule, not Linux's).
