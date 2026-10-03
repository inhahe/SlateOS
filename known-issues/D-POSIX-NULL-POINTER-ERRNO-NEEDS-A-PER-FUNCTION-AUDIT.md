### [B] D-POSIX-NULL-POINTER-ERRNO-NEEDS-A-PER-FUNCTION-AUDIT. The rest of `posix/`'s `is_null() -> EFAULT` checks have not been classified against glibc — 2026-08-13 — OPEN (tech debt)
**Status:** OPEN — tech debt in `posix/**`, which lane D owns since the six-lane split of 2026-09-22; the entry keeps lane B's tag as the record of who found it.

**Where:** `posix/src/**` — every `if p.is_null() { set_errno(EFAULT); … }`.

**What.** An undocumented sweep (referred to in `todo.txt` only as "Phase 215")
applied a blanket rule that any NULL pointer argument to a `posix/` entry point
yields `EFAULT`. A following phase then edited the tests that disagreed with the
sweep so they asserted `EFAULT` too, which made the divergence invisible: code
and test agreed, and both were wrong.

The blanket rule is correct wherever the pointer is forwarded to a syscall —
Linux's `copy_from_user` rejects the unmapped NULL page with `EFAULT`, so
`open(NULL, …)`, `stat(NULL, …)`, `getxattr(NULL, …)` and friends match Linux.
It is wrong wherever glibc implements the function in userspace and rejects the
NULL argument itself with an early return, because no syscall is ever issued and
glibc's own errno is `EINVAL`.

Three such functions were found and fixed on 2026-08-13, verified line-by-line
against glibc 2.39's source (see `design-decisions.md` §300):

- `realpath(NULL, buf)` → `EINVAL` (`posix/src/unistd.rs`)
- `canonicalize_file_name(NULL)` → `EINVAL` (`posix/src/unistd.rs`)
- `__realpath_chk(NULL, …)` → `EINVAL` (`posix/src/file.rs`, delegates)

A fourth candidate, `ptsname_r`, turned out to be a different and worse bug: it
checked `buf` *before* the descriptor, whereas glibc's `__ptsname_r` issues
`ioctl(fd, TIOCGPTN)` first and never examines `buf` when that fails. The
ordering — not the errno constant — was the divergence. It now matches glibc,
and the function has no NULL check at all. Full analysis in §300.

**Second pass, 2026-08-13 — the mechanical sweep.** Rather than reading all 289
sites, glibc's tree was searched for the inverse pattern: explicit
`if (x == NULL) { __set_errno (E…); … }` checks in *non-stub* translation units
(many `io/*.c` and `posix/*.c` hits are the generic `ENOSYS` stubs that Linux
never uses — those were filtered out by looking for `stub_warning`/`ENOSYS` in
the file). Cross-referencing the survivors against our entry points found nine
more divergences, all fixed:

- `sigemptyset`, `sigfillset`, `sigaddset`, `sigdelset`, `sigismember` — NULL
  `set` was `EFAULT`, is now `EINVAL` (`posix/src/signal.rs`; glibc
  `signal/sigempty.c`, `sigfillset.c`, `sigaddset.c`, `sigdelset.c`,
  `sigismem.c`). These issue no syscall at all, so `EFAULT` was not merely the
  wrong constant — it was an errno the function cannot physically produce on
  Linux. The old doc comments asserted "glibc would segfault"; glibc does not.
- `closedir(NULL)` — was `EBADF`, now `EINVAL` (`posix/src/dirent.rs`; glibc
  `sysdeps/unix/sysv/linux/closedir.c`, which checks before touching the fd).
- `cfsetispeed`, `cfsetospeed`, `cfsetspeed` — NULL `termios_p` was `EFAULT`,
  now `EINVAL` (`posix/src/ioctl.rs`; glibc `termios/speed.c`).

Checked and found already correct in the same pass — recorded so they are not
re-examined: `nanosleep`/`setitimer` check only the mandatory argument;
`timer_create` treats a NULL `sevp` as SIGEV_SIGNAL per POSIX; `aio_suspend`
leaves `timeout` optional; `posix_spawn` leaves `pid`/`file_actions`/`attrp`
optional; `iconv` treats a NULL `inbuf` as a state reset. `telldir`/`seekdir`
looked like hits but the EINVAL there is in glibc's *stub*; Linux's versions
(`sysdeps/unix/sysv/linux/`) have no check.

**Third pass, 2026-08-13 — `pthread.rs` (47 sites), and it was not a judgement
call after all.** This cluster was recorded here as the one needing "a decision
rather than a lookup", because NPTL contains no NULL checks at all and so
offers no errno to copy. That framing was wrong. The answerable question was
the *ordering* one this entry's own **Proper fix** paragraph names: NPTL
validates its **scalar** argument first and dereferences the pointer only
afterwards, so on Linux a call that is bad in both ways returns the scalar's
`EINVAL`/`ERANGE`, never `EFAULT`. Our code checked the pointer first
everywhere. Nine functions fixed, each verified against the glibc (or kernel)
source and cited in both the code and the test — full reasoning in
`design-decisions.md` §303:

- `pthread_attr_setstacksize`, `pthread_attr_setstack` — size checked first
  (`check_stacksize_attr`). **Also** their floor was a hardcoded `4096` while
  the crate's own `PTHREAD_STACK_MIN` is `16384`; they now use the shared
  constant, so three stack sizes glibc rejects are no longer accepted.
- `pthread_attr_setdetachstate`, `pthread_mutexattr_settype`,
  `pthread_condattr_setclock` — scalar checked first.
- `pthread_rwlockattr_setpshared` — scalar first, **and** the errno was wrong:
  any non-`PRIVATE` value used to be `ENOTSUP`, conflating "we don't support
  cross-process rwlocks" with "that isn't a pshared value". glibc's
  `futex_supports_pshared` accepts both POSIX values and gives `EINVAL` for
  anything else; `ENOTSUP` is now `PTHREAD_PROCESS_SHARED` alone.
- `pthread_barrier_init` — count checked first, **and** `u32::MAX` used to be
  accepted, which was a latent hang (the arrival counter is an `AtomicI32`, so
  a count above `i32::MAX` is unreachable and every waiter would block
  forever). Now capped at glibc's `BARRIER_IN_THRESHOLD` (`UINT_MAX / 2`).
- `pthread_getname_np` — `len` checked first, **and** the check was against the
  stored name's length rather than `TASK_COMM_LEN`; glibc compares against the
  constant unconditionally, so a 4-byte buffer is `ERANGE` even for a
  2-character name. The second, now-dead length test is gone.
- `pthread_getaffinity_np` — both length rejections moved above the NULL check,
  and the missing `len & (sizeof (unsigned long) - 1)` test added.
  `pthread_setaffinity_np` keeps NULL-first and gained a comment saying why:
  the asymmetry is Linux's, not ours (`sched_setaffinity`'s `get_user_cpu_mask`
  does no size rejection and copies first, so `EFAULT` wins there).

**Fourth pass, 2026-08-13 — `xattr.rs` (11 sites).** Every one of these
pointers *does* reach a syscall, so §300's rule keeps `EFAULT` for all of them
and not one constant changed. The ordering was wrong throughout, though. Linux
resolves the path in `path_getxattr`/`path_setxattr`/`path_removexattr`
(fs/xattr.c) **before** the attribute name is ever read, and for the setters
`setxattr_copy` (fs/xattr.c:598-602) checks the flags before the name too. Our
entry points checked `path.is_null() || name.is_null()` as one test and then
the flags, giving three divergences:

- A nonexistent path with a NULL name was `EFAULT`; Linux gives `ENOENT`,
  because the name is not read until the path has resolved. (Bare metal only.)
- A bad flag with a NULL name was `EFAULT` in `setxattr`/`lsetxattr`/
  `fsetxattr`; Linux gives `EINVAL`.
- The path and the name were conflated, so neither could outrank the other.

The order is now path → (flags) → name in all nine path/name entry points,
enforced by two shared helpers, `resolve_xattr_path` and `check_xattr_name`,
which carry the citation once instead of at eleven call sites.

