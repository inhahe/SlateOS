# B → A — supplementary groups, chroot and chdir: what is actually missing

**Filed:** 2026-09-06 by Lane B.
**Rewritten:** 2026-09-07, after lane A challenged the premise and it did not
survive measurement.
**Updated:** 2026-09-07 -- item 2 (lane B's own bug) is fixed; the ask of
lane A is unchanged.
**Status:** ✅ LANDED 2026-09-07 by lane A — `SYS_PROCESS_SETGROUPS` (1067) and `SYS_PROCESS_CHROOT` (1068) added as native syscalls. Both gate on `(Process, SET_CREDENTIALS)`. The PCB now has a `root_dir: Option<Vec<u8>>` field inherited across fork/exec. Self-tests added for both.

## The original request was wrong

It said: *"There is no `SYS_SETGROUPS`, no `SYS_CHDIR` and no `SYS_CHROOT` in
`kernel/src/syscall/`."* All three of those exist. Lane A caught the first,
which prompted me to measure the other two instead of asserting them again.

I had taken the claim from `userspace/chroot`'s own DESIGN GAP comment — a
comment I had already corrected once that morning, and which was still wrong
after I corrected it. Reading a stale comment and reporting it as a measurement
is the same error as reading a stale worktree; I made both on the same day.

## What is actually there, measured 2026-09-07 on `E:/…/os-lane-b`

| | Linux-ABI kernel handler | native libc (`posix`) | verdict |
|---|---|---|---|
| `setgroups` | **real** — `linux.rs:3178` → handler at `:15729`; EPERM if uid≠0, EINVAL over `NGROUPS_MAX`, mutates `new_creds.groups`, incl. the `size==0` clear | `unistd.rs:948` — checks `CAP_SETGID`, validates size and NULL… then **`ENOSYS`** (was `0`, having changed nothing) | **was lane B's bug; fixed** |
| `chdir` | real — `linux.rs:3438` | `unistd.rs:438` — resolves the path, stats it, real work | **nothing needed** |
| `chroot` | real — `linux.rs:3216` | `unistd.rs:1964` — validates, checks `CAP_SYS_CHROOT`, then **`ENOSYS`** | needs native wiring |

So the three items are in three different states, and only one of them is a
request of lane A at all.

## 1. `chdir` — withdrawn, nothing is needed

It works. `userspace/chroot` calling its own `enosys("chdir")` stub is a
userspace defect, and mine to fix.

## 2. `setgroups` — **lane B's, and the worst of the three**

`posix::setgroups` returns success without doing anything. That is not a stub in
the ordinary sense; it is a security function that reports it has acted when it
has not. Its own doc comment names the idiom it breaks:

> the classic `setgroups(0, NULL)` drop idiom that container runtimes, `su`/`sudo`,
> and the OpenSSH daemon all rely on

A caller performing that drop is told it succeeded and keeps every supplementary
group. Nothing in this tree calls it today — `userspace/chroot` has its own
ENOSYS stub and `su` only mentions it in a comment — so the exposure is latent,
but it is exactly the shape that bites the moment someone wires up a privilege
drop and trusts the return value.

**Fixed 2026-09-07, ahead of the syscall.** `posix::setgroups` now returns
`-1`/`ENOSYS` after its existing validation instead of `0`. That does not
implement anything -- it cannot, until item 3 lands -- but it stops the function
claiming it did. Reasoning in `design-decisions.md` §1004; the `known-issues.md`
entry is marked FIXED with the audit that unblocked it.

Worth one line on *why* it was fixed early, because it bears on the ordering
hazard below. The known-issues entry had deferred the choice between `0` and
`ENOSYS`, on the stated grounds that flipping it might break C programs linked
against our libc -- the `services/ctest-*` fixtures the boot test runs -- "which
nobody has audited for it". That was true and was a statement about the auditing,
not about the hazard. The audit is one command across all file types rather than
`*.rs`: no caller of `setgroups` exists anywhere in the tree, in any language.
With nothing to break, there was no tradeoff left to defer.

The remaining need for a native syscall number is unchanged and is item 3 below.

## 3. What is actually asked of lane A: a native path to what already exists

The kernel implements all three, but only in the **Linux-ABI** table
(`kernel/src/syscall/linux.rs`), which serves binaries running under
`AbiMode::Linux`. There is no native syscall number for them — `posix/src/syscall.rs`
has no `SYS_SETGROUPS`/`SYS_CHROOT` constant — so native libc has nothing to call,
which is why `posix::chroot` ends in `ENOSYS` -- and, since 2026-09-07,
`posix::setgroups` does too, rather than ending in a lie.

**The ask, therefore, is not "implement these".** It is: *expose the existing
implementations natively* — a syscall number and dispatch entry for `setgroups`
and `chroot`, reaching the handlers that are already written and already gated.
If there is a reason the native table deliberately omits them, that reason is the
answer and I will record it instead; I would rather know than have them added
because I asked.

Please treat the original framing as withdrawn. It asked for three things to be
built, two of which exist and one of which is my own bug.

## What this unblocks, unchanged from the original filing

`userspace/newgrp`/`sg` cannot switch groups; `userspace/chroot` refuses every
privilege operation; `login` and `su` authenticate correctly and then print
"would exec shell". Those remain true. What changed is that the road to them is
shorter than I said.

## The ordering hazard, which stands independently

Lane A's observation, and the reason this file is worth keeping even after the
correction: **a userspace defect that is inert only because a kernel feature is
missing becomes armed the moment lane A grants the request.**

That is not hypothetical here. `userspace/newgrp`'s group-password check was
`!password.is_empty()` — any single character admitted a non-member to any group
— and it was reachable only because nothing in `newgrp` attempts the group change
at all. It was fixed on 2026-09-07 (`2ed808e29`) *before* any of this landed, and
found only because lane A's message sent me looking at my own lane for things
advertised as working that are not.

`posix::setgroups` was the second instance of the same shape, found the same
way and fixed on 2026-09-07 before this request was granted: a function that
answered "yes, done" by default, inert only because nothing calls it yet, and
sitting directly behind the capability being asked for here.

The general form is worth stating: **when lane A grants a capability, the lane
that asked should re-audit what it had left unfinished on the assumption the
capability was absent.** The request is the trigger to re-check, not just to
resume.

A corollary, from the second instance: the re-audit is better done *before* the
grant than after. Both defects were found while the request was still open, and
in both cases the fix was cheap precisely because nothing depended on the broken
behaviour yet. Waiting for the grant would have meant fixing them under a live
caller.
