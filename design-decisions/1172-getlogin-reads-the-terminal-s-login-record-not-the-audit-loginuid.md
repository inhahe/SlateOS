## 1172. `getlogin` reads the terminal's login record in `utmp`, not the audit `loginuid` glibc reads first

**Date:** 2026-10-05
**Decided by:** Claude (autonomous)
**Lane:** D

**In short:** `getlogin` answers "who logged in on this terminal?", and
`logname` prints its answer. On Linux, glibc asks the audit subsystem first
(`/proc/self/loginuid`, the user the login session was opened for) and looks
in the login records (`/var/run/utmp`) only if that file cannot be read.
SlateOS's kernel keeps no audit login sessions, so its `loginuid` says
"unset" for every process -- and glibc treats "unset" as "no login name",
without looking at the records. Copied exactly, `getlogin` would answer
nothing for every process here, a logged-in shell included. So the C library
takes glibc's other path: the terminal on standard input, and that
terminal's login record.

| Option | What a program sees |
|---|---|
| **A. The login record only** (chosen) | the name `login` recorded for the terminal; no name for a process outside a login session |
| B. glibc's order exactly: `loginuid`, then the record | no login name, ever, for any process: SlateOS's `loginuid` is always "unset" |
| C. Keep the constant `root` | `root` for everyone, after `su`, and on a second account; `logname`'s "no login name" unreachable |

**Why A.** It is glibc's own code -- `getlogin_r_fd0`, which glibc runs
where `/proc/self/loginuid` cannot be read, as on a Linux built without
audit -- and its answer is the one `getlogin` exists to give, now that
`login` writes the record (lane B, 2026-10-01) and init starts the file at
boot (lane D, 2026-10-05). B would be glibc to the letter and useless: the
kernel's "unset" means "not tracked" here, not "not logged in"
(`kernel/src/fs/procfs.rs` says why it reports it). C is what this replaces:
true while one account existed, and wrong the day a second one, or `su`, is
used -- the case `logname` is for.

**What would change it:** a kernel that records the login session's user
per process -- a `loginuid` that `login` can set and children inherit, as
`pam_loginuid` sets Linux's. Then glibc's order is right again, and this
reads `loginuid` first.

**Where:** `posix/src/pwd.rs` (`getlogin`, `getlogin_r`) and
`posix/src/utmpx.rs` (`login_on_line`); the boot half in
`services/init/src/main.rs` (`start_login_records`).
