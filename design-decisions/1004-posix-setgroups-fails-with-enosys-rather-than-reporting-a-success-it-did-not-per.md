## 1004. `posix::setgroups` fails with `ENOSYS` rather than reporting a success it did not perform

**Date:** 2026-09-07
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `setgroups` is the C library call a program uses to give up its
extra group memberships -- the standard first move when a privileged program is
about to become an unprivileged one. Ours checked that the caller was permitted
to do it, checked the arguments, and then returned "done" without doing
anything. It now reports "not implemented" instead. Nothing in the system calls
it, so nothing behaves differently today; what changes is that the first thing
which *does* call it will be told the truth.

**What changed:** the last line of the function. `0` became
`errno::set_errno(errno::ENOSYS); -1`. The validation ahead of it is untouched
and still runs in Linux's order: `EPERM` without `CAP_SETGID`, `EINVAL` above
`NGROUPS_MAX`, `EFAULT` for a NULL list with a non-zero size.

**Why a false success is worse here than almost anywhere else.** A stub that
returns an empty answer understates what it knows, and a caller can cope with
that. This one asserted that a *mutation had occurred*. The idiom it exists to
serve is `setgroups(0, NULL)` followed by `setgid` and `setuid` -- a program
deliberately shedding authority, as container runtimes, `su`/`sudo` and sshd all
do. Under a false success, a caller that conscientiously checks the return value
is told the drop happened, keeps every supplementary group, and proceeds to lower
its uid believing it is now unprivileged. The check it wrote is precisely the
reason it stops looking. A security function that lies is worse than one that
refuses, because the refusal is survivable and the lie is load-bearing.

**The options:**

| Option | *What changes* | Verdict |
|---|---|---|
| Keep returning `0` | Nothing, until someone writes a privilege drop against it and it silently does not drop. | Rejected. This is the bug. |
| **`ENOSYS` now** | A caller is told the operation is unimplemented and can fail closed. | **Chosen.** |
| `ENOSYS` only when the native syscall lands | Identical end state, reached later, with the lie left in place meanwhile. | Superseded -- see below. |

The third option was this entry's own earlier recommendation, on the reasoning
that switching early "converts a silent wrong answer into a loud failure in code
that currently works", with the C programs linked against our libc -- the ctest
fixtures the boot test runs -- named as the population at risk, *and explicitly
noted as un-audited*. Auditing them cost one command and found no caller of
`setgroups` anywhere in the tree in any language. With the at-risk population
empty, the third option's only distinguishing feature was the duration of the
lie.

**Why [`getgroups`] was deliberately *not* changed to match.** It still succeeds
while reporting zero supplementary groups, and the asymmetry is the point.
`getgroups` reports **state**, and "no supplementary groups" is a coherent state
this libc can report honestly; `id(1)` is already written against exactly that
reading, synthesising a group list only when `getgroups` fails with `ENOSYS` and
never when it succeeds with none, which is gnulib's rule and not ours to break
casually. `setgroups` reports an **action**. A function that reports state may
report an empty one; a function that performs an action may not report having
performed it. Changing both "for consistency" would have traded a real
distinction for a superficial one and broken a working tool to do it.

**Against the choice, honestly:** this is a divergence from POSIX, where
`setgroups` has no `ENOSYS` in its specified error list, and a program that
checks the return value will now refuse to run where it previously ran. Judged
acceptable on two grounds. The divergence fails *safe* -- a privileged program
declines to continue rather than continuing while wrong -- and `ENOSYS` is
already this file's established spelling for "the kernel has the implementation
but native libc has no number for it": `posix::chroot` sits in the identical
position two thousand lines away and returns exactly this.

**When this reverses:** the moment `posix/src/syscall.rs` gains a native
`SYS_SETGROUPS`. The kernel already implements the call with real gating and a
real credential mutation, but only in the Linux-ABI table
(`kernel/src/syscall/linux.rs`), so native libc has nothing to invoke. Asked of
lane A in
`requests/b-a-no-syscall-sets-supplementary-groups-changes-root-or-changes-directory.md`.
When that number exists, this body becomes a real syscall and the `ENOSYS`
disappears without any of the above needing revisiting.

**If it is never revisited:** nothing degrades, and the failure is loud. The
standing cost is that supplementary-group manipulation remains unavailable to
native binaries -- which was already true, and was merely unsaid.
