### B-WHOAMI-AND-LOGNAME-TRUST-THE-ENVIRONMENT -- FIXED 2026-08-24 (`f3ba2a369` whoami, `9e2e77b69` logname), security-relevant (lane B, 2026-08-23)

**Resolution (recorded 2026-10-01; the heading said OPEN for five weeks after
the fix).** Both were rewritten the day after this entry, as it proposes.
`whoami` is `geteuid()` then the password database, fails with `cannot find
name for user ID N` rather than printing a number, and never reads the
environment; `logname` is `getlogin()`. Both now take `--help`/`--version`,
refuse an extra operand, write the name as bytes and report a failed write.
Each file's module docs list the defects it replaced. The entry is kept below
as it was filed.

**What.** `whoami` and `logname` both answer from environment variables:

    userspace/coreutils/src/bin/whoami.rs:31   env::var("USER") then env::var("LOGNAME")
    userspace/coreutils/src/bin/logname.rs:11  env::var("LOGNAME") then env::var("USER")

Neither GNU utility consults the environment at all. Measured against GNU
coreutils 9.4:

    $ USER=root LOGNAME=root whoami        inhahe          (status 0)
    $ LOGNAME=root USER=root logname       logname: no login name  (status 1)

`whoami` is `geteuid()` + `getpwuid()`; `logname` is `getlogin()`, the utmp
login name, and POSIX explicitly specifies it that way rather than as `$LOGNAME`
precisely so that it cannot be set by the caller.

**Why it matters.** `whoami` is a *privilege check* in idiomatic shell:

    [ "$(whoami)" = root ] || { echo "must be root"; exit 1; }

Ours returns whatever the caller put in `$USER`, so any unprivileged process can
walk through that gate by exporting one variable. The environment is attacker-
controlled data on every system; an identity utility is the one place it must
not be consulted. This is the same class of defect as `id` printing the wrong
account (`TD-B-ID-AND-STAT-STILL-CLAIM-ACCOUNT-NAME-LOOKUP-IS-UNBUILT`), but
worse, because `id`'s failure was a wrong answer and this one is a wrong answer
the caller chooses.

**Found by.** Rewriting `id` against GNU's `src/id.c`. `id` now resolves names
through `pwdb::Db`; reading its two closest siblings showed both still guessing.

**Also wrong in both, found at the same time.** Neither accepts or rejects
operands: GNU reports `whoami: extra operand 'x'` with a `Try 'whoami --help'`
referral and exits 1, ours ignores the operand and prints a name. Neither has
`--help` or `--version`.

**Proper fix.** `whoami`: `geteuid()`, then `pwdb::Db::load().user_by_uid()`,
and GNU's exact failure when the database has no entry --
`whoami: cannot find name for user ID %ju`, status 1. Note this is *not*
`uid_to_name`'s digits fallback: `whoami` fails rather than printing a number,
because a number is not a name and a script comparing against one would be
misled a second time. `logname`: `getlogin()` -- which needs a utmp equivalent,
so check whether `posix` provides one before assuming; if it does not, the
honest port fails with `logname: no login name` unconditionally, which is what
GNU does on this machine anyway. Both need the getopt conversion for
`--help`/`--version`/`extra operand`.

**Not in the argv-utf8 backlog** -- both take no arguments today, so neither
will be opened by that sweep. This entry is the only thing that will surface
them.
