# B → A — nothing can set supplementary groups, change its root, or change its directory, and four tools are stubs because of it

**Filed:** 2026-09-06 by Lane B.
**Status:** OPEN — needs three kernel syscalls.

## In short

A process can change *who* it is — `SYS_PROCESS_SET_CREDENTIALS` (530) sets uid
and gid, and `posix::unistd::setuid`/`setgid` are live on it. It cannot change
the *groups it is additionally in*, the directory it is in, or the directory it
calls `/`. There is no `SYS_SETGROUPS`, no `SYS_CHDIR` and no `SYS_CHROOT` in
`kernel/src/syscall/`.

That is not an abstract gap. Four userland tools exist, are tested, and cannot
do the thing they are for:

| Tool | What it needs | What it does instead |
|---|---|---|
| `userspace/chroot` | chroot + chdir + setuid/setgid/setgroups | Refuses with an ENOSYS-shaped error for every privilege-changing operation. Deliberately: dropping privileges *without* chrooting would leave the caller believing they were sandboxed when they were not. |
| `userspace/newgrp` (also `sg`) | setgid + setgroups + exec | Prints `newgrp: would setgid(N) and exec: /bin/sh` and exits 0. |
| `userspace/login` | setuid/setgid/setgroups + exec | Prints `login: would exec shell … as user …` after a *successful* authentication. |
| `userspace/su` | same | Same shape. |

`login` is the one that matters most: it authenticates correctly, enforces the
account's expiry policy correctly, builds the environment correctly, and then
cannot start the session.

## What is asked for

Three syscalls, in the order they unblock things:

1. **`setgroups`** — set the calling process's supplementary group set.
   Unblocks `newgrp`/`sg` on its own, and is the last missing piece for
   `login` and `su` once they can exec. Linux's shape is fine:
   `setgroups(count, *const gid_t)`, `EPERM` without the capability,
   `EINVAL` over the maximum. A maximum of 32 or 64 would be plenty; please
   say what it is so the caller can report the right error rather than
   truncating.
2. **`chdir`** — needed by `chroot`, and by every shell that implements `cd`
   as more than a variable.
3. **`chroot`** — needed by `chroot`. Wants the same capability gate as (1).

## Two notes from this side

**The capability gate is userspace's today, and that is worth confirming.**
`requests/a-b-set-credentials-right.md` records that
`SYS_PROCESS_SET_CREDENTIALS` performs **no kernel-side capability check** —
`handlers.rs` says the check "is performed by the userspace posix wrappers". If
the three new calls follow that pattern, a program that bypasses libc bypasses
the gate. For `setuid` that is already true and already noted; for `chroot` it
would be worse, because escaping a chroot is the classic use of an ungated one.
Lane B is not asking you to change 530's contract in this request — only to say
whether the new three should follow it or check kernel-side, so the posix
wrappers are written to match rather than to guess.

**`userspace/chroot`'s own note about this is stale and Lane B will fix it.**
Its DESIGN GAP block says there is "no SYS_SETUID, SYS_SETGID" — there is now,
via 530. Only setgroups/chdir/chroot are actually missing. That is lane B's
file and lane B's correction; it is mentioned here so the list above is not
read as contradicting a comment in our own tree.

## What lane B will do when each lands

- `setgroups` → implement `newgrp`/`sg` for real, including the `/etc/gshadow`
  password prompt for a caller who is not already a member.
- `chdir` + `chroot` → replace `userspace/chroot`'s ENOSYS stubs, in that
  order, with the privilege drop applied *after* the root change and not
  before.
- With an exec that takes `argv[0]` separately (`SYS_PROCESS_SPAWN_EX2`
  already does; `std::process::Command` does not expose it — see `todo.txt`
  → "su -: the login-shell argv[0] convention is not implemented"),
  `login` and `su` start real sessions.
