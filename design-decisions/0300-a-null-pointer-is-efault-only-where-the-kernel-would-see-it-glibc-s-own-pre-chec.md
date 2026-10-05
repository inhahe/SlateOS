## §300 — A NULL pointer is `EFAULT` only where the kernel would see it; glibc's own pre-checks keep their `EINVAL`

**Date:** 2026-08-13
**Decided by:** Claude (autonomous)

### The problem

`posix/` had a blanket rule, applied by an undocumented sweep (recorded only
obliquely in `todo.txt` as "Phase 215"), that any NULL pointer argument to a
libc entry point is reported as `EFAULT`. A follow-up phase then "fixed" the
tests that disagreed with the sweep, cementing the behaviour with green tests.

The rule is right most of the time and wrong in a specific, identifiable class
of cases. It is right whenever the pointer is forwarded to a syscall: Linux
validates user pointers in `copy_from_user`/`copy_to_user`, a NULL page is not
mapped, and the kernel returns `EFAULT`. So `open(NULL, …)`, `stat(NULL, …)`,
`getxattr(NULL, …)` all correctly produce `EFAULT`, and our sweep matched
Linux there.

It is wrong whenever the function is implemented *entirely in userspace* and
glibc rejects the NULL argument with an explicit early return, before the
pointer is ever dereferenced or handed to the kernel. Those functions produce
`EINVAL`, and no syscall is issued at all, so there is no kernel to produce an
`EFAULT`. Three of ours had drifted:

| Function | glibc source | glibc errno | ours (before) |
|---|---|---|---|
| `realpath(NULL, buf)` | `stdlib/canonicalize.c:195` — `if (name == NULL) { __set_errno (EINVAL); return NULL; }`, citing SUSv2 | `EINVAL` | `EFAULT` |
| `canonicalize_file_name(NULL)` | `stdlib/canonicalize.c:460` — `return __realpath (name, NULL);` | `EINVAL` | `EFAULT` |
| `__realpath_chk(NULL, …)` | forwards to `realpath` | `EINVAL` | `EFAULT` |

A fourth candidate, `ptsname_r(fd, NULL, n)`, turned out to be a different bug
and is treated separately below.

### The `ptsname_r` case, and why it is not the same bug

`ptsname_r` was the function that started this: its *doc comment* said
`buf == NULL -> EINVAL` while its body said `EFAULT`, and both `ptsname_r(3)`
and POSIX.1-2024 document `EINVAL` for a NULL `buf`. The obvious conclusion —
that the sweep had overwritten a correct `EINVAL` — was wrong, and it was wrong
in a way that only reading the source could show.

glibc 2.39's `__ptsname_r` (`sysdeps/unix/sysv/linux/ptsname.c`) has **no NULL
check at all**. It opens with `__ioctl (fd, TIOCGPTN, &ptyno)` and returns that
call's errno on failure, touching `buf` only afterwards:

```c
int
__ptsname_r (int fd, char *buf, size_t buflen)
{
  int save_errno = errno;
  unsigned int ptyno;

  if (__ioctl (fd, TIOCGPTN, &ptyno) == 0)
    { …  memcpy (__stpcpy (buf, devpts), p, …);  }
  else
    /* Bad file descriptor, or not a ptmx descriptor.  */
    return errno;
```

So on Linux the *descriptor verdict outranks the buffer*: a bad fd gives
`EBADF` and a live non-PTY fd gives `ENOTTY`, with a NULL `buf` making no
difference to either. `EINVAL` is reachable only on a real ptmx fd — where
glibc does not return it but segfaults in `__stpcpy (buf, devpts)`. (musl
clamps the length to 0 and yields `ERANGE`.) The documented `EINVAL` is a
survival from an older glibc whose check the TIOCGPTN fast path removed.

Our implementation checked `buf` *first*, so it answered a NULL-buf caller with
an argument complaint where Linux answers with the fd's errno — the check being
`EFAULT` rather than `EINVAL` was the lesser half of the divergence. The
ordering is now glibc's: fd checks, then the PTY-master verdict (always `ENOTTY`
here, since `posix_openpt` returns `ENOSYS`), and `buf` is not examined at all.
A comment marks where the NULL check belongs once PTY support lands, and says
to use `EINVAL` there — following the man page and POSIX rather than copying a
segfault.

### Decision

The policy is stated positively rather than as a blanket:

