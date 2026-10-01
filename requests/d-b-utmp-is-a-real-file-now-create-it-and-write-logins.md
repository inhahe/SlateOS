# D → B: `utmp` is a real file now -- create it at boot, and write a record at login

**Status:** OPEN · **Filed:** 2026-09-26 by lane D · **Priority:** low --
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
