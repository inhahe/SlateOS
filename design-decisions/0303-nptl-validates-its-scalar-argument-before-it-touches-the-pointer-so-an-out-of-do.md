## §303 — NPTL validates its scalar argument before it touches the pointer, so an out-of-domain scalar outranks a NULL pointer

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)
**Lane:** B

### The problem

§300 established the rule for *which* errno a NULL pointer produces, and its
Consequences section asked for a per-function audit of the ~330 remaining
`is_null() -> EFAULT` sites. `known-issues.md`'s
`D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT` singled out
`posix/src/pthread.rs` (47 sites) as "the one that still needs a decision
rather than a lookup", because glibc's NPTL contains **no NULL checks at
all** — every one of these functions simply dereferences and faults. There is
no upstream errno to copy, so the audit appeared to be blocked on a judgement
call about what to substitute.

That framing was wrong. §300's own closing instruction says to "check the
*order* of the validations too, not only the constant — that is what
`ptsname_r` actually got wrong", and ordering is exactly what `pthread.rs` had
wrong, nine times over. The question is answerable from the source without any
judgement at all.

### What the source says

NPTL has one consistent shape. Every entry point that takes both an attribute
pointer and a scalar validates the **scalar first**, returns `EINVAL`/`ERANGE`
from that test, and dereferences the pointer only afterwards:

| Function | glibc source | first test |
|---|---|---|
| `pthread_attr_setstacksize` | `nptl/pthread_attr_setstacksize.c` | `int ret = check_stacksize_attr (stacksize); if (ret) return ret;` |
| `pthread_attr_setstack` | `nptl/pthread_attr_setstack.c` | same `check_stacksize_attr` prologue |
| `pthread_attr_setdetachstate` | `nptl/pthread_attr_setdetachstate.c` | `/* Catch invalid values.  */` on `detachstate` |
| `pthread_mutexattr_settype` | `nptl/pthread_mutexattr_settype.c` | `if (kind < PTHREAD_MUTEX_NORMAL \|\| kind > PTHREAD_MUTEX_ERRORCHECK) return EINVAL;` |
| `pthread_condattr_setclock` | `nptl/pthread_condattr_setclock.c` | full `clock_id` validation |
| `pthread_rwlockattr_setpshared` | `nptl/pthread_rwlockattr_setpshared.c` | `futex_supports_pshared (pshared)` |
| `pthread_barrier_init` | `nptl/pthread_barrier_init.c` | `if (count == 0 \|\| count >= BARRIER_IN_THRESHOLD) return EINVAL;` |
| `pthread_getname_np` | `nptl/pthread_getname.c` | `if (len < TASK_COMM_LEN) return ERANGE;` |

So on Linux, a call that is bad in *both* ways — a NULL pointer **and** an
out-of-domain scalar — returns the scalar's errno, and never faults. Our code
checked the pointer first in every one of these, so it returned `EFAULT` where
Linux returns `EINVAL` or `ERANGE`.

The affinity pair is the interesting case, because the two halves order their
checks *oppositely*, and the asymmetry is the kernel's rather than glibc's
(both glibc wrappers are bare `INTERNAL_SYSCALL_CALL`s):

- `sched_getaffinity` (`kernel/sched/core.c:8506-8509`) has two `EINVAL`
  tests — `len < cpumask_size()` and `len & (sizeof (unsigned long) - 1)` —
  and both sit **above** the `copy_to_user` that would fault. Length wins.
- `sched_setaffinity`'s `get_user_cpu_mask` (`kernel/sched/core.c:8429`) has
  **no** size rejection at all; a short `len` merely leaves the top of the
  mask clear. `copy_from_user` runs first, so the NULL pointer wins. (A short
  `len` still reaches `EINVAL` there, but the long way round: the cleared mask
  is empty, and `__sched_setaffinity` rejects an empty CPU set.)

### The decision

1. **Order our checks the way NPTL and the kernel order theirs.** Scalar
   first in the eight table rows above and in `pthread_getaffinity_np`;
   pointer first in `pthread_setaffinity_np` alone.