> **`EFAULT` is what the kernel reports for a pointer it cannot access.** Use
> it in `posix/` when the argument would reach a syscall. When a function is
> implemented in userspace and upstream glibc rejects the argument itself with
> an early return, mirror glibc's errno — which for a NULL argument is
> `EINVAL`, an argument-domain error, not a fault. **Read the glibc source
> before deciding which case you are in**; a man page is not sufficient
> evidence, as `ptsname_r` shows.

The three `realpath`-family functions now return `EINVAL`, each with a comment
naming the glibc translation unit and quoting its check, so the next sweep has
something to read before it changes them back. `ptsname_r` keeps no NULL check
and its tests now assert the fd's errno; `test_ptsname_r_null_buf_efault`
became `test_ptsname_r_null_buf_still_reports_the_fd_verdict`, and
`test_ptsname_r_null_buf_beats_bad_fd` became `…_bad_fd_beats_null_buf` — the
test names carry the ordering so it cannot be silently inverted again.

To make this checkable rather than recalled, glibc 2.39's source now sits
beside bash's at `D:\refsrc\glibc-2.39` (shallow clone of the `glibc-2.39` tag
from the `bminor/glibc` GitHub mirror; `sourceware.org` answered 429). Every
citation in this entry was verified against it — including the two that a
from-memory first draft of this entry got wrong.

### Alternatives considered

**Keep the blanket `EFAULT`.** Simpler to state, and uniform: one rule, no
per-function research, no risk of a half-applied sweep. But it is wrong in a
way that is visible to real programs. A portable caller that branches on
`EINVAL` — the errno every `realpath(3)` man page shows for a NULL path, and
the one glibc actually returns — takes the wrong branch on SlateOS. The whole
point of the compatibility layer is that unmodified Linux binaries behave as
they do on Linux; "uniform but divergent" is the failure mode this project
exists to avoid, and it is exactly the "correct-but-naive" pattern CLAUDE.md
warns about.

**Make it configurable / strict-vs-lenient mode.** Rejected as unjustified
machinery: there is a single correct answer per function (whatever glibc
does), so there is nothing for a knob to select.

**Follow the man page rather than the source.** Cheaper, and it is what the
first draft of this change did. It produces exactly the wrong answer for
`ptsname_r`, whose documented `EINVAL` no glibc has returned since the TIOCGPTN
fast path landed. Man pages describe the contract an implementation once had;
the compatibility target is the contract binaries are actually built against.
Where the two differ and neither is dangerous, the source wins; where the
source's behaviour is a crash (as it is here for a real ptmx fd), take the
documented errno instead — and say so at the site.

### Consequences

- The rule now requires *reading glibc* for each NULL check rather than
  applying a regex. That is the cost, and it is the right cost — it is the
  same discipline the rest of the compatibility layer already uses.
- Argument-validation *ordering* is part of the ABI too, not just the errno
  values. `ptsname_r`'s bug was not really the choice of `EFAULT` over
  `EINVAL`; it was checking `buf` before the descriptor, which changes the
  answer for every NULL-buf caller regardless of which errno the check sets.
  The audit below has to look at order, not only at the constant.
- A cheap mechanical form of the audit exists and was run the same day:
  search *glibc* for `if (x == NULL) { __set_errno (E…) … }` and intersect the
  results with our entry points, rather than reading our 289 sites one by one.
  The one trap is that many hits — most of `io/*.c` and `posix/*.c` — are the
  generic `ENOSYS` stubs that no Linux build uses; filter them by looking for
  `stub_warning`/`ENOSYS` in the same file. That sweep found nine further
  divergences (the five sigsetops, `closedir`, and the three `cfset*speed`
  functions), which are listed in the known-issues entry. The `pthread.rs`
  cluster is explicitly *not* answerable this way: glibc has no checks there
  to copy, so what we return in place of its segfault is a decision, not a
  lookup, and it is left for a separate pass.
- A future audit should sweep the remaining `is_null() -> EFAULT` sites in
  `posix/` and classify each as syscall-forwarding (keep `EFAULT`) or
  userspace-pre-check (switch to glibc's errno). The `xattr`, `stat`, and
  `file` NULL-path checks spot-checked during this change are all
  syscall-forwarding and correctly `EFAULT`; the audit is about coverage, not
  a suspicion that more are wrong.
- Tests that encode an errno now carry a one-line citation of the upstream
  source. Without it, a test is only evidence that the code and the test were
  written by the same pass — which is precisely what happened here.
