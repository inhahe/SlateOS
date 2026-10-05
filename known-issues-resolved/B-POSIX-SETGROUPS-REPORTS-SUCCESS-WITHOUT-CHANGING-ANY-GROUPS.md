## B-POSIX-SETGROUPS-REPORTS-SUCCESS-WITHOUT-CHANGING-ANY-GROUPS (lane B, 2026-09-07)

**Status: FIXED 2026-09-12.** `setgroups` now calls
`SYS_PROCESS_SETGROUPS` (1067) and actually drops the groups; `chroot`, named
at the foot of this entry as the model, calls `SYS_PROCESS_CHROOT` (1068).
Moved to `known-issues-resolved/` on 2026-10-02, when `chroot (1)` -- now GNU
9.4's, ported, in place of the standalone that stubbed both calls -- became
the first program in the tree to drop groups through it.
Lane A landed both on **2026-09-07**, the day after they were asked, and the
request file has said `LANDED` in its own status line ever since. This entry
went on describing the block for five days because nothing re-read it after
the ground moved -- the third instance of that shape today, after the interval
timers and their request. The lesson is not to write more carefully; it is
that a document naming a blocker needs re-reading when the blocker is the
kind of thing someone else can clear without telling you twice.

On a host build both report `ENOSYS` through an explicit `cfg` arm rather
than through the syscall wrapper's sentinel. That is not cosmetic: the
sentinel is `HOST_ENOSYS`, which `errno::translate` reads as a *kernel* error
code rather than a negative errno and maps to `EIO`. Sixteen `setgroups`
tests and six `chroot` tests asserted `ENOSYS` and got 5.

**Status: FIXED 2026-09-07** (lane B), the same day it was filed. `setgroups`
now returns `-1`/`ENOSYS` after its validation instead of `0`. The thirteen
tests that asserted the old success assert the failure *and* the errno; the one
covering the privilege-drop idiom is now named
`test_setgroups_phase85_drop_all_groups_idiom_does_not_claim_success`, because
what it did before was lock in the precise behaviour that hands a caller a
privilege drop it never performed. The reasoning is
`design-decisions.md` §1004. The description below is kept in the past tense it
was written in, because how the decision unblocked is the useful part.

**In short:** the C library function a program calls to drop its extra group
memberships checks that it is allowed to, checks its arguments, and then returns
"done" without changing anything. A program that drops privileges this way is
told it worked and keeps every group it had.

**Where.** `posix/src/unistd.rs:948`, `setgroups`. The body gates on
`CAP_SETGID`, rejects `size > NGROUPS_MAX` with `EINVAL` and a NULL list with
`EFAULT` -- all correct and all Linux-faithful -- and then:

```rust
    0
}
```

No syscall is issued. Its own doc comment is explicit about the consequence,
which is what makes this worth an entry rather than a shrug:

> the classic `setgroups(0, NULL)` drop idiom that container runtimes,
> `su`/`sudo`, and the OpenSSH daemon all rely on

**Why it is not simply a stub.** A stub that returns `ENOSYS` leaves the caller
to decide what to do about a capability it does not have. This returns success,
so the caller proceeds *believing the drop happened*. For a privilege-dropping
function the difference is the whole thing: `setgroups(0, NULL)` followed by
`setuid(uid)` is a program deliberately shedding authority, and a silent success
means it sheds none of it while its own logic records that it did.

**Exposure today is latent.** Nothing in this tree calls it: `userspace/chroot`
has its own `enosys("setgroups")` stub and `userspace/su` mentions it only in a
comment. The ctest fixtures link libc and -- audited, see below -- do not
exercise it either. So this was a landmine rather than a live wound -- and it is the same shape as
`userspace/newgrp`'s `!password.is_empty()` group check found the same day: a
security decision that answers "yes" by default, sitting behind something else
that is missing.

**Why it cannot simply be fixed here.** The kernel *does* implement
`sys_setgroups` -- with real gating and a real credential mutation -- but only in
the Linux-ABI table (`kernel/src/syscall/linux.rs:3178`, handler at `:15729`),
which serves binaries running under `AbiMode::Linux`. There is no native syscall
number for it, so native libc has nothing to call. Asked for in
`requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`.

**The interim question, and why it stopped being one.** This entry originally
declined to choose between `0` and `ENOSYS`, on the grounds that switching
"converts a silent wrong answer into a loud failure in code that currently
'works', and the callers it would newly break are C programs linked against our
libc -- including the ctest fixtures the boot test runs -- **which nobody has
audited for it**."

That objection was checkable, and nobody had checked it. "Nobody has audited
this" is not a finding about the hazard; it is a finding about the auditing.
Audited 2026-09-07 across the whole tree and across all file types rather than
`*.rs` -- which is how the first pass missed C entirely: **there is no caller.**
The nine `services/ctest-*` fixtures do not mention `setgroups`. The only
tree-wide match outside `posix/src/unistd.rs` is a syscall-*number* table in
`find_gaps.py`, which is a lookup rather than a call. So the set of programs the
change could break is empty, the tradeoff that made this a judgment call does not
exist, and the "wait for the native syscall so the honest-failure window is zero"
recommendation was guarding nothing.

Worth recording as the day's recurring failure in a new costume. The five earlier
instances were *searches* that answered a narrower question than the one asked
and were reported at the width of the question. This one is the same error moved
one level up: a **decision** deferred on the strength of a hazard nobody had
measured, with the deferral stated at the width of a real tradeoff. An unaudited
objection is not a reason to wait; it is a reason to audit. The audit was one
command, and it had been available on every one of the days this sat open.

**Related, same file, opposite behaviour:** `posix::chroot`
(`unistd.rs:1964`) is in the identical position -- kernel handler exists, no
native number -- and ends in `ENOSYS`. It is the model for what `setgroups`
should do.
