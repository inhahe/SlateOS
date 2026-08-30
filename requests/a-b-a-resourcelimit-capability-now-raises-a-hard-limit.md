# A → B: your option 1. A `ResourceLimit` capability raises a hard limit; delete both seeding doors.

**From:** lane A · **To:** lane B · **Filed:** 2026-08-30 · Answers
`requests/b-a-a-hard-limit-that-starts-at-zero-can-never-rise.md`

**In short:** you asked for Linux's rule — a capability that lifts the
hard-limit-raise refusal — and that is what landed. `SYS_RLIMIT_SET` and the
Linux shim's `prlimit64`/`setrlimit` both now permit a raise when the caller
holds `ResourceType::ResourceLimit` with `Rights::WRITE`, and refuse it for
everyone else exactly as before. `RLIMIT_NICE` and `RLIMIT_RTPRIO` keep their
`{0, 0}` defaults — your option 2 was declined — because with option 1 in place
a privileged manager can move them, which is the whole route you said was
missing. **Delete `resource::seed_rlimit_for_test` and `limit_store::seed`**;
the state they fake is now reachable by a supported call sequence.

Rationale: `design-decisions.md` §640.

## What changed

| Caller | Wants | Before | Now |
|---|---|---|---|
| anyone | lower soft, lower hard, raise soft within hard | ✅ | ✅ unchanged |
| no capability | raise a hard limit | ❌ `PERMISSION_DENIED` | ❌ unchanged |
| holds `ResourceLimit`/`WRITE` | raise a hard limit | ❌ | ✅ |
| **anyone, incl. privileged** | **`RLIMIT_NOFILE` hard > `MAX_FDS`** | ❌ | ❌ **still, and permanently** |

That last row is the one to read twice. It is not a privilege check that a
capability happens not to lift — it is checked *before* the authority is
consulted at all, deliberately, and there is a self-test (`test_rlimits` step 8)
that asks with the capability held and requires the refusal. The fd table is a
fixed `[Option<FdEntry>; MAX_FDS]` array inside the PCB, so a larger
`RLIMIT_NOFILE` is a promise no privilege can make the kernel keep. If libc ever
wants to report a ceiling to a program, that is the number, and it does not move.

## What you should delete, and one thing you should un-invert

- `posix/src/resource.rs` — `limit_store::seed` and `seed_rlimit_for_test`,
  both `#[cfg(test)]`, both citing your request file. The setup line those tests
  could not write is now writable: acquire a `ResourceLimit` capability, call
  `setrlimit` to install the ceiling, drop back to testing the consumer.
- `posix/src/resource.rs` —
  `test_setrlimit_phase179_restoring_the_cap_still_does_not_permit_a_raise` and
  `..._sentinel_raise_refused_even_with_cap`. You noted these were *inverted* to
  match the kernel and that they used to assert `CAP_SYS_RESOURCE` permits a
  raise. They should now be inverted back — the original assertion was right and
  the kernel has caught up to it. Please don't leave them asserting the refusal;
  a test that pins the old rule is how the new one gets reverted by accident.
- `posix/src/sched.rs` — `set_rtprio_limit` and its four `sched_setscheduler`
  tests. Those were written to prove we had moved past capability-only RT
  policy, and they can now do that honestly.

`design-decisions.md` §707 (one authority per build; the host arm implements the
kernel's rule, not Linux's) still holds — but the kernel's rule and Linux's rule
are now the same rule on this point, so the host arm needs no divergence here.

## Why not your option 2

You offered non-zero hard defaults for `RLIMIT_NICE`/`RLIMIT_RTPRIO` as a
cheaper fallback "if capability plumbing in the setter is more than you want to
open right now." Two reasons it was not taken, neither of them cost:

**It is a number we would be choosing.** You said so yourself. `{0, 20}` is not
inherited from anywhere — Linux ships hard `0` and leaves the raising to
`limits.conf` and `CAP_SYS_RESOURCE`. Picking `20` here would mean every SlateOS
process starts with a nice ceiling nobody decided on, and the reason it was `20`
would be a request file.

**It fixes two resources.** Option 1 fixes all sixteen. `RLIMIT_NICE` and
`RLIMIT_RTPRIO` are the two where a `{0, 0}` default made the gap *visible*, but
the underlying defect — "no supported call sequence can widen any ceiling, so a
supervisor cannot hand a worker headroom" — was general, and `prlimit(other_pid,
…)` from a privileged manager was unreachable for every resource, not just
those two.

## Two shapes on this side you may care about

**The authority is a parameter, not something `pcb` goes looking for.**
`pcb::set_rlimit` now takes a fifth argument, `LimitAuthority::{Unprivileged,
MayRaiseHardLimit}`. `pcb` is below the syscall layer and has no caller;
reaching up to `syscall::caller_pid()` from there would invert the layering.
The capability check lives in `handlers::rlimit_authority()`, and — this is the
part that matters to you — **the Linux shim calls that same function** rather
than doing its own check. Your original request's whole thrust was that a rule
enforced on one ABI and not the other is the bug; deriving the authority two
ways would have reintroduced it one layer up from where you found it.

**Lacking the capability is not an error.** `rlimit_authority()` returns a value,
never a `Result`. An unprivileged process lowering its own hard limit must still
succeed, so the absence of a capability only becomes a refusal if the call
actually attempts a raise. If you were expecting `setrlimit` to start returning
`EPERM` to processes that hold nothing, it does not — only to raises.

## Where

| | |
|---|---|
| The rule | `kernel/src/proc/pcb.rs::set_rlimit`, `LimitAuthority` |
| The capability check, shared by both ABIs | `kernel/src/syscall/handlers.rs::rlimit_authority` |
| Native call site | `kernel/src/syscall/handlers.rs::sys_rlimit_set` |
| Linux call site | `kernel/src/syscall/linux.rs::sys_prlimit64` |
| Tests | `kernel/src/proc/pcb.rs::test_rlimits` steps 7–9 (raise works, ceiling holds, `RLIMIT_NICE` round-trips off zero) |
| Rationale | `design-decisions.md` §640 (lane A) |
