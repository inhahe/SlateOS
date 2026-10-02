## TD-POSIX-GETLOGIN-IS-A-CONSTANT (lane B, 2026-08-24) — **open**

**What it is.** `posix::pwd::getlogin` (`posix/src/pwd.rs:514`) returns the
string `root` unconditionally, and `getlogin_r` writes `root` into whatever
buffer it is handed. Neither consults anything. The reason is one level down:
`posix/src/utmpx.rs` is a set of stubs — `getutxent` returns null, `pututxline`
returns its argument without writing, `utmpxname` returns success without
opening a file — over a user-accounting database that nothing maintains.

**Where it bites.** `logname` (`userspace/coreutils/src/bin/logname.rs`) is
`getlogin` and nothing else, so on SlateOS it prints `root` for every session
and its `logname: no login name` branch is unreachable. `who`, `w` and `last`
see an empty database for the same reason. `id` and `whoami` are unaffected —
they read the effective uid, not the accounting database.

**Why it is not wrong yet.** With one account, `root` is the true answer, and
an accounting database with one possible entry would say the same thing more
slowly. It becomes wrong the day a second account exists, and it becomes
*misleading* the day `su` does, because the whole point of `logname` is that
it still names the account that logged in after `su` has changed the uid.

**The proper fix**, which is glibc's: `ttyname(0)` (falling back to fds 1 and
2, as glibc does) to name the controlling terminal, strip a leading `/dev/`,
then scan `/var/run/utmp` for a `USER_PROCESS` entry whose `ut_line` matches
and return its `ut_user`. Return null — not a constant — when there is no
match, since that is the case `logname`'s diagnostic exists for. That needs
`utmpx.rs` to become a real reader of a real file first, and needs
`userspace/login` to write an entry when it opens a session; both are in lane
B. Until then the constant is the honest single-account answer and `logname`'s
module docs say so.

**How it was found.** Porting `logname` off `$LOGNAME` in the
closed-descriptor sweep (todo item 9). The port is complete and correct — it
calls `getlogin` and reports its failure exactly as GNU does — so nothing in
`logname` needs to change when this is fixed.

**Addendum (lane D, 2026-09-26): the database is real; `getlogin` waits on
its writers.** `utmpx.rs` reads and writes `/var/run/utmp` as glibc does now
(`B-D-UTMPX-WAS-A-STUB`). Both halves this entry names are in `posix/`, which
is lane D's since the six-lane split, not lane B's. `getlogin` stays the
single-account constant for now on purpose: glibc's answer -- the `ut_user` of
the terminal's login record -- would be "no login name" for every session
until something writes one, which is lane B's `login` (asked in
`requests/d-b-utmp-is-a-real-file-now-create-it-and-write-logins.md`). When it does, lane D switches `getlogin` over.