2. **Keep `EFAULT` for the pointer itself**, when every scalar is valid. glibc
   segfaults there, which is not an errno we can return, so a substitute is
   unavoidable. `EFAULT` is the right substitute because on the two paths where
   the pointer genuinely does reach a syscall — the affinity pair, and
   `pthread_setname_np`'s `prctl` — `EFAULT` is precisely what Linux gives; a
   caller who checks for it is never surprised by the ones that don't syscall.

The alternative to (2) was to fault as glibc does, on the grounds that
returning *any* errno invents behaviour no Linux program can observe. Rejected:
we are a hosted libc whose callers include our own test suite and our own
userland, and a diagnosable errno is worth more than bug-compatibility with a
segfault. The alternative to (1) was to leave the order alone and document the
divergence. Rejected because the order is part of the ABI — a program that
passes a stack size of 8192 and a null attribute expects `EINVAL`, and getting
`EFAULT` sends it down an error path that was written for a different bug.

### Three latent bugs the audit turned up

Reordering the checks was not the whole of it. Reading the glibc source
line-by-line exposed three cases where our *constants* were wrong too:

- **`PTHREAD_STACK_MIN` was hardcoded as `4096`** in `pthread_attr_setstacksize`
  and `pthread_attr_setstack`, while the crate's own
  `linux_pthread_key_types::PTHREAD_STACK_MIN` is `16384` (glibc's
  `check_stacksize_attr`, `sysdeps/nptl/pthreadP.h:704`, on x86-64). We
  accepted three stack sizes glibc rejects, and contradicted ourselves. The
  setters now use the shared constant.

- **`pthread_getname_np` applied `ERANGE` only when the stored name did not
  fit.** glibc compares `len` against `TASK_COMM_LEN` (16) *unconditionally*,
  never against the name's actual length, so `pthread_getname_np(t, buf, 4)`
  is `ERANGE` even for a two-character name — a call we used to accept. With
  the entry test corrected, the second length test becomes dead and is gone;
  glibc has no second test either.

- **`pthread_barrier_init` accepted `count == u32::MAX`.** That was a latent
  hang: our arrival counter is an `AtomicI32`, so a count above `i32::MAX`
  can never be reached and every waiter would block forever. glibc caps
  `count` at `BARRIER_IN_THRESHOLD` (`UINT_MAX / 2`,
  `sysdeps/nptl/internaltypes.h:119`) for its own reset protocol; we adopt the
  same bound for our own reason, and the constant is now named in the code.

And one errno was simply the wrong constant:
**`pthread_rwlockattr_setpshared` returned `ENOTSUP` for any non-`PRIVATE`
value**, conflating "we don't support cross-process rwlocks" with "that isn't
a pshared value". glibc gates on `futex_supports_pshared`
(`sysdeps/nptl/futex-internal.h:102`), which accepts *both* POSIX values and
returns `EINVAL` for anything else. `ENOTSUP` is our own verdict on
`PTHREAD_PROCESS_SHARED` — which glibc accepts and we do not — and must not
swallow out-of-domain values with it.

### Consequences

Nine functions changed behaviour, all in the direction of matching Linux.
Callers that passed a bad scalar *and* a null pointer see a different errno;
callers that passed only one of the two are unaffected, except for the three
constant fixes above, which reject inputs we previously accepted (an 8 KiB
stack, a short `getname` buffer, a `u32::MAX` barrier). Each is a case where
the old acceptance was itself the bug.

Every changed site carries the glibc (or kernel) file name and the actual
check in a comment, at both the code and the test, per §300's rule that "a test
that encodes an errno with no upstream citation is only evidence that the code
and the test were written by the same pass."

The audit's remaining clusters — `file.rs` (28), `spawn.rs` (16), `socket.rs`
(15), `unistd.rs` (13), `xattr.rs` (11) — are unaffected by this entry's
reasoning, which is specific to NPTL's shape. They are ordinary §300 lookups:
does the pointer reach a syscall or not.
