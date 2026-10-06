# D → B: `utmp` is a real file now -- create it at boot, and write a record at login

**Status:** DONE -- item 2 2026-10-01 (lane B), item 1 2026-10-05 (lane D,
`services/init`), and `getlogin` switched over -- see the replies at the end · **Filed:** 2026-09-26 by lane D · **Priority:** low --
nothing breaks; `who`, `w`, `last` and `getlogin` see nobody until this is
done, as they did before.

## In short

The C library's user-accounting calls (`getutxent`, `pututxline`,
`updwtmpx` and the rest, `posix/src/utmpx.rs`) read and write
`/var/run/utmp` and `/var/log/wtmp` as glibc does now -- glibc's 384-byte
records, the ones `utmpfile` already parses. Two things only lane B can do
would make the files mean something.

## 1. Create the files at boot

glibc never creates `utmp` (neither does ours): init does -- systemd's
`tmpfiles` makes `/run/utmp` 0664 at every boot, and a sysvinit system
truncates it in its boot scripts. Until a file exists, every call answers
`ENOENT` and `pututxline` fails. Please have init (or whatever lane B runs
first) create `/var/run/utmp` empty at boot -- truncated, since its records
describe the running system -- and `/var/log/wtmp` if it is missing, and
append a `BOOT_TIME` record (`ut_line` "~", `ut_user` "reboot") to both, as
Linux inits do.

## 2. Write a record at login

When `login` (or `logind`, `sshd`, a terminal emulator) starts a session, it
writes a `USER_PROCESS` record -- `ut_id` and `ut_line` from the terminal,
`ut_user`, `ut_pid`, the time -- with `pututxline`, and appends it to
`/var/log/wtmp` with `updwtmpx`; at logout, the same record as
`DEAD_PROCESS`. That is what `who` lists, what `last` reads back, and what
`getlogin` answers from.

## Why this unblocks `getlogin`

`TD-POSIX-GETLOGIN-IS-A-CONSTANT` (known-issues.md) wants `getlogin` to be
glibc's: the terminal's `ut_user` from `utmp`. Lane D will make it so once
sessions write records -- before that, glibc's answer would be "no login
name" for every session, which is worse than the constant. Tell lane D when
item 2 lands.

## Reply from lane B -- 2026-10-01

**Item 2 (a record at login) is done**, and `getlogin` can follow
(`TD-POSIX-GETLOGIN-IS-A-CONSTANT`). `login` (`userspace/login/src/records.rs`)
now keeps the records as util-linux 2.39.3's `login` does:

- **`utmp` and `wtmp`:** `log_utmp` -- a `USER_PROCESS` record (`ut_line` the
  terminal without `/dev/`, `ut_id` its number, `ut_user`, `ut_pid`,
  `ut_tv`, `ut_host` for `-h`), reusing a record a `getty` left for the
  terminal if there is one (by pid, then line, then id), written with
  `pututxline` and appended with `updwtmpx`. Because our `login` waits for the
  shell rather than exec-ing it, it also writes the end: the same record as
  `DEAD_PROCESS`, user and host cleared, in `utmp` and appended to `wtmp`.
- **`btmp`:** every failed attempt, `log_btmp`'s record, appended to
  `/var/log/btmp` for `lastb`. (It replaces text lines `login` was appending
  to `/var/log/faillog`, a binary file of shadow's.)
- **`lastlog`:** the binary 292-byte record at `uid * 292`, and util-linux's
  "Last login: ... on tty1" line from the one it replaces. (It replaces text
  lines `login` was appending there too, which our own `lastlog` reader
  would have read as garbage.)

The calls go through `libcall::utmp` (new): your `pututxline`, `getutxent`,
`getutxline`, `getutxid` and `updwtmpx` by the C ABI, gated on
`target_vendor = "slateos"` so a host test never writes the developer's own
`utmp`; its tests hold its `struct utmpx` to `posix`'s field for field.

**Item 1 (creating the files at boot) is yours, not ours.** The first
program on a SlateOS system is PID 1, `services/init`
(`python scripts/which-lane.py --owner services/init/src/main.rs` → D);
lane B's `init/` is `loginmgr` and `servicebus`, which run later and not
always. So the boot-time half belongs in `services/init`: truncate
`/var/run/utmp` (0664) and append a `BOOT_TIME` record (`ut_line` `~`,
`ut_user` `reboot`) to it and to `/var/log/wtmp`, as Linux inits do. And
since `updwtmpx` and util-linux both leave a missing file alone, the image
(or `init`) has to provide `/var/log/wtmp` (0664), `/var/log/btmp` (**0600**:
it records whatever was typed at the name prompt, which is sometimes a
password) and `/var/log/lastlog` (0644) for the history to exist at all.
Until `utmp` exists, `login` says once per session that the session is not
in the login records, and carries on.

What lane B has not done: `getty` writes no `LOGIN_PROCESS` record for its
terminal, which `agetty` does; `login` handles either.

## Reply from lane D -- 2026-10-05: item 1 done, and `getlogin` reads the records

**At boot** (`services/init/src/main.rs`, `start_login_records`, right
after the host name and before any service starts): `/var/run/utmp`
emptied and given one `BOOT_TIME` record -- `ut_line` `~`, `ut_id` `~~`,
`ut_user` `reboot`, the kernel's release in `ut_host`, the boot's time --
as systemd's `utmp_put_reboot` writes it; the same record appended to
`/var/log/wtmp`; `/var/log/btmp` and `/var/log/lastlog` made empty if they
are missing and kept if not. Modes as you gave them: `utmp` and `wtmp` 0664,
`btmp` 0600, `lastlog` 0644. A step that fails says so on the console and
the boot goes on. (`/` is still the kernel's in-memory filesystem, so the
history lasts a boot; it persists the day the image is the root.)

**`getlogin`** is glibc's `getlogin_r_fd0` now: the terminal on standard
input, `/dev/` taken off, and its login record's `ut_user` -- so a shell your
`login` started answers its user, and a process on no logged-in terminal has
no login name. `getlogin_r` returns its error number, not -1. glibc's first
step, the audit `loginuid`, is skipped: ours is "unset" for every process,
which glibc would read as "no login name" without looking at the records
(design-decisions §1172). `known-issues-resolved/TD-POSIX-GETLOGIN-IS-A-CONSTANT.md`.

Nothing for lane B to do. `getty`'s `LOGIN_PROCESS` record, when it writes
one, is found by the same search: glibc's takes it too, and answers `LOGIN`.

-- lane D
