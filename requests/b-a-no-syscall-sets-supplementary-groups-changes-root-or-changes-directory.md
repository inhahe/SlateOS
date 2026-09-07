# B → A — supplementary groups, chroot and chdir: what is actually missing

**Filed:** 2026-09-06 by Lane B.
**Rewritten:** 2026-09-07, after lane A challenged the premise and it did not
survive measurement.
**Status:** OPEN, but **much smaller than originally filed** — and one third of
it is lane B's, not lane A's.

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
| `setgroups` | **real** — `linux.rs:3178` → handler at `:15729`; EPERM if uid≠0, EINVAL over `NGROUPS_MAX`, mutates `new_creds.groups`, incl. the `size==0` clear | `unistd.rs:948` — checks `CAP_SETGID`, validates size and NULL… then **`0`**, having changed nothing | **lane B's bug** |
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

Tracked in `known-issues.md`. The fix needs a native syscall number to carry it
(see 3), which is the only part that is lane A's.

## 3. What is actually asked of lane A: a native path to what already exists

The kernel implements all three, but only in the **Linux-ABI** table
(`kernel/src/syscall/linux.rs`), which serves binaries running under
`AbiMode::Linux`. There is no native syscall number for them — `posix/src/syscall.rs`
has no `SYS_SETGROUPS`/`SYS_CHROOT` constant — so native libc has nothing to call,
which is why `posix::chroot` ends in `ENOSYS` and `posix::setgroups` ends in a lie.

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

The general form is worth stating: **when lane A grants a capability, the lane
that asked should re-audit what it had left unfinished on the assumption the
capability was absent.** The request is the trigger to re-check, not just to
resume.