The same pass found an *invented* check: `setxattr_flags_valid` rejected
`XATTR_CREATE | XATTR_REPLACE` with `EINVAL`, an errno Linux never returns for
it. The kernel's only flag test is the mask `flags & ~(XATTR_CREATE |
XATTR_REPLACE)`, which both bits pass; the filesystem then answers from the
attribute's state — `EEXIST` if it exists, `ENODATA` if it does not (ext4's
`ext4_xattr_set_handle`, fs/ext4/xattr.c:2412-2423). Our own kernel already
agreed with Linux (`xattr_validate_size_flags` in `kernel/src/syscall/linux.rs`
masks with `0x3` and has no both-flags test), so the libc check was also
inconsistent with the layer below it. Removed: it is the filesystem's
judgement, not libc's.

**Fifth pass, 2026-08-13 — `unistd.rs` (13 sites).** Most were already correct
and several already carried citations (`realpath`, `canonicalize_file_name`
and `getrandom` were fixed in the earlier passes; `chdir`, `chroot`,
`getresuid`/`getresgid`, `sysinfo` and the `*at` paths all forward the pointer
to a syscall, so `EFAULT` stands). Three did not:

- **`getentropy(NULL, 512)` was `EFAULT`; glibc gives `EIO`.** glibc's
  `getentropy` (sysdeps/unix/sysv/linux/getentropy.c) opens with
  `if (length > 256) { __set_errno (EIO); return -1; }` before it forms
  `end = buffer + length` or issues `getrandom`. And `getentropy(NULL, 0)`
  now *succeeds*: the `while (buffer < end)` loop does not run, so no syscall
  is issued and the pointer is never touched — the same shape `getrandom`
  already documented one function above.
- **`gethostname(NULL, 0)` was `EFAULT`; glibc gives `ENAMETOOLONG`.**
  `__gethostname` (sysdeps/posix/gethostname.c) never checks the pointer; it
  `memcpy`s `min (len, node_len)` bytes and *then* tests
  `node_len > len`. With `len == 0` the copy moves nothing, so a NULL buffer
  is safe and the length test decides.
- **`gethostname` did not truncate.** The same `memcpy` runs unconditionally,
  so glibc fills a too-small buffer with as much of the name as fits —
  truncated and not null-terminated — before returning `ENAMETOOLONG`. We
  left the caller's buffer untouched, so a caller that ignored the return
  value read stale bytes instead of a truncated hostname. Now matched.

`getdomainname` is a **deliberate** divergence, now documented at the function:
glibc (misc/getdomain.c, the `_UTSNAME_DOMAIN_LENGTH` branch Linux takes) has
no length rejection at all and returns 0 after truncating, so a short buffer
silently receives an unterminated string. We keep `EINVAL` — quiet truncation
is the corruption this codebase forbids, and it is what the Linux man page
documents. Because the check is ours, it is ours to order, and it now goes
first, so `getdomainname(NULL, 0)` is `EINVAL` rather than `EFAULT`.

**Sixth pass, 2026-08-14 — `socket.rs` (15 sites).** Every constant was already
right; every bug was an ordering bug, and the answers this time came almost
entirely from `net/socket.c` rather than glibc, because the socket calls are
thin syscall wrappers. The one thing worth carrying forward is that *the
descriptor lookup is not uniformly first*: Linux splits the socket entry points
into two families, and the split is deliberate, not accidental.

- **`bind` / `connect` checked the address before the descriptor.**
  `__sys_bind` (net/socket.c:1835) calls `sockfd_lookup_light` and only then
  `move_addr_to_kernel`, so `bind(-1, NULL, len)` is `EBADF`; ours said
  `EFAULT`. `__sys_connect` (:2056) is the same via `fdget`. Fixed, and the
  existing `test_phase201_bind_null_addr_efault_before_eacces` had to move to a
  real socket fd — it had been passing `-1` and so was silently asserting the
  wrong precedence all along.
- **The two calls disagree about `ENOTSOCK`, and that is upstream's doing.**
  `bind` uses `sockfd_lookup_light` (:553), which resolves *and* type-checks
  before the address is read, so `ENOTSOCK` outranks `EFAULT`. `connect` uses a
  bare `fdget` and calls `sock_from_file` inside `__sys_connect_file`, i.e.
  *after* `move_addr_to_kernel`, so there `EFAULT` outranks `ENOTSOCK`. Both
  are now spelled out at the call sites with a cross-reference, because the
  asymmetry reads like a mistake and would otherwise be "tidied up".
- **`addrlen == 0` must not fault.** `move_addr_to_kernel` (:247) returns 0 at
  `ulen == 0` *before* its `copy_from_user`, so `bind(fd, NULL, 0)` never
  touches the pointer and falls through to the protocol's own length test:
  `EINVAL`, not `EFAULT`. A short-but-nonzero `addrlen` does reach the copy, so
  there `EFAULT` wins over that same `EINVAL`. Both directions now have tests.
- **`socketpair` faulted before it validated the type flags.**
  `__sys_socketpair` (:1729) tests `flags & ~(SOCK_CLOEXEC | SOCK_NONBLOCK)` at
  :1737, before it has reserved a descriptor — but reaches
  `put_user(fd1, &usockvec[0])` at :1758 *before* `sock_create` at :1771. So
  exactly one check outranks `EFAULT` (the flag `EINVAL`) and the family, type
  and protocol verdicts all rank below it. The first guess — that all of them
  outranked `EFAULT` — was wrong, and only reading the function settled it.
- **`inet_ntop` rejected NULL pointers before looking at the family.** glibc's
  `inet_ntop` (resolv/inet_ntop.c) is a bare `switch (af)` with no pointer
  checks at all, so an unrecognised family is `EAFNOSUPPORT` whatever the
  pointers hold. Within a family, `inet_ntop4` formats into a stack buffer and
  compares against `size` before its closing `strcpy`, so `ENOSPC` also
  outranks a NULL `dst`. The `dst` check now lives in both formatters, just
  after the `ENOSPC` test; the full order is `EAFNOSUPPORT`, `src`, `ENOSPC`,
  `dst`.
- **`setsockopt`'s multicast path conflated two errors into one.** It answered
  `EINVAL` for both a short `optlen` and a NULL `optval`.
  `do_ip_setsockopt` (net/ipv4/ip_sockglue.c:1222-1233) rejects
  `optlen < sizeof(struct ip_mreq)` and only then runs `copy_from_sockptr`, so
  they are distinct verdicts. Folding them told a caller with a correctly-sized
  buffer at a bad address that its *length* was wrong.
- **`sendmsg` / `recvmsg` read the header before the descriptor**, and worse,
  returned 0 for an empty `msg_iov` without ever looking at the fd.
  `__sys_sendmsg` (:2634) and `__sys_recvmsg` run `sockfd_lookup_light` before
  `___sys_sendmsg` copies the header in. Note the contrast with `sendto` /
  `recvfrom`: `__sys_sendto` (:2161) and `__sys_recvfrom` (:2224) import the
  user buffer *first*, so there `EFAULT` genuinely does outrank `EBADF` and our
  existing order was right. A new `socket_fd_is_valid()` helper carries the
  `sockfd_lookup_light` semantics (and the citation) once.
- **A zero-length `send`/`recv`/`recvfrom` returned 0 on a closed fd.** The
  POSIX no-op was short-circuiting above the descriptor lookup, but
  `__sys_sendto`/`__sys_recvfrom` look the descriptor up whatever the length
  is. A zero-length send is a common idiom for probing a socket's liveness;
  ours could not distinguish a live socket from a closed one.

**Seventh pass, 2026-08-14 — `spawn.rs` (16 sites).** The prediction going in
was that `spawn.rs` would be an ordinary §300 lookup with the constants right
and the ordering wrong. Half true: the ordering was indeed wrong at five call
sites, but here the *constants* were wrong too, and one check was missing
outright. `posix_spawn` is a pure-userspace API — no syscall is involved in
building a file-actions object — so every answer came from glibc, and all five
`add*`/`setflags` entry points turn out to hinge on a single 4-line helper.

- **`__spawn_valid_fd` is the whole story, and we had reimplemented it wrong.**
  posix/spawn_valid_fd.c is
  `fd >= 0 && (maxfd < 0 || fd < maxfd)`, with `maxfd = __sysconf
  (_SC_OPEN_MAX)`. Ours was `fd < 0` and returned `EINVAL`. Two independent
  bugs: **the errno is `EBADF`**, and **the upper bound was missing entirely**,
  so `addclose(acts, 1_000_000)` was accepted and would only fail later, in the
  child, after the fork. It is now one `spawn_valid_fd()` helper carrying the
  citation once, used by all four `add*` calls.
- **The descriptor check comes first, before the object and before the path.**
  `spawn_faction_addclose.c`, `spawn_faction_adddup2.c:32`,
  `spawn_faction_addopen.c` and `spawn_faction_addclosefrom.c:31` all open with
  `if (!__spawn_valid_fd (fd)) return EBADF;` — ahead of any read of
  `file_actions->__used` and, in `addopen`, ahead of `__strdup (path)`. We
  checked the pointers first, so `addopen(acts, -1, NULL, …)` reported the
  *path* when glibc reports the *descriptor*. `adddup2` tests both fds in the
  same expression, so an out-of-range `newfd` is `EBADF` on the same footing as
  a bad `fd`.
- **`addclosefrom_np` already had the right constant and the wrong order** —
  the one site where a previous pass got `EBADF` right by inspection but still
  put it below the NULL check.
- **`posix_spawnattr_setflags` had the order inverted *and a doc comment
  justifying the inversion*.** `__posix_spawnattr_setflags`
  (posix/spawnattr_setflags.c) is two statements — the `flags & ~ALL_FLAGS`
  rejection, then the store — with no NULL check at all; a NULL `attr` faults
  on the store, so the flag word is decided while the pointer is still
  untouched. Ours checked `attr` first and explained that this gave the caller
  "the more informative `EFAULT`". That is the **second** invented
  justification this audit has found in a doc comment, after `bind`'s in the
  sixth pass, and it is the more dangerous failure mode of the two: a wrong
  constant looks like an oversight and invites checking, whereas a wrong
  constant with a rationale attached reads as deliberate and deflects it. Both
  are now replaced by the upstream text, and the misnamed test
  (`test_setflags_null_attr_precedes_flag_check`, which asserted the opposite)
  is renamed with a note in its body recording what it used to claim.

The habit this pass adds: **when a check is duplicated across sibling
functions, find the shared helper upstream and port *it*, not the check.** All
four `add*` bugs were one bug, replicated four times by four separate readings
of four man pages that each say only "EBADF … the value specified by fildes is
negative".

**Eighth pass, 2026-08-14 — `file.rs` (28 sites).** The last dense cluster, and
the one that most rewarded reading. `file.rs` wraps thin syscalls, so — as in
`socket.rs` — the constants were nearly all right and the *orderings* were
wrong; but three of the entry points here have a **three- or four-deep** rank,
and we had several of them almost exactly reversed.

- **`read`/`write` short-circuited a zero-length call above the descriptor
  lookup.** `ksys_read` (fs/read_write.c:604) is `fdget_pos` first, and there
  is no zero-length shortcut anywhere upstream; `EFAULT` arises only later from
  `access_ok` inside `vfs_read` (:458). So `read(closed_fd, buf, 0)` was
  silently returning 0. This is the *same bug* the sixth pass fixed in
  `send`/`recv` — and a zero-length read is a common liveness probe, so the
  silent success is the worst possible answer.
- **`pread`/`pwrite` ran a four-deep order backwards.** `ksys_pread64` (:652)
  is `pos < 0` → `EINVAL`, `fdget` → `EBADF`, `!FMODE_PREAD` → `ESPIPE`, then
  `EFAULT`. We had `EFAULT`, zero-count, `EINVAL`, `EBADF`, `ESPIPE`.
- **The `pread`/`preadv` tests had been passing `fd 0`** — a console — so once
  `ESPIPE` was ordered correctly they *all* failed. They had only ever
  exercised the shortcuts that used to sit above the descriptor checks; none of
  them had ever reached the code they were named for. Same failure mode as
  `test_phase201_bind_null_addr_efault_before_eacces` in the sixth pass, at
  larger scale: **a test that picks a convenient fd rather than the right one
  silently stops testing anything once the ordering is fixed.**
- **The vectored calls folded three distinct verdicts into one `EINVAL`.**
  `iov.is_null() || iovcnt <= 0 || iovcnt > 1024` was a single branch.
  `iovec_from_user` (lib/iov_iter.c) separates them: `nr_segs == 0` returns an
  *empty iterator* (success, 0 bytes) before anything else, `nr_segs >
  UIO_MAXIOV` is `EINVAL`, and `copy_iovec_from_user` is `EFAULT`. The zero
  case is deliberate and commented upstream — "SuS says the readv() function
  *may* fail if the iovcnt argument was less than or equal to 0 … Linux has
  traditionally returned zero for zero segments" — and we were failing it.
  Note the interaction with the previous point: the zero-segment call is
  exactly the one a per-segment loop never checks a descriptor for, so it is
  also the one that must still report `EBADF`.
- **`truncate` checked the path before the length, and its own sibling ten
  lines below already had it right.** `do_sys_truncate` (fs/open.c:129) rejects
  a negative length before `user_path_at`; `do_sys_ftruncate` (:164-170) puts
  the same `EINVAL` above `fdget`'s `EBADF`, and `ftruncate` implemented that
  correctly. Adjacent functions, same file, one right and one wrong — the
  sixth pass's "do not generalise from a sibling" habit cuts both ways: you
  also cannot assume a sibling's *correctness* transfers.
- **`open`/`openat` never rejected `O_DIRECTORY|O_CREAT`** (nor `O_TMPFILE`
  without `O_DIRECTORY`, nor without write access). `build_open_flags`
  (fs/open.c) returns `EINVAL` for all three and runs ahead of both
  `getname(filename)` and `do_filp_open`'s use of `dfd`, so they outrank
  `EFAULT` for the path *and* `EBADF` for the directory. Factored into
  `validate_open_flags()` so both entry points run it in upstream's position.
  Our `EOPNOTSUPP` for a well-formed-but-unsupported `O_TMPFILE` correctly
  ranks below them: it corresponds to `do_tmpfile`, reached only inside
  `path_openat`.
- **`name_to_handle_at` — the third invented justification.** It checked
  `pathname`, `handle` and `mount_id` together, with a doc comment conceding
  that Linux "defers `handle`/`mount_id` checks until after `user_path_at`, but
  our model can do the cheap NULL check up front **without observable
  difference**." The difference is observable and obvious:
  `name_to_handle_at(bad_fd, "p", NULL, NULL, 0)` is `EBADF` upstream and was
  `EFAULT` here. After `bind` (sixth pass) and `posix_spawnattr_setflags`
  (seventh), that is three doc comments that *argued for* an order instead of
  citing one, and all three arguments were wrong.
- **`openat2` was already correct**, cited step by step against `sys_openat2`.
  Worth recording: the pass is not uniformly finding bugs, and the one function
  that had been written *from* the upstream source rather than from a man page
  is the one that needed nothing.

**The habit this pass adds — and it is the most useful one so far.** Every
wrong answer in this file was produced by a comment or a test that reasoned
about what an errno *ought* to be. Every right answer was produced by someone
reading the function. So: **a doc comment that argues for an ordering is a
defect marker.** Three for three. When you find one, do not evaluate the
argument — go read the upstream function, because the argument exists precisely
because nobody did.

**Ninth pass, 2026-08-14 — the timed waits (4 sites).** This one started as a
`pthread.rs` pass and immediately stopped being one: §303 had already walked
that file, so there was nothing left to classify there. What the re-read *did*
surface is that none of the blocking primitives validated `tv_nsec` at all.
glibc has a single shared predicate for it, `valid_nanoseconds`
(`include/time.h:517`, `0 <= ns && ns < 1000000000`), and calls it from
`pthread_cond_timedwait`, `pthread_mutex_timedlock`, the rwlock timed
variants and `sem_timedwait`. We called it from none of them, so a malformed
deadline silently became a very long — or instantly expired — wait.

The interesting part is that the *same* predicate is invoked from three
different places in the control flow, and the differences are deliberate:

- `pthread_cond_timedwait` checks **first**, before the mutex is released
  (`nptl/pthread_cond_wait.c:635` is the function's opening statement). The
  rejection is side-effect-free and the caller still holds the mutex.
- `pthread_mutex_timedlock` checks **lazily**, inside the contended branch —
  `nptl/pthread_mutex_timedlock.c:221` is literally commented "We are about
  to block; check whether the timeout is invalid." So an *uncontended*
  `timedlock` with `tv_nsec = 1e9` returns 0 and never reads the timespec.
  POSIX licenses this: "the validity of the abstime parameter need not be
  checked if the lock can be immediately acquired."
- `sem_timedwait` checks **eagerly**, above `__new_sem_wait_fast`
  (`nptl/sem_timedwait.c:28`), so the same call shape that succeeds for a
  mutex is `EINVAL` for a semaphore.

That is the sixth pass's "do not generalise from a sibling" rule again, but
sharper: here the three call sites share a *predicate* and still differ in
placement, and glibc knows it — `pthread_rwlock_common.c:286-291` carries a
comment explaining that the rwlocks were **switched from lazy to eager**, with
POSIX permitting either. So the lesson is not just that siblings differ, it is
that **a shared helper is not evidence of a shared control flow.** Porting the
helper (the seventh pass's rule) is necessary and not sufficient; you still
have to place each call where its own caller places it.

The fourth site is the counterexample that keeps the predicate honest.
`mq_timedsend`/`mq_timedreceive` are bare syscalls, so their deadline is
vetted by the *kernel*: `prepare_timeout` (ipc/mqueue.c) → `timespec64_valid`
(include/linux/time64.h), which additionally rejects `tv_sec < 0` ("Dates
before 1970 are bogus"). glibc's `valid_nanoseconds` never looks at `tv_sec`,
so a negative one is a deadline in the past — `ETIMEDOUT`, not `EINVAL`. Our
`deadline_from_timespec` had the nsec half and was missing the `tv_sec` half.
Two predicates that look interchangeable, differ in one line, and apply to
adjacent functions in the same crate: the §300 question ("does the pointer
reach a syscall") turns out to decide not only the errno but *which validity
rule applies at all*.

Habit from this pass: **when you find a validation missing, find its
predicate upstream before you write one** — and then check whether the
upstream predicate is glibc's or the kernel's, because the two disagree and
the disagreement is load-bearing.

**Tenth pass, 2026-09-25 — `process.rs` and `epoll.rs` (19 sites), lane D.**
The two files the ninth pass named as the last clusters. Eleven functions were
wrong, two were right, and the pass turned up a fact about the kernel that
changes how the NULL test should be placed everywhere.

- **`clone` was this entry's founding bug in miniature.** glibc's
  `x86_64/clone.S` loads `-EINVAL` and refuses a NULL function, then a stack
  that is zero once aligned down to 16 (`andq $-16, %rsi`) — so a stack of
  1-15 as well. The doc comment above `clone` said `EINVAL` throughout; the
  code and three tests said `EFAULT`. That is the blanket sweep's signature
  exactly: code and tests edited to agree, the one sentence that was right
  left behind.
- **`clone3` had a comment claiming "Linux order" for the reverse of it.**
  `copy_clone_args_from_user` (kernel/fork.c:3074-3077) tests the size before
  `copy_struct_from_user` reads the struct, so `clone3(NULL, 8)` is `EINVAL`.
- **`process_vm_readv`/`writev` had four faults.** The pid came second;
  upstream looks it up last, after both vectors (mm/process_vm_access.c:196).
  Both counts came before either pointer; upstream validates the local vector
  completely before it reads the remote one. A rule that each vector's *summed*
  lengths fit `ssize_t` came from the man page; the code tests each length
  alone (lib/iov_iter.c:1380) and caps the total. And the local vector's range
  test (`access_ok`, :1484 — once per segment, or once on the truncated length
  for a single segment) was missing, so a local range running into the kernel
  half was accepted. The doc comment also placed `process_vm_rw` in
  fs/read_write.c. Upstream's two zero-byte early returns are honoured but
  answered `ENOSYS` rather than 0 (design-decisions §1107).
- **`mount` was reworked against fs/namespace.c.** It refused flag bits
  outside a whitelist (upstream refuses only `MS_NOUSER`), refused any two
  "mode" bits together (refusing `MS_REMOUNT | MS_BIND`, the usual way to make
  a bind mount read-only), gave `EFAULT` for a NULL source or type (upstream:
  `EINVAL` from the operation, and a NULL source is legal for a new mount),
  collapsed an empty type to `EINVAL` (upstream: `ENODEV`), capped the type at
  an invented 256 bytes, and asked for privilege last (upstream asks before the
  operation is chosen). The string limits were off by one — 4096 bytes of name
  were accepted where `PATH_MAX` counts the NUL — and a too-long type or source
  is `EINVAL` (`strndup_user`), not `ENAMETOOLONG`, and outranks the target.
- **`umount`/`umount2` described a kernel older than the reference.** They
  asked for privilege before the path, citing `ksys_umount`. That was true
  before 5.9; since then `may_mount` is in `can_umount` (fs/namespace.c:1873),
  after `user_path_at`. `umount` is now `umount2(name, 0)`, as glibc's is.
- **`waitid` wrote the wrong amount.** Linux writes six fields of `*infop` —
  zeros — on a `WNOHANG` miss *and on every error* (kernel/exit.c:1726-1737).
  We wrote nothing on an error and zeroed all 128 bytes on a miss, each under a
  comment giving a reason.
- **`epoll_ctl`** copied the event only for ADD and MOD; upstream copies it for
  every op but DEL (`ep_op_has_event`), so an unknown op with a NULL event is
  `EFAULT`. It had no `EPERM` for a target without `poll` — a regular file was
  accepted and then reported ready on every wait — and compared descriptor
  numbers where upstream compares files, so a `dup` of the epoll fd could be
  added to itself.
- **`epoll_wait` had been "fixed" away from upstream.** A comment said the
  order before it — `maxevents`, `access_ok`, `fdget` — had been a bug, and
  moved the lookup first. That order *is* `do_epoll_wait`'s (:2291-2299).
- **`eventfd_read`/`eventfd_write`** refused a descriptor of another kind with
  an `EINVAL` attributed to the kernel's read. glibc's are `read`/`write` of
  eight bytes and nothing else; they are that now.
- **`signalfd`** sent an open descriptor to `ENOSYS` behind a `TODO` (with no
  `todo.txt` entry) saying the kind could not be told. It can: nothing libc
  holds is a signalfd, so an open one is `EINVAL`, as `do_signalfd4` says.
- **`inotify_add_watch`** tested for one of the twelve event bits, which is
  wrong both ways: upstream refuses a bit outside `ALL_INOTIFY_BITS` and a zero
  mask, and accepts a mask of flags alone. The `IN_MASK_ADD | IN_MASK_CREATE`
  refusal was missing.
- **Right already:** `timerfd_settime`/`timerfd_gettime`, cited and checked
  against fs/timerfd.c:454-578; `wait4`'s and `waitid`'s optional pointers.
- Every descriptor lookup in `epoll.rs` now goes through an `fdget` that, like
  upstream's (fs/file.c:1030), cannot see an `O_PATH` descriptor.

**The habit this pass adds: on x86-64, `access_ok` admits NULL.**
`valid_user_address` is `(long)(x) >= 0` (arch/x86/include/asm/uaccess_64.h:57)
and `__access_ok` a range test on the end (:85), so a NULL buffer passes it and
faults at its first *use*. Earlier passes put the NULL test where `access_ok`
sits. That keeps the `EBADF` ordering right when a descriptor lookup precedes
the range check — `ksys_read` looks up first — but answers `EFAULT` where the
call would have copied nothing, and it is wrong outright when the lookup comes
second: `do_epoll_wait` runs `access_ok` *before* `fdget`, so NULL there must
not outrank `EBADF`, and does not fault at all when nothing is ready. **Put the
NULL test where the copy is, not where the range check is.** (The x86 headers were added to the
`D:\refsrc\linux-6.6` sparse checkout for this: `git -c
core.protectNTFS=false sparse-checkout add arch/x86/include`.)

And the defect-marker habit held again: `clone3`'s "Linux order", `epoll_wait`'s
"previously … a bug", `eventfd_read`'s kernel `EINVAL`, `umount`'s
unprivileged caller who "never learns" about its path, and two `mount` test
stories — one in which Linux refuses a stale flag through a whitelist it does
not have, one in which it "requires two separate calls" for a bind remount it
performs in one. Nine for nine across four passes. One refinement: `umount`'s
comment was *true of an older kernel*. Before trusting a cited order, check
which kernel it describes.

**Eleventh pass, 2026-09-26 — a seeded sample of twenty from the tail, lane D.**
The tenth pass proposed retiring this entry by sampling: classify twenty of the
tail's sites at random and close the entry if they came back clean. They did
not. Twelve of the twenty were wrong — nearly all in their *order* — and
functions beside five of them were wrong too. (The sample is reproducible: the
sites are every non-comment `is_null()` line in `posix/src` with `EFAULT` in
the next four lines, outside test modules and the seven files walked by passes
three to ten, sorted; then `random.seed(20260926); random.sample(sites, 20)`.)

Sampled and wrong:

- **`aio_fsync`** tested `aiocbp` before `op`, and let a closed descriptor
  through to the asynchronous status; glibc tests `op`, then asks
  `fcntl(F_GETFL)` (rt/aio_fsync.c) — under a comment calling the reverse
  "Linux's libaio/glibc convention".
- **`scandir`** refused a NULL `namelist` before opening the directory, walked
  the directory twice (calling the filter twice per entry), and stored an
  empty allocation for no entries. glibc's `__scandir_tail` opens first, walks
  once, and stores NULL; `scandirat` had the same order.
- **`getdents64`** refused a zero count and a NULL buffer before a closed
  descriptor. fs/readdir.c looks the descriptor up first and judges the count
  and the buffer per entry, so at the end of a directory the answer is 0
  whatever they are; a NULL buffer is now probed rather than refused. Legacy
  `getdents`, beside it, had the same order and answered `ENOSYS` to every valid
  call, "because the legacy record's inode field is 32 bits" — true only of
  32-bit architectures. It writes `struct linux_dirent` now (§1108).
- **`ftw`** refused `nopenfd < 1` with `EINVAL`; glibc's `ftw_startup` makes it
  1. Beside it, the walk stopped `nopenfd` levels deep
  (`B-D-FTW-STOPS-AT-NOPENFD-DEEP`) — both fixed the same day by porting
  glibc's walker (§1109).
- **`io_getevents`** refused a NULL `events` before looking for events; fs/aio.c
  faults only in the copy, so with nothing to deliver the answer is 0, and a
  fault leaves the event queued.
- **`landlock_create_ruleset`** read flags 0 with a NULL `attr` as a malformed
  probe (`EINVAL`). Flags 0 is the create form, whose NULL `attr` is
  `copy_min_struct_from_user`'s `EFAULT`, before the size — the comment said
  the reverse.
- **`posix_memalign`** refused a NULL `memptr` before the alignment; glibc tests
  the alignment (`EINVAL`), allocates, and only then writes.
- **`mq_getattr`/`mq_setattr`**: a NULL `attr` was `EFAULT`. glibc's
  `mq_getattr` is `mq_setattr (mqdes, NULL, attr)`, and ipc/mqueue.c treats
  either NULL as "not asked". `mq_setattr` also accepted flags other than
  `O_NONBLOCK`.
- **`mq_timedsend`/`mq_timedreceive`** refused a NULL timeout. It is no
  timeout: glibc's own `mq_send` is `mq_timedsend` with NULL.
- **`sched_setaffinity`** refused a mask shorter than 128 bytes with `EINVAL`
  ("our stub does not zero-pad"), and a NULL mask of length 0 with `EFAULT`.
  `get_user_cpu_mask` zero-extends a short mask — `sizeof (unsigned long)` is
  a size Linux programs pass — and copies nothing for length 0.
- **`mknod`** refused type 0 with `EINVAL`, which Linux makes a regular file,
  and a directory with `EINVAL` rather than `EPERM` — both after the path,
  where `may_mknod` runs first; its tests called Linux "strict" about type 0.
  `mknodat`, beside it, the same.
- **`fattach`** validated its arguments; glibc 2.39's is `ENOSYS` whatever they
  are (posix/streams-compat.c). So are `fdetach`, `putmsg`, `putpmsg`,
  `getmsg` and `getpmsg`, which had the same invented validation — added by an
  earlier phase to give "probing callers meaningful feedback", which did the
  opposite: a probe with placeholder arguments was told `EBADF`, not the
  `ENOSYS` it tests for (§1108). `isastream` called a closed descriptor "not a
  stream"; glibc says `EBADF`.

Sampled and right: `setkey`'s `EFAULT` (crypt.rs: the §303 substitute, with no
upstream to check against since glibc 2.39 dropped `setkey`); `TIOCSPGRP`
(ioctl.rs: `ENOTTY` before the `get_user`, as `tiocspgrp`); `perf_event_open`
(`perf_copy_attr`'s order); `SECCOMP_GET_ACTION_AVAIL`; `getpwuid_r` (the §303
substitute — upstream's answer depends on the NSS backend); the NULL pointers of
`clock_adjtime`, `clock_gettime` and `timer_create`. Beside those last three,
though: `clock_adjtime(CLOCK_TAI)` adjusted the real-time clock, where 6.6's
`clock_tai` has no `clock_adj` and the answer is `EOPNOTSUPP`; and `timer_create`
armed `CLOCK_MONOTONIC_RAW` and the two `_COARSE` clocks, which can be read but
not armed (`EOPNOTSUPP`), and reported a full timer table after the event and
pointer checks it precedes.

**So sampling cannot retire this entry.** With twelve of twenty wrong, the
tail is presumptively wrong, not presumptively right, and gets a full sweep.

**Twelfth pass, 2026-09-26 — `ioctl.rs` (8 sites), lane D.** The first file
of the full sweep. `TIOCGWINSZ`, `TIOCSWINSZ`, `FIONBIO`, `TCGETS` and
`TIOCSPGRP`'s pointer were in Linux's place already; three were not, and each
opened onto more:

- **`FIONREAD`** tested its pointer before asking whether the file answers
  FIONREAD at all, so an epoll descriptor with a NULL `arg` said `EFAULT`
  where Linux says `ENOTTY`. Behind it: a regular file was `ENOTTY` ("files
  don't support FIONREAD"), where `do_vfs_ioctl` answers size less offset; an
  inotify descriptor was `ENOTTY` under a comment saying inotify has no ioctl
  — `inotify_ioctl` counts the queued events' bytes; and a listening TCP
  socket said 0, where `tcp_ioctl` says `EINVAL`.
- **`TIOCGPGRP`** tested its pointer before the controlling-terminal check
  `tiocgpgrp` makes first on a console or slave (on a master there is no such
  check and a NULL is `EFAULT`, as it was).
- **`TIOCSPGRP`** left the negative-group test to `tcsetpgrp`, and
  **`tcgetpgrp`/`tcsetpgrp`** accepted any open descriptor — a regular file's
  included — under a comment saying descriptor kinds were not tracked, which
  `ioctl` two files away was using. They are glibc's now: `ioctl(fd,
  TIOCGPGRP/TIOCSPGRP, …)`. `tcsetpgrp` also refused a group of 0 itself; that
  is the kernel's call, and Linux's answer is `ESRCH`, not `EINVAL`
  (`requests/d-a-tcsetpgrp-of-group-0-and-a-terminal-that-is-not-ours.md`).
- In the inotify read path the FIONREAD count needed: names were cut at 63
  bytes — another file's name, not a shorter one — and rounded to 8 bytes
  where Linux rounds to 16; and a dead instance read as an empty queue rather
  than `EBADF`.

`TCSETS`'s pointer is Linux's too, with one ordering left: `set_termios` runs
`tty_check_change` (which stops a background caller with `SIGTTOU`) before
its copy, and our kernel makes that check inside the call, which a NULL
pointer never reaches.

**Thirteenth pass, 2026-09-26 — `semaphore.rs` (8 sites), lane D.** Against
glibc 2.39's nptl and posix/shm-directory.c. `sem_wait`, `sem_trywait`,
`sem_post` and `sem_getvalue` were right: the pointer is the first thing each
touches. The rest:

- **`sem_init`** tested `sem` before the value; glibc refuses a value past
  `SEM_VALUE_MAX` first.
- **`sem_timedwait`** tested both pointers before the deadline; glibc reads
  `abstime->tv_nsec` first, so a NULL `sem` with a malformed deadline is
  `EINVAL`.
- **`sem_close(NULL)`** was `EFAULT`; glibc looks the pointer up among its
  mappings and never dereferences it, so it is `EINVAL`.
- **The name check** required one leading `/`, refused a second, and called a
  long name `EINVAL`. glibc's `__shm_get_name` strips *every* leading `/` —
  `"sem"`, `"/sem"` and `"//sem"` are one semaphore — refuses only an empty
  name or an inner `/`, and a name too long for `/dev/shm/sem.NAME` is
  `ENAMETOOLONG`; names were also capped at 63 bytes.
- Beside them: `sem_init` accepts a non-zero `pshared` and then works only
  inside one process, and the pthread calls that ask for process-shared
  objects do not exist — `B-D-PROCESS-SHARED-SYNC-IS-SILENTLY-PRIVATE` (new).

**Fourteenth pass, 2026-09-26 — `time.rs` (6 sites; `clock_gettime` and
`timer_create` were the eleventh pass's), lane D.** `nanosleep`,
`timer_gettime` and `getitimer` were right.

- **`clock_nanosleep`** refused every flag bit but `TIMER_ABSTIME`, ahead of
  everything, under a comment citing `if (flags & ~TIMER_ABSTIME) return
  -EINVAL;` in `common_nsleep` — which is not there; Linux reads only
  `TIMER_ABSTIME`. Behind it: the clocks that cannot be slept on
  (`CLOCK_MONOTONIC_RAW` and the `_COARSE` pair) are `EOPNOTSUPP`, and the
  calling thread's CPU clock is glibc's `EINVAL`; a negative `tv_sec` was
  "already past" (0) in the absolute form and `EINTR` in the relative one,
  where `timespec64_valid` says `EINVAL`; every failure of a relative sleep
  was reported as `EINTR`; an interrupted absolute sleep answered 0; and a
  distant absolute deadline overflowed its nanosecond sum.
- **`clock_settime`** lacked `timespec64_valid_settod`'s upper bound
  (`TIME_SETTOD_SEC_MAX`).
- **`setitimer`** refused a NULL new value with `EFAULT`; Linux 6.6 takes it as
  zeros — disarm — a "misfeature" it still supports, and reads the value before
  judging `which`.

The defect-marker habit held again, in its strongest form yet: a comment
quoting upstream code that upstream does not contain. **Check a quoted line
against the source before believing the quotation.**

**Fifteenth pass, 2026-09-26 — `aio.rs` (6 sites), lane D.** Against glibc
2.39's rt/ (`aio_misc.c`, `aio_suspend.c`, `lio_listio-common.c`,
`aio_cancel.c`), read in full. `aio_fsync` was right (the eleventh pass). The
rest:

- **`aio_read`/`aio_write`** refused a bad descriptor, buffer or offset at
  once. glibc refuses only a priority outside `0..=AIO_PRIO_DELTA_MAX` before
  it queues a request (recording `EINVAL` in the `aiocb` too); everything
  else is the request's own outcome, read through `aio_error`. The priority
  check was missing.
- **`aio_suspend`** tested the list before `nent` and refused `nent == 0`;
  glibc refuses a negative `nent` first, returns 0 for an empty list, and
  reads the list only when it has entries.
- **`lio_listio`** tested the list before `mode` and `nent`, refused an
  unknown opcode at once, and returned the last request's `errno` where glibc
  returns `EIO`.
- **`aio_error(NULL)`** returned `EINVAL` as its *value* — "the request failed
  with EINVAL" — where glibc faults; it is -1 with `EFAULT`, POSIX's shape for
  `aio_error` itself failing.
- Beside them, reading the rest of rt/ turned up the module's real faults —
  outcomes evicted after 16 requests, notification ignored —
  `B-D-AIO-OUTCOMES-EVICTED-AND-NEVER-NOTIFIED` (new, fixed with it).

**Sixteenth pass, 2026-09-26 — `sched.rs` (6 sites), lane D.** Against Linux
6.6's kernel/sched/core.c and glibc 2.39's `sched_getaffinity` wrapper.
`sched_rr_get_interval` was right, and `sched_setaffinity` was the eleventh
pass's.

- **`sched_setscheduler`, `sched_setparam`, `sched_getparam`** answered a NULL
  `param` with `EFAULT`; Linux's `if (!param || pid < 0) return -EINVAL;`
  answers `EINVAL`, before any copy. They had been `EINVAL` until "Phase
  210" changed them, reasoning from `copy_from_user` without reading the line
  above it -- the defect marker again, this time as a phase header.
- **`sched_getaffinity`** refused every mask shorter than the whole 128-byte
  `cpu_set_t`, though Linux takes any whole number of `unsigned long`s that
  covers the CPUs -- 8 bytes on a machine of up to 64 -- and it did not
  refuse a length that is not such a number. A mask longer than 128 bytes
  kept its old tail, which glibc's wrapper zeroes.
- Beside them: `SCHED_RESET_ON_FORK` made any policy unknown, where Linux
  strips it; and `sched_setscheduler(SCHED_DEADLINE)` was accepted, or
  `EPERM`, where Linux says `EINVAL` -- a deadline task's parameters come only
  from `sched_setattr`, and through this call they are zero.

**Seventeenth pass, 2026-09-26 — `mqueue.rs` (5 sites), lane D.** Against Linux
6.6's ipc/mqueue.c and glibc 2.39's wrappers. The name's NULL (`EFAULT`, where
glibc reads `name[0]`) was right, and the eleventh pass had already made NULL
timeouts and attributes Linux's.

- **`mq_send`** refused a NULL message before the priority, the descriptor
  and the size; Linux's order is the priority (`EINVAL`), the descriptor
  (`EBADF`), its write access (`EBADF` -- not kept at all), the size
  (`EMSGSIZE`), and the message last (`load_msg`, `EFAULT`).
- **`mq_receive`** refused a NULL buffer before the descriptor, the size and
  an empty queue. Linux reaches the buffer only after taking the message:
  `store_msg` faults, the call fails with `EFAULT`, and the message is gone.
- **`mq_notify`** looked at the descriptor before the `sigevent`; Linux
  judges `sigev_notify` and the signal first.
- Beside them, the module was a small static pool -- 8 queues of 32
  messages of 256 bytes, a default message size of 64, busy-spinning waits,
  no access modes, no `mq_notify` --
  `B-D-MQUEUE-LIMITS-ACCESS-AND-ERROR-ORDER` (new, fixed with it).

**Eighteenth pass, 2026-09-26 — `linux_futex.rs` (4 sites), lane D.** Against
Linux 6.6's kernel/futex/ (`SYSCALL_DEFINE6(futex)`, `do_futex`,
`get_futex_key`).

- A malformed timeout is `EINVAL` before the word is looked at (it was
  `EFAULT` for a NULL word); a misaligned word is `EINVAL` before `EFAULT`;
  a private `FUTEX_WAKE` never reads its word, so a NULL one answers 0.
- `FUTEX_CLOCK_REALTIME` was stripped and ignored; Linux answers `ENOSYS`
  for it on any command without an absolute timeout.
- Beside them, the finding of the pass: `FUTEX_WAIT_BITSET` was `ENOSYS` --
  `B-D-FUTEX-WAIT-BITSET-WAS-ENOSYS` (new, fixed with it).

**Nineteenth pass, 2026-09-26 — `resolv.rs` (5 sites), lane D.** The query
calls were stubs that checked their arguments and answered `ENOSYS`, and two
of the checks were inventions: `""` refused as `EINVAL` (it is the root name)
and `res_send` judging the query's length (glibc judges the answer buffer's,
after "no nameserver"). So the pass became the resolver --
`B-D-RES-QUERY-WAS-ENOSYS` (new, fixed with it). Its NULLs now fall where
glibc's would: a NULL name or answer is `EFAULT` with `h_errno`
`NETDB_INTERNAL`, since glibc faults; `res_send` makes glibc's two checks
first (`ESRCH`, then `EINVAL`); `res_mkquery` answers -1, as glibc answers
its own refusals; and `dn_expand`, `dn_comp` and `dn_skipname` set
`EMSGSIZE` when they fail, as glibc's do.

**Twentieth pass, 2026-09-26 — `statvfs.rs` (4 sites), lane D.** Against
Linux 6.6's fs/statfs.c and glibc 2.39's `statvfs64.c` and `fstatvfs64.c`.

- **`statvfs`, `statfs`** refused a NULL buffer before they looked at the
  path. Linux finds the filesystem first (`user_statfs`) and copies the
  answer out last (`do_statfs_native`, `EFAULT`); glibc's `statvfs` makes
  that call into a local of its own and converts it afterwards. So an empty
  path is `ENOENT`, and an overlong one `ENAMETOOLONG`, even beside a NULL
  buffer. A NULL path stays `EFAULT` -- the kernel's `getname` faults on it.
- **`fstatvfs`, `fstatfs`** already put `EBADF` first; the query of the
  descriptor's stored path now comes before the buffer too, as `fd_statfs`
  comes before the copy.
- Beside them: every answer is written whole, so `statvfs` zeroes
  `__f_spare` as glibc's conversion does, and `statfs` sets `ST_VALID` in
  `f_flags`, as Linux does on every answer. The host tests resolve the path
  as the target does; only the kernel's figures are defaults there.

**Twenty-first pass, 2026-09-26 — `linux_module.rs` (4 sites), lane D.**
Against Linux 6.6's kernel/module/main.c. Every call still ends in `ENOSYS`
where Linux would start loading or unloading; what changed is which checks
come before that.

- **`init_module`, `finit_module`** answered a NULL parameter string with
  `EFAULT`. Linux copies `uargs` only inside `load_module`, once the image
  has been checked and accepted as a module -- past the point where these
  calls end -- so it is no longer looked at. The image's NULL (`EFAULT`,
  after the length checks) was right.
- **`finit_module`** refused only a negative descriptor; one that was not
  open, was `O_PATH`, or was not open for reading went on to `ENOSYS`.
  Linux's `fdget` finds neither of the first two and
  `idempotent_init_module` refuses the third, all with `EBADF`.
- **`delete_module`**'s NULL (`EFAULT`, after `EPERM`) was right, but it
  refused an empty name, a long one, one holding a `/`, and an unknown flag,
  all with `EINVAL`. Linux refuses none of them: it looks any name up
  (`ENOENT` when no module has it) and reads the flags only for a module it
  finds in use. The four checks are gone.

**Twenty-second pass, 2026-09-26 — `sysv_msg.rs` (4 sites), lane D.** Against
Linux 6.6's ipc/msg.c (`ksys_msgsnd`, `do_msgrcv`, `ksys_msgctl`).

- **`msgsnd`**'s NULL (`EFAULT`, first: `get_user` of the type) was right.
- **`msgrcv`** refused a NULL buffer before the id and the queue. Linux
  reaches the buffer only when it writes the message out: a bad id is
  `EINVAL`, an empty queue with `IPC_NOWAIT` is `ENOMSG`, and with a message
  there the call takes it and then fails with `EFAULT`. `MSG_COPY` alone
  reads the buffer first (`prepare_copy`).
- **`msgctl(IPC_SET)`** looked the queue up before its buffer; Linux copies
  the buffer in first, so a NULL one is `EFAULT` whatever the id.
  `IPC_STAT`'s order (the queue, then `EFAULT`) was right.
- Beside them, the module was a small static pool with none of Linux's
  limits, permissions or waiting --
  `B-D-SYSV-MSG-LIMITS-PERMISSIONS-AND-ERROR-ORDER` (new, fixed with it).

**Twenty-third pass, 2026-09-26 — `sys_sysctl.rs` (4 sites), lane D.** Against
glibc 2.39's `sysdeps/unix/sysv/linux/sysctl.c` and Linux 6.6, which has no
`sysctl` system call (removed in 5.5).

- **All four NULLs** (`args`, the name, `oldlenp` beside a buffer, `newval`
  beside a length) were `EFAULT`, among checks for `EINVAL`, `ENOTDIR` and
  `E2BIG` that no kernel made in that combination -- the pre-5.5 kernel said
  `ENOTDIR` for a bad length, not `EINVAL`. glibc keeps `sysctl` only as a
  compat stub that answers `ENOSYS` whatever it is given, so every one of
  them is `ENOSYS` now.
- Beside them: `sysctl` took the removed system call's one argument, a
  `struct __sysctl_args *`, where glibc's function takes six (`name`, `nlen`,
  `oldval`, `oldlenp`, `newval`, `newlen`) -- a C caller's name array was
  read as that structure. It has glibc's signature now, and `SysctlArgs` the
  kernel's 80 bytes (`__unused[4]` was missing).

**Twenty-fourth pass, 2026-09-26 — `stat.rs` (4 sites), lane D.** Against
glibc 2.39's `__mknodat`, `mknod`, `mkfifo` and `mkfifoat` (io/,
sysdeps/posix/) and Linux 6.6's `do_mknodat` (fs/namei.c).

- **`mknod` and `mknodat`** put their NULL (`EFAULT`) after `may_mknod`, as
  `do_mknodat` does -- right -- but glibc makes one check first: a device
  number wider than the kernel's 32 bits is `EINVAL` before the system call.
  It was ignored.
- **`mkfifo` and `mkfifoat`** judged their NULL first. glibc's are
  `mknodat(fd, path, mode | S_IFIFO, 0)`, so type bits in `mode` beside the
  FIFO's make a type `may_mknod` refuses: `EINVAL`, ahead of the path. They
  are that call now, as `mknod` is `mknodat(AT_FDCWD, ...)`.
- Beside them: the directory descriptor was judged for an absolute path too
  (`EBADF`), which `path_init` never looks at; and a regular file -- type 0
  or `S_IFREG` -- answered `ENOSYS` where Linux's `vfs_create` makes it. It
  is made now, through `openat(O_CREAT | O_EXCL)`.

**Twenty-fifth pass, 2026-09-26 — `sysv_sem.rs` (3 sites), lane D.** Against
Linux 6.6's ipc/sem.c (`ksys_semtimedop`, `do_semtimedop`, `semctl_main`) and
glibc 2.39's `__semctl64`.

- **`semop`, `semtimedop`** refused a NULL operation array before the count.
  Linux judges the count first -- more than `SEMOPM` (500) is `E2BIG`, none
  is `EINVAL` (it was `E2BIG`) -- and only then copies the array (`EFAULT`).
- **`GETALL`, `SETALL`** reach their array after the lookup and the
  permission, as `semctl_main` does: a bad id is `EINVAL` whatever the
  array. They were separate functions `semctl` could not reach -- the
  finding below.
- Beside them, the finding of the pass: `semctl` took three arguments, so
  C's fourth never arrived, and `semtimedop`'s timeout was read as a date --
  `B-D-SYSV-SEM-SEMCTL-TIMEOUT-AND-LIMITS` (new, fixed with it).

**Twenty-sixth pass, 2026-09-26 — `linux_aio_abi.rs` (3 sites), lane D.**
Against Linux 6.6's fs/aio.c.

- **`io_submit`** refused a NULL iocb pointer with `EINVAL`; `io_submit_one`'s
  `copy_from_user` makes it `EFAULT`. A NULL `iocbpp` was already `EFAULT`,
  and stays so, after the context.
- **`io_setup`**'s NULL `ctxp` was right (`get_user`, first) -- and nothing
  after it was: the limits, the context's id, and how much it holds.
- **`io_getevents`**' NULL `events` was fixed by the eleventh pass.
- Beside them, the finding of the pass: nothing Linux refuses at submission
  was refused at submission here, `io_getevents` did not wait, and a context
  id was a slot number libaio dereferences -- `B-D-AIO-WAS-NOT-LINUXS` (new,
  fixed with it).

**Twenty-seventh pass, 2026-09-26 — `linux_seccomp.rs` (4 sites), lane D.**
Against Linux 6.6's kernel/seccomp.c and net/core/filter.c.

- **`SECCOMP_SET_MODE_FILTER`** faulted on a NULL program header in the right
  place, but never read a real one: after the header come its length (0, or
  more than 4096 instructions, is `EINVAL`), then the privilege gate
  (`EACCES`), then the program pointer (NULL is `EINVAL`,
  `bpf_check_basics_ok`) -- a length of 0 reached the gate, and a NULL
  program was `ENOSYS`.
- **`SECCOMP_GET_ACTION_AVAIL`, `SECCOMP_GET_NOTIF_SIZES`, strict mode** --
  right.
- Beside them, the finding of the pass: two flag rules 6.6 does not have,
  and `prctl`'s seccomp options answering `EINVAL` --
  `B-D-SECCOMP-FLAGS-AND-PRCTL` (new, fixed with it).

**Twenty-eighth pass, 2026-09-26 — `mman.rs` (5 sites), lane D.** Against
Linux 6.6's mm/mmap.c, mm/mprotect.c, mm/mincore.c and mm/memfd.c.

- **`munmap(NULL, n)`** was `EINVAL`; `do_vmi_munmap` unmaps `[0, n)` and
  answers 0.  Its range check was missing, so a kernel-half address reached
  the kernel -- which then unmapped it (lane A's, reported and being fixed).
- **`mprotect(NULL, n)`** was `EINVAL` before anything; `do_mprotect_pkey`
  judges the growth flags, the alignment, a zero length (0), the range's end
  (`ENOMEM`) and only then the prot bits, and a NULL range is the kernel's
  `ENOMEM`.
- **`mincore(addr, 0, NULL)`** was `EFAULT`; with no pages nothing is copied,
  so it is 0.
- **`memfd_create(NULL, …)`** was right, and the name beside it was not --
  `B-D-MEMFD-NAME-WAS-A-PATH` (new, fixed with it).
- `shm_open(NULL, …)` keeps the §303 substitute: glibc reads the name at
  once and faults.

**Twenty-ninth pass, 2026-09-26 — `resource.rs` (5 sites), lane D.** Against
glibc 2.39's getrlimit64.c/setrlimit64.c and Linux 6.6's `prlimit64`.

- **`getrlimit(res, NULL)`, `setrlimit(res, NULL)`** were `EFAULT`.  glibc on
  x86-64 makes neither system call: both are `prlimit64` with the other
  pointer NULL, so a NULL pointer asks for nothing and a valid resource is 0
  -- `B-D-RLIMIT-NULL-WAS-EFAULT` (new, fixed with it).
- **`prlimit`**'s two pointers: the new limit is read first and the old one
  written last, only if nothing before it failed.
- **`getrusage(who, NULL)`** -- right: `who` first, then the copy.

**Thirtieth pass, 2026-09-26 — `crypt.rs` (4 sites), lane D.** Against
libxcrypt 4.4.36 -- glibc 2.39 has no `crypt` -- as Ubuntu 24.04 builds it,
failure tokens on, and probed there.

- **`crypt(NULL, s)`, `crypt(k, NULL)`, `crypt_r(NULL, s, d)`,
  `crypt_r(k, NULL, d)`** were NULL with `EFAULT`; libxcrypt returns its
  failure token, `"*0"`, with `EINVAL`.
- **`crypt_r(k, s, NULL)`** stays NULL, now documented as the substitute for
  the fault libxcrypt takes writing its token there; it is `EFAULT`.
- **`encrypt(NULL, …)`, `setkey(NULL)`** keep `EFAULT` for the same reason.
- Beside them, the finding of the pass: every failure was NULL where
  libxcrypt returns the token, and three of libxcrypt's refusals were missing
  -- `B-D-CRYPT-FAILED-WITH-NULL` (new, fixed with it).

**Thirty-first pass, 2026-09-26 — `iconv.rs` (3 sites), lane D.** Against
glibc 2.39's iconv/iconv.c, iconv_open.c and iconv_close.c, and probed on
Ubuntu 24.04.

- **`iconv_open(NULL, …)`, `iconv_open(…, NULL)`** were `EINVAL`, the answer
  for an unknown character set; glibc faults reading the name, so the
  substitute is `EFAULT`.
- **`iconv(cd, NULL, …)` and `iconv(cd, &p, …)` with `p` NULL** -- the reset
  -- were 0 for any descriptor; glibc checks it first (`EBADF`).
- **`iconv` with a NULL count or output pointer** was `EFAULT` -- right, as the
  substitute for glibc's fault -- but after the reset's test rather than in
  glibc's order, which reads `*outbuf` first.
- Beside them, the finding of the pass: the conversions themselves were not
  glibc's -- `B-D-ICONV-WAS-NOT-GLIBCS` (new, fixed with it).

**Thirty-second pass, 2026-09-26 — `linux_io_uring.rs` (3 sites), lane D.**
Against Linux 6.6's io_uring/io_uring.c and io_uring/sqpoll.c.

- **`io_uring_setup(n, NULL)`** was `EFAULT` -- right, first, as
  `copy_from_user` is -- and a block in the kernel half now is too.
- **`io_uring_enter(fd, …, sig, sigsz)`** judged `sig` and `sigsz` before
  the descriptor; Linux reads them only from a ring it has found, and there
  is none, so a bad descriptor is `EBADF` whatever they are.
- **`io_uring_register(fd, op, NULL, n)`** refused a NULL argument per
  operation before the descriptor; the same holds -- the per-operation
  checks come after the ring.
- Beside them, the finding of the pass: the three calls validated in an
  order of their own -- `B-D-IO-URING-WAS-NOT-LINUXS` (new, fixed with it).

**Thirty-third pass, 2026-09-26 — `sysv_shm.rs` (2 sites), lane D.** Against
Linux 6.6's ipc/shm.c.

- **`shmctl(id, IPC_SET, NULL)`** looked the segment up first, so a bad id
  was `EINVAL`; `ksys_shmctl` copies the buffer before anything
  (`copy_shmid_from_user`), so it is `EFAULT` whatever the id.
- **`shmctl(id, IPC_STAT, NULL)`** was `EFAULT` before the lookup; Linux
  looks the segment up and checks the permission first, and faults writing.
- Beside them, the finding of the pass: the segments were a four-slot pool
  of 64 KiB -- `B-D-SYSV-SHM-WAS-A-STATIC-POOL` (new, fixed with it).

**Thirty-fourth pass, 2026-09-26 — `sys_quota.rs` (2 sites), lane D.** Against
Linux 6.6's fs/quota/quota.c.

- **`quotactl(cmd, NULL, …)`** was `EFAULT`, the doc comment citing a
  `getname` of the NULL name; Linux tests `special` for NULL itself and
  answers `ENODEV` -- or, for `Q_SYNC`, syncs every filesystem with quotas
  and returns 0.
- **`quotactl(cmd, special, …, NULL)`** was `EFAULT` before the device was
  looked at; Linux reaches `addr` only inside a filesystem's quota
  operations, and no filesystem here has any.
- Beside them, the finding of the pass: the whole order was its own --
  `B-D-QUOTACTL-WAS-NOT-LINUXS` (new, fixed with it).

**Thirty-fifth pass, 2026-09-26 — `sys_timex.rs` (2 sites), lane D.** Against
Linux 6.6's kernel/time/timekeeping.c and ntp.c, and glibc 2.39's
adjtime.c.

- **`adjtimex(NULL)`, `clock_adjtime(id, NULL)`** were `EFAULT`, first --
  right, as `copy_from_user` is -- and a block in the kernel half now is too.
- Beside them, the finding of the pass: what came after the copy was not
  Linux's, and `adjtime` did not exist -- `B-D-ADJTIMEX-WAS-NOT-LINUXS` (new,
  fixed with it).

**Thirty-sixth pass, 2026-09-26 — `linux_landlock.rs` (3 sites), lane D.**
Against Linux 6.6's security/landlock/syscalls.c and glibc 2.39, which has
no Landlock functions.

- **`landlock_create_ruleset(NULL, …)`, `landlock_add_rule(…, NULL, 0)`,
  `landlock_restrict_self`'s gate** -- the three sites were validators for a
  call that cannot succeed here: the kernel has no Landlock. They went with
  the calls themselves.
- Beside them, the finding of the pass: the version probe said ABI 1 and
  every ruleset was then refused -- `B-D-LANDLOCK-SAID-YES-THEN-NO` (new,
  fixed with it).

**Thirty-seventh pass, 2026-09-26 — `fts.rs` and `ftw.rs` (2 sites each),
lane D.** Against glibc 2.39's io/fts.c and io/ftw.c, on which both files
were rebuilt on 2026-09-25 and 26 -- but their NULLs had not been put to
them.

- **`fts_open(NULL, …)`** was `EFAULT` before the options were looked at;
  glibc judges the options, allocates the stream, and faults only when it
  reads `argv`. A bad option beside a NULL list is `EINVAL` now. The same
  reading found an invented check: `fts_open` refused both and neither of
  `FTS_LOGICAL` and `FTS_PHYSICAL`, because the manual page says one must be
  given. glibc checks neither -- `FTS_LOGICAL` makes the walk logical, and
  without it the walk is physical -- so a program that gave neither worked
  there and failed here. Removed.
- **`fts_set(sp, NULL, …)`** was `EFAULT` before the instruction was
  judged, and a NULL `sp` was `EBADF`; glibc judges the instruction first
  and never reads `sp`. Both are glibc's now, and `fts_children` judges its
  instruction before its stream.
- **`ftw(NULL, …)`, `nftw(NULL, …)`** were right: glibc reads `dir[0]`
  first, after `nftw`'s flags. Their callback is another matter -- it could
  not be NULL at all, and nor could a dozen others; that is the
  thirty-ninth pass.

**Thirty-eighth pass, 2026-09-26 — `xattr.rs` (2 sites), lane D.** Against
Linux 6.6's fs/xattr.c. The fourth pass had put the path before the flags
and the name; this one read what each of those steps does besides.

- **The name** is `strncpy_from_user` into `XATTR_NAME_MAX + 1` bytes, which
  answers more than the NULL: an empty name and one of 256 bytes or more are
  `ERANGE`. The kernel below let the first through and called the second
  `EINVAL`, so both are the libc's now, at the name's place in the order.
- **A setter's value** is judged after the name -- `E2BIG` over 64 KiB, before
  it is read, then `EFAULT` for a NULL one -- where the kernel said `EINVAL`
  to both.
- **A getter's or lister's buffer** is never tested by Linux; a NULL one
  with a size was `EINVAL`, before the lookup.
- Beside them, the finding of the pass: the edges were the kernel's, not
  Linux's -- `B-D-XATTR-SIZES-AND-BUFFERS-WERE-NOT-LINUXS` (new; the libc's
  half fixed with it, the kernel's requested of lane A).

**Thirty-ninth pass, 2026-09-26 — the C callbacks, lane D.** Not a file at a
count, but a kind of site the count could not see: a function pointer a C
caller may pass as NULL, which the Rust signature typed as a function --
never NULL -- so that a NULL was undefined behaviour before the call began. A
scan of every exported signature found fourteen calls in four files: `ftw`,
`nftw`, `ftw64` and `nftw64`; `tsearch`, `tfind`, `tdelete`, `lfind` and
`lsearch`; `qsort`, `qsort_r` and `bsearch`; `pthread_create` and
`pthread_once`. The exit handlers in `crt.rs` were nullable already, but
answered as glibc does not. Each now takes a NULL and answers it as
design-decisions.md §1115 sets out -- `B-D-C-CALLBACKS-COULD-NOT-BE-NULL`
(new, fixed with it). Beside them, `lfind`, `lsearch` and `bsearch` lost
checks glibc does not make.

**Fortieth pass, 2026-09-26 — `pipe.rs` (1 site), lane D.** Against Linux
6.6's fs/pipe.c. The first of the ten files the sweep counted at one.

- **`pipe(NULL)`, `pipe2(NULL, …)`** were `EFAULT` before the pipe existed.
  `do_pipe2` copies the descriptors out last -- after the flags, the pipe and
  the two descriptors -- so a full descriptor table is `EMFILE` there, not
  `EFAULT`. The check is at the copy now, and the pipe and its descriptors
  are given back.
- Beside it, `O_NOTIFICATION_PIPE` (`O_EXCL`'s bit, Linux 5.8's keyring and
  mount notifications) was outside the flag mask and `EINVAL`; Linux 6.6
  accepts it there and refuses it as the pipe is made, `ENOPKG` from a kernel
  built without watch queues -- which is what this one is.

**Forty-first pass, 2026-09-26 — `poll.rs` (1 site), lane D.** Against
glibc 2.39's `select`, `pselect` and `ppoll` and Linux 6.6's fs/select.c.

- **`poll(NULL, n, …)`** was right: `EFAULT` after the `nfds` check, as
  `do_sys_poll` copies.
- Beside it, the finding of the pass: every timeout but `poll`'s was judged
  in an order of its own, or not at all --
  `B-D-SELECT-AND-PPOLL-TIMEOUTS-WERE-NOT-LINUXS` (new, fixed with it).

**Forty-second pass, 2026-09-26 — `shadow.rs` (1 site), lane D.** Against
glibc 2.39's `getspnam_r`.

- **`getspnam_r(NULL, …)`** was `EFAULT` first. glibc has no nscd path for
  shadow, so nss_files opens `/etc/shadow` and reads its first entry before
  the name is touched -- comparing it with that entry. An unreadable file
  is its own `EACCES` now, and a file with no entries "not found", whatever
  the name; `EFAULT` comes at the first comparison. (`getpwnam_r`'s NULL
  stays first: glibc's nscd client reads the name before anything else.)

**Forty-third pass, 2026-09-26 — `ndbm.rs` (1 site), lane D.** The module is
gone. `<ndbm.h>` is not glibc's or musl's -- on Linux the `dbm_*` calls come
from a library, gdbm's `gdbm_compat` or Berkeley DB -- and no header in this
system's sysroot declares them. The module was validators in front of a
`dbm_open` that always failed with `ENOSYS`, so the only thing it could do to
a C program was shadow a ported `gdbm_compat`'s `dbm_open` with one that
opens nothing: a static link takes whichever definition comes first. Nothing
in the tree used it. As design-decisions.md §1114 took libaio's names out,
this took the module out. (`scripts/check-libc-abi.py`'s `NO_ORACLE` entry
for `Dbm` now names a type that is gone; it is harmless, and `scripts/**` is
unassigned -- A-Q11 -- so it is left for whoever next edits that table.)

**Forty-fourth pass, 2026-09-26 — `linux_perf_event.rs` and `linux_bpf.rs`
(1 site each), lane D.** Neither needed anything at its NULL -- `perf_event_open`
judges its flags, then the attribute; `bpf` reads its attribute only when
there is something to read -- but both were exported under names glibc does
not have, as were `linux_io_uring.rs`'s three. perf, libbpf and liburing make
these calls by number, and liburing (2.2 on) defines `io_uring_setup`,
`io_uring_enter` and `io_uring_register` itself, so ours could only shadow a
ported one at a static link; meanwhile `syscall()` answered all five `ENOSYS`
without a look. Now `syscall()` runs the same Linux 6.6 checks -- the answers
the kernel's own Linux table gives -- and the names are gone, as libaio's
went (§1114).

**Forty-fifth pass, 2026-09-26 — `utmpx.rs` (1 site), lane D.** Against
glibc 2.39's login/utmp_file.c. The site -- `pututxline(NULL)`, `EFAULT` --
sat in a module of stubs: nothing read, `pututxline` reporting success while
writing nothing. So the pass became the database -- `B-D-UTMPX-WAS-A-STUB`
(new, fixed with it) -- and the NULL now falls where glibc first touches the
entry: comparing it with a record read, or writing it, after the file's own
errors. `malloc.rs`'s site needed nothing (its `posix_memalign` was put in
glibc's order earlier on 2026-09-26), nor did `utsname.rs`'s (`uname`'s only
error is its copy-out) or `uio.rs`'s.

**What remains.** The surviving `is_null() -> EFAULT` sites have not been
individually classified. This entry stays open for coverage, not because any
specific remaining site is known wrong. **No dense cluster is left.**
`pthread.rs` looks like the largest concentration in a raw `rg` count (~51
sites), but it is not open work: design-decisions.md §303 already walked it,
fixed its nine ordering bugs, and settled the pointer sites wholesale —
NPTL has no NULL checks at all, so there is no upstream errno to look up and
`EFAULT` is the adopted substitute. Do not re-open it by grep count. The same
goes for `file.rs`, `spawn.rs`, `socket.rs`, `unistd.rs`, `process.rs` and
`epoll.rs`, walked by passes five to ten. What is left is a long tail, and
the eleventh pass showed it cannot be retired by sampling: it needs the
file-at-a-time sweep. On 2026-09-26 the sampling script counted 128 sites in 39
files — about a dozen of them classified by that pass. Passes twelve to
thirty-eight swept `ioctl.rs`, `semaphore.rs`, `time.rs`, `aio.rs`,
`sched.rs`, `mqueue.rs`, `linux_futex.rs`, `resolv.rs`, `statvfs.rs`,
`linux_module.rs`, `sysv_msg.rs`, `sys_sysctl.rs`, `stat.rs`, `sysv_sem.rs`,
`linux_aio_abi.rs`, `linux_seccomp.rs`, `mman.rs`, `resource.rs`, `crypt.rs`,
`iconv.rs`, `linux_io_uring.rs`, `sysv_shm.rs`, `sys_quota.rs`,
`sys_timex.rs`, `linux_landlock.rs`, `fts.rs`, `ftw.rs` and `xattr.rs`. That
finishes every file the sweep counted at four, three and two, the three the
recount of 2026-09-26 added among them: `pwd.rs`, `dirent.rs` and `signal.rs`
needed nothing at their NULLs -- `pwd.rs`'s database did
(`B-D-PWD-KNEW-ONLY-ROOT`). The ten it counted at one are done too, by
the fortieth to forty-fifth passes, and with them every file the count of
2026-09-26 named. The thirty-ninth pass was across files, not at a count:
the callbacks. The thirty-ninth pass was across files,
not at a count: the callbacks.

One item is not a site count: `read`, `write`, `pread` and `pwrite`
(`posix/src/file.rs`) still test a NULL buffer where `access_ok` sits, so a NULL
read at end of file, of an empty non-blocking pipe or of a directory says
`EFAULT` where Linux says 0, `EAGAIN` or `EISDIR`. The fix is the tenth pass's
habit — test at each per-kind copy — and it has to be done arm by arm, because
several arms (eventfd, timerfd, inotify) dereference the buffer themselves
(design-decisions §1107, point 2).

Six habits carry forward, one per pass that produced one. From the
eleventh pass: **port an upstream stub as a stub** — validation in front of its
`ENOSYS` changes the one answer its callers test for. From the tenth pass: **the NULL test belongs where the copy is, not where `access_ok` is**,
because on x86-64 `access_ok` admits NULL. From `socket.rs`:
**do not generalise a rule from one sibling call to the next** — `bind` and
`connect` order `ENOTSOCK` oppositely, and `sendmsg` and `sendto` order
`EBADF` oppositely, in the same file. From `spawn.rs`: **when sibling
functions share a check, port the upstream helper rather than the check.**
From `file.rs`: **a doc comment that argues for an ordering is a defect
marker** — three for three across the last three passes — and **a test that
picks a convenient fd rather than the correct one silently stops testing
anything**, which is how eight `pread`/`pwrite` tests spent their whole lives
never reaching the code they were named for. And across all eight: expect the
*ordering* to be wrong more often than the constant, and do not assume a
sibling's correctness transfers — `ftruncate` was right and `truncate` was
wrong ten lines apart.

**Reproduce.** Not a runtime failure. `rg -A3 'is_null\(\)' posix/src`, filtered
for `EFAULT` in the following lines, enumerates the candidate sites; each has to
be checked against the corresponding glibc translation unit.

**Proper fix.** Walk the list once. For each site decide: does the pointer reach
a syscall (keep `EFAULT`) or does glibc reject it in userspace (use glibc's
errno)? Check the *order* of the validations too, not only the constant — that
is what `ptsname_r` actually got wrong, and an ordering bug changes the answer
for every NULL caller regardless of which errno the check sets. Where a change
is warranted, change the code, change the test, and put the glibc file name and
its actual check in a comment at both: a test that encodes an errno with no
upstream citation is only evidence that the code and the test were written by
the same pass.

**Tooling.** `D:\refsrc\glibc-2.39` (added 2026-08-13; shallow clone of the
`glibc-2.39` tag from the `bminor/glibc` GitHub mirror — `sourceware.org`
answers 429) is the reference for this work, alongside the bash checkout in
`TOOLING-BASH-5.2.37-SOURCE`.

`D:\refsrc\linux-6.6` (added 2026-08-13; shallow clone of the `v6.6` tag from
`gitlab.com/linux-kernel/linux`, sparse-checked-out to
`kernel fs mm ipc include/linux include/uapi`, ~130 MB) is the **other half**
of this audit and was missing until the pthread pass needed it. §300's rule has
a kernel side — "does the pointer reach a syscall, and what does the kernel do
with it" — which cannot be answered from glibc, because for a thin wrapper like
`pthread_getaffinity_np` glibc is nothing but `INTERNAL_SYSCALL_CALL` and every
validation lives in `kernel/sched/core.c`. Note for whoever clones it next:
`git checkout` fails on Windows with `invalid path …/aux.c` — `AUX` is a
reserved DOS device name and git validates *every* index entry, not just the
ones inside the sparse cone, so narrowing the cone does not help. Use
`git -c core.protectNTFS=false checkout v6.6`.

Do **not** do this audit from man pages:
`ptsname_r`'s documented `EINVAL` has not matched any glibc implementation
since the TIOCGPTN fast path landed, and trusting the man page is exactly how
the first attempt at this fix went wrong.
