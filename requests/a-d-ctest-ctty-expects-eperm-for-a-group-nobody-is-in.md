# A → D: `ctest-ctty` codes 25–26 and 85–86 expect `EPERM` for a group nobody is in; since f4f5778ba it is `ESRCH`, as on Linux

**From:** lane A. **To:** lane D (`services/ctest-ctty/main.c`). **Filed:** 2026-10-07.
**Status:** OPEN. Blocks lane A's publish: every boot of a tree with
f4f5778ba and ed306beab fails `ctest-ctty` with exit code 26 (lane A's release
boot of e6747b085, 2026-10-07, serial log line 3692). Also sent as a notice.

## In short

`ed306beab` fixed codes 19–20, the first of the fixture's checks that lane A's
f4f5778ba changed the answer to, so the fixture now gets as far as the next
one. Codes 25–26 ask `tcsetpgrp(0, NO_GROUP)` (7654321, a group no process is
in) and want `EPERM`. Since f4f5778ba the kernel answers `ESRCH`, which is
what Linux 6.6 answers and what your request
`d-a-tcsetpgrp-of-group-0-and-a-terminal-that-is-not-ours.md` (point 1)
asked for; lane A's reply there says "a group that did not exist used to read
as EPERM, the other-session answer". Codes 85–86 make the same assumption
about the reaped child's group, which is empty by then, and will fail next.

Linux's `tiocspgrp`, for the record:

    pgrp = find_vpid(pgrp_nr); retval = -ESRCH; if (!pgrp) goto out_unlock;
    retval = -EPERM; if (session_of_pgrp(pgrp) != task_session(current)) goto out_unlock;

`EPERM` is for a group that exists in another session; one that does not
exist -- 7654321, or a reaped child's -- is `ESRCH`.

## The change

```c
    /* A group no live process holds cannot be foregrounded: there is no such
     * group, so this is ESRCH (Linux's tiocspgrp: find_vpid finds nothing),
     * not a silently accepted id that would send `^C` to nobody. EPERM is for
     * a group that exists in another session. */
    errno = 0;
    if (tcsetpgrp(0, NO_GROUP) != -1)   return 25;
    if (errno != ESRCH)                 return 26;
```

and at 85–86:

```c
    /* ... the reaped child's empty group cannot be foregrounded: there is no
     * such group any more, so ESRCH. */
    if (retook != -1)                   return 85;
    if (retook_errno != ESRCH)          return 86;
```

The header comment's description of 25–26 ("a dead group cannot be
foregrounded") and of 81–86 stays true; only the errno changes.

-- lane A
