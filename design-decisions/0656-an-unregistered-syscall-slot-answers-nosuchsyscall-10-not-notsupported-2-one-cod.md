## 656. An unregistered syscall slot answers `NoSuchSyscall` (-10), not `NotSupported` (-2) — one code cannot carry both "I have never heard of this" and "the answer is no"

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** When a program asks the kernel to do something and the kernel
answers "no", there are two very different reasons it might say that. Either
this kernel is too old to have the feature at all — in which case the program
should quietly do the thing the old way — or the feature exists and refused,
in which case the program must take no for an answer and stop. Until now the
kernel gave the *same* reply for both, so programs had to guess which one they
had been told. They now get two different replies, and there is nothing left to
guess.

### The concrete ambiguity

`dispatch.rs` returned `KernelError::NotSupported` (-2) for a syscall number
whose table slot is `None`. A *registered* handler also returns `NotSupported`
when the operation cannot be performed on this filesystem or device. Same code,
two facts, opposite correct responses:

| The caller sees | It might mean | Correct response |
|---|---|---|
| `-2` | this kernel predates the syscall | fall back to the older route |
| `-2` | the syscall ran and refused | honour it; do **not** fall back |

Falling back on a real refusal is the dangerous direction. The whole reason the
pinned `*at` syscalls (662-665) exist is that the path-based fallback is racy —
it re-walks a remembered path that an attacker may have re-pointed. Downgrading
a genuine refusal into that fallback reintroduces exactly the race the call was
built to close, silently, on the failure path where nobody is looking.

### Why it was fixed in dispatch rather than in the callers

Lane B had already built a workaround in `posix/src/file.rs`: a per-syscall
`AtomicBool` latch — *fall back on -2 only until the first non-`-2` answer has
been seen*. It is sound reasoning and it was the right thing for them to do
with the information the kernel was giving them. But it is a guess, and it is
wrong for precisely as long as no answer has arrived yet: the **first** call on
a filesystem that genuinely refuses is downgraded to the racy route, and that
is the one case a latch can never cover.

The deciding fact was that lane B was about to write the same latch a *second*
time, for `fstatat`. One caller compensating for an ambiguity is a workaround;
two is a defect in the thing being compensated for. And "latch until the first
non-`-2`" is subtle enough that the second copy would have differed from the
first in some detail that surfaced much later.

### Alternatives considered

**Leave it, and let each caller latch.** Cheapest, and no ABI change at all.
Rejected: it scales by copies of a subtle rule, and every copy has a first-call
hole that its author has to rediscover.

**Change handlers instead — keep -2 for "unregistered", give handlers a new
code.** Rejected on both correctness and size. The narrower, more specific fact
is "there is no handler here", and the narrow fact should get the new name; and
dozens of handlers return `NotSupported` today, so this version is a sweep
across the kernel instead of one arm in one function.

**Make `NoSuchSyscall` map to `EOPNOTSUPP` on the Linux ABI, so Linux callers
can see the distinction too.** Rejected outright: Linux spends one errno
(`ENOSYS`) on both facts, and inventing a divergence from Linux in order to
expose a distinction Linux does not have is how a compatibility layer stops
being one.

### The cost, stated plainly

This is an ABI change on the native path, and `error.rs` calls its codes stable.
A native-ABI caller that probes for `-2` to mean "missing syscall" now sees
`-10`. In practice there is one such caller and it is lane B's latch, which is
being retired anyway — but the honest accounting is that the change is not free
and it lands in someone else's tree.

Worse, it lands *before* the fix for it can. `posix/src/errno.rs` maps kernel
codes to errno with a catch-all of `EIO`, so between lane B merging `main` and
lane B adding two lines, an unregistered syscall reaches native-ABI userspace as
`EIO`. It used to arrive as `ENOTSUP` (that file maps `-2 → ENOTSUP`, not
`ENOSYS`), and once the two lines land it will arrive as `ENOSYS` — which is
both more correct and a second win, because it makes the distinction visible at
the errno level and not only in the raw code. But the intermediate state is
`EIO`, and `EIO` is not a value any caller acts on. Lane B's own tripwire test
(`kernel_error_codes_are_all_accounted_for`, which reads `kernel/src/error.rs`
and fails on any code it has not heard of) fires in the same merge that causes
the problem, and `requests/a-b-an-unregistered-syscall-now-answers-10-not-2.md`
gives the two lines verbatim. That pairing — the alarm and the instructions
arriving together — is the reason this was judged acceptable to land ahead of
the other half rather than blocking on it.

I had told lane B "nothing breaks if it never does" when I promised this work.
That was not quite true, and the request says so; the `EIO` window is the part
that does break.

### What is pinned so it cannot quietly regress

`test_dispatch_unimplemented` asserts three things, and the second and third
are the load-bearing ones:

1. an empty slot answers `NoSuchSyscall`;
2. the two codes are **not equal to each other**. This is not a restatement of
   (1): (1) compares the answer against the *symbol*, so it stays green through
   any renumbering of the variant — including a renumbering back to -2, which
   would silently restore the exact ambiguity this entry is about. (2) compares
   the numbers, which is the property that actually matters;
3. both codes still map to `ENOSYS` on the Linux ABI — so the arm in
   `linux_errno_for` that looks like a redundant duplicate cannot be tidied
   away by someone who has not read this entry. Its redundancy *is* the
   compatibility promise.

### Not done here, on purpose

`io_ring.rs`'s `execute_sqe` has the same shape — an unknown opcode returns
`NotSupported`, indistinguishable from a registered opcode's handler refusing —
and it is not changed in this commit. It is a separate return contract with its
own callers and deserves its own decision rather than being swept along with
this one. Logged in `known-issues.md` as
`A-IO-URING-UNKNOWN-OPCODE-IS-STILL-AMBIGUOUS`.
